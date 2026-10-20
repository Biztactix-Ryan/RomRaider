//! Every formula in the pinned definition corpus (US-RR-20-10, D-028).
//!
//! RomRaider definitions carry their formulas as XML attributes. This module
//! reads them out of the fetched definition tree (`RR_CORPUS_DIR`'s
//! `defs/romraider/`, pinned by `[defs]` in `fixtures/corpus-manifest.toml`)
//! so that tests can parse every one of them with `rr_core::expr` and
//! evaluate the hand-checked ones (`fixtures/kat/expr-corpus.toml`).
//!
//! # What is harvested
//!
//! Exactly the attributes legacy RomRaider hands to its formula evaluator,
//! read the way legacy reads them:
//!
//! | File (root element) | Element | Attribute | [`FormulaKind`] | Legacy |
//! |---|---|---|---|---|
//! | ECU definition (`<roms>`, or a bare `<rom>`) | `scaling`, any case | `expression`, any case | [`FormulaKind::EcuExpression`] (raw to real) | `xml/TableScaleUnmarshaller.java:273` (`equalsIgnoreCase("scaling")`), `:451` |
//! | ECU definition | `scaling`, any case | `to_byte` | [`FormulaKind::EcuToByte`] (real to raw) | `xml/TableScaleUnmarshaller.java:442` |
//! | logger definition (`<logger>`) | `conversion` | `expr` | [`FormulaKind::LoggerExpr`]: an `x` conversion, or a derived parameter's formula over `[P8:rpm]`-style references | `logger/ecu/definition/xml/LoggerDefinitionHandler.java:72`, `:92`, `:247-249` |
//!
//! Every `.xml` file (any case) the manifest's `[[defs.files]]` lists is
//! read; a file with any other root element (`dyno/cars_def.xml`) has no
//! formulas. An attribute value is taken after XML attribute-value
//! normalisation (entities expanded, as a DOM or SAX parser gives it to
//! legacy), byte for byte otherwise: two formulas are the same formula only
//! when their text is identical. An empty value is not a formula (legacy
//! refuses one: `checkNotNullOrEmpty` in `EcuParameterConvertorImpl.java:59`;
//! an empty `to_byte` is legacy's "none"); [`Harvest::empty`] counts them, so
//! none is dropped unseen. An element whose name only resembles `scaling` (at
//! the pin, four `<scalin>` typos) is not read by legacy and not harvested.
//!
//! Each file is checked against the manifest's size and SHA-256 before it is
//! read, so a harvest is always of the pinned bytes. The forum logger package
//! `logger_v370.zip` (D-028) is never harvested here: the public tier uses the
//! v290 logger files in the pinned tree ([`logger_v370_status`] says whether
//! a private copy is present).
//!
//! # Licence rule (D-028)
//!
//! The definitions have no licence, so formula *text* never leaves the
//! fetched corpus: the committed `fixtures/corpus-formulas.toml`
//! ([`Harvest::render_metadata`]) holds, per distinct formula, its SHA-256
//! (of the UTF-8 text: `printf '%s' "$formula" | sha256sum`), its occurrence
//! count, its kinds and where it first occurs, as a corpus-relative path and
//! a positional XPath ([`Occurrence::xpath`], element names and indices only,
//! no names or units from the file). Known-answer cases name a corpus formula
//! by that hash ([`crate::kat::Formula::Ref`]).

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};

use super::{Corpus, CorpusError, plain_relative};

/// Where the definition tree sits in the corpus directory (xtask's
/// `fetch_corpus::defs_dir`).
pub const DEFS_ROOT: &str = "defs/romraider";

/// Where the harvest metadata is committed, relative to the repository root.
pub const METADATA_PATH: &str = "fixtures/corpus-formulas.toml";

/// Format version of the harvest metadata.
pub const METADATA_SCHEMA: u32 = 1;

/// Which attribute a formula came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FormulaKind {
    /// An ECU `<scaling expression="...">`: raw value `x` to real value.
    EcuExpression,
    /// An ECU `<scaling to_byte="...">`: real value `x` back to raw.
    EcuToByte,
    /// A logger `<conversion expr="...">`.
    LoggerExpr,
}

