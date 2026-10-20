//! `cargo xtask gen-corpus-coverage [--check] [--dir <DIR>] [--manifest
//! <FILE>] [--coverage <FILE>]` (US-RR-16-6, D-025, D-028): which images of
//! the stock-ROM corpus have a RomRaider ECU definition in the pinned
//! definition corpus, committed as `fixtures/corpus-coverage.toml`.
//!
//! # What it reads
//!
//! The corpus directory `fetch-corpus` keeps (`--dir`, else `RR_CORPUS_DIR`,
//! else the per-user cache; [`crate::fetch_corpus::corpus_dir`]): every
//! image of the manifest's `[roms]` at its path, and every `.xml` file of
//! its `[defs]` under [`crate::fetch_corpus::defs_dir`]. Each file read is
//! checked against the manifest's size and SHA-256 first, so the report is
//! always about the pinned bytes; a mismatch fails and points at
//! `cargo xtask fetch-corpus`.
//!
//! # How an image is matched (legacy RomRaider semantics)
//!
//! This is a deliberately small, standalone reader, not the rr-defs one: it
//! extracts only each definition's `<romid>`, and matches exactly the way
//! legacy RomRaider picks a definition for an opened image
//! (`legacy/src/main/java/com/romraider/editor/ecu/OpenImageWorker.java`
//! `doInBackground`, `xml/DOMRomUnmarshaller.java` `findRomNodeMatch` /
//! `getBaseRom` / `unmarshallRomID`, `maps/RomID.java` `checkMatch`,
//! `util/HexUtil.java`):
//!
//! * only the `<rom>` children of a file's root element are candidates, in
//!   document order, and only each one's first `<romid>` child; the first
//!   candidate that matches is that file's match. A file whose root element
//!   is itself a `<rom>` (`ecu/AE5L500V_RR.xml` at the pin) never matches.
//!   Element names compare case-insensitively; a field's text is its
//!   direct text children (entities expanded, CDATA and nested elements
//!   ignored), not trimmed; a repeated field keeps its last value;
//! * `internalidaddress` is hex with every `0x` removed (`2000` is
//!   `0x2000`; absent means 0). One that does not parse as a 32-bit integer
//!   makes legacy throw out of the whole file, so the file yields no match
//!   for that image (the scan stops there);
//! * an empty or absent `internalidstring` never matches. Address -1 with
//!   `force` matches every image. A string starting `0x` is hex bytes
//!   (spaces and every `0x` dropped, an odd digit count left-padded with
//!   `0`) compared exactly; anything else is text, compared
//!   case-insensitively with the image bytes at the address, as many bytes
//!   as the string has UTF-16 units. Out-of-range reads do not match. (Legacy
//!   decodes those bytes with the platform charset; for an ASCII ID, as every
//!   ID at the pin is, that is the same comparison.)
//! * `<rom base="X">` inherits from the first `<rom>` of the *same file*
//!   whose `<xmlid>` equals `X` case-insensitively (`none` means no base),
//!   recursively. A missing base, a loop, or a `<romid>` without `<xmlid>`
//!   met while searching makes legacy fail to load the definition; such a
//!   match is listed with its `base_error`, and an image whose every match
//!   has one is `unresolved-base`, not `matched`.
//!
//! Legacy stops at the first definition file (in the user's configured
//! order) that matches; the report lists *every* matching file, in path
//! order, with the matched `<rom>`'s `xmlid`.
//!
//! A suspect image (`[roms.files.suspect]`, US-RR-11-7) is classified, never
//! a failure: when the manifest gives a standard-size image inside it
//! (`image_offset`, `image_size`: the two D-025 containers) that embedded
//! image is matched and the entry records `image_offset`; otherwise the raw
//! bytes are matched as legacy would.
//!
//! # ROM ID read from the image
//!
//! `image_rom_id` is what the image itself says, read by this code (the
//! manifest's `filename_rom_id` only repeats the file name): for a matched
//! image, the bytes at the first match's address (text when printable
//! ASCII, else `0x` hex), `image_rom_id_from = "definition"`; otherwise the
//! first [`ID_PROBE_LEN`]-byte run of ASCII letters and digits (at least one
//! of each) found at any address a pinned definition uses for a text ID, the
//! one equal to `filename_rom_id` preferred, `image_rom_id_from = "probe"`.
//! `[summary] image_id_differs_from_filename` lists the images where the two
//! differ (ignoring case and trailing spaces).
//!
//! # 2004 ADM (Australian) Liberty
//!
//! Story US-RR-16 asks specifically whether the 2004 AU Liberty naturally
//! aspirated ROM IDs are covered. Every image whose file-name fields say
//! region `ADM`, model `Liberty` and a year range including 2004 carries
//! `liberty_2004_adm`, its engine class from the file-name trim
//! ([`engine_class`]: `turbo`, `na-h6`, `na-4cyl`, `unknown`), and
//! `[liberty_2004_adm]` lists them by class with the definitions whose
//! `<romid>` says market `ADM`, model `Liberty`, year `04`/`2004` (by
//! `xmlid`; `definitions_na_4cyl` for the 2.0i/2.5i ones).
//!
//! # Licence rule (D-028)
//!
//! The definitions carry no licence, so the committed report holds metadata
//! only: paths, ROM IDs, definition file names and `xmlid`s, addresses, match
//! status and counts. No table, scaling or formula content is read into it.
//!
//! # Output and `--check`
//!
//! Without `--check` the report is written (only when it changed); with
//! `--check` nothing is written and the command fails unless the committed
//! file is byte-identical to a fresh generation, naming the entries that
//! differ. Output is deterministic (manifest order, sorted lists, `\n`
//! endings). The CI `corpus` job runs the check after fetching the corpus.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde::{Deserialize, Serialize};

use crate::corpus_manifest::{Manifest, RomFile, SuspectKind, sha256_hex};
use crate::fetch_corpus::defs_dir;

/// Where the report is committed, relative to the workspace root.
pub const COMMITTED_PATH: &str = "fixtures/corpus-coverage.toml";

/// Format version of the report.
pub const SCHEMA: u32 = 1;

/// Length of the ROM ID the probe looks for (`A2WC400H`).
pub const ID_PROBE_LEN: usize = 8;

const HEADER: &str = "\
# ECU-definition coverage of the stock-ROM corpus (US-RR-16-6, D-025, D-028).
# GENERATED: do not edit.
#   regenerate: cargo xtask gen-corpus-coverage   (needs a fetched corpus)
#   check:      cargo xtask gen-corpus-coverage --check
# For every [roms] image of fixtures/corpus-manifest.toml: the definition
# files of the pinned [defs] whose <romid> matches the image bytes (legacy
# RomRaider matching: internalidaddress + internalidstring; `base`
# inheritance resolved within the file), or status \"no-definition\".
# image_rom_id is read from the image by our own code; filename_rom_id is
# only what the file name says. liberty_2004_adm flags the 2004 ADM
# (Australian) Liberty images by engine class.
# Metadata only (D-028): paths, IDs, xmlids, addresses, status and counts;
# no definition content. Format: xtask/src/corpus_coverage.rs.
";

// ---------------------------------------------------------------------------
// The standalone <romid> reader
// ---------------------------------------------------------------------------

/// The `<romid>` fields of one `<rom>` element (only what matching, `base`
/// resolution and the Liberty check need).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefRom {
    /// The `base` attribute; `None` when absent or `none` (any case).
    pub base: Option<String>,
    /// Whether the `<rom>` has a `<romid>` child at all (legacy skips one
    /// that does not).
    pub has_romid: bool,
    /// `<xmlid>`.
    pub xmlid: Option<String>,
    /// `<internalidaddress>`, raw text ([`parse_address`]).
    pub address: Option<String>,
    /// `<internalidstring>`, empty when absent.
    pub id: String,
    /// `<market>`.
    pub market: Option<String>,
    /// `<model>`.
    pub model: Option<String>,
    /// `<year>`.
    pub year: Option<String>,
    /// `<submodel>`.
    pub submodel: Option<String>,
}

