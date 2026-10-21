//! Fixture-tier self-test (`cargo xtask fixtures-selftest`): proves that tests
//! needing an absent corpus are *skipped*, never passed.
//!
//! Proves US-RR-6-1: "Tests requiring the external or private corpus are
//! reported as skipped (with the env var to set) when it is absent, and never
//! counted as passed."
//!
//! Reading `rr_testkit::corpus_test!` / `private_test!` proves nothing about
//! what the test runner reports, so this runs `cargo nextest` for real, with
//! `RR_CORPUS_DIR` and `RR_FIXTURES_DIR` removed from the environment, once per
//! feature set in [`FEATURE_SETS`] (default features, where the private tier is
//! compiled out, and `--features private-fixtures`, where it is compiled in but
//! its variable is unset). For each feature set:
//!
//! 1. `cargo nextest list --workspace -E <`[`FILTER`]`> --message-format json`:
//!    every tier test (a test whose name ends in a [`TIERS`] suffix, which
//!    spells the variable to set) must be listed as `ignored: true` and
//!    excluded for the reason `ignored`; the [`SENTINEL_BINARIES`] must each
//!    contain at least one test of every tier, so a renamed suffix or a macro
//!    that stops generating tests cannot make the check vacuous.
//! 2. `cargo nextest run` with the same filter: must succeed, report every
//!    tier test as `SKIP` and never as `PASS`, and its summary must count
//!    `0 passed`.
//! 3. The same run with `--run-ignored only` (forcing the skipped tests to
//!    run): must **fail**, report every tier test as failed and none as
//!    passed, count `0 passed`, and print `set <VAR>` for every tier.
//!
//! Then the other half, US-RR-13-3 ("Private-fixture tests run when
//! RR_FIXTURES_DIR is set and report skipped otherwise"): with
//! `--features private-fixtures` and `RR_FIXTURES_DIR` pointing at a scratch
//! directory outside the repo (under the system temp dir) in which a random
//! nonce is planted ([`NONCE_FILE`]), restricted to the [`PRESENT_SENTINELS`]
//! (real private-tier tests need real ROMs, which a scratch directory lacks):
//!
//! 4. `cargo nextest list`: every present sentinel is listed, *not* ignored,
//!    and matched by the filter.
//! 5. `cargo nextest run` with the nonce in [`NONCE_ENV`]: must succeed, report
//!    every present sentinel as `PASS` only, and count exactly that many passed.
//! 6. The same run with a different nonce in [`NONCE_ENV`]: must **fail**, with
//!    the [`NONCE_SENTINEL`] reported as failed and naming the planted file.
//!    So the test body really executed and read the planted directory; it did
//!    not just compile, or pass without looking.
//!
//! The nested `cargo nextest` calls use their own target directory
//! ([`TARGET_DIR`]), so they never fight the caller's build lock and never
//! rebuild the caller's `target/debug` with a different `RR_*` environment.
//! This is a subcommand (run as its own CI step in the `clippy` job), not a
//! `#[test]`, because a nested cargo inside `cargo test` would run while the
//! outer test run holds the build. The pure evaluation functions below are
//! unit-tested in `xtask/tests/fixtures_selftest.rs` against both real and
//! mutated nextest output.

use std::collections::hash_map::RandomState;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::hash::BuildHasher as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail};
use serde::Deserialize;

/// Target directory of the nested cargo calls, relative to the workspace root.
pub const TARGET_DIR: &str = "target/fixtures-selftest";

/// One optional fixture tier, as seen in test names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TierSpec {
    /// Short label used in messages.
    pub label: &'static str,
    /// Suffix that `corpus_test!` / `private_test!` give every test name.
    pub suffix: &'static str,
    /// The environment variable that turns the tier on (and that the name and
    /// the forced-run failure message must spell).
    pub env_var: &'static str,
}

/// The optional tiers (rr-testkit `fixtures` module docs).
pub const TIERS: &[TierSpec] = &[
    TierSpec {
        label: "corpus",
        suffix: "::requires_RR_CORPUS_DIR",
        env_var: "RR_CORPUS_DIR",
    },
    TierSpec {
        label: "private",
        suffix: "::requires_feature_private_fixtures_and_RR_FIXTURES_DIR",
        env_var: "RR_FIXTURES_DIR",
    },
];