impl FormulaKind {
    /// The name used in the metadata file.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EcuExpression => "ecu-expression",
            Self::EcuToByte => "ecu-to_byte",
            Self::LoggerExpr => "logger-expr",
        }
    }
}

impl fmt::Display for FormulaKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where one formula attribute is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Occurrence {
    /// The file, relative to the corpus directory, `/`-separated:
    /// `defs/romraider/ecu/standard/ecu_defs.xml`.
    pub path: String,
    /// The attribute as a positional XPath 1.0 location: every step is the
    /// element name with its 1-based index among same-named siblings, then
    /// the attribute, `/roms[1]/rom[2]/table[17]/scaling[1]/@expression`
    /// (`xmllint --xpath` finds it).
    pub xpath: String,
    /// The attribute's kind.
    pub kind: FormulaKind,
}

/// One distinct formula and every place it occurs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Formula {
    /// The formula text, exactly as the definition gives it (after XML
    /// attribute-value normalisation). Never commit it (D-028).
    pub text: String,
    /// Lowercase hex SHA-256 of `text`'s UTF-8 bytes.
    pub sha256: String,
    /// Every occurrence, in harvest order (manifest path order, then
    /// document order); never empty.
    pub occurrences: Vec<Occurrence>,
}

impl Formula {
    /// The kinds it occurs as, sorted.
    #[must_use]
    pub fn kinds(&self) -> BTreeSet<FormulaKind> {
        self.occurrences.iter().map(|o| o.kind).collect()
    }

    /// Where it first occurs.
    #[must_use]
    pub fn first(&self) -> &Occurrence {
        // `occurrences` is never empty: a Formula is only made with one.
        &self.occurrences[0]
    }

    /// Whether it occurs at `path` / `xpath`.
    #[must_use]
    pub fn occurs_at(&self, path: &str, xpath: &str) -> bool {
        self.occurrences
            .iter()
            .any(|o| o.path == path && o.xpath == xpath)
    }
}

/// The formulas of one definition file, as [`harvest_xml`] reads them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileFormulas {
    /// The root element's name.
    pub root: String,
    /// Each formula attribute, in document order: its text and where it is.
    pub formulas: Vec<(String, Occurrence)>,
    /// Formula attributes whose value is empty (not formulas).
    pub empty: usize,
}

/// Every distinct formula of the pinned definition corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Harvest {
    /// The pinned upstream commit of the definition tree (`[defs] commit`).
    pub defs_commit: String,
    /// XML files read, by root element (`roms`, `rom`, `logger`, `dyno`).
    pub files_by_root: BTreeMap<String, usize>,
    /// Formula attributes with an empty value (module docs).
    pub empty: usize,
    /// The distinct formulas, sorted by SHA-256.
    pub formulas: Vec<Formula>,
}

impl Harvest {
    /// XML files read.
    #[must_use]
    pub fn files(&self) -> usize {
        self.files_by_root.values().sum()
    }

    /// Formula attributes read (every occurrence of every formula).
    #[must_use]
    pub fn occurrences(&self) -> usize {
        self.formulas.iter().map(|f| f.occurrences.len()).sum()
    }

    /// Distinct formulas that occur at least once as `kind`.
    #[must_use]
    pub fn distinct_of(&self, kind: FormulaKind) -> usize {
        self.formulas
            .iter()
            .filter(|f| f.occurrences.iter().any(|o| o.kind == kind))
            .count()
    }

    /// The formula whose text hashes to `sha256` (lowercase hex).
    #[must_use]
    pub fn by_sha256(&self, sha256: &str) -> Option<&Formula> {
        self.formulas
            .binary_search_by(|f| f.sha256.as_str().cmp(sha256))
            .ok()
            .map(|i| &self.formulas[i])
    }

