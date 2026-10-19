//! T6 safety-invariant suite: the T6 suite itself stays wired into CI as a
//! release-blocking gate (PROJECT.md Testing Strategy, story US-RR-7).
//!
//! Proves acceptance criterion **US-RR-7-4** ("The T6 suite runs in ci.yml as
//! a release-blocking job") and keeps it from regressing silently. Against the
//! committed `.github/workflows/ci.yml` it asserts that:
//!
//! * the workflow runs on `push` and `pull_request`;
//! * a job `t6` exists and runs `cargo xtask check-writes`, the workspace
//!   nextest run with exactly the T6 selection filter ([`T6_FILTER`]) and
//!   `--no-tests=fail` (an empty selection is a failure), and the rr-core
//!   capability tests with the `confirmation-authority` feature;
//! * nothing in the `t6` job can skip or soften a step (`if:`,
//!   `continue-on-error`, `|| true`);
//! * the aggregate `ci-ok` job lists `t6` in `needs`, runs `if: always()`, and
//!   fails when any needed job failed, was cancelled or was skipped.
//!
//! "Release-blocking" here means: `t6` is part of `ci-ok`, the single
//! required status check (ci.yml header). `release.yml` does not exist yet;
//! when it is written it must `needs:` `t6` or gate on `ci-ok` for the tagged
//! commit (ci.yml comment on the `t6` job), and this file should then grow a
//! matching assertion.
//!
//! ci.yml is read with the std-only [`Workflow`] reader shared with the other
//! ci-config guards (`common/workflow.rs`). Anything it cannot find is
//! reported as missing, so a layout change fails closed. The
//! `t6_ci_check_rejects_*` tests prove each assertion bites on a mutated copy
//! of the real file.

#[path = "common/workflow.rs"]
mod workflow;

use workflow::{GATE_JOB, Job, Workflow, assert_flags, ci_yml, indent, key_at_any, normalize};

/// The nextest filter expression that selects the T6 suite in CI. Changing
/// the selection in ci.yml must be a deliberate change here too.
const T6_FILTER: &str =
    "binary(/^t6_/) | test(/(^|::)t6_/) | (package(rr-core) & test(/^capability::/))";

/// The job that runs the T6 suite.
const T6_JOB: &str = "t6";

/// The `-E '<filter>'` argument of a normalized command, if any.
fn filter_arg(cmd: &str) -> Option<&str> {
    let rest = &cmd[cmd.find("-E '")? + 4..];
    Some(&rest[..rest.find('\'')?])
}

/// Every way `src` fails the US-RR-7-4 invariant; empty when it holds.
fn check(src: &str) -> Vec<String> {
    let wf = Workflow::parse(src);
    let mut problems = Vec::new();

    if !wf.top_level_keys.contains(&"jobs") {
        problems.push("no top-level `jobs:`".to_owned());
    }
    for trigger in ["push", "pull_request"] {
        if key_at_any(wf.on_block(), trigger).is_none() {
            problems.push(format!("workflow does not run on `{trigger}`"));
        }
    }

    match wf.job(T6_JOB) {
        None => problems.push(format!("no `{T6_JOB}` job")),
        Some(t6) => check_t6_job(t6, &mut problems),
    }

    match wf.job(GATE_JOB) {
        None => problems.push(format!("no `{GATE_JOB}` job")),
        Some(gate) => {
            if !gate.needs().iter().any(|n| n == T6_JOB) {
                problems.push(format!(
                    "`{GATE_JOB}.needs` does not include `{T6_JOB}` (needs: {:?})",
                    gate.needs()
                ));
            }
            let content: Vec<&str> = gate.content_lines().collect();
            if !content
                .iter()
                .any(|l| normalize(l) == "if: always()" && l.starts_with("if:"))
            {
                problems.push(format!(
                    "`{GATE_JOB}` is not `if: always()`, so a failed gate skips it"
                ));
            }
            let joined = content.join("\n");
            for result in ["failure", "cancelled", "skipped"] {
                if !joined.contains(&format!("contains(needs.*.result, '{result}')")) {
                    problems.push(format!(
                        "`{GATE_JOB}` does not fail on a needed job's `{result}` result"
                    ));
                }
            }
            if !gate.run_scripts().iter().any(|s| s.contains("exit 1")) {
                problems.push(format!("`{GATE_JOB}` has no failing (`exit 1`) step"));
            }
            if content.iter().any(|l| l.starts_with("continue-on-error")) {
                problems.push(format!("`{GATE_JOB}` uses continue-on-error"));
            }
        }
    }
    problems
}

fn check_t6_job(t6: &Job<'_>, problems: &mut Vec<String>) {
    let scripts = t6.run_scripts();

    if !scripts.iter().any(|s| s == "cargo xtask check-writes") {
        problems.push(format!(
            "`{T6_JOB}` does not run `cargo xtask check-writes`"
        ));
    }

    let suite = scripts.iter().find(|s| {
        s.starts_with("cargo nextest run")
            && s.contains(" --workspace")
            && filter_arg(s) == Some(T6_FILTER)
    });
    match suite {
        None => problems.push(format!(
            "`{T6_JOB}` does not run `cargo nextest run --workspace ... -E '{T6_FILTER}'`"
        )),
        Some(s) => {
            for flag in ["--no-tests=fail", "--locked"] {
                if !s.split(' ').any(|w| w == flag) {
                    problems.push(format!("`{T6_JOB}` T6 nextest run lacks `{flag}`: {s}"));
                }
            }
        }
    }

    let authority = scripts.iter().any(|s| {
        s.starts_with("cargo nextest run")
            && s.contains("-p rr-core")
            && s.contains("--features confirmation-authority")
            && filter_arg(s) == Some("test(/^capability::/)")
    });
    if !authority {
        problems.push(format!(
            "`{T6_JOB}` does not run the rr-core capability tests with `--features confirmation-authority`"
        ));
    }

    for line in t6.content_lines() {
        let l = line.strip_prefix("- ").unwrap_or(line);
        if l.starts_with("if:") {
            problems.push(format!("`{T6_JOB}` has a conditional (`{l}`)"));
        }
        if l.starts_with("continue-on-error") {
            problems.push(format!("`{T6_JOB}` uses `{l}`"));
        }
    }
    for s in &scripts {
        if s.contains("|| true") || s.contains("|| :") || s.contains("exit 0") {
            problems.push(format!("`{T6_JOB}` step swallows failures: {s}"));
        }
    }
}

