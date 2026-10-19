//! CI gate wiring for story US-RR-2 (ci.yml: cargo-deny, pending insta
//! snapshots, the i686-pc-windows-msvc test target).
//!
//! Each criterion is a pure check over the text of `.github/workflows/ci.yml`
//! that returns a list of problems. One test runs it on the committed file
//! (so the nextest jobs fail if the wiring regresses), and `*_rejects_*`
//! tests prove each assertion bites on a mutated copy of the real file.
//!
//! * **US-RR-2-2** ([`deny_problems`]): job `deny` runs `cargo deny --locked
//!   check` (every check, no subset) and `cargo xtask deny-selftest` (which
//!   proves that command rejects a GPL-incompatible licence and an open
//!   RustSec advisory, see `xtask/src/deny_selftest.rs`), can't be skipped,
//!   and is in `ci-ok.needs`.
//! * **US-RR-2-3** ([`snapshots_problems`]): job `snapshots` fails on
//!   committed `*.snap.new` / `*.pending-snap` files (`git ls-files`, before
//!   any test runs), runs `cargo insta test --workspace --check
//!   --unreferenced=reject` with `INSTA_UPDATE=no`, finishes with an
//!   `if: always()` check that no pending file was left behind, and is in
//!   `ci-ok.needs`.
//! * **US-RR-2-4** ([`i686_problems`]): the `test` matrix has an
//!   `i686-pc-windows-msvc` entry on a Windows runner, and for that entry the
//!   job installs the target and runs `cargo nextest run --workspace` and the
//!   doctests with `--target i686-pc-windows-msvc`; `test` is in
//!   `ci-ok.needs`. Whether that run is green can only be seen on GitHub.
//!
//! The std-only ci.yml reader is shared with `t6_ci_config.rs`
//! (`common/workflow.rs`).

#[path = "common/workflow.rs"]
mod workflow;

use workflow::{
    Job, Workflow, assert_flags, ci_yml, gate_needs_problems, normalize, skippable_problems,
    trigger_problems,
};

fn words(s: &str) -> Vec<&str> {
    s.split(' ').collect()
}

// --- US-RR-2-2: cargo-deny --------------------------------------------------

const DENY_JOB: &str = "deny";

/// The CI cargo-deny command: every check (no `check licenses` subset).
const DENY_CMD: &str = "cargo deny --locked check";

/// Every way `src` fails the US-RR-2-2 wiring; empty when it holds.
fn deny_problems(src: &str) -> Vec<String> {
    let wf = Workflow::parse(src);
    let mut problems = trigger_problems(&wf);
    problems.extend(gate_needs_problems(&wf, DENY_JOB));
    let Some(job) = wf.job(DENY_JOB) else {
        problems.push(format!("no `{DENY_JOB}` job"));
        return problems;
    };
    let scripts = job.run_scripts();
    if !scripts.iter().any(|s| s == DENY_CMD) {
        problems.push(format!(
            "`{DENY_JOB}` does not run `{DENY_CMD}` (scripts: {scripts:?})"
        ));
    }
    if !scripts.iter().any(|s| s == "cargo xtask deny-selftest") {
        problems.push(format!(
            "`{DENY_JOB}` does not run `cargo xtask deny-selftest`"
        ));
    }
    problems.extend(skippable_problems(job, &[]));
    problems
}

#[test]
fn ci_deny_job_runs_cargo_deny_and_selftest_as_gate() {
    let problems = deny_problems(&ci_yml());
    assert!(
        problems.is_empty(),
        ".github/workflows/ci.yml violates US-RR-2-2:\n{}",
        problems.join("\n")
    );
}

#[test]
fn ci_deny_check_rejects_weakened_deny_job() {
    let m = |edit: &dyn Fn(&str) -> String| workflow::mutated(deny_problems, edit);
    assert_flags(
        &m(&|s| {
            s.replace(
                "run: cargo deny --locked check\n",
                "run: cargo deny --locked check licenses\n",
            )
        }),
        "does not run `cargo deny --locked check`",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "run: cargo deny --locked check\n",
                "run: cargo deny check\n",
            )
        }),
        "does not run `cargo deny --locked check`",
    );
    assert_flags(
        &m(&|s| s.replace("run: cargo xtask deny-selftest", "run: echo skipped")),
        "does not run `cargo xtask deny-selftest`",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: cargo deny check\n",
                "      - name: cargo deny check\n        continue-on-error: true\n",
            )
        }),
        "continue-on-error",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: cargo deny check\n",
                "      - name: cargo deny check\n        if: false\n",
            )
        }),
        "has a conditional",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "run: cargo deny --locked check\n",
                "run: cargo deny --locked check || true\n",
            )
        }),
        "swallows failures",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "needs: [fmt, clippy, test, deny, ",
                "needs: [fmt, clippy, test, ",
            )
        }),
        "`ci-ok.needs` does not include `deny`",
    );
    assert_flags(
        &m(&|s| s.replace("\n  deny:\n", "\n  supply-chain:\n")),
        "no `deny` job",
    );
}

