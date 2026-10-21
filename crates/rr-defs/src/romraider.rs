//! The RomRaider ECU-definition reader (US-RR-29): existing RomRaider
//! definition XML files, loaded unchanged, into layered documents (C1).
//!
//! A definition file is a `<roms>` element holding `<rom>` elements. Each
//! `<rom>` describes one ROM: its `<romid>` (what the ROM is and how an image
//! is recognised as it), and its `<table>`s. A `<rom base="X">` extends the
//! `<rom>` whose `xmlid` is `X`, changing only what it says, so each `<rom>`
//! is one partial document ([`RomDoc`]) with a [`Layer`](rr_core::layer::Layer) identity
//! ([`RomEntry::layer`]) and a `base` reference ([`RomEntry::base`]),
//! which a [`DefinitionSet`] stacks and resolves into a [`ResolvedRom`].
//!
//! [`read`] reads one file's bytes through the hardened XML reader
//! ([`crate::xml`]), then the tree; [`read_document`] reads an already-parsed
//! tree. Both return every diagnostic, with the file's `line:col`; an Error
//! withholds the result ([`Loaded`]).
//!
//! # What is read
//!
//! | Element | Read into | By |
//! |---------|-----------|----|
//! | `<roms>` (the root) | [`DefinitionFile`] | here |
//! | `<rom base="...">` | [`RomEntry`]: `xmlid`, [`BaseRef`], [`RomDoc`] | here |
//! | `<romid>` | [`RomIdDoc`], [`Identification`] | here |
//! | `<table>` (in `<rom>`), its axes, `<state>`s and `<bit>`s | [`TableDoc`] in [`RomDoc::tables`] | here (see the [`TableDoc`] docs) |
//! | `<scalingbase>` (in `<roms>`) | [`ScalingDoc`] in [`DefinitionFile::scaling_bases`] | here (see the [`ScalingDoc`] docs) |
//! | `<scaling>` (in a `<table>` or axis) | [`ScalingDoc`] in [`CellsDoc::scalings`] | here |
//! | `<checksum>`, `<Simulation>` (in `<rom>`) | not supported yet: `DEF0006` warning | — |
//!
//! Values are typed as they are read (storage types, byte orders, masks,
//! formulas, display formats, numbers); a table's [`Encoding`](rr_core::encoding::Encoding)
//! and [`Conversion`](rr_core::conversion::Conversion) are built from its
//! merged layers by [`build_cells`].
//!
//! # Resolving
//!
//! A [`DefinitionSet`] loads many files together ([`DefinitionSet::read_all`])
//! and indexes their `<rom>`s by `xmlid`; its [`RomResolver`] stacks each
//! `<rom>` on its `base` chain with rr-core's resolver (C1), across files,
//! and resolves it into a [`ResolvedRom`]: tables merged by name, `omit`
//! applied, table and scaling `base`s resolved, and each table's cells
//! typed ([`ResolvedTable`]), with the chain kept for
//! [`explain`](rr_core::layer::Lineage::explain).
//!
//! # Legacy semantics kept
//!
//! The reader follows legacy RomRaider's loader
//! (`legacy/src/main/java/com/romraider/xml/DOMRomUnmarshaller.java`,
//! `maps/RomID.java`) so that every definition that loaded there loads
//! here, to the same values:
//!
//! * Element names compare ignoring ASCII case (`<ROMID>` is `<romid>`), as
//!   legacy's `equalsIgnoreCase` did; attribute names are case-sensitive, as
//!   legacy's `getNamedItem` was.
//! * Text values are kept exactly as written, untrimmed: an
//!   `internalidstring` of `A2WA20  ` (as in the pinned corpus) matches the
//!   two spaces in the image. Numbers (addresses, sizes) may have
//!   whitespace around them, which legacy refused.
//! * A repeated element: the later one wins, as in legacy, with a warning
//!   (`DEF0008`). A `<rom>` with two `<romid>`s merges both, and is
//!   identified by the first, as legacy did.
//! * A `<rom>` with a `base` is not obsolete unless it says so: legacy reset
//!   `obsolete` after reading the base.
//! * Tables are known by name ignoring case, as legacy's table map; a
//!   `<table>` naming one the `<rom>` already has, or inherits, changes it
//!   (legacy, `xml/DOMRomUnmarshaller.java:168-182`). Two of one name in
//!   one `<rom>` (126 in the pinned corpus) merge with a warning
//!   (`DEF0008`).
//!
//! # Not silently dropped
//!
//! Legacy skipped anything it did not know without a word. Here every
//! unknown element (`DEF0004`) and attribute (`DEF0005`), stray text
//! (`DEF0007`) and repeated element (`DEF0008`) is a Warning naming it at its
//! `line:col` (MF-23); malformed values (a bad address, `DEF0010`; file size,
//! `DEF0011`; hex ID, `DEF0012`; a table's number, `DEF0017`, or switch
//! data, `DEF0021`; its encoding attributes, `ENC` codes; a scaling's
//! number, `DEF0023`) are Errors where legacy failed the whole file with an
//! exception, and so is a scaling `expression` that does not parse (its
//! `EXP` code; legacy's JEP showed every value as NaN), and a table it
//! could not place (no name, `DEF0015`; an unknown or missing type,
//! `DEF0016`; a missing axis, `DEF0019`). Content legacy ignored where it
//! is (an axis on a table that has none) is a warning (`DEF0020`). A root other than `<roms>` is an Error
//! (`DEF0002`), except a lone `<rom>`, read as a file of one ROM with a
//! warning (`DEF0003`; legacy found no ROM in it).
//!
//! # Example
//!
//! ```
//! use rr_core::layer::Field;
//! use rr_defs::romraider;
//!
//! let xml = br#"<roms>
//!   <rom>
//!     <romid><xmlid>16BITBASE</xmlid><filesize>192kb</filesize></romid>
//!   </rom>
//!   <rom base="16BITBASE">
//!     <romid>
//!       <xmlid>A4SG900C</xmlid>
//!       <internalidaddress>200</internalidaddress>
//!       <internalidstring>A4SG900C</internalidstring>
//!     </romid>
//!   </rom>
//! </roms>"#;
//! let (file, diagnostics) = romraider::read("defs.xml", xml).into_result().unwrap();
//! assert!(diagnostics.is_empty());
//! let file = file.unwrap();
//! let [base, rom] = file.roms() else { panic!() };
//! assert_eq!(base.doc().romid.file_size, Field::Set(192 * 1024));
//! assert_eq!(rom.base().map(|b| b.name()), Some("16BITBASE"));
//! assert_eq!(rom.layer().id().as_str(), "A4SG900C");
//!
//! let mut image = vec![0; 0x300];
//! image[0x200..0x208].copy_from_slice(b"a4sg900c");
//! assert!(rom.identification().unwrap().matches(&image));
//! assert!(base.identification().is_none());
//! ```

