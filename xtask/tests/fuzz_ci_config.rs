//! `fuzz.yml` wiring for story US-RR-5 (task US-RR-5-6): every fuzz target
//! is fuzzed nightly for 10 minutes through `cargo xtask fuzz run` (so the
//! per-input limits apply), its corpus is cached and keeps growing, and a
//! failure is uploaded and filed as a GitHub issue labelled `fuzz`.
//!
//! [`fuzz_workflow_problems`] is a pure check over the text of
//! `.github/workflows/fuzz.yml`; one test runs it on the committed file and
//! the `*_rejects_*` test proves each assertion bites on a mutated copy. It
//! asserts that:
//!
//! * the workflow runs on a nightly `schedule` and on `workflow_dispatch`
//!   with the inputs `target`, `seconds` (default 600), `planted_panic` and
//!   `dry_run_issues`; the top-level token is `contents: read`;
//! * job `targets` builds the matrix with `cargo xtask fuzz list --json`
//!   (never a hand-written list) and exposes it as output `targets`;
//! * job `fuzz` takes its matrix from that output (`fail-fast: false`),
//!   restores the corpus `fuzz/corpus/<target>` from the newest cache of the
//!   target (`restore-keys`), runs `cargo xtask fuzz run "$FUZZ_TARGET"
//!   --max-total-time "$FUZZ_SECONDS"` with `FUZZ_SECONDS` 600 unless a
//!   manual run says otherwise, never calls `cargo fuzz` directly or
//!   overrides a limit flag, enables the planted panic only when asked, saves
//!   the corpus under a new key with `if: always()`, and uploads
//!   `fuzz-out/` as `fuzz-failure-<target>` with `if: failure()`;
//! * job `report` runs only when `fuzz` failed, is the only job with
//!   `issues: write` (and nothing else writable), downloads the
//!   `fuzz-failure-*` artifacts and runs `cargo xtask fuzz file-issues` on
//!   them with `GH_TOKEN` from the workflow token, `--dry-run` only when
//!   asked;
//! * no `run:` script interpolates `${{ ... }}` (values reach scripts as
//!   environment variables, so inputs and crash data cannot inject shell),
//!   every checkout drops its credentials, and nothing is softened with
//!   `continue-on-error` or `|| true`.
//!
//! The names shared with xtask (artifact prefix, log and features files,
//! the planted feature and toy target) are taken from xtask itself, and
//! `fuzz list --json` is run here and compared with the fuzz manifest. The
//! issue filing is tested in `fuzz_issue.rs`; whether the jobs are green
//! can only be seen on GitHub.

#[path = "common/workflow.rs"]
mod workflow;

use std::path::Path;
use std::process::Command;

use workflow::{
    Job, Workflow, assert_flags, indent, is_blank_or_comment, normalize, skippable_problems,
    workflow_yml,
};
use xtask::fuzz::{self, SELFTEST_TARGET};
use xtask::fuzz_issue::{ARTIFACT_PREFIX, FEATURES_FILE, LABEL, LOG_FILE};

const FILE: &str = "fuzz.yml";
const TARGETS_JOB: &str = "targets";
const FUZZ_JOB: &str = "fuzz";
const REPORT_JOB: &str = "report";
const LIST_CMD: &str = "cargo xtask fuzz list --json --only \"$ONLY_TARGET\"";
const MATRIX: &str = "${{ fromJSON(needs.targets.outputs.targets) }}";
const RUN_CMD: &str = "cargo xtask fuzz run \"$FUZZ_TARGET\" --max-total-time \"$FUZZ_SECONDS\"";
const SECONDS: &str = "FUZZ_SECONDS: ${{ inputs.seconds || 600 }}";
const OUT_DIR: &str = "fuzz-out";
const PLANTED: &str = "planted-panic";
const CORPUS_PATH: &str = "fuzz/corpus/${{ matrix.target }}";
const CORPUS_KEY: &str =
    "fuzz-corpus-${{ matrix.target }}-${{ github.run_id }}-${{ github.run_attempt }}";
const CORPUS_RESTORE: &str = "fuzz-corpus-${{ matrix.target }}-";
const REPORT_IF: &str = "if: always() && needs.fuzz.result == 'failure'";
const FAILURES_DIR: &str = "fuzz-failures";
const FILE_CMD: &str = "cargo xtask fuzz file-issues --dir fuzz-failures --run-url \"$RUN_URL\"";
const DISPATCH_INPUTS: &[&str] = &["target", "seconds", "planted_panic", "dry_run_issues"];
/// libFuzzer flags whose value `cargo xtask fuzz run` sets from
/// fuzz/Cargo.toml; the workflow must not pass its own.
const LIMIT_FLAGS: &[&str] = &["-timeout=", "-rss_limit_mb=", "-malloc_limit_mb="];

fn uses(step: &Job<'_>) -> Option<String> {
    step.value_at("uses", 8)
}