    /// Builds a harvest from per-file results, in the order given.
    #[must_use]
    pub fn from_files(defs_commit: &str, files: Vec<FileFormulas>) -> Self {
        let mut files_by_root = BTreeMap::new();
        let mut empty = 0;
        let mut by_text: BTreeMap<String, Vec<Occurrence>> = BTreeMap::new();
        for file in files {
            *files_by_root.entry(file.root).or_insert(0) += 1;
            empty += file.empty;
            for (text, occurrence) in file.formulas {
                by_text.entry(text).or_default().push(occurrence);
            }
        }
        let mut formulas: Vec<Formula> = by_text
            .into_iter()
            .map(|(text, occurrences)| Formula {
                sha256: sha256_hex(text.as_bytes()),
                text,
                occurrences,
            })
            .collect();
        formulas.sort_by(|a, b| a.sha256.cmp(&b.sha256));
        Self {
            defs_commit: defs_commit.to_owned(),
            files_by_root,
            empty,
            formulas,
        }
    }

    /// The committed metadata file (`fixtures/corpus-formulas.toml`): counts,
    /// then one `[[formulas]]` entry per distinct formula, sorted by SHA-256.
    /// Metadata only (D-028): no formula text. Deterministic, `\n` endings.
    #[must_use]
    pub fn render_metadata(&self) -> String {
        let mut out = String::from(METADATA_HEADER);
        let q = |s: &str| toml::Value::String(s.to_owned()).to_string();
        // Writing to a String cannot fail.
        let _ = writeln!(out, "\nschema = {METADATA_SCHEMA}");
        let _ = writeln!(out, "defs_commit = {}", q(&self.defs_commit));
        let _ = writeln!(out, "\n[summary]");
        let _ = writeln!(out, "files = {}", self.files());
        for (root, n) in &self.files_by_root {
            let _ = writeln!(out, "files_root_{root} = {n}");
        }
        let _ = writeln!(out, "occurrences = {}", self.occurrences());
        let _ = writeln!(out, "empty = {}", self.empty);
        let _ = writeln!(out, "distinct = {}", self.formulas.len());
        for kind in [
            FormulaKind::EcuExpression,
            FormulaKind::EcuToByte,
            FormulaKind::LoggerExpr,
        ] {
            let key = kind.as_str().replace('-', "_");
            let _ = writeln!(out, "distinct_{key} = {}", self.distinct_of(kind));
        }
        for f in &self.formulas {
            let kinds: Vec<String> = f.kinds().iter().map(|k| q(k.as_str())).collect();
            let first = f.first();
            let _ = writeln!(out, "\n[[formulas]]");
            let _ = writeln!(out, "sha256 = {}", q(&f.sha256));
            let _ = writeln!(out, "occurrences = {}", f.occurrences.len());
            let _ = writeln!(out, "kinds = [{}]", kinds.join(", "));
            let _ = writeln!(
                out,
                "first = {{ path = {}, xpath = {} }}",
                q(&first.path),
                q(&first.xpath)
            );
        }
        out
    }
}

const METADATA_HEADER: &str = "\
# Formulas of the pinned definition corpus (US-RR-20-10, D-028).
# GENERATED: do not edit.
#   regenerate: RR_CORPUS_DIR=<dir> cargo run -p rr-testkit --example gen-corpus-formulas
#   check:      RR_CORPUS_DIR=<dir> cargo run -p rr-testkit --example gen-corpus-formulas -- --check
#               (and the corpus-tier test corpus_formula_metadata_is_current, CI `corpus` job)
# Every <scaling expression|to_byte> of the ECU definitions and every
# <conversion expr> of the logger definitions listed in the manifest's
# [defs], deduplicated by exact text. Metadata only (D-028): per distinct
# formula its SHA-256 (of the UTF-8 text), occurrence count, kinds and first
# location (corpus-relative path, positional XPath); never the formula text.
# The v370 forum logger package is not harvested (public tier: v290 only).
# Format: crates/rr-testkit/src/corpus/formulas.rs.
";

