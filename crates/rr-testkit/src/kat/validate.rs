//! KAT fixture validator (US-RR-9-6): the semantic rules on top of the
//! schema-1 types in [`crate::kat`]. CI runs it as
//! `cargo xtask validate-fixtures [--base <git-rev> | --push-before <git-rev>]
//! [PATH...]`, which runs
//! this crate's `validate-fixtures` example (a thin `main` around [`cli`]; an
//! example rather than a binary, because the workspace's only binaries are
//! `romraider-rs`, `romraider-cli` and `xtask`).
//!
//! Every fixture file must first load with [`crate::kat::parse`] (schema,
//! shape, unknown keys, hex, ...). On top of that, each violation below is
//! reported with the file, the case (`[[decode]] #2 (offset 2)`,
//! `` [[checksum]] #1 `after` ``, `` [[expr]] #3 (`2+3*4`) ``,
//! `[[unit]] #1 (1 mph -> kmh)`, `[[timing]] #3 (iso14230_2::P3_MIN)`,
//! `[[codec]] #2 (ncs-crc16)`, `[range]`,
//! `[[changelog]] #1`; cases count from 1) and a rule code:
//!
//! | Rule | Rejected when |
//! |------|---------------|
//! | `load` | The file does not load as a schema-1 fixture. |
//! | `missing-source` | An expected value (a `[[decode]]`, `[[expr]]`, `[[unit]]`, `[[timing]]` or `[[codec]]` case, a `[[checksum]]` case's `before` / `after`, or the `[range]`) has no `source`. |
//! | `hand-calc-working` | A `hand-calc` source has no `working`, or its `working` breaks the rule below. |
//! | `empty-source` | A `standard` source has an empty `doc` or `section`; a `stock-rom` an empty `rom`; a `car-trace` an empty `trace`. |
//! | `cross-source` | A `cross-source` source cites fewer than two distinct, non-empty sources. |
//! | `forbidden-source` | A source cites Java (the word `java`, any case): values taken from the Java application, or from the code under test, are not a source (D-024). A `legacy-test` source's `file` path is not searched: it names a legacy test file by design (next rule). |
//! | `legacy-test` | A `legacy-test` source (literal data transcribed from a legacy test file, US-RR-14-8) is used outside a `[[codec]]` case; or its `file` is not a relative, `/`-separated path under `legacy/src/test/` (no `.` or `..` parts, no backslash); or its `lines` is not ascending line numbers and `first-last` ranges ([`super::parse_line_spec`]). That the file exists and the lines hold the data is the consuming test's check (rr-testkit's `ncs_codec_kat`). |
//! | `citation` | A standard test-vector fixture ([`is_vector_fixture`]: file name starting with one of [`VECTOR_PREFIXES`], or any `[[timing]]` or `[[codec]]` fixture) has no `[[citation]]`; or one of its cases has a `standard` source whose `doc` does not start with a cited `standard` document's `doc`, or a `legacy-test` source whose `file` is not a cited `legacy-test` file. In any fixture, a `[[citation]]` is malformed: a `standard` one with an empty `doc`, `edition` or `section`, or an `edition` naming no year (four digits, 1900-2099) and no `rev` / `revision` / `version` / `issue` identifier; `verified = false` outside [`UNVERIFIED_CITATION_PREFIXES`] or without `via`; a `legacy-test` one outside a `[[codec]]` fixture or not under `legacy/src/test/`; a `legacy-algorithm` one outside [`LEGACY_ALGORITHM_FIXTURES`], not under `legacy/src/main/` or without a `note`; or bad `lines`. That a cited legacy file exists and has the lines is rr-testkit's `vector_citations` test (this validator reads fixtures only). A `standard` citation that mentions Java is `forbidden-source`. |
//! | `real-without-conversion` | A case has `real`, or the fixture has a `[range]`, but its inline `[definition]` has no `[definition.conversion]`. |
//! | `tolerance` | `tolerance` is negative or not finite, or is given without `real` (decode) or with a `value` that is not finite (expr). Also for a `[[unit]]` case's `tolerance` on `expected`. |
//! | `range` | A `[range]` contradicts itself: `real_min` or `real_max` is not finite, `real_min` is above `real_max`, or its `monotonicity` disagrees with where the extremes are (`increasing` needs `real_min < real_max` with `raw_at_min` below `raw_at_max`, `decreasing` the same with `raw_at_min` above `raw_at_max`, `constant` needs `real_min == real_max`). |
//! | `offset-out-of-range` | With `hex` input: a decode cell (inline `encoding`, default layout) or a stored checksum does not fit inside the input. |
//! | `checksum-width` | A checksum case's `before.hex` and `after.hex` differ in length. |
//! | `changelog-reason` | A `[[changelog]]` entry has an empty `reason`. |
//! | `changed-without-changelog` | Base mode only: expected values differ from the base version and no new `[[changelog]]` entry was added. |
//!
//! # The `hand-calc` working rule
//!
//! A `hand-calc` source's `working` must show the calculation, not just
//! assert the answer. Concretely, after trimming it must
//!
//! 1. contain `=`: the working is written as an equation, formula → result;
//! 2. contain at least **two numbers**: the result and at least one input or
//!    intermediate value (decimal such as `244`, `-1`, `6533.6`, `1e-3`, or
//!    `0x` / `0b` prefixed such as `0x01F4`; digits glued to a letter, as in
//!    `u16`, are not numbers);
//! 3. show a formula: at least one binary arithmetic or bitwise operator
//!    (`+ - * / % & | ^ << >> **`) between two operands, at least one of them
//!    a number or a parenthesised expression. A sign (`= -1`, `= +0.0`), a
//!    hyphen between words (`big-endian`), an exponent (`1e-3`) and a
//!    comparison (`0 < 128`) are not operators;
//! 4. arrive at the value it justifies: for a `[[decode]]` case it must
//!    contain the expected `raw` value and, when the case has one, the
//!    expected `real` value (within the case's `tolerance`, else exactly up to
//!    rounding of 1e-9 relative); for an `[[expr]]` case, the expected
//!    `value` the same way, or for a value that is not finite the word `NaN`
//!    (any case) or `inf` / `infinity` (preceded by `-` for `-inf`); for a
//!    `[[unit]]` case, the expected `expected` the same way; for a
//!    `[[timing]]` case, its `ms` or `us` value exactly; for a
//!    `[[codec]]` case, its `output` the way a checksum's stored bytes are
//!    (next); for a
//!    checksum `before` / `after` it must contain the stored checksum, either
//!    as an integer (bytes read big- or little-endian) or as its bytes in hex
//!    (`00 0A`, `0x000A`); for a `[range]` it must contain `real_min` and
//!    `real_max` (within its `tolerance`, else up to 1e-9 relative) and
//!    `raw_at_min` and `raw_at_max`;
//! 5. show an intermediate value: at least one number that is not the
//!    result (not the expected `raw` / `real` / `value` / `expected`, not a
//!    timing case's `ms` / `us`, not one of the four
//!    values of a `[range]`, or not the stored checksum or codec `output`
//!    read big- or little-endian; `0x01F4` *is* 500).
//!
//! So `"500"` (no equation), `"x = 500"` (one number),
//! `"raw = 500, real = 50.0"` and `"0x01F4 = 500 = 50.0"` (no formula),
//! `"0x01F4 = 1*256 + 244 = 499"` (wrong result) and
//! `"500 - 500 + 500 = 500; 50.0 = 50.0"` (no intermediate value) are
//! rejected; `"0x01F4 = 1*256 + 244 = 500; 500 * 0.1 = 50.0"` passes for
//! `raw = 500, real = 50.0`.
//!
//! # Base mode: expected values need a changelog line to change
//!
//! With a base (`--base <git-rev>`, [`GitBase::resolve`]: the merge-base of
//! that revision and `HEAD`; or `--push-before <git-rev>`,
//! [`GitBase::for_push`]: what CI uses on a push, including a new branch or
//! a force push), every fixture that also exists in the base is compared
//! with its base version. A fixture that git sees as renamed or moved since
//! the base is compared with its version under the old path. The fixture's
//! *expected values* changed when any of these differ:
//!
//! * the `[input]` or the `[definition]` (the same answer to a different
//!   question is a different expected value: new input bytes, a different
//!   ROM reference or encoding);
//! * a base `[[decode]]` case (its `offset`, `raw`, `real`, `tolerance`),
//!   `[[checksum]]` case (`algorithm`, `params`, `offset`, `before` / `after`
//!   `hex` and `valid`) or `[[expr]]` case (`formula` or `formula_ref`, `x`,
//!   `params`, `value`, `tolerance`, compared bit for bit so a `nan` matches
//!   itself), `[[unit]]` case (`from`, `to`, `value`, `expected`,
//!   `tolerance`), `[[timing]]` case (`name` and its `ms` / `us`) or
//!   `[[codec]]` case (`op`, `params`, `input`, `output`) has no
//!   identical case in the new version, i.e. it was
//!   changed or removed (a moved `offset` is a change, so swapping two
//!   cases' offsets is caught). Cases are matched as a multiset, so adding
//!   or reordering cases is not a change;
//! * a base `[range]` (`real_min`, `raw_at_min`, `real_max`, `raw_at_max`,
//!   `monotonicity`, `tolerance`) was changed or removed (adding one is not
//!   a change);
//! * the fixture switched between decode, checksum, expr, unit, timing and
//!   codec cases.
//!
//! Sources, citations, notes and titles may change freely. When expected values changed,
//! the new version must **add** a `[[changelog]]` entry, else
//! `changed-without-changelog`. An added entry is one not in the base
//! version whose `reason` is not blank and is not the reason of a base
//! entry; and it must be net new: the number of added entries must exceed
//! the number of base entries that were edited or removed. So an entry that
//! was already in the base, a blank reason, a copy of an old entry, and an
//! old entry reworded or re-dated do not count (the changelog is
//! append-only).
//!
//! Not compared, so not caught by this rule (they are visible in review):
//! a fixture that is new (absent from the base, including a rename git does
//! not detect because the file was also heavily edited: it is then checked
//! as a new fixture); a fixture whose base version does not load; a deleted
//! fixture (there is no file to carry the changelog line); and a change to
//! an external file a fixture refers to (`[input] rom` is compared by its
//! reference and hash, a `[definition] ref` by its reference only).

use std::ffi::OsString;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{
    ChangelogEntry, ChecksumCase, ChecksumState, Citation, CodecCase, DecodeCase, Definition,
    ExprCase, Formula, HexBytes, Input, Kat, KatKind, Layout, Monotonicity, RangeCase, RawValue,
    Source, Storage, TimingCase, UnitCase,
};

