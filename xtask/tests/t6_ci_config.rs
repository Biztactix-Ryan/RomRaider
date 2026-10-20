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
//! required status check (ci.yml header), and nothing is released without it:
//! `release.yml` runs ci.yml as its job `ci` (`uses:
//! ./.github/workflows/ci.yml`, ci.yml `on: workflow_call`), and every other
//! release job except `plan` needs `ci`, directly or through another job
//! ([`release_problems`], US-RR-38-5). Within the job that publishes, the
//! release artifact content check (`check-release-artifacts`, US-RR-38-8) is
//! the last gate: every step that runs `gh release` comes after it.
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

// --- Nothing is released without the gate (release.yml, US-RR-38-5) ---------

/// The release.yml job that runs the whole of ci.yml.
const RELEASE_CI_JOB: &str = "ci";

/// Release jobs allowed to run before the gate: deciding what to build only.
/// They build and publish nothing.
const PRE_GATE_JOBS: &[&str] = &["plan"];

/// Whether `from` needs `target` in `wf`, directly or through other jobs.
fn needs_transitively(wf: &Workflow<'_>, from: &str, target: &str) -> bool {
    let mut seen: Vec<String> = Vec::new();
    let mut todo = vec![from.to_owned()];
    while let Some(id) = todo.pop() {
        let Some(job) = wf.job(&id) else { continue };
        for need in job.needs() {
            if need == target {
                return true;
            }
            if !seen.contains(&need) {
                seen.push(need.clone());
                todo.push(need);
            }
        }
    }
    false
}

/// Every way `release` (release.yml) with `ci` (ci.yml) lets a release job
/// build or publish without the full ci.yml gate (`t6` and `ci-ok`
/// included) having passed on the commit; empty when it cannot.
fn release_problems(release: &str, ci: &str) -> Vec<String> {
    let mut problems = Vec::new();
    if key_at_any(Workflow::parse(ci).on_block(), "workflow_call").is_none() {
        problems
            .push("ci.yml has no `workflow_call` trigger, so release.yml cannot run it".to_owned());
    }
    let wf = Workflow::parse(release);
    match wf.job(RELEASE_CI_JOB) {
        None => problems.push(format!("release.yml has no `{RELEASE_CI_JOB}` job")),
        Some(job) => {
            if job.value_at("uses", 4).as_deref() != Some("./.github/workflows/ci.yml") {
                problems.push(format!(
                    "release.yml `{RELEASE_CI_JOB}` does not `uses: ./.github/workflows/ci.yml`"
                ));
            }
        }
    }
    let gated: Vec<&Job<'_>> = wf
        .jobs
        .iter()
        .filter(|j| j.id != RELEASE_CI_JOB && !PRE_GATE_JOBS.contains(&j.id))
        .collect();
    if gated.is_empty() {
        problems.push("release.yml has no build or publish jobs".to_owned());
    }
    for job in gated {
        let id = job.id;
        if !needs_transitively(&wf, id, RELEASE_CI_JOB) {
            problems.push(format!(
                "release.yml job `{id}` does not need `{RELEASE_CI_JOB}`, so it can run without the CI gate"
            ));
        }
        // US-RR-38-8: nothing is published before the content check.
        let steps = job.steps();
        let check = steps.iter().position(|st| {
            st.run_scripts()
                .iter()
                .any(|s| s.contains("check-release-artifacts"))
        });
        for (i, st) in steps.iter().enumerate() {
            let publishes = st.run_scripts().iter().any(|s| s.contains("gh release"));
            if publishes && check.is_none_or(|c| c > i) {
                problems.push(format!(
                    "release.yml job `{id}` step `{}` runs `gh release` before (or without) the release artifact content check",
                    st.id
                ));
            }
        }
        for line in job.lines.iter().filter(|l| indent(l) == 4) {
            let l = line.trim();
            // A job-level always()/failure()/cancelled() condition runs the
            // job even when `ci` failed.
            if l.starts_with("if:")
                && ["always()", "failure()", "cancelled()"]
                    .iter()
                    .any(|f| l.contains(f))
            {
                problems.push(format!(
                    "release.yml job `{id}` runs past a failed gate (`{l}`)"
                ));
            }
            if l.starts_with("continue-on-error") {
                problems.push(format!("release.yml job `{id}` uses `{l}`"));
            }
        }
    }
    problems
}

