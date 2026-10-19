//! `coverage.yml` wiring for story US-RR-4 (task US-RR-4-5): line coverage of
//! rr-core, rr-rom and rr-comms is measured with `cargo llvm-cov nextest` on
//! every pull request and every push to master / rust-rewrite, and the
//! workflow fails if a crate is below 90% or below its coverage on the base.
//!
//! [`coverage_problems`] is a pure check over the text of
//! `.github/workflows/coverage.yml`; one test runs it on the committed file
//! and the `*_rejects_*` test proves each assertion bites on a mutated copy.
//! It asserts that:
//!
//! * the workflow runs on `push` (branches include `master` and
//!   `rust-rewrite`) and `pull_request`, neither path-filtered;
//! * job `coverage` installs cargo-llvm-cov and llvm-tools, runs
//!   `cargo llvm-cov nextest --workspace --locked --lcov --output-path
//!   lcov.info` and uploads that file as artifact `lcov`, failing if it is
//!   missing;
//! * job `base-coverage` checks out the full history, then the base (the
//!   pull request's base commit on `pull_request`), before running the same
//!   command, and uploads artifact `lcov-base`;
//! * job `gate` (check name `coverage-gate`) needs both, downloads both
//!   artifacts and runs `cargo xtask coverage-gate` on them with `--min 90
//!   --max-drop 0.25 --crates rr-core,rr-rom,rr-comms` (0.25 points of
//!   measurement noise, pinned so it cannot quietly grow);
//! * no step of the three jobs can be skipped or softened (`if:`,
//!   `continue-on-error`, `|| true`) and none has a job-level `if:`.
//!
//! The gate's rules themselves are tested in `coverage_gate.rs`. Whether the
//! jobs are green can only be seen on GitHub.

#[path = "common/workflow.rs"]
mod workflow;

use workflow::{
    Job, Workflow, assert_flags, indent, is_blank_or_comment, normalize, skippable_problems,
    trigger_problems, workflow_yml,
};

const FILE: &str = "coverage.yml";
const HEAD_JOB: &str = "coverage";
const BASE_JOB: &str = "base-coverage";
const GATE_JOB: &str = "gate";
const GATE_NAME: &str = "coverage-gate";

const LLVM_COV: &str = "cargo llvm-cov nextest --workspace --locked --lcov --output-path lcov.info";
const LCOV_FILE: &str = "lcov.info";
const HEAD_ARTIFACT: &str = "lcov";
const BASE_ARTIFACT: &str = "lcov-base";
const GATE_CMD: &str = "cargo xtask coverage-gate";
const MIN: &str = "90";
/// The noise tolerance (xtask/src/coverage_gate.rs, "Regression"); raising
/// it weakens the no-regression rule, so it is pinned here.
const MAX_DROP: &str = "0.25";
const CRATES: &str = "rr-core,rr-rom,rr-comms";
const PUSH_BRANCHES: &[&str] = &["master", "rust-rewrite"];

/// The lines under trigger `trigger` in the `on:` block.
fn trigger_body<'a>(wf: &Workflow<'a>, trigger: &str) -> Vec<&'a str> {
    let on = wf.on_block();
    let Some(start) = on
        .iter()
        .position(|l| indent(l) == 2 && l.trim() == format!("{trigger}:"))
    else {
        return Vec::new();
    };
    on[start + 1..]
        .iter()
        .copied()
        .take_while(|l| is_blank_or_comment(l) || indent(l) > 2)
        .collect()
}

