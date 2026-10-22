//! The ROM-ID index (US-RR-34-7, C2, MF-40, EC-03): which definition of a
//! [`DefinitionSet`] an image is, found by its ROM ID bytes, refused when
//! more than one is.
//!
//! # Legacy
//!
//! Legacy RomRaider re-read every definition file on every ROM open
//! (MF-40), in the user's order, and took the first `<rom>` of the first
//! file whose `<romid>` matched the image
//! (`editor/ecu/OpenImageWorker.java:219-250`; within a file, the first
//! match, `xml/DOMRomUnmarshaller.java:118-124`). Two definitions matching
//! one image (two files defining the same ROM, or one ID a prefix of
//! another at the same address) were never noticed: the image silently
//! opened through whichever came first, possibly the wrong one (EC-03).
//!
//! # Here
//!
//! A [`RomIdIndex`] is built once per version of a set ([`RomIdIndex::new`])
//! and answers every open after that without touching a definition file.
//! [`RomIdIndex::lookup`] tries every indexed definition and returns a
//! [`RomMatch`]: [`Unique`](RomMatch::Unique),
//! [`Ambiguous`](RomMatch::Ambiguous) with every candidate, or
//! [`None`](RomMatch::None). [`RomIdIndex::identify`] turns an ambiguous
//! match into an [`AMBIGUOUS_ROM_ID`](codes::AMBIGUOUS_ROM_ID) Error naming
//! each candidate and where it is defined, and applies no definition.
//!
//! What is indexed is each `<rom>` the set can resolve
//! ([`DefinitionSet::roms`]) that has an [`Identification`]: its own first
//! `<romid>`, never an inherited one, as legacy matched it. A `<rom>`
//! without one is left out and can never be a match: a definition that
//! only serves as a base (`32BITBASE`, no `internalidstring`), or one whose
//! ID is incomplete or invalid (`DEF0012`, `DEF0013`, already reported when
//! the file was read). A `<rom>` ignored as a repeat of an earlier one of
//! its file ([`DUPLICATE_XMLID`](codes::DUPLICATE_XMLID)) is left out too.
//! Two `<rom>`s of one `xmlid` in two files are both indexed: the set keeps
//! both, and they may differ (RomRaider's standard and metric files define
//! the same ROMs with different units), so an image both match is
//! ambiguous, where legacy took the first file.
//!
//! A [`Force`](Identification::Force) definition matches every image, as
//! in legacy; it is therefore ambiguous with any other definition the image
//! matches, and the unique match only of an image nothing else matches.
//!
//! # Matching
//!
//! A match is exactly [`Identification::matches`]: an ID that does not lie
//! wholly inside the image (an image shorter than its address plus its
//! length) does not match, and lookup never panics, whatever the image.
//! The index does not compare each definition's ID with the image one by
//! one: the IDs are grouped by (address, mask), so one lookup reads the
//! image once per group, masks the window and finds every definition with
//! that ID in one ordered-map lookup.
//!
//! The index is plain, immutable data: no cache, cell or lock (this module
//! is under `cargo xtask check-globals`' stateless rule), `Send + Sync`,
//! with no borrow of the set, so one index serves every open of every
//! thread. The [`RomRef`]s it hands out refer to the set it was built
//! from; a set changed is a new version, with a new index.
//!
//! # Example
//!
//! ```
//! use rr_defs::resolve::{RomIdIndex, RomMatch};
//! use rr_defs::romraider::DefinitionSet;
//!
//! let xml = br#"<roms>
//!   <rom><romid><xmlid>BASE</xmlid></romid></rom>
//!   <rom base="BASE">
//!     <romid><xmlid>ROM A</xmlid><internalidaddress>4</internalidaddress>
//!       <internalidstring>ROMA</internalidstring></romid>
//!   </rom>
//! </roms>"#;
//! let (set, _) = DefinitionSet::read_all([("defs.xml", &xml[..])]).into_result().unwrap();
//! let index = RomIdIndex::new(&set);
//!
//! let image = b"....romA....";
//! let RomMatch::Unique(rom) = index.lookup(image) else { panic!() };
//! assert_eq!(rom.xmlid(), "ROM A");
//! assert_eq!(rom.rom(), set.find("rom a").unwrap());
//!
//! assert!(matches!(index.lookup(b"....ROMB...."), RomMatch::None));
//! // Too short to hold the ID: no match, never a panic.
//! assert!(matches!(index.lookup(b"....ROM"), RomMatch::None));
//! ```

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fmt::Write as _;

