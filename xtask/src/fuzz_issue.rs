//! `cargo xtask fuzz file-issues` (US-RR-5-6): turns the failures of a
//! nightly fuzz run into GitHub issues labelled [`LABEL`], one open issue
//! per target (and feature set), commented on when the target fails again.
//!
//! The nightly workflow (`.github/workflows/fuzz.yml`) uploads, for every
//! failing target, an artifact `fuzz-failure-<target>` holding the inputs
//! libFuzzer saved (`crash-*`, `timeout-*`, `oom-*`, ...), the run log
//! [`LOG_FILE`] and the features the run used ([`FEATURES_FILE`]). The
//! `report` job downloads them into one directory and runs this command on
//! it. Everything here is plain data in, `gh` argument vectors out:
//!
//! * [`collect`] reads the directory. Directory names must name a target of
//!   the fuzz crate, features must be features it declares, and saved-input
//!   file names must be plain (`[A-Za-z0-9._-]`); anything else is skipped
//!   with a warning, so nothing the run wrote reaches a title unchecked.
//! * [`issue_title`] is the dedupe key; [`existing_issue`] finds an open
//!   issue with exactly that title in `gh issue list --json` output.
//! * [`issue_body`] renders the report (log excerpt fenced so it cannot
//!   become markup), and [`plan`] gives the `gh` calls: list, ensure the
//!   label, then `issue create` or `issue comment`. Bodies go through
//!   `--body-file`, and `gh` is run without a shell, so crash data is never
//!   interpolated into a command line.
//!
//! `--dry-run` prints the calls (and bodies) instead of running `gh`, which
//! is how this is tested locally and in `xtask/tests/fuzz_issue.rs`.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use crate::fuzz::Limits;

/// The label every fuzz issue carries.
pub const LABEL: &str = "fuzz";

/// Colour of [`LABEL`] when it has to be created.
pub const LABEL_COLOR: &str = "B60205";

/// Description of [`LABEL`] when it has to be created.
pub const LABEL_DESCRIPTION: &str = "Found by nightly fuzzing (fuzz.yml)";

/// Prefix of the per-target failure artifact (and directory) names.
pub const ARTIFACT_PREFIX: &str = "fuzz-failure-";

/// The run log inside a failure directory.
pub const LOG_FILE: &str = "fuzz.log";

/// The features the run used, comma-separated, inside a failure directory.
pub const FEATURES_FILE: &str = "features.txt";

/// How many saved-input bytes the issue shows in hex.
pub const PREVIEW_BYTES: usize = 64;

/// Longest log excerpt, in lines.
pub const EXCERPT_LINES: usize = 60;

/// Longest log excerpt, in bytes (GitHub caps a body at 65 536 characters).
pub const EXCERPT_BYTES: usize = 20_000;

/// Stack frames kept in the excerpt.
pub const EXCERPT_FRAMES: usize = 8;

/// The open issues `gh issue list` is asked for (newest first); the dedupe
/// search covers this many open `fuzz` issues.
pub const LIST_LIMIT: u32 = 200;

/// What kind of failure libFuzzer saved an input for, from its file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `crash-*`: a panic, failed assertion or deadly signal.
    Crash,
    /// `timeout-*`: one input ran past `-timeout`.
    Timeout,
    /// `oom-*`: `-rss_limit_mb` or `-malloc_limit_mb` exceeded.
    OutOfMemory,
    /// `leak-*`: LeakSanitizer.
    Leak,
    /// `slow-unit-*`: slower than `-report_slow_units`.
    SlowUnit,
}