fn with(step: &Job<'_>, key: &str) -> Option<String> {
    step.value_at(key, 10)
}

fn step_if(step: &Job<'_>) -> Option<String> {
    step.value_at("if", 8)
}

fn uses_action(step: &Job<'_>, action: &str) -> bool {
    uses(step).is_some_and(|u| u.starts_with(&format!("{action}@")))
}

/// Whether some content line of `lines` normalizes to `want`.
fn has_line(lines: &[&str], want: &str) -> bool {
    lines
        .iter()
        .filter(|l| !is_blank_or_comment(l))
        .any(|l| normalize(l) == want)
}

/// The lines of the `key:` block at `at` spaces of indent within `lines`.
fn block<'a>(lines: &[&'a str], key: &str, at: usize) -> Vec<&'a str> {
    let Some(start) = lines
        .iter()
        .position(|l| indent(l) == at && l.trim() == format!("{key}:"))
    else {
        return Vec::new();
    };
    lines[start + 1..]
        .iter()
        .copied()
        .take_while(|l| is_blank_or_comment(l) || indent(l) > at)
        .collect()
}

/// The job's `permissions:` entries that grant `write`.
fn writes(job: &Job<'_>) -> Vec<String> {
    block(&job.lines, "permissions", 4)
        .iter()
        .filter(|l| !is_blank_or_comment(l) && l.trim_end().ends_with(": write"))
        .map(|l| l.trim().to_owned())
        .collect()
}

fn on_problems(wf: &Workflow<'_>, problems: &mut Vec<String>) {
    let on = wf.on_block();
    let schedule = block(on, "schedule", 2);
    if !schedule
        .iter()
        .any(|l| l.trim_start().starts_with("- cron:"))
    {
        problems.push("the workflow has no `schedule` cron (nightly run)".to_owned());
    }
    let inputs = block(&block(on, "workflow_dispatch", 2), "inputs", 4);
    if !on
        .iter()
        .any(|l| indent(l) == 2 && l.trim().starts_with("workflow_dispatch"))
    {
        problems.push("the workflow does not run on `workflow_dispatch`".to_owned());
    }
    for i in DISPATCH_INPUTS {
        if !inputs
            .iter()
            .any(|l| indent(l) == 6 && l.trim() == format!("{i}:"))
        {
            problems.push(format!("`workflow_dispatch` has no input `{i}`"));
        }
    }
    let seconds = block(&inputs, "seconds", 6);
    if !has_line(&seconds, "default: 600") {
        problems.push("input `seconds` does not default to 600".to_owned());
    }
    for trigger in ["push", "pull_request", "pull_request_target"] {
        if on
            .iter()
            .any(|l| indent(l) == 2 && l.trim().starts_with(trigger))
        {
            problems.push(format!(
                "the workflow runs on `{trigger}` (it is the nightly job)"
            ));
        }
    }
    let top = wf.section("permissions");
    if !has_line(top, "contents: read") || top.iter().any(|l| l.contains("write")) {
        problems.push("the top-level `permissions` is not just `contents: read`".to_owned());
    }
}

fn job_or_problem<'w, 'a>(
    wf: &'w Workflow<'a>,
    id: &str,
    problems: &mut Vec<String>,
) -> Option<&'w Job<'a>> {
    let job = wf.job(id);
    if job.is_none() {
        problems.push(format!("no `{id}` job"));
    }
    job
}

fn targets_problems(job: &Job<'_>, problems: &mut Vec<String>) {
    let steps = job.steps();
    let list = steps.iter().find(|s| {
        s.run_scripts()
            .iter()
            .any(|r| r.contains(&format!("targets=$({LIST_CMD})")))
    });
    match list {
        None => problems.push(format!(
            "`{TARGETS_JOB}` does not build the matrix with `{LIST_CMD}` (scripts: {:?})",
            job.run_scripts()
        )),
        Some(s) => {
            if !s
                .run_scripts()
                .iter()
                .any(|r| r.contains("echo \"targets=$targets\" >> \"$GITHUB_OUTPUT\""))
            {
                problems.push(format!(
                    "`{TARGETS_JOB}` does not write the target list to $GITHUB_OUTPUT"
                ));
            }
            let id = s.value_at("id", 8).unwrap_or_default();
            let want = format!("targets: ${{{{ steps.{id}.outputs.targets }}}}");
            if id.is_empty() || !has_line(&block(&job.lines, "outputs", 4), &want) {
                problems.push(format!(
                    "`{TARGETS_JOB}` does not expose the list as output `targets`"
                ));
            }
            if !has_line(&s.lines, "ONLY_TARGET: ${{ inputs.target }}") {
                problems.push(format!(
                    "`{TARGETS_JOB}` does not pass input `target` as ONLY_TARGET"
                ));
            }
        }
    }
}

