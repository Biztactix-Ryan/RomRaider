//! Tests for `cargo xtask fixtures-selftest` (`xtask::fixtures_selftest`).
//!
//! US-RR-6-1: "Tests requiring the external or private corpus are reported as
//! skipped (with the env var to set) when it is absent, and never counted as
//! passed." The end-to-end proof is the CI step `cargo xtask
//! fixtures-selftest` in the `clippy` job, which runs cargo-nextest for real
//! with `RR_CORPUS_DIR` / `RR_FIXTURES_DIR` removed. These tests prove the
//! checker itself: it accepts nextest's real output (captured from such a
//! run) and rejects every mutation that would let a tier test pass or go
//! unreported. [`ci_clippy_job_runs_fixtures_selftest`] keeps the CI step from
//! being dropped.

use xtask::fixtures_selftest::{
    FILTER, ListedTest, SENTINEL_BINARIES, TIERS, check_default_run, check_forced_run, check_list,
    parse_list, parse_status_lines, summary_passed,
};

const CORPUS_ROM: &str = "stock_rom_corpus_is_reachable::requires_RR_CORPUS_DIR";
const PRIVATE_ROM: &str =
    "private_corpus_is_reachable::requires_feature_private_fixtures_and_RR_FIXTURES_DIR";
const CORPUS_KIT: &str = "corpus_tier_root_is_a_directory::requires_RR_CORPUS_DIR";
const PRIVATE_KIT: &str =
    "private_tier_root_is_a_directory::requires_feature_private_fixtures_and_RR_FIXTURES_DIR";

/// `cargo nextest list --workspace -E FILTER --message-format json`, trimmed
/// to the two sentinel binaries plus one ordinary test, as nextest 0.9
/// prints it with both tiers absent.
fn list_json(ignored: bool, reason: &str, corpus_rom: &str) -> String {
    let case = |name: &str| {
        format!(
            r#""{name}": {{"kind": "test", "ignored": {ignored},
                "filter-match": {{"status": "mismatch", "reason": "{reason}"}}}}"#
        )
    };
    format!(
        r#"{{
  "rust-build-meta": {{}},
  "test-count": 5,
  "rust-suites": {{
    "rr-rom::corpus_tier": {{
      "package-name": "rr-rom", "binary-id": "rr-rom::corpus_tier", "kind": "test",
      "status": "listed",
      "testcases": {{ {}, {} }}
    }},
    "rr-testkit::fixtures": {{
      "package-name": "rr-testkit", "binary-id": "rr-testkit::fixtures", "kind": "test",
      "status": "listed",
      "testcases": {{
        {}, {},
        "rejects_newer_schema_naming_file_found_and_supported": {{"kind": "test",
          "ignored": false, "filter-match": {{"status": "mismatch", "reason": "expression"}}}}
      }}
    }}
  }}
}}"#,
        case(corpus_rom),
        case(PRIVATE_ROM),
        case(CORPUS_KIT),
        case(PRIVATE_KIT)
    )
}

fn real_list() -> anyhow::Result<Vec<ListedTest>> {
    parse_list(&list_json(true, "ignored", CORPUS_ROM))
}

fn line(status: &str, timing: &str, progress: &str, binary: &str, name: &str) -> String {
    format!("        {status} [{timing}] ({progress}) {binary} {name}\n")
}

/// The default (not forced) run, as nextest prints it with `--color never
/// --status-level all --final-status-level none`.
fn default_run(status_for_corpus_rom: &str, passed: u32) -> String {
    let skip = "         ";
    let mut out = String::from(
        "    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.03s\n\
         ────────────\n Nextest run ID d6a7 with nextest profile: default\n    \
         Starting 0 tests across 28 binaries (220 tests skipped)\n",
    );
    out += &line(
        status_for_corpus_rom,
        skip,
        "───",
        "rr-rom::corpus_tier",
        CORPUS_ROM,
    );
    out += &line("SKIP", skip, "───", "rr-rom::corpus_tier", PRIVATE_ROM);
    out += &line("SKIP", skip, "───", "rr-testkit::fixtures", CORPUS_KIT);
    out += &line("SKIP", skip, "───", "rr-testkit::fixtures", PRIVATE_KIT);
    out += &line(
        "SKIP",
        skip,
        "───",
        "xtask::time_exceptions",
        "only_realclock_opts_out",
    );
    out += &format!(
        "────────────\n     Summary [   0.000s] {passed} tests run: {passed} passed, 220 skipped\n"
    );
    out
}