/// nextest filter selecting exactly the tier tests.
pub const FILTER: &str =
    "test(/::requires_(RR_CORPUS_DIR|feature_private_fixtures_and_RR_FIXTURES_DIR)$/)";

/// Test binaries that must contain at least one test of every tier:
/// rr-testkit's own fixture tests, and a dependent crate's tests (the macros
/// are expanded there, so this covers the cross-crate path).
pub const SENTINEL_BINARIES: &[&str] = &["rr-testkit::fixtures", "rr-rom::corpus_tier"];

/// Feature sets to check (`--features` values; empty means default features).
pub const FEATURE_SETS: &[&[&str]] = &[&[], &["private-fixtures"]];

/// The private tier's entry in [`TIERS`].
const PRIVATE_TIER: &TierSpec = &TIERS[1];

/// Private-tier sentinel tests run with the tier *present* (US-RR-13-3), as
/// `(binary id, test name)`. They need nothing but a directory, so they can run
/// against a scratch `RR_FIXTURES_DIR`; every other private-tier test needs the
/// real private corpus and is left out of the present phase.
pub const PRESENT_SENTINELS: &[(&str, &str)] = &[
    (
        "rr-testkit::fixtures",
        "private_tier_root_is_a_directory::requires_feature_private_fixtures_and_RR_FIXTURES_DIR",
    ),
    (
        "rr-rom::corpus_tier",
        "private_corpus_is_reachable::requires_feature_private_fixtures_and_RR_FIXTURES_DIR",
    ),
];

/// The present sentinel that checks the planted nonce
/// (`crates/rr-testkit/tests/fixtures.rs`).
pub const NONCE_SENTINEL: (&str, &str) = PRESENT_SENTINELS[0];

/// Packages holding the [`PRESENT_SENTINELS`] (the present phase builds only
/// these).
pub const PRESENT_PACKAGES: &[&str] = &["rr-testkit", "rr-rom"];

/// Variable carrying the planted nonce to the [`NONCE_SENTINEL`].
pub const NONCE_ENV: &str = "RR_FIXTURES_SELFTEST_NONCE";

/// File, in the scratch `RR_FIXTURES_DIR`, holding the nonce.
pub const NONCE_FILE: &str = "rr-fixtures-selftest.nonce";

/// nextest filter selecting exactly the [`PRESENT_SENTINELS`].
#[must_use]
pub fn present_filter() -> String {
    PRESENT_SENTINELS
        .iter()
        .map(|(binary, name)| format!("(binary_id(={binary}) & test(={name}))"))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// One test from `cargo nextest list --message-format json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedTest {
    /// nextest binary id, e.g. `rr-rom::corpus_tier`.
    pub binary_id: String,
    /// Test name within the binary.
    pub name: String,
    /// Whether the test is `#[ignore]`d.
    pub ignored: bool,
    /// Why the filter excluded it (`ignored`, `expression`, ...), if it did.
    pub mismatch_reason: Option<String>,
}

impl ListedTest {
    /// The tier this test belongs to, from its name suffix.
    #[must_use]
    pub fn tier(&self) -> Option<&'static TierSpec> {
        TIERS.iter().find(|t| self.name.ends_with(t.suffix))
    }

    /// `binary_id name`, as nextest prints it in status lines.
    #[must_use]
    pub fn display(&self) -> String {
        format!("{} {}", self.binary_id, self.name)
    }
}

#[derive(Deserialize)]
struct ListJson {
    #[serde(rename = "rust-suites")]
    rust_suites: BTreeMap<String, SuiteJson>,
}

#[derive(Deserialize)]
struct SuiteJson {
    testcases: BTreeMap<String, CaseJson>,
}

#[derive(Deserialize)]
struct CaseJson {
    ignored: bool,
    #[serde(rename = "filter-match")]
    filter_match: FilterMatchJson,
}

#[derive(Deserialize)]
struct FilterMatchJson {
    status: String,
    reason: Option<String>,
}

