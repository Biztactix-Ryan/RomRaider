//! Workspace automation tasks (cargo xtask).

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context as _, Result};
use clap::{Parser, Subcommand};
use xtask::deps::{DepGraph, check};
use xtask::{
    codeowners, coverage_gate, deny_selftest, diagnostics_doc, fixtures_selftest, fuzz, fuzz_issue,
    gates_selftest, gen_synthetic, globals, lint_selftest, mutants_exclusions, mutants_scope,
    mutants_selftest, reference_isolation, validate_fixtures, writes,
};

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
    /// Fail if code reachable from rr-testkit::reference names rr-core (or any
    /// workspace crate), so the reference codec stays independent of the
    /// Encoding it cross-checks (US-RR-10, T2(b)).
    CheckReferenceIsolation {
        /// Workspace root to scan (defaults to this workspace).
        #[arg(long)]
        root: Option<PathBuf>,
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
    /// Run the CI fmt, clippy and nextest commands on generated scratch
    /// crates and fail unless a rustfmt violation, a clippy warning and a
    /// failing test each fail exactly their gate and a clean control passes
    /// all three (US-RR-2-1). Offline.
    GatesSelftest {
        /// Workspace root whose lint configuration is used (defaults to this workspace).
        #[arg(long)]
        root: Option<PathBuf>,
        /// Target triple for clippy and nextest, as in the CI `test` matrix
        /// (defaults to the host).
        #[arg(long)]
        target: Option<String>,
    },
    /// Compare the live branch protection, CODEOWNERS and CODEOWNERS errors
    /// of each protected branch on GitHub with CONTRIBUTING.md "Branch
    /// protection (maintainers)" and the local .github/CODEOWNERS
    /// (US-RR-8-2). Read-only `gh api` GETs; needs network and repo-admin
    /// read access, so it is opt-in and never part of the test suite.
    CheckBranchProtection {
        /// Workspace root (defaults to this workspace).
        #[arg(long)]
        root: Option<PathBuf>,
        /// Repository as owner/name (defaults to the one in CONTRIBUTING.md).
        #[arg(long)]
        repo: Option<String>,
        /// Branch to check; repeatable (defaults to the CONTRIBUTING.md loop).
        #[arg(long)]
        branch: Vec<String>,
    },
    /// Run cargo-nextest with RR_CORPUS_DIR / RR_FIXTURES_DIR removed and
    /// fail unless every corpus/private-tier test is reported as skipped
    /// (named after the variable to set), never passed, and fails when forced
    /// to run (US-RR-6-1).
    FixturesSelftest {
        /// Workspace root to test (defaults to this workspace).
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// T6 write gating: every #[rr_write] fn in rr-rom / rr-comms takes a
    /// capability token, and no public fn there looks like an unmarked write.
    CheckWrites {
        /// Workspace root to scan (defaults to this workspace).
        #[arg(long)]
        root: Option<PathBuf>,
        /// Also list every #[rr_write] function found.
        #[arg(long)]
        list: bool,
    },
    /// Regenerate docs/diagnostics.md from the diagnostic code registry
    /// (rr_core::diag::codes); with --check, fail instead if the registry
    /// breaks a rule or the committed file is out of date (US-RR-17-6).
    DiagnosticsDoc {
        /// Workspace root (defaults to this workspace).
        #[arg(long)]
        root: Option<PathBuf>,
        /// Only check; never write.
        #[arg(long)]
        check: bool,
    },
    /// Validate KAT fixtures (rr_testkit::kat::validate): every expected
    /// value has a source, hand-calc sources show their working, and, with
    /// --base, changed expected values have a new changelog entry (US-RR-9-6).
    /// Runs rr-testkit's validate-fixtures example (rr-testkit is
    /// dev-dependency only, so xtask cannot link it).
    ValidateFixtures {
        /// Compare each fixture with its version at the merge-base of this
        /// git revision and HEAD (CI: the PR's base branch).
        #[arg(long)]
        base: Option<String>,
        /// Fixture files or directories (defaults to fixtures/kat and
        /// crates/rr-testkit/tests/data/kat).
        paths: Vec<PathBuf>,
    },
    /// Regenerate the synthetic ROMs and manifests in fixtures/public/synthetic
    /// (plus SHA256SUMS) from rr-testkit's published plans; with --check,
    /// write nothing and fail on any byte difference from the committed files
    /// (US-RR-10-7). Runs rr-testkit's gen-synthetic example (rr-testkit is
    /// dev-dependency only, so xtask cannot link it).
    GenSynthetic {
        /// Only check; never write.
        #[arg(long)]
        check: bool,
        /// Directory to write or check (defaults to fixtures/public/synthetic).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Fail when a gated crate's line coverage in an lcov report (from
    /// `cargo llvm-cov nextest --lcov`) is below --min percent, or below its
    /// coverage in --base-lcov (US-RR-4-5; rules in xtask/src/coverage_gate.rs).
    CoverageGate {
        /// lcov report of the change under test.
        #[arg(long)]
        lcov: PathBuf,
        /// lcov report of the base revision; omit to skip the regression check.
        #[arg(long)]
        base_lcov: Option<PathBuf>,
        /// Minimum line coverage per crate, in percent.
        #[arg(long, default_value_t = coverage_gate::DEFAULT_MIN_PERCENT)]
        min: u32,
        /// Largest allowed drop below the base, in percentage points (at
        /// most two decimals; default 0: any drop fails). For measurement
        /// noise only.
        #[arg(long, default_value = "0", value_parser = coverage_gate::parse_points)]
        max_drop: u32,
        /// Comma-separated crates to gate (defaults to rr-core,rr-rom,rr-comms).
        #[arg(long, value_delimiter = ',')]
        crates: Vec<String>,
        /// Workspace root holding crates/<name>/src (defaults to this workspace).
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Fail unless every `exclude_globs` / `exclude_re` / `skip_calls` entry
    /// in .cargo/mutants.toml has a non-trivial justification comment on the
    /// line(s) directly above it (US-RR-4-7; rules in
    /// xtask/src/mutants_exclusions.rs).
    CheckMutantsExclusions {
        /// Workspace root (defaults to this workspace).
        #[arg(long)]
        root: Option<PathBuf>,
        /// File to check (defaults to <root>/.cargo/mutants.toml).
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Print `full` or `diff`: whether a pull request's mutation run must
    /// cover every gated module (its diff touches test code that can kill
    /// gated mutants) or only the changed code (`cargo mutants --in-diff`).
    /// Reasons go to stderr (US-RR-4-6; rules in xtask/src/mutants_scope.rs).
    MutantsScope {
        /// The pull request's `git diff <base>...HEAD`.
        #[arg(long)]
        diff: PathBuf,
        /// Workspace root the diff's paths are relative to (defaults to this workspace).
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Run cargo-mutants with this workspace's .cargo/mutants.toml on scratch
    /// repositories and fail unless a weakened test and an untested change
    /// in a gated module each leave a surviving mutant and exit 2, and a
    /// fully tested control passes (US-RR-4-6). Needs cargo-mutants,
    /// cargo-nextest and git.
    MutantsSelftest {
        /// Workspace root whose mutants.toml is tested (defaults to this workspace).
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Fuzz targets in fuzz/ (cargo-fuzz, nightly), always run with the
    /// per-input time and memory limits from fuzz/Cargo.toml
    /// (US-RR-5-5; docs/fuzzing.md).
    Fuzz {
        #[command(subcommand)]
        cmd: FuzzCmd,
    },
}

#[derive(Subcommand)]
enum FuzzCmd {
    /// List the fuzz targets and the limits applied to them.
    List {
        /// Print only the target names, as a JSON array (the nightly
        /// workflow's matrix, fuzz.yml).
        #[arg(long)]
        json: bool,
        /// Only this target (an error if it is not one); empty means all.
        #[arg(long)]
        only: Option<String>,
    },
    /// Check the fuzz crate layout: excluded from the workspace, limits set,
    /// every target has its [[bin]], fuzz_targets file, TARGETS entry and seeds.
    Check,
    /// `cargo +nightly fuzz run <target>` with the limits applied, the
    /// corpus in fuzz/corpus/<target> and the seeds in fuzz/seeds/<target>.
    /// Toolchain: $RR_FUZZ_TOOLCHAIN or `nightly`.
    Run {
        /// Target name (`cargo xtask fuzz list`).
        target: String,
        /// Stop after this many seconds (libFuzzer -max_total_time).
        #[arg(long)]
        max_total_time: Option<u64>,
        /// Fuzz-crate features to enable, comma-separated.
        #[arg(long, value_delimiter = ',')]
        features: Vec<String>,
        /// Run this input once instead of fuzzing (reproduce a crash); repeatable.
        #[arg(long)]
        input: Vec<PathBuf>,
        /// Where crash / timeout / oom inputs are written (libFuzzer -artifact_prefix).
        #[arg(long)]
        artifact_prefix: Option<PathBuf>,
        /// Further libFuzzer flags, after `--` (they come after the limits).
        #[arg(last = true)]
        libfuzzer_args: Vec<String>,
    },
    /// Replay every committed crash input in fuzz/regressions/<target>/
    /// through its target on stable (no nightly, no libFuzzer), each in a
    /// child process under the per-input timeout and memory caps, after a
    /// self-test that a planted panic, hang and memory blow-up each fail it
    /// (US-RR-5-7). Runs `cargo test --manifest-path fuzz/Cargo.toml --locked`.
    Replay {
        /// Fuzz-crate features to enable, comma-separated (e.g.
        /// `planted-panic`, which makes the committed toy input fail).
        #[arg(long, value_delimiter = ',')]
        features: Vec<String>,
    },
    /// Run the toy target's planted panic, hang, oversized allocation and
    /// unbounded memory growth under the limits and fail unless libFuzzer
    /// reports each one, and a clean input passes. Needs nightly and cargo-fuzz.
    Selftest,
    /// File the failures of a nightly fuzz run as GitHub issues labelled
    /// `fuzz` (US-RR-5-6): reads the `fuzz-failure-<target>/` directories
    /// fuzz.yml downloads, opens one issue per target or comments on the
    /// open one with the same title. Runs `gh` (GH_TOKEN, GH_REPO) unless
    /// --dry-run, which prints the `gh` calls and bodies instead.
    FileIssues {
        /// Directory holding the `fuzz-failure-<target>/` artifact directories.
        #[arg(long)]
        dir: PathBuf,
        /// URL of the workflow run, quoted in the issue.
        #[arg(long)]
        run_url: Option<String>,
        /// Print the `gh` calls and issue bodies; run nothing.
        #[arg(long)]
        dry_run: bool,
        /// With --dry-run: `gh issue list --json number,title` output to
        /// dedupe against (default: no open issues).
        #[arg(long, requires = "dry_run")]
        open_issues: Option<PathBuf>,
        /// Where issue bodies are written (default target/fuzz-issues).
        #[arg(long)]
        body_dir: Option<PathBuf>,
    },
}

fn workspace_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

fn fuzz_main(cmd: FuzzCmd) -> Result<ExitCode> {
    let root = workspace_root();
    let root = root.canonicalize().unwrap_or(root);
    match cmd {
        FuzzCmd::List { json, only } => {
            let m = fuzz::load(&root)?;
            let names = fuzz::select_targets(&m, only.as_deref())?;
            if json {
                println!("{}", serde_json::to_string(&names)?);
                return Ok(ExitCode::SUCCESS);
            }
            if let Some(l) = m.limits {
                println!("limits: {}", l.libfuzzer_args().join(" "));
            }
            for t in &names {
                println!("{t}");
            }
            Ok(ExitCode::SUCCESS)
        }
        FuzzCmd::FileIssues {
            dir,
            run_url,
            dry_run,
            open_issues,
            body_dir,
        } => file_issues(&root, &dir, run_url, dry_run, open_issues, body_dir),
        FuzzCmd::Check => {
            let problems = fuzz::check(&root)?;
            if problems.is_empty() {
                let m = fuzz::load(&root)?;
                println!(
                    "fuzz check: ok ({} target(s), limits {})",
                    m.targets.len(),
                    m.limits
                        .map(|l| l.libfuzzer_args().join(" "))
                        .unwrap_or_default()
                );
                return Ok(ExitCode::SUCCESS);
            }
            for p in &problems {
                eprintln!("fuzz check: {p}");
            }
            eprintln!("fuzz check: {} problem(s)", problems.len());
            Ok(ExitCode::FAILURE)
        }
        FuzzCmd::Run {
            target,
            max_total_time,
            features,
            input,
            artifact_prefix,
            libfuzzer_args,
        } => {
            let opts = fuzz::RunOptions {
                target,
                features,
                max_total_time,
                inputs: input,
                artifact_prefix,
                extra: libfuzzer_args,
            };
            let status = fuzz::run(&root, &opts)?.status().context(
                "fuzz: cannot run `cargo fuzz` (install: cargo install cargo-fuzz --locked)",
            )?;
            Ok(match status.code().and_then(|c| u8::try_from(c).ok()) {
                Some(0) => ExitCode::SUCCESS,
                Some(code) => ExitCode::from(code),
                None => ExitCode::FAILURE,
            })
        }
        FuzzCmd::Replay { features } => {
            let status = fuzz::replay_command(&root, &features)
                .status()
                .context("fuzz replay: cannot run cargo")?;
            Ok(if status.success() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        FuzzCmd::Selftest => {
            let reports = fuzz::selftest(&root)?;
            let mut failed = 0;
            for r in &reports {
                match &r.problem {
                    None => println!(
                        "fuzz selftest: {}: {} as expected",
                        r.case.name,
                        if r.success { "passed" } else { "reported" }
                    ),
                    Some(p) => {
                        failed += 1;
                        eprintln!("fuzz selftest: {p} (log: {})", r.log.display());
                    }
                }
            }
            if failed == 0 {
                println!(
                    "fuzz selftest: ok ({} case(s): a panic, a hang and runaway memory each fail the run)",
                    reports.len()
                );
                return Ok(ExitCode::SUCCESS);
            }
            eprintln!("fuzz selftest: {failed} problem(s)");
            Ok(ExitCode::FAILURE)
        }
    }
}

/// Runs `gh` with `call`'s arguments (no shell) and returns its stdout.
fn run_gh(call: &fuzz_issue::GhCall) -> Result<String> {
    let o = std::process::Command::new("gh")
        .args(&call.args)
        .stderr(std::process::Stdio::inherit())
        .output()
        .with_context(|| format!("cannot run `{call}` (is the GitHub CLI installed?)"))?;
    if !o.status.success() {
        anyhow::bail!("`{call}` failed ({})", o.status);
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

fn file_issues(
    root: &std::path::Path,
    dir: &std::path::Path,
    run_url: Option<String>,
    dry_run: bool,
    open_issues: Option<PathBuf>,
    body_dir: Option<PathBuf>,
) -> Result<ExitCode> {
    let m = fuzz::load(root)?;
    let targets: Vec<String> = m.targets.iter().map(|t| t.name.clone()).collect();
    let collected = fuzz_issue::collect(dir, &targets, &m.features)?;
    for w in &collected.warnings {
        eprintln!("fuzz file-issues: warning: {w}");
    }
    if collected.failures.is_empty() {
        println!("fuzz file-issues: no failure with a saved input; nothing to file");
        return Ok(ExitCode::SUCCESS);
    }
    let ctx = fuzz_issue::Context {
        run_url: run_url.filter(|u| !u.is_empty()),
        limits: m.limits,
    };
    let body_dir = body_dir.unwrap_or_else(|| root.join("target").join("fuzz-issues"));
    let list = fuzz_issue::list_call();
    let open_json = if dry_run {
        println!("+ {list}");
        match open_issues {
            Some(p) => {
                std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?
            }
            None => "[]".to_owned(),
        }
    } else {
        run_gh(&list)?
    };
    let filings = fuzz_issue::plan(&collected.failures, &ctx, &open_json, &body_dir)?;
    std::fs::create_dir_all(&body_dir)
        .with_context(|| format!("creating {}", body_dir.display()))?;
    let label = fuzz_issue::label_call();
    if dry_run {
        println!("+ {label}");
    } else {
        run_gh(&label)?;
    }
    for f in &filings {
        std::fs::write(&f.body_file, &f.body)
            .with_context(|| format!("writing {}", f.body_file.display()))?;
        if dry_run {
            println!("+ {}", f.call);
            println!("--- {} ---\n{}--- end ---", f.body_file.display(), f.body);
        } else {
            let out = run_gh(&f.call)?;
            match f.existing {
                Some(n) => println!("fuzz file-issues: {}: commented on #{n}", f.target),
                None => println!("fuzz file-issues: {}: opened {}", f.target, out.trim()),
            }
        }
    }
    println!(
        "fuzz file-issues: {} {} target(s)",
        if dry_run { "would file" } else { "filed" },
        filings.len()
    );
    Ok(ExitCode::SUCCESS)
}

fn gen_synthetic_main(check: bool, dir: Option<PathBuf>) -> Result<ExitCode> {
    let root = workspace_root();
    let root = root.canonicalize().unwrap_or(root);
    let dir = dir.unwrap_or_else(|| root.join(gen_synthetic::COMMITTED_DIR));
    let expected = gen_synthetic::expected(&gen_synthetic::generate(&root)?);
    for (name, bytes) in &expected {
        println!(
            "gen-synthetic: {} {name} ({} bytes)",
            gen_synthetic::sha256_hex(bytes),
            bytes.len()
        );
    }
    if !check {
        let changed = gen_synthetic::install(&dir, &expected)?;
        for c in &changed {
            println!("gen-synthetic: {c}");
        }
        println!(
            "gen-synthetic: {} ({} file(s) in {})",
            if changed.is_empty() {
                "unchanged"
            } else {
                "updated"
            },
            expected.len(),
            dir.display()
        );
        return Ok(ExitCode::SUCCESS);
    }
    let problems = gen_synthetic::compare(&expected, &gen_synthetic::read_files(&dir)?);
    if problems.is_empty() {
        println!(
            "gen-synthetic: ok ({} file(s) in {} are byte-identical to a fresh generation)",
            expected.len(),
            dir.display()
        );
        return Ok(ExitCode::SUCCESS);
    }
    for p in &problems {
        eprintln!("gen-synthetic: {p}");
    }
    eprintln!(
        "gen-synthetic: {} problem(s) in {}; run `cargo xtask gen-synthetic` and commit the result",
        problems.len(),
        dir.display()
    );
    Ok(ExitCode::FAILURE)
}

fn main() -> Result<ExitCode> {
    match Cli::parse().cmd {
        Cmd::Fuzz { cmd } => fuzz_main(cmd),
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
        Cmd::CheckReferenceIsolation { root } => {
            let root =
                root.unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")));
            let rule = reference_isolation::RULE;
            let report = reference_isolation::run(&root, &rule)?;
            if report.problems.is_empty() {
                if report.module_present {
                    println!(
                        "check-reference-isolation: ok ({} file(s) checked; modules reached besides {}: {:?})",
                        report.files.len(),
                        rule.module,
                        report.reached
                    );
                } else {
                    println!(
                        "check-reference-isolation: ok (module {}/{} not present yet; nothing to check)",
                        rule.crate_src, rule.module
                    );
                }
                return Ok(ExitCode::SUCCESS);
            }
            for p in &report.problems {
                eprintln!("check-reference-isolation: {p}");
            }
            eprintln!(
                "check-reference-isolation: {} problem(s)",
                report.problems.len()
            );
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
        Cmd::GatesSelftest { root, target } => {
            let root =
                root.unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")));
            let reports = gates_selftest::run(&root, target.as_deref())?;
            let on = target.as_deref().unwrap_or("host");
            let mut failed = 0;
            for r in &reports {
                let verdicts: Vec<String> = r
                    .runs
                    .iter()
                    .map(|g| format!("{} {}", g.gate, if g.success { "passed" } else { "failed" }))
                    .collect();
                if r.runs.iter().all(|g| g.problem.is_none()) {
                    println!(
                        "gates-selftest: {} ({on}): {} as expected",
                        r.case.name,
                        verdicts.join(", ")
                    );
                }
                for p in r.runs.iter().filter_map(|g| g.problem.as_ref()) {
                    failed += 1;
                    eprintln!("gates-selftest: {p} (scratch: {})", r.dir.display());
                }
            }
            if failed == 0 {
                println!(
                    "gates-selftest: ok ({} case(s), target {on})",
                    reports.len()
                );
                return Ok(ExitCode::SUCCESS);
            }
            eprintln!("gates-selftest: {failed} problem(s)");
            Ok(ExitCode::FAILURE)
        }
        Cmd::CheckBranchProtection { root, repo, branch } => {
            let root =
                root.unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")));
            let payload = codeowners::contributing_payload(&codeowners::read_text(
                &root.join(codeowners::CONTRIBUTING),
            )?)?;
            let local = codeowners::read_text(&root.join(codeowners::CODEOWNERS))?;
            let repo = repo.unwrap_or(payload.repo);
            let branches = if branch.is_empty() {
                payload.branches
            } else {
                branch
            };
            let reports = codeowners::check_live(
                &codeowners::Gh,
                &repo,
                &branches,
                &payload.protection,
                &local,
            )?;
            let mut failed = 0;
            for r in &reports {
                if r.problems.is_empty() {
                    println!(
                        "check-branch-protection: {repo} {}: ok (protection matches {}, CODEOWNERS matches, no CODEOWNERS errors)",
                        r.branch,
                        codeowners::CONTRIBUTING
                    );
                }
                for p in &r.problems {
                    failed += 1;
                    eprintln!("check-branch-protection: {repo} {}: {p}", r.branch);
                }
            }
            if failed == 0 {
                return Ok(ExitCode::SUCCESS);
            }
            eprintln!("check-branch-protection: {failed} problem(s)");
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
        Cmd::DiagnosticsDoc { root, check } => {
            let root =
                root.unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")));
            if !check {
                let changed = diagnostics_doc::write(&root)?;
                let verb = if changed { "wrote" } else { "unchanged" };
                println!("diagnostics-doc: {verb} {}", diagnostics_doc::DOC_PATH);
                return Ok(ExitCode::SUCCESS);
            }
            let problems = diagnostics_doc::check(&root)?;
            if problems.is_empty() {
                println!(
                    "diagnostics-doc: ok ({} code(s), {} deprecated, {} up to date)",
                    rr_core::diag::codes::REGISTRY.len(),
                    rr_core::diag::codes::DEPRECATED.len(),
                    diagnostics_doc::DOC_PATH
                );
                return Ok(ExitCode::SUCCESS);
            }
            for p in &problems {
                eprintln!("diagnostics-doc: {p}");
            }
            eprintln!("diagnostics-doc: {} problem(s)", problems.len());
            Ok(ExitCode::FAILURE)
        }
        Cmd::ValidateFixtures { base, paths } => {
            let workspace = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."));
            let status = validate_fixtures::command(&workspace, base.as_deref(), &paths)
                .status()
                .context("validate-fixtures: cannot run cargo")?;
            Ok(match status.code().and_then(|c| u8::try_from(c).ok()) {
                Some(0) => ExitCode::SUCCESS,
                Some(code) => ExitCode::from(code),
                None => ExitCode::FAILURE,
            })
        }
        Cmd::GenSynthetic { check, dir } => gen_synthetic_main(check, dir),
        Cmd::CoverageGate {
            lcov,
            base_lcov,
            min,
            max_drop,
            crates,
            root,
        } => {
            let root =
                root.unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")));
            let crates = if crates.is_empty() {
                coverage_gate::GATED_CRATES
                    .iter()
                    .map(|&c| c.to_owned())
                    .collect()
            } else {
                crates
            };
            let report =
                coverage_gate::run(&root, &lcov, base_lcov.as_deref(), &crates, min, max_drop)?;
            for r in &report.rows {
                let base = r
                    .base
                    .map_or_else(|| "not compared".to_owned(), |b| format!("base {b}"));
                let notes = if r.notes.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", r.notes.join("; "))
                };
                println!(
                    "coverage-gate: {}: line coverage {} ({base}){notes}",
                    r.krate, r.head
                );
            }
            if report.problems.is_empty() {
                println!(
                    "coverage-gate: ok ({} crate(s), minimum {min}%{})",
                    report.rows.len(),
                    if base_lcov.is_some() {
                        format!(
                            ", no regression against the base beyond {}",
                            coverage_gate::Points(max_drop)
                        )
                    } else {
                        ", no base report".to_owned()
                    }
                );
                return Ok(ExitCode::SUCCESS);
            }
            for p in &report.problems {
                eprintln!("coverage-gate: {p}");
            }
            eprintln!("coverage-gate: {} problem(s)", report.problems.len());
            Ok(ExitCode::FAILURE)
        }
        Cmd::CheckMutantsExclusions { root, config } => {
            let root =
                root.unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")));
            let report = mutants_exclusions::run(&root, config.as_deref())?;
            if report.problems.is_empty() {
                println!(
                    "check-mutants-exclusions: ok ({} exclusion(s), each with a justification comment)",
                    report.entries.len()
                );
                return Ok(ExitCode::SUCCESS);
            }
            for p in &report.problems {
                eprintln!("check-mutants-exclusions: {p}");
            }
            eprintln!(
                "check-mutants-exclusions: {} problem(s)",
                report.problems.len()
            );
            Ok(ExitCode::FAILURE)
        }
        Cmd::MutantsScope { diff, root } => {
            let root =
                root.unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")));
            let text = std::fs::read_to_string(&diff)
                .with_context(|| format!("reading {}", diff.display()))?;
            let scope = mutants_scope::scope(&text, |p| std::fs::read_to_string(root.join(p)).ok());
            match &scope {
                mutants_scope::Scope::Diff => {
                    eprintln!("mutants-scope: diff (no test code of a gated crate changed)");
                }
                mutants_scope::Scope::Full(reasons) => {
                    for r in reasons {
                        eprintln!("mutants-scope: full: {r}");
                    }
                }
            }
            println!("{scope}");
            Ok(ExitCode::SUCCESS)
        }
        Cmd::MutantsSelftest { root } => {
            let root =
                root.unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")));
            let reports = mutants_selftest::run_all(&root)?;
            let mut failed = 0;
            for r in &reports {
                let scope = r
                    .scope
                    .as_ref()
                    .map_or_else(|| "weekly full run".to_owned(), |s| format!("PR scope {s}"));
                if r.problems.is_empty() {
                    println!(
                        "mutants-selftest: {} ({scope}): exit {:?}, {} caught, {} missed, as expected",
                        r.case,
                        r.exit,
                        r.caught,
                        r.missed.len()
                    );
                }
                for p in &r.problems {
                    failed += 1;
                    eprintln!(
                        "mutants-selftest: {} ({scope}): {p} (scratch: {})",
                        r.case,
                        r.dir.display()
                    );
                }
            }
            if failed == 0 {
                println!(
                    "mutants-selftest: ok ({} case(s): a weakened test and an untested change in a gated module fail the gate)",
                    reports.len()
                );
                return Ok(ExitCode::SUCCESS);
            }
            eprintln!("mutants-selftest: {failed} problem(s)");
            Ok(ExitCode::FAILURE)
        }
    }
}