/// The rule a [`Violation`] breaks (see the module docs for each).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rule {
    /// The file does not load as a schema-1 fixture.
    Load,
    /// An expected value has no `source`.
    MissingSource,
    /// A `hand-calc` source without proper `working`.
    HandCalcWorking,
    /// A source with an empty required field.
    EmptySource,
    /// A `cross-source` citing fewer than two distinct sources.
    CrossSource,
    /// A source citing Java (D-024).
    ForbiddenSource,
    /// A misplaced or malformed `legacy-test` source.
    LegacyTest,
    /// A standard test-vector fixture without a structured citation, or a
    /// malformed or misplaced `[[citation]]` (US-RR-14-1).
    Citation,
    /// `real` without a conversion in the inline definition.
    RealWithoutConversion,
    /// A bad or orphaned `tolerance`.
    Tolerance,
    /// A `[range]` that contradicts itself.
    Range,
    /// A cell or stored checksum outside the hex input.
    OffsetOutOfRange,
    /// `before.hex` and `after.hex` of different lengths.
    ChecksumWidth,
    /// A changelog entry with an empty `reason`.
    ChangelogReason,
    /// Expected values changed since the base without a new changelog entry.
    ChangedWithoutChangelog,
}

impl Rule {
    /// The rule code printed with each violation.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Load => "load",
            Self::MissingSource => "missing-source",
            Self::HandCalcWorking => "hand-calc-working",
            Self::EmptySource => "empty-source",
            Self::CrossSource => "cross-source",
            Self::ForbiddenSource => "forbidden-source",
            Self::LegacyTest => "legacy-test",
            Self::Citation => "citation",
            Self::RealWithoutConversion => "real-without-conversion",
            Self::Tolerance => "tolerance",
            Self::Range => "range",
            Self::OffsetOutOfRange => "offset-out-of-range",
            Self::ChecksumWidth => "checksum-width",
            Self::ChangelogReason => "changelog-reason",
            Self::ChangedWithoutChangelog => "changed-without-changelog",
        }
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// One broken rule in one fixture file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The fixture file.
    pub path: PathBuf,
    /// The case or entry (`[[decode]] #2 (offset 2)`), or `None` for the
    /// whole file.
    pub location: Option<String>,
    /// The rule broken.
    pub rule: Rule,
    /// What is wrong and how to fix it.
    pub message: String,
}

impl Violation {
    fn new(path: &Path, location: Option<&str>, rule: Rule, message: impl Into<String>) -> Self {
        Self {
            path: path.to_path_buf(),
            location: location.map(str::to_owned),
            rule,
            message: message.into(),
        }
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.path.display())?;
        if let Some(location) = &self.location {
            write!(f, "{location}: ")?;
        }
        write!(f, "{} [{}]", self.message, self.rule)
    }
}

/// Errors that stop validation altogether (as opposed to [`Violation`]s).
#[derive(Debug, thiserror::Error)]
pub enum ValidateError {
    /// A fixture file or directory could not be read.
    #[error("cannot read {}: {source}", path.display())]
    Io {
        /// The file or directory.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// A path given to [`discover`] does not exist.
    #[error("no such fixture file or directory: {}", path.display())]
    NotFound {
        /// The path.
        path: PathBuf,
    },
    /// A `git` command failed (bad base revision, not a repository, ...).
    #[error("`git {args}` failed: {message}")]
    Git {
        /// The git arguments.
        args: String,
        /// Its stderr, or why it could not run.
        message: String,
    },
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// The default fixture locations: the seed fixtures under
/// `<repo>/fixtures/kat` (when that directory exists) and rr-testkit's own
/// sample fixtures under `crates/rr-testkit/tests/data/kat`.
#[must_use]
pub fn default_paths() -> Vec<PathBuf> {
    let seed = crate::fixtures::repo_root().join("fixtures").join("kat");
    let samples = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join("kat");
    let mut paths = Vec::new();
    if seed.is_dir() {
        paths.push(seed);
    }
    paths.push(samples);
    paths
}

/// Expands `paths` into fixture files: a file is taken as is, a directory
/// contributes every `*.toml` file below it (recursively, sorted).
///
/// # Errors
///
/// [`ValidateError::NotFound`] for a path that does not exist,
/// [`ValidateError::Io`] for an unreadable directory.
pub fn discover(paths: &[PathBuf]) -> Result<Vec<PathBuf>, ValidateError> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_file() {
            files.push(path.clone());
        } else if path.is_dir() {
            walk(path, &mut files)?;
        } else {
            return Err(ValidateError::NotFound { path: path.clone() });
        }
    }
    Ok(files)
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), ValidateError> {
    let io = |source| ValidateError::Io {
        path: dir.to_path_buf(),
        source,
    };
    let mut entries = std::fs::read_dir(dir)
        .map_err(io)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(io)?;
    entries.sort();
    for entry in entries {
        if entry.is_dir() {
            walk(&entry, files)?;
        } else if entry.extension().is_some_and(|ext| ext == "toml") {
            files.push(entry);
        }
    }
    Ok(())
}

/// Where base versions of fixtures come from (base mode).
pub trait BaseSource {
    /// How the base is named in messages (e.g. `origin/master (1a2b3c4d5e6f)`).
    fn label(&self) -> &str;

    /// The base version of the fixture at `path`, or `None` if the fixture
    /// does not exist in the base.
    ///
    /// # Errors
    ///
    /// When the base cannot be read at all.
    fn read(&self, path: &Path) -> Result<Option<String>, ValidateError>;
}

/// The outcome of [`validate_files`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    /// How many fixture files were checked.
    pub files: usize,
    /// How many of them had a base version to compare with.
    pub compared: usize,
    /// Every violation found, in file order.
    pub violations: Vec<Violation>,
}

/// Validates every file in `files`, comparing each with its base version when
/// `base` is given.
///
/// # Errors
///
/// [`ValidateError::Io`] for an unreadable file, or the base's error.
pub fn validate_files(
    files: &[PathBuf],
    base: Option<&dyn BaseSource>,
) -> Result<Report, ValidateError> {
    let mut report = Report::default();
    for path in files {
        let text = std::fs::read_to_string(path).map_err(|source| ValidateError::Io {
            path: path.clone(),
            source,
        })?;
        let base_text = match base {
            Some(base) => base.read(path)?,
            None => None,
        };
        report.files += 1;
        if base_text.is_some() {
            report.compared += 1;
        }
        let label = base.map_or("", BaseSource::label);
        report.violations.extend(validate_text(
            path,
            &text,
            base_text.as_deref().map(|b| (label, b)),
        ));
    }
    Ok(report)
}

/// Validates one fixture's `text`; `path` names it in violations. With
/// `base = Some((label, base_text))` the fixture is also compared with its
/// base version (see the module docs, "Base mode").
#[must_use]
pub fn validate_text(path: &Path, text: &str, base: Option<(&str, &str)>) -> Vec<Violation> {
    let mut out = Vec::new();
    let Some(head) = load_checked(path, text, &mut out) else {
        return out;
    };
    check_fixture(path, &head, &mut out);
    if let Some((label, base_text)) = base
        && let Ok(base) = super::parse(path, base_text)
        && let Some(violation) = check_change(path, label, &base.data, &head)
    {
        out.push(violation);
    }
    out
}