impl Kind {
    /// The file-name prefixes libFuzzer uses, with their kind.
    pub const PREFIXES: &'static [(&'static str, Self)] = &[
        ("crash-", Self::Crash),
        ("timeout-", Self::Timeout),
        ("oom-", Self::OutOfMemory),
        ("leak-", Self::Leak),
        ("slow-unit-", Self::SlowUnit),
    ];

    /// The kind of a saved input named `name`, if it is one.
    #[must_use]
    pub fn from_file_name(name: &str) -> Option<Self> {
        Self::PREFIXES
            .iter()
            .find(|(p, _)| name.starts_with(p))
            .map(|&(_, k)| k)
    }

    /// Words for the issue.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::Crash => "crash (panic, failed assertion or deadly signal)",
            Self::Timeout => "timeout (one input ran longer than the per-input limit)",
            Self::OutOfMemory => "out of memory (RSS or single-allocation limit exceeded)",
            Self::Leak => "memory leak",
            Self::SlowUnit => "slow input",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Crash => "crash",
            Self::Timeout => "timeout",
            Self::OutOfMemory => "oom",
            Self::Leak => "leak",
            Self::SlowUnit => "slow-unit",
        })
    }
}

/// One input libFuzzer saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedInput {
    /// File name (plain characters only; see [`is_plain_name`]).
    pub name: String,
    /// What it triggered.
    pub kind: Kind,
    /// Size in bytes.
    pub len: usize,
    /// The first [`PREVIEW_BYTES`] bytes.
    pub head: Vec<u8>,
}

/// One failing target of the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// Target name (a target of the fuzz crate).
    pub target: String,
    /// Features the run enabled (each declared by the fuzz crate).
    pub features: Vec<String>,
    /// The saved inputs, sorted by name.
    pub inputs: Vec<SavedInput>,
    /// The run log ([`LOG_FILE`]), possibly empty.
    pub log: String,
}

/// What [`collect`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Collected {
    /// Failures with at least one saved input.
    pub failures: Vec<Failure>,
    /// Directories or files skipped, and why.
    pub warnings: Vec<String>,
}

/// Context the issue body mentions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Context {
    /// The workflow run (`$GITHUB_SERVER_URL/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID`).
    pub run_url: Option<String>,
    /// The limits the run applied.
    pub limits: Option<Limits>,
}