// --- US-RR-2-3: pending insta snapshots -------------------------------------

const SNAP_JOB: &str = "snapshots";

/// Every way `src` fails the US-RR-2-3 wiring; empty when it holds.
fn snapshots_problems(src: &str) -> Vec<String> {
    let wf = Workflow::parse(src);
    let mut problems = trigger_problems(&wf);
    problems.extend(gate_needs_problems(&wf, SNAP_JOB));

    // INSTA_UPDATE=no at workflow level (every job) or on the job itself.
    let insta_no = |lines: &[&str]| {
        lines
            .iter()
            .any(|l| normalize(l).replace(['"', '\''], "") == "INSTA_UPDATE: no")
    };
    let Some(job) = wf.job(SNAP_JOB) else {
        problems.push(format!("no `{SNAP_JOB}` job"));
        return problems;
    };
    if !insta_no(wf.section("env")) && !insta_no(&job.lines) {
        problems.push("`INSTA_UPDATE: \"no\"` is not set for the snapshots job".to_owned());
    }

    let steps = job.steps();
    let script_of = |step: &Job<'_>| step.run_scripts().join(" ");
    let fails = |s: &str| s.contains("exit 1");
    let names_pending = |s: &str| s.contains("'*.snap.new'") && s.contains("'*.pending-snap'");

    let committed = steps.iter().position(|st| {
        let s = script_of(st);
        s.contains("git ls-files") && names_pending(&s) && fails(&s)
    });
    let insta = steps.iter().position(|st| {
        st.run_scripts().iter().any(|s| {
            let w = words(s);
            s.starts_with("cargo insta test")
                && ["--workspace", "--check", "--unreferenced=reject"]
                    .iter()
                    .all(|f| w.contains(f))
                && !["--accept", "--review", "--force-update-snapshots"]
                    .iter()
                    .any(|f| w.contains(f))
        })
    });
    let left_behind = steps.iter().position(|st| {
        let s = script_of(st);
        s.contains("find ")
            && s.contains("'*.snap.new'")
            && s.contains("'*.pending-snap'")
            && fails(&s)
    });

    match (committed, insta) {
        (None, _) => problems.push(format!(
            "`{SNAP_JOB}` has no `git ls-files` step failing on committed '*.snap.new' / '*.pending-snap'"
        )),
        (Some(c), Some(i)) if c > i => problems.push(format!(
            "`{SNAP_JOB}` checks for committed pending snapshots after the tests ran (insta deletes them)"
        )),
        _ => {}
    }
    if insta.is_none() {
        problems.push(format!(
            "`{SNAP_JOB}` does not run `cargo insta test --workspace --check --unreferenced=reject`"
        ));
    }
    match left_behind {
        None => problems.push(format!(
            "`{SNAP_JOB}` has no final `find` step failing on left-behind pending snapshots"
        )),
        Some(f) => {
            let always = steps[f]
                .content_lines()
                .any(|l| normalize(l) == "if: always()");
            if !always {
                problems.push(format!(
                    "`{SNAP_JOB}` left-behind check is not `if: always()`"
                ));
            }
            if insta.is_some_and(|i| f < i) {
                problems.push(format!(
                    "`{SNAP_JOB}` left-behind check runs before the tests"
                ));
            }
        }
    }
    let allowed: Vec<&str> = left_behind.map(|f| steps[f].id).into_iter().collect();
    problems.extend(skippable_problems(job, &allowed));
    problems
}

#[test]
fn ci_snapshots_job_fails_on_pending_snapshots() {
    let problems = snapshots_problems(&ci_yml());
    assert!(
        problems.is_empty(),
        ".github/workflows/ci.yml violates US-RR-2-3:\n{}",
        problems.join("\n")
    );
}