/// What [`read_definition`] extracts from one definition file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefDoc {
    /// The root element's name.
    pub root: String,
    /// The `<rom>` children of the root, in document order.
    pub roms: Vec<DefRom>,
    /// The root element itself, when it is a `<rom>` (never matched).
    pub root_rom: Option<DefRom>,
}

/// Reads the `<romid>` of every `<rom>` child of the root of a RomRaider
/// ECU definition (module docs). Tables and everything else are skipped.
///
/// # Errors
///
/// The XML is not well-formed (mismatched or unclosed tags), or a `<romid>`
/// field or `base` attribute does not decode.
pub fn read_definition(xml: &[u8]) -> Result<DefDoc> {
    let mut reader = Reader::from_reader(xml);
    let mut st = ReadState::default();
    loop {
        let event = reader
            .read_event()
            .with_context(|| format!("XML error near byte {}", reader.error_position()))?;
        match event {
            Event::Start(e) => {
                st.open(&e)?;
                st.depth += 1;
            }
            Event::Empty(e) => {
                st.open(&e)?;
                st.close();
            }
            Event::End(_) => {
                st.depth = st.depth.saturating_sub(1);
                st.close();
            }
            Event::Text(t) => {
                if let Some(text) = st.field_text() {
                    text.push_str(&t.xml10_content().context("romid text")?);
                }
            }
            Event::GeneralRef(r) => {
                if let Some(text) = st.field_text() {
                    let name = r.decode().context("romid entity")?;
                    if let Some(c) = r.resolve_char_ref().context("romid character reference")? {
                        text.push(c);
                    } else if let Some(s) = quick_xml::escape::resolve_predefined_entity(&name) {
                        text.push_str(s);
                    } else {
                        bail!("unknown entity `&{name};` in a <romid>");
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if st.depth != 0 || st.doc.root.is_empty() {
        bail!("XML ends inside an element, or has no root element");
    }
    Ok(st.doc)
}

/// [`read_definition`]'s state between events.
#[derive(Default)]
struct ReadState {
    doc: DefDoc,
    /// Open elements: the depth of the next element's parent.
    depth: usize,
    /// The `<rom>` being read and its depth (0: the root, 1: a root child).
    rom: Option<(usize, DefRom)>,
    /// Inside that `<rom>`'s first `<romid>`.
    in_romid: bool,
    /// That `<rom>` has had its first `<romid>`.
    seen_romid: bool,
    /// The `<romid>` field being read: lowercase name and text so far.
    field: Option<(String, String)>,
}

impl ReadState {
    /// The text of the field being read, when the reader is directly inside
    /// it (not in a nested element).
    fn field_text(&mut self) -> Option<&mut String> {
        let d = self.rom.as_ref()?.0;
        if !self.in_romid || self.depth != d + 3 {
            return None;
        }
        self.field.as_mut().map(|(_, text)| text)
    }

    fn open(&mut self, e: &BytesStart<'_>) -> Result<()> {
        let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
        let depth = self.depth;
        if depth == 0 {
            self.doc.root.clone_from(&name);
        }
        let is_rom = name.eq_ignore_ascii_case("rom");
        if is_rom && (depth == 0 || (depth == 1 && !self.doc.root.eq_ignore_ascii_case("rom"))) {
            let base = match e.try_get_attribute("base").context("<rom> attributes")? {
                Some(a) => {
                    let v = a
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .context("<rom base>")?;
                    (!v.eq_ignore_ascii_case("none")).then(|| v.into_owned())
                }
                None => None,
            };
            self.rom = Some((
                depth,
                DefRom {
                    base,
                    ..DefRom::default()
                },
            ));
            self.seen_romid = false;
            return Ok(());
        }
        if let Some((d, r)) = self.rom.as_mut() {
            if depth == *d + 1 && name.eq_ignore_ascii_case("romid") && !self.seen_romid {
                self.seen_romid = true;
                self.in_romid = true;
                r.has_romid = true;
            } else if self.in_romid && depth == *d + 2 {
                self.field = Some((name.to_ascii_lowercase(), String::new()));
            }
        }
        Ok(())
    }

    /// The element at `self.depth` has just closed.
    fn close(&mut self) {
        let Some(d) = self.rom.as_ref().map(|(d, _)| *d) else {
            return;
        };
        let depth = self.depth;
        if depth == d + 2 && self.in_romid {
            if let (Some((name, text)), Some((_, r))) = (self.field.take(), self.rom.as_mut()) {
                match name.as_str() {
                    "xmlid" => r.xmlid = Some(text),
                    "internalidaddress" => r.address = Some(text),
                    "internalidstring" => r.id = text,
                    "market" => r.market = Some(text),
                    "model" => r.model = Some(text),
                    "year" => r.year = Some(text),
                    "submodel" => r.submodel = Some(text),
                    _ => {}
                }
            }
        } else if depth == d + 1 && self.in_romid {
            self.in_romid = false;
        } else if depth == d
            && let Some((_, r)) = self.rom.take()
        {
            if d == 0 {
                self.doc.root_rom = Some(r);
            } else {
                self.doc.roms.push(r);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Legacy matching
// ---------------------------------------------------------------------------

/// `internalidaddress` as legacy reads it (`HexUtil.hexToInt`:
/// `Integer.parseInt(s.replace("0x", ""), 16)`); absent is 0.
///
/// # Errors
///
/// A description of a value that does not parse (legacy throws).
pub fn parse_address(raw: Option<&str>) -> std::result::Result<i32, String> {
    let Some(raw) = raw else { return Ok(0) };
    i32::from_str_radix(&raw.replace("0x", ""), 16)
        .map_err(|e| format!("internalidaddress {raw:?} does not parse: {e}"))
}

/// The bytes of a `0x...` ID (`HexUtil.asBytes`), or `None` where legacy
/// throws (and so does not match).
fn hex_id_bytes(hex: &str) -> Option<Vec<u8>> {
    let spaced: Vec<char> = hex.chars().filter(|c| *c != ' ').collect();
    // `replaceAll("(?i)0x", "")`: one left-to-right pass.
    let mut s = String::new();
    let mut i = 0;
    while let Some(&c) = spaced.get(i) {
        if c == '0'
            && spaced
                .get(i + 1)
                .is_some_and(|x| x.eq_ignore_ascii_case(&'x'))
        {
            i += 2;
        } else {
            s.push(c);
            i += 1;
        }
    }
    if !s.chars().count().is_multiple_of(2) {
        s.insert(0, '0');
    }
    let digits: Vec<u32> = s.chars().map(|c| c.to_digit(16)).collect::<Option<_>>()?;
    Some(
        digits
            .chunks(2)
            .map(|p| p.iter().fold(0u8, |acc, d| (acc << 4) | (*d as u8)))
            .collect(),
    )
}

/// Java `String.equalsIgnoreCase`, char by char.
fn java_eq_ignore_case(a: &str, b: &str) -> bool {
    a.chars().count() == b.chars().count()
        && a.chars().zip(b.chars()).all(|(x, y)| {
            x == y || x.to_uppercase().eq(y.to_uppercase()) || x.to_lowercase().eq(y.to_lowercase())
        })
}

/// The image window `[address, address + len)`, if in range.
fn window(image: &[u8], address: i32, len: usize) -> Option<&[u8]> {
    let start = usize::try_from(address).ok()?;
    image.get(start..start.checked_add(len)?)
}

/// Legacy `RomID.checkMatch`: does the ID `id` at `address` match `image`?
#[must_use]
pub fn id_matches(address: i32, id: &str, image: &[u8]) -> bool {
    if id.is_empty() {
        return false;
    }
    if address == -1 && id.eq_ignore_ascii_case("force") {
        return true;
    }
    let units = id.encode_utf16().count();
    if units > 2 && id.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("0x")) {
        let Some(want) = id.get(2..).and_then(hex_id_bytes) else {
            return false;
        };
        return window(image, address, want.len()) == Some(want.as_slice());
    }
    window(image, address, units)
        .is_some_and(|got| java_eq_ignore_case(&String::from_utf8_lossy(got), id))
}

/// How a definition file answers one image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileMatch {
    /// The `<rom>` at this index of [`DefDoc::roms`] matches first.
    Rom(usize),
    /// No `<rom>` matches.
    None,
    /// Legacy throws before finding a match (a bad `internalidaddress`).
    Aborted(String),
}

/// Legacy `findRomNodeMatch` by image bytes over one file.
#[must_use]
pub fn match_file(doc: &DefDoc, image: &[u8]) -> FileMatch {
    for (i, rom) in doc.roms.iter().enumerate().filter(|(_, r)| r.has_romid) {
        match parse_address(rom.address.as_deref()) {
            Err(e) => return FileMatch::Aborted(e),
            Ok(a) if id_matches(a, &rom.id, image) => return FileMatch::Rom(i),
            Ok(_) => {}
        }
    }
    FileMatch::None
}

/// Legacy `findRomNodeMatch` by `xmlid` (for `base`).
fn find_by_xmlid(doc: &DefDoc, xmlid: &str) -> std::result::Result<Option<usize>, String> {
    for (i, rom) in doc.roms.iter().enumerate().filter(|(_, r)| r.has_romid) {
        parse_address(rom.address.as_deref())?;
        match &rom.xmlid {
            None => {
                return Err(format!(
                    "a <romid> without <xmlid> is met while looking for base `{xmlid}`"
                ));
            }
            Some(x) if java_eq_ignore_case(x, xmlid) => return Ok(Some(i)),
            Some(_) => {}
        }
    }
    Ok(None)
}

/// The `base` chain of `doc.roms[index]` (legacy `getBaseRom`, recursive),
/// nearest first.
///
/// # Errors
///
/// Why legacy cannot load it: a base not in the file, a loop, or a lookup
/// that throws.
pub fn base_chain(doc: &DefDoc, index: usize) -> std::result::Result<Vec<String>, String> {
    let mut chain = Vec::new();
    let mut seen = BTreeSet::new();
    let mut at = index;
    while let Some(base) = doc.roms.get(at).and_then(|r| r.base.clone()) {
        if !seen.insert(base.to_lowercase()) {
            return Err(format!("base `{base}` loops"));
        }
        match find_by_xmlid(doc, &base)? {
            Some(i) => {
                chain.push(base);
                at = i;
            }
            None => return Err(format!("base `{base}` is not in this file")),
        }
    }
    Ok(chain)
}

// ---------------------------------------------------------------------------
// The report
// ---------------------------------------------------------------------------

/// The whole report file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    /// Format version, [`SCHEMA`].
    pub schema: u32,
    /// The manifest's `[roms] commit`.
    pub roms_commit: String,
    /// The manifest's `[defs] commit`.
    pub defs_commit: String,
    /// Totals.
    pub summary: Summary,
    /// The 2004 ADM Liberty check.
    pub liberty_2004_adm: Liberty,
    /// Every `.xml` file of the definition corpus, by path.
    pub definition_files: Vec<DefinitionFile>,
    /// Every manifest image, in manifest order.
    pub roms: Vec<RomCoverage>,
}

/// Totals over all images.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    /// Images in the manifest.
    pub images: usize,
    /// Images with at least one usable matching definition.
    pub matched: usize,
    /// Images whose every match has a `base_error`.
    pub unresolved_base: usize,
    /// Images no definition matches.
    pub no_definition: usize,
    /// Suspect images (classified, matched on the embedded image if any).
    pub suspect: usize,
    /// Images whose `filename_rom_id` equals some `<romid>` `xmlid` in the
    /// definitions (ignoring case and surrounding spaces, any `<rom>`
    /// including a root one): D-028's method, for the cross-check.
    pub filename_id_has_definition: usize,
    /// Images whose ID read from the image differs from `filename_rom_id`.
    pub image_id_differs_from_filename: Vec<String>,
    /// The same counts by the manifest's (file-name) region; an entry
    /// without `region` counts the images whose name gives none.
    pub regions: Vec<RegionCount>,
}

/// Counts for one region.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionCount {
    /// The region; absent for images whose file name gives none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// Images.
    pub images: usize,
    /// Of those, matched.
    pub matched: usize,
    /// Of those, with a definition by file-name ID (D-028's method).
    pub filename_id_has_definition: usize,
}

/// The 2004 ADM Liberty check (module docs). Image lists hold
/// `filename_rom_id`s (the path when there is none).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Liberty {
    /// Images flagged.
    pub images: usize,
    /// Of those, matched.
    pub matched: usize,
    /// Turbo images.
    pub turbo: Vec<String>,
    /// Naturally aspirated flat-six (3.0R) images.
    pub na_h6: Vec<String>,
    /// Naturally aspirated four-cylinder (2.0i/2.5i) images.
    pub na_4cyl: Vec<String>,
    /// Images whose trim does not say.
    pub unknown: Vec<String>,
    /// Images flagged that no definition matches.
    pub no_definition: Vec<String>,
    /// `xmlid`s of definitions whose `<romid>` is a 2004 ADM Liberty.
    pub definitions: Vec<String>,
    /// Of those, the naturally aspirated four-cylinder ones.
    pub definitions_na_4cyl: Vec<String>,
}

/// One `.xml` file of the definition corpus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefinitionFile {
    /// Path relative to the definitions tree (as in the manifest).
    pub path: String,
    /// Its root element; absent when the file does not parse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// `<rom>` children of the root with a `<romid>`.
    pub roms: usize,
    /// Of those, with a non-empty `internalidstring`.
    pub with_id: usize,
    /// Corpus images this file matches.
    pub matched_images: usize,
    /// Why some or all of it cannot match or load (parse error, root
    /// `<rom>`, bad address, unresolvable `base`), by `xmlid`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<String>,
}

/// Whether an image has a definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// At least one matching definition loads.
    Matched,
    /// Definitions match, but none can load (every one has a `base_error`).
    UnresolvedBase,
    /// No definition matches.
    NoDefinition,
}

/// Where `image_rom_id` comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IdSource {
    /// The address and length of the first matching definition.
    Definition,
    /// The probe over known ID addresses (no definition matched).
    Probe,
}

