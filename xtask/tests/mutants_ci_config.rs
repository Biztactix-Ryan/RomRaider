//! `mutants.yml` and `.cargo/mutants.toml` wiring for story US-RR-4 (task
//! US-RR-4-6): cargo-mutants runs `--in-diff` on pull requests touching the
//! gated modules (every gated module when their tests change), a full run
//! weekly whose report is uploaded as an artifact, and any surviving mutant
//! fails the job.
//!
//! [`workflow_problems`] and [`config_problems`] are pure checks over the
//! text of the two files; one test runs them on the committed files and the
//! `*_rejects_*` tests prove each assertion bites on a mutated copy. They
//! assert that:
//!
//! * `.cargo/mutants.toml` gates every module named by PROJECT.md T7 and the
//!   story ([`REQUIRED_GLOBS`]), only under `crates/<gated crate>/src/`,
//!   gates the self-test's module path, and runs tests with nextest;
//! * the workflow runs on `pull_request` (every base branch), on a weekly
//!   `schedule` and on `workflow_dispatch`, and the pull request `paths:`
//!   cover every crate holding a gated glob, the shared test support, the
//!   config files the scope rules treat as the gate, and the self-test;
//! * job `pr` (pull requests only) checks out the full history, writes
//!   `git diff --no-color --no-ext-diff "$PR_BASE_SHA"...HEAD > pr.diff`
//!   from the pull request's base commit, asks `cargo xtask mutants-scope`
//!   and runs `cargo mutants` (full) or `cargo mutants --in-diff pr.diff`,
//!   then uploads `mutants.out` with `if: always()`;
//! * job `full` (every other event: the schedule and manual runs) runs
//!   `cargo mutants` over every gated module and uploads `mutants.out` as
//!   artifact `mutants-report-full` with `if: always()` and
//!   `if-no-files-found: error`;
//! * job `selftest` runs `cargo xtask mutants-selftest` on every event;
//! * no `cargo mutants` command narrows or replaces the configuration
//!   (`--file`, `--exclude`, `--re`, `--shard`, `--config`, ...), every job
//!   installs cargo-mutants and cargo-nextest, and no step can be skipped or
//!   softened (`if:` other than the uploads' `always()`,
//!   `continue-on-error`, `|| true`); the only job-level `if:`s are the two
//!   event selectors.
//!
//! What the commands do with a weakened test is proved by
//! `cargo xtask mutants-selftest`; whether the jobs are green can only be
//! seen on GitHub.

#[path = "common/workflow.rs"]
mod workflow;

use std::fs;
use std::path::Path;

use workflow::{
    Job, Workflow, assert_flags, indent, is_blank_or_comment, key_at_any, normalize,
    skippable_problems, workflow_yml,
};
use xtask::mutants_scope::{CONFIG_FILES, GATED_CRATES, TEST_SUPPORT_DIRS};
use xtask::mutants_selftest::{DIFF_ARGS, GATED_FILE};

const FILE: &str = "mutants.yml";
const PR_JOB: &str = "pr";
const FULL_JOB: &str = "full";
const SELFTEST_JOB: &str = "selftest";
const PR_IF: &str = "if: github.event_name == 'pull_request'";
const FULL_IF: &str = "if: github.event_name != 'pull_request'";
const UPLOAD: &str = "Upload mutants report";
const REPORT_DIR: &str = "mutants.out";
const FULL_ARTIFACT: &str = "mutants-report-full";
const PR_ARTIFACT: &str = "mutants-report-pr";
const MUTANTS: &str = "cargo mutants --jobs 2 --output .";
const SCOPE_CMD: &str = "cargo xtask mutants-scope --diff pr.diff";
const SELFTEST_CMD: &str = "cargo xtask mutants-selftest";
/// Flags that narrow, widen or replace what `.cargo/mutants.toml` gates.
const FORBIDDEN_FLAGS: &[&str] = &[
    "-f",
    "--file",
    "-e",
    "--exclude",
    "-E",
    "--exclude-re",
    "-F",
    "--re",
    "-p",
    "--package",
    "--shard",
    "--config",
    "--no-config",
    "--iterate",
    "--check",
    "--list",
    "--baseline",
];