/// The forced run (`--run-ignored only`), abridged from a real run.
fn forced_run(status_for_private_kit: &str, passed: u32, corpus_message: &str) -> String {
    let mut out = String::from("    Starting 4 tests across 28 binaries (207 tests skipped)\n");
    out += &line(
        "FAIL",
        "   0.003s",
        "1/4",
        "rr-testkit::fixtures",
        CORPUS_KIT,
    );
    out += &format!(
        "  output ───\n    thread '{CORPUS_KIT}' (1) panicked at crates/rr-testkit/tests/fixtures.rs:206:1:\n    \
         {corpus_message}\n    test result: FAILED. 0 passed; 1 failed; 0 ignored\n"
    );
    out += &line(
        "FAIL",
        "   0.003s",
        "2/4",
        "rr-rom::corpus_tier",
        CORPUS_ROM,
    );
    out += &line(
        "FAIL",
        "   0.003s",
        "3/4",
        "rr-rom::corpus_tier",
        PRIVATE_ROM,
    );
    out += "    private fixtures are disabled in this build: build with `--features \
            private-fixtures` and set RR_FIXTURES_DIR to run this test\n";
    out += &line(
        status_for_private_kit,
        "   0.004s",
        "4/4",
        "rr-testkit::fixtures",
        PRIVATE_KIT,
    );
    let failed = 4 - passed;
    out += &format!(
        "────────────\n     Summary [   0.004s] 4 tests run: {passed} passed, {failed} failed, 207 skipped\n\
         error: test run failed\n"
    );
    out
}

const CORPUS_MESSAGE: &str =
    "external corpus fixtures are absent: set RR_CORPUS_DIR to run this test";

#[test]
fn tier_constants_agree() {
    for tier in TIERS {
        assert!(tier.suffix.contains(tier.env_var), "{tier:?}");
        let alternative = tier.suffix.trim_start_matches("::requires_");
        assert!(FILTER.contains(alternative), "{FILTER} lacks {alternative}");
    }
    assert!(SENTINEL_BINARIES.contains(&"rr-rom::corpus_tier"));
    assert!(SENTINEL_BINARIES.contains(&"rr-testkit::fixtures"));
}

/// US-RR-6-1: the real listing (every tier test ignored, excluded because it
/// is ignored, named after its variable) passes.
#[test]
fn list_check_accepts_ignored_tier_tests() {
    let (tier_tests, problems) = check_list(&real_list().unwrap());
    assert_eq!(problems, Vec::<String>::new());
    assert_eq!(tier_tests.len(), 4);
}

/// US-RR-6-1: a tier test that is not `#[ignore]`d with its tier absent (it
/// would run and could pass) is rejected.
#[test]
fn list_check_rejects_tier_test_not_ignored() {
    let tests = parse_list(&list_json(false, "expression", CORPUS_ROM)).unwrap();
    let (_, problems) = check_list(&tests);
    assert_eq!(problems.len(), 4, "{problems:#?}");
    assert!(
        problems.iter().all(|p| p.contains("is not ignored")),
        "{problems:#?}"
    );
}

#[test]
fn list_check_rejects_exclusion_for_another_reason() {
    let tests = parse_list(&list_json(true, "expression", CORPUS_ROM)).unwrap();
    let (_, problems) = check_list(&tests);
    assert_eq!(problems.len(), 4, "{problems:#?}");
}

/// US-RR-6-1: a test name that no longer spells the variable to set (here the
/// rr-rom corpus test) leaves a sentinel without a corpus test: rejected, so
/// the check can never pass vacuously.
#[test]
fn list_check_rejects_name_without_env_var() {
    let tests = parse_list(&list_json(true, "ignored", "stock_rom_corpus_is_reachable")).unwrap();
    let (tier_tests, problems) = check_list(&tests);
    assert_eq!(tier_tests.len(), 3);
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert!(problems[0].contains("rr-rom::corpus_tier"), "{problems:#?}");
    assert!(
        problems[0].contains("requires_RR_CORPUS_DIR"),
        "{problems:#?}"
    );
}

#[test]
fn parses_status_lines_and_summary() {
    let lines = parse_status_lines(&default_run("SKIP", 0));
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[0].status, "SKIP");
    assert_eq!(lines[0].test, format!("rr-rom::corpus_tier {CORPUS_ROM}"));
    assert_eq!(summary_passed(&default_run("SKIP", 0)), Some(0));

    let forced = parse_status_lines(&forced_run("FAIL", 0, CORPUS_MESSAGE));
    assert_eq!(forced.len(), 4, "{forced:#?}");
    assert!(forced.iter().all(|l| l.status == "FAIL"));
    assert_eq!(
        forced[3].test,
        format!("rr-testkit::fixtures {PRIVATE_KIT}")
    );
    assert_eq!(
        summary_passed(&forced_run("FAIL", 0, CORPUS_MESSAGE)),
        Some(0)
    );
    assert_eq!(summary_passed("no summary here"), None);
}

