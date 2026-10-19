//! `cargo xtask fuzz` (US-RR-5-5): the one way fuzz targets are run, so the
//! per-input time and memory limits are never forgotten.
//!
//! The fuzz crate (`fuzz/`, cargo-fuzz + libfuzzer-sys) is its own workspace,
//! excluded from the root one: it needs nightly Rust, the main workspace
//! builds on stable. Its manifest carries the limits in
//! `[package.metadata.rr-fuzz]` (`timeout_secs`, `rss_limit_mb`,
//! `malloc_limit_mb`), and [`run_command`] turns them into libFuzzer's
//! `-timeout`, `-rss_limit_mb` and `-malloc_limit_mb` on every run, so a hang
//! or unbounded allocation is a reported failure, like a panic.
//!
//! * `fuzz list`: the targets (`--json`: a JSON array, the nightly matrix;
//!   `--only <t>`: just that one, see [`select_targets`]).
//! * `fuzz file-issues`: files a nightly run's failures as GitHub issues
//!   ([`crate::fuzz_issue`]).
//! * `fuzz check` ([`check`]; also run on the real tree by
//!   `xtask/tests/fuzz.rs` in CI): the crate is excluded from the root
//!   workspace, is its own workspace, declares sane limits, and every target
//!   has its `[[bin]]`, `fuzz_targets/<name>.rs`, `src/lib.rs` `TARGETS` entry
//!   and a non-empty `seeds/<name>/`; the replay harness is declared; and
//!   every directory under `regressions/` names a target (see
//!   [`regression_problems`]).
//! * `fuzz run <target>`: `cargo +nightly fuzz run` with the limits, the
//!   growing corpus `fuzz/corpus/<target>` and the committed seeds.
//! * `fuzz replay` ([`replay_command`], US-RR-5-7): `cargo test
//!   --manifest-path fuzz/Cargo.toml --locked` on the **stable** toolchain:
//!   the seeds, then every committed crash input in
//!   `fuzz/regressions/<target>/`, each in its own child process with the
//!   per-input timeout and memory caps (`fuzz/tests/regressions.rs`, after
//!   a self-test that a planted panic, hang and memory blow-up each fail
//!   it). No nightly, no libFuzzer; the `fuzz-regressions` job in ci.yml.
//! * `fuzz selftest` ([`SELFTEST_CASES`]): runs the toy target's planted
//!   panic, hang and both kinds of memory blow-up under the limits and fails
//!   unless each is reported by libFuzzer, and a clean input passes. Needs
//!   nightly and cargo-fuzz.
//!
//! How to add a target: docs/fuzzing.md.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result};
use toml::Value;

/// The fuzz crate, relative to the workspace root.
pub const FUZZ_DIR: &str = "fuzz";

/// Environment variable naming the toolchain for cargo-fuzz (default
/// [`DEFAULT_TOOLCHAIN`]), e.g. a dated nightly in CI.
pub const TOOLCHAIN_ENV: &str = "RR_FUZZ_TOOLCHAIN";

/// Toolchain used when [`TOOLCHAIN_ENV`] is unset.
pub const DEFAULT_TOOLCHAIN: &str = "nightly";

/// Committed minimised crash inputs, `fuzz/regressions/<target>/<file>`,
/// relative to the fuzz crate.
pub const REGRESSIONS_DIR: &str = "regressions";

/// The fuzz crate's `[[test]]` that replays [`REGRESSIONS_DIR`]
/// (`fuzz/tests/regressions.rs`, a custom harness).
pub const REPLAY_TEST: &str = "regressions";

/// Allowed range of `timeout_secs`: long enough for ASan-instrumented
/// parsing of one input, short enough that a hang is caught in a 10-minute
/// nightly run.
pub const TIMEOUT_RANGE: (u64, u64) = (1, 60);

/// Allowed range of `rss_limit_mb`: a non-zero cap (0 disables it in
/// libFuzzer) with room for ASan's quarantine.
pub const RSS_RANGE: (u64, u64) = (256, 4096);

/// Per-input limits from `fuzz/Cargo.toml` `[package.metadata.rr-fuzz]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Seconds one input may run.
    pub timeout_secs: u64,
    /// Resident-memory cap for the fuzzing process, in MB.
    pub rss_limit_mb: u64,
    /// Largest single allocation, in MB.
    pub malloc_limit_mb: u64,
}