/// Engine class of a 2004 ADM Liberty ([`engine_class`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EngineClass {
    /// Turbocharged (GT, B4).
    Turbo,
    /// Naturally aspirated flat-six (3.0R).
    NaH6,
    /// Naturally aspirated four-cylinder (2.0i, 2.5i, 2.0R).
    Na4cyl,
    /// The trim does not say.
    Unknown,
}

/// One image's coverage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RomCoverage {
    /// Manifest path.
    pub path: String,
    /// The manifest's `filename_rom_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename_rom_id: Option<String>,
    /// The ROM ID read from the image (module docs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_rom_id: Option<String>,
    /// Where it was read, `0x` hex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_rom_id_address: Option<String>,
    /// How it was found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_rom_id_from: Option<IdSource>,
    /// Match status.
    pub status: Status,
    /// The manifest's suspect kind, when the image is suspect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suspect: Option<SuspectKind>,
    /// Offset of the embedded image that was matched instead of the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_offset: Option<u64>,
    /// Set on 2004 ADM Liberty images: the engine class.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub liberty_2004_adm: Option<EngineClass>,
    /// Every matching definition file, by path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub definitions: Vec<DefinitionMatch>,
}

/// One matching definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefinitionMatch {
    /// The definition file (path relative to the definitions tree).
    pub file: String,
    /// The matching `<rom>`'s `xmlid`; absent when it has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xmlid: Option<String>,
    /// Why legacy cannot load it, when it cannot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_error: Option<String>,
}

impl Coverage {
    /// Parses a report.
    ///
    /// # Errors
    ///
    /// Invalid TOML, an unknown field, or a schema other than [`SCHEMA`].
    pub fn from_toml(text: &str) -> Result<Self> {
        let c: Self = toml::from_str(text).context("corpus coverage: invalid TOML")?;
        if c.schema != SCHEMA {
            bail!("corpus coverage: schema {} (expected {SCHEMA})", c.schema);
        }
        Ok(c)
    }

