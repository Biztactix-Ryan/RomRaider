//! cargo-deny self-test (`cargo xtask deny-selftest`): proves the `deny` gate
//! actually fails the build on a bad dependency.
//!
//! Proves US-RR-2-2: "cargo-deny fails the build for a dependency with a
//! GPL-incompatible licence or an open RUSTSEC advisory". Reading `deny.toml`
//! proves nothing about what cargo-deny does with it, so this runs cargo-deny
//! for real, the local, repeatable equivalent of pushing a throwaway branch.
//!
//! It generates one scratch workspace per [`CASES`] entry under
//! `<root>/target/deny-selftest/<case>/`. Each is a single private package
//! with exactly one registry dependency, pinned with `=`, next to a
//! byte-for-byte copy of the real `deny.toml`. It then runs the CI `deny`
//! job's command (`cargo deny --locked check`, every check, plus
//! `--format json` so the result can be read) and requires:
//!
//! * **control** (a crate this workspace already depends on, at the version
//!   in the real `Cargo.lock`): cargo-deny passes with no errors, so the
//!   harness itself does not fail everything;
//! * **licence** (`skaidb-types`, SSPL-1.0, which is incompatible with
//!   GPL-2.0-or-later, D-016): cargo-deny exits non-zero, the `licenses`
//!   check has errors, and a `rejected` licence error names that crate;
//! * **advisory** (`smallvec 1.6.0`, RUSTSEC-2021-0003, a heap buffer
//!   overflow fixed in 1.6.1): cargo-deny exits non-zero, the `advisories`
//!   check has errors, and a `vulnerability` error for RUSTSEC-2021-0003
//!   names that crate.
//!
//! A run that never got as far as checking (no summary in the output, e.g.
//! no network for the crates.io index or the advisory database) is a
//! self-test failure, never a "proved" failure. Loosening `deny.toml` (adding
//! SSPL-1.0 to the allowlist, ignoring the advisory, `version = 1` advisory
//! levels) therefore makes this fail with "cargo-deny passed".
//!
//! Needs network access (crates.io index, RustSec advisory database).

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail};

/// Scratch directory, relative to the workspace root (inside `target/`, so it
/// is never committed and never scanned by the other xtask checks).
pub const SCRATCH_DIR: &str = "target/deny-selftest";

/// The cargo-deny invocation of the CI `deny` job (after `cargo`). The
/// self-test adds `--format json` before `check` to read the result.
pub const CI_ARGS: &[&str] = &["deny", "--locked", "check"];

/// The crate the control case depends on, at the version locked in the real
/// workspace `Cargo.lock` (so it is a dependency CI already accepts).
pub const CONTROL_CRATE: &str = "anyhow";

/// What a case's dependency must make cargo-deny do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    /// cargo-deny passes with no errors in any check.
    Pass,
    /// cargo-deny fails, with an error from `check` coded `code` on `krate`.
    Fail {
        /// cargo-deny check that must report the error (`licenses`, `advisories`, ...).
        check: &'static str,
        /// Diagnostic code (`rejected`, `vulnerability`, ...).
        code: &'static str,
        /// RustSec advisory id the error must carry, if any.
        advisory: Option<&'static str>,
    },
}

/// One scratch workspace: a single dependency and the expected outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Case {
    /// Case name, also the scratch sub-directory.
    pub name: &'static str,
    /// Dependency crate name.
    pub krate: &'static str,
    /// Exact version (written as `="<version>"`); `None` means the version
    /// in the real `Cargo.lock`.
    pub version: Option<&'static str>,
    /// Expected outcome.
    pub expect: Expect,
}

/// The self-test cases.
pub const CASES: &[Case] = &[
    Case {
        name: "control",
        krate: CONTROL_CRATE,
        version: None,
        expect: Expect::Pass,
    },
    Case {
        name: "licence",
        // SSPL-1.0; its own dependencies (serde, serde_json, thiserror) are
        // on the allowlist, so the licence failure is attributable to it.
        krate: "skaidb-types",
        version: Some("0.308.0"),
        expect: Expect::Fail {
            check: "licenses",
            code: "rejected",
            advisory: None,
        },
    },
    Case {
        name: "advisory",
        // No dependencies of its own.
        krate: "smallvec",
        version: Some("1.6.0"),
        expect: Expect::Fail {
            check: "advisories",
            code: "vulnerability",
            advisory: Some("RUSTSEC-2021-0003"),
        },
    },
];