/// Whether `s` is a non-empty run of `[A-Za-z0-9._-]` not starting with `.`.
#[must_use]
pub fn is_plain_name(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('.')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Reads every `fuzz-failure-<target>/` under `dir`. `targets` and
/// `features` are what the fuzz crate declares; anything else is skipped
/// with a warning. A failure without a saved input (the build broke, say)
/// is a warning, not an issue: the red workflow run reports it.
///
/// # Errors
///
/// Fails if `dir` cannot be read.
pub fn collect(dir: &Path, targets: &[String], features: &[String]) -> Result<Collected> {
    let mut out = Collected::default();
    let mut dirs: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for d in dirs {
        let name = d
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(target) = name.strip_prefix(ARTIFACT_PREFIX) else {
            out.warnings.push(format!(
                "{name}/: not a {ARTIFACT_PREFIX}<target> directory, skipped"
            ));
            continue;
        };
        if !targets.iter().any(|t| t == target) {
            out.warnings.push(format!(
                "{name}/: `{}` is not a fuzz target ({}), skipped",
                target.escape_debug(),
                targets.join(", ")
            ));
            continue;
        }
        let mut run_features = Vec::new();
        let raw = fs::read_to_string(d.join(FEATURES_FILE)).unwrap_or_default();
        for f in raw
            .split([',', '\n'])
            .map(str::trim)
            .filter(|f| !f.is_empty())
        {
            if features.iter().any(|x| x == f) {
                run_features.push(f.to_owned());
            } else {
                out.warnings.push(format!(
                    "{name}/{FEATURES_FILE}: `{}` is not a fuzz-crate feature, ignored",
                    f.escape_debug()
                ));
            }
        }
        let mut inputs = Vec::new();
        let mut files: Vec<PathBuf> = fs::read_dir(&d)
            .with_context(|| format!("reading {}", d.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file())
            .collect();
        files.sort();
        for f in files {
            let file = f
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if file == LOG_FILE || file == FEATURES_FILE {
                continue;
            }
            let Some(kind) = Kind::from_file_name(&file) else {
                continue;
            };
            if !is_plain_name(&file) {
                out.warnings.push(format!(
                    "{name}/: saved input `{}` has an unexpected name, skipped",
                    file.escape_debug()
                ));
                continue;
            }
            let bytes = fs::read(&f).with_context(|| format!("reading {}", f.display()))?;
            inputs.push(SavedInput {
                name: file,
                kind,
                len: bytes.len(),
                head: bytes.iter().copied().take(PREVIEW_BYTES).collect(),
            });
        }
        if inputs.is_empty() {
            out.warnings.push(format!(
                "{name}/: the run failed without saving an input (build or infrastructure failure?); not filed, see the workflow log"
            ));
            continue;
        }
        let log = fs::read(d.join(LOG_FILE))
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        out.failures.push(Failure {
            target: target.to_owned(),
            features: run_features,
            inputs,
            log,
        });
    }
    Ok(out)
}

/// The issue title: one open issue per target and feature set. Runs with
/// planted bugs enabled (the harness self-test) get their own issue, so a
/// self-test never comments on, or hides behind, a real finding.
#[must_use]
pub fn issue_title(target: &str, features: &[String]) -> String {
    if features.is_empty() {
        format!("fuzz: target `{target}` found a failing input")
    } else {
        format!(
            "fuzz: target `{target}` found a failing input (features: {})",
            features.join(",")
        )
    }
}

/// The number of the open issue titled exactly `title` in `json`, the
/// output of [`list_call`].
///
/// # Errors
///
/// Fails if `json` is not an array of `{number, title}` objects.
pub fn existing_issue(json: &str, title: &str) -> Result<Option<u64>> {
    let v: serde_json::Value =
        serde_json::from_str(json).context("`gh issue list --json` output is not JSON")?;
    let items = v
        .as_array()
        .context("`gh issue list --json` output is not an array")?;
    let mut numbers = Vec::new();
    for item in items {
        let n = item
            .get("number")
            .and_then(serde_json::Value::as_u64)
            .context("`gh issue list --json` item without a number")?;
        if item.get("title").and_then(serde_json::Value::as_str) == Some(title) {
            numbers.push(n);
        }
    }
    // The oldest issue keeps the history.
    Ok(numbers.into_iter().min())
}

/// Hex of `bytes`, space-separated.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `s` without ANSI escape sequences (`ESC [ ... final-byte`) or other
/// control characters except tab.
#[must_use]
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            continue;
        }
        if c.is_control() && c != '\t' && c != '\n' {
            continue;
        }
        out.push(c);
    }
    out
}

/// The interesting end of a fuzz log: from the first panic or libFuzzer
/// error report to the end, at most [`EXCERPT_FRAMES`] stack frames, at most
/// [`EXCERPT_LINES`] lines and [`EXCERPT_BYTES`] bytes. Falls back to the
/// last [`EXCERPT_LINES`] lines.
#[must_use]
pub fn log_excerpt(log: &str) -> String {
    let clean = strip_ansi(log);
    let lines: Vec<&str> = clean.lines().collect();
    let is_start = |l: &str| {
        l.contains("panicked at")
            || l.contains("ERROR: libFuzzer")
            || l.contains("ERROR: AddressSanitizer")
            || l.contains("ERROR: LeakSanitizer")
    };
    let start = lines
        .iter()
        .position(|l| is_start(l))
        .unwrap_or_else(|| lines.len().saturating_sub(EXCERPT_LINES));
    let mut out: Vec<&str> = Vec::new();
    let mut frames = 0usize;
    let mut elided = 0usize;
    for l in &lines[start..] {
        let t = l.trim_start();
        let is_frame = t.starts_with('#') && t.contains(" 0x");
        if is_frame {
            frames += 1;
            if frames > EXCERPT_FRAMES {
                elided += 1;
                continue;
            }
        } else if elided > 0 {
            out.push("    ... (more frames in the run log)");
            elided = 0;
        }
        out.push(l);
        if out.len() >= EXCERPT_LINES {
            break;
        }
    }
    let mut text = out.join("\n");
    if text.len() > EXCERPT_BYTES {
        let mut cut = EXCERPT_BYTES;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("\n... (truncated)");
    }
    text
}

