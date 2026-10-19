//! PR-gate self-test (`cargo xtask gates-selftest [--target <triple>]`):
//! proves the `fmt`, `clippy` and `nextest` commands CI runs actually fail
//! on a rustfmt violation, a clippy warning and a failing test.
//!
//! Proves US-RR-2-1: "A PR with a rustfmt violation, a clippy warning, or a
//! failing test is blocked on every matrix target". Reading `ci.yml` proves
//! the commands are wired as hard gates (`xtask/tests/ci_config_gates.rs`);
//! this proves what those commands do with a bad change, for real, the local
//! and repeatable equivalent of pushing a throwaway PR. CI runs it in every
//! entry of the `test` matrix with `--target ${{ matrix.target }}`, so the
//! per-target clippy and nextest commands are exercised on each target.
//!
//! It generates one stand-alone scratch crate per [`CASES`] entry under
//! `<root>/target/gates-selftest/<case>/`, configured like the real
//! workspace: the root `[workspace.lints]` table copied verbatim into the
//! crate's `[lints]`, plus byte-for-byte copies of `clippy.toml`,
//! `rust-toolchain.toml` and any `rustfmt.toml` / `.config/nextest.toml`. On
//! each it runs every [`Gate`] with exactly the arguments CI uses
//! ([`Gate::ci_args`]; `ci_config_gates.rs` keeps `ci.yml` in step with them)
//! and requires:
//!
//! * **control** (clean crate, one passing test): every gate passes, so the
//!   harness does not simply fail everything;
//! * **fmt** (one misformatted line): `fmt` fails reporting a diff in the
//!   planted file; clippy and nextest pass;
//! * **clippy** (a `clippy::needless_return`, a default `warn` lint, not a
//!   rustc warning): `clippy` fails naming that lint; fmt and nextest pass;
//! * **test** (one failing `#[test]`): `nextest` fails reporting that test as
//!   FAIL; fmt and clippy pass.
//!
//! A gate that fails without the planted cause in its output (a missing tool,
//! a broken harness) is a self-test failure, never a "proved" failure.
//! Weakening a command (`cargo fmt` without `--check`, clippy without
//! `-D warnings`, lowering `clippy::all` in `[workspace.lints]`) therefore
//! makes this fail with "passed ... CI would not block it".
//!
//! Offline: the scratch crates have no dependencies.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail};

/// Scratch directory, relative to the workspace root (inside `target/`, so it
/// is never committed and never scanned by the other xtask checks).
pub const SCRATCH_DIR: &str = "target/gates-selftest";

/// A PR gate: one cargo command CI runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// `cargo fmt --all --check` (target-independent: rustfmt ignores `cfg`).
    Fmt,
    /// `cargo clippy --workspace --all-targets --locked [--target T] -- -D warnings`.
    Clippy,
    /// `cargo nextest run --workspace --locked [--target T]`.
    Nextest,
}

impl Gate {
    /// Every gate, in the order the self-test runs them.
    pub const ALL: [Self; 3] = [Self::Fmt, Self::Clippy, Self::Nextest];

    /// Short name used in reports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Fmt => "fmt",
            Self::Clippy => "clippy",
            Self::Nextest => "nextest",
        }
    }

    /// The arguments (after `cargo`) CI runs this gate with, for `target`
    /// (`None` = the host, as in the `fmt` and `clippy` jobs). `fmt` takes no
    /// target.
    #[must_use]
    pub fn ci_args(self, target: Option<&str>) -> Vec<String> {
        let with_target = |base: &[&str], tail: &[&str]| {
            let mut v: Vec<String> = base.iter().map(|s| (*s).to_owned()).collect();
            if let Some(t) = target {
                v.push("--target".to_owned());
                v.push(t.to_owned());
            }
            v.extend(tail.iter().map(|s| (*s).to_owned()));
            v
        };
        match self {
            Self::Fmt => ["fmt", "--all", "--check"].map(str::to_owned).to_vec(),
            Self::Clippy => with_target(
                &["clippy", "--workspace", "--all-targets", "--locked"],
                &["--", "-D", "warnings"],
            ),
            Self::Nextest => with_target(&["nextest", "run", "--workspace", "--locked"], &[]),
        }
    }

    /// The CI command line for `target`, as `ci.yml` spells it.
    #[must_use]
    pub fn ci_command(self, target: Option<&str>) -> String {
        format!("cargo {}", self.ci_args(target).join(" "))
    }
}