/// US-RR-7-4 / US-RR-38-5: every release.yml build and publish job needs the
/// job that runs ci.yml.
#[test]
fn t6_release_needs_the_ci_gate() {
    let problems = release_problems(&workflow::workflow_yml("release.yml"), &ci_yml());
    assert!(
        problems.is_empty(),
        ".github/workflows/release.yml can release without the T6 gate:\n{}",
        problems.join("\n")
    );
}

/// Each release-gating assertion bites on a mutated copy of the real files.
#[test]
fn t6_release_check_rejects_ungated_jobs() {
    let release = workflow::workflow_yml("release.yml");
    let ci = ci_yml();
    let on_release = |from: &str, to: &str| {
        let changed = release.replacen(from, to, 1);
        assert_ne!(changed, release, "mutation {from:?} did not apply");
        release_problems(&changed, &ci)
    };
    assert_flags(
        &release_problems(&release, &ci.replacen("  workflow_call:\n", "", 1)),
        "no `workflow_call` trigger",
    );
    // build-cli loses `ci`: it, and publish through it, are ungated.
    let p = on_release("    needs: [plan, ci]\n", "    needs: [plan]\n");
    assert_flags(&p, "job `build-cli` does not need `ci`");
    assert_flags(&p, "job `publish` does not need `ci`");
    // build-appimage loses build-cli and so the gate (publish keeps it
    // through build-cli).
    assert_flags(
        &on_release("    needs: [plan, build-cli]\n", "    needs: [plan]\n"),
        "job `build-appimage` does not need `ci`",
    );
    // So does build-deb (US-RR-38-7).
    assert_flags(
        &on_release(
            "    needs: [plan, build-cli]\n    runs-on: ubuntu-22.04\n    timeout-minutes: 20\n",
            "    needs: [plan]\n    runs-on: ubuntu-22.04\n    timeout-minutes: 20\n",
        ),
        "job `build-deb` does not need `ci`",
    );
    assert_flags(
        &on_release(
            "uses: ./.github/workflows/ci.yml",
            "uses: ./.github/workflows/fuzz.yml",
        ),
        "does not `uses: ./.github/workflows/ci.yml`",
    );
    assert_flags(&on_release("\n  ci:\n", "\n  gate:\n"), "has no `ci` job");
    assert_flags(
        &on_release(
            "    needs: [plan, build-cli, build-appimage, test-appimage, build-deb, test-deb]\n",
            "    needs: [plan, build-cli, build-appimage, test-appimage, build-deb, test-deb]\n    if: always()\n",
        ),
        "job `publish` runs past a failed gate",
    );
    assert_flags(
        &on_release(
            "    needs: [plan, build-cli, build-appimage, test-appimage, build-deb, test-deb]\n",
            "    needs: [plan, build-cli, build-appimage, test-appimage, build-deb, test-deb]\n    continue-on-error: true\n",
        ),
        "job `publish` uses `continue-on-error",
    );
    // The content check removed from `publish` (US-RR-38-8).
    assert_flags(
        &on_release(
            "        run: cargo run --locked --package xtask -- check-release-artifacts assets\n",
            "        run: ls assets\n",
        ),
        "step `Publish GitHub Release` runs `gh release` before (or without) the release artifact content check",
    );
    // A new artifact job (.deb, ...) added without the gate.
    assert_flags(
        &on_release(
            "\n  publish:\n",
            "\n  deb:\n    needs: [plan]\n    runs-on: ubuntu-22.04\n\n  publish:\n",
        ),
        "job `deb` does not need `ci`",
    );
}