/// Every check cargo-deny runs by default; a passing control must report no
/// error in any of them.
pub const CHECKS: &[&str] = &["advisories", "bans", "licenses", "sources"];

/// One diagnostic from cargo-deny's JSON output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    /// `error`, `warning`, `note`, `help`.
    pub severity: String,
    /// Diagnostic code (`rejected`, `vulnerability`, ...).
    pub code: String,
    /// Advisory id, for advisory diagnostics.
    pub advisory: Option<String>,
    /// Crates the diagnostic is about (`name@version`).
    pub krates: Vec<String>,
}

/// cargo-deny's result, as read from its JSON output.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DenyOutput {
    /// Every diagnostic.
    pub diags: Vec<Diag>,
    /// Error count per check, from the final summary; `None` if there was no
    /// summary (cargo-deny never got to checking).
    pub errors: Option<Vec<(String, u64)>>,
    /// Log lines at `ERROR` level (e.g. a failed `cargo metadata`).
    pub log_errors: Vec<String>,
}

/// Parse cargo-deny `--format json` output (one JSON object per line, on
/// stderr). Lines that are not JSON objects are ignored.
pub fn parse_output(text: &str) -> DenyOutput {
    let mut out = DenyOutput::default();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let fields = &v["fields"];
        match v["type"].as_str() {
            Some("diagnostic") => {
                let krates = fields["graphs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|g| {
                        let k = &g["Krate"];
                        Some(format!(
                            "{}@{}",
                            k["name"].as_str()?,
                            k["version"].as_str()?
                        ))
                    })
                    .collect();
                out.diags.push(Diag {
                    severity: fields["severity"].as_str().unwrap_or("").to_owned(),
                    code: fields["code"].as_str().unwrap_or("").to_owned(),
                    advisory: fields["advisory"]["id"].as_str().map(str::to_owned),
                    krates,
                });
            }
            Some("summary") => {
                let errors = fields
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(check, counts)| (check.clone(), counts["errors"].as_u64().unwrap_or(0)))
                    .collect();
                out.errors = Some(errors);
            }
            Some("log") if fields["level"].as_str() == Some("ERROR") => {
                let msg = fields["message"].as_str().unwrap_or("");
                out.log_errors
                    .push(msg.lines().next().unwrap_or("").to_owned());
            }
            _ => {}
        }
    }
    out
}

/// Why a case did not behave as expected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// cargo-deny produced no summary: it never checked anything.
    NotChecked {
        /// Case name.
        case: &'static str,
        /// Error log lines, if any.
        log: Vec<String>,
    },
    /// A bad dependency passed (CI would have gone green).
    DenyPassed {
        /// Case name.
        case: &'static str,
    },
    /// cargo-deny failed, but not with the expected error.
    WrongFailure {
        /// Case name.
        case: &'static str,
        /// What was expected.
        wanted: String,
        /// Error diagnostics actually reported.
        got: Vec<String>,
    },
    /// The control failed: the harness, not the dependency, is the cause.
    ControlFailed {
        /// Case name.
        case: &'static str,
        /// Error diagnostics reported.
        got: Vec<String>,
    },
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotChecked { case, log } => write!(
                f,
                "{case}: cargo-deny did not complete its checks (network or config?): {}",
                if log.is_empty() {
                    "no summary".to_owned()
                } else {
                    log.join("; ")
                }
            ),
            Self::DenyPassed { case } => write!(
                f,
                "{case}: cargo-deny passed, so CI would accept this dependency"
            ),
            Self::WrongFailure { case, wanted, got } => write!(
                f,
                "{case}: cargo-deny failed, but not with {wanted}; errors: {got:?}"
            ),
            Self::ControlFailed { case, got } => write!(
                f,
                "{case}: the clean control failed cargo-deny; errors: {got:?}"
            ),
        }
    }
}