use rr_core::diag::{Diagnostic, Loaded, Provenance, Span, codes};

use crate::romraider::{DefinitionSet, Identification, RomRef};

/// The ROM-ID index of one version of a [`DefinitionSet`]. See the
/// [module docs](self).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RomIdIndex {
    /// Every indexed definition, in set order; the index of one here is
    /// its number in `force` and `groups`.
    candidates: Vec<Candidate>,
    /// The [`Identification::Force`] candidates, ascending.
    force: Vec<usize>,
    /// The byte-pattern candidates, grouped by (address, mask), in that
    /// order.
    groups: Vec<Group>,
}

/// The candidates whose ID is at one address under one mask.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Group {
    /// Where the ID starts in the image.
    offset: usize,
    /// The bits checked in each byte; its length is the ID's.
    mask: Box<[u8]>,
    /// The candidates of each (pre-masked) ID value, ascending.
    by_value: ByValue,
}

/// Candidate numbers by pre-masked ID value.
type ByValue = BTreeMap<Box<[u8]>, Vec<usize>>;

/// One definition of a [`RomIdIndex`]: which `<rom>`, its name, where it
/// is defined and how an image is recognised as it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    rom: RomRef,
    xmlid: String,
    at: Provenance,
    identification: Identification,
}

impl Candidate {
    /// The `<rom>` in the set the index was built from, to resolve
    /// ([`DefinitionResolver::resolve`](super::DefinitionResolver::resolve)).
    #[must_use]
    pub const fn rom(&self) -> RomRef {
        self.rom
    }

    /// The `<rom>`'s own `<xmlid>`, as written.
    #[must_use]
    pub fn xmlid(&self) -> &str {
        &self.xmlid
    }

    /// Where the `<rom>` element is: its file and position, with its layer
    /// id ([`DefinitionSet::id`]: `KEY@file` tells a later file's copy of a
    /// name from the first).
    #[must_use]
    pub const fn at(&self) -> &Provenance {
        &self.at
    }

    /// How an image is recognised as the ROM.
    #[must_use]
    pub const fn identification(&self) -> &Identification {
        &self.identification
    }

    /// `` `xmlid` (file:line:col, ID at 0x2000) ``, for a message.
    fn describe(&self, out: &mut String) {
        // Writing to a `String` cannot fail.
        let _ = write!(out, "`{}` ({}, ", self.xmlid, self.at);
        let _ = match &self.identification {
            Identification::Force => write!(out, "internalidstring force: matches every image)"),
            Identification::Bytes(p) => write!(out, "ID at {:#X})", p.offset()),
        };
    }
}

/// What a [`RomIdIndex::lookup`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RomMatch<'a> {
    /// No indexed definition matches the image.
    None,
    /// Exactly one does: the image's definition.
    Unique(&'a Candidate),
    /// More than one does: every one, in set order. None of them may be
    /// applied (EC-03).
    Ambiguous(Vec<&'a Candidate>),
}

impl<'a> RomMatch<'a> {
    /// The definition, for a [`Unique`](Self::Unique) match only.
    #[must_use]
    pub const fn unique(&self) -> Option<&'a Candidate> {
        match self {
            Self::Unique(c) => Some(*c),
            Self::None | Self::Ambiguous(_) => None,
        }
    }
}