/// Parses `cargo nextest list --message-format json` output.
///
/// # Errors
///
/// If `json` is not nextest's list format.
pub fn parse_list(json: &str) -> Result<Vec<ListedTest>> {
    let list: ListJson =
        serde_json::from_str(json).context("parsing `cargo nextest list --message-format json`")?;
    let mut tests = Vec::new();
    for (binary_id, suite) in list.rust_suites {
        for (name, case) in suite.testcases {
            let mismatch_reason = if case.filter_match.status == "matches" {
                None
            } else {
                Some(
                    case.filter_match
                        .reason
                        .unwrap_or_else(|| case.filter_match.status.clone()),
                )
            };
            tests.push(ListedTest {
                binary_id: binary_id.clone(),
                name,
                ignored: case.ignored,
                mismatch_reason,
            });
        }
    }
    Ok(tests)
}

/// Checks the listed tests; returns the tier tests found and any problems.
#[must_use]
pub fn check_list(tests: &[ListedTest]) -> (Vec<ListedTest>, Vec<String>) {
    let mut problems = Vec::new();
    let tier_tests: Vec<ListedTest> = tests
        .iter()
        .filter(|t| t.tier().is_some())
        .cloned()
        .collect();
    for sentinel in SENTINEL_BINARIES {
        for tier in TIERS {
            if !tier_tests
                .iter()
                .any(|t| t.binary_id == *sentinel && t.tier() == Some(tier))
            {
                problems.push(format!(
                    "no {} tier test (name ending `{}`) in `{sentinel}`: the tier macros no longer \
                     name tests after the variable to set, or the sentinel test is gone",
                    tier.label, tier.suffix
                ));
            }
        }
    }
    for test in &tier_tests {
        if !test.ignored {
            problems.push(format!(
                "`{}` is not ignored with its tier absent: it would run (and could pass) instead of \
                 being reported as skipped",
                test.display()
            ));
        } else if test.mismatch_reason.as_deref() != Some("ignored") {
            problems.push(format!(
                "`{}` is excluded for {:?}, not because it is ignored",
                test.display(),
                test.mismatch_reason
            ));
        }
    }
    (tier_tests, problems)
}

/// A nextest status line: `STATUS [ time ] (progress) binary-id test-name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    /// The status word, e.g. `PASS`, `FAIL`, `SKIP`.
    pub status: String,
    /// `binary-id test-name`.
    pub test: String,
}

/// Extracts the per-test status lines from nextest's human output (run with
/// `--color never --status-level all`).
#[must_use]
pub fn parse_status_lines(output: &str) -> Vec<StatusLine> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (status, rest) = line.split_once(' ')?;
            if status.is_empty() || !status.chars().all(|c| c.is_ascii_uppercase()) {
                return None;
            }
            let rest = rest.trim_start().strip_prefix('[')?;
            let (_, rest) = rest.split_once(']')?;
            let rest = rest.trim_start().strip_prefix('(')?;
            let (_, rest) = rest.split_once(')')?;
            Some(StatusLine {
                status: status.to_owned(),
                test: rest.trim().to_owned(),
            })
        })
        .collect()
}

/// The `N passed` count from nextest's `Summary` line.
#[must_use]
pub fn summary_passed(output: &str) -> Option<u64> {
    let line = output
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with("Summary"))?;
    let (_, counts) = line.split_once("tests run:")?;
    counts.split(',').find_map(|part| {
        let part = part.trim();
        part.strip_suffix(" passed")?.trim().parse().ok()
    })
}

fn statuses_by_test(output: &str) -> BTreeMap<String, BTreeSet<String>> {
    let mut map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for line in parse_status_lines(output) {
        map.entry(line.test).or_default().insert(line.status);
    }
    map
}

fn check_zero_passed(output: &str, what: &str, problems: &mut Vec<String>) {
    match summary_passed(output) {
        Some(0) => {}
        Some(n) => problems.push(format!(
            "{what}: nextest summary counts {n} passed; tier tests must never count as passed"
        )),
        None => problems.push(format!(
            "{what}: no nextest `Summary ... N passed` line found"
        )),
    }
}