fn fuzz_problems(job: &Job<'_>, problems: &mut Vec<String>) {
    if !job.needs().iter().any(|n| n == TARGETS_JOB) {
        problems.push(format!(
            "`{FUZZ_JOB}.needs` does not include `{TARGETS_JOB}`"
        ));
    }
    let strategy = block(&job.lines, "strategy", 4);
    if !has_line(&strategy, &format!("target: {MATRIX}")) {
        problems.push(format!(
            "`{FUZZ_JOB}` matrix is not `target: {MATRIX}` (the `fuzz list` output)"
        ));
    }
    if !has_line(&strategy, "fail-fast: false") {
        problems.push(format!(
            "`{FUZZ_JOB}` is not `fail-fast: false` (one crash would cancel the other targets)"
        ));
    }
    let env = block(&job.lines, "env", 4);
    for want in [
        SECONDS,
        "FUZZ_TARGET: ${{ matrix.target }}",
        "PLANTED_PANIC: ${{ inputs.planted_panic || false }}",
        &format!("FUZZ_OUT: {OUT_DIR}"),
    ] {
        if !has_line(&env, want) {
            problems.push(format!("`{FUZZ_JOB}` env lacks `{want}`"));
        }
    }
    if !writes(job).is_empty() {
        problems.push(format!(
            "`{FUZZ_JOB}` has write permissions {:?}",
            writes(job)
        ));
    }

    let steps = job.steps();
    let installs = steps.iter().any(|s| {
        uses_action(s, "taiki-e/install-action")
            && with(s, "tool").is_some_and(|t| t.split(',').any(|t| t.trim() == "cargo-fuzz"))
    });
    if !installs {
        problems.push(format!("`{FUZZ_JOB}` does not install cargo-fuzz"));
    }
    if !job
        .run_scripts()
        .iter()
        .any(|r| r.contains("rustup toolchain install nightly"))
    {
        problems.push(format!("`{FUZZ_JOB}` does not install a nightly toolchain"));
    }

    let run = steps
        .iter()
        .position(|s| s.run_scripts().iter().any(|r| r.contains(RUN_CMD)));
    match run {
        None => problems.push(format!(
            "`{FUZZ_JOB}` does not run `{RUN_CMD}` (scripts: {:?})",
            job.run_scripts()
        )),
        Some(i) => {
            let script = steps[i].run_scripts().join("\n");
            let cmd = script
                .split(RUN_CMD)
                .nth(1)
                .and_then(|rest| rest.split(" 2>&1").next())
                .unwrap_or("");
            if !cmd.contains("--artifact-prefix \"$FUZZ_OUT/\"") {
                problems.push(format!(
                    "`{FUZZ_JOB}` does not save failing inputs to $FUZZ_OUT: {script}"
                ));
            }
            if !script.contains(&format!("| tee \"$FUZZ_OUT/{LOG_FILE}\"")) {
                problems.push(format!(
                    "`{FUZZ_JOB}` does not keep the log as $FUZZ_OUT/{LOG_FILE}"
                ));
            }
            if !script.contains(&format!("> \"$FUZZ_OUT/{FEATURES_FILE}\"")) {
                problems.push(format!(
                    "`{FUZZ_JOB}` does not record the features in $FUZZ_OUT/{FEATURES_FILE}"
                ));
            }
            if cmd.split(' ').any(|w| w == "--") {
                problems.push(format!(
                    "`{FUZZ_JOB}` passes extra libFuzzer flags to `fuzz run`: {cmd}"
                ));
            }
            let guard = format!(
                "if [ \"$PLANTED_PANIC\" = true ] && [ \"$FUZZ_TARGET\" = {SELFTEST_TARGET} ]; then features={PLANTED}"
            );
            if script.contains(PLANTED) && !script.contains(&guard) {
                problems.push(format!(
                    "`{FUZZ_JOB}` enables `{PLANTED}` other than on request for `{SELFTEST_TARGET}`: {script}"
                ));
            }
        }
    }

    let restore = steps.iter().position(|s| {
        uses_action(s, "actions/cache/restore")
            && with(s, "path").as_deref() == Some(CORPUS_PATH)
            && with(s, "key").as_deref() == Some(CORPUS_KEY)
            && restore_keys(s).iter().any(|k| k == CORPUS_RESTORE)
    });
    match (restore, run) {
        (None, _) => problems.push(format!(
            "`{FUZZ_JOB}` does not restore `{CORPUS_PATH}` (key `{CORPUS_KEY}`, restore-keys `{CORPUS_RESTORE}`)"
        )),
        (Some(r), Some(i)) if r > i => problems.push(format!(
            "`{FUZZ_JOB}` restores the corpus after fuzzing"
        )),
        _ => {}
    }
    let save = steps.iter().position(|s| {
        uses_action(s, "actions/cache/save")
            && with(s, "path").as_deref() == Some(CORPUS_PATH)
            && with(s, "key").as_deref() == Some(CORPUS_KEY)
            && step_if(s).as_deref() == Some("always()")
    });
    match (save, run) {
        (None, _) => problems.push(format!(
            "`{FUZZ_JOB}` does not save `{CORPUS_PATH}` under `{CORPUS_KEY}` with `if: always()`"
        )),
        (Some(s), Some(i)) if s < i => {
            problems.push(format!("`{FUZZ_JOB}` saves the corpus before fuzzing"));
        }
        _ => {}
    }
    let upload = steps.iter().position(|s| {
        uses_action(s, "actions/upload-artifact")
            && with(s, "name").as_deref()
                == Some(format!("{ARTIFACT_PREFIX}${{{{ matrix.target }}}}").as_str())
            && with(s, "path").as_deref() == Some(format!("{OUT_DIR}/").as_str())
            && step_if(s).as_deref() == Some("failure()")
    });
    match (upload, run) {
        (None, _) => problems.push(format!(
            "`{FUZZ_JOB}` does not upload `{OUT_DIR}/` as `{ARTIFACT_PREFIX}<target>` with `if: failure()`"
        )),
        (Some(u), Some(i)) if u < i => {
            problems.push(format!("`{FUZZ_JOB}` uploads before fuzzing"));
        }
        _ => {}
    }
    for step in &steps {
        let allowed = match step_if(step).as_deref() {
            None => true,
            Some("always()") => uses_action(step, "actions/cache/save"),
            Some("failure()") => uses_action(step, "actions/upload-artifact"),
            Some(_) => false,
        };
        if !allowed {
            problems.push(format!(
                "`{FUZZ_JOB}` step `{}` has a conditional {:?}",
                step.id,
                step_if(step)
            ));
        }
    }
}