impl Limits {
    /// The libFuzzer flags that enforce these limits.
    #[must_use]
    pub fn libfuzzer_args(&self) -> Vec<String> {
        vec![
            format!("-timeout={}", self.timeout_secs),
            format!("-rss_limit_mb={}", self.rss_limit_mb),
            format!("-malloc_limit_mb={}", self.malloc_limit_mb),
        ]
    }
}

/// One `[[bin]]` of the fuzz crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// Target name (`cargo fuzz run <name>`).
    pub name: String,
    /// `path` from the manifest, if given.
    pub path: Option<String>,
    /// Whether `test`, `doc` and `bench` are all `false`.
    pub excluded_from_test_doc_bench: bool,
}

/// What `fuzz/Cargo.toml` declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// The `[[bin]]` targets, in manifest order.
    pub targets: Vec<Target>,
    /// The limits, if all three are present as integers.
    pub limits: Option<Limits>,
    /// Whether the manifest has its own `[workspace]` table.
    pub own_workspace: bool,
    /// Whether `[package.metadata] cargo-fuzz = true`.
    pub cargo_fuzz: bool,
    /// Names of the declared features.
    pub features: Vec<String>,
    /// Whether `[[test]] name = "regressions"` is declared with
    /// `path = "tests/regressions.rs"` and `harness = false`.
    pub replay_harness: bool,
}

