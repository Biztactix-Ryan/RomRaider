//! `fixtures.yml` wiring for story US-RR-9 (task US-RR-9-7): the KAT fixture
//! validator runs in CI on every fixture, on push and pull request touching
//! the fixtures or rr-testkit, and checks changed expected values against the
//! base revision.
//!
//! [`fixtures_problems`] is a pure check over the text of
//! `.github/workflows/fixtures.yml`; one test runs it on the committed file
//! and the `*_rejects_*` test proves each assertion bites on a mutated copy.
//! It asserts that:
//!
//! * the workflow runs on `push` and `pull_request` with no trigger-level
//!   `paths:` filter, so the required check `fixtures-ok` reports on every
//!   change (task US-RR-4-8); job `scope` (`common/scoped_gate.rs`) decides
//!   whether the event is in scope, and its `paths` list includes
//!   `fixtures/**`, `crates/rr-testkit/**`, `xtask/**` and the workflow file
//!   itself; `validate` and `synthetic` need it and run only `if:
//!   needs.scope.outputs.in_scope == 'true'`;
//! * job `fixtures-ok` (the required check) runs `if: always()`, needs every
//!   other job, and fails when `scope` did not succeed, a fixture job failed
//!   or was cancelled, or one was skipped although the event is in scope; it
//!   passes out of scope. On Unix the test runs its script for every result
//!   combination, and the scope script on scratch repositories (pull
//!   request, fast-forward / new-branch / force push, workflow_dispatch);
//! * job `validate` checks out the full history (`fetch-depth: 0`, so the
//!   merge-base that `--base` compares with is present), runs `cargo xtask
//!   validate-fixtures` with no paths (the validator's defaults: every
//!   fixture), and runs `cargo xtask validate-fixtures --base "$base"` where
//!   `$base` is the pull request's base commit on `pull_request`, and
//!   `cargo xtask validate-fixtures --push-before="$BEFORE"` (the push's
//!   `github.event.before`) otherwise, so a push, including a new branch's
//!   first push or a force push, is checked too (US-RR-9-4; how the push
//!   base is picked is tested in rr-testkit's `kat_validate.rs`);
//! * no step of the job can be skipped or softened (`if:`,
//!   `continue-on-error`, `|| true`) and the job's only job-level `if:` is
//!   the scope's.
//!
//! [`synthetic_problems`] checks the US-RR-10-7 job `synthetic`: it runs on
//! a matrix of both an `ubuntu-*` and a `windows-*` runner (`runs-on: ${{
//! matrix.os }}`, no `exclude:`, `fail-fast: false`), runs `cargo xtask
//! gen-synthetic --check` (regenerate, fail on any byte difference from
//! `fixtures/public/synthetic`) and `sha256sum -c SHA256SUMS` in that
//! directory, and can be neither skipped (except out of scope) nor
//! softened.
//!
//! Whether the jobs are green, and so whether the output really is identical
//! on Windows, can only be seen on GitHub.

#[path = "common/workflow.rs"]
mod workflow;

#[path = "common/scoped_gate.rs"]
mod scoped_gate;

use scoped_gate::{Gated, IN_SCOPE_IF};
use workflow::{Workflow, assert_flags, skippable_problems, trigger_problems, workflow_yml};

const FILE: &str = "fixtures.yml";
const JOB: &str = "validate";

/// The aggregate job: the required status check (CONTRIBUTING.md "Branch
/// protection").
const AGGREGATE: &str = "fixtures-ok";

/// The fixture jobs `fixtures-ok` checks; both run on every in-scope event.
const GATED: &[Gated] = &[
    Gated {
        job: JOB,
        var: "VALIDATE",
        runs_on: |_| true,
    },
    Gated {
        job: SYNTHETIC_JOB,
        var: "SYNTHETIC",
        runs_on: |_| true,
    },
];

/// The scope step's env: the event, the pull request's base and the commit
/// before a push.
const SCOPE_ENV: &[(&str, &str)] = &[
    ("EVENT", "${{ github.event_name }}"),
    ("BASE", "${{ github.event.pull_request.base.sha }}"),
    ("BEFORE", "${{ github.event.before }}"),
];

