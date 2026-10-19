//! Coverage gate (`cargo xtask coverage-gate`, story US-RR-4, task US-RR-4-5;
//! PROJECT.md Testing Strategy T8, D-003).
//!
//! Reads the lcov report written by `cargo llvm-cov nextest --lcov` and fails
//! when a gated crate's line coverage is below the minimum, or below the same
//! crate's line coverage in a base report (`--base-lcov`, CI: the base
//! branch's report, see `.github/workflows/coverage.yml`).
//!
//! Rules:
//!
//! * A file belongs to crate `<name>` when its path contains
//!   `crates/<name>/src/`. Only `src/` counts: integration tests, examples and
//!   benches are not the code under test. A crate's line coverage is the sum
//!   of its files' `LH:` (lines hit) over the sum of their `LF:` (lines
//!   found), the numbers `cargo llvm-cov report` prints; a record without
//!   `LF:`/`LH:` is counted from its `DA:` lines instead.
//! * Threshold: `hit * 100 >= min * found`, in integers (no rounding).
//! * Regression: the head ratio may not be lower than the base ratio by more
//!   than `--max-drop` percentage points (default 0: any drop fails), in
//!   integers: fails when
//!   `head_hit / head_found < base_hit / base_found - max_drop / 100`.
//!   The tolerance exists only for measurement noise. Coverage of the same
//!   commit is not perfectly repeatable: a test whose threads race (e.g.
//!   rr-core's `shared_manual_clock_readers_see_monotonic_time_while_driver_advances`,
//!   whose reader loop body may never run) hits a few lines in some runs and
//!   not others; measured locally, rr-core moved by 3 of 2176 lines
//!   (0.14 points) in about one run in eight. CI uses the smallest tolerance
//!   above that noise; see `.github/workflows/coverage.yml`. Deleting
//!   well-tested code from a crate below 100% lowers its ratio and can fail
//!   too, which is the "must not regress" rule as written. Skipped when the
//!   base report has no instrumented lines for the crate (the crate had no
//!   code, or did not exist, on the base).
//! * A crate with no instrumented lines in the head report passes only if
//!   its `src/` holds no function at all (a crate that is still only a doc
//!   comment, as rr-rom and rr-comms are at the time of writing; nothing in
//!   it can execute). If any `.rs` file under `src/` contains a `fn`
//!   token, the crate has code and missing coverage data fails the gate. A
//!   file that does not lex counts as having code (fail closed), and a crate
//!   with no `src/` directory is an error.
//! * A head report with no `SF:` record at all is an error: it means the
//!   coverage run produced nothing, not that everything is covered.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;
use std::str::FromStr as _;

use anyhow::{Context as _, Result, bail};
use proc_macro2::{TokenStream, TokenTree};

/// The crates CI gates (PROJECT.md Testing Strategy, T8).
pub const GATED_CRATES: &[&str] = &["rr-core", "rr-rom", "rr-comms"];

/// The CI minimum line coverage, in percent.
pub const DEFAULT_MIN_PERCENT: u32 = 90;

/// Per-file line counts parsed from an lcov report, by `SF:` path.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Lcov {
    pub files: BTreeMap<String, LineCount>,
}

/// One `SF:` record being read.
#[derive(Default)]
struct Record {
    path: String,
    /// `DA:` hit counts by line (the maximum when a line repeats).
    lines: BTreeMap<u32, u64>,
    lf: Option<u64>,
    lh: Option<u64>,
}

impl Record {
    /// `LF:` / `LH:` when the record has both (cargo-llvm-cov always writes
    /// them, and they are what `cargo llvm-cov report` prints), else counted
    /// from the `DA:` lines.
    fn count(&self) -> Result<LineCount> {
        let count = match (self.lf, self.lh) {
            (Some(found), Some(hit)) => LineCount { found, hit },
            _ => LineCount {
                found: self.lines.len() as u64,
                hit: self.lines.values().filter(|&&h| h > 0).count() as u64,
            },
        };
        if count.hit > count.found {
            bail!(
                "{}: LH {} is above LF {}",
                self.path,
                count.hit,
                count.found
            );
        }
        Ok(count)
    }
}

fn number<T: std::str::FromStr>(n: usize, line: &str, field: &str) -> Result<T> {
    field
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("line {n}: malformed `{line}`"))
}