fn error_summaries(out: &DenyOutput) -> Vec<String> {
    out.diags
        .iter()
        .filter(|d| d.severity == "error")
        .map(|d| {
            let id = d
                .advisory
                .as_deref()
                .map_or(String::new(), |a| format!(" {a}"));
            format!("{}{id} {}", d.code, d.krates.join(","))
        })
        .collect()
}

/// Judge one case's cargo-deny run. `krate` is `name@version` of the case's
/// dependency; `success` is cargo-deny's exit status.
pub fn evaluate(case: &Case, krate: &str, success: bool, out: &DenyOutput) -> Vec<Problem> {
    let Some(errors) = &out.errors else {
        return vec![Problem::NotChecked {
            case: case.name,
            log: out.log_errors.clone(),
        }];
    };
    let errors_in = |check: &str| {
        errors
            .iter()
            .find(|(c, _)| c == check)
            .map_or(0, |(_, n)| *n)
    };
    match case.expect {
        Expect::Pass => {
            let total: u64 = errors.iter().map(|(_, n)| n).sum();
            if success && total == 0 && CHECKS.iter().all(|c| errors.iter().any(|(e, _)| e == c)) {
                Vec::new()
            } else {
                vec![Problem::ControlFailed {
                    case: case.name,
                    got: error_summaries(out),
                }]
            }
        }
        Expect::Fail {
            check,
            code,
            advisory,
        } => {
            if success {
                return vec![Problem::DenyPassed { case: case.name }];
            }
            let found = out.diags.iter().any(|d| {
                d.severity == "error"
                    && d.code == code
                    && d.krates.iter().any(|k| k == krate)
                    && (advisory.is_none() || d.advisory.as_deref() == advisory)
            });
            if found && errors_in(check) > 0 {
                Vec::new()
            } else {
                vec![Problem::WrongFailure {
                    case: case.name,
                    wanted: format!(
                        "a `{check}` error `{code}`{} on {krate}",
                        advisory.map_or(String::new(), |a| format!(" ({a})"))
                    ),
                    got: error_summaries(out),
                }]
            }
        }
    }
}

/// The version of `krate` in a `Cargo.lock`'s text.
pub fn locked_version(lock: &str, krate: &str) -> Option<String> {
    let lock: toml::Table = toml::from_str(lock).ok()?;
    lock.get("package")?.as_array()?.iter().find_map(|p| {
        (p.get("name")?.as_str()? == krate)
            .then(|| p.get("version")?.as_str().map(str::to_owned))?
    })
}

/// The scratch manifest for a case depending on `krate = "=<version>"`.
pub fn case_manifest(case: &str, krate: &str, version: &str) -> String {
    format!(
        "# Generated by `cargo xtask deny-selftest`. Do not edit.\n\
         [package]\n\
         name = \"deny-selftest-{case}\"\n\
         version = \"0.0.0\"\n\
         edition = \"2024\"\n\
         publish = false\n\
         \n\
         # Stand-alone: stop cargo from adopting the enclosing workspace.\n\
         [workspace]\n\
         \n\
         [dependencies]\n\
         {krate} = \"={version}\"\n"
    )
}

/// Write case `name`'s scratch workspace into `dir`, replacing any old one.
pub fn write_case(root: &Path, dir: &Path, name: &str, krate: &str, version: &str) -> Result<()> {
    if dir.exists() {
        fs::remove_dir_all(dir).with_context(|| format!("removing {}", dir.display()))?;
    }
    fs::create_dir_all(dir.join("src")).with_context(|| format!("creating {}", dir.display()))?;
    fs::write(dir.join("Cargo.toml"), case_manifest(name, krate, version))
        .with_context(|| format!("writing {}", dir.display()))?;
    fs::write(dir.join("src/lib.rs"), "//! deny-selftest scratch crate.\n")
        .with_context(|| format!("writing {}", dir.display()))?;
    let deny = root.join("deny.toml");
    fs::copy(&deny, dir.join("deny.toml"))
        .with_context(|| format!("copying {}", deny.display()))?;
    Ok(())
}

