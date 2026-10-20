//! `corpus-cold.yml` wiring for story US-RR-11, task US-RR-11-4: a cold
//! fetch of the external stock-ROM corpus (D-025) is measured against the CI
//! time budget every week, so the budget stays checked while ci.yml's
//! `corpus` job only ever sees a warm cache.
//!
//! [`cold_problems`] is a pure check over the text of
//! `.github/workflows/corpus-cold.yml` (given ci.yml and the documented
//! budget); one test runs it on the committed files and the `*_rejects_*`
//! test proves each assertion bites on a mutated copy. It asserts that:
//!
//! * the workflow runs on one weekly `schedule` cron and on
//!   `workflow_dispatch`, with `permissions: contents: read` only;
//! * job `cold-fetch` runs on Linux, has a job timeout and no job-level
//!   `if:`, and no step anywhere uses `actions/cache` (nothing is restored,
//!   nothing saved: every run is cold);
//! * it builds xtask (`cargo build -p xtask --locked`) before the timed step;
//! * step `Fresh corpus directory` removes `$RUNNER_TEMP/rr-corpus-cold` and
//!   points `RR_CORPUS_DIR` at it, before the fetch;
//! * step `cargo xtask fetch-corpus` (id `fetch`) runs ci.yml's `corpus`
//!   fetch step script byte for byte (so `corpus_ci_config.rs`'s behaviour
//!   tests of that script, cold, warm, repair, failing and over budget, hold
//!   here too), with `CACHE_HIT: "false"`, and the same `timeout-minutes` and
//!   `BUDGET_SECONDS` as ci.yml, which are the budget documented in
//!   `.project/INFRASTRUCTURE.md`;
//! * step `The fetch was cold`, after it, is given the fetch step's `fetch`
//!   output and fails unless it is `cold`;
//! * nothing points fetch-corpus at another source (`RR_CORPUS_SOURCE`,
//!   `--source`), uses secrets, or pushes anywhere (read-only towards the
//!   upstream), and no step is conditional, `continue-on-error` or swallows a
//!   failure;
//! * INFRASTRUCTURE.md lists the workflow.
//!
//! On Unix, the `Fresh corpus directory` and `The fetch was cold` scripts are
//! also run under bash: a leftover directory is removed and `RR_CORPUS_DIR`
//! set; `cold` passes, `warm` and a missing output fail.
//!
//! Whether a scheduled run is green, and how long a real cold fetch takes on
//! a hosted runner, can only be seen on GitHub.

#[path = "common/workflow.rs"]
mod workflow;

use std::fs;
use std::path::Path;

use workflow::{
    Job, Workflow, assert_flags, ci_yml, indent, is_blank_or_comment, key_at_any, normalize,
    workflow_yml,
};

const FILE: &str = "corpus-cold.yml";
const JOB: &str = "cold-fetch";
const CI_JOB: &str = "corpus";
const BUILD_STEP: &str = "Build xtask";
const BUILD_CMD: &str = "cargo build -p xtask --locked";
const DIR_STEP: &str = "Fresh corpus directory";
const DIR_SCRIPT: &str = "dir=\"$RUNNER_TEMP/rr-corpus-cold\"\nrm -rf \"$dir\"\necho \"RR_CORPUS_DIR=$dir\" >> \"$GITHUB_ENV\"\n";
const FETCH_STEP: &str = "cargo xtask fetch-corpus";
const FETCH_ID: &str = "fetch";
const COLD_STEP: &str = "The fetch was cold";
const COLD_CHECK: &str = "if [ \"$FETCH\" != cold ]; then";
const INFRA_DOC: &str = "../.project/INFRASTRUCTURE.md";
const BUDGET_PHRASE: &str = "Cold-fetch budget:";

fn infra_doc() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(INFRA_DOC);
    fs::read_to_string(&path)
        .map(|s| s.replace("\r\n", "\n"))
        .unwrap_or_else(|e| format!("<unreadable {}: {e}>", path.display()))
}

/// The whole number before ` minutes` after [`BUDGET_PHRASE`] in `doc`
/// (same reading as `corpus_ci_config.rs`).
fn documented_budget_minutes(doc: &str) -> Option<u64> {
    let line = doc.lines().find(|l| l.contains(BUDGET_PHRASE))?;
    let rest = &line[line.find(BUDGET_PHRASE)? + BUDGET_PHRASE.len()..];
    let rest = rest.trim_start_matches(|c: char| !c.is_ascii_digit());
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    rest[digits.len()..]
        .starts_with(" minutes")
        .then(|| digits.parse().ok())
        .flatten()
}