/// The items of a step's `restore-keys: |` block scalar.
fn restore_keys(step: &Job<'_>) -> Vec<String> {
    let Some(pos) = step
        .lines
        .iter()
        .position(|l| indent(l) == 10 && l.trim_start().starts_with("restore-keys:"))
    else {
        return Vec::new();
    };
    step.lines[pos + 1..]
        .iter()
        .take_while(|l| is_blank_or_comment(l) || indent(l) > 10)
        .filter(|l| !is_blank_or_comment(l))
        .map(|l| l.trim().to_owned())
        .collect()
}

fn report_problems(job: &Job<'_>, problems: &mut Vec<String>) {
    if !job.needs().iter().any(|n| n == FUZZ_JOB) {
        problems.push(format!(
            "`{REPORT_JOB}.needs` does not include `{FUZZ_JOB}`"
        ));
    }
    let job_if = job
        .lines
        .iter()
        .find(|l| indent(l) == 4 && l.trim_start().starts_with("if:"))
        .map(|l| normalize(l));
    if job_if.as_deref() != Some(REPORT_IF) {
        problems.push(format!(
            "`{REPORT_JOB}` does not run exactly when `{FUZZ_JOB}` failed (`{REPORT_IF}`; found {job_if:?})"
        ));
    }
    let w = writes(job);
    if w != ["issues: write"] {
        problems.push(format!(
            "`{REPORT_JOB}` permissions grant {w:?}; it needs exactly `issues: write`"
        ));
    }
    let steps = job.steps();
    let download = steps.iter().position(|s| {
        uses_action(s, "actions/download-artifact")
            && with(s, "pattern").as_deref() == Some(format!("{ARTIFACT_PREFIX}*").as_str())
            && with(s, "path").as_deref() == Some(FAILURES_DIR)
    });
    let file = steps
        .iter()
        .position(|s| s.run_scripts().iter().any(|r| r.contains(FILE_CMD)));
    match (download, file) {
        (None, _) => problems.push(format!(
            "`{REPORT_JOB}` does not download the `{ARTIFACT_PREFIX}*` artifacts to `{FAILURES_DIR}`"
        )),
        (_, None) => problems.push(format!(
            "`{REPORT_JOB}` does not run `{FILE_CMD}` (scripts: {:?})",
            job.run_scripts()
        )),
        (Some(d), Some(f)) if d > f => problems.push(format!(
            "`{REPORT_JOB}` files issues before downloading the failures"
        )),
        (Some(_), Some(f)) => {
            let s = &steps[f];
            let token = has_line(&s.lines, "GH_TOKEN: ${{ github.token }}")
                || has_line(&s.lines, "GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}");
            if !token {
                problems.push(format!(
                    "`{REPORT_JOB}` does not give gh the workflow token (GH_TOKEN)"
                ));
            }
            if !has_line(&s.lines, "GH_REPO: ${{ github.repository }}") {
                problems.push(format!("`{REPORT_JOB}` does not set GH_REPO"));
            }
            let script = s.run_scripts().join("\n");
            let guard = "if [ \"$DRY_RUN\" = true ]; then dry=(--dry-run) fi";
            let tail = script
                .split(FILE_CMD)
                .nth(1)
                .map(str::trim)
                .unwrap_or_default();
            if !script.contains(guard)
                || script.matches("--dry-run").count() != 1
                || tail != "\"${dry[@]}\""
            {
                problems.push(format!(
                    "`{REPORT_JOB}` passes `--dry-run` other than on request (`{guard}`), or not on request: {script}"
                ));
            }
            if !has_line(&s.lines, "DRY_RUN: ${{ inputs.dry_run_issues || false }}") {
                problems.push(format!(
                    "`{REPORT_JOB}` does not take DRY_RUN from input `dry_run_issues`"
                ));
            }
        }
    }
    problems.extend(skippable_problems(job, &[]));
}

