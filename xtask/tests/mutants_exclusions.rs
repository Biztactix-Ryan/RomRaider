//! US-RR-4-3 / US-RR-4-7: every exclusion in `.cargo/mutants.toml` has a
//! justification comment, and a CI check rejects uncommented exclusions.
//!
//! * The committed file passes `cargo xtask check-mutants-exclusions` (rules
//!   in `xtask/src/mutants_exclusions.rs`), and the check is not vacuous: it
//!   finds the file's real exclusions.
//! * The binary, as CI runs it, exits non-zero naming the entry on planted
//!   samples: an uncommented entry, a comment separated from its entry by a
//!   blank line, and a trivial comment (`# TODO`); a justified sample passes.
//! * [`wiring_problems`]: `ci.yml`'s `clippy` job runs the check on every push
//!   and pull request, the step cannot be skipped or softened, and `clippy` is
//!   in `ci-ok.needs`. The `*_rejects_*` test proves each assertion bites on a
//!   mutated copy of the real file.

#[path = "common/workflow.rs"]
mod workflow;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use workflow::{
    Workflow, assert_flags, ci_yml, gate_needs_problems, skippable_problems, trigger_problems,
};
use xtask::mutants_exclusions::{self, CONFIG};

const JOB: &str = "clippy";
const CMD: &str = "cargo xtask check-mutants-exclusions";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn committed_mutants_toml_exclusions_are_all_justified() {
    let report = mutants_exclusions::run(&root(), None).unwrap();
    assert!(
        report.problems.is_empty(),
        "{CONFIG} has unjustified exclusions:\n{}",
        report.problems.join("\n")
    );
    // Not vacuous: the file's real `exclude_re` entry is found, with its comment.
    let mint = report
        .entries
        .iter()
        .find(|e| e.key == "exclude_re" && e.value == "replace authority::mint_")
        .unwrap_or_else(|| panic!("exclusion not found in {:?}", report.entries));
    assert!(
        mint.justification
            .as_deref()
            .is_some_and(|j| j.contains("confirmation-authority")),
        "{mint:?}"
    );
}

/// Writes `text` as a planted mutants.toml and runs the CI command on it.
fn check(name: &str, text: &str) -> io::Result<(bool, String)> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("check-mutants-exclusions");
    fs::create_dir_all(&dir)?;
    let file = dir.join(format!("{name}.toml"));
    fs::write(&file, text)?;
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("check-mutants-exclusions")
        .arg("--config")
        .arg(&file)
        .output()?;
    Ok((
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    ))
}

const WHY: &str = "    # Compiled only with a feature this crate's own tests never enable,\n    # so no test sees it; the wrapped function is gated and killed.\n";