/// Errors from harvesting.
#[derive(Debug, thiserror::Error)]
pub enum FormulaError {
    /// The corpus could not be opened.
    #[error(transparent)]
    Corpus(#[from] CorpusError),
    /// A file could not be read.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The manifest has no usable `[defs]` table.
    #[error("{}: [defs]: {reason}", path.display())]
    Manifest {
        /// The manifest.
        path: PathBuf,
        /// What is wrong.
        reason: String,
    },
    /// A definition file is not the pinned one.
    #[error(
        "{path}: {what} differs from the manifest (want {want}, got {got}); run `cargo xtask fetch-corpus`"
    )]
    NotPinned {
        /// The file, corpus-relative.
        path: String,
        /// `size` or `sha256`.
        what: &'static str,
        /// The manifest's value.
        want: String,
        /// The file's.
        got: String,
    },
    /// A definition file is not well-formed XML, or an attribute does not
    /// decode.
    #[error("{path}: {message}")]
    Xml {
        /// The file, corpus-relative.
        path: String,
        /// What went wrong, and where.
        message: String,
    },
}

/// Lowercase hex SHA-256 of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// What a file's root element makes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Root {
    Ecu,
    Logger,
    Other,
}

impl Root {
    fn of(name: &str) -> Self {
        if name.eq_ignore_ascii_case("roms") || name.eq_ignore_ascii_case("rom") {
            Self::Ecu
        } else if name == "logger" {
            Self::Logger
        } else {
            Self::Other
        }
    }

    /// The kind of attribute `key` on element `element`, if it is a formula.
    fn kind(self, element: &str, key: &str) -> Option<FormulaKind> {
        match self {
            Self::Ecu if element.eq_ignore_ascii_case("scaling") => {
                if key.eq_ignore_ascii_case("expression") {
                    Some(FormulaKind::EcuExpression)
                } else if key == "to_byte" {
                    Some(FormulaKind::EcuToByte)
                } else {
                    None
                }
            }
            Self::Logger if element == "conversion" && key == "expr" => {
                Some(FormulaKind::LoggerExpr)
            }
            Self::Ecu | Self::Logger | Self::Other => None,
        }
    }
}

/// One open element: its XPath and how many children of each name it has
/// had so far.
struct Frame {
    xpath: String,
    children: BTreeMap<String, usize>,
}