/// Every way `src` fails the US-RR-5-6 wiring; empty when it holds.
fn fuzz_workflow_problems(src: &str) -> Vec<String> {
    let wf = Workflow::parse(src);
    let mut problems = Vec::new();
    on_problems(&wf, &mut problems);
    if let Some(job) = job_or_problem(&wf, TARGETS_JOB, &mut problems) {
        targets_problems(job, &mut problems);
    }
    if let Some(job) = job_or_problem(&wf, FUZZ_JOB, &mut problems) {
        fuzz_problems(job, &mut problems);
    }
    if let Some(job) = job_or_problem(&wf, REPORT_JOB, &mut problems) {
        report_problems(job, &mut problems);
    }
    for job in &wf.jobs {
        if job.id != REPORT_JOB && !writes(job).is_empty() {
            problems.push(format!(
                "`{}` has write permissions {:?}; only `{REPORT_JOB}` may",
                job.id,
                writes(job)
            ));
        }
        if job.id != REPORT_JOB && job.content_lines().any(|l| l.contains("GH_TOKEN")) {
            problems.push(format!("`{}` sees GH_TOKEN", job.id));
        }
        for line in job.content_lines() {
            let l = line.strip_prefix("- ").unwrap_or(line);
            if l.starts_with("continue-on-error") {
                problems.push(format!("`{}` uses `{l}`", job.id));
            }
        }
        for s in job.run_scripts() {
            if s.contains("${{") {
                problems.push(format!(
                    "`{}` interpolates an expression into a script (use env): {s}",
                    job.id
                ));
            }
            if s.contains("|| true") || s.contains("|| :") || s.contains("exit 0") {
                problems.push(format!("`{}` step swallows failures: {s}", job.id));
            }
            if s.contains("cargo fuzz") || s.contains("+nightly fuzz") {
                problems.push(format!(
                    "`{}` runs cargo-fuzz directly (bypassing the limits in `cargo xtask fuzz run`): {s}",
                    job.id
                ));
            }
            for flag in LIMIT_FLAGS {
                if s.contains(flag) {
                    problems.push(format!("`{}` overrides libFuzzer `{flag}`: {s}", job.id));
                }
            }
        }
        for step in job.steps() {
            if uses_action(&step, "actions/checkout")
                && with(&step, "persist-credentials").as_deref() != Some("false")
            {
                problems.push(format!(
                    "`{}` checks out without `persist-credentials: false`",
                    job.id
                ));
            }
        }
    }
    problems
}

#[test]
fn fuzz_workflow_fuzzes_every_target_and_files_failures() {
    let problems = fuzz_workflow_problems(&workflow_yml(FILE));
    assert!(
        problems.is_empty(),
        ".github/workflows/{FILE} violates US-RR-5-6:\n{}",
        problems.join("\n")
    );
}

