//! Mutation-gate self-test (`cargo xtask mutants-selftest`, story US-RR-4,
//! task US-RR-4-6): proves that a weakened test on a gated module produces a
//! surviving mutant and fails the mutants job, for real, with this
//! workspace's `.cargo/mutants.toml`.
//!
//! Reading `mutants.yml` proves the job is wired as a hard gate
//! (`xtask/tests/mutants_ci_config.rs`); this proves what its commands do
//! with a bad change. It builds a scratch git repository per [`Case`] under
//! `<root>/target/mutants-selftest/<case>/`: a one-crate workspace whose
//! module sits at [`GATED_FILE`], a path the real configuration gates (it is
//! copied verbatim, with `rust-toolchain.toml`), commits [`BASE`], commits
//! the case's change on top, and then does what the pull-request job does:
//! writes `git diff <base>...HEAD`, asks [`mutants_scope::scope`] for the
//! scope, and runs `cargo mutants` (full) or `cargo mutants --in-diff`. It
//! requires:
//!
//! * **control** (no change; a full run, as the weekly job does): exit 0, at least one mutant caught
//!   and none missed, so the harness and the gated path are live (a config
//!   that stops gating [`GATED_FILE`] generates no mutant and fails here);
//! * **weakened-test** (one assertion deleted from the module's tests): the
//!   scope is `full` (an `--in-diff` run would see no changed code and pass),
//!   and cargo-mutants exits 2 with a MISSED mutant in [`GATED_FILE`];
//! * **untested-change** (a new function and no test for it): the scope is
//!   `diff`, and `cargo mutants --in-diff` exits 2 with that function's
//!   mutants MISSED.
//!
//! Needs `cargo-mutants`, `cargo-nextest` (the configured `test_tool`) and
//! `git` on PATH. Offline: the scratch crate has no dependencies. Takes
//! about a minute.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail};

use crate::mutants_scope::{self, Scope};

/// Scratch directory, relative to the workspace root.
pub const SCRATCH_DIR: &str = "target/mutants-selftest";

/// The scratch module's path: gated by `.cargo/mutants.toml`.
pub const GATED_FILE: &str = "crates/rr-core/src/encoding.rs";

/// `git` arguments that write a pull request's diff, followed by
/// `<base>...HEAD`. mutants.yml uses the same (checked by
/// `xtask/tests/mutants_ci_config.rs`).
pub const DIFF_ARGS: &[&str] = &["diff", "--no-color", "--no-ext-diff"];

/// The gated module before the change: fully tested.
pub const BASE: &str = r"//! mutants-selftest: a module at a gated path.

/// True for a negative reading.
pub fn is_negative(x: i32) -> bool {
    x < 0
}

#[cfg(test)]
mod tests {
    use super::is_negative;

    #[test]
    fn sign() {
        assert!(is_negative(-1));
        assert!(!is_negative(0));
        assert!(!is_negative(1));
    }
}
";

/// What a case's mutation run must report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    /// Exit 0, some mutants caught, none missed.
    Pass,
    /// Exit 2 with a MISSED mutant whose name contains this text.
    Missed(&'static str),
}

/// One scratch repository and what it must do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Case {
    pub name: &'static str,
    /// The gated module after the change (equal to [`BASE`]: no change).
    pub head: &'static str,
    /// `Some(scope)`: run as the pull-request job does, and require
    /// [`mutants_scope::scope`] to choose it (`"diff"` or `"full"`). `None`:
    /// a full run, as the weekly job does.
    pub pr_scope: Option<&'static str>,
    pub expect: Expect,
}