/// Reads the formula attributes of one definition file (module docs). `path`
/// is the corpus-relative path recorded in each [`Occurrence`].
///
/// # Errors
///
/// [`FormulaError::Xml`]: the XML is not well-formed (mismatched or unclosed
/// tags), an element or attribute name is not UTF-8, or a formula attribute
/// does not decode.
pub fn harvest_xml(path: &str, xml: &[u8]) -> Result<FileFormulas, FormulaError> {
    let err = |message: String| FormulaError::Xml {
        path: path.to_owned(),
        message,
    };
    let mut reader = Reader::from_reader(xml);
    let mut out = FileFormulas::default();
    let mut root = Root::Other;
    // The document node, then one frame per open element.
    let mut stack = vec![Frame {
        xpath: String::new(),
        children: BTreeMap::new(),
    }];
    loop {
        let position = reader.buffer_position();
        let event = reader
            .read_event()
            .map_err(|e| err(format!("XML error near byte {position}: {e}")))?;
        match event {
            Event::Start(e) => {
                let frame = open(&mut stack, &e, path, &mut root, &mut out)?;
                stack.push(frame);
            }
            Event::Empty(e) => {
                open(&mut stack, &e, path, &mut root, &mut out)?;
            }
            Event::End(_) => {
                if stack.len() <= 1 {
                    return Err(err(format!("unmatched end tag near byte {position}")));
                }
                stack.pop();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if stack.len() != 1 || out.root.is_empty() {
        return Err(err(
            "XML ends inside an element, or has no root element".to_owned()
        ));
    }
    Ok(out)
}

/// An element has opened: harvest its formula attributes and return its
/// frame.
fn open(
    stack: &mut [Frame],
    e: &BytesStart<'_>,
    path: &str,
    root: &mut Root,
    out: &mut FileFormulas,
) -> Result<Frame, FormulaError> {
    let err = |message: String| FormulaError::Xml {
        path: path.to_owned(),
        message,
    };
    let name = std::str::from_utf8(e.name().as_ref())
        .map_err(|_| err("an element name is not UTF-8".to_owned()))?
        .to_owned();
    let parent = stack
        .last_mut()
        .ok_or_else(|| err("internal: no parent frame".to_owned()))?;
    if parent.xpath.is_empty() {
        if !out.root.is_empty() {
            return Err(err(format!("a second root element <{name}>")));
        }
        out.root.clone_from(&name);
        *root = Root::of(&name);
    }
    let index = parent.children.entry(name.clone()).or_insert(0);
    *index += 1;
    let xpath = format!("{}/{name}[{index}]", parent.xpath);
    for attr in e.attributes() {
        let attr = attr.map_err(|e| err(format!("{xpath}: attribute: {e}")))?;
        let key = std::str::from_utf8(attr.key.as_ref())
            .map_err(|_| err(format!("{xpath}: an attribute name is not UTF-8")))?;
        let Some(kind) = root.kind(&name, key) else {
            continue;
        };
        let value: Cow<'_, str> = attr
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| err(format!("{xpath}/@{key}: {e}")))?;
        if value.is_empty() {
            out.empty += 1;
            continue;
        }
        out.formulas.push((
            value.into_owned(),
            Occurrence {
                path: path.to_owned(),
                xpath: format!("{xpath}/@{key}"),
                kind,
            },
        ));
    }
    Ok(Frame {
        xpath,
        children: BTreeMap::new(),
    })
}

/// The part of the manifest the harvest needs.
#[derive(Deserialize)]
struct ManifestDefs {
    defs: Option<DefsTable>,
}

#[derive(Deserialize)]
struct DefsTable {
    commit: String,
    files: Vec<DefFileEntry>,
}

#[derive(Deserialize)]
struct DefFileEntry {
    path: String,
    size: u64,
    sha256: String,
}

/// Harvests the corpus at `RR_CORPUS_DIR` (else the default directory) with
/// the committed manifest ([`Corpus::open`]: refuses a directory inside the
/// repository).
///
/// # Errors
///
/// As [`Corpus::open`] and [`harvest_dir`].
pub fn harvest() -> Result<Harvest, FormulaError> {
    let corpus = Corpus::open()?;
    harvest_dir(corpus.dir(), corpus.manifest().path())
}

/// Harvests the definition tree in corpus directory `corpus_dir`, as pinned
/// by the manifest at `manifest`: every `.xml` file (any case) of its
/// `[[defs.files]]`, each checked against its size and SHA-256 first, in
/// path order.
///
/// # Errors
///
/// [`FormulaError::Io`], [`FormulaError::Manifest`],
/// [`FormulaError::NotPinned`] or [`FormulaError::Xml`].
pub fn harvest_dir(corpus_dir: &Path, manifest: &Path) -> Result<Harvest, FormulaError> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| FormulaError::Io { path, source }
    };
    let bad = |reason: String| FormulaError::Manifest {
        path: manifest.to_path_buf(),
        reason,
    };
    let text = std::fs::read_to_string(manifest).map_err(io(manifest))?;
    let parsed: ManifestDefs = toml::from_str(&text).map_err(|e| bad(e.to_string()))?;
    let defs = parsed
        .defs
        .ok_or_else(|| bad("missing: the manifest pins no definition corpus".to_owned()))?;
    let mut entries: Vec<&DefFileEntry> = defs
        .files
        .iter()
        .filter(|f| {
            Path::new(&f.path)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("xml"))
        })
        .collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let mut files = Vec::with_capacity(entries.len());
    for entry in entries {
        if !plain_relative(&entry.path) {
            return Err(bad(format!(
                "{:?} is not a plain relative `/`-separated path",
                entry.path
            )));
        }
        let rel = format!("{DEFS_ROOT}/{}", entry.path);
        let full = corpus_dir.join(&rel);
        let bytes = std::fs::read(&full).map_err(io(&full))?;
        if bytes.len() as u64 != entry.size {
            return Err(FormulaError::NotPinned {
                path: rel,
                what: "size",
                want: entry.size.to_string(),
                got: bytes.len().to_string(),
            });
        }
        let got = sha256_hex(&bytes);
        if !got.eq_ignore_ascii_case(&entry.sha256) {
            return Err(FormulaError::NotPinned {
                path: rel,
                what: "sha256",
                want: entry.sha256.clone(),
                got,
            });
        }
        files.push(harvest_xml(&rel, &bytes)?);
    }
    Ok(Harvest::from_files(&defs.commit, files))
}