    /// The canonical text: the generated-file header, then the TOML.
    ///
    /// # Errors
    ///
    /// Only if TOML serialisation fails.
    pub fn render(&self) -> Result<String> {
        let body = toml::to_string(self).context("corpus coverage: serialising")?;
        Ok(format!("{HEADER}\n{body}"))
    }
}

/// The engine class a 2004 Liberty trim or submodel names.
#[must_use]
pub fn engine_class(trim: &str) -> EngineClass {
    let t = trim.to_ascii_uppercase();
    let has = |s: &str| t.contains(s);
    if has("3.0R") || has("H6") {
        EngineClass::NaH6
    } else if has("GT") || has("B4") || has("TURBO") {
        EngineClass::Turbo
    } else if has("2.0I") || has("2.5I") || has("2.0R") {
        EngineClass::Na4cyl
    } else {
        EngineClass::Unknown
    }
}

/// Whether the manifest's file-name fields make `f` a 2004 ADM Liberty.
#[must_use]
pub fn is_liberty_2004_adm(f: &RomFile) -> bool {
    f.region.as_deref() == Some("ADM")
        && f.model
            .as_deref()
            .is_some_and(|m| m.to_ascii_lowercase().contains("liberty"))
        && f.year
            .is_some_and(|y| y <= 2004 && 2004 <= f.year_end.unwrap_or(y))
}

fn is_liberty_2004_adm_def(r: &DefRom) -> bool {
    let is =
        |v: &Option<String>, f: &dyn Fn(&str) -> bool| v.as_deref().map(str::trim).is_some_and(f);
    is(&r.market, &|m| m.eq_ignore_ascii_case("ADM"))
        && is(&r.model, &|m| m.to_ascii_lowercase().contains("liberty"))
        && is(&r.year, &|y| y == "04" || y == "2004")
}

/// A definition file and what [`read_definition`] made of it.
#[derive(Debug, Clone)]
pub struct DefFileDoc {
    /// Path relative to the definitions tree.
    pub path: String,
    /// The extracted `<romid>`s, or why the file does not parse.
    pub doc: std::result::Result<DefDoc, String>,
}

fn printable(bytes: &[u8]) -> String {
    if bytes.iter().all(|b| (0x20..0x7f).contains(b)) {
        String::from_utf8_lossy(bytes).into_owned()
    } else {
        format!("0x{}", upper_hex(bytes))
    }
}

fn upper_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02X}");
        s
    })
}

fn hex_address(a: i32) -> String {
    format!("0x{a:X}")
}

/// The probe (module docs): the first plausible ROM ID at one of
/// `addresses`, the one equal to `filename_id` preferred.
#[must_use]
pub fn probe_id(
    image: &[u8],
    addresses: &[i32],
    filename_id: Option<&str>,
) -> Option<(i32, String)> {
    let plausible = |w: &[u8]| {
        w.iter().all(u8::is_ascii_alphanumeric)
            && w.iter().any(u8::is_ascii_alphabetic)
            && w.iter().any(u8::is_ascii_digit)
    };
    let found: Vec<(i32, String)> = addresses
        .iter()
        .filter_map(|a| {
            let w = window(image, *a, ID_PROBE_LEN)?;
            plausible(w).then(|| (*a, String::from_utf8_lossy(w).into_owned()))
        })
        .collect();
    let preferred =
        filename_id.and_then(|f| found.iter().find(|(_, id)| id.eq_ignore_ascii_case(f)));
    preferred.or_else(|| found.first()).cloned()
}