/// Lines of trigger `trigger` under `on:` (empty when absent).
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

/// Whether `cron` fires once a week: a fixed minute and hour on one day of
/// the week, every month.
fn is_weekly_cron(cron: &str) -> bool {
    let num = |f: &str, max: u32| f.parse::<u32>().is_ok_and(|n| n <= max);
    match cron.split_whitespace().collect::<Vec<_>>().as_slice() {
        [min, hour, "*", "*", dow] => num(min, 59) && num(hour, 23) && num(dow, 6),
        _ => false,
    }
}

fn env_of(step: &Job<'_>, key: &str) -> Option<String> {
    step.map_at("env", 8)
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
}

/// The fetch step of ci.yml's `corpus` job: its script, `timeout-minutes`
/// and `BUDGET_SECONDS`.
fn ci_fetch(ci_src: &str) -> Option<(String, Option<String>, Option<String>)> {
    let wf = Workflow::parse(ci_src);
    let job = wf.job(CI_JOB)?;
    let steps = job.steps();
    let s = steps.iter().find(|s| s.id == FETCH_STEP)?;
    Some((
        s.run_block()?,
        s.value_at("timeout-minutes", 8),
        env_of(s, "BUDGET_SECONDS"),
    ))
}

/// Every way `src` (corpus-cold.yml) fails the US-RR-11-4 wiring, given
/// ci.yml (`ci_src`), the documented budget `doc_minutes` and the
/// INFRASTRUCTURE.md text `doc`; empty when it holds.
fn cold_problems(src: &str, ci_src: &str, doc_minutes: Option<u64>, doc: &str) -> Vec<String> {
    let wf = Workflow::parse(src);
    let mut problems = Vec::new();

    for t in ["schedule", "workflow_dispatch"] {
        if key_at_any(wf.on_block(), t).is_none() {
            problems.push(format!("workflow does not run on `{t}`"));
        }
    }
    let crons: Vec<String> = trigger_body(&wf, "schedule")
        .iter()
        .filter_map(|l| l.trim_start().strip_prefix("- cron:"))
        .map(|c| c.trim().trim_matches('"').to_owned())
        .collect();
    match crons.as_slice() {
        [] => problems.push("`schedule` has no `cron` (weekly cold fetch)".to_owned()),
        [c] if !is_weekly_cron(c) => problems.push(format!(
            "`schedule` cron `{c}` is not once a week (`M H * * D`)"
        )),
        [_] => {}
        _ => problems.push(format!(
            "`schedule` has more than one cron, not one weekly run: {crons:?}"
        )),
    }
    let perms: Vec<String> = wf
        .section("permissions")
        .iter()
        .filter(|l| !is_blank_or_comment(l))
        .map(|l| normalize(l))
        .collect();
    if perms != ["contents: read"] {
        problems.push(format!(
            "`permissions` is not exactly `contents: read`: {perms:?}"
        ));
    }

    // Read-only towards the upstream, at the committed pin, no secrets.
    for line in src.lines().filter(|l| !is_blank_or_comment(l)) {
        for bad in ["RR_CORPUS_SOURCE", "--source", "secrets.", "git push"] {
            if line.contains(bad) {
                problems.push(format!("workflow uses `{bad}`: {}", line.trim()));
            }
        }
        if line.split_whitespace().any(|w| w == "gh") {
            problems.push(format!("workflow runs `gh`: {}", line.trim()));
        }
        let t = line.trim_start().trim_start_matches("- ");
        if t.starts_with("uses: actions/cache") {
            problems.push(format!(
                "workflow uses the Actions cache (`{}`): the fetch would not be cold",
                t.trim()
            ));
        }
    }

    let Some(job) = wf.job(JOB) else {
        problems.push(format!("no `{JOB}` job"));
        return problems;
    };
    if !job
        .value_at("runs-on", 4)
        .is_some_and(|r| r.starts_with("ubuntu-"))
    {
        problems.push(format!("`{JOB}` does not run on an ubuntu- runner"));
    }
    if job.value_at("timeout-minutes", 4).is_none() {
        problems.push(format!("`{JOB}` has no job timeout-minutes"));
    }
    if let Some(cond) = job.value_at("if", 4) {
        problems.push(format!(
            "`{JOB}` has a job-level conditional (`if: {cond}`)"
        ));
    }

    let steps = job.steps();
    let find = |name: &str| steps.iter().position(|s| s.id == name);
    let (build, dir, fetch, cold) = (
        find(BUILD_STEP),
        find(DIR_STEP),
        find(FETCH_STEP),
        find(COLD_STEP),
    );
    for (name, pos) in [
        (BUILD_STEP, build),
        (DIR_STEP, dir),
        (FETCH_STEP, fetch),
        (COLD_STEP, cold),
    ] {
        if pos.is_none() {
            problems.push(format!("`{JOB}` has no step `{name}`"));
        }
    }
    for (a, b, what) in [
        (
            build,
            fetch,
            "xtask is not built before the timed fetch step",
        ),
        (
            dir,
            fetch,
            "RR_CORPUS_DIR is not made fresh before the fetch",
        ),
        (fetch, cold, "the cold check runs before the fetch"),
    ] {
        if let (Some(a), Some(b)) = (a, b)
            && a >= b
        {
            problems.push(format!("`{JOB}`: {what}"));
        }
    }

    if let Some(s) = build.map(|i| &steps[i])
        && !s.run_scripts().iter().any(|r| r == BUILD_CMD)
    {
        problems.push(format!(
            "`{JOB}` step `{BUILD_STEP}` does not run `{BUILD_CMD}`"
        ));
    }
    if let Some(s) = dir.map(|i| &steps[i])
        && s.run_block().as_deref() != Some(DIR_SCRIPT)
    {
        problems.push(format!(
            "`{JOB}` step `{DIR_STEP}` does not remove and set a fresh RR_CORPUS_DIR, exactly {DIR_SCRIPT:?} (got {:?})",
            s.run_block()
        ));
    }

    if let Some(s) = fetch.map(|i| &steps[i]) {
        if s.value_at("id", 8).as_deref() != Some(FETCH_ID) {
            problems.push(format!(
                "`{JOB}` step `{FETCH_STEP}` does not have id `{FETCH_ID}`"
            ));
        }
        if env_of(s, "CACHE_HIT").as_deref() != Some("false") {
            problems.push(format!(
                "`{JOB}` fetch step is not told CACHE_HIT: \"false\" (got {:?})",
                env_of(s, "CACHE_HIT")
            ));
        }
        let timeout = s.value_at("timeout-minutes", 8);
        let budget = env_of(s, "BUDGET_SECONDS");
        match ci_fetch(ci_src) {
            None => problems.push(format!(
                "ci.yml job `{CI_JOB}` has no `{FETCH_STEP}` step with a `run: |` script to match"
            )),
            Some((script, ci_timeout, ci_budget)) => {
                if s.run_block().as_deref() != Some(script.as_str()) {
                    problems.push(format!(
                        "`{JOB}` fetch step script is not ci.yml's `{CI_JOB}` fetch step script byte for byte"
                    ));
                }
                if timeout.is_none() || timeout != ci_timeout {
                    problems.push(format!(
                        "`{JOB}` fetch step timeout-minutes {timeout:?} is not ci.yml's {ci_timeout:?}"
                    ));
                }
                if budget.is_none() || budget != ci_budget {
                    problems.push(format!(
                        "`{JOB}` fetch step BUDGET_SECONDS {budget:?} is not ci.yml's {ci_budget:?}"
                    ));
                }
            }
        }
        let t = timeout.and_then(|t| t.parse::<u64>().ok());
        let b = budget.and_then(|b| b.parse::<u64>().ok());
        if let (Some(t), Some(b)) = (t, b)
            && t * 60 != b
        {
            problems.push(format!(
                "`{JOB}` fetch step's timeout-minutes ({t}) and BUDGET_SECONDS ({b}) disagree"
            ));
        }
        match (t, doc_minutes) {
            (_, None) => problems.push(format!(
                "{INFRA_DOC} documents no `{BUDGET_PHRASE} N minutes`"
            )),
            (Some(t), Some(d)) if t != d => problems.push(format!(
                "`{JOB}` fetch step's timeout-minutes ({t}) is not the documented budget ({d} minutes)"
            )),
            _ => {}
        }
    }

    if let Some(s) = cold.map(|i| &steps[i]) {
        let want = format!("${{{{ steps.{FETCH_ID}.outputs.fetch }}}}");
        if env_of(s, "FETCH").as_deref() != Some(want.as_str()) {
            problems.push(format!(
                "`{JOB}` step `{COLD_STEP}` is not given the fetch output (FETCH: {want})"
            ));
        }
        let script = s.run_block().unwrap_or_default();
        if !script.contains(COLD_CHECK) || !script.contains("exit 1") {
            problems.push(format!(
                "`{JOB}` step `{COLD_STEP}` does not fail unless the fetch was cold (`{COLD_CHECK}` ... `exit 1`)"
            ));
        }
    }

    problems.extend(workflow::skippable_problems(job, &[]));

    if !doc.contains(&format!("`{FILE}`")) {
        problems.push(format!("{INFRA_DOC} does not list `{FILE}`"));
    }
    problems
}