/// The gated modules (PROJECT.md T7; story US-RR-4).
const REQUIRED_GLOBS: &[&str] = &[
    "crates/rr-core/src/encoding.rs",
    "crates/rr-core/src/encoding/**",
    "crates/rr-core/src/expr.rs",
    "crates/rr-core/src/expr/**",
    "crates/rr-core/src/conversion.rs",
    "crates/rr-core/src/conversion/**",
    "crates/rr-core/src/layer.rs",
    "crates/rr-core/src/layer/**",
    "crates/rr-rom/src/journal.rs",
    "crates/rr-rom/src/journal/**",
    "crates/rr-rom/src/checksum.rs",
    "crates/rr-rom/src/checksum/**",
    "crates/rr-rom/src/table/write.rs",
    "crates/rr-rom/src/table/write/**",
    "crates/rr-comms/src/engine.rs",
    "crates/rr-comms/src/engine/**",
    "crates/rr-comms/src/write.rs",
    "crates/rr-comms/src/write/**",
];

fn mutants_toml() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.cargo/mutants.toml");
    fs::read_to_string(&path)
        .map(|s| s.replace("\r\n", "\n"))
        .unwrap_or_else(|e| format!("<unreadable {}: {e}>", path.display()))
}

/// Every way `src` (`.cargo/mutants.toml`) fails the gate configuration.
fn config_problems(src: &str) -> Vec<String> {
    let table: toml::Table = match src.parse() {
        Ok(t) => t,
        Err(e) => return vec![format!(".cargo/mutants.toml does not parse: {e}")],
    };
    let mut problems = Vec::new();
    let globs: Vec<&str> = table
        .get("examine_globs")
        .and_then(toml::Value::as_array)
        .map(|a| a.iter().filter_map(toml::Value::as_str).collect())
        .unwrap_or_default();
    for g in REQUIRED_GLOBS {
        if !globs.contains(g) {
            problems.push(format!("`examine_globs` does not gate `{g}`"));
        }
    }
    if !globs.contains(&GATED_FILE) {
        problems.push(format!(
            "`examine_globs` does not gate `{GATED_FILE}`, the mutants-selftest module"
        ));
    }
    for g in &globs {
        let in_gated = GATED_CRATES
            .iter()
            .any(|c| g.starts_with(&format!("crates/{c}/src/")));
        if !in_gated {
            problems.push(format!(
                "`examine_globs` entry `{g}` is outside crates/{{{}}}/src/",
                GATED_CRATES.join(",")
            ));
        }
    }
    if table.get("test_tool").and_then(toml::Value::as_str) != Some("nextest") {
        problems.push("`test_tool` is not \"nextest\"".to_owned());
    }
    for key in ["test_workspace", "test_package", "examine_re"] {
        if table.contains_key(key) {
            problems.push(format!(
                "`{key}` is set: it changes which tests or mutants the gate runs"
            ));
        }
    }
    problems
}

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

/// The block list under `key:` (at indent 4) of trigger `trigger`.
fn trigger_list(wf: &Workflow<'_>, trigger: &str, key: &str) -> Vec<String> {
    let body = trigger_body(wf, trigger);
    let Some(pos) = body
        .iter()
        .position(|l| indent(l) == 4 && l.trim() == format!("{key}:"))
    else {
        return Vec::new();
    };
    body[pos + 1..]
        .iter()
        .take_while(|l| is_blank_or_comment(l) || indent(l) > 4)
        .filter(|l| !is_blank_or_comment(l))
        .filter_map(|l| l.trim().strip_prefix("- "))
        .map(|s| s.trim().trim_matches(|c| c == '"' || c == '\'').to_owned())
        .collect()
}

fn uses(step: &Job<'_>) -> Option<String> {
    step.value_at("uses", 8)
}

fn with(step: &Job<'_>, key: &str) -> Option<String> {
    step.value_at(key, 10)
}

/// The job-level `if:` line, normalized.
fn job_if(job: &Job<'_>) -> Option<String> {
    job.lines
        .iter()
        .find(|l| indent(l) == 4 && l.trim_start().starts_with("if:"))
        .map(|l| normalize(l))
}