/// Paths the scope's `paths` list must include (at least).
const REQUIRED_PATHS: &[&str] = &[
    "fixtures/**",
    "crates/rr-testkit/**",
    "xtask/**",
    ".github/workflows/fixtures.yml",
];

const VALIDATE_CMD: &str = "cargo xtask validate-fixtures";
const BASE_CMD: &str = "cargo xtask validate-fixtures --base \"$base\"";
const PUSH_CMD: &str = "cargo xtask validate-fixtures --push-before=\"$BEFORE\"";

/// Every way `src` fails the US-RR-9-7 wiring; empty when it holds.
fn fixtures_problems(src: &str) -> Vec<String> {
    let wf = Workflow::parse(src);
    let mut problems = trigger_problems(&wf);
    for trigger in ["push", "pull_request"] {
        problems.extend(scoped_gate::trigger_filter_problems(&wf, trigger));
    }
    problems.extend(scoped_gate::scope_job_problems(&wf, SCOPE_ENV));
    problems.extend(scoped_gate::aggregate_problems(&wf, AGGREGATE, GATED));
    let paths = scoped_gate::scope_paths(&wf);
    for required in REQUIRED_PATHS {
        if !paths.iter().any(|p| p == required) {
            problems.push(format!(
                "scope paths do not include `{required}` (paths: {paths:?})"
            ));
        }
    }
    let Some(job) = wf.job(JOB) else {
        problems.push(format!("no `{JOB}` job"));
        return problems;
    };
    problems.extend(scoped_gate::gated_job_problems(&wf, JOB, IN_SCOPE_IF));
    let steps = job.steps();
    let checkout_full = steps.iter().any(|s| {
        s.id.starts_with("actions/checkout@")
            && s.content_lines()
                .any(|l| workflow::normalize(l) == "fetch-depth: 0")
    });
    if !checkout_full {
        problems.push(format!(
            "`{JOB}` does not check out the full history (`actions/checkout` with `fetch-depth: 0`)"
        ));
    }
    let scripts = job.run_scripts();
    if !scripts.iter().any(|s| s == VALIDATE_CMD) {
        problems.push(format!(
            "`{JOB}` does not run `{VALIDATE_CMD}` on every fixture (scripts: {scripts:?})"
        ));
    }
    let base_step = steps
        .iter()
        .find(|s| s.run_scripts().iter().any(|r| r.contains(BASE_CMD)));
    match base_step {
        None => problems.push(format!("`{JOB}` does not run `{BASE_CMD}`")),
        Some(step) => {
            let script = step.run_scripts().join("\n");
            let pr_base = step.content_lines().any(|l| {
                workflow::normalize(l) == "PR_BASE_SHA: ${{ github.event.pull_request.base.sha }}"
            });
            if !pr_base || !script.contains("base=\"$PR_BASE_SHA\"") {
                problems.push(format!(
                    "`{JOB}` does not use the pull request's base commit as `--base` on `pull_request`"
                ));
            }
            let before = step
                .content_lines()
                .any(|l| workflow::normalize(l) == "BEFORE: ${{ github.event.before }}");
            // The push command must be the `else` of the pull-request test, so
            // every event that is not a pull request runs it (`script` is
            // whitespace-normalized: one line).
            let push_in_else = script
                .split_once("if [ \"$EVENT\" = pull_request ]; then ")
                .and_then(|(_, rest)| rest.split_once(" else "))
                .and_then(|(_, rest)| rest.rsplit_once(" fi"))
                .is_some_and(|(otherwise, _)| otherwise.contains(PUSH_CMD));
            if !before || !push_in_else {
                problems.push(format!(
                    "`{JOB}` does not run `{PUSH_CMD}` (with `BEFORE: ${{{{ github.event.before }}}}`) \
                     on events other than `pull_request`"
                ));
            }
        }
    }
    problems.extend(skippable_problems(job, &[]));
    problems
}