impl fmt::Display for Gate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One scratch crate and what it must do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Case {
    /// Case name, also the scratch sub-directory and package-name suffix.
    pub name: &'static str,
    /// Contents of `src/lib.rs`.
    pub lib_rs: &'static str,
    /// The gate that must fail, and text its output must contain to prove
    /// it failed on the planted violation. `None`: every gate passes.
    pub fails: Option<(Gate, &'static str)>,
}

/// The test name planted in the `test` case.
pub const FAILING_TEST: &str = "planted_failing_test";

/// The self-test cases.
pub const CASES: &[Case] = &[
    Case {
        name: "control",
        lib_rs: r"//! gates-selftest control: passes every gate.

/// Adds two numbers.
pub fn add(a: u32, b: u32) -> u32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::add;

    #[test]
    fn adds() {
        assert_eq!(add(2, 2), 4);
    }
}
",
        fails: None,
    },
    Case {
        name: "fmt",
        lib_rs: r"//! gates-selftest fmt: one misformatted line.

/// Adds two numbers.
pub fn add(a:u32,b:u32)->u32{a+b}

#[cfg(test)]
mod tests {
    use super::add;

    #[test]
    fn adds() {
        assert_eq!(add(2, 2), 4);
    }
}
",
        fails: Some((Gate::Fmt, "Diff in")),
    },
    Case {
        name: "clippy",
        lib_rs: r"//! gates-selftest clippy: one clippy warning (not a rustc warning).

/// Adds two numbers.
pub fn add(a: u32, b: u32) -> u32 {
    return a + b;
}

#[cfg(test)]
mod tests {
    use super::add;

    #[test]
    fn adds() {
        assert_eq!(add(2, 2), 4);
    }
}
",
        fails: Some((Gate::Clippy, "clippy::needless_return")),
    },
    Case {
        name: "test",
        lib_rs: r"//! gates-selftest test: one failing test.

/// Adds two numbers.
pub fn add(a: u32, b: u32) -> u32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::add;

    #[test]
    fn planted_failing_test() {
        assert_eq!(add(2, 2), 5);
    }
}
",
        fails: Some((Gate::Nextest, FAILING_TEST)),
    },
];

/// Why a case did not behave as expected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// The gate passed on its planted violation: CI would not block the PR.
    GatePassed {
        /// Case name.
        case: &'static str,
        /// Gate that should have failed.
        gate: Gate,
    },
    /// The gate failed, but its output does not show the planted cause.
    WrongFailure {
        /// Case name.
        case: &'static str,
        /// Gate.
        gate: Gate,
        /// Text that was expected in the output.
        wanted: &'static str,
        /// Last lines of the output.
        tail: String,
    },
    /// A gate failed on a case that should pass it (the harness, or a
    /// violation bleeding into another gate, is the cause).
    UnexpectedFailure {
        /// Case name.
        case: &'static str,
        /// Gate.
        gate: Gate,
        /// Last lines of the output.
        tail: String,
    },
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GatePassed { case, gate } => write!(
                f,
                "{case}: `{gate}` passed on the planted violation, so CI would not block it"
            ),
            Self::WrongFailure {
                case,
                gate,
                wanted,
                tail,
            } => write!(
                f,
                "{case}: `{gate}` failed, but its output does not mention {wanted:?}:\n{tail}"
            ),
            Self::UnexpectedFailure { case, gate, tail } => {
                write!(f, "{case}: `{gate}` should pass but failed:\n{tail}")
            }
        }
    }
}

