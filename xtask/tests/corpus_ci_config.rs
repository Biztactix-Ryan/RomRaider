//! `ci.yml` wiring for story US-RR-11, task US-RR-11-8: CI caches the
//! external stock-ROM corpus (D-025) keyed by the manifest's hash, and logs
//! how long `cargo xtask fetch-corpus` took against the CI time budget.
//!
//! [`corpus_problems`] is a pure check over the text of
//! `.github/workflows/ci.yml`; one test runs it on the committed file and
//! the `*_rejects_*` test proves each assertion bites on a mutated copy. It
//! asserts that job `corpus`:
//!
//! * exists, runs on Linux, has a job timeout, has no job-level `if:`, and is
//!   in `ci-ok.needs` (the one required check); the workflow runs on `push`
//!   and `pull_request`;
//! * builds xtask in a step of its own before the timed fetch;
//! * sets `RR_CORPUS_DIR` (via `$GITHUB_ENV`) to fetch-corpus's own default,
//!   `$HOME/.cache/romraider-rs/corpus` ([`CACHE_SUBDIR`]), outside the
//!   workspace;
//! * restores that path with `actions/cache/restore`, keyed by
//!   `hashFiles(<the committed manifest>)` behind an OS and schema-version
//!   prefix, with no `restore-keys`;
//! * then runs plain `cargo xtask fetch-corpus` (no `--verify-only`, and no
//!   `--dir` / `--source` / `--manifest` that would bypass `RR_CORPUS_DIR`
//!   or the committed pin), told the restore's `cache-hit`, under a step
//!   `timeout-minutes` equal to the budget it logs (`BUDGET_SECONDS`) and to
//!   the cold-fetch budget documented in `.project/INFRASTRUCTURE.md`;
//! * then saves the same path under the restore's own primary key, only when
//!   the restore missed (never `always()`: only a verified corpus is cached);
//! * has no other conditional, no `continue-on-error` and no script that
//!   swallows a failure.
//!
//! For task US-RR-11-1 it also asserts, after the cache is saved:
//!
//! * a step `Verify all corpus images` runs plain
//!   `cargo xtask fetch-corpus --verify-only` once and fails unless its
//!   output says `ok: N/N image(s)` with `N` = `CORPUS_IMAGES`, which must be
//!   501 ([`CORPUS_IMAGES`], D-025) and the committed manifest's entry count;
//! * cargo-nextest is installed, then a step runs every corpus-tier test
//!   (`*::requires_RR_CORPUS_DIR`) with `--run-ignored all` and
//!   `--no-tests=fail`, so none can be skipped ([`TESTS_CMD`]).
//!
//! For task US-RR-16-6, after the cache is saved, a step
//! `Corpus coverage report` runs exactly [`COVERAGE_CMD`] (plain `--check`:
//! no `--dir`, `--manifest` or `--coverage` that would bypass
//! `RR_CORPUS_DIR`, the committed manifest or the committed report), so CI
//! fails when `fixtures/corpus-coverage.toml` is stale.
//!
//! For task US-RR-16-2 (the definitions are fetched and hash-verified
//! alongside the ROMs, D-028), [`defs_env_problems`] asserts that nothing
//! in the workflow sets `RR_CORPUS_SOURCE`, `RR_DEFS_SOURCE` or
//! `RR_LOGGER_ZIP`, so CI's plain fetch reads the committed manifest's
//! public upstreams; and [`committed_defs_problems`] asserts that the
//! committed manifest (the one that plain fetch reads) has a `[defs]` table
//! at the pinned commit of the pinned upstream, so that fetch fetches and
//! verifies the definitions too.
//!
//! [`verify_step_behaviour_problems`] (Unix: bash) runs that verify script
//! the same way as below: a verified cache passes, a short count, a flipped
//! byte and a truncated file each fail, the last listing both paths, and a
//! flipped byte in a definition file fails, naming it.
//!
//! [`fetch_step_behaviour_problems`] (Unix: bash) runs the fetch step's
//! script as GitHub would, with `cargo` on `PATH` forwarding to this
//! package's real xtask binary, against a synthetic source repository and
//! manifest in the system temp directory (never real ROM images, never inside
//! the repository): a cold fetch, a warm cache, a restored cache that does
//! not verify, a failing fetch, a fetch that exits 0 without its ok line and
//! an over-budget run must each be logged (job log, step summary, step
//! outputs) or fail as documented. The synthetic manifest has a `[defs]`
//! table and a synthetic definitions source too, so a cold fetch must also
//! fetch the definitions, and a definitions cache that does not verify (and
//! cannot be refetched) must fail the step although the ROMs verify.
//!
//! Whether the job is green, and how long a real cold fetch takes on a
//! hosted runner, can only be seen on GitHub. With a warm cache this job no
//! longer fetches cold, so `corpus-cold.yml` re-measures a cold fetch weekly
//! with this step's script (US-RR-11-4, `corpus_cold_ci_config.rs`).

#[path = "common/workflow.rs"]
mod workflow;

use std::fs;
use std::path::Path;

use workflow::{
    Job, Workflow, assert_flags, ci_yml, gate_needs_problems, indent, is_blank_or_comment,
    normalize, trigger_problems,
};
use xtask::corpus_manifest::{COMMITTED_PATH, DEFS_PIN_COMMIT, DEFS_UPSTREAM_URL, Manifest};
use xtask::fetch_corpus::{CACHE_SUBDIR, DEFS_SOURCE_ENV, DIR_ENV, LOGGER_ZIP_ENV, SOURCE_ENV};

const JOB: &str = "corpus";
const BUILD_STEP: &str = "Build xtask";
const BUILD_CMD: &str = "cargo build -p xtask --locked";
const DIR_STEP: &str = "Corpus directory";
const RESTORE_STEP: &str = "Restore corpus cache";
const RESTORE_ID: &str = "corpus-cache";
const FETCH_STEP: &str = "cargo xtask fetch-corpus";
const FETCH_CMD: &str = "cargo xtask fetch-corpus";
const SAVE_STEP: &str = "Save corpus cache";
/// The cached path: whatever the directory step put in `RR_CORPUS_DIR`.
const CACHE_PATH: &str = "${{ env.RR_CORPUS_DIR }}";
/// Bumped by hand to drop every cached corpus (a layout change).
const KEY_PREFIX: &str = "rr-corpus-v1-${{ runner.os }}-";
const INFRA_DOC: &str = "../.project/INFRASTRUCTURE.md";
/// The phrase that introduces the documented budget in [`INFRA_DOC`].
const BUDGET_PHRASE: &str = "Cold-fetch budget:";
/// US-RR-11-1: the step proving the real corpus verifies in full.
const VERIFY_STEP: &str = "Verify all corpus images";
const VERIFY_CMD: &str = "cargo xtask fetch-corpus --verify-only";
/// Images in the corpus (D-025): the verify step's `CORPUS_IMAGES`.
const CORPUS_IMAGES: usize = 501;
/// The verify step's check of the count in fetch-corpus's ok line.
const VERIFY_CHECK: &str = "if ! grep -q \"^fetch-corpus: ok: ${CORPUS_IMAGES}/${CORPUS_IMAGES} image(s), \" \"$log\"; then";
/// US-RR-16-6: the step checking the committed coverage report.
const COVERAGE_STEP: &str = "Corpus coverage report";
const COVERAGE_CMD: &str = "cargo xtask gen-corpus-coverage --check";
const TESTS_STEP: &str = "Corpus-tier tests (cargo nextest)";
/// Every corpus-tier test, forced to run (an ignored one fails in
/// `require_tier`), and an empty selection fails.
const TESTS_CMD: &str = "cargo nextest run --workspace --locked --run-ignored all --no-tests=fail -E 'test(/::requires_RR_CORPUS_DIR$/)'";

fn cache_key() -> String {
    format!("{KEY_PREFIX}${{{{ hashFiles('{COMMITTED_PATH}') }}}}")
}

fn dir_cmd() -> String {
    format!(
        "echo \"{DIR_ENV}=$HOME/.cache/{}\" >> \"$GITHUB_ENV\"",
        CACHE_SUBDIR.join("/")
    )
}