fn committed() -> Vec<String> {
    cold_problems(
        &workflow_yml(FILE),
        &ci_yml(),
        documented_budget_minutes(&infra_doc()),
        &infra_doc(),
    )
}

#[test]
fn corpus_cold_fetch_runs_weekly_without_cache_within_the_budget() {
    let problems = committed();
    assert!(
        problems.is_empty(),
        ".github/workflows/{FILE} violates US-RR-11-4:\n{}",
        problems.join("\n")
    );
}

#[test]
fn corpus_cold_check_rejects_weakened_workflow() {
    let src = workflow_yml(FILE);
    let ci = ci_yml();
    let doc = infra_doc();
    let minutes = documented_budget_minutes(&doc);
    let m = |from: &str, to: &str| {
        let changed = src.replacen(from, to, 1);
        assert_ne!(changed, src, "mutation did not apply: {from:?}");
        cold_problems(&changed, &ci, minutes, &doc)
    };

    // Triggers and permissions.
    assert_flags(
        &m("  schedule:\n", "  push:\n"),
        "does not run on `schedule`",
    );
    assert_flags(
        &m(
            "    - cron: \"29 4 * * 3\"\n",
            "    - cron: \"29 4 * * *\"\n",
        ),
        "not once a week",
    );
    assert_flags(
        &m("  workflow_dispatch:\n", ""),
        "does not run on `workflow_dispatch`",
    );
    assert_flags(
        &m("  contents: read\n", "  contents: write\n"),
        "not exactly `contents: read`",
    );
    // A cache anywhere makes the fetch warm.
    assert_flags(
        &m(
            "      - name: Fresh corpus directory\n",
            "      - uses: actions/cache@v4\n        with:\n          path: ${{ env.RR_CORPUS_DIR }}\n          key: rr-corpus-cold\n      - name: Fresh corpus directory\n",
        ),
        "uses the Actions cache",
    );
    // Not fresh: a leftover directory would be verified, not fetched.
    assert_flags(
        &m("          rm -rf \"$dir\"\n", ""),
        "does not remove and set a fresh RR_CORPUS_DIR",
    );
    assert_flags(
        &m(
            "          dir=\"$RUNNER_TEMP/rr-corpus-cold\"\n",
            "          dir=\"$HOME/.cache/romraider-rs/corpus\"\n",
        ),
        "does not remove and set a fresh RR_CORPUS_DIR",
    );
    // Another source, a secret, a push.
    assert_flags(
        &m(
            "          BUDGET_SECONDS: \"300\"\n",
            "          BUDGET_SECONDS: \"300\"\n          RR_CORPUS_SOURCE: /tmp/mirror.git\n",
        ),
        "uses `RR_CORPUS_SOURCE`",
    );
    assert_flags(
        &m(
            "          cargo xtask fetch-corpus 2>&1 | tee \"$log\"\n",
            "          cargo xtask fetch-corpus --source /tmp/mirror.git 2>&1 | tee \"$log\"\n",
        ),
        "uses `--source`",
    );
    assert_flags(
        &m(
            "          CACHE_HIT: \"false\"\n",
            "          CACHE_HIT: \"false\"\n          TOKEN: ${{ secrets.GITHUB_TOKEN }}\n",
        ),
        "uses `secrets.`",
    );
    assert_flags(
        &m(
            "          echo \"Cold fetch of the whole corpus: ${SECONDS_TAKEN}s\"\n",
            "          echo \"Cold fetch of the whole corpus: ${SECONDS_TAKEN}s\"\n          gh issue create --title cold\n",
        ),
        "runs `gh`",
    );
    // The timed script drifts from ci.yml's.
    assert_flags(
        &m(
            "if [ \"$ms\" -gt $((BUDGET_SECONDS * 1000)) ]; then",
            "if false; then",
        ),
        "not ci.yml's `corpus` fetch step script byte for byte",
    );
    // Budget.
    assert_flags(
        &m(
            "        timeout-minutes: 5\n",
            "        timeout-minutes: 15\n",
        ),
        "is not ci.yml's",
    );
    assert_flags(
        &m("        timeout-minutes: 5\n", ""),
        "timeout-minutes None is not ci.yml's",
    );
    assert_flags(
        &m(
            "          BUDGET_SECONDS: \"300\"\n",
            "          BUDGET_SECONDS: \"900\"\n",
        ),
        "BUDGET_SECONDS Some(\"900\") is not ci.yml's",
    );
    assert_flags(
        &m(
            "          CACHE_HIT: \"false\"\n",
            "          CACHE_HIT: \"true\"\n",
        ),
        "not told CACHE_HIT: \"false\"",
    );
    assert_flags(&m("        id: fetch\n", ""), "does not have id `fetch`");
    // Order.
    assert_flags(
        &m(
            "      - name: Build xtask\n        run: cargo build -p xtask --locked\n",
            "",
        ),
        "no step `Build xtask`",
    );
    // The cold check.
    assert_flags(
        &m(
            "          FETCH: ${{ steps.fetch.outputs.fetch }}\n",
            "          FETCH: cold\n",
        ),
        "is not given the fetch output",
    );
    assert_flags(
        &m(COLD_CHECK, "if false; then"),
        "does not fail unless the fetch was cold",
    );
    assert_flags(
        &m(
            "      - name: The fetch was cold\n",
            "      - name: Report\n",
        ),
        "no step `The fetch was cold`",
    );
    // Softened.
    assert_flags(
        &m(
            "      - name: The fetch was cold\n",
            "      - name: The fetch was cold\n        continue-on-error: true\n",
        ),
        "uses `continue-on-error",
    );
    assert_flags(
        &m(
            "      - name: The fetch was cold\n",
            "      - name: The fetch was cold\n        if: github.event_name == 'schedule'\n",
        ),
        "has a conditional",
    );
    assert_flags(
        &m("    timeout-minutes: 30\n", ""),
        "no job timeout-minutes",
    );
    assert_flags(
        &m(
            "    runs-on: ubuntu-24.04\n",
            "    runs-on: ubuntu-24.04\n    if: false\n",
        ),
        "job-level conditional",
    );
    assert_flags(
        &m("  cold-fetch:\n", "  warm-fetch:\n"),
        "no `cold-fetch` job",
    );

    // ci.yml's budget changes but this copy does not: flagged.
    let ci_changed = ci.replacen("BUDGET_SECONDS: \"300\"", "BUDGET_SECONDS: \"600\"", 1);
    assert_ne!(ci_changed, ci);
    assert_flags(
        &cold_problems(&src, &ci_changed, minutes, &doc),
        "is not ci.yml's",
    );
    // The documented budget changes: flagged.
    assert_flags(
        &cold_problems(&src, &ci, Some(10), &doc),
        "is not the documented budget (10 minutes)",
    );
    assert_flags(
        &cold_problems(&src, &ci, minutes, "no listing"),
        "does not list `corpus-cold.yml`",
    );
}