/// Parses `fuzz/Cargo.toml` text.
///
/// # Errors
///
/// Fails if the text is not TOML.
pub fn parse_manifest(text: &str) -> Result<Manifest> {
    let doc: Value = toml::from_str(text).context("fuzz/Cargo.toml is not valid TOML")?;
    let metadata = doc.get("package").and_then(|p| p.get("metadata"));
    let cargo_fuzz = metadata
        .and_then(|m| m.get("cargo-fuzz"))
        .and_then(Value::as_bool)
        == Some(true);
    let rr = metadata.and_then(|m| m.get("rr-fuzz"));
    let int = |key: &str| {
        rr.and_then(|t| t.get(key))
            .and_then(Value::as_integer)
            .and_then(|v| u64::try_from(v).ok())
    };
    let limits = match (
        int("timeout_secs"),
        int("rss_limit_mb"),
        int("malloc_limit_mb"),
    ) {
        (Some(timeout_secs), Some(rss_limit_mb), Some(malloc_limit_mb)) => Some(Limits {
            timeout_secs,
            rss_limit_mb,
            malloc_limit_mb,
        }),
        _ => None,
    };
    let targets = doc
        .get("bin")
        .and_then(Value::as_array)
        .map(|bins| {
            bins.iter()
                .map(|b| {
                    let off = |k: &str| b.get(k).and_then(Value::as_bool) == Some(false);
                    Target {
                        name: b
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        path: b.get("path").and_then(Value::as_str).map(str::to_owned),
                        excluded_from_test_doc_bench: off("test") && off("doc") && off("bench"),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let features = doc
        .get("features")
        .and_then(Value::as_table)
        .map(|t| t.keys().cloned().collect())
        .unwrap_or_default();
    let replay_harness = doc
        .get("test")
        .and_then(Value::as_array)
        .is_some_and(|tests| {
            tests.iter().any(|t| {
                t.get("name").and_then(Value::as_str) == Some(REPLAY_TEST)
                    && t.get("path").and_then(Value::as_str)
                        == Some(&format!("tests/{REPLAY_TEST}.rs"))
                    && t.get("harness").and_then(Value::as_bool) == Some(false)
            })
        });
    Ok(Manifest {
        targets,
        limits,
        own_workspace: doc.get("workspace").is_some_and(Value::is_table),
        cargo_fuzz,
        features,
        replay_harness,
    })
}

/// Reads and parses `<root>/fuzz/Cargo.toml`.
///
/// # Errors
///
/// Fails if the file cannot be read or is not TOML.
pub fn load(root: &Path) -> Result<Manifest> {
    let path = root.join(FUZZ_DIR).join("Cargo.toml");
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    parse_manifest(&text)
}

/// The target names of `m` (`fuzz list`), or only `only` when it is given
/// and non-empty (the nightly workflow's `target` input).
///
/// # Errors
///
/// Fails if `only` names no target.
pub fn select_targets(m: &Manifest, only: Option<&str>) -> Result<Vec<String>> {
    let names: Vec<String> = m.targets.iter().map(|t| t.name.clone()).collect();
    match only.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(names),
        Some(o) if names.iter().any(|n| n == o) => Ok(vec![o.to_owned()]),
        Some(o) => anyhow::bail!("no fuzz target {o:?} (targets: {})", names.join(", ")),
    }
}

/// Problems with the limits, if any.
#[must_use]
pub fn limit_problems(limits: Option<Limits>) -> Vec<String> {
    let Some(l) = limits else {
        return vec![
            "fuzz/Cargo.toml: [package.metadata.rr-fuzz] must set integer timeout_secs, rss_limit_mb and malloc_limit_mb".to_owned(),
        ];
    };
    let mut out = Vec::new();
    if !(TIMEOUT_RANGE.0..=TIMEOUT_RANGE.1).contains(&l.timeout_secs) {
        out.push(format!(
            "fuzz/Cargo.toml: timeout_secs = {} is outside {}..={}",
            l.timeout_secs, TIMEOUT_RANGE.0, TIMEOUT_RANGE.1
        ));
    }
    if !(RSS_RANGE.0..=RSS_RANGE.1).contains(&l.rss_limit_mb) {
        out.push(format!(
            "fuzz/Cargo.toml: rss_limit_mb = {} is outside {}..={}",
            l.rss_limit_mb, RSS_RANGE.0, RSS_RANGE.1
        ));
    }
    if l.malloc_limit_mb == 0 || l.malloc_limit_mb > l.rss_limit_mb {
        out.push(format!(
            "fuzz/Cargo.toml: malloc_limit_mb = {} must be in 1..=rss_limit_mb ({})",
            l.malloc_limit_mb, l.rss_limit_mb
        ));
    }
    out
}

/// Checks the fuzz crate layout under `root` (see the module docs).
///
/// # Errors
///
/// Fails only if a manifest cannot be read or parsed; layout problems are
/// returned.
pub fn check(root: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();

    let root_manifest = root.join("Cargo.toml");
    let text = fs::read_to_string(&root_manifest)
        .with_context(|| format!("reading {}", root_manifest.display()))?;
    let doc: Value = toml::from_str(&text).context("root Cargo.toml is not valid TOML")?;
    let list = |key: &str| -> Vec<String> {
        doc.get("workspace")
            .and_then(|w| w.get(key))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.trim_end_matches('/').to_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    if !list("exclude").iter().any(|e| e == FUZZ_DIR) {
        out.push(format!(
            "Cargo.toml: [workspace] exclude must list \"{FUZZ_DIR}\" (the fuzz crate needs nightly; the workspace builds on stable)"
        ));
    }
    for key in ["members", "default-members"] {
        if list(key)
            .iter()
            .any(|m| m == FUZZ_DIR || m.starts_with("fuzz/"))
        {
            out.push(format!(
                "Cargo.toml: [workspace] {key} must not include the fuzz crate"
            ));
        }
    }

    let fuzz = root.join(FUZZ_DIR);
    let m = load(root)?;
    if !m.own_workspace {
        out.push("fuzz/Cargo.toml: needs its own [workspace] table".to_owned());
    }
    if !m.cargo_fuzz {
        out.push("fuzz/Cargo.toml: needs [package.metadata] cargo-fuzz = true".to_owned());
    }
    out.extend(limit_problems(m.limits));
    if m.targets.is_empty() {
        out.push("fuzz/Cargo.toml: no [[bin]] fuzz targets".to_owned());
    }

    let lib = fs::read_to_string(fuzz.join("src/lib.rs")).unwrap_or_default();
    for t in &m.targets {
        let n = &t.name;
        let want = format!("fuzz_targets/{n}.rs");
        if t.path.as_deref() != Some(want.as_str()) {
            out.push(format!(
                "fuzz/Cargo.toml: [[bin]] {n}: path must be \"{want}\""
            ));
        }
        if !fuzz.join(&want).is_file() {
            out.push(format!("fuzz/{want}: missing"));
        }
        if !t.excluded_from_test_doc_bench {
            out.push(format!(
                "fuzz/Cargo.toml: [[bin]] {n}: set test = false, doc = false, bench = false"
            ));
        }
        if !lib.contains(&format!("(\"{n}\", {n})")) {
            out.push(format!(
                "fuzz/src/lib.rs: TARGETS has no (\"{n}\", {n}) entry"
            ));
        }
        let seeds = fuzz.join("seeds").join(n);
        let has_seed = fs::read_dir(&seeds)
            .map(|mut d| d.any(|e| e.is_ok_and(|e| e.path().is_file())))
            .unwrap_or(false);
        if !has_seed {
            out.push(format!("fuzz/seeds/{n}/: needs at least one seed input"));
        }
    }
    if !m.replay_harness {
        out.push(format!(
            "fuzz/Cargo.toml: needs [[test]] name = \"{REPLAY_TEST}\", path = \"tests/{REPLAY_TEST}.rs\", harness = false (the regression replay)"
        ));
    }
    if !fuzz
        .join("tests")
        .join(format!("{REPLAY_TEST}.rs"))
        .is_file()
    {
        out.push(format!("fuzz/tests/{REPLAY_TEST}.rs: missing"));
    }
    let names: Vec<&str> = m.targets.iter().map(|t| t.name.as_str()).collect();
    out.extend(regression_problems(&fuzz.join(REGRESSIONS_DIR), &names));
    if let Ok(entries) = fs::read_dir(fuzz.join("fuzz_targets")) {
        let mut stems: Vec<String> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "rs"))
            .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .collect();
        stems.sort();
        for s in stems {
            if !m.targets.iter().any(|t| t.name == s) {
                out.push(format!(
                    "fuzz/fuzz_targets/{s}.rs: no [[bin]] in fuzz/Cargo.toml"
                ));
            }
        }
    }
    Ok(out)
}

/// Layout problems in a `regressions/` directory `dir` for the fuzz targets
/// `targets` (the same rules the replay harness enforces at run time,
/// `rr_fuzz::replay::discover`): every entry must be a directory named after
/// a target, and holds only files. A missing `dir`, or a target with no
/// directory, is fine: nothing to replay.
#[must_use]
pub fn regression_problems(dir: &Path, targets: &[&str]) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    let mut out = Vec::new();
    for path in paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !path.is_dir() {
            out.push(format!(
                "fuzz/{REGRESSIONS_DIR}/{name}: not replayed; inputs go in fuzz/{REGRESSIONS_DIR}/<target>/"
            ));
        } else if !targets.contains(&name.as_str()) {
            out.push(format!(
                "fuzz/{REGRESSIONS_DIR}/{name}/: unknown fuzz target {name:?} (targets: {})",
                targets.join(", ")
            ));
        } else if let Ok(files) = fs::read_dir(&path) {
            let mut subdirs: Vec<String> = files
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_dir())
                .map(|p| {
                    p.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            subdirs.sort();
            for d in subdirs {
                out.push(format!(
                    "fuzz/{REGRESSIONS_DIR}/{name}/{d}/: subdirectories are not replayed"
                ));
            }
        }
    }
    out
}

/// `cargo test --manifest-path <root>/fuzz/Cargo.toml --locked [--features
/// ...]`: the stable regression replay (`fuzz replay`). No `+toolchain`, so
/// it runs on the toolchain pinned in rust-toolchain.toml, like the rest of
/// CI. `features` are fuzz-crate features (e.g. `planted-panic`, under which
/// the committed `regressions/toy/planted-panic` input must make it fail).
#[must_use]
pub fn replay_command(root: &Path, features: &[String]) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(root)
        .args(["test", "--manifest-path"])
        .arg(root.join(FUZZ_DIR).join("Cargo.toml"))
        .arg("--locked");
    if !features.is_empty() {
        cmd.arg("--features").arg(features.join(","));
    }
    cmd
}

/// What to run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunOptions {
    /// Target name.
    pub target: String,
    /// Cargo features of the fuzz crate to enable.
    pub features: Vec<String>,
    /// Stop after this many seconds (`-max_total_time`); `None` runs until a
    /// failure or Ctrl-C.
    pub max_total_time: Option<u64>,
    /// Run these inputs once each instead of fuzzing (reproduce a crash).
    pub inputs: Vec<PathBuf>,
    /// Where libFuzzer writes crash/timeout/oom inputs (default
    /// `fuzz/artifacts/<target>/`).
    pub artifact_prefix: Option<PathBuf>,
    /// Further libFuzzer flags, after the limits (a later flag wins, so an
    /// explicit override here is deliberate and visible).
    pub extra: Vec<String>,
}