#[test]
fn fuzz_workflow_check_rejects_weakened_workflow() {
    let src = workflow_yml(FILE);
    let m = |edit: &dyn Fn(&str) -> String| {
        let changed = edit(&src);
        assert_ne!(changed, src, "mutation did not apply");
        fuzz_workflow_problems(&changed)
    };
    // Triggers and token.
    assert_flags(
        &m(&|s| s.replacen("    - cron: \"41 2 * * *\"\n", "", 1)),
        "no `schedule` cron",
    );
    assert_flags(
        &m(&|s| s.replacen("      planted_panic:\n", "      planted:\n", 1)),
        "no input `planted_panic`",
    );
    assert_flags(
        &m(&|s| s.replacen("        default: 600\n", "        default: 60\n", 1)),
        "does not default to 600",
    );
    assert_flags(
        &m(&|s| s.replacen("on:\n", "on:\n  push:\n", 1)),
        "runs on `push`",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "permissions:\n  contents: read\n",
                "permissions:\n  contents: write\n",
                1,
            )
        }),
        "top-level `permissions`",
    );

    // Matrix from `fuzz list`.
    assert_flags(
        &m(&|s| {
            s.replacen(
                "cargo xtask fuzz list --json --only",
                "echo '[\"ecu_id\"]' #",
                1,
            )
        }),
        "does not build the matrix",
    );
    assert_flags(
        &m(&|s| s.replacen(MATRIX, "[ecu_id, toy]", 1)),
        "matrix is not",
    );
    assert_flags(
        &m(&|s| s.replacen("      fail-fast: false\n", "", 1)),
        "fail-fast: false",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "      targets: ${{ steps.list.outputs.targets }}\n",
                "      targets: '[\"toy\"]'\n",
                1,
            )
        }),
        "output `targets`",
    );

    // The run: wrapper, duration, limits, planted panic.
    assert_flags(
        &m(&|s| {
            s.replacen(
                "cargo xtask fuzz run \"$FUZZ_TARGET\"",
                "cargo +nightly fuzz run \"$FUZZ_TARGET\"",
                1,
            )
        }),
        "does not run `cargo xtask fuzz run",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "cargo xtask fuzz run \"$FUZZ_TARGET\"",
                "cargo +nightly fuzz run \"$FUZZ_TARGET\"",
                1,
            )
        }),
        "runs cargo-fuzz directly",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "${{ inputs.seconds || 600 }}",
                "${{ inputs.seconds || 60 }}",
                1,
            )
        }),
        "env lacks `FUZZ_SECONDS",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "--artifact-prefix \"$FUZZ_OUT/\"",
                "--artifact-prefix \"$FUZZ_OUT/\" -- -timeout=1200",
                1,
            )
        }),
        "overrides libFuzzer `-timeout=`",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "--artifact-prefix \"$FUZZ_OUT/\"",
                "--artifact-prefix \"$FUZZ_OUT/\" -- -rss_limit_mb=0",
                1,
            )
        }),
        "passes extra libFuzzer flags",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "if [ \"$PLANTED_PANIC\" = true ] && [ \"$FUZZ_TARGET\" = toy ]; then",
                "if true; then",
                1,
            )
        }),
        "enables `planted-panic` other than on request",
    );
    assert_flags(
        &m(&|s| s.replacen(" | tee \"$FUZZ_OUT/fuzz.log\"", "", 1)),
        "does not keep the log",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "--max-total-time \"$FUZZ_SECONDS\"",
                "--max-total-time \"${{ inputs.seconds }}\"",
                1,
            )
        }),
        "interpolates an expression into a script",
    );

    // Corpus cache.
    assert_flags(
        &m(&|s| {
            s.replacen(
                "          restore-keys: |\n            fuzz-corpus-${{ matrix.target }}-\n",
                "",
                1,
            )
        }),
        "does not restore",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "uses: actions/cache/save@v4",
                "uses: actions/cache/restore@v4",
                1,
            )
        }),
        "does not save",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "      - name: Save corpus\n        if: always()\n",
                "      - name: Save corpus\n",
                1,
            )
        }),
        "does not save",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "fuzz-corpus-${{ matrix.target }}-${{ github.run_id }}-${{ github.run_attempt }}",
                "fuzz-corpus-${{ matrix.target }}",
                1,
            )
        }),
        "does not restore",
    );

    // Upload.
    assert_flags(
        &m(&|s| {
            s.replacen(
                "      - name: Upload failing inputs\n        if: failure()\n",
                "      - name: Upload failing inputs\n",
                1,
            )
        }),
        "does not upload",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "name: fuzz-failure-${{ matrix.target }}",
                "name: crash-${{ matrix.target }}",
                1,
            )
        }),
        "does not upload",
    );

    // Report job: least privilege, condition, filing.
    assert_flags(
        &m(&|s| {
            s.replacen(
                "    if: always() && needs.fuzz.result == 'failure'\n",
                "",
                1,
            )
        }),
        "does not run exactly when",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "      issues: write\n",
                "      issues: write\n      pull-requests: write\n",
                1,
            )
        }),
        "needs exactly `issues: write`",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "  fuzz:\n    name: fuzz ${{ matrix.target }}\n",
                "  fuzz:\n    name: fuzz ${{ matrix.target }}\n    permissions:\n      issues: write\n",
                1,
            )
        }),
        "only `report` may",
    );
    assert_flags(
        &m(&|s| s.replacen("          GH_TOKEN: ${{ github.token }}\n", "", 1)),
        "GH_TOKEN",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "--run-url \"$RUN_URL\" \"${dry[@]}\"",
                "--run-url \"$RUN_URL\" --dry-run",
                1,
            )
        }),
        "`--dry-run` other than on request",
    );
    assert_flags(
        &m(&|s| s.replacen("pattern: fuzz-failure-*", "pattern: '*'", 1)),
        "does not download",
    );
    assert_flags(
        &m(&|s| s.replacen("cargo xtask fuzz file-issues --dir", "echo --dir", 1)),
        "does not run `cargo xtask fuzz file-issues",
    );
    assert_flags(
        &m(&|s| {
            s.replacen("cargo xtask fuzz file-issues --dir fuzz-failures --run-url \"$RUN_URL\" \"${dry[@]}\"", "cargo xtask fuzz file-issues --dir fuzz-failures --run-url \"$RUN_URL\" \"${dry[@]}\" || true", 1)
        }),
        "swallows failures",
    );
    assert_flags(
        &m(&|s| s.replacen("          persist-credentials: false\n", "", 1)),
        "persist-credentials: false",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "    timeout-minutes: 40\n",
                "    timeout-minutes: 40\n    continue-on-error: true\n",
                1,
            )
        }),
        "continue-on-error",
    );
}