/// Checks the default run (tier absent, ignored tests not forced): it must
/// succeed and report every tier test as `SKIP` only.
#[must_use]
pub fn check_default_run(tier_tests: &[ListedTest], success: bool, output: &str) -> Vec<String> {
    let what = "default run";
    let mut problems = Vec::new();
    if !success {
        problems.push(format!("{what}: `cargo nextest run` failed"));
    }
    let statuses = statuses_by_test(output);
    for test in tier_tests {
        let shown = test.display();
        match statuses.get(&shown) {
            Some(set) if set.iter().all(|s| s == "SKIP") => {}
            Some(set) => problems.push(format!(
                "{what}: `{shown}` reported as {set:?}, expected only SKIP"
            )),
            None => problems.push(format!("{what}: `{shown}` not reported as SKIP")),
        }
    }
    check_zero_passed(output, what, &mut problems);
    problems
}

/// Checks the forced run (`--run-ignored only`, tier absent): it must fail,
/// every tier test must fail (never pass), and the output must name the
/// variable to set for every tier present.
#[must_use]
pub fn check_forced_run(tier_tests: &[ListedTest], success: bool, output: &str) -> Vec<String> {
    let what = "forced run (--run-ignored only)";
    let mut problems = Vec::new();
    if success {
        problems.push(format!(
            "{what}: `cargo nextest run` succeeded; a tier test without its tier must fail"
        ));
    }
    let statuses = statuses_by_test(output);
    for test in tier_tests {
        let shown = test.display();
        match statuses.get(&shown) {
            Some(set) if !set.is_empty() && set.iter().all(|s| s != "PASS" && s != "SKIP") => {}
            Some(set) => problems.push(format!(
                "{what}: `{shown}` reported as {set:?}, expected a failure and never PASS"
            )),
            None => problems.push(format!("{what}: `{shown}` was not run")),
        }
    }
    for tier in TIERS {
        if tier_tests.iter().any(|t| t.tier() == Some(tier)) {
            let needle = format!("set {}", tier.env_var);
            if !output.contains(&needle) {
                problems.push(format!(
                    "{what}: no `{needle}` in the failure output of the {} tier tests",
                    tier.label
                ));
            }
        }
    }
    check_zero_passed(output, what, &mut problems);
    problems
}

fn sentinel_display((binary, name): (&str, &str)) -> String {
    format!("{binary} {name}")
}

/// Checks the listing with the private tier present: every present sentinel is
/// listed, not ignored, and matched by the filter (so it will run).
#[must_use]
pub fn check_present_list(tests: &[ListedTest]) -> Vec<String> {
    let what = "private tier present: list";
    let mut problems = Vec::new();
    for &(binary, name) in PRESENT_SENTINELS {
        let shown = sentinel_display((binary, name));
        match tests
            .iter()
            .find(|t| t.binary_id == binary && t.name == name)
        {
            None => problems.push(format!(
                "{what}: `{shown}` is not listed; the private sentinel test is gone or renamed"
            )),
            Some(t) if t.ignored => problems.push(format!(
                "{what}: `{shown}` is still ignored with the feature on and {} set: it would be \
                 skipped instead of run",
                PRIVATE_TIER.env_var
            )),
            Some(t) if t.mismatch_reason.is_some() => problems.push(format!(
                "{what}: `{shown}` is excluded ({:?}); it would not run",
                t.mismatch_reason
            )),
            Some(_) => {}
        }
    }
    problems
}

/// Checks the run with the private tier present and the matching nonce: it
/// must succeed with every present sentinel `PASS` only and counted as passed.
#[must_use]
pub fn check_present_run(success: bool, output: &str) -> Vec<String> {
    let what = "private tier present: run";
    let mut problems = Vec::new();
    if !success {
        problems.push(format!("{what}: `cargo nextest run` failed"));
    }
    let statuses = statuses_by_test(output);
    for &sentinel in PRESENT_SENTINELS {
        let shown = sentinel_display(sentinel);
        match statuses.get(&shown) {
            Some(set) if set.iter().all(|s| s == "PASS") && !set.is_empty() => {}
            Some(set) => problems.push(format!(
                "{what}: `{shown}` reported as {set:?}, expected only PASS"
            )),
            None => problems.push(format!("{what}: `{shown}` was not run")),
        }
    }
    let expected = PRESENT_SENTINELS.len() as u64;
    match summary_passed(output) {
        Some(n) if n == expected => {}
        Some(n) => problems.push(format!(
            "{what}: nextest summary counts {n} passed, expected {expected}"
        )),
        None => problems.push(format!(
            "{what}: no nextest `Summary ... N passed` line found"
        )),
    }
    problems
}