// --- The invariant on the real file -----------------------------------------

/// US-RR-7-4: ci.yml runs the T6 suite in job `t6`, and `ci-ok` (the
/// required check) depends on it.
#[test]
fn t6_ci_runs_t6_suite_as_release_blocking_job() {
    let src = ci_yml();
    let problems = check(&src);
    assert!(
        problems.is_empty(),
        ".github/workflows/ci.yml violates US-RR-7-4:\n{}",
        problems.join("\n")
    );
}

// --- Each assertion bites (mutations of the real file) ----------------------

/// The problems [`check`] finds in the real ci.yml after `edit`.
fn mutated(edit: impl Fn(&str) -> String) -> Vec<String> {
    workflow::mutated(check, edit)
}

/// US-RR-7-4: dropping `t6` from `ci-ok.needs` is caught.
#[test]
fn t6_ci_check_rejects_t6_missing_from_gate_needs() {
    let p = mutated(|s| {
        s.split_inclusive('\n')
            .map(|l| {
                if l.trim_start().starts_with("needs:") && indent(l) == 4 {
                    l.replace(", t6", "").replace("t6, ", "")
                } else {
                    l.to_owned()
                }
            })
            .collect::<String>()
    });
    assert_flags(&p, "`ci-ok.needs` does not include `t6`");
}

/// US-RR-7-4: removing or renaming the `t6` job is caught.
#[test]
fn t6_ci_check_rejects_missing_t6_job() {
    let p = mutated(|s| s.replace("\n  t6:\n", "\n  safety:\n"));
    assert_flags(&p, "no `t6` job");
}

/// US-RR-7-4: dropping `--no-tests=fail`, narrowing the filter, or removing
/// check-writes / the authority-feature run is caught.
#[test]
fn t6_ci_check_rejects_weakened_t6_steps() {
    assert_flags(
        &mutated(|s| s.replacen(" --no-tests=fail", "", 1)),
        "lacks `--no-tests=fail`",
    );
    assert_flags(
        &mutated(|s| s.replace("binary(/^t6_/) | ", "")),
        "does not run `cargo nextest run --workspace",
    );
    assert_flags(
        &mutated(|s| s.replace("run: cargo xtask check-writes", "run: echo skipped")),
        "does not run `cargo xtask check-writes`",
    );
    assert_flags(
        &mutated(|s| s.replace("--features confirmation-authority", "")),
        "--features confirmation-authority",
    );
}

/// US-RR-7-4: making a T6 step conditional or non-fatal is caught.
#[test]
fn t6_ci_check_rejects_skippable_t6_steps() {
    assert_flags(
        &mutated(|s| {
            s.replace(
                "      - name: cargo xtask check-writes\n",
                "      - name: cargo xtask check-writes\n        continue-on-error: true\n",
            )
        }),
        "continue-on-error",
    );
    assert_flags(
        &mutated(|s| {
            s.replace(
                "      - name: cargo xtask check-writes\n",
                "      - name: cargo xtask check-writes\n        if: false\n",
            )
        }),
        "has a conditional",
    );
    assert_flags(
        &mutated(|s| {
            s.replace(
                "run: cargo xtask check-writes",
                "run: cargo xtask check-writes || true",
            )
        }),
        "swallows failures",
    );
}

/// US-RR-7-4: a gate that ignores skipped jobs, or stops running when a
/// needed job fails, is caught.
#[test]
fn t6_ci_check_rejects_weakened_gate() {
    assert_flags(
        &mutated(|s| s.replace(" || contains(needs.*.result, 'skipped')", "")),
        "`skipped` result",
    );
    assert_flags(
        &mutated(|s| s.replace("    if: always()\n", "")),
        "is not `if: always()`",
    );
}

/// The reader accepts the other `needs:` spellings, so reformatting ci.yml
/// does not break the check.
#[test]
fn t6_ci_needs_reader_handles_block_and_scalar_lists() {
    let block = "on:\n  push:\n  pull_request:\njobs:\n  ci-ok:\n    needs:\n      - fmt\n      - t6\n    runs-on: x\n";
    assert_eq!(
        Workflow::parse(block).job(GATE_JOB).map(Job::needs),
        Some(vec!["fmt".to_owned(), "t6".to_owned()])
    );
    let scalar = "jobs:\n  ci-ok:\n    needs: t6\n";
    assert_eq!(
        Workflow::parse(scalar).job(GATE_JOB).map(Job::needs),
        Some(vec!["t6".to_owned()])
    );
    let multiline = "jobs:\n  ci-ok:\n    needs: [fmt,\n      \"t6\"]\n";
    assert_eq!(
        Workflow::parse(multiline).job(GATE_JOB).map(Job::needs),
        Some(vec!["fmt".to_owned(), "t6".to_owned()])
    );
}