/// Missing sources (and `hand-calc` without `working`) are found on the raw
/// TOML first, so the message names the case; anything else that stops the
/// fixture loading is reported as [`Rule::Load`] with the loader's message.
fn load_checked(path: &Path, text: &str, out: &mut Vec<Violation>) -> Option<Kat> {
    if let Ok(table) = toml::from_str::<toml::Table>(text) {
        let before = out.len();
        raw_source_checks(path, &table, out);
        if out.len() > before {
            return None;
        }
    }
    match super::parse(path, text) {
        Ok(fixture) => Some(fixture.data),
        Err(error) => {
            out.push(Violation::new(path, None, Rule::Load, error.to_string()));
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Locations
// ---------------------------------------------------------------------------

fn decode_location(index: usize, offset: u64) -> String {
    format!("[[decode]] #{} (offset {offset})", index + 1)
}

fn checksum_location(index: usize, state: &str) -> String {
    format!("[[checksum]] #{} `{state}`", index + 1)
}

fn expr_location(index: usize, formula: &dyn fmt::Display) -> String {
    format!("[[expr]] #{} (`{formula}`)", index + 1)
}

fn unit_location(index: usize, value: &dyn fmt::Display, from: &str, to: &str) -> String {
    format!("[[unit]] #{} ({value} {from} -> {to})", index + 1)
}

fn timing_location(index: usize, name: &str) -> String {
    format!("[[timing]] #{} ({name})", index + 1)
}

fn codec_location(index: usize, op: &str) -> String {
    format!("[[codec]] #{} ({op})", index + 1)
}

const RANGE_LOCATION: &str = "[range]";

// ---------------------------------------------------------------------------
// Raw (pre-load) checks
// ---------------------------------------------------------------------------

fn raw_source_checks(path: &Path, table: &toml::Table, out: &mut Vec<Violation>) {
    let cases = |key: &str| {
        table
            .get(key)
            .and_then(toml::Value::as_array)
            .map(|cases| {
                cases
                    .iter()
                    .filter_map(toml::Value::as_table)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    for (index, case) in cases("decode").into_iter().enumerate() {
        let offset = match case.get("offset") {
            Some(toml::Value::Integer(n)) => u64::try_from(*n).ok(),
            Some(toml::Value::String(s)) => super::parse_int(s).and_then(|n| u64::try_from(n).ok()),
            None => Some(0),
            Some(_) => None,
        };
        let location = match offset {
            Some(offset) => decode_location(index, offset),
            None => format!("[[decode]] #{}", index + 1),
        };
        raw_source(path, &location, case.get("source"), out);
    }
    for (index, case) in cases("checksum").into_iter().enumerate() {
        for state in ["before", "after"] {
            if let Some(state_table) = case.get(state).and_then(toml::Value::as_table) {
                let location = checksum_location(index, state);
                raw_source(path, &location, state_table.get("source"), out);
            }
        }
    }
    for (index, case) in cases("expr").into_iter().enumerate() {
        let reference = case
            .get("formula_ref")
            .and_then(|r| r.get("sha256"))
            .and_then(toml::Value::as_str)
            .map(|sha| format!("sha256:{}", sha.get(..12).unwrap_or(sha)));
        let location = match (case.get("formula").and_then(toml::Value::as_str), reference) {
            (Some(formula), _) => expr_location(index, &formula),
            (None, Some(reference)) => expr_location(index, &reference),
            (None, None) => format!("[[expr]] #{}", index + 1),
        };
        raw_source(path, &location, case.get("source"), out);
    }
    for (index, case) in cases("unit").into_iter().enumerate() {
        let text = |key: &str| case.get(key).and_then(toml::Value::as_str);
        let value = case
            .get("value")
            .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)));
        let location = match (value, text("from"), text("to")) {
            (Some(value), Some(from), Some(to)) => unit_location(index, &value, from, to),
            _ => format!("[[unit]] #{}", index + 1),
        };
        raw_source(path, &location, case.get("source"), out);
    }
    for (index, case) in cases("timing").into_iter().enumerate() {
        let location = match case.get("name").and_then(toml::Value::as_str) {
            Some(name) => timing_location(index, name),
            None => format!("[[timing]] #{}", index + 1),
        };
        raw_source(path, &location, case.get("source"), out);
    }
    for (index, case) in cases("codec").into_iter().enumerate() {
        let location = match case.get("op").and_then(toml::Value::as_str) {
            Some(op) => codec_location(index, op),
            None => format!("[[codec]] #{}", index + 1),
        };
        raw_source(path, &location, case.get("source"), out);
    }
    if let Some(range) = table.get("range").and_then(toml::Value::as_table) {
        raw_source(path, RANGE_LOCATION, range.get("source"), out);
    }
}

fn raw_source(path: &Path, location: &str, source: Option<&toml::Value>, out: &mut Vec<Violation>) {
    match source {
        None => out.push(Violation::new(
            path,
            Some(location),
            Rule::MissingSource,
            "expected value has no `source`; every expected value needs one: \
             `source = { kind = \"hand-calc\", working = \"...\" }`, or kind \
             \"standard\", \"stock-rom\", \"car-trace\" or \"cross-source\"",
        )),
        Some(toml::Value::Table(source))
            if source.get("kind").and_then(toml::Value::as_str) == Some("hand-calc")
                && !source.contains_key("working") =>
        {
            out.push(Violation::new(
                path,
                Some(location),
                Rule::HandCalcWorking,
                "`hand-calc` source has no `working`: show the formula and the \
                 intermediate values, e.g. working = \"0x01F4 = 1*256 + 244 = 500\"",
            ));
        }
        Some(_) => {}
    }
}

// ---------------------------------------------------------------------------
// Semantic checks on a loaded fixture
// ---------------------------------------------------------------------------

/// What a source justifies, for the `hand-calc` working rule.
enum Expected<'a> {
    Decode(&'a DecodeCase),
    Checksum(&'a ChecksumState),
    Expr(&'a ExprCase),
    Unit(&'a UnitCase),
    Timing(&'a TimingCase),
    Codec(&'a CodecCase),
    Range(&'a RangeCase),
}

fn check_fixture(path: &Path, kat: &Kat, out: &mut Vec<Violation>) {
    let input = match &kat.input {
        Some(Input::Hex(bytes)) => Some(bytes.as_bytes().len()),
        Some(Input::Rom(_)) | None => None,
    };
    match &kat.kind {
        KatKind::Decode {
            definition,
            cases,
            range,
        } => {
            for (index, case) in cases.iter().enumerate() {
                let location = decode_location(index, case.offset);
                check_source(path, &location, &case.source, &Expected::Decode(case), out);
                check_decode_case(path, &location, definition, input, case, out);
            }
            if let Some(range) = range {
                check_source(
                    path,
                    RANGE_LOCATION,
                    &range.source,
                    &Expected::Range(range),
                    out,
                );
                check_range(path, definition, range, out);
            }
        }
        KatKind::Checksum { cases, .. } => {
            for (index, case) in cases.iter().enumerate() {
                for (state, value) in [("before", &case.before), ("after", &case.after)] {
                    let location = checksum_location(index, state);
                    check_source(
                        path,
                        &location,
                        &value.source,
                        &Expected::Checksum(value),
                        out,
                    );
                }
                check_checksum_case(path, index, input, case, out);
            }
        }
        KatKind::Expr { cases } => {
            for (index, case) in cases.iter().enumerate() {
                let location = expr_location(index, &case.formula);
                check_source(path, &location, &case.source, &Expected::Expr(case), out);
                check_expr_case(path, &location, case, out);
            }
        }
        KatKind::Unit { cases } => {
            for (index, case) in cases.iter().enumerate() {
                let location = unit_location(index, &case.value, &case.from, &case.to);
                check_source(path, &location, &case.source, &Expected::Unit(case), out);
                if let Some(tolerance) = case.tolerance
                    && (!tolerance.is_finite() || tolerance < 0.0)
                {
                    out.push(Violation::new(
                        path,
                        Some(&location),
                        Rule::Tolerance,
                        format!(
                            "`tolerance` must be a finite, non-negative number, got {tolerance}"
                        ),
                    ));
                }
            }
        }
        KatKind::Timing { cases } => {
            for (index, case) in cases.iter().enumerate() {
                let location = timing_location(index, &case.name);
                check_source(path, &location, &case.source, &Expected::Timing(case), out);
            }
        }
        KatKind::Codec { cases } => {
            for (index, case) in cases.iter().enumerate() {
                let location = codec_location(index, &case.op);
                check_source(path, &location, &case.source, &Expected::Codec(case), out);
            }
        }
    }
    for (index, entry) in kat.changelog.iter().enumerate() {
        if entry.reason.trim().is_empty() {
            out.push(Violation::new(
                path,
                Some(&format!("[[changelog]] #{}", index + 1)),
                Rule::ChangelogReason,
                "changelog `reason` is empty: say why the expected value changed",
            ));
        }
    }
    check_citations(path, kat, out);
}

// ---------------------------------------------------------------------------
// Citations (US-RR-14-1)
// ---------------------------------------------------------------------------

/// File-name prefixes of the standard test-vector fixtures (US-RR-14): the
/// ISO timing parameters, the SAE J1979 PID vectors, the checksum and CRC
/// reference vectors and the transcribed NCS codec vectors. A fixture whose
/// file name starts with one of these, or any `[[timing]]` or `[[codec]]`
/// fixture whatever its name, is a vector fixture
/// ([`is_vector_fixture`]) and must cite its documents (rule `citation`).
pub const VECTOR_PREFIXES: &[&str] = &["timing-", "j1979-", "checksum-", "ncs-codec-"];

/// The flagged exception for SAE J1979: fixtures whose file name starts with
/// one of these may carry a `standard` citation with `verified = false`
/// (with `via` sources). SAE J1979 is paywalled; its PID formulas were taken
/// from secondary sources and its edition and section were not read
/// first-hand (US-RR-14-6). Nothing else may.
pub const UNVERIFIED_CITATION_PREFIXES: &[&str] = &["j1979-01-"];

/// The flagged exception for checksums no standard defines: only these
/// fixture files may carry a `legacy-algorithm` citation (the legacy code
/// whose algorithm their hand-worked sums and XORs model, US-RR-14-7).
pub const LEGACY_ALGORITHM_FIXTURES: &[&str] = &[
    "checksum-sum-bytes.toml",
    "checksum-sum-words.toml",
    "checksum-xor.toml",
];

/// Whether the fixture at `path` is a standard test-vector fixture: its file
/// name starts with one of [`VECTOR_PREFIXES`], or it is a `[[timing]]` or
/// `[[codec]]` fixture (shapes that exist only for vector fixtures).
#[must_use]
pub fn is_vector_fixture(path: &Path, kat: &Kat) -> bool {
    let name = file_name(path);
    VECTOR_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
        || matches!(kat.kind, KatKind::Timing { .. } | KatKind::Codec { .. })
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Whether `edition` names an edition: a year from 1900 to 2099 written as
/// four digits (`1999`, `2004-10-15`), or a revision, version or issue
/// identifier (`version 3`, `rev. B`).
fn names_edition(edition: &str) -> bool {
    let lower = edition.to_lowercase();
    let year = lower
        .split(|c: char| !c.is_ascii_digit())
        .any(|run| run.len() == 4 && matches!(run.parse::<u32>(), Ok(1900..=2099)));
    let words: Vec<&str> = lower.split_whitespace().collect();
    let revision = words.windows(2).any(|pair| {
        matches!(pair[0], "rev" | "rev." | "revision" | "version" | "issue")
            && pair[1].chars().any(|c| c.is_ascii_alphanumeric())
    });
    year || revision
}

/// Every case source of `kat`, with its location.
fn case_sources(kat: &Kat) -> Vec<(String, &Source)> {
    let mut out = Vec::new();
    match &kat.kind {
        KatKind::Decode { cases, range, .. } => {
            for (index, case) in cases.iter().enumerate() {
                out.push((decode_location(index, case.offset), &case.source));
            }
            if let Some(range) = range {
                out.push((RANGE_LOCATION.to_owned(), &range.source));
            }
        }
        KatKind::Checksum { cases, .. } => {
            for (index, case) in cases.iter().enumerate() {
                for (state, value) in [("before", &case.before), ("after", &case.after)] {
                    out.push((checksum_location(index, state), &value.source));
                }
            }
        }
        KatKind::Expr { cases } => {
            for (index, case) in cases.iter().enumerate() {
                out.push((expr_location(index, &case.formula), &case.source));
            }
        }
        KatKind::Unit { cases } => {
            for (index, case) in cases.iter().enumerate() {
                let location = unit_location(index, &case.value, &case.from, &case.to);
                out.push((location, &case.source));
            }
        }
        KatKind::Timing { cases } => {
            for (index, case) in cases.iter().enumerate() {
                out.push((timing_location(index, &case.name), &case.source));
            }
        }
        KatKind::Codec { cases } => {
            for (index, case) in cases.iter().enumerate() {
                out.push((codec_location(index, &case.op), &case.source));
            }
        }
    }
    out
}

/// The `citation` rule (module docs).
fn check_citations(path: &Path, kat: &Kat, out: &mut Vec<Violation>) {
    let name = file_name(path);
    let mut push = |location: Option<&str>, rule: Rule, message: String| {
        out.push(Violation::new(path, location, rule, message));
    };
    let empty = |field: &str| field.trim().is_empty();
    for (index, citation) in kat.citations.iter().enumerate() {
        let location = format!("[[citation]] #{} ({})", index + 1, citation.kind());
        let location = Some(location.as_str());
        match citation {
            Citation::Standard {
                doc,
                edition,
                section,
                verified,
                via,
                note,
            } => {
                if empty(doc) || empty(edition) || empty(section) {
                    push(
                        location,
                        Rule::Citation,
                        "a `standard` citation needs a non-empty `doc`, `edition` and `section`"
                            .to_owned(),
                    );
                } else if !names_edition(edition) {
                    push(
                        location,
                        Rule::Citation,
                        format!(
                            "`edition` {edition:?} names no edition: give its year (four \
                             digits) or its revision / version identifier"
                        ),
                    );
                }
                if !verified {
                    if !UNVERIFIED_CITATION_PREFIXES
                        .iter()
                        .any(|prefix| name.starts_with(prefix))
                    {
                        push(
                            location,
                            Rule::Citation,
                            format!(
                                "`verified = false` is a flagged exception allowed only in the \
                                 fixtures {UNVERIFIED_CITATION_PREFIXES:?}* (SAE J1979, \
                                 paywalled); read the cited edition and section first-hand"
                            ),
                        );
                    }
                    if via.is_empty() || via.iter().any(|v| empty(v)) {
                        push(
                            location,
                            Rule::Citation,
                            "an unverified citation (`verified = false`) needs `via`: the \
                             secondary sources its values were taken from, none empty"
                                .to_owned(),
                        );
                    }
                }
                let texts = [Some(doc), Some(edition), Some(section), note.as_ref()]
                    .into_iter()
                    .flatten()
                    .chain(via);
                if texts.into_iter().any(|text| mentions_java(text)) {
                    push(
                        location,
                        Rule::ForbiddenSource,
                        "citation cites Java: a `standard` citation names a published \
                         document, never the Java application (D-024)"
                            .to_owned(),
                    );
                }
            }
            Citation::LegacyTest { file, lines } => {
                if !matches!(kat.kind, KatKind::Codec { .. }) {
                    push(
                        location,
                        Rule::Citation,
                        "a `legacy-test` citation is only for a [[codec]] fixture (the \
                         transcribed legacy test vectors, US-RR-14-8)"
                            .to_owned(),
                    );
                }
                if !legacy_path_ok(file, "legacy/src/test/") {
                    push(
                        location,
                        Rule::Citation,
                        format!(
                            "`legacy-test` citation `file` {file:?} must be a relative, \
                             `/`-separated path to a file under `legacy/src/test/`"
                        ),
                    );
                }
                if let Err(problem) = super::parse_line_spec(lines) {
                    push(location, Rule::Citation, problem);
                }
            }
            Citation::LegacyAlgorithm { file, lines, note } => {
                if !LEGACY_ALGORITHM_FIXTURES.contains(&name.as_str()) {
                    push(
                        location,
                        Rule::Citation,
                        format!(
                            "a `legacy-algorithm` citation is a flagged exception allowed only \
                             in {LEGACY_ALGORITHM_FIXTURES:?} (checksums no standard defines); \
                             cite a published standard"
                        ),
                    );
                }
                if !legacy_path_ok(file, "legacy/src/main/") {
                    push(
                        location,
                        Rule::Citation,
                        format!(
                            "`legacy-algorithm` citation `file` {file:?} must be a relative, \
                             `/`-separated path to a file under `legacy/src/main/`"
                        ),
                    );
                }
                if let Err(problem) = super::parse_line_spec(lines) {
                    push(location, Rule::Citation, problem);
                }
                if empty(note) {
                    push(
                        location,
                        Rule::Citation,
                        "a `legacy-algorithm` citation needs a `note` saying why no standard \
                         is cited"
                            .to_owned(),
                    );
                }
            }
        }
    }
    if !is_vector_fixture(path, kat) {
        return;
    }
    if kat.citations.is_empty() {
        push(
            None,
            Rule::Citation,
            format!(
                "a standard test-vector fixture (file name {VECTOR_PREFIXES:?}*, or a [[timing]] \
                 or [[codec]] fixture) needs a [[citation]]: the document, edition and section \
                 its values rest on (`kind = \"standard\"`), or the legacy test file and lines \
                 it transcribes (`kind = \"legacy-test\"`)"
            ),
        );
    }
    // Per case: a `standard` source names a cited document; a `legacy-test`
    // source a cited test file.
    for (location, source) in case_sources(kat) {
        match source {
            Source::Standard { doc, .. } => {
                let cited = kat.citations.iter().any(|citation| {
                    matches!(citation, Citation::Standard { doc: cited, .. }
                        if !cited.trim().is_empty() && doc.trim().starts_with(cited.trim()))
                });
                if !cited {
                    push(
                        Some(&location),
                        Rule::Citation,
                        format!(
                            "source document {doc:?} is not cited: add a `standard` \
                             [[citation]] whose `doc` it starts with"
                        ),
                    );
                }
            }
            // Outside a [[codec]] fixture the source is already rule
            // `legacy-test`.
            Source::LegacyTest { file, .. } if matches!(kat.kind, KatKind::Codec { .. }) => {
                let cited = kat.citations.iter().any(|citation| {
                    matches!(citation, Citation::LegacyTest { file: cited, .. } if cited == file)
                });
                if !cited {
                    push(
                        Some(&location),
                        Rule::Citation,
                        format!(
                            "legacy test file {file:?} is not cited: add a `legacy-test` \
                             [[citation]] with that `file`"
                        ),
                    );
                }
            }
            _ => {}
        }
    }
}

fn check_decode_case(
    path: &Path,
    location: &str,
    definition: &Definition,
    input: Option<usize>,
    case: &DecodeCase,
    out: &mut Vec<Violation>,
) {
    let mut push = |rule, message: String| {
        out.push(Violation::new(path, Some(location), rule, message));
    };
    if case.real.is_some()
        && let Definition::Inline {
            conversion: None, ..
        } = definition
    {
        push(
            Rule::RealWithoutConversion,
            "`real` is given but the inline [definition] has no [definition.conversion]; \
             add one or drop `real`"
                .to_owned(),
        );
    }
    if let Some(tolerance) = case.tolerance {
        if !tolerance.is_finite() || tolerance < 0.0 {
            push(
                Rule::Tolerance,
                format!("`tolerance` must be a finite, non-negative number, got {tolerance}"),
            );
        } else if case.real.is_none() {
            push(
                Rule::Tolerance,
                "`tolerance` applies to `real`, but this case has no `real`".to_owned(),
            );
        }
    }
    if let (
        Some(len),
        Definition::Inline {
            encoding:
                super::Encoding {
                    storage,
                    layout: Layout::Default,
                    ..
                },
            ..
        },
    ) = (input, definition)
        && let Some(width) = storage_width(*storage)
        && case.offset.saturating_add(width) > len as u64
    {
        push(
            Rule::OffsetOutOfRange,
            format!(
                "a {storage} cell at offset {} needs {width} byte(s) but the input is only {len} byte(s)",
                case.offset
            ),
        );
    }
}

fn check_range(path: &Path, definition: &Definition, range: &RangeCase, out: &mut Vec<Violation>) {
    let mut push = |rule, message: String| {
        out.push(Violation::new(path, Some(RANGE_LOCATION), rule, message));
    };
    if let Definition::Inline {
        conversion: None, ..
    } = definition
    {
        push(
            Rule::RealWithoutConversion,
            "[range] gives real values but the inline [definition] has no \
             [definition.conversion]; add one or drop [range]"
                .to_owned(),
        );
    }
    if let Some(tolerance) = range.tolerance
        && (!tolerance.is_finite() || tolerance < 0.0)
    {
        push(
            Rule::Tolerance,
            format!("`tolerance` must be a finite, non-negative number, got {tolerance}"),
        );
    }
    let (min, max) = (range.real_min, range.real_max);
    let raws = raw_order(range.raw_at_min, range.raw_at_max);
    let problem = if !min.is_finite() || !max.is_finite() {
        Some(format!(
            "`real_min` ({min}) and `real_max` ({max}) must be finite: a display range \
             leaves out raw values with no real value"
        ))
    } else if min > max {
        Some(format!(
            "`real_min` ({min}) is above `real_max` ({max}): the minimum and maximum are \
             swapped (MF-13 is exactly this mistake for a decreasing scaling)"
        ))
    } else {
        match range.monotonicity {
            Monotonicity::Increasing | Monotonicity::Decreasing if min == max => Some(format!(
                "`monotonicity` is {} but `real_min` equals `real_max` ({min}); \
                 a conversion with one real value is `constant`",
                range.monotonicity.name()
            )),
            Monotonicity::Increasing if raws != Some(std::cmp::Ordering::Less) => Some(format!(
                "`monotonicity` is increasing, so `raw_at_min` ({}) must be below \
                 `raw_at_max` ({})",
                show_raw(range.raw_at_min),
                show_raw(range.raw_at_max)
            )),
            Monotonicity::Decreasing if raws != Some(std::cmp::Ordering::Greater) => Some(format!(
                "`monotonicity` is decreasing, so `raw_at_min` ({}) must be above \
                     `raw_at_max` ({}): a decreasing scaling is lowest at the top of the raw range",
                show_raw(range.raw_at_min),
                show_raw(range.raw_at_max)
            )),
            Monotonicity::Constant if min != max => Some(format!(
                "`monotonicity` is constant but `real_min` ({min}) differs from `real_max` ({max})"
            )),
            _ => None,
        }
    };
    if let Some(problem) = problem {
        push(Rule::Range, problem);
    }
}

/// How two raw values compare (integers exactly, floats by value); `None`
/// for a NaN.
fn raw_order(a: RawValue, b: RawValue) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (RawValue::Int(a), RawValue::Int(b)) => Some(a.cmp(&b)),
        (a, b) => raw_f64(a).partial_cmp(&raw_f64(b)),
    }
}

const fn raw_f64(raw: RawValue) -> f64 {
    match raw {
        RawValue::Int(n) => n as f64,
        RawValue::Float(x) => x,
    }
}

fn check_expr_case(path: &Path, location: &str, case: &ExprCase, out: &mut Vec<Violation>) {
    if let Some(tolerance) = case.tolerance {
        let message = if !tolerance.is_finite() || tolerance < 0.0 {
            format!("`tolerance` must be a finite, non-negative number, got {tolerance}")
        } else if !case.value.is_finite() {
            format!(
                "`tolerance` applies to a finite `value`, but this case expects {}",
                case.value
            )
        } else {
            return;
        };
        out.push(Violation::new(
            path,
            Some(location),
            Rule::Tolerance,
            message,
        ));
    }
}

/// Bytes one cell of `storage` occupies in the default layout, where the
/// fixture format pins it down (`u20` is an instruction immediate, so not).
const fn storage_width(storage: Storage) -> Option<u64> {
    match storage {
        Storage::U8 | Storage::I8 => Some(1),
        Storage::U16 | Storage::I16 => Some(2),
        Storage::U24 | Storage::I24 => Some(3),
        Storage::U32 | Storage::I32 | Storage::F32 => Some(4),
        Storage::U64 | Storage::I64 | Storage::F64 => Some(8),
        Storage::U20 => None,
    }
}

fn check_checksum_case(
    path: &Path,
    index: usize,
    input: Option<usize>,
    case: &ChecksumCase,
    out: &mut Vec<Violation>,
) {
    let location = format!("[[checksum]] #{}", index + 1);
    let before = case.before.hex.as_bytes().len();
    let after = case.after.hex.as_bytes().len();
    if before != after {
        out.push(Violation::new(
            path,
            Some(&location),
            Rule::ChecksumWidth,
            format!(
                "`before.hex` is {before} byte(s) but `after.hex` is {after}; \
                 fixing a checksum does not change its width"
            ),
        ));
    }
    if let Some(len) = input
        && case.offset.saturating_add(before.max(after) as u64) > len as u64
    {
        out.push(Violation::new(
            path,
            Some(&location),
            Rule::OffsetOutOfRange,
            format!(
                "the stored checksum at offset {} is {} byte(s) but the input is only {len} byte(s)",
                case.offset,
                before.max(after)
            ),
        ));
    }
}

fn check_source(
    path: &Path,
    location: &str,
    source: &Source,
    expected: &Expected<'_>,
    out: &mut Vec<Violation>,
) {
    let mut push = |rule, message: String| {
        out.push(Violation::new(path, Some(location), rule, message));
    };
    let empty = |field: &str| field.trim().is_empty();
    match source {
        Source::HandCalc { working } => {
            if let Err(problem) = check_working(working, expected) {
                push(Rule::HandCalcWorking, problem);
            }
        }
        Source::Standard { doc, section } => {
            if empty(doc) || empty(section) {
                push(
                    Rule::EmptySource,
                    "`standard` source needs a non-empty `doc` and `section`".to_owned(),
                );
            }
        }
        Source::StockRom { rom, .. } => {
            if empty(rom) {
                push(
                    Rule::EmptySource,
                    "`stock-rom` source needs a non-empty `rom` (ROM ID or file digest)".to_owned(),
                );
            }
        }
        Source::CarTrace { trace, .. } => {
            if empty(trace) {
                push(
                    Rule::EmptySource,
                    "`car-trace` source needs a non-empty `trace`".to_owned(),
                );
            }
        }
        Source::CrossSource { sources } => {
            let mut distinct: Vec<String> = sources
                .iter()
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect();
            distinct.sort();
            distinct.dedup();
            if distinct.len() < 2 || distinct.len() != sources.len() {
                push(
                    Rule::CrossSource,
                    format!(
                        "`cross-source` needs at least two distinct, non-empty `sources`, got {sources:?}"
                    ),
                );
            }
        }
        Source::LegacyTest { file, lines } => {
            if !matches!(expected, Expected::Codec(_)) {
                push(
                    Rule::LegacyTest,
                    "a `legacy-test` source is only for a [[codec]] case (the transcribed \
                     legacy test vectors, US-RR-14-8); elsewhere a value needs a Java-free \
                     source (D-024)"
                        .to_owned(),
                );
            }
            if let Err(problem) = check_legacy_file(file) {
                push(Rule::LegacyTest, problem);
            }
            if let Err(problem) = super::parse_line_spec(lines) {
                push(Rule::LegacyTest, problem);
            }
        }
    }
    if source_texts(source).iter().any(|text| mentions_java(text)) {
        push(
            Rule::ForbiddenSource,
            "source cites Java: values from the Java application (or from the code \
             under test) are not a source (D-024); use a hand calculation, a \
             standard, a stock-ROM invariant, a car trace or independent sources"
                .to_owned(),
        );
    }
}

fn source_texts(source: &Source) -> Vec<&str> {
    match source {
        Source::HandCalc { working } => vec![working],
        Source::Standard { doc, section } => vec![doc, section],
        Source::StockRom { rom, note } => [Some(rom.as_str()), note.as_deref()]
            .into_iter()
            .flatten()
            .collect(),
        Source::CarTrace { trace, note } => [Some(trace.as_str()), note.as_deref()]
            .into_iter()
            .flatten()
            .collect(),
        Source::CrossSource { sources } => sources.iter().map(String::as_str).collect(),
        // The file path names a legacy test file by design (rule
        // `legacy-test` checks it); the line numbers are numbers.
        Source::LegacyTest { .. } => Vec::new(),
    }
}

/// The `legacy-test` rule on a source's `file`: a relative, `/`-separated
/// path under `legacy/src/test/`, with no empty, `.` or `..` part.
fn check_legacy_file(file: &str) -> Result<(), String> {
    const ROOT: &str = "legacy/src/test/";
    if !legacy_path_ok(file, ROOT) {
        return Err(format!(
            "`legacy-test` source `file` {file:?} must be a relative, `/`-separated path \
             to a file under `{ROOT}` (a legacy test file, not application code)"
        ));
    }
    Ok(())
}

/// Whether `file` is a relative, `/`-separated path to a file under `root`
/// (which ends in `/`), with no empty, `.` or `..` part and no backslash.
fn legacy_path_ok(file: &str, root: &str) -> bool {
    let parts_ok = file
        .split('/')
        .all(|part| !part.is_empty() && part != "." && part != "..");
    file.starts_with(root) && file.len() > root.len() && !file.contains('\\') && parts_ok
}

/// Whether `text` contains the word `java` (any case, not part of a longer
/// word such as `javascript`).
fn mentions_java(text: &str) -> bool {
    let lower = text.to_lowercase();
    let bytes = lower.as_bytes();
    lower.match_indices("java").any(|(at, _)| {
        let before = at.checked_sub(1).map(|i| bytes[i]);
        let after = bytes.get(at + 4).copied();
        !before.is_some_and(|b| b.is_ascii_alphanumeric())
            && !after.is_some_and(|b| b.is_ascii_alphanumeric())
    })
}

/// The `hand-calc` working rule (module docs).
fn check_working(working: &str, expected: &Expected<'_>) -> Result<(), String> {
    let working = working.trim();
    if working.is_empty() {
        return Err(
            "`hand-calc` working is empty: show the formula and the intermediate values, \
             e.g. \"0x01F4 = 1*256 + 244 = 500\""
                .to_owned(),
        );
    }
    if !working.contains('=') {
        return Err(format!(
            "`hand-calc` working {working:?} has no `=`: write the calculation as an \
             equation from the formula to the result"
        ));
    }
    let numbers = numbers(working);
    if numbers.len() < 2 {
        return Err(format!(
            "`hand-calc` working {working:?} shows fewer than two numbers: include the \
             inputs or intermediate values, not just the result"
        ));
    }
    if !has_binary_operator(working) {
        return Err(format!(
            "`hand-calc` working {working:?} shows no formula: write the calculation with \
             its operators (`+ - * / % & | ^ << >>`), e.g. \"0x01F4 = 1*256 + 244 = 500\", \
             not just the answer"
        ));
    }
    match expected {
        Expected::Decode(case) => {
            if !mentions_raw(&numbers, case.raw) {
                return Err(format!(
                    "`hand-calc` working {working:?} does not arrive at the expected raw value {}",
                    show_raw(case.raw)
                ));
            }
            if let Some(real) = case.real
                && !mentions_float(&numbers, real, case.tolerance.unwrap_or(0.0))
            {
                return Err(format!(
                    "`hand-calc` working {working:?} does not arrive at the expected real value {real}"
                ));
            }
        }
        Expected::Checksum(state) => {
            if !mentions_checksum(working, &numbers, &state.hex) {
                return Err(format!(
                    "`hand-calc` working {working:?} does not mention the stored checksum {}",
                    show_hex(&state.hex)
                ));
            }
        }
        Expected::Range(range) => {
            let tolerance = range.tolerance.unwrap_or(0.0);
            for (what, raw) in [
                ("raw_at_min", range.raw_at_min),
                ("raw_at_max", range.raw_at_max),
            ] {
                if !mentions_raw(&numbers, raw) {
                    return Err(format!(
                        "`hand-calc` working {working:?} does not mention `{what}` {}",
                        show_raw(raw)
                    ));
                }
            }
            for (what, real) in [("real_min", range.real_min), ("real_max", range.real_max)] {
                // A non-finite extreme is rule `range`; no working reaches it.
                if !real.is_finite() || !mentions_float(&numbers, real, tolerance) {
                    return Err(format!(
                        "`hand-calc` working {working:?} does not arrive at `{what}` {real}"
                    ));
                }
            }
        }
        Expected::Expr(case) => {
            let found = if case.value.is_finite() {
                mentions_float(&numbers, case.value, case.tolerance.unwrap_or(0.0))
            } else {
                mentions_non_finite(working, case.value)
            };
            if !found {
                return Err(format!(
                    "`hand-calc` working {working:?} does not arrive at the expected value {}",
                    case.value
                ));
            }
        }
        Expected::Unit(case) => {
            if !mentions_float(&numbers, case.expected, case.tolerance.unwrap_or(0.0)) {
                return Err(format!(
                    "`hand-calc` working {working:?} does not arrive at the expected value {} {}",
                    case.expected, case.to
                ));
            }
        }
        Expected::Codec(case) => {
            if !mentions_checksum(working, &numbers, &case.output) {
                return Err(format!(
                    "`hand-calc` working {working:?} does not arrive at the expected output {}",
                    show_hex(&case.output)
                ));
            }
        }
        Expected::Timing(case) => {
            if !mentions_int(&numbers, i128::from(case.amount)) {
                return Err(format!(
                    "`hand-calc` working {working:?} does not arrive at the expected value {} {}",
                    case.amount,
                    case.unit.key()
                ));
            }
        }
    }
    if numbers.iter().all(|&n| is_result(n, expected)) {
        return Err(format!(
            "`hand-calc` working {working:?} shows no intermediate value: every number in \
             it is the expected result; show the input bytes or an intermediate step"
        ));
    }
    Ok(())
}

/// Whether `n` is (a spelling of) the value the working justifies: the
/// expected `raw` or `real` of a decode case, or the stored checksum of a
/// checksum state read big- or little-endian.
fn is_result(n: Num, expected: &Expected<'_>) -> bool {
    let one = [n];
    match expected {
        Expected::Range(range) => {
            let tolerance = range.tolerance.unwrap_or(0.0);
            mentions_raw(&one, range.raw_at_min)
                || mentions_raw(&one, range.raw_at_max)
                || mentions_float(&one, range.real_min, tolerance)
                || mentions_float(&one, range.real_max, tolerance)
        }
        Expected::Decode(case) => {
            let raw = mentions_raw(&one, case.raw);
            raw || case
                .real
                .is_some_and(|real| mentions_float(&one, real, case.tolerance.unwrap_or(0.0)))
        }
        Expected::Expr(case) => {
            case.value.is_finite()
                && mentions_float(&one, case.value, case.tolerance.unwrap_or(0.0))
        }
        Expected::Unit(case) => mentions_float(&one, case.expected, case.tolerance.unwrap_or(0.0)),
        Expected::Timing(case) => mentions_int(&one, i128::from(case.amount)),
        Expected::Checksum(ChecksumState { hex, .. })
        | Expected::Codec(CodecCase { output: hex, .. }) => {
            let bytes = hex.as_bytes();
            bytes.len() <= 16 && {
                let big = bytes
                    .iter()
                    .fold(0_u128, |acc, &b| (acc << 8) | u128::from(b));
                let little = bytes
                    .iter()
                    .rev()
                    .fold(0_u128, |acc, &b| (acc << 8) | u128::from(b));
                [big, little]
                    .into_iter()
                    .any(|v| i128::try_from(v).is_ok_and(|v| mentions_int(&one, v)))
            }
        }
    }
}

/// Whether `text` writes a formula: a binary arithmetic or bitwise operator
/// (`+ - * / % & | ^ << >> **`) with an operand on each side, at least one of
/// them a number or a parenthesised expression. A sign (`= -1`, `= +0.0`), a
/// hyphen between words (`big-endian`), an exponent (`1e-3`) and a comparison
/// (`0 < 128`) are not operators.
fn has_binary_operator(text: &str) -> bool {
    let b = text.as_bytes();
    let prev_non_space = |i: usize| {
        b[..i]
            .iter()
            .rev()
            .copied()
            .find(|c| !c.is_ascii_whitespace())
    };
    let next_non_space = |i: usize| b[i..].iter().copied().find(|c| !c.is_ascii_whitespace());
    let mut i = 0;
    while i < b.len() {
        let len = match (b[i], b.get(i + 1).copied()) {
            (b'<', Some(b'<')) | (b'>', Some(b'>')) | (b'*', Some(b'*')) => 2,
            (b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^', _) => 1,
            _ => {
                i += 1;
                continue;
            }
        };
        let exponent = matches!(b[i], b'+' | b'-')
            && i >= 2
            && matches!(b[i - 1], b'e' | b'E')
            && b[i - 2].is_ascii_digit()
            && b.get(i + 1).is_some_and(u8::is_ascii_digit);
        let before = prev_non_space(i);
        let after = next_non_space(i + len);
        let ends_operand = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'_' | b')' | b']');
        let starts_operand =
            |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'(' | b'-' | b'+');
        let numeric = before.is_some_and(|c| c.is_ascii_digit() || c == b')')
            || after.is_some_and(|c| c.is_ascii_digit() || c == b'(');
        if !exponent
            && before.is_some_and(ends_operand)
            && after.is_some_and(starts_operand)
            && numeric
        {
            return true;
        }
        i += len;
    }
    false
}

fn show_raw(raw: RawValue) -> String {
    match raw {
        RawValue::Int(n) => n.to_string(),
        RawValue::Float(x) => x.to_string(),
    }
}

fn show_hex(hex: &HexBytes) -> String {
    hex.as_bytes()
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A number written in a `working` string.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Num {
    Int(i128),
    Float(f64),
}

impl Num {
    /// As a float, for comparison with an `f64` expected value (integers
    /// above 2^53 round; exact integer comparison uses [`mentions_int`]).
    fn as_f64(self) -> f64 {
        match self {
            Self::Int(n) => n as f64,
            Self::Float(x) => x,
        }
    }
}

fn mentions_int(numbers: &[Num], expected: i128) -> bool {
    numbers.iter().any(|n| match *n {
        Num::Int(m) => m == expected,
        Num::Float(x) => x.fract() == 0.0 && Num::Int(expected).as_f64() == x,
    })
}

fn mentions_raw(numbers: &[Num], expected: RawValue) -> bool {
    match expected {
        RawValue::Int(n) => mentions_int(numbers, n),
        RawValue::Float(x) => mentions_float(numbers, x, 0.0),
    }
}

fn mentions_float(numbers: &[Num], expected: f64, tolerance: f64) -> bool {
    let slack = tolerance.max(1e-9 * expected.abs().max(1.0));
    numbers
        .iter()
        .any(|n| (n.as_f64() - expected).abs() <= slack)
}

/// Whether `working` names the non-finite `value`: the word `nan` (any case)
/// for NaN; `inf` or `infinity` for an infinity, preceded by `-` exactly when
/// it is negative.
fn mentions_non_finite(working: &str, value: f64) -> bool {
    let lower = working.to_lowercase();
    let bytes = lower.as_bytes();
    let words = |word: &'static str| {
        lower.match_indices(word).filter_map(move |(at, _)| {
            let before = at.checked_sub(1).map(|i| bytes[i]);
            let after = bytes.get(at + word.len()).copied();
            (!before.is_some_and(|b| b.is_ascii_alphanumeric())
                && !after.is_some_and(|b| b.is_ascii_alphanumeric()))
            .then_some(before)
        })
    };
    if value.is_nan() {
        return words("nan").next().is_some();
    }
    let negative = value < 0.0;
    words("inf")
        .chain(words("infinity"))
        .any(|before| (before == Some(b'-')) == negative)
}

fn mentions_checksum(working: &str, numbers: &[Num], hex: &HexBytes) -> bool {
    let bytes = hex.as_bytes();
    if bytes.len() <= 16 {
        let big = bytes
            .iter()
            .fold(0_u128, |acc, &b| (acc << 8) | u128::from(b));
        let little = bytes
            .iter()
            .rev()
            .fold(0_u128, |acc, &b| (acc << 8) | u128::from(b));
        let found = |value: u128| i128::try_from(value).is_ok_and(|v| mentions_int(numbers, v));
        if found(big) || found(little) {
            return true;
        }
    }
    let compact: String = working
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_')
        .collect::<String>()
        .to_uppercase();
    let digits: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
    compact.contains(&digits)
}

/// The numbers in `text`: decimal integers and floats (with optional
/// fraction and exponent), `0x` hex and `0b` binary integers, `_` separators
/// allowed; a leading `-` makes a number negative unless it follows a word
/// character or `)` (then it is a minus sign). Digits glued to a preceding
/// letter or `_` (`u16`, `x2`) are part of a word, not a number.
fn numbers(text: &str) -> Vec<Num> {
    let b = text.as_bytes();
    let is_word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let prev = i.checked_sub(1).map(|p| b[p]);
        if !b[i].is_ascii_digit() || prev.is_some_and(is_word) {
            i += 1;
            continue;
        }
        let negative = prev == Some(b'-')
            && !i
                .checked_sub(2)
                .map(|p| b[p])
                .is_some_and(|c| is_word(c) || c == b')');
        let at = |j: usize| b.get(j).copied().unwrap_or(0);
        let mut j = i;
        let radix_prefix = match (b[i], at(i + 1)) {
            (b'0', b'x' | b'X') if at(i + 2).is_ascii_hexdigit() => Some(16),
            (b'0', b'b' | b'B') if matches!(at(i + 2), b'0' | b'1') => Some(2),
            _ => None,
        };
        let value = if let Some(radix) = radix_prefix {
            j += 2;
            while j < b.len() && (char::from(b[j]).is_digit(radix) || b[j] == b'_') {
                j += 1;
            }
            let digits: String = text[i + 2..j].chars().filter(|&c| c != '_').collect();
            i128::from_str_radix(&digits, radix).ok().map(Num::Int)
        } else {
            while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'_') {
                j += 1;
            }
            let mut float = false;
            if at(j) == b'.' && at(j + 1).is_ascii_digit() {
                float = true;
                j += 1;
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
            }
            if matches!(at(j), b'e' | b'E') {
                let exp = if matches!(at(j + 1), b'+' | b'-') {
                    j + 2
                } else {
                    j + 1
                };
                if at(exp).is_ascii_digit() {
                    float = true;
                    j = exp;
                    while j < b.len() && b[j].is_ascii_digit() {
                        j += 1;
                    }
                }
            }
            let digits: String = text[i..j].chars().filter(|&c| c != '_').collect();
            if float {
                digits.parse::<f64>().ok().map(Num::Float)
            } else {
                digits.parse::<i128>().ok().map(Num::Int)
            }
        };
        if let Some(value) = value {
            out.push(match (negative, value) {
                (true, Num::Int(n)) => Num::Int(-n),
                (true, Num::Float(x)) => Num::Float(-x),
                (false, v) => v,
            });
        }
        // A number glued to trailing letters (`0A`, `4th`) ends there.
        while j < b.len() && is_word(b[j]) {
            j += 1;
        }
        i = j;
    }
    out
}

// ---------------------------------------------------------------------------
// Base mode
// ---------------------------------------------------------------------------

/// The expected-value part of a decode case.
fn decode_key(case: &DecodeCase) -> (u64, RawValue, Option<f64>, Option<f64>) {
    (case.offset, case.raw, case.real, case.tolerance)
}

/// The expected-value part of a `[range]`, floats by their bits.
type RangeKey = (RawValue, u64, RawValue, u64, Monotonicity, Option<u64>);

fn range_key(range: &RangeCase) -> RangeKey {
    (
        range.raw_at_min,
        range.real_min.to_bits(),
        range.raw_at_max,
        range.real_max.to_bits(),
        range.monotonicity,
        range.tolerance.map(f64::to_bits),
    )
}

/// The expected-value part of a checksum case.
type ChecksumKey<'a> = (
    &'a str,
    &'a toml::Table,
    u64,
    &'a HexBytes,
    bool,
    &'a HexBytes,
    bool,
);

/// The expected-value part of an expr case, floats by their bits so that a
/// `nan` case matches itself.
type ExprKey<'a> = (&'a Formula, u64, Vec<(&'a str, u64)>, u64, Option<u64>);

/// The [`ExprKey`] of `case`.
fn expr_key(case: &ExprCase) -> ExprKey<'_> {
    (
        &case.formula,
        case.x.to_bits(),
        case.params
            .iter()
            .map(|(id, v)| (id.as_str(), v.to_bits()))
            .collect(),
        case.value.to_bits(),
        case.tolerance.map(f64::to_bits),
    )
}

/// The expected-value part of a unit case, floats by their bits.
type UnitKey<'a> = (&'a str, &'a str, Option<&'a str>, u64, u64, Option<u64>);

/// The [`UnitKey`] of `case`.
fn unit_key(case: &UnitCase) -> UnitKey<'_> {
    (
        &case.from,
        &case.to,
        case.fuel.as_deref(),
        case.value.to_bits(),
        case.expected.to_bits(),
        case.tolerance.map(f64::to_bits),
    )
}

/// The expected-value part of a timing case: its name and the duration
/// (so rewriting `ms = 1` as `us = 1000` is not a change).
type TimingKey<'a> = (&'a str, std::time::Duration);

/// The [`TimingKey`] of `case`.
fn timing_key(case: &TimingCase) -> TimingKey<'_> {
    (&case.name, case.duration())
}

