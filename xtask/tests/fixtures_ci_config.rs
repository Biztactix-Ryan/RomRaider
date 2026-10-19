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
//! * the workflow runs on `push` and `pull_request`, and both triggers'
//!   `paths:` include `fixtures/**`, `crates/rr-testkit/**`, `xtask/**` and
//!   the workflow file itself;
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
//!   `continue-on-error`, `|| true`) and the job has no job-level `if:`.
//!
//! [`synthetic_problems`] checks the US-RR-10-7 job `synthetic`: it runs on
//! a matrix of both an `ubuntu-*` and a `windows-*` runner (`runs-on: ${{
//! matrix.os }}`, no `exclude:`, `fail-fast: false`), runs `cargo xtask
//! gen-synthetic --check` (regenerate, fail on any byte difference from
//! `fixtures/public/synthetic`) and `sha256sum -c SHA256SUMS` in that
//! directory, and can be neither skipped nor softened.
//!
//! Whether the jobs are green, and so whether the output really is identical
//! on Windows, can only be seen on GitHub.

#[path = "common/workflow.rs"]
mod workflow;

use workflow::{
    Workflow, assert_flags, indent, is_blank_or_comment, skippable_problems, trigger_problems,
    workflow_yml,
};

const FILE: &str = "fixtures.yml";
const JOB: &str = "validate";

/// Paths each trigger must be filtered to (at least).
const REQUIRED_PATHS: &[&str] = &[
    "fixtures/**",
    "crates/rr-testkit/**",
    "xtask/**",
    ".github/workflows/fixtures.yml",
];

const VALIDATE_CMD: &str = "cargo xtask validate-fixtures";
const BASE_CMD: &str = "cargo xtask validate-fixtures --base \"$base\"";
const PUSH_CMD: &str = "cargo xtask validate-fixtures --push-before=\"$BEFORE\"";

/// The `paths:` items under trigger `trigger` in the `on:` block.
fn trigger_paths(wf: &Workflow<'_>, trigger: &str) -> Vec<String> {
    let on = wf.on_block();
    let Some(start) = on
        .iter()
        .position(|l| indent(l) == 2 && l.trim() == format!("{trigger}:"))
    else {
        return Vec::new();
    };
    let body: Vec<&str> = on[start + 1..]
        .iter()
        .copied()
        .take_while(|l| is_blank_or_comment(l) || indent(l) > 2)
        .collect();
    let Some(paths) = body
        .iter()
        .position(|l| indent(l) == 4 && l.trim() == "paths:")
    else {
        return Vec::new();
    };
    body[paths + 1..]
        .iter()
        .take_while(|l| is_blank_or_comment(l) || indent(l) > 4)
        .filter_map(|l| l.trim().strip_prefix("- "))
        .map(|s| s.trim().trim_matches(|c| c == '"' || c == '\'').to_owned())
        .collect()
}

/// Every way `src` fails the US-RR-9-7 wiring; empty when it holds.
fn fixtures_problems(src: &str) -> Vec<String> {
    let wf = Workflow::parse(src);
    let mut problems = trigger_problems(&wf);
    for trigger in ["push", "pull_request"] {
        let paths = trigger_paths(&wf, trigger);
        for required in REQUIRED_PATHS {
            if !paths.iter().any(|p| p == required) {
                problems.push(format!(
                    "`{trigger}` is not path-filtered to include `{required}` (paths: {paths:?})"
                ));
            }
        }
    }
    let Some(job) = wf.job(JOB) else {
        problems.push(format!("no `{JOB}` job"));
        return problems;
    };
    if job
        .lines
        .iter()
        .any(|l| indent(l) == 4 && l.trim_start().starts_with("if:"))
    {
        problems.push(format!("`{JOB}` has a job-level `if:`"));
    }
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
    if job
        .lines
        .iter()
        .any(|l| indent(l) == 4 && l.trim_start().starts_with("if:"))
    {
        problems.push(format!("`{id}` has a job-level `if:`"));
    }
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
                "    runs-on: ${{ matrix.os }}\n",
                "    if: github.event_name == 'push'\n    runs-on: ${{ matrix.os }}\n",
            )
        }),
        "job-level `if:`",
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
        &m(&|s| s.replacen("      - \"fixtures/**\"\n", "", 1)),
        "`push` is not path-filtered to include `fixtures/**`",
    );
    assert_flags(
        &m(&|s| {
            let at = s.find("  pull_request:\n").unwrap_or(0);
            let (head, tail) = s.split_at(at);
            format!(
                "{head}{}",
                tail.replacen("      - \"crates/rr-testkit/**\"\n", "", 1)
            )
        }),
        "`pull_request` is not path-filtered to include `crates/rr-testkit/**`",
    );
    assert_flags(
        &m(&|s| s.replace("          fetch-depth: 0\n", "          fetch-depth: 1\n")),
        "full history",
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
            s.replace(
                "    runs-on: ubuntu-24.04\n",
                "    if: github.event_name == 'push'\n    runs-on: ubuntu-24.04\n",
            )
        }),
        "job-level `if:`",
    );
}