fn save_if() -> String {
    format!("steps.{RESTORE_ID}.outputs.cache-hit != 'true'")
}

fn uses_action(step: &Job<'_>, action: &str) -> bool {
    step.value_at("uses", 8)
        .is_some_and(|u| u.starts_with(&format!("{action}@")))
}

fn with(step: &Job<'_>, key: &str) -> Option<String> {
    step.value_at(key, 10)
}

/// The step's `if:` exactly as written (quotes kept), whitespace-normalized.
fn step_if(step: &Job<'_>) -> Option<String> {
    step.lines.iter().find_map(|l| {
        let t = match indent(l) {
            8 => l.trim_start(),
            6 => l.trim_start().strip_prefix("- ")?,
            _ => return None,
        };
        t.strip_prefix("if:").map(normalize)
    })
}

/// The first whole number before ` minutes` on the [`BUDGET_PHRASE`] line of
/// `doc`.
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

fn infra_doc() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(INFRA_DOC);
    fs::read_to_string(&path)
        .map(|s| s.replace("\r\n", "\n"))
        .unwrap_or_else(|e| format!("<unreadable {}: {e}>", path.display()))
}

/// Every way `src` (ci.yml) fails the US-RR-11-8 wiring, given the documented
/// budget `doc_minutes`; empty when it holds.
fn corpus_problems(src: &str, doc_minutes: Option<u64>) -> Vec<String> {
    let wf = Workflow::parse(src);
    let mut problems = trigger_problems(&wf);
    problems.extend(gate_needs_problems(&wf, JOB));
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
    let (build, dir, restore, fetch, save) = (
        find(BUILD_STEP),
        find(DIR_STEP),
        find(RESTORE_STEP),
        find(FETCH_STEP),
        find(SAVE_STEP),
    );
    for (name, pos) in [
        (BUILD_STEP, build),
        (DIR_STEP, dir),
        (RESTORE_STEP, restore),
        (FETCH_STEP, fetch),
        (SAVE_STEP, save),
    ] {
        if pos.is_none() {
            problems.push(format!("`{JOB}` has no step `{name}`"));
        }
    }
    let before = |a: Option<usize>, b: Option<usize>, what: &str, problems: &mut Vec<String>| {
        if let (Some(a), Some(b)) = (a, b)
            && a >= b
        {
            problems.push(format!("`{JOB}`: {what}"));
        }
    };
    before(
        build,
        fetch,
        "xtask is not built before the timed fetch step",
        &mut problems,
    );
    before(
        dir,
        restore,
        "the cache is restored before RR_CORPUS_DIR is set",
        &mut problems,
    );
    before(
        restore,
        fetch,
        "the cache is restored after the fetch step",
        &mut problems,
    );
    before(
        fetch,
        save,
        "the cache is saved before the fetch step verified it",
        &mut problems,
    );

    if let Some(s) = build.map(|i| &steps[i])
        && !s.run_scripts().iter().any(|r| r == BUILD_CMD)
    {
        problems.push(format!(
            "`{JOB}` step `{BUILD_STEP}` does not run `{BUILD_CMD}`"
        ));
    }

    if let Some(s) = dir.map(|i| &steps[i])
        && s.run_scripts() != [dir_cmd()]
    {
        problems.push(format!(
            "`{JOB}` step `{DIR_STEP}` does not set {DIR_ENV} to fetch-corpus's default, exactly `{}` (scripts: {:?})",
            dir_cmd(),
            s.run_scripts()
        ));
    }

    if let Some(s) = restore.map(|i| &steps[i]) {
        if !uses_action(s, "actions/cache/restore") {
            problems.push(format!(
                "`{JOB}` step `{RESTORE_STEP}` does not use actions/cache/restore"
            ));
        }
        if s.value_at("id", 8).as_deref() != Some(RESTORE_ID) {
            problems.push(format!(
                "`{JOB}` step `{RESTORE_STEP}` does not have id `{RESTORE_ID}`"
            ));
        }
        if with(s, "path").as_deref() != Some(CACHE_PATH) {
            problems.push(format!(
                "`{JOB}` restores {:?}, not RR_CORPUS_DIR (`{CACHE_PATH}`)",
                with(s, "path")
            ));
        }
        let key = with(s, "key");
        if key.as_deref() != Some(cache_key().as_str()) {
            problems.push(format!(
                "`{JOB}` cache key is {key:?}, not the manifest hash `{}`",
                cache_key()
            ));
        }
        if with(s, "restore-keys").is_some() {
            problems.push(format!(
                "`{JOB}` has restore-keys: a corpus for another manifest never verifies"
            ));
        }
    }

    if let Some(s) = fetch.map(|i| &steps[i]) {
        let script = s.run_block().unwrap_or_default();
        let calls: Vec<&str> = script
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with('#') && l.contains(FETCH_CMD))
            .collect();
        match calls.as_slice() {
            [call] => {
                // The words after the command, up to a pipe or a redirection.
                let args: Vec<&str> = call
                    .split_once(FETCH_CMD)
                    .map_or("", |(_, rest)| rest)
                    .split_whitespace()
                    .take_while(|w| *w != "|" && !w.contains('>'))
                    .collect();
                if !args.is_empty() {
                    problems.push(format!(
                        "`{JOB}` passes arguments to fetch-corpus ({}): it must fetch the committed pin into RR_CORPUS_DIR, and repair a bad cache",
                        args.join(" ")
                    ));
                }
            }
            _ => problems.push(format!(
                "`{JOB}` step `{FETCH_STEP}` does not run `{FETCH_CMD}` exactly once (block `run: |`)"
            )),
        }
        if !script.contains(">> \"$GITHUB_STEP_SUMMARY\"") {
            problems.push(format!(
                "`{JOB}` does not log the fetch timing to the step summary"
            ));
        }
        let env = s.map_at("env", 8);
        let get = |k: &str| env.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
        let hit = format!("${{{{ steps.{RESTORE_ID}.outputs.cache-hit }}}}");
        if get("CACHE_HIT").as_deref() != Some(hit.as_str()) {
            problems.push(format!(
                "`{JOB}` fetch step is not told the cache hit (CACHE_HIT: {hit})"
            ));
        }
        let timeout = s
            .value_at("timeout-minutes", 8)
            .and_then(|v| v.parse::<u64>().ok());
        let budget = get("BUDGET_SECONDS").and_then(|v| v.parse::<u64>().ok());
        match (timeout, budget) {
            (None, _) => problems.push(format!(
                "`{JOB}` fetch step has no timeout-minutes (the CI time budget)"
            )),
            (_, None) => problems.push(format!(
                "`{JOB}` fetch step has no BUDGET_SECONDS for the timing log"
            )),
            (Some(t), Some(b)) if t * 60 != b => problems.push(format!(
                "`{JOB}` fetch step's timeout-minutes ({t}) and BUDGET_SECONDS ({b}) disagree"
            )),
            _ => {}
        }
        match (timeout, doc_minutes) {
            (_, None) => problems.push(format!(
                "{INFRA_DOC} documents no `{BUDGET_PHRASE} N minutes`"
            )),
            (Some(t), Some(d)) if t != d => problems.push(format!(
                "`{JOB}` fetch step's timeout-minutes ({t}) is not the documented budget ({d} minutes)"
            )),
            _ => {}
        }
    }

    if let Some(s) = save.map(|i| &steps[i]) {
        if !uses_action(s, "actions/cache/save") {
            problems.push(format!(
                "`{JOB}` step `{SAVE_STEP}` does not use actions/cache/save"
            ));
        }
        if with(s, "path").as_deref() != Some(CACHE_PATH) {
            problems.push(format!(
                "`{JOB}` saves {:?}, not RR_CORPUS_DIR (`{CACHE_PATH}`)",
                with(s, "path")
            ));
        }
        let key = format!("${{{{ steps.{RESTORE_ID}.outputs.cache-primary-key }}}}");
        if with(s, "key").as_deref() != Some(key.as_str()) {
            problems.push(format!(
                "`{JOB}` saves under {:?}, not the restore's primary key `{key}`",
                with(s, "key")
            ));
        }
        if step_if(s) != Some(save_if()) {
            problems.push(format!(
                "`{JOB}` saves the cache with {:?}, not only on a miss after a verified fetch (`if: {}`)",
                step_if(s),
                save_if()
            ));
        }
    }

    problems.extend(verification_problems(&steps, fetch, save));
    problems.extend(defs_env_problems(src));

    for (i, s) in steps.iter().enumerate() {
        if Some(i) != save && step_if(s).is_some() {
            problems.push(format!(
                "`{JOB}` step `{}` has a conditional ({:?})",
                s.id,
                step_if(s)
            ));
        }
    }
    for line in job.lines.iter().filter(|l| !is_blank_or_comment(l)) {
        if line
            .trim_start()
            .trim_start_matches("- ")
            .starts_with("continue-on-error")
        {
            problems.push(format!("`{JOB}` uses `{}`", line.trim()));
        }
    }
    for s in job.run_scripts() {
        if s.contains("|| true") || s.contains("|| :") || s.contains("exit 0") {
            problems.push(format!("`{JOB}` step swallows failures: {s}"));
        }
    }
    problems
}