/// Judge one gate's run on one case: `success` is the command's exit status,
/// `output` its combined stdout and stderr.
#[must_use]
pub fn evaluate(case: &Case, gate: Gate, success: bool, output: &str) -> Option<Problem> {
    let output = strip_ansi(output);
    match case.fails {
        Some((g, wanted)) if g == gate => {
            if success {
                Some(Problem::GatePassed {
                    case: case.name,
                    gate,
                })
            } else if output.contains(wanted) {
                None
            } else {
                Some(Problem::WrongFailure {
                    case: case.name,
                    gate,
                    wanted,
                    tail: tail(&output),
                })
            }
        }
        _ if success => None,
        _ => Some(Problem::UnexpectedFailure {
            case: case.name,
            gate,
            tail: tail(&output),
        }),
    }
}

/// `s` without ANSI escape sequences (CI sets `CARGO_TERM_COLOR=always`).
#[must_use]
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn tail(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    lines[lines.len().saturating_sub(20)..].join("\n")
}

/// The scratch manifest for case `name`, with `lints` (the root
/// `[workspace.lints]` table) as its `[lints]`.
pub fn case_manifest(name: &str, edition: &toml::Value, lints: &toml::Value) -> Result<String> {
    let mut pkg = toml::Table::new();
    pkg.insert("name".to_owned(), format!("gates-selftest-{name}").into());
    pkg.insert("version".to_owned(), "0.0.0".into());
    pkg.insert("edition".to_owned(), edition.clone());
    pkg.insert("publish".to_owned(), false.into());
    let mut manifest = toml::Table::new();
    manifest.insert("package".to_owned(), toml::Value::Table(pkg));
    manifest.insert("lints".to_owned(), lints.clone());
    // Stand-alone: stop cargo from adopting the enclosing workspace.
    manifest.insert(
        "workspace".to_owned(),
        toml::Value::Table(toml::Table::new()),
    );
    Ok(format!(
        "# Generated by `cargo xtask gates-selftest` from the workspace root \
         Cargo.toml [workspace.lints]. Do not edit.\n{}",
        toml::to_string(&manifest)?
    ))
}

/// Files copied byte for byte from the root into every scratch crate, when
/// present (`clippy.toml` must be).
pub const COPIED: &[&str] = &[
    "clippy.toml",
    "rust-toolchain.toml",
    "rustfmt.toml",
    ".rustfmt.toml",
    ".config/nextest.toml",
];

/// Write `case`'s scratch crate for `root` into `dir`, replacing any old one
/// but keeping its `target/` (build cache).
pub fn write_case(root: &Path, dir: &Path, case: &Case) -> Result<()> {
    let manifest_path = root.join("Cargo.toml");
    let text = fs::read_to_string(&manifest_path)
        .with_context(|| format!("reading {}", manifest_path.display()))?;
    let manifest: toml::Table =
        toml::from_str(&text).with_context(|| format!("parsing {}", manifest_path.display()))?;
    let ws = manifest
        .get("workspace")
        .and_then(toml::Value::as_table)
        .context("root Cargo.toml has no [workspace]")?;
    let lints = ws
        .get("lints")
        .context("root Cargo.toml has no [workspace.lints]")?;
    let edition = ws
        .get("package")
        .and_then(|p| p.get("edition"))
        .cloned()
        .unwrap_or_else(|| "2024".into());

    if dir.exists() {
        for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let entry = entry?;
            if entry.file_name() == "target" {
                continue;
            }
            let p = entry.path();
            if entry.file_type()?.is_dir() {
                fs::remove_dir_all(&p)
            } else {
                fs::remove_file(&p)
            }
            .with_context(|| format!("removing {}", p.display()))?;
        }
    }
    write(
        &dir.join("Cargo.toml"),
        &case_manifest(case.name, &edition, lints)?,
    )?;
    write(&dir.join("src/lib.rs"), case.lib_rs)?;
    for rel in COPIED {
        let src = root.join(rel);
        if *rel == "clippy.toml" || src.is_file() {
            let dst = dir.join(rel);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
            fs::copy(&src, &dst).with_context(|| format!("copying {}", src.display()))?;
        }
    }
    Ok(())
}