/// The expected-value part of a codec case: everything but its source and
/// note.
type CodecKey<'a> = (&'a str, &'a toml::Table, &'a HexBytes, &'a HexBytes);

/// The [`CodecKey`] of `case`.
fn codec_key(case: &CodecCase) -> CodecKey<'_> {
    (&case.op, &case.params, &case.input, &case.output)
}

fn checksum_key(case: &ChecksumCase) -> ChecksumKey<'_> {
    (
        &case.algorithm,
        &case.params,
        case.offset,
        &case.before.hex,
        case.before.valid,
        &case.after.hex,
        case.after.valid,
    )
}

/// Indices of `base` items with no identical item left in `head` (multiset
/// matching, so order does not matter).
fn unmatched<T: PartialEq>(base: &[T], head: &[T]) -> Vec<usize> {
    let mut used = vec![false; head.len()];
    base.iter()
        .enumerate()
        .filter_map(
            |(index, item)| match (0..head.len()).find(|&j| !used[j] && head[j] == *item) {
                Some(j) => {
                    used[j] = true;
                    None
                }
                None => Some(index),
            },
        )
        .collect()
}

/// Compares a fixture with its base version: `Some` violation when expected
/// values changed and no new changelog entry was added (module docs).
#[must_use]
pub fn check_change(path: &Path, base_label: &str, base: &Kat, head: &Kat) -> Option<Violation> {
    let mut changes = Vec::new();
    if base.input != head.input {
        changes.push("the [input] changed".to_owned());
    }
    match (&base.kind, &head.kind) {
        (
            KatKind::Decode {
                definition: base_def,
                cases: base_cases,
                range: base_range,
            },
            KatKind::Decode {
                definition: head_def,
                cases: head_cases,
                range: head_range,
            },
        ) => {
            if base_def != head_def {
                changes.push("the [definition] changed".to_owned());
            }
            if let Some(base_range) = base_range
                && head_range.as_ref().map(range_key) != Some(range_key(base_range))
            {
                changes.push(format!(
                    "the [range] (min {} at raw {}, max {} at raw {}, {}) was changed or removed",
                    base_range.real_min,
                    show_raw(base_range.raw_at_min),
                    base_range.real_max,
                    show_raw(base_range.raw_at_max),
                    base_range.monotonicity.name()
                ));
            }
            let base_keys: Vec<_> = base_cases.iter().map(decode_key).collect();
            let head_keys: Vec<_> = head_cases.iter().map(decode_key).collect();
            for index in unmatched(&base_keys, &head_keys) {
                let case = &base_cases[index];
                let mut was = format!("raw = {}", show_raw(case.raw));
                if let Some(real) = case.real {
                    was.push_str(&format!(", real = {real}"));
                }
                changes.push(format!(
                    "{} ({was}) was changed or removed",
                    decode_location(index, case.offset)
                ));
            }
        }
        (
            KatKind::Checksum {
                definition: base_def,
                cases: base_cases,
            },
            KatKind::Checksum {
                definition: head_def,
                cases: head_cases,
            },
        ) => {
            if base_def != head_def {
                changes.push("the [definition] changed".to_owned());
            }
            let base_keys: Vec<_> = base_cases.iter().map(checksum_key).collect();
            let head_keys: Vec<_> = head_cases.iter().map(checksum_key).collect();
            for index in unmatched(&base_keys, &head_keys) {
                let case = &base_cases[index];
                changes.push(format!(
                    "[[checksum]] #{} ({} at offset {}: before {}, after {}) was changed or removed",
                    index + 1,
                    case.algorithm,
                    case.offset,
                    show_hex(&case.before.hex),
                    show_hex(&case.after.hex)
                ));
            }
        }
        (KatKind::Expr { cases: base_cases }, KatKind::Expr { cases: head_cases }) => {
            let base_keys: Vec<_> = base_cases.iter().map(expr_key).collect();
            let head_keys: Vec<_> = head_cases.iter().map(expr_key).collect();
            for index in unmatched(&base_keys, &head_keys) {
                let case = &base_cases[index];
                changes.push(format!(
                    "{} (x = {}, value = {}) was changed or removed",
                    expr_location(index, &case.formula),
                    case.x,
                    case.value
                ));
            }
        }
        (KatKind::Unit { cases: base_cases }, KatKind::Unit { cases: head_cases }) => {
            let base_keys: Vec<_> = base_cases.iter().map(unit_key).collect();
            let head_keys: Vec<_> = head_cases.iter().map(unit_key).collect();
            for index in unmatched(&base_keys, &head_keys) {
                let case = &base_cases[index];
                changes.push(format!(
                    "{} (expected = {}, tolerance = {:?}) was changed or removed",
                    unit_location(index, &case.value, &case.from, &case.to),
                    case.expected,
                    case.tolerance
                ));
            }
        }
        (KatKind::Timing { cases: base_cases }, KatKind::Timing { cases: head_cases }) => {
            let base_keys: Vec<_> = base_cases.iter().map(timing_key).collect();
            let head_keys: Vec<_> = head_cases.iter().map(timing_key).collect();
            for index in unmatched(&base_keys, &head_keys) {
                let case = &base_cases[index];
                changes.push(format!(
                    "{} ({} = {}) was changed or removed",
                    timing_location(index, &case.name),
                    case.unit.key(),
                    case.amount
                ));
            }
        }
        (KatKind::Codec { cases: base_cases }, KatKind::Codec { cases: head_cases }) => {
            let base_keys: Vec<_> = base_cases.iter().map(codec_key).collect();
            let head_keys: Vec<_> = head_cases.iter().map(codec_key).collect();
            for index in unmatched(&base_keys, &head_keys) {
                let case = &base_cases[index];
                changes.push(format!(
                    "{} (output {}) was changed or removed",
                    codec_location(index, &case.op),
                    show_hex(&case.output)
                ));
            }
        }
        _ => changes.push(
            "the fixture switched between [[decode]], [[checksum]], [[expr]], [[unit]], \
             [[timing]] and [[codec]] cases"
                .to_owned(),
        ),
    }
    if changes.is_empty() {
        return None;
    }
    let (added, dropped) = changelog_delta(&base.changelog, &head.changelog);
    if added > dropped {
        return None;
    }
    let mut why = String::from("no new [[changelog]] entry says why");
    if dropped > 0 {
        why.push_str(&format!(
            " ({dropped} entr{} of the base changelog {} edited or removed, so as many \
             entries in the new changelog only replace old ones: the changelog is \
             append-only)",
            if dropped == 1 { "y" } else { "ies" },
            if dropped == 1 { "was" } else { "were" },
        ));
    }
    Some(Violation::new(
        path,
        None,
        Rule::ChangedWithoutChangelog,
        format!(
            "expected values changed since {base_label} ({}) but {why}; add one: \
             [[changelog]] date = YYYY-MM-DD, reason = \"why the old value was wrong\"",
            changes.join("; ")
        ),
    ))
}