/// The workflow and xtask agree on names: the planted feature exists, the
/// toy target exists, the label is `fuzz`.
#[test]
fn fuzz_workflow_names_match_the_fuzz_crate() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let m = fuzz::load(&root).unwrap();
    assert!(m.features.iter().any(|f| f == PLANTED), "{:?}", m.features);
    assert!(m.targets.iter().any(|t| t.name == SELFTEST_TARGET));
    assert_eq!(LABEL, "fuzz");
}

/// The matrix command prints exactly the fuzz crate's targets, as JSON, and
/// `--only` narrows it (or fails on an unknown name).
#[test]
fn fuzz_list_json_is_the_manifest_targets() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let want: Vec<String> = fuzz::load(&root)
        .unwrap()
        .targets
        .iter()
        .map(|t| t.name.clone())
        .collect();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["fuzz", "list", "--json"])
            .args(args)
            .output()
            .unwrap()
    };
    let o = run(&[]);
    assert!(o.status.success());
    let got: Vec<String> = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(got, want);
    let o = run(&["--only", ""]);
    assert_eq!(
        serde_json::from_slice::<Vec<String>>(&o.stdout).unwrap(),
        want
    );
    let o = run(&["--only", SELFTEST_TARGET]);
    assert_eq!(
        serde_json::from_slice::<Vec<String>>(&o.stdout).unwrap(),
        [SELFTEST_TARGET]
    );
    let o = run(&["--only", "no-such-target"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("no fuzz target"));
}

/// The template target (US-RR-5-1): docs/fuzzing.md names `ecu_id` as the
/// template every new target copies, it is a `[[bin]]` of the fuzz crate, and
/// so it is a leg of the nightly matrix (`fuzz list`). A target added the way
/// the docs describe (a new `[[bin]]` block) joins the matrix with no
/// workflow edit, and dropping the template's `[[bin]]` drops it from the
/// matrix, so the list really is derived from the manifest.
#[test]
fn nightly_matrix_runs_the_template_and_picks_up_new_targets() {
    const TEMPLATE: &str = "ecu_id";
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let docs = std::fs::read_to_string(root.join("docs/fuzzing.md")).unwrap();
    assert!(
        docs.contains(&format!("Use `{TEMPLATE}` (the template) as the model")),
        "docs/fuzzing.md no longer names `{TEMPLATE}` as the template"
    );
    let text = std::fs::read_to_string(root.join("fuzz/Cargo.toml"))
        .unwrap()
        .replace("\r\n", "\n");
    let listed =
        |text: &str| fuzz::select_targets(&fuzz::parse_manifest(text).unwrap(), None).unwrap();
    let committed = listed(&text);
    assert!(
        committed.iter().any(|t| t == TEMPLATE),
        "template `{TEMPLATE}` is not in the nightly matrix {committed:?}"
    );

    let bin = |name: &str| {
        format!(
            "[[bin]]\nname = \"{name}\"\npath = \"fuzz_targets/{name}.rs\"\ntest = false\ndoc = false\nbench = false\n"
        )
    };
    let added = listed(&format!("{text}\n{}", bin("defs_xml")));
    let mut want = committed.clone();
    want.push("defs_xml".to_owned());
    assert_eq!(added, want, "a new [[bin]] is not picked up by `fuzz list`");

    let template_bin = bin(TEMPLATE);
    assert!(
        text.contains(&template_bin),
        "template [[bin]] block changed"
    );
    let without = listed(&text.replacen(&template_bin, "", 1));
    assert!(
        !without.iter().any(|t| t == TEMPLATE),
        "`fuzz list` still lists `{TEMPLATE}` without its [[bin]]: {without:?}"
    );
}