fn same_id(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

fn id_label(f: &RomFile) -> String {
    f.filename_rom_id.clone().unwrap_or_else(|| f.path.clone())
}

/// Builds the report for `roms` against the parsed definition files `defs`
/// (in path order); `image` returns an image's bytes.
///
/// # Errors
///
/// Only what `image` returns.
pub fn cover(
    roms_commit: &str,
    defs_commit: &str,
    roms: &[RomFile],
    defs: &[DefFileDoc],
    mut image: impl FnMut(&RomFile) -> Result<Vec<u8>>,
) -> Result<Coverage> {
    let parsed: Vec<(&str, &DefDoc)> = defs
        .iter()
        .filter_map(|d| d.doc.as_ref().ok().map(|doc| (d.path.as_str(), doc)))
        .collect();
    let probe_addresses: Vec<i32> = parsed
        .iter()
        .flat_map(|(_, d)| &d.roms)
        .filter(|r| r.has_romid && !r.id.is_empty() && !r.id.to_ascii_lowercase().starts_with("0x"))
        .filter_map(|r| parse_address(r.address.as_deref()).ok())
        .filter(|a| *a >= 0)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let all_xmlids: Vec<&str> = parsed
        .iter()
        .flat_map(|(_, d)| d.roms.iter().chain(&d.root_rom))
        .filter_map(|r| r.xmlid.as_deref())
        .collect();

    let mut matched_per_file: BTreeMap<&str, usize> = BTreeMap::new();
    let mut out_roms = Vec::with_capacity(roms.len());
    for f in roms {
        let bytes = image(f)?;
        let embedded = f.suspect.as_ref().and_then(|s| {
            let off = s.image_offset?;
            let start = usize::try_from(off).ok()?;
            let len = usize::try_from(s.image_size?).ok()?;
            bytes.get(start..start.checked_add(len)?).map(|b| (off, b))
        });
        let (image_offset, img) = match embedded {
            Some((off, b)) => (Some(off), b),
            None => (None, bytes.as_slice()),
        };
        let mut definitions = Vec::new();
        let mut first: Option<(i32, usize)> = None;
        for (path, doc) in &parsed {
            if let FileMatch::Rom(i) = match_file(doc, img) {
                let rom = &doc.roms[i];
                let base_error = base_chain(doc, i).err();
                if first.is_none()
                    && base_error.is_none()
                    && let Ok(a) = parse_address(rom.address.as_deref())
                {
                    let len = if rom.id.to_ascii_lowercase().starts_with("0x") {
                        rom.id
                            .get(2..)
                            .and_then(hex_id_bytes)
                            .map_or(0, |b| b.len())
                    } else {
                        rom.id.encode_utf16().count()
                    };
                    first = Some((a, len));
                }
                *matched_per_file.entry(path).or_default() += 1;
                definitions.push(DefinitionMatch {
                    file: (*path).to_owned(),
                    xmlid: rom.xmlid.clone(),
                    base_error,
                });
            }
        }
        let status = if definitions.iter().any(|d| d.base_error.is_none()) {
            Status::Matched
        } else if definitions.is_empty() {
            Status::NoDefinition
        } else {
            Status::UnresolvedBase
        };
        let (image_rom_id, address, from) = match first {
            Some((a, len)) if a >= 0 => {
                let id = window(img, a, len).map(printable);
                (id, Some(a), Some(IdSource::Definition))
            }
            Some((a, _)) => (None, Some(a), Some(IdSource::Definition)),
            None => match probe_id(img, &probe_addresses, f.filename_rom_id.as_deref()) {
                Some((a, id)) => (Some(id), Some(a), Some(IdSource::Probe)),
                None => (None, None, None),
            },
        };
        out_roms.push(RomCoverage {
            path: f.path.clone(),
            filename_rom_id: f.filename_rom_id.clone(),
            image_rom_id,
            image_rom_id_address: address.map(hex_address),
            image_rom_id_from: from,
            status,
            suspect: f.suspect.as_ref().map(|s| s.kind),
            image_offset,
            liberty_2004_adm: is_liberty_2004_adm(f)
                .then(|| engine_class(f.trim.as_deref().unwrap_or(""))),
            definitions,
        });
    }

    let by_filename = |f: &RomFile| {
        f.filename_rom_id
            .as_deref()
            .is_some_and(|id| all_xmlids.iter().any(|x| same_id(x, id)))
    };
    let mut regions: BTreeMap<Option<String>, RegionCount> = BTreeMap::new();
    for (f, c) in roms.iter().zip(&out_roms) {
        let e = regions
            .entry(f.region.clone())
            .or_insert_with(|| RegionCount {
                region: f.region.clone(),
                images: 0,
                matched: 0,
                filename_id_has_definition: 0,
            });
        e.images += 1;
        e.matched += usize::from(c.status == Status::Matched);
        e.filename_id_has_definition += usize::from(by_filename(f));
    }
    let count = |s: Status| out_roms.iter().filter(|c| c.status == s).count();
    let summary = Summary {
        images: roms.len(),
        matched: count(Status::Matched),
        unresolved_base: count(Status::UnresolvedBase),
        no_definition: count(Status::NoDefinition),
        suspect: roms.iter().filter(|f| f.is_suspect()).count(),
        filename_id_has_definition: roms.iter().filter(|f| by_filename(f)).count(),
        image_id_differs_from_filename: out_roms
            .iter()
            .filter(|c| match (&c.image_rom_id, &c.filename_rom_id) {
                (Some(i), Some(n)) => !same_id(i, n),
                _ => false,
            })
            .map(|c| c.path.clone())
            .collect(),
        regions: regions.into_values().collect(),
    };

    let mut liberty = Liberty {
        images: 0,
        matched: 0,
        turbo: Vec::new(),
        na_h6: Vec::new(),
        na_4cyl: Vec::new(),
        unknown: Vec::new(),
        no_definition: Vec::new(),
        definitions: Vec::new(),
        definitions_na_4cyl: Vec::new(),
    };
    for (f, c) in roms.iter().zip(&out_roms) {
        let Some(class) = c.liberty_2004_adm else {
            continue;
        };
        liberty.images += 1;
        if c.status == Status::Matched {
            liberty.matched += 1;
        } else {
            liberty.no_definition.push(id_label(f));
        }
        match class {
            EngineClass::Turbo => &mut liberty.turbo,
            EngineClass::NaH6 => &mut liberty.na_h6,
            EngineClass::Na4cyl => &mut liberty.na_4cyl,
            EngineClass::Unknown => &mut liberty.unknown,
        }
        .push(id_label(f));
    }
    let mut lib_defs = BTreeSet::new();
    let mut lib_na4 = BTreeSet::new();
    for r in parsed
        .iter()
        .flat_map(|(_, d)| d.roms.iter().chain(&d.root_rom))
        .filter(|r| is_liberty_2004_adm_def(r))
    {
        let x = r.xmlid.clone().unwrap_or_default();
        if engine_class(r.submodel.as_deref().unwrap_or("")) == EngineClass::Na4cyl {
            lib_na4.insert(x.clone());
        }
        lib_defs.insert(x);
    }
    liberty.definitions = lib_defs.into_iter().collect();
    liberty.definitions_na_4cyl = lib_na4.into_iter().collect();

    let definition_files = defs
        .iter()
        .map(|d| definition_file(d, matched_per_file.get(d.path.as_str()).copied()))
        .collect();

    Ok(Coverage {
        schema: SCHEMA,
        roms_commit: roms_commit.to_owned(),
        defs_commit: defs_commit.to_owned(),
        summary,
        liberty_2004_adm: liberty,
        definition_files,
        roms: out_roms,
    })
}

fn definition_file(d: &DefFileDoc, matched: Option<usize>) -> DefinitionFile {
    let doc = match &d.doc {
        Ok(doc) => doc,
        Err(e) => {
            return DefinitionFile {
                path: d.path.clone(),
                root: None,
                roms: 0,
                with_id: 0,
                matched_images: 0,
                problems: vec![format!("does not parse: {e}")],
            };
        }
    };
    let with_romid: Vec<(usize, &DefRom)> = doc
        .roms
        .iter()
        .enumerate()
        .filter(|(_, r)| r.has_romid)
        .collect();
    let mut problems = Vec::new();
    if doc.root_rom.is_some() {
        problems.push(
            "the root element is a <rom>: legacy only matches <rom> children of the root, so this file never matches"
                .to_owned(),
        );
    }
    let name = |r: &DefRom| r.xmlid.clone().unwrap_or_else(|| "(no xmlid)".to_owned());
    for (i, r) in &with_romid {
        if let Err(e) = parse_address(r.address.as_deref()) {
            problems.push(format!("{}: {e}", name(r)));
        }
        if let Err(e) = base_chain(doc, *i) {
            problems.push(format!("{}: {e}", name(r)));
        }
    }
    DefinitionFile {
        path: d.path.clone(),
        root: Some(doc.root.clone()),
        roms: with_romid.len(),
        with_id: with_romid.iter().filter(|(_, r)| !r.id.is_empty()).count(),
        matched_images: matched.unwrap_or(0),
        problems,
    }
}

// ---------------------------------------------------------------------------
// Generation from a fetched corpus, and --check
// ---------------------------------------------------------------------------

/// The result of [`generate`].
#[derive(Debug)]
pub struct Generated {
    /// The report.
    pub coverage: Coverage,
    /// Its canonical bytes ([`Coverage::render`]).
    pub text: String,
}

/// Reads `rel` under `dir`, failing unless it has the manifest's `size` and
/// `sha256`.
fn read_verified(dir: &Path, rel: &str, size: u64, sha256: &str) -> Result<Vec<u8>> {
    let path = dir.join(rel);
    let bytes = fs::read(&path).with_context(|| {
        format!(
            "reading {} (run `cargo xtask fetch-corpus` first)",
            path.display()
        )
    })?;
    if u64::try_from(bytes.len()).ok() != Some(size) || sha256_hex(&bytes) != sha256 {
        bail!(
            "{}: not the manifest's bytes (size or SHA-256 differs); run `cargo xtask fetch-corpus`",
            path.display()
        );
    }
    Ok(bytes)
}

/// Whether a definitions path is an XML file.
#[must_use]
pub fn is_xml(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("xml"))
}

/// Generates the report from the fetched corpus at `corpus` (module docs).
///
/// # Errors
///
/// The manifest has no `[defs]`, or a file is missing or differs from the
/// manifest.
pub fn generate(corpus: &Path, manifest: &Manifest) -> Result<Generated> {
    let Some(defs) = &manifest.defs else {
        bail!("the manifest has no [defs] table: no definitions to cover the corpus with");
    };
    let defs_root = defs_dir(corpus);
    let mut docs = Vec::new();
    for f in defs.files.iter().filter(|f| is_xml(&f.path)) {
        let bytes = read_verified(&defs_root, &f.path, f.size, &f.sha256)?;
        docs.push(DefFileDoc {
            path: f.path.clone(),
            doc: read_definition(&bytes).map_err(|e| format!("{e:#}")),
        });
    }
    let coverage = cover(
        &manifest.roms.commit,
        &defs.commit,
        &manifest.roms.files,
        &docs,
        |f| read_verified(corpus, &f.path, f.size, &f.sha256),
    )?;
    let text = coverage.render()?;
    Ok(Generated { coverage, text })
}