/// US-RR-16-2: every line of the workflow `src` (comments aside) that sets
/// or names a variable redirecting fetch-corpus away from the committed
/// manifest's public sources: the ROM source, the definitions source, or a
/// logger package (which CI must never have, D-028).
fn defs_env_problems(src: &str) -> Vec<String> {
    src.lines()
        .filter(|l| !is_blank_or_comment(l))
        .filter_map(|l| {
            [SOURCE_ENV, DEFS_SOURCE_ENV, LOGGER_ZIP_ENV]
                .into_iter()
                .find(|v| l.contains(v))
                .map(|v| {
                    format!(
                        "ci.yml sets or names {v} (`{}`): CI's fetch must read the committed manifest's public sources",
                        l.trim()
                    )
                })
        })
        .collect()
}

/// US-RR-16-2: why the committed manifest text `text` would not make CI's
/// plain `cargo xtask fetch-corpus` fetch and verify the pinned definitions;
/// empty when it would.
fn committed_defs_problems(text: &str) -> Vec<String> {
    let m = match Manifest::from_toml(text) {
        Ok(m) => m,
        Err(e) => return vec![format!("{COMMITTED_PATH} does not parse: {e:#}")],
    };
    let Some(defs) = m.defs else {
        return vec![format!(
            "{COMMITTED_PATH} has no [defs]: CI's fetch would fetch no definitions"
        )];
    };
    let mut problems = Vec::new();
    if defs.upstream != DEFS_UPSTREAM_URL {
        problems.push(format!(
            "{COMMITTED_PATH} [defs] upstream is {:?}, not the pinned {DEFS_UPSTREAM_URL}",
            defs.upstream
        ));
    }
    if defs.commit != DEFS_PIN_COMMIT {
        problems.push(format!(
            "{COMMITTED_PATH} [defs] commit is {}, not the pinned {DEFS_PIN_COMMIT}",
            defs.commit
        ));
    }
    if defs.files.is_empty() {
        problems.push(format!(
            "{COMMITTED_PATH} [defs] lists no files: CI's fetch would verify nothing"
        ));
    }
    problems
}

fn committed_manifest_text() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(COMMITTED_PATH);
    fs::read_to_string(path).unwrap_or_default()
}

/// The committed manifest's entry count, or `None` when it cannot be read.
fn committed_manifest_len() -> Option<usize> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(COMMITTED_PATH);
    let text = fs::read_to_string(path).ok()?;
    Manifest::from_toml(&text).ok().map(|m| m.roms.files.len())
}

