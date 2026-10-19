//! Shared checks for a workflow whose gate jobs run only when the event is
//! in scope, with an always-running aggregate job as the required status
//! check (task US-RR-4-8: `mutants.yml` / `mutants-ok`, `fixtures.yml` /
//! `fixtures-ok`). Used by `mutants_ci_config.rs` and
//! `fixtures_ci_config.rs`; builds on `workflow.rs`, which the including
//! test binary must declare as `mod workflow`.
//!
//! A path-filtered workflow never starts on an unrelated pull request, so a
//! required check from it would wait at "Expected" forever. Instead the
//! workflow starts on every event; job [`SCOPE_JOB`] works out whether the
//! event is in scope (`git diff` of the event's changes against a `paths`
//! list in its script) and publishes output [`OUTPUT`]; each gate job runs
//! only `if:` it is `'true'`; and the aggregate job runs `if: always()` and
//! decides from the results:
//!
//! * `scope` not `success`, or no `true`/`false` answer: fail (fail closed);
//! * any gate job `failure` or `cancelled`: fail;
//! * a gate job `skipped` although it should have run (in scope, and on an
//!   event it runs on): fail;
//! * otherwise (out of scope, or every expected job passed): pass.
//!
//! The text checks ([`scope_job_problems`], [`aggregate_problems`],
//! [`gated_job_problems`]) run everywhere. The behaviour checks
//! ([`aggregate_behaviour_problems`], [`scope_behaviour_problems`]) run the
//! scripts themselves with bash, the way GitHub's `shell: bash` does
//! (`bash --noprofile --norc -eo pipefail`), so they are Unix-only; CI's
//! Linux test jobs run them.

#![allow(dead_code, reason = "shared by several test binaries")]

use std::collections::BTreeSet;

use crate::workflow::{Job, Workflow, normalize, skippable_problems};

/// The job that decides whether the event is in scope.
pub const SCOPE_JOB: &str = "scope";
/// The `id:` of the step in [`SCOPE_JOB`] that writes [`OUTPUT`].
pub const SCOPE_STEP: &str = "scope";
/// The output of [`SCOPE_JOB`]: `true` or `false`.
pub const OUTPUT: &str = "in_scope";
/// The job-level condition of every gate job (GitHub adds the implicit
/// `success()`, so a failed `scope` skips them too).
pub const IN_SCOPE_IF: &str = "needs.scope.outputs.in_scope == 'true'";

/// The events the workflows run on.
pub const EVENTS: [&str; 4] = ["pull_request", "push", "schedule", "workflow_dispatch"];

/// A gate job the aggregate job checks.
pub struct Gated {
    /// Job id.
    pub job: &'static str,
    /// The aggregate step's env var carrying `needs.<job>.result`.
    pub var: &'static str,
    /// Whether the job runs on `event` when the event is in scope.
    pub runs_on: fn(&str) -> bool,
}

/// The job-level `if:` of `job`, normalized and without `if: `.
pub fn job_if(job: &Job<'_>) -> Option<String> {
    job.lines
        .iter()
        .find(|l| crate::workflow::indent(l) == 4 && l.trim_start().starts_with("if:"))
        .map(|l| normalize(l.trim_start().trim_start_matches("if:")))
}

fn step_with_id<'a>(job: &Job<'a>, id: &str) -> Option<Job<'a>> {
    job.steps()
        .into_iter()
        .find(|s| s.value_at("id", 8).as_deref() == Some(id))
}

/// The `paths=( ... )` items of the scope step's script, unquoted.
pub fn scope_paths(wf: &Workflow<'_>) -> Vec<String> {
    let script = wf
        .job(SCOPE_JOB)
        .and_then(|j| step_with_id(j, SCOPE_STEP))
        .and_then(|s| s.run_block())
        .unwrap_or_default();
    script
        .lines()
        .skip_while(|l| l.trim() != "paths=(")
        .skip(1)
        .take_while(|l| l.trim() != ")")
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.trim_matches(|c| c == '"' || c == '\'').to_owned())
        .collect()
}