/// A Markdown code fence that `text` cannot close: one backtick longer than
/// its longest backtick run, at least three.
#[must_use]
pub fn fence_for(text: &str) -> String {
    let mut longest = 0usize;
    let mut run = 0usize;
    for c in text.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    "`".repeat((longest + 1).max(3))
}

fn fenced(text: &str) -> String {
    let fence = fence_for(text);
    format!("{fence}text\n{text}\n{fence}\n")
}

/// The issue body (or, with `again`, the comment on the open issue).
#[must_use]
pub fn issue_body(f: &Failure, ctx: &Context, again: bool) -> String {
    let t = &f.target;
    let mut kinds: Vec<Kind> = Vec::new();
    for i in &f.inputs {
        if !kinds.contains(&i.kind) {
            kinds.push(i.kind);
        }
    }
    let kinds = kinds
        .iter()
        .map(|k| k.describe())
        .collect::<Vec<_>>()
        .join("; ");
    let mut b = String::new();
    if again {
        b.push_str(&format!(
            "Nightly fuzzing found fuzz target `{t}` failing again: **{kinds}**.\n\n"
        ));
    } else {
        b.push_str(&format!(
            "Nightly fuzzing found an input that makes fuzz target `{t}` fail: **{kinds}**.\n\n"
        ));
    }
    if let Some(url) = &ctx.run_url {
        b.push_str(&format!("- Run: {url}\n"));
    }
    b.push_str(&format!(
        "- Saved inputs: artifact `{ARTIFACT_PREFIX}{t}` of that run.\n"
    ));
    if let Some(l) = ctx.limits {
        b.push_str(&format!(
            "- Per-input limits (applied by `cargo xtask fuzz run`): `{}`.\n",
            l.libfuzzer_args().join(" ")
        ));
    }
    let features = f.features.join(",");
    if f.features.is_empty() {
        b.push_str("- Features: none.\n");
    } else {
        b.push_str(&format!("- Features: `{features}`."));
        if f.features.iter().any(|x| x.starts_with("planted-")) {
            b.push_str(
                " These enable deliberately planted bugs: this run was the harness self-test \
                 (a manual dispatch of fuzz.yml); close this issue once seen.",
            );
        }
        b.push('\n');
    }
    b.push_str("\n| Input | Kind | Size | First bytes (hex) |\n|---|---|---|---|\n");
    for i in &f.inputs {
        let more = if i.len > i.head.len() { " ..." } else { "" };
        b.push_str(&format!(
            "| `{}` | {} | {} B | `{}{more}` |\n",
            i.name,
            i.kind,
            i.len,
            hex(&i.head)
        ));
    }
    let excerpt = log_excerpt(&f.log);
    if !excerpt.is_empty() {
        b.push_str("\nLog excerpt:\n\n");
        b.push_str(&fenced(&excerpt));
    }
    if !again {
        let input = f.inputs.first().map_or("<file>", |i| i.name.as_str());
        let feat = if f.features.is_empty() {
            String::new()
        } else {
            format!(" --features {features}")
        };
        b.push_str(&format!(
            "\n### Reproduce and fix\n\n\
             See docs/fuzzing.md, \"When the fuzzer finds a bug\".\n\n\
             1. Download artifact `{ARTIFACT_PREFIX}{t}` from the run and unzip it.\n\
             2. Reproduce: `cargo xtask fuzz run {t}{feat} --input {input}`\n\
             3. Minimise: `cargo +nightly fuzz tmin --fuzz-dir fuzz {t} {input}`\n\
             4. Commit the minimised input (`tmin` writes \
             `fuzz/artifacts/{t}/minimized-from-*`) as `fuzz/regressions/{t}/<short-name>`. \
             It is then replayed as an ordinary stable test in ci.yml (job `fuzz-regressions`); \
             `cargo xtask fuzz replay{feat}` must fail on it now.\n\
             5. Fix the bug: the replay passes again, and the input stays as a regression test.\n"
        ));
    }
    b.push_str(&format!("\n<!-- rr-fuzz target={t} -->\n"));
    b
}