#[test]
fn ci_snapshots_check_rejects_weakened_snapshots_job() {
    let m = |edit: &dyn Fn(&str) -> String| workflow::mutated(snapshots_problems, edit);
    assert_flags(
        &m(&|s| {
            s.replace(
                "pending=$(git ls-files -- '*.snap.new' '*.pending-snap')",
                "pending=",
            )
        }),
        "no `git ls-files` step",
    );
    assert_flags(
        &m(&|s| s.replacen(" --unreferenced=reject", "", 1)),
        "does not run `cargo insta test",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "cargo insta test --workspace --check",
                "cargo insta test --workspace --accept",
                1,
            )
        }),
        "does not run `cargo insta test",
    );
    assert_flags(
        &m(&|s| s.replace("  INSTA_UPDATE: \"no\"\n", "")),
        "INSTA_UPDATE",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: No pending snapshots left behind\n        if: always()\n",
                "      - name: No pending snapshots left behind\n",
            )
        }),
        "is not `if: always()`",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "left=$(find . -path ./target -prune -o \\( -name '*.snap.new' -o -name '*.pending-snap' \\) -print)",
                "left=",
            )
        }),
        "no final `find` step",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: No pending snapshots committed\n",
                "      - name: No pending snapshots committed\n        if: false\n",
            )
        }),
        "has a conditional",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "needs: [fmt, clippy, test, deny, t6, snapshots]",
                "needs: [fmt, clippy, test, deny, t6]",
            )
        }),
        "`ci-ok.needs` does not include `snapshots`",
    );
}

/// Moving the committed-pending check after the tests is caught (insta would
/// already have deleted a committed stale `.snap.new`).
#[test]
fn ci_snapshots_check_rejects_committed_check_after_tests() {
    let src = ci_yml();
    let start = src
        .find("      - name: No pending snapshots committed\n")
        .unwrap_or(0);
    let end = src[start..]
        .find("      - name: Install toolchain\n")
        .map_or(start, |e| start + e);
    let block = &src[start..end];
    assert!(!block.is_empty(), "committed-pending step not found");
    let anchor = "      - name: No pending snapshots left behind\n";
    let moved = src
        .replacen(block, "", 1)
        .replacen(anchor, &format!("{block}{anchor}"), 1);
    assert_ne!(moved, src);
    assert_flags(&snapshots_problems(&moved), "after the tests ran");
}

// --- US-RR-2-4: i686-pc-windows-msvc -----------------------------------------

const TEST_JOB: &str = "test";
const I686: &str = "i686-pc-windows-msvc";

/// The `strategy.matrix.include` entries of a job, as key/value lists.
fn matrix_include(job: &Job<'_>) -> Vec<Vec<(String, String)>> {
    let Some(start) = job.lines.iter().position(|l| l.trim() == "include:") else {
        return Vec::new();
    };
    let base = workflow::indent(job.lines[start]);
    let mut entries: Vec<Vec<(String, String)>> = Vec::new();
    for &line in &job.lines[start + 1..] {
        if workflow::is_blank_or_comment(line) {
            continue;
        }
        if workflow::indent(line) <= base {
            break;
        }
        let t = line.trim();
        let t = if let Some(rest) = t.strip_prefix("- ") {
            entries.push(Vec::new());
            rest
        } else {
            t
        };
        if let (Some(entry), Some((k, v))) = (entries.last_mut(), t.split_once(':')) {
            entry.push((
                k.trim().to_owned(),
                v.trim().trim_matches(|c| c == '"' || c == '\'').to_owned(),
            ));
        }
    }
    entries
}

/// `text` with `${{ matrix.<key> }}` replaced from `entry`.
fn expand(text: &str, entry: &[(String, String)]) -> String {
    entry.iter().fold(text.to_owned(), |t, (k, v)| {
        t.replace(&format!("${{{{ matrix.{k} }}}}"), v)
    })
}

/// A predicate over one normalized, matrix-expanded `run:` script.
type StepPred<'a> = dyn Fn(&str) -> bool + 'a;