/// Problems if trigger `trigger` has a trigger-level path filter, which
/// would keep the aggregate check from ever reporting on other changes.
pub fn trigger_filter_problems(wf: &Workflow<'_>, trigger: &str) -> Vec<String> {
    let on = wf.on_block();
    let Some(start) = on
        .iter()
        .position(|l| crate::workflow::indent(l) == 2 && l.trim() == format!("{trigger}:"))
    else {
        return vec![format!("workflow does not run on `{trigger}`")];
    };
    on[start + 1..]
        .iter()
        .take_while(|l| crate::workflow::is_blank_or_comment(l) || crate::workflow::indent(l) > 2)
        .filter(|l| !crate::workflow::is_blank_or_comment(l))
        .map(|l| l.trim())
        .filter(|l| l.starts_with("paths:") || l.starts_with("paths-ignore:"))
        .map(|l| {
            format!(
                "`{trigger}` has a trigger-level `{l}` filter: the workflow (and its required check) never starts on other changes"
            )
        })
        .collect()
}

/// Problems with job [`SCOPE_JOB`]: unconditional, full-history checkout,
/// a step `id: scope` with the `env` entries in `want_env` and a `paths`
/// list, its output wired to the job's [`OUTPUT`], nothing softened.
pub fn scope_job_problems(wf: &Workflow<'_>, want_env: &[(&str, &str)]) -> Vec<String> {
    let Some(job) = wf.job(SCOPE_JOB) else {
        return vec![format!("no `{SCOPE_JOB}` job")];
    };
    let mut problems = Vec::new();
    if let Some(cond) = job_if(job) {
        problems.push(format!(
            "`{SCOPE_JOB}` has a job-level `if: {cond}`: a skipped scope must not happen"
        ));
    }
    let outputs = job.map_at("outputs", 4);
    let want_out = format!("${{{{ steps.{SCOPE_STEP}.outputs.{OUTPUT} }}}}");
    if !outputs
        .iter()
        .any(|(k, v)| k == OUTPUT && normalize(v) == want_out)
    {
        problems.push(format!(
            "`{SCOPE_JOB}` does not output `{OUTPUT}: {want_out}` (outputs: {outputs:?})"
        ));
    }
    let full = job.steps().iter().any(|s| {
        s.id.starts_with("actions/checkout@")
            && s.content_lines().any(|l| normalize(l) == "fetch-depth: 0")
    });
    if !full {
        problems.push(format!(
            "`{SCOPE_JOB}` does not check out the full history (`fetch-depth: 0`)"
        ));
    }
    match step_with_id(job, SCOPE_STEP) {
        None => problems.push(format!("`{SCOPE_JOB}` has no step `id: {SCOPE_STEP}`")),
        Some(step) => {
            let env = step.map_at("env", 8);
            for (k, v) in want_env {
                if !env.iter().any(|(ek, ev)| ek == k && normalize(ev) == *v) {
                    problems.push(format!(
                        "`{SCOPE_JOB}` step `{SCOPE_STEP}` does not set `{k}: {v}` (env: {env:?})"
                    ));
                }
            }
            let script = step.run_block().unwrap_or_default();
            if !script.contains(&format!(
                "echo \"{OUTPUT}=$in_scope\" >> \"$GITHUB_OUTPUT\""
            )) {
                problems.push(format!(
                    "`{SCOPE_JOB}` step `{SCOPE_STEP}` does not write `{OUTPUT}` to $GITHUB_OUTPUT"
                ));
            }
        }
    }
    if scope_paths(wf).is_empty() {
        problems.push(format!(
            "`{SCOPE_JOB}` step `{SCOPE_STEP}` has no `paths=( ... )` list"
        ));
    }
    problems.extend(skippable_problems(job, &[]));
    problems
}

/// Problems if gate job `id` does not wait for [`SCOPE_JOB`] or its
/// job-level `if:` is not `want_if`.
pub fn gated_job_problems(wf: &Workflow<'_>, id: &str, want_if: &str) -> Vec<String> {
    let Some(job) = wf.job(id) else {
        return vec![format!("no `{id}` job")];
    };
    let mut problems = Vec::new();
    if !job.needs().iter().any(|n| n == SCOPE_JOB) {
        problems.push(format!(
            "`{id}.needs` does not include `{SCOPE_JOB}` (needs: {:?})",
            job.needs()
        ));
    }
    let got = job_if(job);
    if got.as_deref() != Some(want_if) {
        problems.push(format!(
            "`{id}` job-level `if:` is {got:?}, expected {want_if:?}"
        ));
    }
    problems
}