/// Parses the `SF:`, `DA:`, `LF:`, `LH:` and `end_of_record` lines of an lcov
/// report; every other record type is ignored. A malformed line, a `DA:` /
/// `LF:` / `LH:` outside an `SF:` record, the same `SF:` path twice (merge
/// reports with llvm-cov, not by concatenation), or a report with no `SF:`
/// record is an error.
pub fn parse_lcov(text: &str) -> Result<Lcov> {
    let mut lcov = Lcov::default();
    let mut current: Option<Record> = None;
    let finish = |rec: Record, lcov: &mut Lcov| -> Result<()> {
        let count = rec.count()?;
        if lcov.files.insert(rec.path.clone(), count).is_some() {
            bail!("{}: more than one `SF:` record", rec.path);
        }
        Ok(())
    };
    for (n, line) in text.lines().enumerate() {
        let n = n + 1;
        let line = line.trim();
        if let Some(path) = line.strip_prefix("SF:") {
            if let Some(rec) = current.take() {
                finish(rec, &mut lcov)?;
            }
            current = Some(Record {
                path: path.trim().to_owned(),
                ..Record::default()
            });
            continue;
        }
        if line == "end_of_record" {
            if let Some(rec) = current.take() {
                finish(rec, &mut lcov)?;
            }
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if !matches!(key, "DA" | "LF" | "LH") {
            continue;
        }
        let Some(rec) = current.as_mut() else {
            bail!("line {n}: `{key}:` outside an `SF:` record");
        };
        match key {
            "DA" => {
                let mut parts = value.split(',');
                let (Some(ln), Some(hits)) = (parts.next(), parts.next()) else {
                    bail!("line {n}: malformed `{line}`");
                };
                let ln: u32 = number(n, line, ln)?;
                let hits: u64 = number(n, line, hits)?;
                let entry = rec.lines.entry(ln).or_insert(0);
                *entry = (*entry).max(hits);
            }
            "LF" => rec.lf = Some(number(n, line, value)?),
            _ => rec.lh = Some(number(n, line, value)?),
        }
    }
    if let Some(rec) = current.take() {
        finish(rec, &mut lcov)?;
    }
    if lcov.files.is_empty() {
        bail!("no `SF:` records: the coverage report is empty");
    }
    Ok(lcov)
}

/// The crate a source path belongs to: `<name>` for a path containing
/// `crates/<name>/src/` (the last such match; `\` is treated as `/`).
#[must_use]
pub fn crate_of(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    let comps: Vec<&str> = normalized.split('/').collect();
    let i = comps
        .windows(4)
        .rposition(|w| w[0] == "crates" && !w[1].is_empty() && w[2] == "src")?;
    Some(comps[i + 1].to_owned())
}

/// Instrumented and hit line counts.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LineCount {
    pub found: u64,
    pub hit: u64,
}

impl LineCount {
    /// Whether `hit / found >= min_percent / 100` (false when nothing was found).
    #[must_use]
    pub fn meets(self, min_percent: u32) -> bool {
        self.found > 0
            && u128::from(self.hit) * 100 >= u128::from(min_percent) * u128::from(self.found)
    }

    /// Whether this ratio is lower than `base`'s by more than
    /// `max_drop_hundredths` hundredths of a percentage point.
    #[must_use]
    pub fn regressed_from(self, base: Self, max_drop_hundredths: u32) -> bool {
        let (h, hf) = (u128::from(self.hit), u128::from(self.found));
        let (b, bf) = (u128::from(base.hit), u128::from(base.found));
        h * bf * 10_000 + u128::from(max_drop_hundredths) * hf * bf < b * hf * 10_000
    }
}

impl fmt::Display for LineCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.found == 0 {
            return write!(f, "n/a (0 lines)");
        }
        // Display only; the gate itself compares in integers.
        let pct = self.hit as f64 * 100.0 / self.found as f64;
        write!(f, "{pct:.2}% ({}/{})", self.hit, self.found)
    }
}

/// Line counts of the files of crate `name` in `lcov`, summed.
#[must_use]
pub fn crate_lines(lcov: &Lcov, name: &str) -> LineCount {
    lcov.files
        .iter()
        .filter(|(path, _)| crate_of(path).as_deref() == Some(name))
        .fold(LineCount::default(), |acc, (_, c)| LineCount {
            found: acc.found + c.found,
            hit: acc.hit + c.hit,
        })
}

/// One gated crate's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub krate: String,
    pub head: LineCount,
    pub base: Option<LineCount>,
    /// Why the crate passed without a percentage (no code yet, no base).
    pub notes: Vec<String>,
}

/// The gate's result: one row per gated crate, and every failure.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub rows: Vec<Row>,
    pub problems: Vec<String>,
}