/// The toolchain to run cargo-fuzz with: [`TOOLCHAIN_ENV`] or
/// [`DEFAULT_TOOLCHAIN`].
#[must_use]
pub fn toolchain() -> String {
    std::env::var(TOOLCHAIN_ENV)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_TOOLCHAIN.to_owned())
}

/// The growing (uncommitted, CI-cached) corpus directory of `target`.
#[must_use]
pub fn corpus_dir(root: &Path, target: &str) -> PathBuf {
    root.join(FUZZ_DIR).join("corpus").join(target)
}

/// The committed seed directory of `target`.
#[must_use]
pub fn seeds_dir(root: &Path, target: &str) -> PathBuf {
    root.join(FUZZ_DIR).join("seeds").join(target)
}

/// The `cargo +<toolchain> fuzz run` invocation for `opts` with `limits`
/// always applied. When fuzzing (no `inputs`), the corpus is
/// [`corpus_dir`] (written) plus [`seeds_dir`] (read); the caller creates
/// the corpus directory ([`run`] does).
///
/// `cargo` is the rustup proxy from `PATH`, not `$CARGO` (which, under
/// `cargo xtask`, is the stable toolchain's own cargo and ignores `+nightly`).
#[must_use]
pub fn run_command(root: &Path, limits: Limits, toolchain: &str, opts: &RunOptions) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.env_remove("RUSTUP_TOOLCHAIN")
        .current_dir(root)
        .arg(format!("+{toolchain}"))
        .args(["fuzz", "run", "--fuzz-dir"])
        .arg(root.join(FUZZ_DIR));
    if !opts.features.is_empty() {
        cmd.arg("--features").arg(opts.features.join(","));
    }
    cmd.arg(&opts.target);
    if opts.inputs.is_empty() {
        cmd.arg(corpus_dir(root, &opts.target));
        cmd.arg(seeds_dir(root, &opts.target));
    } else {
        cmd.args(&opts.inputs);
    }
    cmd.arg("--").args(limits.libfuzzer_args());
    if let Some(t) = opts.max_total_time {
        cmd.arg(format!("-max_total_time={t}"));
    }
    if let Some(p) = &opts.artifact_prefix {
        let mut a = OsString::from("-artifact_prefix=");
        a.push(p.as_os_str());
        cmd.arg(a);
    }
    cmd.args(&opts.extra);
    cmd
}