/// Problems with the aggregate job `agg`: named `agg` (the check context),
/// `if: always()`, `needs` every other job of the workflow, every gate job
/// listed in `gated`, one step whose `env` passes the scope's result and
/// answer and each gate job's result, nothing softened.
pub fn aggregate_problems(wf: &Workflow<'_>, agg: &str, gated: &[Gated]) -> Vec<String> {
    let Some(job) = wf.job(agg) else {
        return vec![format!("no `{agg}` job")];
    };
    let mut problems = Vec::new();
    if job.value_at("name", 4).as_deref() != Some(agg) {
        problems.push(format!(
            "`{agg}` is not named `{agg}` (`name:` is the required check's context)"
        ));
    }
    if job_if(job).as_deref() != Some("always()") {
        problems.push(format!(
            "`{agg}` is not `if: always()`, so a failed or skipped gate job skips it (a skipped required check counts as passed)"
        ));
    }
    let needs: BTreeSet<String> = job.needs().into_iter().collect();
    let others: BTreeSet<String> = wf
        .jobs
        .iter()
        .map(|j| j.id.to_owned())
        .filter(|j| j != agg)
        .collect();
    for j in others.difference(&needs) {
        problems.push(format!("`{agg}.needs` does not include `{j}`"));
    }
    for j in needs.difference(&others) {
        problems.push(format!("`{agg}.needs` names unknown job `{j}`"));
    }
    for j in others.iter().filter(|j| *j != SCOPE_JOB) {
        if !gated.iter().any(|g| g.job == j) {
            problems.push(format!(
                "job `{j}` is not a gate job `{agg}` is checked for (add it to the test's table)"
            ));
        }
    }
    let steps = job.steps();
    if steps.len() != 1 {
        problems.push(format!(
            "`{agg}` has {} steps, expected one deciding step",
            steps.len()
        ));
    }
    if let Some(step) = steps.first() {
        let env = step.map_at("env", 8);
        let mut want: Vec<(String, String)> = vec![
            ("EVENT".into(), "${{ github.event_name }}".into()),
            (
                "SCOPE".into(),
                format!("${{{{ needs.{SCOPE_JOB}.result }}}}"),
            ),
            (
                "IN_SCOPE".into(),
                format!("${{{{ needs.{SCOPE_JOB}.outputs.{OUTPUT} }}}}"),
            ),
        ];
        want.extend(gated.iter().map(|g| {
            (
                g.var.to_owned(),
                format!("${{{{ needs.{}.result }}}}", g.job),
            )
        }));
        for (k, v) in &want {
            if !env.iter().any(|(ek, ev)| ek == k && normalize(ev) == *v) {
                problems.push(format!(
                    "`{agg}` step does not set `{k}: {v}` (env: {env:?})"
                ));
            }
        }
        if step.run_block().is_none() {
            problems.push(format!("`{agg}` step has no `run: |` script"));
        }
    }
    problems.extend(skippable_problems(job, &[]));
    problems
}

/// The aggregate step's script.
pub fn aggregate_script(wf: &Workflow<'_>, agg: &str) -> Option<String> {
    wf.job(agg)?.steps().first()?.run_block()
}

#[cfg(unix)]
#[cfg(test)]
pub use unix::*;