/// Applies the gate. `has_code(crate)` says whether the crate's `src/` holds
/// any function ([`src_has_functions`] in the real gate); it is only asked
/// about crates with no instrumented lines.
pub fn check(
    head: &Lcov,
    base: Option<&Lcov>,
    crates: &[String],
    min_percent: u32,
    max_drop_hundredths: u32,
    has_code: impl Fn(&str) -> Result<bool>,
) -> Result<Report> {
    let mut report = Report::default();
    for name in crates {
        let h = crate_lines(head, name);
        let b = base.map(|b| crate_lines(b, name));
        let mut notes = Vec::new();
        if h.found == 0 {
            if has_code(name)? {
                report.problems.push(format!(
                    "{name}: crates/{name}/src has functions but the coverage report has no \
                     instrumented lines for it (was it built and tested under cargo llvm-cov?)"
                ));
            } else {
                notes.push("no functions in src/ yet; nothing to cover".to_owned());
            }
        } else if !h.meets(min_percent) {
            report.problems.push(format!(
                "{name}: line coverage {h} is below the {min_percent}% minimum"
            ));
        }
        match b {
            Some(b) if b.found > 0 && h.found > 0 => {
                if h.regressed_from(b, max_drop_hundredths) {
                    report.problems.push(format!(
                        "{name}: line coverage regressed from {b} on the base to {h} \
                         (allowed drop: {})",
                        Points(max_drop_hundredths)
                    ));
                }
            }
            Some(_) if h.found > 0 => {
                notes.push("no instrumented lines on the base; regression check skipped".into());
            }
            _ => {}
        }
        report.rows.push(Row {
            krate: name.clone(),
            head: h,
            base: b,
            notes,
        });
    }
    Ok(report)
}

/// Percentage points in hundredths, as `--max-drop` takes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Points(pub u32);

impl fmt::Display for Points {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{:02} points", self.0 / 100, self.0 % 100)
    }
}

/// Parses a percentage-point amount with at most two decimals (`0`, `0.2`,
/// `0.25`, `1`) into hundredths; at most 100 points.
pub fn parse_points(text: &str) -> Result<u32> {
    let t = text.trim();
    let (int, frac) = t.split_once('.').unwrap_or((t, ""));
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(int) || (t.contains('.') && !digits(frac)) || frac.len() > 2 {
        bail!("`{text}` is not a percentage-point amount with at most two decimals");
    }
    let total = int
        .parse::<u32>()
        .ok()
        .and_then(|i| i.checked_mul(100))
        .and_then(|i| i.checked_add(format!("{frac:0<2}").parse().unwrap_or(0)))
        .filter(|&t| t <= 10_000);
    total.with_context(|| format!("`{text}` is above 100 points"))
}

/// Whether any `.rs` file under `src_dir` (recursively) contains a `fn`
/// token. Comments and string literals never match; a file that does not lex
/// counts as having code. A missing `src_dir` is an error.
pub fn src_has_functions(src_dir: &Path) -> Result<bool> {
    if !src_dir.is_dir() {
        bail!("{} is not a directory (unknown crate?)", src_dir.display());
    }
    let mut stack = vec![src_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("cannot read {}", dir.display()))? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = fs::read_to_string(&path)
                    .with_context(|| format!("cannot read {}", path.display()))?;
                if TokenStream::from_str(&text).map_or(true, has_fn) {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn has_fn(tokens: TokenStream) -> bool {
    tokens.into_iter().any(|t| match t {
        TokenTree::Ident(i) => i == "fn",
        TokenTree::Group(g) => has_fn(g.stream()),
        TokenTree::Punct(_) | TokenTree::Literal(_) => false,
    })
}

/// Reads and parses the lcov file at `path`.
pub fn read_lcov(path: &Path) -> Result<Lcov> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    parse_lcov(&text).with_context(|| format!("{}", path.display()))
}

/// The whole gate on files: `lcov` (head), optional `base_lcov`, crates
/// looked up under `<root>/crates/<name>/src`.
pub fn run(
    root: &Path,
    lcov: &Path,
    base_lcov: Option<&Path>,
    crates: &[String],
    min_percent: u32,
    max_drop_hundredths: u32,
) -> Result<Report> {
    if min_percent > 100 {
        bail!("--min {min_percent} is above 100");
    }
    if crates.is_empty() {
        bail!("no crates to gate");
    }
    let head = read_lcov(lcov)?;
    let base = base_lcov.map(read_lcov).transpose()?;
    check(
        &head,
        base.as_ref(),
        crates,
        min_percent,
        max_drop_hundredths,
        |name| src_has_functions(&root.join("crates").join(name).join("src")),
    )
}