/// Loads the limits from `root`'s fuzz crate and builds [`run_command`],
/// creating the corpus directory when fuzzing.
///
/// # Errors
///
/// Fails if the manifest cannot be read, its limits are missing or out of
/// range, `opts.target` is not a target, or the corpus directory cannot be
/// created.
pub fn run(root: &Path, opts: &RunOptions) -> Result<Command> {
    let m = load(root)?;
    let problems = limit_problems(m.limits);
    let Some(limits) = m.limits.filter(|_| problems.is_empty()) else {
        anyhow::bail!("{}", problems.join("; "));
    };
    if !m.targets.iter().any(|t| t.name == opts.target) {
        let names: Vec<&str> = m.targets.iter().map(|t| t.name.as_str()).collect();
        anyhow::bail!(
            "no fuzz target {:?} (targets: {})",
            opts.target,
            names.join(", ")
        );
    }
    if opts.inputs.is_empty() {
        let dir = corpus_dir(root, &opts.target);
        fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    Ok(run_command(root, limits, &toolchain(), opts))
}

/// One selftest case: run the `toy` target once on `input` with every
/// planted bug compiled in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelftestCase {
    /// Case name.
    pub name: &'static str,
    /// The input.
    pub input: &'static [u8],
    /// `Some(text)`: the run must fail and its output contain `text`;
    /// `None`: the run must succeed.
    pub expect_failure: Option<&'static str>,
}