/// The `branches:` of trigger `trigger` (flow or block list).
fn trigger_branches(wf: &Workflow<'_>, trigger: &str) -> Vec<String> {
    let body = trigger_body(wf, trigger);
    let Some(pos) = body
        .iter()
        .position(|l| indent(l) == 4 && l.trim_start().starts_with("branches:"))
    else {
        return Vec::new();
    };
    let first = body[pos].trim_start()["branches:".len()..].trim();
    let items: Vec<&str> = if let Some(flow) = first.strip_prefix('[') {
        flow.split(']')
            .next()
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .collect()
    } else {
        body[pos + 1..]
            .iter()
            .take_while(|l| is_blank_or_comment(l) || indent(l) > 4)
            .filter_map(|l| l.trim().strip_prefix("- "))
            .collect()
    };
    items
        .into_iter()
        .map(|s| s.trim().trim_matches(|c| c == '"' || c == '\'').to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

/// The `uses:` of a step, if any.
fn uses(step: &Job<'_>) -> Option<String> {
    step.value_at("uses", 8)
}

/// The `with:` value `key` of a step.
fn with(step: &Job<'_>, key: &str) -> Option<String> {
    step.value_at(key, 10)
}

/// Index of the first step running a script that contains `needle`.
fn step_running(steps: &[Job<'_>], needle: &str) -> Option<usize> {
    steps
        .iter()
        .position(|s| s.run_scripts().iter().any(|r| r.contains(needle)))
}

/// Problems if `job` is missing, has a job-level `if:` or a skippable step.
fn job_or_problem<'w, 'a>(
    wf: &'w Workflow<'a>,
    id: &str,
    problems: &mut Vec<String>,
) -> Option<&'w Job<'a>> {
    let Some(job) = wf.job(id) else {
        problems.push(format!("no `{id}` job"));
        return None;
    };
    if job
        .lines
        .iter()
        .any(|l| indent(l) == 4 && l.trim_start().starts_with("if:"))
    {
        problems.push(format!("`{id}` has a job-level `if:`"));
    }
    problems.extend(skippable_problems(job, &[]));
    Some(job)
}

/// Problems with a measuring job: tools installed, the llvm-cov command run,
/// its report uploaded as `artifact`. Returns the llvm-cov step index.
fn measure_problems(job: &Job<'_>, artifact: &str, problems: &mut Vec<String>) -> Option<usize> {
    let id = job.id;
    let steps = job.steps();
    let installs_tool = steps.iter().any(|s| {
        uses(s).is_some_and(|u| u.starts_with("taiki-e/install-action@"))
            && with(s, "tool").is_some_and(|t| t.split(',').any(|t| t.trim() == "cargo-llvm-cov"))
    });
    if !installs_tool {
        problems.push(format!("`{id}` does not install cargo-llvm-cov"));
    }
    if step_running(&steps, "rustup component add llvm-tools-preview").is_none() {
        problems.push(format!("`{id}` does not install llvm-tools-preview"));
    }
    let run = steps
        .iter()
        .position(|s| s.run_scripts().iter().any(|r| r == LLVM_COV));
    if run.is_none() {
        problems.push(format!(
            "`{id}` does not run `{LLVM_COV}` (scripts: {:?})",
            job.run_scripts()
        ));
    }
    let upload = steps.iter().position(|s| {
        uses(s).is_some_and(|u| u.starts_with("actions/upload-artifact@"))
            && with(s, "name").as_deref() == Some(artifact)
            && with(s, "path").as_deref() == Some(LCOV_FILE)
            && with(s, "if-no-files-found").as_deref() == Some("error")
    });
    match (run, upload) {
        (_, None) => problems.push(format!(
            "`{id}` does not upload `{LCOV_FILE}` as artifact `{artifact}` with `if-no-files-found: error`"
        )),
        (Some(r), Some(u)) if u < r => problems.push(format!(
            "`{id}` uploads the lcov report before producing it"
        )),
        _ => {}
    }
    run
}

/// The value after `flag` in a whitespace-normalized command line.
fn arg<'a>(script: &'a str, flag: &str) -> Option<&'a str> {
    let mut words = script.split(' ');
    words.find(|w| *w == flag)?;
    words.next()
}

/// Every way `src` fails the US-RR-4-5 wiring; empty when it holds.
fn coverage_problems(src: &str) -> Vec<String> {
    let wf = Workflow::parse(src);
    let mut problems = trigger_problems(&wf);
    let push = trigger_branches(&wf, "push");
    for b in PUSH_BRANCHES {
        if !push.iter().any(|p| p == b || p == "**") {
            problems.push(format!("`push` does not run on `{b}` (branches: {push:?})"));
        }
    }
    for trigger in ["push", "pull_request"] {
        let body = trigger_body(&wf, trigger);
        if body
            .iter()
            .any(|l| indent(l) == 4 && l.trim_start().starts_with("paths"))
        {
            problems.push(format!("`{trigger}` is path-filtered"));
        }
    }
    let pr = trigger_branches(&wf, "pull_request");
    if !pr.is_empty() && !pr.iter().any(|b| b == "**") {
        problems.push(format!(
            "`pull_request` is limited to branches {pr:?}, not every base"
        ));
    }

    if let Some(job) = job_or_problem(&wf, HEAD_JOB, &mut problems) {
        measure_problems(job, HEAD_ARTIFACT, &mut problems);
    }

    if let Some(job) = job_or_problem(&wf, BASE_JOB, &mut problems) {
        let run = measure_problems(job, BASE_ARTIFACT, &mut problems);
        let steps = job.steps();
        let full = steps.iter().any(|s| {
            uses(s).is_some_and(|u| u.starts_with("actions/checkout@"))
                && with(s, "fetch-depth").as_deref() == Some("0")
        });
        if !full {
            problems.push(format!(
                "`{BASE_JOB}` does not check out the full history (`fetch-depth: 0`)"
            ));
        }
        match step_running(&steps, "git checkout --quiet --detach \"$base\"") {
            None => problems.push(format!("`{BASE_JOB}` does not check out the base")),
            Some(i) => {
                let step = &steps[i];
                let script = step.run_scripts().join("\n");
                let pr_base = step.content_lines().any(|l| {
                    normalize(l) == "PR_BASE_SHA: ${{ github.event.pull_request.base.sha }}"
                });
                if !pr_base || !script.contains("base=\"$PR_BASE_SHA\"") {
                    problems.push(format!(
                        "`{BASE_JOB}` does not measure the pull request's base commit on `pull_request`"
                    ));
                }
                if run.is_some_and(|r| r < i) {
                    problems.push(format!(
                        "`{BASE_JOB}` measures coverage before checking out the base"
                    ));
                }
            }
        }
    }

    if let Some(job) = job_or_problem(&wf, GATE_JOB, &mut problems) {
        if job.value_at("name", 4).as_deref() != Some(GATE_NAME) {
            problems.push(format!("`{GATE_JOB}` is not named `{GATE_NAME}`"));
        }
        let needs = job.needs();
        for n in [HEAD_JOB, BASE_JOB] {
            if !needs.iter().any(|x| x == n) {
                problems.push(format!("`{GATE_JOB}.needs` does not include `{n}`"));
            }
        }
        let steps = job.steps();
        let download = |artifact: &str| {
            steps.iter().find_map(|s| {
                (uses(s).is_some_and(|u| u.starts_with("actions/download-artifact@"))
                    && with(s, "name").as_deref() == Some(artifact))
                .then(|| with(s, "path").unwrap_or_default())
            })
        };
        let scripts = job.run_scripts();
        let gate = scripts.iter().find(|s| s.starts_with(GATE_CMD));
        match gate {
            None => problems.push(format!("`{GATE_JOB}` does not run `{GATE_CMD}`")),
            Some(cmd) => {
                for (flag, artifact) in [("--lcov", HEAD_ARTIFACT), ("--base-lcov", BASE_ARTIFACT)]
                {
                    let want = download(artifact).map(|p| format!("{p}/{LCOV_FILE}"));
                    if want.is_none() {
                        problems.push(format!(
                            "`{GATE_JOB}` does not download artifact `{artifact}`"
                        ));
                    } else if arg(cmd, flag) != want.as_deref() {
                        problems.push(format!(
                            "`{GATE_CMD}` `{flag}` is not the downloaded `{artifact}` report ({want:?}): {cmd}"
                        ));
                    }
                }
                if arg(cmd, "--min") != Some(MIN) {
                    problems.push(format!("`{GATE_CMD}` does not pass `--min {MIN}`: {cmd}"));
                }
                if arg(cmd, "--max-drop") != Some(MAX_DROP) {
                    problems.push(format!(
                        "`{GATE_CMD}` does not pass `--max-drop {MAX_DROP}`: {cmd}"
                    ));
                }
                if arg(cmd, "--crates") != Some(CRATES) {
                    problems.push(format!(
                        "`{GATE_CMD}` does not pass `--crates {CRATES}`: {cmd}"
                    ));
                }
            }
        }
    }
    problems
}

#[test]
fn coverage_workflow_measures_and_gates_the_safety_crates() {
    let problems = coverage_problems(&workflow_yml(FILE));
    assert!(
        problems.is_empty(),
        ".github/workflows/{FILE} violates US-RR-4-5:\n{}",
        problems.join("\n")
    );
}

#[test]
fn coverage_workflow_check_rejects_weakened_workflow() {
    let src = workflow_yml(FILE);
    let m = |edit: &dyn Fn(&str) -> String| {
        let changed = edit(&src);
        assert_ne!(changed, src, "mutation did not apply");
        coverage_problems(&changed)
    };
    // Triggers.
    assert_flags(
        &m(&|s| s.replacen("  pull_request:\n", "  pull_request_target:\n", 1)),
        "does not run on `pull_request`",
    );
    assert_flags(
        &m(&|s| s.replacen("[master, rust-rewrite]", "[master]", 1)),
        "`push` does not run on `rust-rewrite`",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "  pull_request:\n    branches: [\"**\"]\n",
                "  pull_request:\n    branches: [\"**\"]\n    paths: [\"crates/**\"]\n",
                1,
            )
        }),
        "`pull_request` is path-filtered",
    );
    assert_flags(
        &m(&|s| s.replacen("    branches: [\"**\"]\n", "    branches: [master]\n", 1)),
        "not every base",
    );
    // Measuring.
    assert_flags(
        &m(&|s| {
            s.replacen(
                LLVM_COV,
                "cargo llvm-cov nextest -p rr-core --lcov --output-path lcov.info",
                1,
            )
        }),
        "`coverage` does not run",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "          tool: cargo-nextest,cargo-llvm-cov\n",
                "          tool: cargo-nextest\n",
                1,
            )
        }),
        "`coverage` does not install cargo-llvm-cov",
    );
    assert_flags(
        &m(&|s| s.replacen("          rustup component add llvm-tools-preview\n", "", 1)),
        "`coverage` does not install llvm-tools-preview",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "          if-no-files-found: error\n",
                "          if-no-files-found: warn\n",
                1,
            )
        }),
        "`coverage` does not upload `lcov.info` as artifact `lcov`",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "          path: lcov.info\n",
                "          path: target/lcov.info\n",
                1,
            )
        }),
        "`coverage` does not upload",
    );
    // Base.
    assert_flags(
        &m(&|s| s.replacen("          name: lcov-base\n", "          name: lcov\n", 1)),
        "`base-coverage` does not upload `lcov.info` as artifact `lcov-base`",
    );
    assert_flags(
        &m(&|s| s.replace("          fetch-depth: 0\n", "          fetch-depth: 1\n")),
        "full history",
    );
    assert_flags(
        &m(&|s| s.replace("          git checkout --quiet --detach \"$base\"\n", "")),
        "`base-coverage` does not check out the base",
    );
    assert_flags(
        &m(&|s| s.replace("base=\"$PR_BASE_SHA\"", "base=\"$BEFORE\"")),
        "base commit on `pull_request`",
    );
    assert_flags(
        &m(&|s| {
            // Move the base checkout after the measurement.
            let from = s.find("      - name: Check out the base\n").unwrap_or(0);
            let to = s[from..]
                .find("      # After the base checkout")
                .map_or(from, |i| from + i);
            let step = &s[from..to];
            let rest = format!("{}{}", &s[..from], &s[to..]);
            let upload = rest[from..]
                .find("      - name: Upload lcov report\n")
                .map_or(from, |i| from + i);
            format!("{}{step}{}", &rest[..upload], &rest[upload..])
        }),
        "before checking out the base",
    );
    // Gate.
    assert_flags(
        &m(&|s| {
            s.replacen(
                "    needs: [coverage, base-coverage]\n",
                "    needs: [coverage]\n",
                1,
            )
        }),
        "`gate.needs` does not include `base-coverage`",
    );
    assert_flags(
        &m(&|s| s.replacen("--min 90", "--min 80", 1)),
        "does not pass `--min 90`",
    );
    assert_flags(
        &m(&|s| s.replacen("--min 90 --max-drop 0.25", "--min 90 --max-drop 5", 1)),
        "does not pass `--max-drop 0.25`",
    );
    assert_flags(
        &m(&|s| s.replacen("--min 90 --max-drop 0.25", "--min 90", 1)),
        "does not pass `--max-drop 0.25`",
    );
    assert_flags(
        &m(&|s| s.replacen("--crates rr-core,rr-rom,rr-comms", "--crates rr-core", 1)),
        "does not pass `--crates rr-core,rr-rom,rr-comms`",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "          --base-lcov coverage/base/lcov.info ",
                "          ",
                1,
            )
        }),
        "`--base-lcov` is not the downloaded `lcov-base` report",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "--lcov coverage/head/lcov.info",
                "--lcov coverage/base/lcov.info",
                1,
            )
        }),
        "`--lcov` is not the downloaded `lcov` report",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "          name: lcov\n          path: coverage/head\n",
                "          name: lcov\n",
                1,
            )
        }),
        "`--lcov` is not the downloaded",
    );
    assert_flags(
        &m(&|s| s.replacen("    name: coverage-gate\n", "    name: coverage\n", 1)),
        "not named `coverage-gate`",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "          cargo xtask coverage-gate --lcov",
                "          echo cargo xtask coverage-gate --lcov",
                1,
            )
        }),
        "`gate` does not run `cargo xtask coverage-gate`",
    );
    // Skippable.
    assert_flags(
        &m(&|s| {
            s.replacen(
                "      - name: cargo xtask coverage-gate\n",
                "      - name: cargo xtask coverage-gate\n        if: github.event_name == 'pull_request'\n",
                1,
            )
        }),
        "has a conditional",
    );
    assert_flags(
        &m(&|s| s.replacen(LLVM_COV, &format!("{LLVM_COV} || true"), 1)),
        "swallows failures",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "    needs: [coverage, base-coverage]\n",
                "    needs: [coverage, base-coverage]\n    if: github.event_name == 'push'\n",
                1,
            )
        }),
        "`gate` has a job-level `if:`",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "    name: coverage (base)\n",
                "    name: coverage (base)\n    continue-on-error: true\n",
                1,
            )
        }),
        "`base-coverage` uses `continue-on-error",
    );
}