/// The formula target (US-RR-20-3, story US-RR-20 "the expr fuzz target runs
/// nightly"): `expr` is a leg of the nightly matrix, from the same `fuzz
/// list --json` the `targets` job runs, a manual run can pick it alone, and
/// it starts each night from committed seeds and replays committed
/// regressions. Pinned by name, so dropping the target (its `[[bin]]` and
/// `TARGETS` entry together, which `fuzz check` alone would accept) fails
/// here.
#[test]
fn nightly_matrix_fuzzes_the_expr_target() {
    const EXPR: &str = "expr";
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let list = |only: &str| {
        let o = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["fuzz", "list", "--json", "--only", only])
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        serde_json::from_slice::<Vec<String>>(&o.stdout).unwrap()
    };
    let nightly = list("");
    assert!(
        nightly.iter().any(|t| t == EXPR),
        "`{EXPR}` is not in the nightly matrix {nightly:?}"
    );
    assert_eq!(list(EXPR), [EXPR]);
    for dir in ["seeds", "regressions"] {
        let path = root.join("fuzz").join(dir).join(EXPR);
        let n = std::fs::read_dir(&path)
            .unwrap_or_else(|e| panic!("{path:?}: {e}"))
            .count();
        assert!(n > 0, "{path:?} is empty");
    }
}

/// The XML target (US-RR-27-3, story US-RR-27 "the XML fuzz target runs
/// nightly with no panics, hangs or unbounded allocation"): `xml` is a leg
/// of the nightly matrix (the `fuzz list --json` the `targets` job runs), a
/// manual run can pick it alone, and every run of it, fuzzing or
/// reproducing, gets the committed `-timeout`, `-rss_limit_mb` and
/// `-malloc_limit_mb` as its first libFuzzer flags, which is what makes a
/// hang or a runaway allocation fail the leg (`cargo xtask fuzz selftest`
/// proves each limit fires). It starts each night from committed seeds, and
/// its committed regressions are exactly the hostile sample set of
/// `crates/rr-defs/tests/data/xml/hostile/` (billion laughs, XXE, deep
/// nesting, ...), byte for byte, so a grown corpus leaking into
/// `fuzz/regressions/xml/` fails here. Pinned by name, so dropping the target
/// (its `[[bin]]` and `TARGETS` entry together) fails here.
#[test]
fn nightly_matrix_fuzzes_the_xml_target_under_the_limits() {
    const XML: &str = "xml";
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let list = |only: &str| {
        let o = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["fuzz", "list", "--json", "--only", only])
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        serde_json::from_slice::<Vec<String>>(&o.stdout).unwrap()
    };
    let nightly = list("");
    assert!(
        nightly.iter().any(|t| t == XML),
        "`{XML}` is not in the nightly matrix {nightly:?}"
    );
    assert_eq!(list(XML), [XML]);

    let limits = fuzz::load(&root).unwrap().limits.expect("committed limits");
    assert!(fuzz::limit_problems(Some(limits)).is_empty());
    let want = [
        format!("-timeout={}", limits.timeout_secs),
        format!("-rss_limit_mb={}", limits.rss_limit_mb),
        format!("-malloc_limit_mb={}", limits.malloc_limit_mb),
    ];
    for inputs in [
        vec![],
        vec![root.join("fuzz/regressions/xml/billion-laughs.xml")],
    ] {
        let opts = fuzz::RunOptions {
            target: XML.to_owned(),
            max_total_time: Some(600),
            inputs,
            ..fuzz::RunOptions::default()
        };
        let cmd = fuzz::run_command(&root, limits, "nightly", "x86_64-unknown-linux-gnu", &opts);
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let dash = args.iter().position(|a| a == "--").expect("--");
        assert_eq!(args[dash + 1..dash + 4], want, "{args:?}");
        assert!(args[..dash].iter().any(|a| a == XML), "{args:?}");
    }

    let files = |dir: &Path| {
        let mut v: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("{dir:?}: {e}"))
            .map(|e| {
                let e = e.unwrap();
                (
                    e.file_name().to_string_lossy().into_owned(),
                    std::fs::read(e.path()).unwrap(),
                )
            })
            // `.gitattributes` (the hostile set is `binary`) is not an input.
            .filter(|(name, _)| !name.starts_with('.'))
            .collect();
        v.sort();
        v
    };
    let seeds = files(&root.join("fuzz/seeds/xml"));
    assert!(!seeds.is_empty(), "fuzz/seeds/xml is empty");
    assert!(
        seeds.len() <= 32,
        "fuzz/seeds/xml has {} files: the grown corpus belongs in fuzz/corpus/xml",
        seeds.len()
    );
    let regressions = files(&root.join("fuzz/regressions/xml"));
    let hostile = files(&root.join("crates/rr-defs/tests/data/xml/hostile"));
    for name in [
        "billion-laughs.xml",
        "xxe-system-file.xml",
        "deep-nesting.xml",
    ] {
        assert!(
            hostile.iter().any(|(n, _)| n == name),
            "hostile sample {name} missing"
        );
    }
    let names = |v: &[(String, Vec<u8>)]| v.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>();
    assert_eq!(
        names(&regressions),
        names(&hostile),
        "fuzz/regressions/xml is not the hostile sample set"
    );
    assert!(
        regressions == hostile,
        "fuzz/regressions/xml differs from the hostile samples byte for byte"
    );
}