/// One `gh` invocation: its arguments (without `gh`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhCall {
    /// Arguments after `gh`.
    pub args: Vec<String>,
}

impl GhCall {
    fn new<I: IntoIterator<Item = S>, S: Into<String>>(args: I) -> Self {
        Self {
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

/// POSIX-shell quoting of `s` (for display only; `gh` is never run through
/// a shell).
#[must_use]
pub fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/' | b',' | b'=' | b':')
        })
    {
        return s.to_owned();
    }
    format!("'{}'", s.replace('\'', r"'\''"))
}

impl fmt::Display for GhCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("gh")?;
        for a in &self.args {
            write!(f, " {}", shell_quote(a))?;
        }
        Ok(())
    }
}

/// `gh issue list` for the open [`LABEL`] issues, as JSON.
#[must_use]
pub fn list_call() -> GhCall {
    GhCall::new([
        "issue".to_owned(),
        "list".to_owned(),
        "--label".to_owned(),
        LABEL.to_owned(),
        "--state".to_owned(),
        "open".to_owned(),
        "--limit".to_owned(),
        LIST_LIMIT.to_string(),
        "--json".to_owned(),
        "number,title".to_owned(),
    ])
}

/// `gh label create` for [`LABEL`] (`--force`: succeeds if it exists).
#[must_use]
pub fn label_call() -> GhCall {
    GhCall::new([
        "label",
        "create",
        LABEL,
        "--color",
        LABEL_COLOR,
        "--description",
        LABEL_DESCRIPTION,
        "--force",
    ])
}

/// Files a report: a new issue, or a comment on `existing`.
#[must_use]
pub fn file_call(title: &str, existing: Option<u64>, body_file: &Path) -> GhCall {
    let body = body_file.to_string_lossy().into_owned();
    match existing {
        Some(n) => GhCall::new([
            "issue".to_owned(),
            "comment".to_owned(),
            n.to_string(),
            "--body-file".to_owned(),
            body,
        ]),
        None => GhCall::new([
            "issue".to_owned(),
            "create".to_owned(),
            "--title".to_owned(),
            title.to_owned(),
            "--label".to_owned(),
            LABEL.to_owned(),
            "--body-file".to_owned(),
            body,
        ]),
    }
}

/// One issue to file: the call, and the body file it reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filing {
    /// Target.
    pub target: String,
    /// The dedupe title.
    pub title: String,
    /// The open issue it comments on, if any.
    pub existing: Option<u64>,
    /// Where the body is written.
    pub body_file: PathBuf,
    /// The body.
    pub body: String,
    /// `gh issue create` or `gh issue comment`.
    pub call: GhCall,
}

/// The filings for `failures`, given the open issues `open_json` (output of
/// [`list_call`]). Bodies are to be written under `body_dir` as
/// `<target>.md` before the calls run.
///
/// # Errors
///
/// Fails if `open_json` is not `gh issue list --json number,title` output.
pub fn plan(
    failures: &[Failure],
    ctx: &Context,
    open_json: &str,
    body_dir: &Path,
) -> Result<Vec<Filing>> {
    let mut out = Vec::new();
    for f in failures {
        let title = issue_title(&f.target, &f.features);
        let existing = existing_issue(open_json, &title)?;
        let body = issue_body(f, ctx, existing.is_some());
        let body_file = body_dir.join(format!("{}.md", f.target));
        let call = file_call(&title, existing, &body_file);
        out.push(Filing {
            target: f.target.clone(),
            title,
            existing,
            body_file,
            body,
            call,
        });
    }
    Ok(out)
}