/// Index of the first step running a script that contains `needle`.
fn step_running(steps: &[Job<'_>], needle: &str) -> Option<usize> {
    steps
        .iter()
        .position(|s| s.run_scripts().iter().any(|r| r.contains(needle)))
}

/// Problems with a job's tools, `if:` and skippable steps.
fn job_or_problem<'w, 'a>(
    wf: &'w Workflow<'a>,
    id: &str,
    want_if: Option<&str>,
    problems: &mut Vec<String>,
) -> Option<&'w Job<'a>> {
    let Some(job) = wf.job(id) else {
        problems.push(format!("no `{id}` job"));
        return None;
    };
    let got = job_if(job);
    if got.as_deref() != want_if {
        problems.push(format!(
            "`{id}` job-level `if:` is {got:?}, expected {want_if:?}"
        ));
    }
    problems.extend(skippable_problems(job, &[UPLOAD]));
    let installs = job.steps().iter().any(|s| {
        uses(s).is_some_and(|u| u.starts_with("taiki-e/install-action@"))
            && with(s, "tool").is_some_and(|t| {
                let tools: Vec<&str> = t.split(',').map(str::trim).collect();
                tools.contains(&"cargo-mutants") && tools.contains(&"cargo-nextest")
            })
    });
    if !installs {
        problems.push(format!(
            "`{id}` does not install cargo-mutants and cargo-nextest"
        ));
    }
    for s in job.run_scripts() {
        for cmd in s.split('\n').flat_map(|l| l.split(" cargo mutants")) {
            let words: Vec<&str> = cmd.split(' ').collect();
            if let Some(w) = words.iter().find(|w| {
                FORBIDDEN_FLAGS.contains(w)
                    || FORBIDDEN_FLAGS
                        .iter()
                        .any(|f| f.starts_with("--") && w.starts_with(&format!("{f}=")))
            }) && s.contains("cargo mutants")
            {
                problems.push(format!(
                    "`{id}` runs cargo mutants with `{w}`, overriding .cargo/mutants.toml: {s}"
                ));
            }
        }
    }
    Some(job)
}

/// Problems with the report upload of `job`, which must follow `run_step`.
fn upload_problems(
    job: &Job<'_>,
    artifact: &str,
    if_no_files: &str,
    run_step: Option<usize>,
    problems: &mut Vec<String>,
) {
    let id = job.id;
    let steps = job.steps();
    let upload = steps.iter().position(|s| {
        uses(s).is_some_and(|u| u.starts_with("actions/upload-artifact@"))
            && with(s, "name").as_deref() == Some(artifact)
            && with(s, "path").as_deref() == Some(REPORT_DIR)
            && with(s, "if-no-files-found").as_deref() == Some(if_no_files)
            && s.id == UPLOAD
            && s.content_lines().any(|l| normalize(l) == "if: always()")
    });
    match (run_step, upload) {
        (_, None) => problems.push(format!(
            "`{id}` does not upload `{REPORT_DIR}` as artifact `{artifact}` in step `{UPLOAD}` with `if: always()` and `if-no-files-found: {if_no_files}`"
        )),
        (Some(r), Some(u)) if u < r => {
            problems.push(format!("`{id}` uploads the report before running cargo mutants"));
        }
        _ => {}
    }
}

/// Every way `src` (`mutants.yml`) fails the US-RR-4-6 wiring.
fn workflow_problems(src: &str, toml_src: &str) -> Vec<String> {
    let wf = Workflow::parse(src);
    let mut problems = Vec::new();
    for t in ["pull_request", "schedule", "workflow_dispatch"] {
        if key_at_any(wf.on_block(), t).is_none() {
            problems.push(format!("workflow does not run on `{t}`"));
        }
    }
    if !trigger_body(&wf, "schedule")
        .iter()
        .any(|l| l.trim_start().starts_with("- cron:"))
    {
        problems.push("`schedule` has no `cron` (weekly full run)".to_owned());
    }
    let branches = trigger_body(&wf, "pull_request")
        .iter()
        .find(|l| indent(l) == 4 && l.trim_start().starts_with("branches:"))
        .map(|l| normalize(l));
    if branches.as_deref() != Some("branches: [\"**\"]") {
        problems.push(format!(
            "`pull_request` is not on every base branch: {branches:?}"
        ));
    }
    let paths = trigger_list(&wf, "pull_request", "paths");
    let mut want: Vec<String> = Vec::new();
    // Every crate holding a gated glob (from the config, so a new glob in a
    // new place needs its path here too).
    let table: toml::Table = toml_src.parse().unwrap_or_default();
    let globs = table
        .get("examine_globs")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for g in globs.iter().filter_map(toml::Value::as_str) {
        let krate: Vec<&str> = g.splitn(3, '/').take(2).collect();
        want.push(format!("{}/**", krate.join("/")));
    }
    want.extend(GATED_CRATES.iter().map(|c| format!("crates/{c}/**")));
    want.extend(TEST_SUPPORT_DIRS.iter().map(|d| format!("{d}**")));
    want.extend(CONFIG_FILES.iter().map(|&f| f.to_owned()));
    want.push("xtask/src/mutants_selftest.rs".to_owned());
    want.dedup();
    for w in &want {
        if !paths.contains(w) {
            problems.push(format!(
                "`pull_request` paths do not include `{w}` (paths: {paths:?})"
            ));
        }
    }

    let diff_cmd = format!(
        "git {} \"$PR_BASE_SHA\"...HEAD > pr.diff",
        DIFF_ARGS.join(" ")
    );
    if let Some(job) = job_or_problem(&wf, PR_JOB, Some(PR_IF), &mut problems) {
        let steps = job.steps();
        let full = steps.iter().any(|s| {
            uses(s).is_some_and(|u| u.starts_with("actions/checkout@"))
                && with(s, "fetch-depth").as_deref() == Some("0")
        });
        if !full {
            problems.push(format!(
                "`{PR_JOB}` does not check out the full history (`fetch-depth: 0`)"
            ));
        }
        let diff = steps
            .iter()
            .position(|s| s.run_scripts().contains(&diff_cmd));
        match diff {
            None => problems.push(format!("`{PR_JOB}` does not run `{diff_cmd}`")),
            Some(i) => {
                let base = steps[i].content_lines().any(|l| {
                    normalize(l) == "PR_BASE_SHA: ${{ github.event.pull_request.base.sha }}"
                });
                if !base {
                    problems.push(format!(
                        "`{PR_JOB}` does not diff against the pull request's base commit"
                    ));
                }
            }
        }
        let run = step_running(&steps, SCOPE_CMD);
        match run {
            None => problems.push(format!("`{PR_JOB}` does not run `{SCOPE_CMD}`")),
            Some(i) => {
                let script = steps[i].run_scripts().join("\n");
                let want = format!(
                    "scope=$({SCOPE_CMD}) echo \"Mutation scope: $scope\" if [ \"$scope\" = full ]; then {MUTANTS} else {MUTANTS} --in-diff pr.diff fi"
                );
                if script != want {
                    problems.push(format!(
                        "`{PR_JOB}` does not run `{MUTANTS}` on scope `full` and `{MUTANTS} --in-diff pr.diff` otherwise:\n{script}"
                    ));
                }
                if diff.is_some_and(|d| d > i) {
                    problems.push(format!(
                        "`{PR_JOB}` runs cargo mutants before writing the diff"
                    ));
                }
            }
        }
        upload_problems(job, PR_ARTIFACT, "warn", run, &mut problems);
    }

    if let Some(job) = job_or_problem(&wf, FULL_JOB, Some(FULL_IF), &mut problems) {
        let steps = job.steps();
        let run = steps
            .iter()
            .position(|s| s.run_scripts().iter().any(|r| r == MUTANTS));
        if run.is_none() {
            problems.push(format!(
                "`{FULL_JOB}` does not run `{MUTANTS}` (scripts: {:?})",
                job.run_scripts()
            ));
        }
        if job.run_scripts().iter().any(|s| s.contains("--in-diff")) {
            problems.push(format!(
                "`{FULL_JOB}` runs `--in-diff`, not every gated module"
            ));
        }
        upload_problems(job, FULL_ARTIFACT, "error", run, &mut problems);
    }

    if let Some(job) = job_or_problem(&wf, SELFTEST_JOB, None, &mut problems)
        && !job.run_scripts().iter().any(|s| s == SELFTEST_CMD)
    {
        problems.push(format!("`{SELFTEST_JOB}` does not run `{SELFTEST_CMD}`"));
    }
    problems
}

#[test]
fn mutants_config_gates_the_safety_modules() {
    let problems = config_problems(&mutants_toml());
    assert!(
        problems.is_empty(),
        ".cargo/mutants.toml violates US-RR-4-6:\n{}",
        problems.join("\n")
    );
}

#[test]
fn mutants_workflow_runs_and_gates_the_mutation_tests() {
    let problems = workflow_problems(&workflow_yml(FILE), &mutants_toml());
    assert!(
        problems.is_empty(),
        ".github/workflows/{FILE} violates US-RR-4-6:\n{}",
        problems.join("\n")
    );
}

#[test]
fn mutants_config_check_rejects_weakened_config() {
    let src = mutants_toml();
    let m = |edit: &dyn Fn(&str) -> String| {
        let changed = edit(&src);
        assert_ne!(changed, src, "mutation did not apply");
        config_problems(&changed)
    };
    assert_flags(
        &m(&|s| s.replacen("    \"crates/rr-rom/src/journal/**\",\n", "", 1)),
        "does not gate `crates/rr-rom/src/journal/**`",
    );
    assert_flags(
        &m(&|s| s.replacen("    \"crates/rr-core/src/encoding.rs\",\n", "", 1)),
        "the mutants-selftest module",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "examine_globs = [\n",
                "examine_globs = [\n    \"crates/rr-cli/src/main.rs\",\n",
                1,
            )
        }),
        "`crates/rr-cli/src/main.rs` is outside",
    );
    assert_flags(
        &m(&|s| s.replacen("test_tool = \"nextest\"", "test_tool = \"cargo\"", 1)),
        "`test_tool` is not",
    );
    assert_flags(
        &m(&|s| format!("{s}test_workspace = true\n")),
        "`test_workspace` is set",
    );
    assert_flags(
        &m(&|s| format!("{s}examine_re = [\"x\"]\n")),
        "`examine_re` is set",
    );
    assert_flags(&m(&|s| format!("{s}[[[")), "does not parse");
}