#[cfg(unix)]
#[cfg(test)]
mod unix {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};

    use super::{EVENTS, Gated, OUTPUT, SCOPE_JOB, SCOPE_STEP, Workflow, scope_paths};

    /// Runs `script` as GitHub's `shell: bash` does, in `dir`, with only
    /// `PATH`, `HOME` and `env` set.
    pub fn run_bash(script: &str, dir: &Path, env: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new("bash");
        cmd.args(["--noprofile", "--norc", "-eo", "pipefail", "-c", script])
            .current_dir(dir)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", dir)
            .env("GIT_CONFIG_NOSYSTEM", "1");
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.output().expect("run bash")
    }

    fn passes(script: &str, env: &[(&str, &str)]) -> bool {
        run_bash(script, Path::new(env!("CARGO_TARGET_TMPDIR")), env)
            .status
            .success()
    }

    /// Every result combination where the aggregate script `script` decides
    /// wrongly. It must fail when `scope` did not succeed or gave no
    /// `true`/`false`, when any gate job failed or was cancelled, and when a
    /// gate job expected to run was skipped; and pass out of scope, when the
    /// expected jobs succeeded, and when a job not expected ran and passed.
    pub fn aggregate_behaviour_problems(script: &str, agg: &str, gated: &[Gated]) -> Vec<String> {
        let mut problems = Vec::new();
        let mut case = |event: &str,
                        scope: &str,
                        in_scope: &str,
                        results: &[(&str, &str)],
                        want_pass: bool| {
            let mut env = vec![("EVENT", event), ("SCOPE", scope), ("IN_SCOPE", in_scope)];
            for g in gated {
                let r = results
                    .iter()
                    .find(|(j, _)| *j == g.job)
                    .map_or("skipped", |(_, r)| *r);
                env.push((g.var, r));
            }
            if passes(script, &env) != want_pass {
                problems.push(format!(
                    "`{agg}` {} when {env:?}",
                    if want_pass { "fails" } else { "passes" }
                ));
            }
        };
        for event in EVENTS {
            for in_scope in [true, false] {
                let flag = if in_scope { "true" } else { "false" };
                let expected = |g: &Gated| in_scope && (g.runs_on)(event);
                let base: Vec<(&str, &str)> = gated
                    .iter()
                    .map(|g| (g.job, if expected(g) { "success" } else { "skipped" }))
                    .collect();
                case(event, "success", flag, &base, true);
                for scope in ["failure", "cancelled", "skipped"] {
                    case(event, scope, flag, &base, false);
                    case(event, scope, "", &base, false);
                }
                for g in gated {
                    let with = |r: &'static str| -> Vec<(&str, &str)> {
                        base.iter()
                            .map(|&(j, old)| (j, if j == g.job { r } else { old }))
                            .collect()
                    };
                    case(event, "success", flag, &with("failure"), false);
                    case(event, "success", flag, &with("cancelled"), false);
                    if expected(g) {
                        case(event, "success", flag, &with("skipped"), false);
                    } else {
                        case(event, "success", flag, &with("success"), true);
                    }
                }
            }
            let skipped: Vec<(&str, &str)> = gated.iter().map(|g| (g.job, "skipped")).collect();
            for answer in ["", "maybe", "TRUE"] {
                case(event, "success", answer, &skipped, false);
            }
        }
        problems
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("scoped-gate")
            .join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(["-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
            .args(args)
            .current_dir(dir)
            .env("HOME", dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// Commits `files` (path, content) on top of HEAD; returns the commit.
    fn commit(dir: &Path, files: &[(&str, &str)]) -> String {
        for (path, content) in files {
            let p = dir.join(path);
            fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
            fs::write(p, content).expect("write");
        }
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "--allow-empty", "-m", "c"]);
        git(dir, &["rev-parse", "HEAD"])
    }

    /// A file a `paths` entry matches, or `None` for a pattern this check
    /// does not model (only `dir/**` and plain files are used).
    fn sample(pattern: &str) -> Option<String> {
        if let Some(dir) = pattern.strip_suffix("/**") {
            (!dir.contains('*')).then(|| format!("{dir}/sample/changed.txt"))
        } else {
            (!pattern.contains('*')).then(|| pattern.to_owned())
        }
    }

    /// What the scope script answered: `Ok("true" | "false")` or `Err` with
    /// its output when it failed or wrote no single answer.
    fn answer(script: &str, dir: &Path, env: &[(&str, &str)]) -> Result<String, String> {
        // Inside .git: never committed by the scenarios' `git add -A`.
        let out_file = dir.join(".git").join("github-output");
        let _ = fs::remove_file(&out_file);
        let out_path = out_file.to_string_lossy().into_owned();
        let mut env = env.to_vec();
        env.push(("GITHUB_OUTPUT", &out_path));
        let out = run_bash(script, dir, &env);
        let log = format!(
            "exit {:?}: {}{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        if !out.status.success() {
            return Err(log);
        }
        let written = fs::read_to_string(&out_file).unwrap_or_default();
        let answers: Vec<&str> = written
            .lines()
            .filter_map(|l| l.strip_prefix(&format!("{OUTPUT}=")))
            .collect();
        match answers.as_slice() {
            [a] => Ok((*a).to_owned()),
            _ => Err(format!("wrote {written:?}; {log}")),
        }
    }

    /// Runs the scope step of `wf` on scratch repositories and returns every
    /// case it gets wrong: a pull request changing any `paths` entry is in
    /// scope, one changing only other files (also when its base moved on
    /// with an in-scope change, and files in a sibling directory with the
    /// same prefix) is not, and a missing or unknown base fails the step; each
    /// event in `always` is in scope; with `push`, a fast-forward push is
    /// compared with the commit before it and any other push (new branch,
    /// force push, unknown or empty `before`) is in scope.
    pub fn scope_behaviour_problems(
        wf: &Workflow<'_>,
        name: &str,
        always: &[&str],
        push: bool,
    ) -> Vec<String> {
        let Some(script) = wf
            .job(SCOPE_JOB)
            .and_then(|j| {
                j.steps()
                    .into_iter()
                    .find(|s| s.value_at("id", 8).as_deref() == Some(SCOPE_STEP))
            })
            .and_then(|s| s.run_block())
        else {
            return vec![format!("no `{SCOPE_JOB}` step `{SCOPE_STEP}` script")];
        };
        let mut problems = Vec::new();
        let mut samples: Vec<String> = Vec::new();
        for p in scope_paths(wf) {
            match sample(&p) {
                Some(s) => samples.push(s),
                None => problems.push(format!(
                    "`paths` entry `{p}` is neither `dir/**` nor a plain file"
                )),
            }
        }
        let dir = scratch(name);
        git(&dir, &["init", "-q", "-b", "main"]);
        let root = commit(&dir, &[("README.md", "root\n")]);
        let mut check = |label: &str, env: &[(&str, &str)], want: Option<&str>| match (
            answer(&script, &dir, env),
            want,
        ) {
            (Ok(got), Some(w)) if got == w => {}
            (Err(_), None) => {}
            (got, want) => problems.push(format!(
                "scope {label} ({env:?}): expected {}, got {got:?}",
                want.map_or_else(|| "a failure".to_owned(), |w| format!("`{w}`"))
            )),
        };
        fn pr(base: &str) -> Vec<(&'static str, &str)> {
            vec![("EVENT", "pull_request"), ("BASE", base)]
        }

        for s in &samples {
            git(&dir, &["checkout", "-q", "--detach", &root]);
            commit(&dir, &[(s, "changed\n")]);
            check(
                &format!("of a pull request changing `{s}`"),
                &pr(&root),
                Some("true"),
            );
        }
        // Out of scope: unrelated files, and same-prefix siblings of `dir/**`.
        git(&dir, &["checkout", "-q", "--detach", &root]);
        let mut outside = vec![("docs/unrelated.md".to_owned(), "x\n")];
        for p in scope_paths(wf) {
            if let Some(d) = p.strip_suffix("/**") {
                outside.push((format!("{d}-sibling/x.txt"), "x\n"));
            }
        }
        let outside: Vec<(&str, &str)> = outside.iter().map(|(p, c)| (p.as_str(), *c)).collect();
        let pr_head = commit(&dir, &outside);
        check(
            "of a pull request changing other files",
            &pr(&root),
            Some("false"),
        );
        // The base moved on with an in-scope change the pull request does
        // not have: the change is measured from the merge-base (`...`), as
        // GitHub's "Files changed" does, so it is still out of scope.
        if let Some(s) = samples.first() {
            git(&dir, &["checkout", "-q", "--detach", &root]);
            let moved = commit(&dir, &[(s, "base moved\n")]);
            git(&dir, &["checkout", "-q", "--detach", &pr_head]);
            check(
                "of a pull request changing other files, base moved on in scope",
                &pr(&moved),
                Some("false"),
            );
            // And as GitHub checks it out: merged into the moved base.
            git(&dir, &["checkout", "-q", "--detach", &moved]);
            git(&dir, &["merge", "-q", "--no-ff", "--no-edit", &pr_head]);
            check(
                "of a pull request changing other files, merged into its moved base",
                &pr(&moved),
                Some("false"),
            );
        }
        check("with no pull request base", &pr(""), None);
        check(
            "with an unknown pull request base",
            &pr("1234567890abcdef1234567890abcdef12345678"),
            None,
        );
        for event in always {
            check(&format!("on `{event}`"), &[("EVENT", event)], Some("true"));
        }
        if push {
            fn ev(before: &str) -> Vec<(&'static str, &str)> {
                vec![("EVENT", "push"), ("BEFORE", before)]
            }
            for s in &samples {
                git(&dir, &["checkout", "-q", "--detach", &root]);
                commit(&dir, &[(s, "pushed\n")]);
                check(
                    &format!("of a push changing `{s}`"),
                    &ev(&root),
                    Some("true"),
                );
            }
            git(&dir, &["checkout", "-q", "--detach", &root]);
            commit(&dir, &outside);
            check("of a push changing other files", &ev(&root), Some("false"));
            check(
                "of a new branch's first push",
                &ev("0000000000000000000000000000000000000000"),
                Some("true"),
            );
            check("of a push with no `before`", &ev(""), Some("true"));
            check(
                "of a push whose `before` is unknown",
                &ev("1234567890abcdef1234567890abcdef12345678"),
                Some("true"),
            );
            // A force push: `before` is not an ancestor of the new head.
            git(&dir, &["checkout", "-q", "--detach", &root]);
            let old = commit(&dir, &[("docs/old.md", "old\n")]);
            git(&dir, &["checkout", "-q", "--detach", &root]);
            commit(&dir, &outside);
            check("of a force push", &ev(&old), Some("true"));
        }
        problems
    }
}