/// Checks the run with the private tier present but a nonce that does not match
/// the planted one: it must fail, with the [`NONCE_SENTINEL`] failed (never
/// passed) and its failure naming [`NONCE_FILE`]. A pass here would mean the
/// test body did not read the directory it was given.
#[must_use]
pub fn check_mismatch_run(success: bool, output: &str) -> Vec<String> {
    let what = "private tier present, wrong nonce: run";
    let mut problems = Vec::new();
    if success {
        problems.push(format!(
            "{what}: `cargo nextest run` succeeded; the sentinel did not check the planted file"
        ));
    }
    let shown = sentinel_display(NONCE_SENTINEL);
    match statuses_by_test(output).get(&shown) {
        Some(set) if set.iter().any(|s| s == "FAIL") && !set.contains("PASS") => {}
        Some(set) => problems.push(format!(
            "{what}: `{shown}` reported as {set:?}, expected FAIL and never PASS"
        )),
        None => problems.push(format!("{what}: `{shown}` was not run")),
    }
    if !output.contains(NONCE_FILE) {
        problems.push(format!(
            "{what}: the failure output does not name the planted `{NONCE_FILE}`"
        ));
    }
    problems
}

/// Outcome of [`run`].
#[derive(Debug, Default)]
pub struct Report {
    /// Tier tests checked, per feature-set label.
    pub checked: Vec<(String, usize)>,
    /// Private sentinel tests run (and passed, then failed on a wrong nonce)
    /// with the private tier present.
    pub present_checked: usize,
    /// Everything that went wrong; empty means the self-test passed.
    pub problems: Vec<String>,
}

fn nextest(root: &Path, target_dir: &Path, features: &[&str], args: &[&str]) -> Command {
    let mut cmd = nextest_base(root, target_dir, args);
    cmd.args(["--workspace", "-E", FILTER]);
    if !features.is_empty() {
        cmd.arg("--features").arg(features.join(","));
    }
    cmd
}

/// `cargo nextest <args>` with colour off, the nested target directory, and
/// every tier variable (and [`NONCE_ENV`]) removed.
fn nextest_base(root: &Path, target_dir: &Path, args: &[&str]) -> Command {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(root)
        .arg("nextest")
        .args(args)
        .args(["--color", "never"])
        .env("CARGO_TARGET_DIR", target_dir)
        .env_remove(NONCE_ENV);
    for tier in TIERS {
        cmd.env_remove(tier.env_var);
    }
    cmd
}

/// `cargo nextest <args>` over the [`PRESENT_PACKAGES`] with the private tier
/// present: `--features private-fixtures`, `RR_FIXTURES_DIR=<private_dir>`,
/// and `nonce` (if any) in [`NONCE_ENV`].
fn nextest_present(
    root: &Path,
    target_dir: &Path,
    private_dir: &Path,
    nonce: Option<&str>,
    args: &[&str],
) -> Command {
    let mut cmd = nextest_base(root, target_dir, args);
    for package in PRESENT_PACKAGES {
        cmd.args(["-p", package]);
    }
    cmd.args(["--features", "private-fixtures", "-E", &present_filter()])
        .env(PRIVATE_TIER.env_var, private_dir);
    if let Some(nonce) = nonce {
        cmd.env(NONCE_ENV, nonce);
    }
    cmd
}