/// The self-test cases.
pub const CASES: &[Case] = &[
    Case {
        name: "control",
        head: BASE,
        pr_scope: None,
        expect: Expect::Pass,
    },
    Case {
        name: "weakened-test",
        head: r"//! mutants-selftest: a module at a gated path.

/// True for a negative reading.
pub fn is_negative(x: i32) -> bool {
    x < 0
}

#[cfg(test)]
mod tests {
    use super::is_negative;

    #[test]
    fn sign() {
        assert!(is_negative(-1));
        assert!(!is_negative(1));
    }
}
",
        pr_scope: Some("full"),
        // `x <= 0` passes once `is_negative(0)` is no longer asserted.
        expect: Expect::Missed("replace < with <= in is_negative"),
    },
    Case {
        name: "untested-change",
        head: r"//! mutants-selftest: a module at a gated path.

/// True for a negative reading.
pub fn is_negative(x: i32) -> bool {
    x < 0
}

/// True for a zero reading.
pub fn is_zero(x: i32) -> bool {
    x == 0
}

#[cfg(test)]
mod tests {
    use super::is_negative;

    #[test]
    fn sign() {
        assert!(is_negative(-1));
        assert!(!is_negative(0));
        assert!(!is_negative(1));
    }
}
",
        pr_scope: Some("diff"),
        expect: Expect::Missed("is_zero"),
    },
];

/// The outcome of one case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub case: &'static str,
    pub dir: PathBuf,
    /// The pull-request scope chosen (`None` for a weekly-style run).
    pub scope: Option<Scope>,
    pub exit: Option<i32>,
    pub missed: Vec<String>,
    pub caught: usize,
    /// Empty when the case behaved as required.
    pub problems: Vec<String>,
}

/// Judges a case's run: `exit` is cargo-mutants' exit code, `missed` the
/// lines of `mutants.out/missed.txt`, `caught` the count in `caught.txt`.
#[must_use]
pub fn evaluate(
    case: &Case,
    scope: Option<&Scope>,
    exit: Option<i32>,
    missed: &[String],
    caught: usize,
) -> Vec<String> {
    let mut p = Vec::new();
    if scope.map(Scope::word) != case.pr_scope {
        p.push(format!(
            "pull-request scope is {scope:?}, expected {:?}",
            case.pr_scope
        ));
    }
    match case.expect {
        Expect::Pass => {
            if exit != Some(0) {
                p.push(format!("cargo mutants exited {exit:?}, expected 0"));
            }
            if caught == 0 {
                p.push(format!(
                    "no mutant was caught: {GATED_FILE} is not gated by .cargo/mutants.toml, or the run tested nothing"
                ));
            }
            if !missed.is_empty() {
                p.push(format!("mutants survived the full tests: {missed:?}"));
            }
        }
        Expect::Missed(name) => {
            if exit != Some(2) {
                p.push(format!(
                    "cargo mutants exited {exit:?}, expected 2 (missed mutants): the job would not fail"
                ));
            }
            if !missed
                .iter()
                .any(|m| m.contains(GATED_FILE) && m.contains(name))
            {
                p.push(format!(
                    "no MISSED mutant in {GATED_FILE} matching {name:?} (missed: {missed:?})"
                ));
            }
        }
    }
    p
}

fn run(dir: &Path, program: &str, args: &[&str]) -> Result<(Option<i32>, String)> {
    let out = Command::new(program)
        .args(args)
        .current_dir(dir)
        .output()
        .with_context(|| format!("running {program} {}", args.join(" ")))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok((out.status.code(), text))
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let mut all = vec![
        "-c",
        "user.name=mutants-selftest",
        "-c",
        "user.email=mutants-selftest@invalid",
        "-c",
        "commit.gpgsign=false",
    ];
    all.extend_from_slice(args);
    let (code, out) = run(dir, "git", &all)?;
    if code != Some(0) {
        bail!("git {} failed in {}:\n{out}", args.join(" "), dir.display());
    }
    Ok(out)
}