/// Every way `src` fails the US-RR-2-4 wiring; empty when it holds.
fn i686_problems(src: &str) -> Vec<String> {
    let wf = Workflow::parse(src);
    let mut problems = trigger_problems(&wf);
    problems.extend(gate_needs_problems(&wf, TEST_JOB));
    let Some(job) = wf.job(TEST_JOB) else {
        problems.push(format!("no `{TEST_JOB}` job"));
        return problems;
    };
    let include = matrix_include(job);
    let Some(entry) = include
        .iter()
        .find(|e| e.iter().any(|(k, v)| k == "target" && v == I686))
    else {
        problems.push(format!("`{TEST_JOB}` matrix has no `target: {I686}` entry"));
        return problems;
    };
    if job.lines.iter().any(|l| l.trim() == "exclude:") {
        problems.push(format!("`{TEST_JOB}` matrix has an `exclude:` list"));
    }
    let runs_on = job
        .value_at("runs-on", 4)
        .map(|r| expand(&r, entry))
        .unwrap_or_default();
    if !runs_on.starts_with("windows-") {
        problems.push(format!(
            "`{TEST_JOB}` ({I686}) does not run on a Windows runner (runs-on: {runs_on:?})"
        ));
    }
    // Each required command must be in a step that always runs: other steps
    // of the job (e.g. host-only self-tests) may carry their own `if:`.
    let steps = job.steps();
    let targets_i686 = |w: &[&str]| w.windows(2).any(|p| p == ["--target", I686]);
    let add_target = format!("rustup target add {I686}");
    let required: [(&str, &StepPred<'_>); 3] = [
        (&format!("`rustup target add {I686}`"), &|s: &str| {
            s.contains(&add_target)
        }),
        (
            &format!("`cargo nextest run --workspace --target {I686}`"),
            &|s: &str| {
                let w = words(s);
                w.starts_with(&["cargo", "nextest", "run"])
                    && w.contains(&"--workspace")
                    && targets_i686(&w)
            },
        ),
        (
            &format!("the doctests with `--target {I686}`"),
            &|s: &str| {
                let w = words(s);
                w.starts_with(&["cargo", "test", "--doc"])
                    && w.contains(&"--workspace")
                    && targets_i686(&w)
            },
        ),
    ];
    for (what, pred) in required {
        let step = steps
            .iter()
            .find(|st| st.run_scripts().iter().any(|s| pred(&expand(s, entry))));
        match step {
            None => problems.push(format!("`{TEST_JOB}` ({I686}) does not run {what}")),
            Some(st) => {
                for l in st.content_lines() {
                    let l = l.strip_prefix("- ").unwrap_or(l);
                    if l.starts_with("if:") || l.starts_with("continue-on-error") {
                        problems.push(format!(
                            "`{TEST_JOB}` step `{}` (runs {what}) has `{l}`",
                            st.id
                        ));
                    }
                }
                for s in st.run_scripts() {
                    if s.contains("|| true") || s.contains("|| :") || s.contains("exit 0") {
                        problems.push(format!("`{TEST_JOB}` step swallows failures: {s}"));
                    }
                }
            }
        }
    }
    if job
        .content_lines()
        .any(|l| l.starts_with("continue-on-error"))
    {
        problems.push(format!("`{TEST_JOB}` uses continue-on-error"));
    }
    if job.value_at("if", 4).is_some() {
        problems.push(format!("`{TEST_JOB}` job has a job-level `if:`"));
    }
    problems
}

#[test]
fn ci_test_matrix_builds_and_tests_i686_windows() {
    let problems = i686_problems(&ci_yml());
    assert!(
        problems.is_empty(),
        ".github/workflows/ci.yml violates US-RR-2-4:\n{}",
        problems.join("\n")
    );
}

#[test]
fn ci_i686_check_rejects_weakened_matrix() {
    let m = |edit: &dyn Fn(&str) -> String| workflow::mutated(i686_problems, edit);
    assert_flags(
        &m(&|s| {
            s.replace(
                "- target: i686-pc-windows-msvc",
                "- target: x86_64-pc-windows-gnu",
            )
        }),
        "no `target: i686-pc-windows-msvc` entry",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "          - target: i686-pc-windows-msvc\n            os: windows-2022\n",
                "          - target: i686-pc-windows-msvc\n            os: ubuntu-24.04\n",
            )
        }),
        "does not run on a Windows runner",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "cargo nextest run --workspace --locked --target ${{ matrix.target }}",
                "cargo nextest run --workspace --locked",
            )
        }),
        "does not run `cargo nextest run --workspace --target",
    );
    assert_flags(
        &m(&|s| s.replace("rustup target add ${{ matrix.target }}", "true")),
        "does not run `rustup target add",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: cargo nextest\n",
                "      - name: cargo nextest\n        if: matrix.target != 'i686-pc-windows-msvc'\n",
            )
        }),
        "has `if: matrix.target",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: doctests\n",
                "      - name: doctests\n        continue-on-error: true\n",
            )
        }),
        "has `continue-on-error",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "    needs: [fmt, clippy, test, ",
                "    needs: [fmt, clippy, ",
            )
        }),
        "`ci-ok.needs` does not include `test`",
    );
}

#[test]
fn matrix_reader_reads_include_entries() {
    let src = "jobs:\n  test:\n    strategy:\n      matrix:\n        include:\n          - target: a\n            os: x\n          # c\n          - os: \"y\"\n            target: b\n    steps:\n      - run: echo\n";
    let wf = Workflow::parse(src);
    let inc = wf.job("test").map(matrix_include).unwrap_or_default();
    let kv = |k: &str, v: &str| (k.to_owned(), v.to_owned());
    assert_eq!(
        inc,
        vec![
            vec![kv("target", "a"), kv("os", "x")],
            vec![kv("os", "y"), kv("target", "b")]
        ]
    );
    assert_eq!(
        expand("run on ${{ matrix.os }} for ${{ matrix.target }}", &inc[1]),
        "run on y for b"
    );
}