/// Result of one case.
#[derive(Debug)]
pub struct CaseReport {
    /// The case.
    pub case: Case,
    /// `name@version` of its dependency.
    pub krate: String,
    /// Scratch workspace.
    pub dir: PathBuf,
    /// cargo-deny exit success.
    pub success: bool,
    /// Error count per check.
    pub errors: Vec<(String, u64)>,
    /// Problems (empty = behaved as expected).
    pub problems: Vec<Problem>,
}

/// Generate every case's scratch workspace under `root`, run cargo-deny on
/// each, and judge the results.
pub fn run(root: &Path) -> Result<Vec<CaseReport>> {
    let lock_path = root.join("Cargo.lock");
    let lock = fs::read_to_string(&lock_path)
        .with_context(|| format!("reading {}", lock_path.display()))?;
    if !root.join("deny.toml").is_file() {
        bail!("{} has no deny.toml", root.display());
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut reports = Vec::new();
    for case in CASES {
        let version = match case.version {
            Some(v) => v.to_owned(),
            None => locked_version(&lock, case.krate)
                .with_context(|| format!("{} not found in {}", case.krate, lock_path.display()))?,
        };
        let dir = root.join(SCRATCH_DIR).join(case.name);
        write_case(root, &dir, case.name, case.krate, &version)?;

        let lockfile = Command::new(&cargo)
            .current_dir(&dir)
            .args(["generate-lockfile"])
            .output()
            .context("running cargo generate-lockfile")?;
        if !lockfile.status.success() {
            bail!(
                "{}: cargo generate-lockfile failed:\n{}",
                case.name,
                String::from_utf8_lossy(&lockfile.stderr)
            );
        }

        let (head, tail) = CI_ARGS.split_at(CI_ARGS.len() - 1);
        let output = Command::new(&cargo)
            .current_dir(&dir)
            .args(head)
            .args(["--format", "json"])
            .args(tail)
            .output()
            .context("running cargo deny (is cargo-deny installed?)")?;
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
        let parsed = parse_output(&text);
        let krate = format!("{}@{version}", case.krate);
        let problems = evaluate(case, &krate, output.status.success(), &parsed);
        reports.push(CaseReport {
            case: *case,
            krate,
            dir,
            success: output.status.success(),
            errors: parsed.errors.unwrap_or_default(),
            problems,
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

    fn summary(lic: u64, adv: u64) -> String {
        format!(
            r#"{{"type":"summary","fields":{{"advisories":{{"errors":{adv}}},"bans":{{"errors":0}},"licenses":{{"errors":{lic}}},"sources":{{"errors":0}}}}}}"#
        )
    }

    const REJECTED: &str = r#"{"type":"diagnostic","fields":{"severity":"error","code":"rejected","message":"failed to satisfy license requirements","graphs":[{"Krate":{"name":"skaidb-types","version":"0.308.0"},"parents":[]}]}}"#;
    const VULN: &str = r#"{"type":"diagnostic","fields":{"severity":"error","code":"vulnerability","advisory":{"id":"RUSTSEC-2021-0003"},"graphs":[{"Krate":{"name":"smallvec","version":"1.6.0"}}]}}"#;

    #[test]
    fn cases_cover_control_licence_and_advisory() {
        assert!(matches!(case("control").expect, Expect::Pass));
        assert!(matches!(
            case("licence").expect,
            Expect::Fail {
                check: "licenses",
                ..
            }
        ));
        assert!(matches!(
            case("advisory").expect,
            Expect::Fail {
                check: "advisories",
                advisory: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn parses_diagnostics_summary_and_logs() {
        let text = format!(
            "not json\n{REJECTED}\n{VULN}\n{}\n{}",
            r#"{"type":"log","fields":{"level":"ERROR","message":"failed to fetch crates\nmore"}}"#,
            summary(1, 1)
        );
        let out = parse_output(&text);
        assert_eq!(out.diags.len(), 2);
        assert_eq!(out.diags[0].krates, vec!["skaidb-types@0.308.0".to_owned()]);
        assert_eq!(out.diags[1].advisory.as_deref(), Some("RUSTSEC-2021-0003"));
        assert_eq!(out.log_errors, vec!["failed to fetch crates".to_owned()]);
        let errors = out.errors.unwrap();
        assert!(errors.contains(&("licenses".to_owned(), 1)));
        assert!(errors.contains(&("advisories".to_owned(), 1)));
    }

    #[test]
    fn expected_failures_pass_the_selftest() {
        let lic = parse_output(&format!("{REJECTED}\n{}", summary(1, 0)));
        assert_eq!(
            evaluate(case("licence"), "skaidb-types@0.308.0", false, &lic),
            vec![]
        );
        let adv = parse_output(&format!("{VULN}\n{}", summary(0, 1)));
        assert_eq!(
            evaluate(case("advisory"), "smallvec@1.6.0", false, &adv),
            vec![]
        );
        let ok = parse_output(&summary(0, 0));
        assert_eq!(evaluate(case("control"), "anyhow@1.0.0", true, &ok), vec![]);
    }

    #[test]
    fn bad_dependency_that_passes_fails_the_selftest() {
        let out = parse_output(&summary(0, 0));
        assert_eq!(
            evaluate(case("licence"), "skaidb-types@0.308.0", true, &out),
            vec![Problem::DenyPassed { case: "licence" }]
        );
    }

    #[test]
    fn failure_for_another_reason_fails_the_selftest() {
        // Advisory error where a licence rejection was expected.
        let out = parse_output(&format!("{VULN}\n{}", summary(0, 1)));
        let p = evaluate(case("licence"), "skaidb-types@0.308.0", false, &out);
        assert!(
            matches!(p.as_slice(), [Problem::WrongFailure { .. }]),
            "{p:?}"
        );
        // Right code, wrong advisory id.
        let other = VULN.replace("RUSTSEC-2021-0003", "RUSTSEC-2099-0001");
        let out = parse_output(&format!("{other}\n{}", summary(0, 1)));
        let p = evaluate(case("advisory"), "smallvec@1.6.0", false, &out);
        assert!(
            matches!(p.as_slice(), [Problem::WrongFailure { .. }]),
            "{p:?}"
        );
    }

    #[test]
    fn unchecked_run_is_never_a_proved_failure() {
        let out = parse_output(
            r#"{"type":"log","fields":{"level":"ERROR","message":"`cargo metadata` exited with an error"}}"#,
        );
        let p = evaluate(case("advisory"), "smallvec@1.6.0", false, &out);
        assert!(
            matches!(p.as_slice(), [Problem::NotChecked { .. }]),
            "{p:?}"
        );
    }

    #[test]
    fn failing_control_fails_the_selftest() {
        let out = parse_output(&format!("{REJECTED}\n{}", summary(1, 0)));
        let p = evaluate(case("control"), "anyhow@1.0.0", false, &out);
        assert!(
            matches!(p.as_slice(), [Problem::ControlFailed { .. }]),
            "{p:?}"
        );
    }

    #[test]
    fn locked_version_reads_cargo_lock() {
        let lock = "version = 4\n\n[[package]]\nname = \"anyhow\"\nversion = \"1.0.99\"\n\n[[package]]\nname = \"b\"\nversion = \"2.0.0\"\n";
        assert_eq!(locked_version(lock, "anyhow").as_deref(), Some("1.0.99"));
        assert_eq!(locked_version(lock, "missing"), None);
    }

    #[test]
    fn scratch_case_pins_the_dependency_and_copies_deny_toml() {
        let dir =
            std::env::temp_dir().join(format!("xtask-deny-selftest-unit-{}", std::process::id()));
        let root = dir.join("root");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("deny.toml"), "[licenses]\nallow = [\"MIT\"]\n").unwrap();
        let case_dir = dir.join("case");
        write_case(&root, &case_dir, "advisory", "smallvec", "1.6.0").unwrap();
        let m: toml::Table =
            toml::from_str(&fs::read_to_string(case_dir.join("Cargo.toml")).unwrap()).unwrap();
        assert_eq!(m["dependencies"]["smallvec"].as_str(), Some("=1.6.0"));
        assert!(m.contains_key("workspace"));
        assert_eq!(m["package"]["publish"].as_bool(), Some(false));
        assert_eq!(
            fs::read_to_string(case_dir.join("deny.toml")).unwrap(),
            "[licenses]\nallow = [\"MIT\"]\n"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