/// US-RR-6-1: the real default run (every tier test SKIP, 0 passed) passes.
#[test]
fn default_run_check_accepts_skips() {
    let (tier_tests, _) = check_list(&real_list().unwrap());
    let problems = check_default_run(&tier_tests, true, &default_run("SKIP", 0));
    assert_eq!(problems, Vec::<String>::new());
}

/// US-RR-6-1: a tier test reported as PASS, or a summary counting it as
/// passed, is rejected.
#[test]
fn default_run_check_rejects_pass() {
    let (tier_tests, _) = check_list(&real_list().unwrap());
    let problems = check_default_run(&tier_tests, true, &default_run("PASS", 1));
    assert_eq!(problems.len(), 2, "{problems:#?}");
    assert!(problems[0].contains("expected only SKIP"), "{problems:#?}");
    assert!(problems[1].contains("1 passed"), "{problems:#?}");
}

#[test]
fn default_run_check_rejects_missing_report_and_failure() {
    let (tier_tests, _) = check_list(&real_list().unwrap());
    let output = default_run("SKIP", 0).replace(CORPUS_ROM, "something_else");
    let problems = check_default_run(&tier_tests, false, &output);
    assert_eq!(problems.len(), 2, "{problems:#?}");
    assert!(problems[0].contains("failed"), "{problems:#?}");
    assert!(
        problems[1].contains("not reported as SKIP"),
        "{problems:#?}"
    );
}

/// US-RR-6-1: forcing the skipped tests to run without their tier makes the
/// run fail, every tier test fail, and the output name the variables.
#[test]
fn forced_run_check_accepts_failures() {
    let (tier_tests, _) = check_list(&real_list().unwrap());
    let problems = check_forced_run(&tier_tests, false, &forced_run("FAIL", 0, CORPUS_MESSAGE));
    assert_eq!(problems, Vec::<String>::new());
}

/// US-RR-6-1: a forced tier test that passes without its tier (e.g. a macro
/// that no longer injects `require_tier`) is rejected, as is a run that
/// succeeds.
#[test]
fn forced_run_check_rejects_vacuous_pass() {
    let (tier_tests, _) = check_list(&real_list().unwrap());
    let problems = check_forced_run(&tier_tests, true, &forced_run("PASS", 1, CORPUS_MESSAGE));
    assert_eq!(problems.len(), 3, "{problems:#?}");
    assert!(problems[0].contains("succeeded"), "{problems:#?}");
    assert!(problems[1].contains(PRIVATE_KIT), "{problems:#?}");
    assert!(problems[2].contains("1 passed"), "{problems:#?}");
}

#[test]
fn forced_run_check_rejects_failure_without_env_var() {
    let (tier_tests, _) = check_list(&real_list().unwrap());
    let output = forced_run("FAIL", 0, "corpus missing");
    let problems = check_forced_run(&tier_tests, false, &output);
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert!(problems[0].contains("set RR_CORPUS_DIR"), "{problems:#?}");
}

#[test]
fn forced_run_check_rejects_unrun_test() {
    let (tier_tests, _) = check_list(&real_list().unwrap());
    let output = forced_run("FAIL", 0, CORPUS_MESSAGE).replace(CORPUS_ROM, "renamed");
    let problems = check_forced_run(&tier_tests, false, &output);
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert!(problems[0].contains("was not run"), "{problems:#?}");
}

/// US-RR-6-1: CI's `clippy` job (Linux, alongside lint-selftest) runs the end-to-end self-test, on a step that
/// is neither conditional nor `continue-on-error`.
#[test]
fn ci_clippy_job_runs_fixtures_selftest() {
    let ci = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    ))
    .unwrap();
    let mut job = String::new();
    let mut in_job = false;
    for l in ci.lines() {
        let is_job_key = l.starts_with("  ") && !l.starts_with("   ") && l.ends_with(':');
        if is_job_key {
            in_job = l == "  clippy:";
        } else if in_job {
            job.push_str(l);
            job.push('\n');
        }
    }
    assert!(!job.is_empty(), "ci.yml has a `clippy` job");
    let step = job
        .split("\n      - ")
        .find(|s| s.contains("run: cargo xtask fixtures-selftest"))
        .expect("the `clippy` job has a `run: cargo xtask fixtures-selftest` step");
    assert!(!step.contains("continue-on-error"), "{step}");
    assert!(!step.contains("if:"), "{step}");
}