/// The target the selftest runs.
pub const SELFTEST_TARGET: &str = "toy";

/// Features the selftest enables.
pub const SELFTEST_FEATURES: &[&str] = &["planted-panic", "planted-hang", "planted-oom"];

/// The selftest cases: a clean control, then one per kind of failure.
pub const SELFTEST_CASES: &[SelftestCase] = &[
    SelftestCase {
        name: "clean input",
        input: b"\x02ok\x00",
        expect_failure: None,
    },
    SelftestCase {
        name: "panic",
        input: b"PANIC",
        expect_failure: Some("planted panic"),
    },
    SelftestCase {
        name: "hang (timeout_secs)",
        input: b"HANG",
        expect_failure: Some("libFuzzer: timeout"),
    },
    SelftestCase {
        name: "oversized allocation (malloc_limit_mb)",
        input: b"OOM-MALLOC",
        expect_failure: Some("libFuzzer: out-of-memory (malloc("),
    },
    SelftestCase {
        name: "unbounded memory growth (rss_limit_mb)",
        input: b"OOM-RSS",
        expect_failure: Some("libFuzzer: out-of-memory (used:"),
    },
];

/// A selftest case whose run did not behave as expected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelftestProblem {
    /// Case name.
    pub case: &'static str,
    /// What went wrong.
    pub detail: String,
}

impl fmt::Display for SelftestProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.case, self.detail)
    }
}

/// Judges one selftest run from its success flag and combined output.
#[must_use]
pub fn evaluate(case: &SelftestCase, success: bool, output: &str) -> Option<SelftestProblem> {
    let detail = match (case.expect_failure, success) {
        (None, true) => return None,
        (None, false) => "expected a clean run, but it failed".to_owned(),
        (Some(_), true) => "expected a reported failure, but the run passed".to_owned(),
        (Some(text), false) if output.contains(text) => return None,
        (Some(text), false) => format!("the run failed, but its output lacks {text:?}"),
    };
    Some(SelftestProblem {
        case: case.name,
        detail,
    })
}

/// Result of one selftest case.
#[derive(Debug, Clone)]
pub struct SelftestReport {
    /// The case.
    pub case: SelftestCase,
    /// Whether cargo-fuzz exited successfully.
    pub success: bool,
    /// The problem, if the run did not behave as expected.
    pub problem: Option<SelftestProblem>,
    /// Full output, kept in the scratch directory.
    pub log: PathBuf,
}

/// Runs every [`SELFTEST_CASES`] case under the committed limits, with
/// inputs, artifacts and logs in `<root>/target/fuzz-selftest/`.
///
/// # Errors
///
/// Fails if the limits are invalid, the scratch directory cannot be written
/// or cargo cannot be started.
pub fn selftest(root: &Path) -> Result<Vec<SelftestReport>> {
    let scratch = root.join("target").join("fuzz-selftest");
    let _ = fs::remove_dir_all(&scratch);
    let artifacts = scratch.join("artifacts");
    fs::create_dir_all(&artifacts).with_context(|| format!("creating {}", artifacts.display()))?;
    let mut out = Vec::new();
    for (i, case) in SELFTEST_CASES.iter().enumerate() {
        let input = scratch.join(format!("input-{i}"));
        fs::write(&input, case.input).with_context(|| format!("writing {}", input.display()))?;
        let opts = RunOptions {
            target: SELFTEST_TARGET.to_owned(),
            features: SELFTEST_FEATURES.iter().map(|&f| f.to_owned()).collect(),
            inputs: vec![input],
            artifact_prefix: Some(artifacts.join(format!("{i}-"))),
            ..RunOptions::default()
        };
        let o = run(root, &opts)?
            .output()
            .context("cannot run cargo fuzz (is cargo-fuzz installed?)")?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        let log = scratch.join(format!("case-{i}.log"));
        fs::write(&log, &text).with_context(|| format!("writing {}", log.display()))?;
        let success = o.status.success();
        out.push(SelftestReport {
            case: *case,
            success,
            problem: evaluate(case, success, &text),
            log,
        });
    }
    Ok(out)
}