fn output_of(mut cmd: Command) -> Result<(bool, String, String)> {
    let out = cmd
        .output()
        .with_context(|| format!("running {cmd:?} (is cargo-nextest installed?)"))?;
    Ok((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

/// Runs the self-test against the workspace at `root`.
///
/// # Errors
///
/// If cargo / cargo-nextest cannot be run or `nextest list` fails.
pub fn run(root: &Path) -> Result<Report> {
    let target_dir: PathBuf = root.join(TARGET_DIR);
    let mut report = Report::default();
    for features in FEATURE_SETS {
        let label = if features.is_empty() {
            "default features".to_owned()
        } else {
            format!("--features {}", features.join(","))
        };

        let (ok, stdout, stderr) = output_of(nextest(
            root,
            &target_dir,
            features,
            &["list", "--message-format", "json"],
        ))?;
        if !ok {
            bail!("[{label}] `cargo nextest list` failed:\n{stderr}");
        }
        let (tier_tests, problems) = check_list(&parse_list(&stdout)?);
        report
            .problems
            .extend(problems.into_iter().map(|p| format!("[{label}] list: {p}")));

        let (ok, stdout, stderr) = output_of(nextest(root, &target_dir, features, &RUN_ARGS))?;
        report.problems.extend(
            check_default_run(&tier_tests, ok, &format!("{stdout}\n{stderr}"))
                .into_iter()
                .map(|p| format!("[{label}] {p}")),
        );

        let mut forced_args = RUN_ARGS.to_vec();
        forced_args.extend(["--run-ignored", "only"]);
        let (ok, stdout, stderr) = output_of(nextest(root, &target_dir, features, &forced_args))?;
        report.problems.extend(
            check_forced_run(&tier_tests, ok, &format!("{stdout}\n{stderr}"))
                .into_iter()
                .map(|p| format!("[{label}] {p}")),
        );

        report.checked.push((label, tier_tests.len()));
    }
    run_present(root, &target_dir, &mut report)?;
    Ok(report)
}

/// The run arguments shared by every `cargo nextest run` here.
const RUN_ARGS: [&str; 7] = [
    "run",
    "--no-fail-fast",
    "--no-tests=pass",
    "--status-level",
    "all",
    "--final-status-level",
    "none",
];

/// Steps 4–6 (module docs): the private tier present, in a scratch directory
/// under the system temp dir (outside the repo, as `fixture_root` requires),
/// removed afterwards.
fn run_present(root: &Path, target_dir: &Path, report: &mut Report) -> Result<()> {
    let nonce = format!(
        "{:016x}",
        RandomState::new().hash_one((std::process::id(), root))
    );
    let private_dir = std::env::temp_dir().join(format!(
        "rr-fixtures-selftest-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&private_dir)
        .with_context(|| format!("creating {}", private_dir.display()))?;
    let result = run_present_in(root, target_dir, &private_dir, &nonce, report);
    let _ = std::fs::remove_dir_all(&private_dir);
    result
}

fn run_present_in(
    root: &Path,
    target_dir: &Path,
    private_dir: &Path,
    nonce: &str,
    report: &mut Report,
) -> Result<()> {
    let planted = private_dir.join(NONCE_FILE);
    std::fs::write(&planted, nonce).with_context(|| format!("writing {}", planted.display()))?;

    let (ok, stdout, stderr) = output_of(nextest_present(
        root,
        target_dir,
        private_dir,
        None,
        &["list", "--message-format", "json"],
    ))?;
    if !ok {
        bail!("[private tier present] `cargo nextest list` failed:\n{stderr}");
    }
    report
        .problems
        .extend(check_present_list(&parse_list(&stdout)?));

    let (ok, stdout, stderr) = output_of(nextest_present(
        root,
        target_dir,
        private_dir,
        Some(nonce),
        &RUN_ARGS,
    ))?;
    report
        .problems
        .extend(check_present_run(ok, &format!("{stdout}\n{stderr}")));

    let wrong = format!("not-{nonce}");
    let (ok, stdout, stderr) = output_of(nextest_present(
        root,
        target_dir,
        private_dir,
        Some(&wrong),
        &RUN_ARGS,
    ))?;
    report
        .problems
        .extend(check_mismatch_run(ok, &format!("{stdout}\n{stderr}")));

    report.present_checked = PRESENT_SENTINELS.len();
    Ok(())
}

/// One-line summary of what a passing report checked.
#[must_use]
pub fn summary(report: &Report) -> String {
    let mut s = String::new();
    for (i, (label, n)) in report.checked.iter().enumerate() {
        if i > 0 {
            s.push_str("; ");
        }
        let _ = write!(
            s,
            "{label}: {n} tier test(s) skipped, and failed when forced"
        );
    }
    let _ = write!(
        s,
        "; private tier present: {} sentinel test(s) ran and passed, and failed on a wrong nonce",
        report.present_checked
    );
    s
}