#[cfg(unix)]
mod behaviour {
    //! The workflow's own (non-fetch) scripts, run under bash.

    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::workflow::{Workflow, workflow_yml};
    use super::{COLD_STEP, DIR_STEP, FILE, JOB};

    pub(super) fn script(src: &str, step: &str) -> String {
        let wf = Workflow::parse(src);
        wf.job(JOB)
            .and_then(|j| j.steps().into_iter().find(|s| s.id == step))
            .and_then(|s| s.run_block())
            .unwrap_or_default()
    }

    pub(super) fn committed(step: &str) -> String {
        script(&workflow_yml(FILE), step)
    }

    fn scratch(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "rr-corpus-cold-ci-config-{}-{tag}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::create_dir_all(&root);
        root
    }

    #[expect(
        clippy::unwrap_used,
        reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
    )]
    fn bash(
        script: &str,
        cwd: &Path,
        env: &[(&str, &Path)],
        vars: &[(&str, &str)],
    ) -> (bool, String) {
        let mut cmd = Command::new("bash");
        cmd.args(["--noprofile", "--norc", "-eo", "pipefail", "-c", script])
            .current_dir(cwd)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default());
        for (k, v) in env {
            cmd.env(k, v);
        }
        for (k, v) in vars {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        (
            out.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )
    }

    /// Every way the `Fresh corpus directory` script misbehaves: run with a
    /// leftover corpus directory, it must remove it and set RR_CORPUS_DIR.
    pub(super) fn dir_problems(script: &str, tag: &str) -> Vec<String> {
        let root = scratch(tag);
        let temp = root.join("runner-temp");
        let leftover = temp.join("rr-corpus-cold");
        let _ = fs::create_dir_all(&leftover);
        let _ = fs::write(leftover.join("stale.hex"), b"stale");
        let env_file = root.join("github-env");
        let (ok, log) = bash(
            script,
            &root,
            &[("RUNNER_TEMP", &temp), ("GITHUB_ENV", &env_file)],
            &[],
        );
        let mut problems = Vec::new();
        if !ok {
            problems.push(format!("{DIR_STEP}: the step failed:\n{log}"));
        }
        if leftover.exists() {
            problems.push(format!(
                "{DIR_STEP}: the leftover directory was not removed"
            ));
        }
        let env = fs::read_to_string(&env_file).unwrap_or_default();
        let want = format!("RR_CORPUS_DIR={}\n", leftover.display());
        if env != want {
            problems.push(format!("{DIR_STEP}: GITHUB_ENV is {env:?}, not {want:?}"));
        }
        let _ = fs::remove_dir_all(&root);
        problems
    }

    /// Every way the `The fetch was cold` script misbehaves: `cold` passes,
    /// `warm` and a missing output fail with an error annotation.
    pub(super) fn cold_problems(script: &str, tag: &str) -> Vec<String> {
        let root = scratch(tag);
        let mut problems = Vec::new();
        for (fetch, want_ok) in [("cold", true), ("warm", false), ("", false)] {
            let (ok, log) = bash(
                script,
                &root,
                &[],
                &[("FETCH", fetch), ("SECONDS_TAKEN", "12.345")],
            );
            if ok != want_ok {
                problems.push(format!(
                    "{COLD_STEP}: FETCH={fetch:?}: the step {} (log:\n{log})",
                    if ok { "passed" } else { "failed" }
                ));
            }
            if want_ok && !log.contains("12.345s") {
                problems.push(format!(
                    "{COLD_STEP}: the elapsed time is not logged: {log}"
                ));
            }
            if !want_ok && !log.contains("::error::expected a cold fetch") {
                problems.push(format!(
                    "{COLD_STEP}: FETCH={fetch:?}: no ::error:: ({log})"
                ));
            }
        }
        let _ = fs::remove_dir_all(&root);
        problems
    }
}