/// The US-RR-11-1 part of [`corpus_problems`] (module docs), over the
/// `corpus` job's `steps` with the fetch and save steps at `fetch`, `save`.
fn verification_problems(
    steps: &[Job<'_>],
    fetch: Option<usize>,
    save: Option<usize>,
) -> Vec<String> {
    let mut problems = Vec::new();
    let find = |name: &str| steps.iter().position(|s| s.id == name);
    let after = |a: Option<usize>, b: Option<usize>| match (a, b) {
        (Some(a), Some(b)) => a > b,
        _ => true,
    };

    let verify = find(VERIFY_STEP);
    match verify.map(|i| (i, &steps[i])) {
        None => problems.push(format!("`{JOB}` has no step `{VERIFY_STEP}`")),
        Some((i, s)) => {
            if !after(Some(i), fetch) || !after(Some(i), save) {
                problems.push(format!(
                    "`{JOB}`: the corpus is verified before it is fetched and saved"
                ));
            }
            let env = s.map_at("env", 8);
            let images = env
                .iter()
                .find(|(k, _)| k == "CORPUS_IMAGES")
                .map(|(_, v)| v.clone());
            if images != Some(CORPUS_IMAGES.to_string()) {
                problems.push(format!(
                    "`{JOB}` step `{VERIFY_STEP}` has CORPUS_IMAGES {images:?}, not the {CORPUS_IMAGES} images of D-025"
                ));
            }
            let manifest = committed_manifest_len();
            if manifest != Some(CORPUS_IMAGES) {
                problems.push(format!(
                    "{COMMITTED_PATH} has {manifest:?} entries, not the {CORPUS_IMAGES} of D-025"
                ));
            }
            let script = s.run_block().unwrap_or_default();
            let calls: Vec<&str> = script
                .lines()
                .map(str::trim)
                .filter(|l| !l.starts_with('#') && l.contains(FETCH_CMD))
                .collect();
            let plain = match calls.as_slice() {
                [call] => call.strip_prefix(VERIFY_CMD).is_some_and(|rest| {
                    rest.split_whitespace()
                        .take_while(|w| *w != "|" && !w.contains('>'))
                        .next()
                        .is_none()
                }),
                _ => false,
            };
            if !plain {
                problems.push(format!(
                    "`{JOB}` step `{VERIFY_STEP}` does not run plain `{VERIFY_CMD}` exactly once (calls: {calls:?})"
                ));
            }
            if !script.lines().any(|l| l.trim() == VERIFY_CHECK) || !script.contains("exit 1") {
                problems.push(format!(
                    "`{JOB}` step `{VERIFY_STEP}` does not fail unless all CORPUS_IMAGES verify (`{VERIFY_CHECK}` ... `exit 1`)"
                ));
            }
        }
    }

    match find(COVERAGE_STEP).map(|i| (i, &steps[i])) {
        None => problems.push(format!("`{JOB}` has no step `{COVERAGE_STEP}`")),
        Some((i, s)) => {
            if !after(Some(i), fetch) || !after(Some(i), save) {
                problems.push(format!(
                    "`{JOB}`: the coverage report is checked before the corpus is fetched and saved"
                ));
            }
            if s.run_scripts() != [COVERAGE_CMD] {
                problems.push(format!(
                    "`{JOB}` step `{COVERAGE_STEP}` does not run exactly `{COVERAGE_CMD}` (scripts: {:?})",
                    s.run_scripts()
                ));
            }
        }
    }

    let tests = find(TESTS_STEP);
    let nextest = steps.iter().position(|s| {
        uses_action(s, "taiki-e/install-action")
            && with(s, "tool").is_some_and(|t| t.split(',').any(|t| t.trim() == "cargo-nextest"))
    });
    match tests.map(|i| (i, &steps[i])) {
        None => problems.push(format!("`{JOB}` has no step `{TESTS_STEP}`")),
        Some((i, s)) => {
            if !after(Some(i), fetch) {
                problems.push(format!(
                    "`{JOB}`: the corpus-tier tests run before the corpus is fetched"
                ));
            }
            if nextest.is_none_or(|n| n > i) {
                problems.push(format!(
                    "`{JOB}` installs no cargo-nextest (taiki-e/install-action) before `{TESTS_STEP}`"
                ));
            }
            if s.run_scripts() != [TESTS_CMD] {
                problems.push(format!(
                    "`{JOB}` step `{TESTS_STEP}` does not run exactly `{TESTS_CMD}` (scripts: {:?})",
                    s.run_scripts()
                ));
            }
        }
    }
    problems
}

#[test]
fn ci_caches_corpus_by_manifest_hash_and_times_the_fetch() {
    let problems = corpus_problems(&ci_yml(), documented_budget_minutes(&infra_doc()));
    assert!(
        problems.is_empty(),
        ".github/workflows/ci.yml violates US-RR-11-8:\n{}",
        problems.join("\n")
    );
}

#[test]
fn infrastructure_doc_states_the_cold_fetch_budget() {
    assert_eq!(documented_budget_minutes(&infra_doc()), Some(5));
    assert_eq!(
        documented_budget_minutes("- **Cold-fetch budget: 12 minutes** per job"),
        Some(12)
    );
    assert_eq!(documented_budget_minutes("Cold-fetch budget: soon"), None);
    assert_eq!(
        documented_budget_minutes("Cold-fetch budget: 5 seconds"),
        None
    );
    assert_eq!(documented_budget_minutes("no budget here"), None);
}

#[test]
fn ci_corpus_check_rejects_weakened_job() {
    let doc = documented_budget_minutes(&infra_doc());
    let m = |edit: &dyn Fn(&str) -> String| workflow::mutated(|s| corpus_problems(s, doc), edit);
    let key_line = format!("          key: {}\n", cache_key());

    assert_flags(
        &m(&|s| s.replace(", corpus, ", ", ")),
        "`ci-ok.needs` does not include `corpus`",
    );
    assert_flags(
        &m(&|s| s.replace("\n  corpus:\n", "\n  stock-roms:\n")),
        "no `corpus` job",
    );
    // The key must be the manifest's hash.
    assert_flags(
        &m(&|s| s.replace(&key_line, "          key: rr-corpus-v1-${{ runner.os }}\n")),
        "not the manifest hash",
    );
    assert_flags(
        &m(&|s| s.replace(&key_line, &key_line.replace(COMMITTED_PATH, "Cargo.lock"))),
        "not the manifest hash",
    );
    assert_flags(
        &m(&|s| s.replace(&key_line, &key_line.replace("${{ runner.os }}-", ""))),
        "not the manifest hash",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &key_line,
                &format!("{key_line}          restore-keys: rr-corpus-\n"),
            )
        }),
        "restore-keys",
    );
    // The cached path must be RR_CORPUS_DIR, and RR_CORPUS_DIR the default.
    assert_flags(
        &m(&|s| {
            s.replacen(
                "          path: ${{ env.RR_CORPUS_DIR }}\n",
                "          path: ~/.cache/romraider-rs\n",
                1,
            )
        }),
        "restores Some(\"~/.cache/romraider-rs\")",
    );
    assert_flags(
        &m(&|s| {
            let save = "          path: ${{ env.RR_CORPUS_DIR }}\n          key: ${{ steps";
            s.replace(save, "          path: corpus\n          key: ${{ steps")
        }),
        "saves Some(\"corpus\")",
    );
    assert_flags(
        &m(&|s| s.replace("$HOME/.cache/romraider-rs/corpus", "$RUNNER_TEMP/corpus")),
        "does not set RR_CORPUS_DIR",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "$HOME/.cache/romraider-rs/corpus",
                "$GITHUB_WORKSPACE/target/corpus",
            )
        }),
        "does not set RR_CORPUS_DIR",
    );
    // Order: restore, then fetch, then save.
    assert_flags(
        &m(&|s| {
            let restore_start = s.find("      - name: Restore corpus cache\n").unwrap_or(0);
            let fetch_start = s
                .find("      - name: cargo xtask fetch-corpus\n")
                .unwrap_or(0);
            let save_start = s.find("      - name: Save corpus cache\n").unwrap_or(0);
            format!(
                "{}{}{}{}",
                &s[..restore_start],
                &s[fetch_start..save_start],
                &s[restore_start..fetch_start],
                &s[save_start..]
            )
        }),
        "restored after the fetch step",
    );
    assert_flags(
        &m(&|s| {
            let fetch_start = s
                .find("      - name: cargo xtask fetch-corpus\n")
                .unwrap_or(0);
            let save_start = s.find("      - name: Save corpus cache\n").unwrap_or(0);
            let ci_ok = s.find("\n  ci-ok:\n").unwrap_or(0);
            format!(
                "{}{}{}{}",
                &s[..fetch_start],
                &s[save_start..ci_ok],
                &s[fetch_start..save_start],
                &s[ci_ok..]
            )
        }),
        "saved before the fetch step",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: Build xtask\n        run: cargo build -p xtask --locked\n",
                "",
            )
        }),
        "has no step `Build xtask`",
    );
    // Save only on a miss, and only after a verified fetch.
    assert_flags(
        &m(&|s| {
            s.replace(
                "        if: steps.corpus-cache.outputs.cache-hit != 'true'\n",
                "        if: always()\n",
            )
        }),
        "not only on a miss",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "        if: steps.corpus-cache.outputs.cache-hit != 'true'\n",
                "",
            )
        }),
        "not only on a miss",
    );
    assert_flags(
        &m(&|s| s.replace("outputs.cache-primary-key }}", "run_id }}")),
        "not the restore's primary key",
    );
    // The fetch itself: plain, told the hit, timed, under the budget.
    for arg in [
        "--verify-only",
        "--dir /tmp/c",
        "--source https://example.invalid/x",
    ] {
        assert_flags(
            &m(&|s| {
                s.replace(
                    "          cargo xtask fetch-corpus 2>&1",
                    &format!("          cargo xtask fetch-corpus {arg} 2>&1"),
                )
            }),
            "passes arguments to fetch-corpus",
        );
    }
    assert_flags(
        &m(&|s| {
            s.replace(
                "          cargo xtask fetch-corpus 2>&1 | tee \"$log\"\n",
                "",
            )
        }),
        "does not run `cargo xtask fetch-corpus` exactly once",
    );
    assert_flags(
        &m(&|s| s.replace("          } >> \"$GITHUB_STEP_SUMMARY\"\n", "          }\n")),
        "does not log the fetch timing to the step summary",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "          CACHE_HIT: ${{ steps.corpus-cache.outputs.cache-hit }}\n",
                "",
            )
        }),
        "not told the cache hit",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "        id: fetch\n        timeout-minutes: 5\n",
                "        id: fetch\n",
            )
        }),
        "fetch step has no timeout-minutes",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "          BUDGET_SECONDS: \"300\"\n",
                "          BUDGET_SECONDS: \"600\"\n",
            )
        }),
        "disagree",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "        id: fetch\n        timeout-minutes: 5\n",
                "        id: fetch\n        timeout-minutes: 10\n",
            )
            .replace(
                "          BUDGET_SECONDS: \"300\"\n",
                "          BUDGET_SECONDS: \"600\"\n",
            )
        }),
        "is not the documented budget (5 minutes)",
    );
    assert_flags(
        &corpus_problems(&ci_yml(), None),
        "documents no `Cold-fetch budget: N minutes`",
    );
    // Nothing skips or softens the job.
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: cargo xtask fetch-corpus\n",
                "      - name: cargo xtask fetch-corpus\n        continue-on-error: true\n",
            )
        }),
        "continue-on-error",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: cargo xtask fetch-corpus\n",
                "      - name: cargo xtask fetch-corpus\n        if: steps.corpus-cache.outputs.cache-hit != 'true'\n",
            )
        }),
        "step `cargo xtask fetch-corpus` has a conditional",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "          cargo xtask fetch-corpus 2>&1 | tee \"$log\"\n",
                "          cargo xtask fetch-corpus 2>&1 | tee \"$log\" || true\n",
            )
        }),
        "swallows failures",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "    name: stock-ROM corpus (fetch-corpus)\n",
                "    name: stock-ROM corpus (fetch-corpus)\n    if: github.event_name == 'push'\n",
            )
        }),
        "job-level conditional",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "    name: stock-ROM corpus (fetch-corpus)\n    runs-on: ubuntu-24.04\n    timeout-minutes: 30\n",
                "    name: stock-ROM corpus (fetch-corpus)\n    runs-on: ubuntu-24.04\n",
            )
        }),
        "has no job timeout-minutes",
    );
}