mod ctx;
mod rom;
mod romid;
mod scaling;
mod set;
mod table;
mod value;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use rr_core::diag::{Loaded, Span, codes};
use rr_core::layer::Keyed;

use crate::xml::{self, Document, Element, Node};

pub use rom::{BaseRef, RomDoc, RomEntry};
pub use romid::{IdAddress, Identification, RamOffset, RomIdDoc};
pub use scaling::{
    CellFormat, DEFAULT_CATEGORY, Formula, ScalingDoc, build_cells, scaling_base_key, scaling_key,
    select_scaling, to_raw_mismatch,
};
pub use set::{
    DefinitionSet, ResolvedAxis, ResolvedRom, ResolvedTable, RomRef, RomResolver, table_path,
    xmlid_key,
};
pub use table::{AxisDoc, BitFlag, CellsDoc, SwitchState, TableDoc, TableKind, table_key};

use ctx::{Ctx, is};

/// One definition file: its `<rom>`s, in document order, and its
/// `<scalingbase>`s.
#[derive(Debug, Clone, PartialEq)]
pub struct DefinitionFile {
    source: Arc<str>,
    roms: Vec<RomEntry>,
    scaling_bases: Keyed<String, ScalingDoc>,
    /// Where each `<scalingbase>` key was first given.
    scaling_base_spans: Vec<(String, Span)>,
}