const SYNTHETIC_JOB: &str = "synthetic";
const GEN_CHECK_CMD: &str = "cargo xtask gen-synthetic --check";
const SUMS_CMD: &str = "sha256sum -c SHA256SUMS";
const SYNTHETIC_DIR: &str = "fixtures/public/synthetic";

/// The items of the `os:` flow list in `job`'s `strategy.matrix`.
fn matrix_os(job: &workflow::Job<'_>) -> Vec<String> {
    job.content_lines()
        .find_map(|l| l.strip_prefix("os:"))
        .map(str::trim)
        .and_then(|v| v.strip_prefix('[')?.strip_suffix(']'))
        .map(|items| {
            items
                .split(',')
                .map(|s| s.trim().trim_matches(|c| c == '"' || c == '\'').to_owned())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Every way `src` fails the US-RR-10-7 wiring; empty when it holds.
fn synthetic_problems(src: &str) -> Vec<String> {
    let wf = Workflow::parse(src);
    let mut problems = trigger_problems(&wf);
    let Some(job) = wf.job(SYNTHETIC_JOB) else {
        problems.push(format!("no `{SYNTHETIC_JOB}` job"));
        return problems;
    };
    let id = SYNTHETIC_JOB;
    problems.extend(scoped_gate::gated_job_problems(&wf, id, IN_SCOPE_IF));
    if job.value_at("runs-on", 4).as_deref() != Some("${{ matrix.os }}") {
        problems.push(format!("`{id}` does not run on `${{{{ matrix.os }}}}`"));
    }
    let os = matrix_os(job);
    for (family, prefix) in [("Linux", "ubuntu-"), ("Windows", "windows-")] {
        if !os.iter().any(|o| o.starts_with(prefix)) {
            problems.push(format!(
                "`{id}` matrix has no {family} (`{prefix}*`) runner (os: {os:?})"
            ));
        }
    }
    if job.content_lines().any(|l| l.starts_with("exclude:")) {
        problems.push(format!("`{id}` matrix has an `exclude:` list"));
    }
    if !job
        .content_lines()
        .any(|l| workflow::normalize(l) == "fail-fast: false")
    {
        problems.push(format!(
            "`{id}` does not set `fail-fast: false` (a failure on one OS would cancel the other)"
        ));
    }
    let scripts = job.run_scripts();
    if !scripts.iter().any(|s| s == GEN_CHECK_CMD) {
        problems.push(format!(
            "`{id}` does not run `{GEN_CHECK_CMD}` (scripts: {scripts:?})"
        ));
    }
    let sums_in_dir = job.steps().iter().any(|s| {
        s.run_scripts().iter().any(|r| r == SUMS_CMD)
            && s.value_at("working-directory", 8).as_deref() == Some(SYNTHETIC_DIR)
    });
    if !sums_in_dir {
        problems.push(format!(
            "`{id}` does not run `{SUMS_CMD}` in `{SYNTHETIC_DIR}`"
        ));
    }
    problems.extend(skippable_problems(job, &[]));
    problems
}

#[test]
fn fixtures_workflow_checks_synthetic_roms_on_linux_and_windows() {
    let problems = synthetic_problems(&workflow_yml(FILE));
    assert!(
        problems.is_empty(),
        ".github/workflows/{FILE} violates US-RR-10-7:\n{}",
        problems.join("\n")
    );
}

#[test]
fn synthetic_check_rejects_weakened_workflow() {
    let src = workflow_yml(FILE);
    let m = |edit: &dyn Fn(&str) -> String| {
        let changed = edit(&src);
        assert_ne!(changed, src, "mutation did not apply");
        synthetic_problems(&changed)
    };
    assert_flags(
        &m(&|s| s.replace("  synthetic:\n", "  synthetic-roms:\n")),
        "no `synthetic` job",
    );
    assert_flags(
        &m(&|s| s.replace("os: [ubuntu-24.04, windows-2022]", "os: [ubuntu-24.04]")),
        "no Windows",
    );
    assert_flags(
        &m(&|s| s.replace("os: [ubuntu-24.04, windows-2022]", "os: [windows-2022]")),
        "no Linux",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "        os: [ubuntu-24.04, windows-2022]\n",
                "        os: [ubuntu-24.04, windows-2022]\n        exclude:\n          - os: windows-2022\n",
            )
        }),
        "`exclude:` list",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "    runs-on: ${{ matrix.os }}\n    timeout-minutes: 20\n    strategy:",
                "    runs-on: ubuntu-24.04\n    timeout-minutes: 20\n    strategy:",
            )
        }),
        "does not run on",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      fail-fast: false\n      matrix:\n        os:",
                "      matrix:\n        os:",
            )
        }),
        "fail-fast: false",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "run: cargo xtask gen-synthetic --check\n",
                "run: cargo xtask gen-synthetic\n",
            )
        }),
        GEN_CHECK_CMD,
    );
    assert_flags(
        &m(&|s| s.replace("run: sha256sum -c SHA256SUMS\n", "run: cat SHA256SUMS\n")),
        SUMS_CMD,
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "working-directory: fixtures/public/synthetic\n",
                "working-directory: fixtures\n",
            )
        }),
        SUMS_CMD,
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: cargo xtask gen-synthetic --check\n",
                "      - name: cargo xtask gen-synthetic --check\n        if: runner.os == 'Linux'\n",
            )
        }),
        "has a conditional",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "run: cargo xtask gen-synthetic --check\n",
                "run: cargo xtask gen-synthetic --check || true\n",
            )
        }),
        "swallows failures",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "    if: needs.scope.outputs.in_scope == 'true'\n    runs-on: ${{ matrix.os }}\n",
                "    if: github.event_name == 'push'\n    runs-on: ${{ matrix.os }}\n",
            )
        }),
        "`synthetic` job-level `if:`",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "    needs: scope\n    if: needs.scope.outputs.in_scope == 'true'\n    runs-on: ${{ matrix.os }}\n",
                "    runs-on: ${{ matrix.os }}\n",
            )
        }),
        "`synthetic.needs` does not include `scope`",
    );
}