fn write(path: &Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

/// One gate run on one case.
#[derive(Debug)]
pub struct GateRun {
    /// Gate.
    pub gate: Gate,
    /// Exit success.
    pub success: bool,
    /// Problem, if the gate did not behave as expected.
    pub problem: Option<Problem>,
}

/// Result of one case.
#[derive(Debug)]
pub struct CaseReport {
    /// The case.
    pub case: Case,
    /// Scratch crate.
    pub dir: PathBuf,
    /// Every gate's run, in [`Gate::ALL`] order.
    pub runs: Vec<GateRun>,
}

/// Generate every case's scratch crate under `root`, run every gate on each
/// (clippy and nextest with `--target target` when given), and judge the
/// results.
pub fn run(root: &Path, target: Option<&str>) -> Result<Vec<CaseReport>> {
    if !root.join("Cargo.toml").is_file() {
        bail!("{} is not a workspace root (no Cargo.toml)", root.display());
    }
    // Absolute (the gates run with the scratch crate as working directory)
    // but not canonical: on Windows that is a `\\?\` path, which some tools
    // mishandle as a working or target directory.
    let root =
        std::path::absolute(root).with_context(|| format!("resolving {}", root.display()))?;
    let root = root.as_path();
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut reports = Vec::new();
    for case in CASES {
        let dir = root.join(SCRATCH_DIR).join(case.name);
        write_case(root, &dir, case)?;
        let cmd = |args: &[String]| {
            let mut c = Command::new(&cargo);
            c.current_dir(&dir)
                .args(args)
                // Own target dir, so this never contends with the main build.
                .env("CARGO_TARGET_DIR", dir.join("target"))
                // Only the copied clippy.toml may configure clippy.
                .env_remove("CLIPPY_CONF_DIR")
                .env("CARGO_TERM_COLOR", "never");
            c
        };
        let lockfile = cmd(&["generate-lockfile".to_owned(), "--offline".to_owned()])
            .output()
            .context("running cargo generate-lockfile")?;
        if !lockfile.status.success() {
            bail!(
                "{}: cargo generate-lockfile failed:\n{}",
                case.name,
                String::from_utf8_lossy(&lockfile.stderr)
            );
        }
        let mut runs = Vec::new();
        for gate in Gate::ALL {
            let args = gate.ci_args(target);
            let output = cmd(&args)
                .output()
                .with_context(|| format!("{}: running cargo {}", case.name, args.join(" ")))?;
            let text = format!(
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let success = output.status.success();
            runs.push(GateRun {
                gate,
                success,
                problem: evaluate(case, gate, success, &text),
            });
        }
        reports.push(CaseReport {
            case: *case,
            dir,
            runs,
        });
    }
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(name: &str) -> &'static Case {
        CASES.iter().find(|c| c.name == name).unwrap()
    }

    #[test]
    fn ci_args_match_the_ci_commands() {
        assert_eq!(Gate::Fmt.ci_command(None), "cargo fmt --all --check");
        assert_eq!(
            Gate::Fmt.ci_command(Some("i686-pc-windows-msvc")),
            "cargo fmt --all --check"
        );
        assert_eq!(
            Gate::Clippy.ci_command(None),
            "cargo clippy --workspace --all-targets --locked -- -D warnings"
        );
        assert_eq!(
            Gate::Clippy.ci_command(Some("x86_64-pc-windows-msvc")),
            "cargo clippy --workspace --all-targets --locked --target x86_64-pc-windows-msvc -- -D warnings"
        );
        assert_eq!(
            Gate::Nextest.ci_command(Some("aarch64-unknown-linux-gnu")),
            "cargo nextest run --workspace --locked --target aarch64-unknown-linux-gnu"
        );
    }

    /// One control plus exactly one violation case per gate, each failing
    /// only its own gate.
    #[test]
    fn cases_cover_each_gate_once_plus_a_control() {
        assert_eq!(CASES.iter().filter(|c| c.fails.is_none()).count(), 1);
        for gate in Gate::ALL {
            assert_eq!(
                CASES
                    .iter()
                    .filter(|c| c.fails.is_some_and(|(g, _)| g == gate))
                    .count(),
                1,
                "{gate}"
            );
        }
        // The planted violations differ from the control only where planted.
        let control = case("control").lib_rs;
        assert!(
            case("fmt")
                .lib_rs
                .contains("pub fn add(a:u32,b:u32)->u32{a+b}")
        );
        assert!(case("clippy").lib_rs.contains("    return a + b;\n"));
        assert!(case("test").lib_rs.contains(&format!(
            "fn {FAILING_TEST}() {{\n        assert_eq!(add(2, 2), 5);"
        )));
        assert!(!control.contains("return") && !control.contains(FAILING_TEST));
    }

    #[test]
    fn violation_gate_passing_is_a_problem() {
        assert_eq!(
            evaluate(case("clippy"), Gate::Clippy, true, ""),
            Some(Problem::GatePassed {
                case: "clippy",
                gate: Gate::Clippy
            })
        );
        let p = evaluate(case("fmt"), Gate::Fmt, true, "").unwrap();
        assert!(p.to_string().contains("CI would not block it"), "{p}");
    }

    #[test]
    fn violation_gate_failing_for_the_planted_reason_passes() {
        let out =
            "error: unneeded `return` statement\n  = help: ... #[allow(clippy::needless_return)]";
        assert_eq!(evaluate(case("clippy"), Gate::Clippy, false, out), None);
        let out =
            "        FAIL [   0.003s] (1/1) gates-selftest-test tests::planted_failing_test\n";
        assert_eq!(evaluate(case("test"), Gate::Nextest, false, out), None);
        let out = "Diff in /x/target/gates-selftest/fmt/src/lib.rs:1:\n";
        assert_eq!(evaluate(case("fmt"), Gate::Fmt, false, out), None);
    }

    #[test]
    fn failure_without_the_planted_cause_is_a_problem() {
        let out = "error: no such command: `nextest`";
        assert!(matches!(
            evaluate(case("test"), Gate::Nextest, false, out),
            Some(Problem::WrongFailure { .. })
        ));
    }

    #[test]
    fn other_gates_failing_on_a_case_is_a_problem() {
        assert!(matches!(
            evaluate(case("control"), Gate::Nextest, false, "boom"),
            Some(Problem::UnexpectedFailure { .. })
        ));
        assert!(matches!(
            evaluate(case("fmt"), Gate::Clippy, false, "Diff in"),
            Some(Problem::UnexpectedFailure { .. })
        ));
        assert_eq!(evaluate(case("fmt"), Gate::Nextest, true, ""), None);
    }

    #[test]
    fn ansi_colour_does_not_hide_the_planted_cause() {
        let out = "\u{1b}[1m\u{1b}[31m        FAIL\u{1b}[0m [ 0.003s] tests::\u{1b}[36mplanted_failing_test\u{1b}[0m";
        assert_eq!(
            strip_ansi(out),
            "        FAIL [ 0.003s] tests::planted_failing_test"
        );
        assert_eq!(evaluate(case("test"), Gate::Nextest, false, out), None);
    }

    #[test]
    fn scratch_crate_inherits_root_workspace_lints_verbatim() {
        let dir =
            std::env::temp_dir().join(format!("xtask-gates-selftest-unit-{}", std::process::id()));
        let root = dir.join("root");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = []\n[workspace.package]\nedition = \"2024\"\n\
             [workspace.lints.clippy]\nall = { level = \"warn\", priority = -1 }\n",
        )
        .unwrap();
        fs::write(root.join("clippy.toml"), "allow-unwrap-in-tests = true\n").unwrap();
        let scratch = dir.join("scratch");
        write_case(&root, &scratch, case("clippy")).unwrap();
        let m: toml::Table =
            toml::from_str(&fs::read_to_string(scratch.join("Cargo.toml")).unwrap()).unwrap();
        assert_eq!(m["lints"]["clippy"]["all"]["level"].as_str(), Some("warn"));
        assert_eq!(
            m["lints"]["clippy"]["all"]["priority"].as_integer(),
            Some(-1)
        );
        assert_eq!(m["package"]["name"].as_str(), Some("gates-selftest-clippy"));
        assert!(m["workspace"].as_table().is_some_and(toml::Table::is_empty));
        assert_eq!(
            fs::read_to_string(scratch.join("clippy.toml")).unwrap(),
            "allow-unwrap-in-tests = true\n"
        );
        assert_eq!(
            fs::read_to_string(scratch.join("src/lib.rs")).unwrap(),
            case("clippy").lib_rs
        );
        assert!(!scratch.join("rustfmt.toml").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