impl RomIdIndex {
    /// Indexes every identified `<rom>` of `set` (see the
    /// [module docs](self) for which are left out).
    #[must_use]
    pub fn new(set: &DefinitionSet) -> Self {
        let mut candidates = Vec::new();
        let mut force = Vec::new();
        let mut groups: BTreeMap<(usize, Box<[u8]>), ByValue> = BTreeMap::new();
        for (rom, entry) in set.roms() {
            let Some(identification) = entry.identification() else {
                continue;
            };
            let n = candidates.len();
            match identification {
                Identification::Force => force.push(n),
                Identification::Bytes(p) => groups
                    .entry((p.offset(), p.mask().into()))
                    .or_default()
                    .entry(p.value().into())
                    .or_default()
                    .push(n),
            }
            let mut at = Provenance::new(entry.source(), entry.span());
            if let Some(id) = set.id(rom) {
                at = at.with_layer(id.as_str());
            }
            candidates.push(Candidate {
                rom,
                xmlid: entry.xmlid().to_owned(),
                at,
                identification: identification.clone(),
            });
        }
        let groups = groups
            .into_iter()
            .map(|((offset, mask), by_value)| Group {
                offset,
                mask,
                by_value,
            })
            .collect();
        Self {
            candidates,
            force,
            groups,
        }
    }

    /// Every indexed definition, in set order.
    #[must_use]
    pub fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }

    /// The definitions `image` matches: exactly those whose
    /// [`Identification::matches`] it, in set order. An ID past the end of
    /// the image does not match; never panics.
    #[must_use]
    pub fn lookup(&self, image: &[u8]) -> RomMatch<'_> {
        let mut found = self.matching(image);
        found.sort_unstable();
        let mut hits: Vec<&Candidate> = found
            .iter()
            .filter_map(|&n| self.candidates.get(n))
            .collect();
        match hits.len() {
            0 => RomMatch::None,
            1 => hits.pop().map_or(RomMatch::None, RomMatch::Unique),
            _ => RomMatch::Ambiguous(hits),
        }
    }

    /// The definition of `image`, refused when ambiguous: `Some` for a
    /// unique match, `None` with no diagnostic when nothing matches, and
    /// `None` with an [`AMBIGUOUS_ROM_ID`](codes::AMBIGUOUS_ROM_ID) Error
    /// naming every candidate when more than one does, so no definition is
    /// applied (EC-03). `source` names the image in the diagnostic.
    pub fn identify(&self, image: &[u8], source: &str) -> Loaded<Option<&Candidate>> {
        match self.lookup(image) {
            RomMatch::None => Loaded::clean(None),
            RomMatch::Unique(c) => Loaded::clean(Some(c)),
            RomMatch::Ambiguous(all) => Loaded::new(None, [ambiguous(&all, source)]),
        }
    }

    /// The numbers of the candidates `image` matches, unordered.
    fn matching(&self, image: &[u8]) -> Vec<usize> {
        let mut found = self.force.clone();
        let mut key = Vec::new();
        for g in &self.groups {
            let window = g
                .offset
                .checked_add(g.mask.len())
                .and_then(|end| image.get(g.offset..end));
            let Some(window) = window else { continue };
            key.clear();
            key.extend(window.iter().zip(&*g.mask).map(|(b, m)| b & m));
            if let Some(hits) = g.by_value.get(key.as_slice()) {
                found.extend_from_slice(hits);
            }
        }
        found
    }
}

/// The [`AMBIGUOUS_ROM_ID`](codes::AMBIGUOUS_ROM_ID) Error for the image
/// `source` matching every one of `all` (at least two), at the lowest ID
/// address among them.
fn ambiguous(all: &[&Candidate], source: &str) -> Diagnostic {
    let address = all
        .iter()
        .filter_map(|c| match &c.identification {
            Identification::Bytes(p) => u64::try_from(p.offset()).ok(),
            Identification::Force => None,
        })
        .min()
        .unwrap_or(0);
    let mut message = format!(
        "the image's ROM ID matches {} definitions, so none is applied: ",
        all.len()
    );
    for (i, c) in all.iter().enumerate() {
        if i > 0 {
            message.push_str("; ");
        }
        c.describe(&mut message);
    }
    Diagnostic::error(
        codes::AMBIGUOUS_ROM_ID,
        message,
        Provenance::new(source, Span::byte_offset(address)),
    )
}