#[test]
fn fixtures_workflow_runs_validator_on_every_fixture_and_against_pr_base() {
    let problems = fixtures_problems(&workflow_yml(FILE));
    assert!(
        problems.is_empty(),
        ".github/workflows/{FILE} violates US-RR-9-7:\n{}",
        problems.join("\n")
    );
}

#[test]
fn fixtures_workflow_check_rejects_weakened_workflow() {
    let src = workflow_yml(FILE);
    let m = |edit: &dyn Fn(&str) -> String| {
        let changed = edit(&src);
        assert_ne!(changed, src, "mutation did not apply");
        fixtures_problems(&changed)
    };
    assert_flags(
        &m(&|s| s.replacen("  pull_request:\n", "  pull_request_target:\n", 1)),
        "does not run on `pull_request`",
    );
    assert_flags(
        &m(&|s| s.replacen("            \"fixtures/**\"\n", "", 1)),
        "scope paths do not include `fixtures/**`",
    );
    assert_flags(
        &m(&|s| s.replacen("            \"crates/rr-testkit/**\"\n", "", 1)),
        "scope paths do not include `crates/rr-testkit/**`",
    );
    assert_flags(
        &m(&|s| s.replacen("            \"xtask/**\"\n", "", 1)),
        "scope paths do not include `xtask/**`",
    );
    // A trigger-level path filter: the check never reports elsewhere.
    assert_flags(
        &m(&|s| {
            s.replacen(
                "  push:\n    branches: [\"**\"]\n",
                "  push:\n    branches: [\"**\"]\n    paths:\n      - \"fixtures/**\"\n",
                1,
            )
        }),
        "`push` has a trigger-level `paths:` filter",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "  pull_request:\n    branches: [\"**\"]\n",
                "  pull_request:\n    branches: [\"**\"]\n    paths:\n      - \"fixtures/**\"\n",
                1,
            )
        }),
        "`pull_request` has a trigger-level `paths:` filter",
    );
    assert_flags(
        &m(&|s| s.replace("          fetch-depth: 0\n", "          fetch-depth: 1\n")),
        "`validate` does not check out the full history",
    );
    assert_flags(
        &m(&|s| s.replace("          fetch-depth: 0\n", "          fetch-depth: 1\n")),
        "`scope` does not check out the full history",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "run: cargo xtask validate-fixtures\n",
                "run: cargo xtask validate-fixtures fixtures/kat\n",
            )
        }),
        "on every fixture",
    );
    assert_flags(
        &m(&|s| s.replace(BASE_CMD, "cargo xtask validate-fixtures")),
        "--base",
    );
    assert_flags(
        &m(&|s| s.replace("base=\"$PR_BASE_SHA\"", "base=\"$BEFORE\"")),
        "base commit as `--base`",
    );
    assert_flags(
        &m(&|s| s.replace(PUSH_CMD, "cargo xtask validate-fixtures")),
        "--push-before",
    );
    assert_flags(
        &m(&|s| s.replace(PUSH_CMD, "echo skipped")),
        "--push-before",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "          else\n            echo \"Comparing fixtures with the push's base",
                "          fi\n          if false; then\n            echo \"Comparing fixtures with the push's base",
            )
        }),
        "--push-before",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "BEFORE: ${{ github.event.before }}",
                "BEFORE: ${{ github.sha }}",
            )
        }),
        "--push-before",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: cargo xtask validate-fixtures\n",
                "      - name: cargo xtask validate-fixtures\n        if: github.event_name == 'push'\n",
            )
        }),
        "has a conditional",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "run: cargo xtask validate-fixtures\n",
                "run: cargo xtask validate-fixtures || true\n",
            )
        }),
        "swallows failures",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "    name: KAT fixtures (validate-fixtures)\n    needs: scope\n    if: needs.scope.outputs.in_scope == 'true'\n",
                "    name: KAT fixtures (validate-fixtures)\n    needs: scope\n    if: github.event_name == 'push'\n",
                1,
            )
        }),
        "`validate` job-level `if:`",
    );
}