/// How the changelog moved from `base` to `head`: `(added, dropped)`, where
/// `added` counts the `head` entries that are not in `base` (multiset) and
/// give a reason, i.e. one that is not blank, not the reason of a `base`
/// entry and not the reason of an earlier added entry; `dropped` counts the
/// `base` entries no longer in `head` (edited or removed). A change is
/// explained when `added > dropped`: rewording, re-dating, copying or
/// replacing an old entry adds nothing.
fn changelog_delta(base: &[ChangelogEntry], head: &[ChangelogEntry]) -> (usize, usize) {
    let mut reasons: Vec<&str> = base.iter().map(|e| e.reason.trim()).collect();
    let mut added = 0;
    for index in unmatched(head, base) {
        let reason = head[index].reason.trim();
        if !reason.is_empty() && !reasons.contains(&reason) {
            reasons.push(reason);
            added += 1;
        }
    }
    (added, unmatched(base, head).len())
}

/// Base versions from git: the merge-base of a revision and `HEAD`.
///
/// A fixture renamed or moved since the base (git rename detection between
/// the base commit and the working tree, `git diff --find-renames`) is
/// compared with its base version under the old path, so a rename cannot
/// hide a changed value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitBase {
    toplevel: PathBuf,
    commit: String,
    label: String,
    /// `(new path, old path)` of every file renamed since the base, relative
    /// to `toplevel` with `/` separators.
    renames: Vec<(String, String)>,
}

