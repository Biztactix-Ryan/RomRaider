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
//! The nested `cargo nextest` calls use their own target directory
//! ([`TARGET_DIR`]), so they never fight the caller's build lock and never
//! rebuild the caller's `target/debug` with a different `RR_*` environment.
//! This is a subcommand (run as its own CI step in the `clippy` job), not a
//! `#[test]`, because a nested cargo inside `cargo test` would run while the
//! outer test run holds the build. The pure evaluation functions below are
//! unit-tested in `xtask/tests/fixtures_selftest.rs` against both real and
//! mutated nextest output.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
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

/// Outcome of [`run`].
#[derive(Debug, Default)]
pub struct Report {
    /// Tier tests checked, per feature-set label.
    pub checked: Vec<(String, usize)>,
    /// Everything that went wrong; empty means the self-test passed.
    pub problems: Vec<String>,
}

fn nextest(root: &Path, target_dir: &Path, features: &[&str], args: &[&str]) -> Command {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(root)
        .arg("nextest")
        .args(args)
        .args(["--workspace", "-E", FILTER, "--color", "never"])
        .env("CARGO_TARGET_DIR", target_dir);
    if !features.is_empty() {
        cmd.arg("--features").arg(features.join(","));
    }
    for tier in TIERS {
        cmd.env_remove(tier.env_var);
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

        let run_args = [
            "run",
            "--no-fail-fast",
            "--no-tests=pass",
            "--status-level",
            "all",
            "--final-status-level",
            "none",
        ];
        let (ok, stdout, stderr) = output_of(nextest(root, &target_dir, features, &run_args))?;
        report.problems.extend(
            check_default_run(&tier_tests, ok, &format!("{stdout}\n{stderr}"))
                .into_iter()
                .map(|p| format!("[{label}] {p}")),
        );

        let mut forced_args = run_args.to_vec();
        forced_args.extend(["--run-ignored", "only"]);
        let (ok, stdout, stderr) = output_of(nextest(root, &target_dir, features, &forced_args))?;
        report.problems.extend(
            check_forced_run(&tier_tests, ok, &format!("{stdout}\n{stderr}"))
                .into_iter()
                .map(|p| format!("[{label}] {p}")),
        );

        report.checked.push((label, tier_tests.len()));
    }
    Ok(report)
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
    s
}