impl DefinitionFile {
    /// The source name the file was read as.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The `<rom>`s that have a name, in document order.
    #[must_use]
    pub fn roms(&self) -> &[RomEntry] {
        &self.roms
    }

    /// The `<scalingbase>`s, keyed by [`scaling_base_key`] (the name
    /// lower-cased; legacy filed an unnamed one under its `units`), in
    /// document order: the scalings a `<scaling base="...">` of any `<rom>`
    /// of the file starts from (legacy, `xml/TableScaleUnmarshaller.java:64-75`).
    #[must_use]
    pub const fn scaling_bases(&self) -> &Keyed<String, ScalingDoc> {
        &self.scaling_bases
    }

    /// Where the `<scalingbase>` of key `key` ([`scaling_base_key`]) is (the
    /// first, when one name is defined twice). `None` if there is none.
    #[must_use]
    pub fn scaling_base_span(&self, key: &str) -> Option<Span> {
        self.scaling_base_spans
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, span)| *span)
    }

    /// The `<rom>`s, by value.
    #[must_use]
    pub fn into_roms(self) -> Vec<RomEntry> {
        self.roms
    }
}

/// Reads one RomRaider ECU-definition file. `source` names it in
/// diagnostics.
///
/// The value is `None` when the bytes are not a well-formed XML file (see
/// [`xml::parse`]) or not a RomRaider ECU definition (`DEF0002`).
pub fn read(source: &str, input: &[u8]) -> Loaded<Option<DefinitionFile>> {
    let parsed = xml::parse(source, input);
    if parsed.has_errors() {
        return Loaded::new(None, parsed.into_diagnostics());
    }
    let (document, mut diagnostics) = parsed.accept_errors();
    let Some(document) = document else {
        return Loaded::new(None, diagnostics);
    };
    let (file, more) = read_document(source, &document).accept_errors();
    diagnostics.extend(more);
    Loaded::new(file, diagnostics)
}

/// Reads a parsed XML document as a RomRaider ECU-definition file; see
/// [`read`].
pub fn read_document(source: &str, document: &Document) -> Loaded<Option<DefinitionFile>> {
    let mut ctx = Ctx::new(source);
    let file = read_root(&mut ctx, document.root());
    ctx.diags.finish(file)
}

/// Reads the root element.
fn read_root(ctx: &mut Ctx<'_>, root: &Element) -> Option<DefinitionFile> {
    let source: Arc<str> = Arc::from(ctx.source);
    let mut roms = Vec::new();
    let mut scaling_bases = Keyed::new();
    let mut first = Vec::new();
    if is(root, "roms") {
        ctx.check_attributes(root, &[]);
        for node in root.children() {
            match node {
                Node::Text(t) => {
                    let message = format!("text directly inside <{}>; ignored", root.name());
                    ctx.warning(codes::UNEXPECTED_TEXT, message, t.span());
                }
                Node::Element(child) if is(child, "rom") => {
                    roms.extend(rom::read_rom(ctx, &source, child));
                }
                Node::Element(child) if is(child, "scalingbase") => {
                    scaling::read_scaling_base(ctx, child, &mut scaling_bases, &mut first);
                }
                Node::Element(child) => ctx.unknown_element(child, root.name()),
            }
        }
    } else if is(root, "rom") {
        ctx.warning(
            codes::SINGLE_ROM_ROOT,
            format!(
                "the root element is <{}>, not <roms>; read as a file holding that one ROM \
                 (legacy RomRaider found no ROM in such a file)",
                root.name()
            ),
            root.span(),
        );
        roms.extend(rom::read_rom(ctx, &source, root));
    } else {
        ctx.error(
            codes::NOT_ROM_DEFINITION,
            format!(
                "the root element is <{}>, not <roms>: this is not a RomRaider ECU definition",
                root.name()
            ),
            root.span(),
        );
        return None;
    }
    Some(DefinitionFile {
        source,
        roms,
        scaling_bases,
        scaling_base_spans: first,
    })
}