impl GitBase {
    /// Resolves `rev` in the git repository containing `dir`. The base is
    /// the merge-base of `rev` and `HEAD` (so a branch is compared with where
    /// it forked, not with later commits on `rev`), or `rev` itself when they
    /// share no history.
    ///
    /// # Errors
    ///
    /// [`ValidateError::Git`] when `dir` is not in a git repository or `rev`
    /// does not name a commit.
    pub fn resolve(dir: &Path, rev: &str) -> Result<Self, ValidateError> {
        let toplevel = PathBuf::from(git(dir, &["rev-parse", "--show-toplevel"])?.trim());
        let verified = git(
            &toplevel,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{rev}^{{commit}}"),
            ],
        )
        .map_err(|error| match error {
            ValidateError::Git { args, .. } => ValidateError::Git {
                args,
                message: format!("{rev:?} does not name a commit"),
            },
            other => other,
        })?
        .trim()
        .to_owned();
        let commit = git(&toplevel, &["merge-base", &verified, "HEAD"])
            .map_or(verified, |out| out.trim().to_owned());
        let short: String = commit.chars().take(12).collect();
        let renames = renames_since(&toplevel, &commit)?;
        Ok(Self {
            toplevel,
            label: format!("{rev} ({short})"),
            commit,
            renames,
        })
    }

    /// The base for a CI push whose previous head was `before` (GitHub's
    /// `github.event.before`):
    ///
    /// * `before` itself ([`Self::resolve`]) when it names a commit in the
    ///   repository: an ordinary push is compared with what the branch held
    ///   before it;
    /// * otherwise (empty or all zeros on a new branch, or a force push whose
    ///   old head is no longer fetched) the point where `HEAD` left the rest
    ///   of the repository: the parent of the oldest commit of `HEAD` that is
    ///   on no remote-tracking branch other than ones that already contain
    ///   `HEAD` (the pushed branch itself). So every commit the push brings
    ///   is checked. When that oldest commit is a root commit (no other
    ///   branches), the root itself.
    ///
    /// # Errors
    ///
    /// [`ValidateError::Git`] when `dir` is not in a git repository or git
    /// fails.
    pub fn for_push(dir: &Path, before: &str) -> Result<Self, ValidateError> {
        let before = before.trim();
        let toplevel = PathBuf::from(git(dir, &["rev-parse", "--show-toplevel"])?.trim());
        let known = !before.is_empty()
            && !before.bytes().all(|b| b == b'0')
            && git(
                &toplevel,
                &["cat-file", "-e", &format!("{before}^{{commit}}")],
            )
            .is_ok();
        if known {
            let mut base = Self::resolve(&toplevel, before)?;
            base.label = format!("the commit before the push, {}", base.label);
            return Ok(base);
        }
        let refs = git(
            &toplevel,
            &["for-each-ref", "--format=%(refname)", "refs/remotes"],
        )?;
        let others: Vec<&str> = refs
            .lines()
            .map(str::trim)
            .filter(|r| {
                !r.is_empty()
                    && git(&toplevel, &["merge-base", "--is-ancestor", "HEAD", r]).is_err()
            })
            .collect();
        let mut args = vec!["rev-list", "--topo-order", "--reverse", "HEAD"];
        if !others.is_empty() {
            args.push("--not");
            args.extend(others.iter().copied());
        }
        let unique = git(&toplevel, &args)?;
        // Not empty in practice: no ref in `others` contains HEAD, so HEAD
        // itself is listed. Should it be, nothing is new: compare with HEAD.
        let Some(oldest) = unique.lines().next().map(str::trim) else {
            return Self::resolve(&toplevel, "HEAD");
        };
        let parent = format!("{oldest}^");
        let has_parent = git(
            &toplevel,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{parent}^{{commit}}"),
            ],
        )
        .is_ok();
        let (rev, why) = if has_parent {
            (parent, "where the pushed commits leave the other branches")
        } else {
            (oldest.to_owned(), "the root of the pushed commits")
        };
        let mut base = Self::resolve(&toplevel, &rev)?;
        let short: String = base.commit.chars().take(12).collect();
        base.label = format!("{why} ({short})");
        Ok(base)
    }

    /// The full id of the base commit.
    #[must_use]
    pub fn commit(&self) -> &str {
        &self.commit
    }
}