#[test]
fn mutants_workflow_check_rejects_weakened_workflow() {
    let src = workflow_yml(FILE);
    let toml_src = mutants_toml();
    let m = |edit: &dyn Fn(&str) -> String| {
        let changed = edit(&src);
        assert_ne!(changed, src, "mutation did not apply");
        workflow_problems(&changed, &toml_src)
    };
    // Triggers.
    assert_flags(
        &m(&|s| s.replacen("  pull_request:\n", "  pull_request_target:\n", 1)),
        "does not run on `pull_request`",
    );
    assert_flags(
        &m(&|s| s.replacen("    - cron: \"17 3 * * 1\"\n", "", 1)),
        "no `cron`",
    );
    assert_flags(
        &m(&|s| s.replacen("  workflow_dispatch:\n", "", 1)),
        "does not run on `workflow_dispatch`",
    );
    assert_flags(
        &m(&|s| s.replacen("    branches: [\"**\"]\n", "    branches: [master]\n", 1)),
        "not on every base branch",
    );
    assert_flags(
        &m(&|s| s.replacen("      - \"crates/rr-comms/**\"\n", "", 1)),
        "paths do not include `crates/rr-comms/**`",
    );
    assert_flags(
        &m(&|s| s.replacen("      - \"fixtures/**\"\n", "", 1)),
        "paths do not include `fixtures/**`",
    );
    assert_flags(
        &m(&|s| s.replacen("      - \".cargo/mutants.toml\"\n", "", 1)),
        "paths do not include `.cargo/mutants.toml`",
    );
    // A glob gated outside the listed crates needs its crate in the paths.
    {
        let toml_src = toml_src.replacen(
            "examine_globs = [\n",
            "examine_globs = [\n    \"crates/rr-live/src/x.rs\",\n",
            1,
        );
        assert_flags(
            &workflow_problems(&src, &toml_src),
            "paths do not include `crates/rr-live/**`",
        );
    }
    // PR job.
    assert_flags(
        &m(&|s| s.replacen(PR_IF, "if: github.event_name == 'push'", 1)),
        "`pr` job-level `if:`",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "          fetch-depth: 0\n",
                "          fetch-depth: 1\n",
                1,
            )
        }),
        "full history",
    );
    assert_flags(
        &m(&|s| s.replacen("\"$PR_BASE_SHA\"...HEAD", "HEAD~1...HEAD", 1)),
        "`pr` does not run `git diff",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "PR_BASE_SHA: ${{ github.event.pull_request.base.sha }}",
                "PR_BASE_SHA: ${{ github.event.before }}",
                1,
            )
        }),
        "base commit",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "            cargo mutants --jobs 2 --output .\n",
                "            true\n",
                1,
            )
        }),
        "on scope `full`",
    );
    assert_flags(
        &m(&|s| s.replacen("if [ \"$scope\" = full ]", "if [ \"$scope\" = all ]", 1)),
        "on scope `full`",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "scope=$(cargo xtask mutants-scope --diff pr.diff)",
                "scope=diff",
                1,
            )
        }),
        "does not run `cargo xtask mutants-scope",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "--output . --in-diff pr.diff",
                "--output . --in-diff pr.diff --file x.rs",
                1,
            )
        }),
        "with `--file`",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "          name: mutants-report-pr\n",
                "          name: report\n",
                1,
            )
        }),
        "artifact `mutants-report-pr`",
    );
    // Full job.
    assert_flags(
        &m(&|s| s.replacen(FULL_IF, "if: github.event_name == 'workflow_dispatch'", 1)),
        "`full` job-level `if:`",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "        run: cargo mutants --jobs 2 --output .\n",
                "        run: cargo mutants --jobs 2 --output . --shard 0/4\n",
                1,
            )
        }),
        "with `--shard`",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "        run: cargo mutants --jobs 2 --output .\n",
                "        run: cargo mutants --jobs 2 --output . --config=other.toml\n",
                1,
            )
        }),
        "with `--config=other.toml`",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "        run: cargo mutants --jobs 2 --output .\n",
                "        run: cargo mutants --jobs 2 --output . || true\n",
                1,
            )
        }),
        "swallows failures",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "          name: mutants-report-full\n          path: mutants.out\n          if-no-files-found: error\n",
                "          name: mutants-report-full\n          path: mutants.out\n          if-no-files-found: warn\n",
                1,
            )
        }),
        "artifact `mutants-report-full`",
    );
    assert_flags(
        &m(&|s| {
            // The full job's upload only on success.
            let at = s.find("name: mutants-report-full").unwrap_or(0);
            let head = &s[..at];
            let i = head.rfind("        if: always()\n").unwrap_or(0);
            format!(
                "{}        if: success()\n{}",
                &s[..i],
                &s[i + "        if: always()\n".len()..]
            )
        }),
        "has a conditional",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "    name: mutants (full)\n",
                "    name: mutants (full)\n    continue-on-error: true\n",
                1,
            )
        }),
        "`full` uses `continue-on-error",
    );
    // Self-test.
    assert_flags(
        &m(&|s| {
            s.replacen(
                "        run: cargo xtask mutants-selftest\n",
                "        run: echo skip\n",
                1,
            )
        }),
        "`selftest` does not run `cargo xtask mutants-selftest`",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "    name: mutants-selftest\n",
                "    name: mutants-selftest\n    if: github.event_name == 'schedule'\n",
                1,
            )
        }),
        "`selftest` job-level `if:`",
    );
    assert_flags(
        &m(&|s| {
            let at = s.find("  selftest:").unwrap_or(0);
            format!(
                "{}{}",
                &s[..at],
                s[at..].replacen(
                    "tool: cargo-nextest,cargo-mutants",
                    "tool: cargo-nextest",
                    1
                )
            )
        }),
        "`selftest` does not install cargo-mutants",
    );
}