/// Whether the private v370 logger package (D-028) is present for corpus
/// directory `corpus_dir`, as a sentence for test output. It is never
/// harvested here; the public tier's logger formulas are the pinned tree's
/// v290 files.
#[must_use]
pub fn logger_v370_status(corpus_dir: &Path) -> String {
    let supplied = std::env::var_os("RR_LOGGER_ZIP").filter(|v| !v.is_empty());
    let path = supplied.map_or_else(
        || corpus_dir.join("defs").join("logger_v370.zip"),
        PathBuf::from,
    );
    if path.is_file() {
        format!(
            "logger v370: present at {} but not harvested (reading the zip is a follow-up); \
             logger formulas are the pinned v290 files",
            path.display()
        )
    } else {
        format!(
            "logger v370: not present (no {}; it is never downloaded, D-028): skipped; \
             logger formulas are the pinned v290 files",
            path.display()
        )
    }
}

/// The committed metadata, read back: the repository's
/// [`METADATA_PATH`].
///
/// # Errors
///
/// [`FormulaError::Io`].
pub fn committed_metadata() -> Result<String, FormulaError> {
    let path = crate::fixtures::repo_root().join(METADATA_PATH);
    std::fs::read_to_string(&path).map_err(|source| FormulaError::Io { path, source })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(file: &FileFormulas) -> Vec<(&str, &str, FormulaKind)> {
        file.formulas
            .iter()
            .map(|(t, o)| (t.as_str(), o.xpath.as_str(), o.kind))
            .collect()
    }

    #[test]
    fn ecu_scalings_any_case_with_positional_xpaths() {
        let xml = br#"<?xml version="1.0"?>
<roms>
  <rom>
    <romid><xmlid>A</xmlid></romid>
    <table name="t1"><scaling units="u" expression="x*2" to_byte="x/2"/></table>
    <table name="t2">
      <table name="y"><scaling expression="x+1" to_byte=""/></table>
      <Scaling EXPRESSION="x&lt;3" to_byte="x" />
    </table>
    <scalin expression="ignored" />
  </rom>
  <rom><scaling name="s" expression="x*2" To_Byte="ignored"/></rom>
</roms>"#;
        let got = harvest_xml("defs/romraider/ecu/a.xml", xml).unwrap();
        assert_eq!(got.root, "roms");
        assert_eq!(got.empty, 1);
        assert_eq!(
            texts(&got),
            [
                (
                    "x*2",
                    "/roms[1]/rom[1]/table[1]/scaling[1]/@expression",
                    FormulaKind::EcuExpression
                ),
                (
                    "x/2",
                    "/roms[1]/rom[1]/table[1]/scaling[1]/@to_byte",
                    FormulaKind::EcuToByte
                ),
                (
                    "x+1",
                    "/roms[1]/rom[1]/table[2]/table[1]/scaling[1]/@expression",
                    FormulaKind::EcuExpression
                ),
                (
                    "x<3",
                    "/roms[1]/rom[1]/table[2]/Scaling[1]/@EXPRESSION",
                    FormulaKind::EcuExpression
                ),
                (
                    "x",
                    "/roms[1]/rom[1]/table[2]/Scaling[1]/@to_byte",
                    FormulaKind::EcuToByte
                ),
                (
                    "x*2",
                    "/roms[1]/rom[2]/scaling[1]/@expression",
                    FormulaKind::EcuExpression
                ),
            ]
        );
    }

    #[test]
    fn logger_conversions_exact_names_only() {
        let xml = "\u{feff}<logger><protocols><protocol>\
<parameters><parameter id=\"P8\"><conversions>\
<conversion units=\"rpm\" expr=\"x/4\"/><Conversion expr=\"no\"/><conversion EXPR=\"no\"/>\
</conversions></parameter>\
<parameter id=\"P90\"><conversions><conversion expr=\"[P8:rpm]*2\"/>\
<conversion expr=\"[P229:\u{b5}s]&gt;1\"/></conversions></parameter>\
</parameters></protocol></protocols>\
<scaling expression=\"not a logger formula\"/></logger>";
        let got = harvest_xml("l.xml", xml.as_bytes()).unwrap();
        assert_eq!(got.root, "logger");
        let p = "/logger[1]/protocols[1]/protocol[1]/parameters[1]";
        assert_eq!(
            texts(&got),
            [
                (
                    "x/4",
                    &*format!("{p}/parameter[1]/conversions[1]/conversion[1]/@expr"),
                    FormulaKind::LoggerExpr
                ),
                (
                    "[P8:rpm]*2",
                    &*format!("{p}/parameter[2]/conversions[1]/conversion[1]/@expr"),
                    FormulaKind::LoggerExpr
                ),
                (
                    "[P229:\u{b5}s]>1",
                    &*format!("{p}/parameter[2]/conversions[1]/conversion[2]/@expr"),
                    FormulaKind::LoggerExpr
                ),
            ]
        );
    }

    #[test]
    fn other_roots_have_no_formulas() {
        let got = harvest_xml("d.xml", br#"<dyno><scaling expression="x"/></dyno>"#).unwrap();
        assert_eq!((got.root.as_str(), got.formulas.len()), ("dyno", 0));
    }

    #[test]
    fn malformed_xml_is_an_error() {
        for bad in [
            &b"<roms><rom></roms>"[..],
            b"<roms>",
            b"",
            b"<roms/><roms/>",
            b"<roms><scaling expression=\"&bogus;\"/></roms>",
        ] {
            assert!(
                matches!(harvest_xml("x.xml", bad), Err(FormulaError::Xml { .. })),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
    }

    #[test]
    fn harvest_dedups_by_exact_text_and_sorts_by_hash() {
        let a = harvest_xml(
            "a.xml",
            br#"<roms><rom><scaling expression="x*2" to_byte="x/2"/></rom></roms>"#,
        )
        .unwrap();
        let b = harvest_xml(
            "b.xml",
            br#"<logger><conversion expr="x*2"/><conversion expr="x * 2"/></logger>"#,
        )
        .unwrap();
        let h = Harvest::from_files("c0ffee", vec![a, b]);
        assert_eq!(h.files(), 2);
        assert_eq!(h.occurrences(), 4);
        assert_eq!(h.formulas.len(), 3);
        assert!(h.formulas.windows(2).all(|w| w[0].sha256 < w[1].sha256));
        let twice = h.by_sha256(&sha256_hex(b"x*2")).unwrap();
        assert_eq!(twice.text, "x*2");
        assert_eq!(twice.occurrences.len(), 2);
        assert_eq!(
            twice.kinds().into_iter().collect::<Vec<_>>(),
            [FormulaKind::EcuExpression, FormulaKind::LoggerExpr]
        );
        assert_eq!(twice.first().path, "a.xml");
        assert!(twice.occurs_at("b.xml", "/logger[1]/conversion[1]/@expr"));
        assert!(!twice.occurs_at("b.xml", "/logger[1]/conversion[2]/@expr"));
        assert_eq!(h.distinct_of(FormulaKind::EcuToByte), 1);
        assert_eq!(h.distinct_of(FormulaKind::LoggerExpr), 2);
        assert!(h.by_sha256(&sha256_hex(b"x*3")).is_none());

        let meta = h.render_metadata();
        assert!(!meta.contains("x*2") && !meta.contains("x/2"), "{meta}");
        let parsed: toml::Table = toml::from_str(&meta).unwrap();
        assert_eq!(parsed["summary"]["distinct"].as_integer(), Some(3));
        assert_eq!(parsed["summary"]["occurrences"].as_integer(), Some(4));
        assert_eq!(parsed["formulas"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn sha256_matches_a_published_vector() {
        // FIPS 180-2, Appendix B.1: SHA-256("abc").
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