/// Why the committed report differs from the generated one; empty when they
/// are byte-identical. Names entries where possible.
#[must_use]
pub fn compare_committed(generated: &Generated, committed: &str) -> Vec<String> {
    if committed == generated.text {
        return Vec::new();
    }
    let old = match Coverage::from_toml(committed) {
        Ok(c) => c,
        Err(e) => return vec![format!("committed report does not parse: {e:#}")],
    };
    let new = &generated.coverage;
    let mut problems = Vec::new();
    for (field, a, b) in [
        ("roms_commit", &old.roms_commit, &new.roms_commit),
        ("defs_commit", &old.defs_commit, &new.defs_commit),
    ] {
        if a != b {
            problems.push(format!("{field} {a:?} (generated {b:?})"));
        }
    }
    if old.summary != new.summary {
        problems.push(format!(
            "[summary] differs: committed {} matched / {} no-definition, generated {} / {}",
            old.summary.matched,
            old.summary.no_definition,
            new.summary.matched,
            new.summary.no_definition
        ));
    }
    if old.liberty_2004_adm != new.liberty_2004_adm {
        problems.push("[liberty_2004_adm] differs".to_owned());
    }
    let index = |v: &[RomCoverage]| -> BTreeMap<String, RomCoverage> {
        v.iter().map(|r| (r.path.clone(), r.clone())).collect()
    };
    let (old_roms, new_roms) = (index(&old.roms), index(&new.roms));
    for (path, want) in &new_roms {
        match old_roms.get(path) {
            None => problems.push(format!("{path}: missing from the committed report")),
            Some(got) if got != want => problems.push(format!(
                "{path}: committed {:?} with {} definition(s), generated {:?} with {}",
                got.status,
                got.definitions.len(),
                want.status,
                want.definitions.len()
            )),
            Some(_) => {}
        }
    }
    for path in old_roms.keys().filter(|p| !new_roms.contains_key(*p)) {
        problems.push(format!(
            "{path}: in the committed report but not in the manifest"
        ));
    }
    let files = |v: &[DefinitionFile]| -> BTreeMap<String, DefinitionFile> {
        v.iter().map(|f| (f.path.clone(), f.clone())).collect()
    };
    let (old_files, new_files) = (files(&old.definition_files), files(&new.definition_files));
    for (path, want) in &new_files {
        if old_files.get(path) != Some(want) {
            problems.push(format!("definition file {path}: committed entry differs"));
        }
    }
    for path in old_files.keys().filter(|p| !new_files.contains_key(*p)) {
        problems.push(format!(
            "definition file {path}: no longer in the definitions"
        ));
    }
    if problems.is_empty() {
        let hint = if committed.contains('\r') {
            " (it has \\r\\n line endings: check fixtures/.gitattributes and core.autocrlf)"
        } else {
            ""
        };
        problems.push(format!(
            "same entries, but the committed bytes are not the canonical rendering{hint}"
        ));
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus_manifest::Suspect;

    /// A synthetic definition: a base `<rom>` and the given `<rom>` bodies.
    fn def(roms: &[&str]) -> Vec<u8> {
        format!(
            "<?xml version=\"1.0\"?>\n<!-- synthetic, not a real definition -->\n<roms>\n\
             <rom><romid><xmlid>32BITBASE</xmlid></romid><table name=\"t\" storageaddress=\"0x10\"/></rom>\n{}\n</roms>\n",
            roms.join("\n")
        )
        .into_bytes()
    }

    fn rom(xmlid: &str, address: &str, id: &str) -> String {
        format!(
            "<rom base=\"32BITBASE\"><romid><xmlid>{xmlid}</xmlid><internalidaddress>{address}</internalidaddress>\
             <internalidstring>{id}</internalidstring></romid><table name=\"x\"><table type=\"X Axis\"/></table></rom>"
        )
    }

    /// A synthetic image of `len` 0xFF bytes with `id` at `at`.
    fn image(len: usize, at: usize, id: &[u8]) -> Vec<u8> {
        let mut v = vec![0xFF; len];
        v[at..at + id.len()].copy_from_slice(id);
        v
    }

    fn rom_file(path: &str, id: Option<&str>) -> RomFile {
        RomFile {
            path: path.to_owned(),
            size: 0,
            sha256: String::new(),
            filename_rom_id: id.map(str::to_owned),
            year: None,
            year_end: None,
            region: None,
            model: None,
            trim: None,
            suspect: None,
        }
    }

    fn doc(path: &str, xml: &[u8]) -> DefFileDoc {
        DefFileDoc {
            path: path.to_owned(),
            doc: read_definition(xml).map_err(|e| format!("{e:#}")),
        }
    }

    #[test]
    fn reads_first_romid_of_each_root_rom_child() {
        let xml = br#"<roms>
  <ROM base="none"><RomId><XmlId>A&amp;B</XmlId><internalidaddress>2000</internalidaddress>
    <internalidstring>A2WC<!-- c -->400H</internalidstring><market>ADM</market></RomId>
    <romid><xmlid>SECOND</xmlid></romid></ROM>
  <rom base="32BITBASE"><table><romid><xmlid>NESTED</xmlid></romid></table></rom>
  <rom/>
  <other><rom><romid><xmlid>DEEP</xmlid></romid></rom></other>
</roms>"#;
        let d = read_definition(xml).unwrap();
        assert_eq!(d.root, "roms");
        assert_eq!(d.root_rom, None);
        assert_eq!(d.roms.len(), 3, "{d:?}");
        let r = &d.roms[0];
        assert_eq!(r.base, None, "base=\"none\" is no base");
        assert!(r.has_romid);
        assert_eq!(r.xmlid.as_deref(), Some("A&B"));
        assert_eq!(r.address.as_deref(), Some("2000"));
        assert_eq!(r.id, "A2WC400H", "text around a comment is joined");
        assert_eq!(r.market.as_deref(), Some("ADM"));
        let r = &d.roms[1];
        assert_eq!(r.base.as_deref(), Some("32BITBASE"));
        assert!(!r.has_romid, "a <romid> inside a <table> is not the rom's");
        assert!(!d.roms[2].has_romid);
    }

    #[test]
    fn root_rom_is_recorded_but_not_a_candidate() {
        let d = read_definition(
            b"<rom><romid><xmlid>AE5L500V</xmlid><internalidaddress>2004</internalidaddress><internalidstring>AE5L500V</internalidstring></romid></rom>",
        )
        .unwrap();
        assert!(d.roms.is_empty());
        assert_eq!(
            d.root_rom.and_then(|r| r.xmlid).as_deref(),
            Some("AE5L500V")
        );
    }

    #[test]
    fn malformed_xml_is_an_error() {
        assert!(read_definition(b"<roms><rom></roms>").is_err());
        assert!(read_definition(b"<roms><rom>").is_err());
        assert!(read_definition(b"").is_err());
        assert!(
            read_definition(b"<roms><rom><romid><xmlid>&bogus;</xmlid></romid></rom></roms>")
                .is_err()
        );
    }

    #[test]
    fn addresses_parse_like_legacy() {
        assert_eq!(parse_address(None), Ok(0));
        assert_eq!(parse_address(Some("2000")), Ok(0x2000));
        assert_eq!(parse_address(Some("0x2004")), Ok(0x2004));
        assert_eq!(parse_address(Some("E1000")), Ok(0xE_1000));
        assert_eq!(parse_address(Some("-1")), Ok(-1));
        assert!(parse_address(Some("")).is_err());
        assert!(
            parse_address(Some(" 2000")).is_err(),
            "legacy does not trim"
        );
        assert!(
            parse_address(Some("0X2000")).is_err(),
            "only lowercase 0x is removed"
        );
        assert!(parse_address(Some("100000000")).is_err(), "beyond 32 bits");
    }

    #[test]
    fn text_ids_match_case_insensitively_in_range_only() {
        let img = image(0x3000, 0x2000, b"A2WC400H");
        assert!(id_matches(0x2000, "A2WC400H", &img));
        assert!(id_matches(0x2000, "a2wc400h", &img));
        assert!(
            id_matches(0x2000, "A2WC40", &img),
            "a prefix of the bytes matches"
        );
        assert!(!id_matches(0x2000, "A2WC400J", &img));
        assert!(!id_matches(0x2004, "A2WC400H", &img));
        assert!(!id_matches(0x2FFC, "A2WC400H", &img), "runs past the end");
        assert!(!id_matches(-4, "A2WC400H", &img));
        assert!(!id_matches(0x2000, "", &img), "an empty ID never matches");
        let padded = image(0x3000, 0x2000, b"A2WC50  ");
        assert!(
            id_matches(0x2000, "A2WC50  ", &padded),
            "spaces are part of the ID"
        );
        assert!(!id_matches(0x2000, "A2WC50ZU", &padded));
    }

    #[test]
    fn hex_ids_and_force() {
        let img = image(0x100, 0x10, &[0x12, 0x34, 0xAB]);
        assert!(id_matches(0x10, "0x1234AB", &img));
        assert!(id_matches(0x10, "0X12 34 ab", &img));
        assert!(
            id_matches(
                0x10,
                "0x234AB",
                &[&[0u8; 0x10][..], &[0x02, 0x34, 0xAB]].concat()
            ),
            "odd count: left-padded"
        );
        assert!(!id_matches(0x10, "0x1234AC", &img));
        assert!(!id_matches(0x10, "0x12G4", &img), "not hex: no match");
        assert!(id_matches(-1, "force", &img));
        assert!(id_matches(-1, "FORCE", &[]));
        assert!(!id_matches(0, "force", &img));
    }

    #[test]
    fn first_matching_rom_of_a_file_wins_and_a_bad_address_aborts() {
        let img = image(0x3000, 0x2000, b"A2WC400H");
        let d = read_definition(&def(&[
            &rom("NOPE", "2000", "ZZZZZZZZ"),
            &rom("FIRST", "2000", "A2WC400H"),
            &rom("SECOND", "2000", "A2WC4"),
        ]))
        .unwrap();
        assert_eq!(
            match_file(&d, &img),
            FileMatch::Rom(2),
            "index 0 is the base"
        );
        let d = read_definition(&def(&[
            &rom("BAD", "zz", "X"),
            &rom("OK", "2000", "A2WC400H"),
        ]))
        .unwrap();
        assert!(matches!(match_file(&d, &img), FileMatch::Aborted(e) if e.contains("\"zz\"")));
        let d = read_definition(&def(&[
            &rom("OK", "2000", "A2WC400H"),
            &rom("BAD", "zz", "X"),
        ]))
        .unwrap();
        assert_eq!(
            match_file(&d, &img),
            FileMatch::Rom(1),
            "a bad address after the match is never reached"
        );
        assert_eq!(
            match_file(&d, &image(0x3000, 0, b"")),
            FileMatch::Aborted(
                "internalidaddress \"zz\" does not parse: invalid digit found in string".to_owned()
            )
        );
    }

    #[test]
    fn base_inheritance_resolves_within_the_file() {
        let d = read_definition(&def(&[
            &rom("CHILD", "2000", "C").replace("base=\"32BITBASE\"", "base=\"parent\""),
            &rom("PARENT", "2000", "P"),
        ]))
        .unwrap();
        assert_eq!(
            base_chain(&d, 1),
            Ok(vec!["parent".to_owned(), "32BITBASE".to_owned()])
        );
        assert_eq!(base_chain(&d, 0), Ok(vec![]));
        let d = read_definition(&def(&[
            &rom("A", "2000", "A").replace("32BITBASE", "MISSING")
        ]))
        .unwrap();
        assert_eq!(
            base_chain(&d, 1),
            Err("base `MISSING` is not in this file".to_owned())
        );
        let d = read_definition(&def(&[
            &rom("A", "2000", "A").replace("32BITBASE", "B"),
            &rom("B", "2000", "B").replace("32BITBASE", "A"),
        ]))
        .unwrap();
        assert!(base_chain(&d, 1).is_err_and(|e| e.contains("loops")));
        let d = read_definition(
            b"<roms><rom><romid><internalidstring>X</internalidstring></romid></rom><rom><romid><xmlid>BASE</xmlid></romid></rom><rom base=\"BASE\"><romid><xmlid>C</xmlid></romid></rom></roms>",
        )
        .unwrap();
        assert!(
            base_chain(&d, 2).is_err_and(|e| e.contains("without <xmlid>")),
            "legacy NPE"
        );
    }

    #[test]
    fn engine_classes_of_liberty_trims() {
        assert_eq!(engine_class("GT-AT"), EngineClass::Turbo);
        assert_eq!(engine_class("GT"), EngineClass::Turbo);
        assert_eq!(engine_class("B4"), EngineClass::Turbo);
        assert_eq!(engine_class("3.0R-MT"), EngineClass::NaH6);
        assert_eq!(engine_class("2.5i"), EngineClass::Na4cyl);
        assert_eq!(engine_class("2.0i-AT"), EngineClass::Na4cyl);
        assert_eq!(engine_class("2.0-GT-Auto"), EngineClass::Turbo);
        assert_eq!(engine_class(""), EngineClass::Unknown);
        assert_eq!(engine_class("Premium"), EngineClass::Unknown);
    }

    #[test]
    fn probe_prefers_the_file_name_id() {
        let mut img = image(0x3000, 0x200, b"ABCD1234");
        img[0x2000..0x2008].copy_from_slice(b"A2WC400H");
        let addrs = [0x200, 0x2000, 0x2004];
        assert_eq!(
            probe_id(&img, &addrs, Some("a2wc400h")),
            Some((0x2000, "A2WC400H".to_owned()))
        );
        assert_eq!(
            probe_id(&img, &addrs, None),
            Some((0x200, "ABCD1234".to_owned()))
        );
        assert_eq!(
            probe_id(&image(0x3000, 0x2000, b"ABCDEFGH"), &addrs, None),
            None,
            "no digit"
        );
        assert_eq!(
            probe_id(&image(0x3000, 0x2000, b"12345678"), &addrs, None),
            None,
            "no letter"
        );
        assert_eq!(probe_id(&[], &addrs, None), None);
    }

    /// Match, no match (probed), suspect container, suspect without an
    /// embedded image, unresolved base, Liberty flags and the totals.
    #[test]
    fn cover_classifies_every_image() {
        let defs = vec![
            doc("ecu/a.xml", &def(&[&rom("A2WC400H", "2000", "A2WC400H"), &rom("A2WD610H", "2000", "D2WD610H")])),
            doc(
                "ecu/b.xml",
                &def(&[&rom("A2WC400H_v2", "2000", "a2wc400h"), &rom("BROKEN", "2000", "BRKN0001").replace("32BITBASE", "NOWHERE")]),
            ),
            doc("ecu/c.xml", b"<roms><rom>"),
            doc(
                "ecu/lib.xml",
                b"<roms><rom><romid><xmlid>E2XX000Z</xmlid><market>ADM</market><model>Liberty</model><year>04</year><submodel>2.5i</submodel></romid></rom></roms>",
            ),
        ];
        let mut matched = rom_file("ADM/Forester/A2WC400H.hex", Some("A2WC400H"));
        matched.region = Some("ADM".to_owned());
        let mut lib = rom_file(
            "ADM/Legacy/D2WD610H-2004-ADM-Subaru-Liberty-3.0R-MT.hex",
            Some("D2WD610H"),
        );
        (lib.region, lib.model, lib.year, lib.trim) = (
            Some("ADM".to_owned()),
            Some("Liberty".to_owned()),
            Some(2004),
            Some("3.0R-MT".to_owned()),
        );
        let mut lib_na4 = rom_file(
            "ADM/Legacy/E2XX000Z-2003-04-ADM-Liberty-2.5i.hex",
            Some("E2XX000Z"),
        );
        (
            lib_na4.region,
            lib_na4.model,
            lib_na4.year,
            lib_na4.year_end,
            lib_na4.trim,
        ) = (
            Some("ADM".to_owned()),
            Some("Liberty".to_owned()),
            Some(2003),
            Some(2004),
            Some("2.5i".to_owned()),
        );
        let mut container = rom_file("JDM/Legacy/A2WC400H-container.hex", Some("A2WC400H"));
        container.suspect = Some(Suspect {
            kind: SuspectKind::Container,
            reason: "synthetic".to_owned(),
            image_offset: Some(16),
            image_size: Some(0x3000),
        });
        let mut blank = rom_file("USDM/X/blank.hex", None);
        blank.suspect = Some(Suspect {
            kind: SuspectKind::Blank,
            reason: "synthetic".to_owned(),
            image_offset: None,
            image_size: None,
        });
        let unmatched = rom_file("USDM/X/E2ZZ999Z.hex", Some("E2ZZ999Z"));
        let broken = rom_file("USDM/X/BRKN0001.hex", Some("BRKN0001"));
        let roms = vec![matched, lib, lib_na4, container, blank, unmatched, broken];
        let c = cover("r", "d", &roms, &defs, |f| {
            Ok(match f.path.as_str() {
                "ADM/Forester/A2WC400H.hex" => image(0x3000, 0x2000, b"A2WC400H"),
                p if p.contains("D2WD610H") => image(0x3000, 0x2000, b"D2WD610H"),
                p if p.contains("container") => {
                    [vec![0u8; 16], image(0x3000, 0x2000, b"A2WC400H")].concat()
                }
                p if p.contains("blank") => vec![0xFF; 0x3000],
                p if p.contains("E2ZZ999Z") => image(0x3000, 0x2000, b"E2ZZ999Y"),
                p if p.contains("E2XX000Z") => image(0x3000, 0x2000, b"E2XX000Z"),
                _ => image(0x3000, 0x2000, b"BRKN0001"),
            })
        })
        .unwrap();
        let by = |p: &str| c.roms.iter().find(|r| r.path.contains(p)).unwrap();

        let m = by("Forester/A2WC400H");
        assert_eq!(m.status, Status::Matched);
        assert_eq!(
            m.definitions,
            vec![
                DefinitionMatch {
                    file: "ecu/a.xml".to_owned(),
                    xmlid: Some("A2WC400H".to_owned()),
                    base_error: None
                },
                DefinitionMatch {
                    file: "ecu/b.xml".to_owned(),
                    xmlid: Some("A2WC400H_v2".to_owned()),
                    base_error: None
                },
            ]
        );
        assert_eq!(
            (
                m.image_rom_id.as_deref(),
                m.image_rom_id_address.as_deref(),
                m.image_rom_id_from
            ),
            (Some("A2WC400H"), Some("0x2000"), Some(IdSource::Definition))
        );

        let s = by("container");
        assert_eq!(
            (s.status, s.suspect, s.image_offset),
            (Status::Matched, Some(SuspectKind::Container), Some(16))
        );
        let b = by("blank");
        assert_eq!(
            (
                b.status,
                b.suspect,
                b.image_offset,
                b.image_rom_id.as_deref()
            ),
            (Status::NoDefinition, Some(SuspectKind::Blank), None, None)
        );

        let n = by("E2ZZ999Z");
        assert_eq!(n.status, Status::NoDefinition);
        assert!(n.definitions.is_empty());
        assert_eq!(
            (n.image_rom_id.as_deref(), n.image_rom_id_from),
            (Some("E2ZZ999Y"), Some(IdSource::Probe)),
            "the ID read from the image, not the file name"
        );

        let u = by("BRKN0001");
        assert_eq!(u.status, Status::UnresolvedBase);
        assert_eq!(
            u.definitions[0].base_error.as_deref(),
            Some("base `NOWHERE` is not in this file")
        );

        assert_eq!(by("D2WD610H").liberty_2004_adm, Some(EngineClass::NaH6));
        assert_eq!(by("E2XX000Z").liberty_2004_adm, Some(EngineClass::Na4cyl));
        assert_eq!(by("Forester").liberty_2004_adm, None);
        let l = &c.liberty_2004_adm;
        assert_eq!((l.images, l.matched), (2, 1));
        assert_eq!(l.na_h6, vec!["D2WD610H"]);
        assert_eq!(l.na_4cyl, vec!["E2XX000Z"]);
        assert_eq!(l.no_definition, vec!["E2XX000Z"]);
        assert_eq!(l.definitions, vec!["E2XX000Z"]);
        assert_eq!(l.definitions_na_4cyl, vec!["E2XX000Z"]);

        let t = &c.summary;
        assert_eq!(
            (
                t.images,
                t.matched,
                t.unresolved_base,
                t.no_definition,
                t.suspect
            ),
            (7, 3, 1, 3, 2)
        );
        // By file-name ID: A2WC400H (twice) and E2XX000Z; not D2WD610H nor
        // BRKN0001 (their xmlids are A2WD610H and BROKEN), nor E2ZZ999Z.
        assert_eq!(t.filename_id_has_definition, 3);
        assert_eq!(
            t.image_id_differs_from_filename,
            vec!["USDM/X/E2ZZ999Z.hex"]
        );
        assert_eq!(t.regions.len(), 2);
        assert_eq!(
            (
                t.regions[1].region.as_deref(),
                t.regions[1].images,
                t.regions[1].matched
            ),
            (Some("ADM"), 3, 2)
        );

        let files: BTreeMap<_, _> = c
            .definition_files
            .iter()
            .map(|f| (f.path.as_str(), f))
            .collect();
        assert_eq!(
            (
                files["ecu/a.xml"].roms,
                files["ecu/a.xml"].with_id,
                files["ecu/a.xml"].matched_images
            ),
            (3, 2, 3),
            "the base plus two IDs; matches A2WC400H, its container and D2WD610H"
        );
        assert_eq!(
            files["ecu/b.xml"].problems,
            vec!["BROKEN: base `NOWHERE` is not in this file"]
        );
        assert_eq!(files["ecu/c.xml"].root, None);
        assert!(files["ecu/c.xml"].problems[0].starts_with("does not parse"));

        // Round trip, and nothing but metadata in the rendering.
        let text = c.render().unwrap();
        assert_eq!(Coverage::from_toml(&text).unwrap(), c);
        assert!(!text.contains("storageaddress") && !text.contains("<table"));
    }

    fn generated(c: &Coverage) -> Generated {
        Generated {
            coverage: c.clone(),
            text: c.render().unwrap(),
        }
    }

    #[test]
    fn check_names_what_is_stale() {
        let defs = vec![doc(
            "ecu/a.xml",
            &def(&[&rom("A2WC400H", "2000", "A2WC400H")]),
        )];
        let roms = vec![
            rom_file("ADM/A2WC400H.hex", Some("A2WC400H")),
            rom_file("ADM/E2ZZ999Z.hex", Some("E2ZZ999Z")),
        ];
        let img = |f: &RomFile| {
            Ok(image(
                0x3000,
                0x2000,
                if f.path.contains("A2WC") {
                    b"A2WC400H"
                } else {
                    b"E2ZZ999Z"
                },
            ))
        };
        let c = cover("r", "d", &roms, &defs, img).unwrap();
        let g = generated(&c);
        assert!(compare_committed(&g, &g.text).is_empty());

        // A definition was added upstream: the committed report is stale.
        let defs2 = vec![doc(
            "ecu/a.xml",
            &def(&[
                &rom("A2WC400H", "2000", "A2WC400H"),
                &rom("E2ZZ999Z", "2000", "E2ZZ999Z"),
            ]),
        )];
        let fresh = generated(&cover("r", "d", &roms, &defs2, img).unwrap());
        let problems = compare_committed(&fresh, &g.text);
        assert!(
            problems
                .iter()
                .any(|p| p.starts_with("ADM/E2ZZ999Z.hex: committed NoDefinition")),
            "{problems:?}"
        );
        assert!(
            problems.iter().any(|p| p.starts_with("[summary] differs")),
            "{problems:?}"
        );
        assert!(
            problems
                .iter()
                .any(|p| p.starts_with("definition file ecu/a.xml")),
            "{problems:?}"
        );

        // A new pin, an image dropped, a hand edit, bad TOML, CRLF.
        let repinned = generated(&cover("r2", "d", &roms, &defs, img).unwrap());
        assert!(
            compare_committed(&repinned, &g.text)
                .iter()
                .any(|p| p.starts_with("roms_commit"))
        );
        let fewer = generated(&cover("r", "d", &roms[..1], &defs, img).unwrap());
        assert!(
            compare_committed(&fewer, &g.text)
                .iter()
                .any(|p| p.contains("not in the manifest"))
        );
        let edited = g.text.replace("schema = 1\n", "schema = 1\n\n");
        assert!(compare_committed(&g, &edited)[0].contains("not the canonical rendering"));
        assert!(compare_committed(&g, "schema = [")[0].contains("does not parse"));
        assert!(compare_committed(&g, &g.text.replace('\n', "\r\n"))[0].contains("\\r\\n"));
        assert!(Coverage::from_toml(&g.text.replace("schema = 1", "schema = 2")).is_err());
        assert!(
            Coverage::from_toml(
                &g.text
                    .replace("schema = 1", "schema = 1\nformula = \"x*2\"")
            )
            .is_err(),
            "unknown fields are refused"
        );
    }
}