fn write(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

fn lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

/// Builds and runs one case under `scratch`.
fn run_case(root: &Path, scratch: &Path, case: &Case) -> Result<Report> {
    let dir = scratch.join(case.name);
    if dir.exists() {
        fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
    }
    for file in [".cargo/mutants.toml", "rust-toolchain.toml"] {
        let text = fs::read_to_string(root.join(file))
            .with_context(|| format!("reading {}", root.join(file).display()))?;
        write(&dir.join(file), &text)?;
    }
    write(
        &dir.join("Cargo.toml"),
        "[workspace]\nresolver = \"3\"\nmembers = [\"crates/rr-core\"]\n",
    )?;
    write(
        &dir.join("crates/rr-core/Cargo.toml"),
        "[package]\nname = \"mutants-selftest\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n",
    )?;
    write(
        &dir.join("crates/rr-core/src/lib.rs"),
        "pub mod encoding;\n",
    )?;
    write(&dir.join(".gitignore"), "/target/\n/mutants.out*/\n")?;
    write(&dir.join(GATED_FILE), BASE)?;
    git(&dir, &["init", "--quiet"])?;
    git(&dir, &["add", "--all"])?;
    git(&dir, &["commit", "--quiet", "--message", "base"])?;
    let base = git(&dir, &["rev-parse", "HEAD"])?.trim().to_owned();
    write(&dir.join(GATED_FILE), case.head)?;
    git(&dir, &["add", "--all"])?;
    git(
        &dir,
        &["commit", "--quiet", "--allow-empty", "--message", "change"],
    )?;

    let range = format!("{base}...HEAD");
    let mut diff_args = DIFF_ARGS.to_vec();
    diff_args.push(&range);
    let diff = git(&dir, &diff_args)?;
    write(&dir.join("pr.diff"), &diff)?;
    let scope = case
        .pr_scope
        .map(|_| mutants_scope::scope(&diff, |p| fs::read_to_string(dir.join(p)).ok()));

    let mut args = vec!["mutants", "--colors", "never", "--output", "."];
    if scope == Some(Scope::Diff) {
        args.extend(["--in-diff", "pr.diff"]);
    }
    let (exit, _output) = run(&dir, "cargo", &args)?;
    let out = dir.join("mutants.out");
    let missed = lines(&out.join("missed.txt"));
    let caught = lines(&out.join("caught.txt")).len();
    let problems = evaluate(case, scope.as_ref(), exit, &missed, caught);
    Ok(Report {
        case: case.name,
        dir,
        scope,
        exit,
        missed,
        caught,
        problems,
    })
}

/// Runs every case against the configuration of the workspace at `root`.
///
/// # Errors
///
/// When a scratch repository cannot be built or a tool cannot be started.
/// A case that misbehaves is a [`Report::problems`] entry, not an error.
pub fn run_all(root: &Path) -> Result<Vec<Report>> {
    let scratch = root.join(SCRATCH_DIR);
    CASES.iter().map(|c| run_case(root, &scratch, c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(name: &str) -> &'static Case {
        CASES.iter().find(|c| c.name == name).unwrap()
    }

    #[test]
    fn cases_change_what_they_say() {
        assert_eq!(case("control").head, BASE);
        assert!(
            !case("weakened-test")
                .head
                .contains("assert!(!is_negative(0))")
        );
        assert!(case("untested-change").head.contains("fn is_zero"));
    }

    #[test]
    fn evaluate_requires_the_planted_survivor_and_exit_2() {
        let c = case("weakened-test");
        let full = Scope::Full(vec!["x".into()]);
        let missed = vec![format!(
            "{GATED_FILE}:5:7: replace < with <= in is_negative"
        )];
        let full = Some(&full);
        assert!(evaluate(c, full, Some(2), &missed, 4).is_empty());
        assert!(evaluate(c, full, Some(0), &missed, 4)[0].contains("would not fail"));
        assert!(evaluate(c, full, Some(2), &[], 5)[0].contains("no MISSED mutant"));
        assert!(
            evaluate(c, Some(&Scope::Diff), Some(2), &missed, 4)[0].contains("scope is Some(Diff)")
        );
    }

    #[test]
    fn evaluate_control_needs_caught_and_no_missed() {
        let c = case("control");
        assert!(evaluate(c, None, Some(0), &[], 5).is_empty());
        assert!(evaluate(c, None, Some(0), &[], 0)[0].contains("not gated"));
        assert!(!evaluate(c, None, Some(2), &["m".into()], 5).is_empty());
    }
}