/// US-RR-11-1: the verify and corpus-tier test steps cannot be dropped,
/// weakened or moved before the fetch.
#[test]
fn ci_corpus_check_rejects_weakened_verification() {
    let doc = documented_budget_minutes(&infra_doc());
    let m = |edit: &dyn Fn(&str) -> String| workflow::mutated(|s| corpus_problems(s, doc), edit);
    let verify_line = "          cargo xtask fetch-corpus --verify-only 2>&1 | tee \"$log\"\n";
    let tests_line = format!("        run: {TESTS_CMD}\n");
    let step = |s: &str, name: &str| {
        // From its `- name:` line to the next step or the end of the job.
        let start = s.find(&format!("      - name: {name}\n")).unwrap_or(0);
        let rest = &s[start + 1..];
        let end = [rest.find("\n      - "), rest.find("\n\n")]
            .into_iter()
            .flatten()
            .min()
            .map_or(s.len(), |e| start + 1 + e + 1);
        (start, end)
    };

    // The verify step: present, counting 501, plain --verify-only, checked.
    assert_flags(
        &m(&|s| {
            let (a, b) = step(s, VERIFY_STEP);
            format!("{}{}", &s[..a], &s[b..])
        }),
        "has no step `Verify all corpus images`",
    );
    for count in ["500", "1", "\"\""] {
        assert_flags(
            &m(&|s| s.replace("CORPUS_IMAGES: \"501\"", &format!("CORPUS_IMAGES: {count}"))),
            "not the 501 images of D-025",
        );
    }
    assert_flags(
        &m(&|s| {
            s.replace(
                verify_line,
                "          cargo xtask fetch-corpus 2>&1 | tee \"$log\"\n",
            )
        }),
        "does not run plain `cargo xtask fetch-corpus --verify-only`",
    );
    for arg in ["--dir /tmp/c", "--manifest /tmp/m.toml"] {
        assert_flags(
            &m(&|s| {
                s.replace(
                    verify_line,
                    &verify_line.replace("--verify-only", &format!("--verify-only {arg}")),
                )
            }),
            "does not run plain `cargo xtask fetch-corpus --verify-only`",
        );
    }
    assert_flags(
        &m(&|s| s.replace(verify_line, "")),
        "does not run plain `cargo xtask fetch-corpus --verify-only`",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                VERIFY_CHECK,
                "if ! grep -q \"^fetch-corpus: ok: \" \"$log\"; then",
            )
        }),
        "does not fail unless all CORPUS_IMAGES verify",
    );
    assert_flags(
        &m(&|s| s.replace(VERIFY_CHECK, "if false; then")),
        "does not fail unless all CORPUS_IMAGES verify",
    );
    // Moved before the fetch (and the save).
    assert_flags(
        &m(&|s| {
            let (a, b) = step(s, VERIFY_STEP);
            let fetch = s
                .find("      - name: cargo xtask fetch-corpus\n")
                .unwrap_or(0);
            format!("{}{}{}{}", &s[..fetch], &s[a..b], &s[fetch..a], &s[b..])
        }),
        "verified before it is fetched and saved",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: Verify all corpus images\n",
                "      - name: Verify all corpus images\n        if: github.event_name == 'push'\n",
            )
        }),
        "step `Verify all corpus images` has a conditional",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: Verify all corpus images\n",
                "      - name: Verify all corpus images\n        continue-on-error: true\n",
            )
        }),
        "continue-on-error",
    );

    // The corpus-tier tests: present, forced, non-empty, after the fetch.
    assert_flags(
        &m(&|s| s.replace(&tests_line, "        run: echo skipped\n")),
        "does not run exactly",
    );
    for weaken in [
        " --run-ignored all",
        " --no-tests=fail",
        " --locked",
        " --workspace",
    ] {
        assert_flags(
            &m(&|s| s.replace(&tests_line, &tests_line.replacen(weaken, "", 1))),
            "does not run exactly",
        );
    }
    assert_flags(
        &m(&|s| {
            s.replace(
                &tests_line,
                &tests_line.replace("requires_RR_CORPUS_DIR", "requires_RR_CORPUS_DIR_or_not"),
            )
        }),
        "does not run exactly",
    );
    assert_flags(
        &m(&|s| {
            let (a, b) = step(s, TESTS_STEP);
            format!("{}{}", &s[..a], &s[b..])
        }),
        "has no step `Corpus-tier tests (cargo nextest)`",
    );
    assert_flags(
        &m(&|s| {
            let (a, b) = step(s, TESTS_STEP);
            let fetch = s
                .find("      - name: cargo xtask fetch-corpus\n")
                .unwrap_or(0);
            format!("{}{}{}{}", &s[..fetch], &s[a..b], &s[fetch..a], &s[b..])
        }),
        "tests run before the corpus is fetched",
    );
    assert_flags(
        &m(&|s| {
            let (start, _) = step(s, VERIFY_STEP);
            let install = "      - uses: taiki-e/install-action@v2\n        with:\n          tool: cargo-nextest\n";
            let at = s[start..].find(install).map_or(0, |i| start + i);
            format!("{}{}", &s[..at], &s[at + install.len()..])
        }),
        "installs no cargo-nextest",
    );
}

/// US-RR-16-6: the coverage check cannot be dropped, weakened, redirected
/// or moved before the fetch.
#[test]
fn ci_corpus_check_rejects_weakened_coverage_check() {
    let doc = documented_budget_minutes(&infra_doc());
    let m = |edit: &dyn Fn(&str) -> String| workflow::mutated(|s| corpus_problems(s, doc), edit);
    let run_line = format!("        run: {COVERAGE_CMD}\n");
    let step = format!("      - name: {COVERAGE_STEP}\n{run_line}");

    assert_flags(
        &m(&|s| s.replace(&step, "")),
        "has no step `Corpus coverage report`",
    );
    for weakened in [
        "cargo xtask gen-corpus-coverage",
        "cargo xtask gen-corpus-coverage --check --dir /tmp/c",
        "cargo xtask gen-corpus-coverage --check --manifest /tmp/m.toml",
        "cargo xtask gen-corpus-coverage --check --coverage /tmp/c.toml",
        "cargo xtask gen-corpus-manifest --check",
        "echo skipped",
    ] {
        assert_flags(
            &m(&|s| s.replace(&run_line, &format!("        run: {weakened}\n"))),
            "does not run exactly `cargo xtask gen-corpus-coverage --check`",
        );
    }
    assert_flags(
        &m(&|s| s.replace(&run_line, &format!("        run: {COVERAGE_CMD} || true\n"))),
        "swallows failures",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &step,
                &format!(
                    "      - name: {COVERAGE_STEP}\n        continue-on-error: true\n{run_line}"
                ),
            )
        }),
        "continue-on-error",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &step,
                &format!("      - name: {COVERAGE_STEP}\n        if: github.event_name == 'push'\n{run_line}"),
            )
        }),
        "step `Corpus coverage report` has a conditional",
    );
    // Moved before the fetch.
    assert_flags(
        &m(&|s| {
            let without = s.replace(&step, "");
            let fetch = without
                .find("      - name: cargo xtask fetch-corpus\n")
                .unwrap_or(0);
            format!("{}{step}{}", &without[..fetch], &without[fetch..])
        }),
        "coverage report is checked before the corpus is fetched and saved",
    );
}