/// The scope job and the required `fixtures-ok` check (task US-RR-4-8):
/// each assertion bites on a weakened copy.
#[test]
fn fixtures_workflow_check_rejects_weakened_scope_and_aggregate() {
    let src = workflow_yml(FILE);
    let m = |edit: &dyn Fn(&str) -> String| {
        let changed = edit(&src);
        assert_ne!(changed, src, "mutation did not apply");
        fixtures_problems(&changed)
    };
    assert_flags(
        &m(&|s| {
            s.replacen(
                "    name: fixtures-ok\n    if: always()\n",
                "    name: fixtures-ok\n",
                1,
            )
        }),
        "`fixtures-ok` is not `if: always()`",
    );
    assert_flags(
        &m(&|s| s.replacen("    name: fixtures-ok\n", "    name: fixtures\n", 1)),
        "`fixtures-ok` is not named `fixtures-ok`",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "needs: [scope, validate, synthetic]",
                "needs: [validate, synthetic]",
                1,
            )
        }),
        "`fixtures-ok.needs` does not include `scope`",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "needs: [scope, validate, synthetic]",
                "needs: [scope, validate]",
                1,
            )
        }),
        "`fixtures-ok.needs` does not include `synthetic`",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "          SYNTHETIC: ${{ needs.synthetic.result }}\n",
                "          SYNTHETIC: success\n",
                1,
            )
        }),
        "does not set `SYNTHETIC: ${{ needs.synthetic.result }}`",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "      - name: Fixture job results\n",
                "      - name: Fixture job results\n        continue-on-error: true\n",
                1,
            )
        }),
        "`fixtures-ok` uses `continue-on-error",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "          BEFORE: ${{ github.event.before }}\n        run: |\n          # Git glob",
                "          BEFORE: ${{ github.sha }}\n        run: |\n          # Git glob",
                1,
            )
        }),
        "does not set `BEFORE: ${{ github.event.before }}`",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "    name: fixtures-scope\n",
                "    name: fixtures-scope\n    continue-on-error: true\n",
                1,
            )
        }),
        "`scope` uses `continue-on-error",
    );
}