impl BaseSource for GitBase {
    fn label(&self) -> &str {
        &self.label
    }

    fn read(&self, path: &Path) -> Result<Option<String>, ValidateError> {
        let canonical = |p: &Path| {
            p.canonicalize().map_err(|source| ValidateError::Io {
                path: p.to_path_buf(),
                source,
            })
        };
        // The file itself may not exist (yet); its directory must.
        let file = match (path.parent(), path.file_name()) {
            (Some(dir), Some(name)) => {
                let dir = if dir.as_os_str().is_empty() {
                    Path::new(".")
                } else {
                    dir
                };
                canonical(dir)?.join(name)
            }
            _ => canonical(path)?,
        };
        let top = canonical(&self.toplevel)?;
        let Ok(relative) = file.strip_prefix(&top) else {
            return Ok(None); // outside the repository: nothing to compare with
        };
        let relative: Vec<String> = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        let relative = relative.join("/");
        let relative = self
            .renames
            .iter()
            .find(|(new, _)| *new == relative)
            .map_or(relative.as_str(), |(_, old)| old.as_str());
        let spec = format!("{}:{relative}", self.commit);
        if git(&top, &["cat-file", "-e", &spec]).is_err() {
            return Ok(None);
        }
        git(&top, &["show", &spec]).map(Some)
    }
}