/// US-RR-16-2: CI's plain fetch reads the committed manifest, which pins
/// the definitions, and nothing in the workflow redirects its sources, so
/// the job fetches and verifies the definitions alongside the ROMs.
#[test]
fn ci_corpus_fetch_covers_the_pinned_definitions() {
    let problems = committed_defs_problems(&committed_manifest_text());
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    let problems = defs_env_problems(&ci_yml());
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The definitions checks bite: a source or logger override anywhere in the
/// workflow, and a committed manifest without the pinned `[defs]`.
#[test]
fn ci_corpus_defs_checks_reject_weakened_setups() {
    let doc = documented_budget_minutes(&infra_doc());
    let m = |edit: &dyn Fn(&str) -> String| workflow::mutated(|s| corpus_problems(s, doc), edit);
    let budget = "          BUDGET_SECONDS: \"300\"\n";
    for var in [SOURCE_ENV, DEFS_SOURCE_ENV, LOGGER_ZIP_ENV] {
        assert_flags(
            &m(&|s| {
                s.replace(
                    budget,
                    &format!("{budget}          {var}: /tmp/elsewhere\n"),
                )
            }),
            &format!("ci.yml sets or names {var}"),
        );
    }
    let fetch_line = "          cargo xtask fetch-corpus 2>&1 | tee \"$log\"\n";
    assert_flags(
        &m(&|s| {
            s.replace(
                fetch_line,
                &format!("          export {DEFS_SOURCE_ENV}=/tmp/defs.git\n{fetch_line}"),
            )
        }),
        &format!("ci.yml sets or names {DEFS_SOURCE_ENV}"),
    );
    // A workflow-level env block.
    assert_flags(
        &m(&|s| {
            s.replacen(
                "\njobs:\n",
                &format!("\nenv:\n  {LOGGER_ZIP_ENV}: /tmp/l.zip\njobs:\n"),
                1,
            )
        }),
        &format!("ci.yml sets or names {LOGGER_ZIP_ENV}"),
    );

    let text = committed_manifest_text();
    let committed = Manifest::from_toml(&text).unwrap();
    let render = |edit: &dyn Fn(&mut Manifest)| {
        let mut man = committed.clone();
        edit(&mut man);
        let changed = man.render().unwrap();
        assert_ne!(changed, text, "mutation did not apply");
        committed_defs_problems(&changed)
    };
    assert_flags(&render(&|man| man.defs = None), "has no [defs]");
    assert_flags(
        &render(&|man| {
            if let Some(d) = man.defs.as_mut() {
                d.commit = "1".repeat(40);
            }
        }),
        "commit is 1111",
    );
    assert_flags(
        &render(&|man| {
            if let Some(d) = man.defs.as_mut() {
                d.upstream = "https://example.invalid/defs".to_owned();
            }
        }),
        "upstream is",
    );
    assert_flags(
        &render(&|man| {
            if let Some(d) = man.defs.as_mut() {
                d.files.clear();
            }
        }),
        "lists no files",
    );
    assert_flags(&committed_defs_problems("schema = ["), "does not parse");
}

#[cfg(unix)]
mod behaviour {
    //! The fetch step's script, run for real (module docs).

    use std::fs;
    use std::hash::{DefaultHasher, Hash as _, Hasher as _};
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use sha2::{Digest as _, Sha256};
    use xtask::corpus_manifest::{
        DefFile, DefsCorpus, LoggerPackage, Manifest, RomCorpus, RomFile, SCHEMA,
    };

    use super::workflow::{Workflow, ci_yml};
    use super::{FETCH_STEP, JOB};

    const IMAGES: &[(&str, usize)] = &[
        (
            "ADM/Forester/A2WC400H-2005-ADM-Subaru-Forester-XT-AT.hex",
            4096,
        ),
        ("USDM/Impreza/B.hex", 3001),
    ];

    /// The synthetic definitions source's pinned subtree.
    const DEFS_PREFIX: &str = "RomRaider/";
    /// Synthetic definition files: path below [`DEFS_PREFIX`] and length.
    pub(super) const DEFS: &[(&str, usize)] = &[
        ("ecu/standard/ecu_defs.xml", 2048),
        ("logger/logger_STD_EN_v290.xml", 777),
    ];

    /// Where the fetch puts the definitions in the corpus directory.
    pub(super) fn defs_tree(corpus: &Path) -> PathBuf {
        corpus.join("defs").join("romraider")
    }

    fn bytes(i: usize, len: usize) -> Vec<u8> {
        (0..len)
            .map(|j| (j.wrapping_mul(31) ^ (i * 97)) as u8)
            .collect()
    }

    fn sha256_hex(b: &[u8]) -> String {
        Sha256::digest(b)
            .iter()
            .map(|x| format!("{x:02x}"))
            .collect()
    }

    #[expect(
        clippy::unwrap_used,
        reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
    )]
    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "core.autocrlf=false",
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// A scratch area outside the repository (fetch-corpus refuses a corpus
    /// directory inside it): a synthetic ROM source repository, a synthetic
    /// definitions source repository, a manifest naming both (`[defs]`
    /// included), and a `cargo` that forwards `cargo xtask <args>` to the
    /// real xtask binary with `--manifest` pointing at that manifest.
    struct Scratch {
        root: PathBuf,
        source: PathBuf,
        defs_source: PathBuf,
        bin: PathBuf,
    }

    #[expect(
        clippy::unwrap_used,
        reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
    )]
    fn scratch(script: &str) -> Scratch {
        // One directory per script, so concurrent runs (`cargo test` runs the
        // committed and the mutated scripts in one process) never share one.
        let mut h = DefaultHasher::new();
        script.hash(&mut h);
        let root = std::env::temp_dir().join(format!(
            "rr-corpus-ci-config-{}-{:016x}",
            std::process::id(),
            h.finish()
        ));
        let _ = fs::remove_dir_all(&root);
        let source = root.join("source");
        fs::create_dir_all(&source).unwrap();
        git(&source, &["init", "--quiet"]);
        let mut files = Vec::new();
        for (i, (path, len)) in IMAGES.iter().enumerate() {
            let b = bytes(i, *len);
            let p = source.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, &b).unwrap();
            files.push(RomFile {
                path: (*path).to_owned(),
                size: b.len() as u64,
                sha256: sha256_hex(&b),
                filename_rom_id: None,
                year: None,
                year_end: None,
                region: None,
                model: None,
                trim: None,
                suspect: None,
            });
        }
        git(&source, &["add", "-A"]);
        git(&source, &["commit", "--quiet", "-m", "corpus"]);

        let defs_source = root.join("defs-source");
        fs::create_dir_all(&defs_source).unwrap();
        git(&defs_source, &["init", "--quiet"]);
        let mut def_files = Vec::new();
        for (i, (path, len)) in DEFS.iter().enumerate() {
            let b = bytes(100 + i, *len);
            let p = defs_source.join(DEFS_PREFIX).join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, &b).unwrap();
            def_files.push(DefFile {
                path: (*path).to_owned(),
                size: b.len() as u64,
                sha256: sha256_hex(&b),
            });
        }
        fs::write(defs_source.join("README.md"), "synthetic\n").unwrap();
        git(&defs_source, &["add", "-A"]);
        git(&defs_source, &["commit", "--quiet", "-m", "defs"]);
        let defs = DefsCorpus {
            upstream: "https://example.invalid/defs-not-used.git".to_owned(),
            branch: "Alpha".to_owned(),
            commit: git(&defs_source, &["rev-parse", "HEAD"]),
            path_prefix: DEFS_PREFIX.to_owned(),
            licence: "none (synthetic test data)".to_owned(),
            logger_v370: LoggerPackage {
                file: "logger_v370.zip".to_owned(),
                size: 1,
                sha256: "0".repeat(64),
                url: "https://example.invalid/never-downloaded".to_owned(),
                note: "synthetic".to_owned(),
            },
            files: def_files,
        };

        let manifest = Manifest {
            schema: SCHEMA,
            roms: RomCorpus {
                upstream: "https://example.invalid/not-used.git".to_owned(),
                commit: git(&source, &["rev-parse", "HEAD"]),
                path_prefix: String::new(),
                files,
            },
            defs: Some(defs),
        };
        let manifest_path = root.join("corpus-manifest.toml");
        fs::write(&manifest_path, manifest.render().unwrap()).unwrap();

        let bin = root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let cargo = bin.join("cargo");
        fs::write(
            &cargo,
            format!(
                "#!/bin/sh\n[ \"$1\" = xtask ] || {{ echo \"stub cargo: $*\" >&2; exit 97; }}\nshift\n[ -z \"$STUB_SLEEP\" ] || sleep \"$STUB_SLEEP\"\n[ -z \"$STUB_SILENT\" ] || exit 0\nexec '{}' \"$@\" --manifest '{}'\n",
                env!("CARGO_BIN_EXE_xtask"),
                manifest_path.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
        Scratch {
            root,
            source,
            defs_source,
            bin,
        }
    }

    struct Run {
        ok: bool,
        log: String,
        summary: String,
        outputs: String,
    }

    #[expect(
        clippy::unwrap_used,
        reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
    )]
    fn run_step(
        script: &str,
        sc: &Scratch,
        case: &str,
        corpus: &Path,
        env: &[(&str, &str)],
    ) -> Run {
        let dir = sc.root.join(case);
        fs::create_dir_all(&dir).unwrap();
        let summary = dir.join("summary.md");
        let outputs = dir.join("outputs");
        let path = format!(
            "{}:{}",
            sc.bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut cmd = Command::new("bash");
        cmd.args(["--noprofile", "--norc", "-eo", "pipefail", "-c", script])
            .current_dir(&dir)
            .env_clear()
            .env("PATH", path)
            .env("HOME", &sc.root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("RUNNER_TEMP", &dir)
            .env("GITHUB_STEP_SUMMARY", &summary)
            .env("GITHUB_OUTPUT", &outputs)
            .env("RR_CORPUS_DIR", corpus)
            .env("RR_CORPUS_SOURCE", &sc.source)
            .env("RR_DEFS_SOURCE", &sc.defs_source)
            .env("BUDGET_SECONDS", "300");
        for (k, v) in env {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        Run {
            ok: out.status.success(),
            log: format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
            summary: fs::read_to_string(summary).unwrap_or_default(),
            outputs: fs::read_to_string(outputs).unwrap_or_default(),
        }
    }

    /// Every way the fetch step's `script` misbehaves (module docs).
    pub(super) fn fetch_step_behaviour_problems(script: &str) -> Vec<String> {
        let sc = scratch(script);
        let corpus = sc.root.join("cache").join("corpus");
        let mut problems = Vec::new();
        // Findings outside `expect`, which holds `problems` until its last use.
        let mut other = Vec::new();
        let mut expect = |case: &str, run: &Run, want_ok: bool, wants: &[&str]| {
            if run.ok != want_ok {
                problems.push(format!(
                    "{case}: the step {} (log:\n{})",
                    if run.ok { "passed" } else { "failed" },
                    run.log
                ));
            }
            let all = format!("{}\n{}\n{}", run.log, run.summary, run.outputs);
            for w in wants {
                if !all.contains(w) {
                    problems.push(format!(
                        "{case}: {w:?} not logged (log:\n{}\nsummary:\n{}\noutputs:\n{})",
                        run.log, run.summary, run.outputs
                    ));
                }
            }
        };

        // Cache miss, empty directory: a cold fetch, timed.
        let r = run_step(script, &sc, "cold", &corpus, &[("CACHE_HIT", "")]);
        expect(
            "cold fetch on a cache miss",
            &r,
            true,
            &[
                "fetch-corpus: ok: fetched ",
                "::notice title=stock-ROM corpus::cache-hit=false; fetch: cold; ",
                "s (budget 300s)",
                "### Stock-ROM corpus",
                "| false | cold | ",
                "s | 300s |",
                "cache-hit=false\n",
                "fetch=cold\n",
                "seconds=",
            ],
        );
        if !corpus.join(IMAGES[0].0).is_file() {
            other.push("cold fetch on a cache miss: no corpus written".to_owned());
        }
        // US-RR-16-2: the same plain fetch fetched and verified the
        // definitions alongside the ROMs.
        let n_defs = DEFS.len();
        if !r.log.contains(&format!(
            "fetch-corpus: defs: ok: fetched `{DEFS_PREFIX}` of "
        )) || !r
            .log
            .contains(&format!("; {n_defs}/{n_defs} definition file(s), "))
        {
            other.push(format!(
                "cold fetch on a cache miss: the definitions were not fetched and verified (log:\n{})",
                r.log
            ));
        }
        for (path, _) in DEFS {
            if !defs_tree(&corpus).join(path).is_file() {
                other.push(format!(
                    "cold fetch on a cache miss: definition {path} not written"
                ));
            }
        }

        // Cache hit, complete: verified, nothing fetched.
        let r = run_step(script, &sc, "warm", &corpus, &[("CACHE_HIT", "true")]);
        expect(
            "warm cache hit",
            &r,
            true,
            &[
                "; nothing fetched",
                "cache-hit=true; fetch: warm (verified, nothing fetched); ",
                "| true | warm (verified, nothing fetched) | ",
                "cache-hit=true\n",
                "fetch=warm\n",
            ],
        );
        if !r.log.contains(&format!(
            "fetch-corpus: defs: ok: {n_defs}/{n_defs} definition file(s), "
        )) {
            other.push(format!(
                "warm cache hit: the definitions were not verified (log:\n{})",
                r.log
            ));
        }

        // US-RR-16-2: a restored definitions file with one byte flipped
        // (same size), and no reachable definitions source to repair it
        // from: the ROMs verify and print their ok line, but the step
        // fails, naming the file.
        let def = defs_tree(&corpus).join(DEFS[0].0);
        let good = fs::read(&def).unwrap_or_default();
        let mut bad = good.clone();
        if let Some(b) = bad.get_mut(10) {
            *b ^= 0x01;
        }
        let _ = fs::write(&def, &bad);
        let r = run_step(
            script,
            &sc,
            "defs-bad",
            &corpus,
            &[
                ("CACHE_HIT", "true"),
                ("RR_DEFS_SOURCE", "/nonexistent/defs.git"),
            ],
        );
        expect(
            "definitions that do not verify",
            &r,
            false,
            &[format!("{}: sha256 ", DEFS[0].0).as_str()],
        );
        let _ = fs::write(&def, &good);

        // Cache hit, but the restored tree does not verify: refetched.
        let _ = fs::write(corpus.join(IMAGES[1].0), b"corrupted");
        let r = run_step(script, &sc, "repair", &corpus, &[("CACHE_HIT", "true")]);
        expect(
            "cache hit that does not verify",
            &r,
            true,
            &[
                "did not verify",
                "cache-hit=true; fetch: cold (restored cache did not verify); ",
                "cache-hit=true\n",
                "fetch=cold\n",
            ],
        );

        // The fetch fails (unreachable source): the step fails.
        let r = run_step(
            script,
            &sc,
            "fails",
            &sc.root.join("cache2").join("corpus"),
            &[
                ("CACHE_HIT", ""),
                ("RR_CORPUS_SOURCE", "/nonexistent/source.git"),
            ],
        );
        expect("failing fetch", &r, false, &[]);
        if r.summary.contains("| false |") {
            other.push("failing fetch: logged as a successful fetch".to_owned());
        }

        // fetch-corpus exits 0 but prints no ok line: the step fails, and
        // logs no timing as if it had fetched.
        let r = run_step(
            script,
            &sc,
            "silent",
            &corpus,
            &[("CACHE_HIT", ""), ("STUB_SILENT", "1")],
        );
        expect(
            "no ok line",
            &r,
            false,
            &["::error::fetch-corpus printed no ok line"],
        );
        if !r.summary.is_empty() || !r.outputs.is_empty() {
            other.push("no ok line: timing logged for a fetch that never reported".to_owned());
        }

        // Over budget: the step fails, after logging the time.
        let r = run_step(
            script,
            &sc,
            "slow",
            &corpus,
            &[
                ("CACHE_HIT", "true"),
                ("BUDGET_SECONDS", "0"),
                ("STUB_SLEEP", "0.05"),
            ],
        );
        expect(
            "over budget",
            &r,
            false,
            &["over the 0s budget", "s (budget 0s)"],
        );

        problems.extend(other);
        let _ = fs::remove_dir_all(&sc.root);
        problems
    }

    /// Every way the verify step's `script` misbehaves (module docs), run
    /// against the synthetic corpus with `CORPUS_IMAGES` set to its size.
    pub(super) fn verify_step_behaviour_problems(script: &str) -> Vec<String> {
        let sc = scratch(script);
        let corpus = sc.root.join("cache").join("corpus");
        let mut problems = Vec::new();
        let setup = run_step("cargo xtask fetch-corpus\n", &sc, "setup", &corpus, &[]);
        if !setup.ok {
            problems.push(format!("setup: the fetch failed:\n{}", setup.log));
            return problems;
        }
        let mut expect = |case: &str, run: &Run, want_ok: bool, wants: &[String]| {
            if run.ok != want_ok {
                problems.push(format!(
                    "{case}: the step {} (log:\n{})",
                    if run.ok { "passed" } else { "failed" },
                    run.log
                ));
            }
            for w in wants {
                if !run.log.contains(w.as_str()) {
                    problems.push(format!("{case}: {w:?} not logged (log:\n{})", run.log));
                }
            }
        };
        let n = IMAGES.len();

        // Every image verifies, and the count is the expected one: passes.
        let images = n.to_string();
        let r = run_step(
            script,
            &sc,
            "verified",
            &corpus,
            &[("CORPUS_IMAGES", &images)],
        );
        expect(
            "verified corpus",
            &r,
            true,
            &[format!("fetch-corpus: ok: {n}/{n} image(s), ")],
        );

        // The manifest has fewer images than expected (a truncated manifest):
        // every image it lists verifies, but the step fails.
        let more = (n + 1).to_string();
        let r = run_step(script, &sc, "short", &corpus, &[("CORPUS_IMAGES", &more)]);
        expect(
            "short count",
            &r,
            false,
            &[format!(
                "::error::the corpus did not verify as {more}/{more} images"
            )],
        );

        // US-RR-16-2: one byte of a definition file flipped (same size):
        // the images all verify, but the step fails, naming the file.
        let def = defs_tree(&corpus).join(DEFS[0].0);
        let good = fs::read(&def).unwrap_or_default();
        let mut bad = good.clone();
        if let Some(b) = bad.get_mut(10) {
            *b ^= 0x01;
        }
        let _ = fs::write(&def, &bad);
        let r = run_step(
            script,
            &sc,
            "defs-corrupt",
            &corpus,
            &[("CORPUS_IMAGES", &images)],
        );
        expect(
            "corrupted definitions",
            &r,
            false,
            &[
                format!("fetch-corpus: ok: {n}/{n} image(s), "),
                format!("{}: sha256 ", DEFS[0].0),
            ],
        );
        let _ = fs::write(&def, &good);

        // A flipped byte and a truncated file: fails, listing both.
        let a = corpus.join(IMAGES[0].0);
        if let Ok(mut b) = fs::read(&a) {
            b[10] ^= 0x80;
            let _ = fs::write(&a, b);
        }
        let _ = fs::write(corpus.join(IMAGES[1].0), vec![0u8; 1000]);
        let r = run_step(
            script,
            &sc,
            "corrupt",
            &corpus,
            &[("CORPUS_IMAGES", &images)],
        );
        expect(
            "corrupted corpus",
            &r,
            false,
            &[
                "(2 problem(s))".to_owned(),
                format!("{}: sha256 ", IMAGES[0].0),
                format!(
                    "{}: size 1000 bytes, manifest says {}",
                    IMAGES[1].0, IMAGES[1].1
                ),
            ],
        );

        let _ = fs::remove_dir_all(&sc.root);
        problems
    }

    pub(super) fn verify_script() -> String {
        let src = ci_yml();
        let wf = Workflow::parse(&src);
        wf.job(JOB)
            .and_then(|j| j.steps().into_iter().find(|s| s.id == super::VERIFY_STEP))
            .and_then(|s| s.run_block())
            .unwrap_or_default()
    }

    pub(super) fn fetch_script(src: &str) -> String {
        let wf = Workflow::parse(src);
        wf.job(JOB)
            .and_then(|j| j.steps().into_iter().find(|s| s.id == FETCH_STEP))
            .and_then(|s| s.run_block())
            .unwrap_or_default()
    }

    pub(super) fn committed_script() -> String {
        fetch_script(&ci_yml())
    }
}

/// The committed fetch step logs a cold fetch, a warm hit and a repair, and
/// fails on a failing or over-budget fetch.
#[cfg(unix)]
#[test]
fn ci_corpus_fetch_step_logs_timing_and_cache_hit() {
    let script = behaviour::committed_script();
    assert!(!script.is_empty(), "no fetch step script in ci.yml");
    let problems = behaviour::fetch_step_behaviour_problems(&script);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The behaviour check bites on weakened fetch scripts.
#[cfg(unix)]
#[test]
fn ci_corpus_fetch_behaviour_check_rejects_weakened_scripts() {
    let script = behaviour::committed_script();
    let edits = |edits: &[(&str, &str)], needle: &str| {
        let mut changed = script.clone();
        for (from, to) in edits {
            let next = changed.replace(from, to);
            assert_ne!(next, changed, "mutation did not apply: {from:?}");
            changed = next;
        }
        assert_flags(&behaviour::fetch_step_behaviour_problems(&changed), needle);
    };
    let check = |from: &str, to: &str, needle: &str| edits(&[(from, to)], needle);
    // No budget check: an over-budget fetch passes.
    check(
        "if [ \"$ms\" -gt $((BUDGET_SECONDS * 1000)) ]; then",
        "if false; then",
        "over budget: the step passed",
    );
    // The hit is not reported.
    check(
        "hit=true; fi",
        "hit=false; fi",
        "warm cache hit: \"cache-hit=true",
    );
    // A repair is reported as warm.
    check(
        "fetch=\"cold (restored cache did not verify)\"",
        "fetch=\"warm (verified, nothing fetched)\"",
        "cache hit that does not verify",
    );
    // No summary.
    check(
        "} >> \"$GITHUB_STEP_SUMMARY\"",
        "} > /dev/null",
        "\"### Stock-ROM corpus\" not logged",
    );
    // A missing ok line no longer fails the step.
    check(
        "  exit 1\nfi\nline=",
        "  fetch=none\nfi\nline=",
        "no ok line: the step passed",
    );
    // US-RR-16-2: the fetch's failure is lost in the pipe. The ROMs still
    // print their ok line, so only the definitions case catches it.
    check(
        "cargo xtask fetch-corpus 2>&1 | tee \"$log\"",
        "{ cargo xtask fetch-corpus 2>&1 || :; } | tee \"$log\"",
        "definitions that do not verify: the step passed",
    );
    // The fetch's failure is lost in the pipe, and a missing ok line no
    // longer fails the step.
    edits(
        &[
            (
                "cargo xtask fetch-corpus 2>&1 | tee \"$log\"",
                "{ cargo xtask fetch-corpus 2>&1 || :; } | tee \"$log\"",
            ),
            ("  exit 1\nfi\nline=", "  fetch=none\nfi\nline="),
        ],
        "failing fetch: the step passed",
    );
}

/// US-RR-11-1: the committed verify step passes on a verified corpus of the
/// expected size, and fails on a short count or on corrupted images, listing
/// every bad path.
#[cfg(unix)]
#[test]
fn ci_corpus_verify_step_fails_unless_every_image_verifies() {
    let script = behaviour::verify_script();
    assert!(!script.is_empty(), "no verify step script in ci.yml");
    let problems = behaviour::verify_step_behaviour_problems(&script);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The verify behaviour check bites on weakened verify scripts.
#[cfg(unix)]
#[test]
fn ci_corpus_verify_behaviour_check_rejects_weakened_scripts() {
    let script = behaviour::verify_script();
    let check = |from: &str, to: &str, needle: &str| {
        let changed = script.replace(from, to);
        assert_ne!(changed, script, "mutation did not apply: {from:?}");
        assert_flags(&behaviour::verify_step_behaviour_problems(&changed), needle);
    };
    // The count is not checked: a truncated manifest passes.
    check(
        VERIFY_CHECK,
        "if false; then",
        "short count: the step passed",
    );
    check(
        "ok: ${CORPUS_IMAGES}/${CORPUS_IMAGES} image(s), ",
        "ok: ",
        "short count: the step passed",
    );
    // The verification's failure is lost in the pipe.
    check(
        "cargo xtask fetch-corpus --verify-only 2>&1 | tee \"$log\"",
        "{ cargo xtask fetch-corpus --verify-only 2>&1 || :; } | tee \"$log\"; echo 'fetch-corpus: ok: 2/2 image(s), ' >> \"$log\"",
        "corrupted corpus: the step passed",
    );
    // US-RR-16-2: lost in the pipe, a corrupted definition passes (the
    // images' ok line is genuine).
    check(
        "cargo xtask fetch-corpus --verify-only 2>&1 | tee \"$log\"",
        "{ cargo xtask fetch-corpus --verify-only 2>&1 || :; } | tee \"$log\"",
        "corrupted definitions: the step passed",
    );
}