/// The committed `Fresh corpus directory` and `The fetch was cold` scripts
/// behave as documented under bash.
#[cfg(unix)]
#[test]
fn corpus_cold_scripts_make_a_fresh_dir_and_fail_unless_cold() {
    let dir = behaviour::committed(DIR_STEP);
    let cold = behaviour::committed(COLD_STEP);
    assert!(
        !dir.is_empty() && !cold.is_empty(),
        "scripts missing in {FILE}"
    );
    let mut problems = behaviour::dir_problems(&dir, "dir");
    problems.extend(behaviour::cold_problems(&cold, "cold"));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The bash checks bite on weakened scripts.
#[cfg(unix)]
#[test]
fn corpus_cold_script_checks_reject_weakened_scripts() {
    let dir = behaviour::committed(DIR_STEP);
    let cold = behaviour::committed(COLD_STEP);
    let edit = |s: &str, from: &str, to: &str| {
        let changed = s.replacen(from, to, 1);
        assert_ne!(changed, s, "mutation did not apply: {from:?}");
        changed
    };
    assert_flags(
        &behaviour::dir_problems(&edit(&dir, "rm -rf \"$dir\"\n", ""), "dir-keep"),
        "leftover directory was not removed",
    );
    assert_flags(
        &behaviour::dir_problems(
            &edit(&dir, ">> \"$GITHUB_ENV\"", "> /dev/null"),
            "dir-noenv",
        ),
        "GITHUB_ENV is",
    );
    assert_flags(
        &behaviour::cold_problems(&edit(&cold, COLD_CHECK, "if false; then"), "cold-off"),
        "FETCH=\"warm\": the step passed",
    );
    assert_flags(
        &behaviour::cold_problems(
            &edit(&cold, "[ \"$FETCH\" != cold ]", "[ \"$FETCH\" = warm ]"),
            "cold-warm-only",
        ),
        "FETCH=\"\": the step passed",
    );
}