#[test]
fn check_cli_rejects_planted_unjustified_exclusions() -> io::Result<()> {
    let good = format!(
        "examine_globs = [\"src/a.rs\"]\n\nexclude_globs = [\n{WHY}    \"src/gen/**\",\n]\n\nexclude_re = [\n{WHY}    \"replace authority::mint_\",\n\n{WHY}    'replace .* -> bool with \\w',\n]\n"
    );
    let (ok, out) = check("justified", &good)?;
    assert!(ok, "{out}");
    assert!(
        out.contains("check-mutants-exclusions: ok (3 exclusion(s)"),
        "{out}"
    );

    for (name, text, needle) in [
        (
            "uncommented",
            format!("exclude_re = [\n{WHY}    \"replace a\",\n    \"replace b\",\n]\n"),
            ":5: exclude_re entry \"replace b\": no justification comment",
        ),
        (
            "uncommented-glob",
            "exclude_globs = [\n    \"src/gen/**\",\n]\n".to_owned(),
            ":2: exclude_globs entry \"src/gen/**\": no justification comment",
        ),
        (
            "blank-line",
            format!("exclude_re = [\n{WHY}\n    \"replace a\",\n]\n"),
            ":5: exclude_re entry \"replace a\": its justification comment is separated from it by a blank line",
        ),
        (
            "trivial",
            "exclude_re = [\n    # TODO\n    \"replace a\",\n]\n".to_owned(),
            ":3: exclude_re entry \"replace a\": justification is a placeholder",
        ),
        (
            "short",
            "exclude_re = [\n    # equivalent mutant\n    \"replace a\",\n]\n".to_owned(),
            ":3: exclude_re entry \"replace a\": justification is too short",
        ),
        (
            "above-key",
            format!("{WHY}exclude_re = [\n    \"replace a\",\n]\n"),
            ":4: exclude_re entry \"replace a\": no justification comment",
        ),
        (
            "same-line",
            format!("{WHY}exclude_re = [\"replace a\"]\n"),
            ":3: exclude_re entry \"replace a\": on the same line as `exclude_re = [`",
        ),
    ] {
        let (ok, out) = check(name, &text)?;
        assert!(!ok, "{name}: passed:\n{out}");
        assert!(
            out.contains(needle),
            "{name}: expected {needle:?} in:\n{out}"
        );
    }

    // A missing file fails too (fail closed).
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["check-mutants-exclusions", "--root"])
        .arg(Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-such-root"))
        .output()?;
    assert!(!out.status.success());
    Ok(())
}

/// Every way `src` (ci.yml) fails to run the check as a blocking gate.
fn wiring_problems(src: &str) -> Vec<String> {
    let wf = Workflow::parse(src);
    let mut problems = trigger_problems(&wf);
    problems.extend(gate_needs_problems(&wf, JOB));
    let Some(job) = wf.job(JOB) else {
        problems.push(format!("no `{JOB}` job"));
        return problems;
    };
    if job.value_at("if", 4).is_some() {
        problems.push(format!("`{JOB}` has a job-level `if:`"));
    }
    let scripts = job.run_scripts();
    if !scripts.iter().any(|s| s == CMD) {
        problems.push(format!(
            "`{JOB}` does not run `{CMD}` (scripts: {scripts:?})"
        ));
    }
    problems.extend(skippable_problems(job, &[]));
    problems
}

#[test]
fn ci_runs_the_exclusion_check_as_a_gate() {
    let problems = wiring_problems(&ci_yml());
    assert!(
        problems.is_empty(),
        ".github/workflows/ci.yml does not gate on {CMD}:\n{}",
        problems.join("\n")
    );
}

#[test]
fn ci_exclusion_check_wiring_rejects_weakened_ci() {
    let m = |edit: &dyn Fn(&str) -> String| workflow::mutated(wiring_problems, edit);
    let step = format!("        run: {CMD}\n");
    assert_flags(
        &m(&|s| s.replace(&step, "        run: echo skipped\n")),
        "does not run `cargo xtask check-mutants-exclusions`",
    );
    assert_flags(
        &m(&|s| s.replace(&step, &format!("        run: # {CMD}\n"))),
        "does not run",
    );
    assert_flags(
        &m(&|s| s.replace(&step, &format!("        run: {CMD} || true\n"))),
        "swallows failures",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &format!("      - name: {CMD}\n"),
                &format!("      - name: {CMD}\n        if: false\n"),
            )
        }),
        "has a conditional",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &format!("      - name: {CMD}\n"),
                &format!("      - name: {CMD}\n        continue-on-error: true\n"),
            )
        }),
        "continue-on-error",
    );
    assert_flags(
        &m(&|s| s.replace("needs: [fmt, clippy, ", "needs: [fmt, ")),
        "`ci-ok.needs` does not include `clippy`",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "  clippy:\n    name: clippy\n",
                "  clippy:\n    name: clippy\n    if: false\n",
            )
        }),
        "job-level `if:`",
    );
    assert_flags(
        &m(&|s| s.replace("  pull_request:\n", "  pull_request_target:\n")),
        "does not run on `pull_request`",
    );
}