/// `(new path, old path)` of every file git sees as renamed between `commit`
/// and the working tree (committed, staged or unstaged changes to tracked
/// files; an untracked copy is not seen until it is added).
fn renames_since(toplevel: &Path, commit: &str) -> Result<Vec<(String, String)>, ValidateError> {
    let out = git(
        toplevel,
        &[
            "-c",
            "core.quotePath=false",
            "diff",
            "--find-renames",
            "--name-status",
            "--diff-filter=R",
            "-z",
            commit,
            "--",
        ],
    )?;
    let mut fields = out.split('\0');
    let mut renames = Vec::new();
    while let Some(status) = fields.next() {
        if !status.starts_with('R') {
            continue;
        }
        if let (Some(old), Some(new)) = (fields.next(), fields.next()) {
            renames.push((new.to_owned(), old.to_owned()));
        }
    }
    Ok(renames)
}

fn git(dir: &Path, args: &[&str]) -> Result<String, ValidateError> {
    let failed = |message: String| ValidateError::Git {
        args: args.join(" "),
        message,
    };
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|error| failed(format!("cannot run git: {error}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(failed(if stderr.is_empty() {
            format!("exit status {}", output.status)
        } else {
            stderr
        }));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

// ---------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------

/// Usage line of [`cli`].
pub const USAGE: &str =
    "usage: validate-fixtures [--base <git-rev> | --push-before <git-rev>] [PATH...]";

/// The `validate-fixtures [--base <git-rev> | --push-before <git-rev>]
/// [PATH...]` command line.
///
/// * `PATH...`: fixture files or directories (every `*.toml` below them),
///   relative to `cwd`; defaults to [`default_paths`].
/// * `--base <git-rev>` (or `--base=<git-rev>`): base mode, against
///   [`GitBase::resolve`] in the repository containing `cwd`.
/// * `--push-before <git-rev>` (or `--push-before=<git-rev>`): base mode for
///   a CI push whose previous head was `<git-rev>` (may be empty or all
///   zeros), against [`GitBase::for_push`].
///
/// Violations go to `stderr` (one line each, file relative to `cwd`, case,
/// rule), the summary to `stdout`. Returns the exit status: 0 when every
/// fixture passes, 1 when any violation is found, 2 on a usage error or when
/// fixtures or the base cannot be read.
pub fn cli(
    args: impl IntoIterator<Item = OsString>,
    cwd: &Path,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    match run_cli(args, cwd, stdout, stderr) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(message) => {
            let _ = writeln!(stderr, "validate-fixtures: error: {message}");
            2
        }
    }
}

fn run_cli(
    args: impl IntoIterator<Item = OsString>,
    cwd: &Path,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<bool, String> {
    let mut base = None;
    let mut push_before = None;
    let mut paths = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => {
                let _ = writeln!(stdout, "{USAGE}");
                return Ok(true);
            }
            Some("--base") => {
                let rev = args
                    .next()
                    .ok_or_else(|| format!("--base needs a git revision\n{USAGE}"))?;
                base = Some(rev.to_string_lossy().into_owned());
            }
            Some(other) if other.starts_with("--base=") => {
                base = Some(other["--base=".len()..].to_owned());
            }
            Some("--push-before") => {
                let rev = args
                    .next()
                    .ok_or_else(|| format!("--push-before needs a git revision\n{USAGE}"))?;
                push_before = Some(rev.to_string_lossy().into_owned());
            }
            Some(other) if other.starts_with("--push-before=") => {
                push_before = Some(other["--push-before=".len()..].to_owned());
            }
            Some(other) if other.starts_with('-') => {
                return Err(format!("unknown option {other}\n{USAGE}"));
            }
            _ => paths.push(cwd.join(arg)),
        }
    }
    if paths.is_empty() {
        paths = default_paths();
    }
    let files = discover(&paths).map_err(|e| e.to_string())?;
    if files.is_empty() {
        let shown: Vec<String> = paths.iter().map(|p| shown(p, cwd)).collect();
        return Err(format!(
            "no fixture files (*.toml) under {}",
            shown.join(", ")
        ));
    }
    let base = match (base, push_before) {
        (Some(_), Some(_)) => {
            return Err(format!(
                "--base and --push-before cannot be used together\n{USAGE}"
            ));
        }
        (Some(rev), None) => Some(GitBase::resolve(cwd, &rev).map_err(|e| e.to_string())?),
        (None, Some(before)) => Some(GitBase::for_push(cwd, &before).map_err(|e| e.to_string())?),
        (None, None) => None,
    };
    let report = validate_files(&files, base.as_ref().map(|b| b as &dyn BaseSource))
        .map_err(|e| e.to_string())?;
    let compared = base.as_ref().map_or_else(String::new, |b| {
        format!(", {} compared with {}", report.compared, b.label())
    });
    if report.violations.is_empty() {
        let _ = writeln!(
            stdout,
            "validate-fixtures: ok ({} fixture(s){compared})",
            report.files
        );
        return Ok(true);
    }
    for violation in &report.violations {
        let mut violation = violation.clone();
        violation.path = PathBuf::from(shown(&violation.path, cwd));
        let _ = writeln!(stderr, "validate-fixtures: {violation}");
    }
    let _ = writeln!(
        stderr,
        "validate-fixtures: {} violation(s) in {} fixture(s){compared}",
        report.violations.len(),
        report.files
    );
    Ok(false)
}

/// `path` relative to `cwd` (with `/` separators) when it is below it, so
/// messages name `fixtures/kat/x.toml` rather than an absolute path.
fn shown(path: &Path, cwd: &Path) -> String {
    let relative = cwd
        .canonicalize()
        .ok()
        .zip(path.canonicalize().ok())
        .and_then(|(cwd, full)| full.strip_prefix(cwd).ok().map(Path::to_path_buf));
    match relative {
        Some(relative) => relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_tokenizes_working() {
        assert_eq!(
            numbers("0x01F4 = 1*256 + 244 = 500; 500 * 0.1 = 50.0"),
            vec![
                Num::Int(0x1F4),
                Num::Int(1),
                Num::Int(256),
                Num::Int(244),
                Num::Int(500),
                Num::Int(500),
                Num::Float(0.1),
                Num::Float(50.0)
            ]
        );
        assert_eq!(numbers("u16 x2 = -1"), vec![Num::Int(-1)]);
        assert_eq!(
            numbers("10-1 = 9"),
            vec![Num::Int(10), Num::Int(1), Num::Int(9)]
        );
        assert_eq!(numbers("(3)-1"), vec![Num::Int(3), Num::Int(1)]);
        assert_eq!(
            numbers("1e-3 0b1010_0101 4..6"),
            vec![Num::Float(1e-3), Num::Int(0xA5), Num::Int(4), Num::Int(6)]
        );
        assert_eq!(
            numbers("00 0A 1_000"),
            vec![Num::Int(0), Num::Int(0), Num::Int(1000)]
        );
        assert_eq!(numbers("no digits"), vec![]);
    }

    #[test]
    fn binary_operators_are_told_from_signs_and_hyphens() {
        for formula in [
            "1*256 + 244",
            "0xFF*0x100",
            "65336 - 65536",
            "0x1F & 0xE0",
            "96 >> 5",
            "2^-149",
            "-(1 + 0) * 2",
            "14.7/1.5",
            "(3)-1",
            "x * 0.1",
        ] {
            assert!(has_binary_operator(formula), "{formula:?}");
        }
        for not_formula in [
            "500",
            "x = -1",
            "value = +0.0",
            "value = -0.0",
            "bytes 00 00 little-endian = 0",
            "sign-extend = 1e-3",
            "0 < 128, 1 > 0",
            "imm20[19:16] = 0x10",
            "- 1",
            "1 +",
            "a + b",
        ] {
            assert!(!has_binary_operator(not_formula), "{not_formula:?}");
        }
    }

    #[test]
    fn java_is_a_word_match() {
        assert!(mentions_java("RomRaider Java 0.5.9"));
        assert!(mentions_java("java"));
        assert!(!mentions_java("JavaScript reference"));
        assert!(!mentions_java("RomRaider ECU XML"));
    }

    #[test]
    fn unmatched_is_a_multiset_difference() {
        assert_eq!(unmatched(&[1, 2, 2], &[2, 1, 3]), vec![2]);
        assert_eq!(unmatched::<i32>(&[], &[1]), Vec::<usize>::new());
        assert_eq!(unmatched(&[1], &[]), vec![0]);
    }
}