/// The `fixtures-ok` script decides right for every result combination, and
/// the scope script answers right on scratch repositories (Unix: bash).
#[cfg(unix)]
#[test]
fn fixtures_ok_and_scope_behave_as_documented() {
    let src = workflow_yml(FILE);
    let wf = Workflow::parse(&src);
    let script = scoped_gate::aggregate_script(&wf, AGGREGATE).unwrap_or_default();
    let mut problems = scoped_gate::aggregate_behaviour_problems(&script, AGGREGATE, GATED);
    problems.extend(scoped_gate::scope_behaviour_problems(
        &wf,
        "fixtures",
        &["workflow_dispatch"],
        true,
    ));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The behaviour checks bite on weakened `fixtures-ok` and scope scripts.
#[cfg(unix)]
#[test]
fn fixtures_behaviour_check_rejects_weakened_scripts() {
    let src = workflow_yml(FILE);
    let agg = |edit: &dyn Fn(&str) -> String| {
        let changed = edit(&src);
        assert_ne!(changed, src, "mutation did not apply");
        let wf = Workflow::parse(&changed);
        let script = scoped_gate::aggregate_script(&wf, AGGREGATE).unwrap_or_default();
        scoped_gate::aggregate_behaviour_problems(&script, AGGREGATE, GATED)
    };
    assert_flags(
        &agg(&|s| {
            s.replacen(
                "              success) ;;\n",
                "              success | failure) ;;\n",
                1,
            )
        }),
        "`fixtures-ok` passes when",
    );
    assert_flags(
        &agg(&|s| {
            s.replacen(
                "          check synthetic \"$SYNTHETIC\" \"$IN_SCOPE\"\n",
                "",
                1,
            )
        }),
        "(\"SYNTHETIC\", \"cancelled\")",
    );
    assert_flags(
        &agg(&|s| {
            s.replacen(
                "          if [ \"$SCOPE\" != success ]; then\n",
                "          if false; then\n",
                1,
            )
        }),
        "(\"SCOPE\", \"cancelled\")",
    );
    assert_flags(
        &agg(&|s| {
            s.replacen(
                "                if [ \"$3\" = true ]; then\n",
                "                if false; then\n",
                1,
            )
        }),
        "(\"VALIDATE\", \"skipped\")",
    );

    let scope = |edit: &dyn Fn(&str) -> String| {
        let changed = edit(&src);
        assert_ne!(changed, src, "mutation did not apply");
        scoped_gate::scope_behaviour_problems(
            &Workflow::parse(&changed),
            "fixtures-sabotage",
            &["workflow_dispatch"],
            true,
        )
    };
    // A new branch or force push compared with nothing: out of scope.
    assert_flags(
        &scope(&|s| {
            s.replacen(
                "            echo \"$EVENT (before: ${BEFORE:-none}): no base to compare with, in scope\"\n            in_scope=true\n",
                "            echo \"$EVENT (before: ${BEFORE:-none}): no base to compare with\"\n            in_scope=false\n",
                1,
            )
        }),
        "of a new branch's first push",
    );
    // `before` used even when it is not an ancestor (a force push).
    assert_flags(
        &scope(&|s| {
            s.replacen(
                "              && git merge-base --is-ancestor \"$BEFORE\" HEAD 2>/dev/null; then\n",
                "              && git cat-file -e \"$BEFORE^{commit}\" 2>/dev/null; then\n",
                1,
            )
        }),
        "of a force push",
    );
    // A missing pull request base read as "nothing changed".
    assert_flags(
        &scope(&|s| {
            s.replacen(
                "            if [ -z \"$BASE\" ]; then\n              echo \"::error::no pull request base commit\"\n              exit 1\n            fi\n",
                "",
                1,
            )
        }),
        "with no pull request base",
    );
    // Only the first listed path compared.
    assert_flags(
        &scope(&|s| s.replacen("-- \"${paths[@]}\")", "-- \"${paths[0]}\")", 1)),
        "of a pull request changing `crates/rr-testkit/sample/changed.txt`",
    );
}
