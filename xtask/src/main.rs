//! Workspace automation tasks (cargo xtask).

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use xtask::deps::{DepGraph, check};
use xtask::{deny_selftest, fixtures_selftest, globals, lint_selftest, writes};

#[derive(Parser)]
#[command(name = "xtask", about = "Workspace automation tasks")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Enforce the ARCHITECTURE.md crate dependency rule against `cargo metadata`.
    CheckDeps {
        /// Workspace manifest to check (defaults to this workspace).
        #[arg(long)]
        manifest_path: Option<PathBuf>,
    },
    /// Fail on `static mut` and global singletons holding mutable state (D-011, D-019).
    CheckGlobals {
        /// Workspace root to scan (defaults to this workspace).
        #[arg(long)]
        root: Option<PathBuf>,
        /// Allowlist file (defaults to <root>/xtask/globals-allowlist.toml).
        #[arg(long)]
        allowlist: Option<PathBuf>,
    },
    /// Run clippy on a generated fixture of planted violations, configured
    /// from this workspace's lints and clippy.toml, and fail unless every
    /// safety lint fires exactly where planted (US-RR-3-1, US-RR-3-2).
    LintSelftest {
        /// Workspace root whose lint configuration is tested (defaults to this workspace).
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Run cargo-deny (the CI `deny` job's command) on generated scratch
    /// workspaces with this workspace's deny.toml, and fail unless an
    /// SSPL-licensed dependency and one with an open RustSec advisory are
    /// both rejected and a clean control passes (US-RR-2-2). Needs network.
    DenySelftest {
        /// Workspace root whose deny.toml is tested (defaults to this workspace).
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// T6 write gating: every #[rr_write] fn in rr-rom / rr-comms takes a
    /// capability token, and no public fn there looks like an unmarked write.
    /// Run cargo-nextest with RR_CORPUS_DIR / RR_FIXTURES_DIR removed and
    /// fail unless every corpus/private-tier test is reported as skipped
    /// (named after the variable to set), never passed, and fails when forced
    /// to run (US-RR-6-1).
    FixturesSelftest {
        /// Workspace root to test (defaults to this workspace).
        #[arg(long)]
        root: Option<PathBuf>,
    },
    CheckWrites {
        /// Workspace root to scan (defaults to this workspace).
        #[arg(long)]
        root: Option<PathBuf>,
        /// Also list every #[rr_write] function found.
        #[arg(long)]
        list: bool,
    },
}

fn main() -> Result<ExitCode> {
    match Cli::parse().cmd {
        Cmd::CheckDeps { manifest_path } => {
            let path = manifest_path.unwrap_or_else(|| {
                PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../Cargo.toml"))
            });
            let violations = check(&DepGraph::load(&path)?);
            if violations.is_empty() {
                println!("check-deps: ok");
                return Ok(ExitCode::SUCCESS);
            }
            for v in &violations {
                eprintln!("check-deps: {v}");
            }
            eprintln!("check-deps: {} violation(s)", violations.len());
            Ok(ExitCode::FAILURE)
        }
        Cmd::CheckGlobals { root, allowlist } => {
            let root =
                root.unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")));
            let allowlist = allowlist.unwrap_or_else(|| root.join(globals::DEFAULT_ALLOWLIST));
            let report = globals::run(&root, &allowlist)?;
            if report.problems.is_empty() {
                println!(
                    "check-globals: ok ({} files, {} allowlisted global(s))",
                    report.files,
                    report.findings.len()
                );
                return Ok(ExitCode::SUCCESS);
            }
            for p in &report.problems {
                eprintln!("check-globals: {p}");
            }
            eprintln!("check-globals: {} problem(s)", report.problems.len());
            Ok(ExitCode::FAILURE)
        }
        Cmd::LintSelftest { root } => {
            let root =
                root.unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")));
            let report = lint_selftest::run(&root)?;
            if report.problems.is_empty() {
                println!(
                    "lint-selftest: ok ({} planted violation(s) all reported, nothing else; fixture {})",
                    report.expected.len(),
                    report.fixture.display()
                );
                return Ok(ExitCode::SUCCESS);
            }
            for p in &report.problems {
                eprintln!("lint-selftest: {p}");
            }
            eprintln!(
                "lint-selftest: {} problem(s); fixture left in {}",
                report.problems.len(),
                report.fixture.display()
            );
            Ok(ExitCode::FAILURE)
        }
        Cmd::DenySelftest { root } => {
            let root =
                root.unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")));
            let reports = deny_selftest::run(&root)?;
            let mut failed = 0;
            for r in &reports {
                let errors: Vec<String> = r
                    .errors
                    .iter()
                    .filter(|(_, n)| *n > 0)
                    .map(|(c, n)| format!("{c}: {n}"))
                    .collect();
                let verdict = if r.success { "passed" } else { "failed" };
                if r.problems.is_empty() {
                    println!(
                        "deny-selftest: {} ({}): cargo-deny {verdict} as expected (errors: {})",
                        r.case.name,
                        r.krate,
                        if errors.is_empty() {
                            "none".to_owned()
                        } else {
                            errors.join(", ")
                        }
                    );
                }
                for p in &r.problems {
                    failed += 1;
                    eprintln!("deny-selftest: {p} (scratch: {})", r.dir.display());
                }
            }
            if failed == 0 {
                println!("deny-selftest: ok ({} case(s))", reports.len());
                return Ok(ExitCode::SUCCESS);
            }
            eprintln!("deny-selftest: {failed} problem(s)");
            Ok(ExitCode::FAILURE)
        }
        Cmd::FixturesSelftest { root } => {
            let root =
                root.unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")));
            let report = fixtures_selftest::run(&root)?;
            if report.problems.is_empty() {
                println!(
                    "fixtures-selftest: ok ({})",
                    fixtures_selftest::summary(&report)
                );
                return Ok(ExitCode::SUCCESS);
            }
            for p in &report.problems {
                eprintln!("fixtures-selftest: {p}");
            }
            eprintln!("fixtures-selftest: {} problem(s)", report.problems.len());
            Ok(ExitCode::FAILURE)
        }
        Cmd::CheckWrites { root, list } => {
            let root =
                root.unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")));
            let report = writes::run(&root)?;
            if list {
                for m in &report.marked {
                    println!("check-writes: marked {m}");
                }
            }
            if report.problems.is_empty() {
                println!(
                    "check-writes: ok ({} files in {}, {} #[rr_write] fn(s), all token-gated)",
                    report.files,
                    writes::SCAN_CRATES.join(", "),
                    report.marked.len()
                );
                return Ok(ExitCode::SUCCESS);
            }
            for p in &report.problems {
                eprintln!("check-writes: {p}");
            }
            eprintln!("check-writes: {} problem(s)", report.problems.len());
            Ok(ExitCode::FAILURE)
        }
    }
}
