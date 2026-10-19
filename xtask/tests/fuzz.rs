//! US-RR-5-5: the fuzz crate (`fuzz/`) and `cargo xtask fuzz`.
//!
//! * The committed tree passes `fuzz check`: the crate is excluded from the
//!   stable root workspace, is its own workspace, declares per-input time and
//!   memory limits, and every target is fully wired. Each rule bites on a
//!   mutated copy of the real files.
//! * `fuzz run` always passes the limits to libFuzzer, before any extra
//!   flags, through the rustup proxy with `+nightly`.
//! * The regression replay (US-RR-5-7) is declared and `fuzz check` rejects
//!   a `fuzz/regressions/` entry that would never be replayed (an unknown
//!   target's directory, a stray file, a nested directory); `fuzz replay`
//!   is a stable `cargo test` of the fuzz crate. (The replay itself, and its
//!   self-test that a planted panic, hang and memory blow-up fail it, run in
//!   the `fuzz-regressions` CI job: `fuzz/tests/regressions.rs`.)
//! * The selftest cases cover a panic, a hang and both memory limits, and are
//!   judged correctly. (Running them needs nightly + cargo-fuzz, so the run
//!   itself is `cargo xtask fuzz selftest`, not part of this suite.)

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use xtask::fuzz::{
    self, Limits, RunOptions, SELFTEST_CASES, SELFTEST_FEATURES, SELFTEST_TARGET, evaluate,
    limit_problems, regression_problems, replay_command, run_command,
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// A scratch copy of the files `fuzz::check` reads.
fn scratch_tree(name: &str) -> io::Result<PathBuf> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("fuzz-check")
        .join(name);
    let _ = fs::remove_dir_all(&dir);
    let real = root();
    for sub in ["fuzz/src", "fuzz/fuzz_targets", "fuzz/tests"] {
        fs::create_dir_all(dir.join(sub))?;
    }
    // Text files with `\r\n` normalised, so the `edit`s below match on a
    // Windows (autocrlf) checkout too.
    for file in [
        "Cargo.toml",
        "fuzz/Cargo.toml",
        "fuzz/src/lib.rs",
        "fuzz/tests/regressions.rs",
    ] {
        let text = fs::read_to_string(real.join(file))?.replace("\r\n", "\n");
        fs::write(dir.join(file), text)?;
    }
    for sub in ["fuzz/fuzz_targets", "fuzz/seeds/ecu_id", "fuzz/seeds/toy"] {
        fs::create_dir_all(dir.join(sub))?;
        for e in fs::read_dir(real.join(sub))? {
            let p = e?.path();
            if let Some(n) = p.file_name() {
                fs::copy(&p, dir.join(sub).join(n))?;
            }
        }
    }
    Ok(dir)
}

fn edit(path: &Path, from: &str, to: &str) -> io::Result<()> {
    let text = fs::read_to_string(path)?;
    assert!(text.contains(from), "{from:?} not in {}", path.display());
    fs::write(path, text.replacen(from, to, 1))
}

fn problems(dir: &Path) -> Vec<String> {
    fuzz::check(dir).unwrap_or_else(|e| vec![format!("fuzz::check failed: {e:#}")])
}

fn has(problems: &[String], needle: &str) -> bool {
    problems.iter().any(|p| p.contains(needle))
}

#[test]
fn committed_fuzz_crate_passes_check() {
    let p = problems(&root());
    assert!(p.is_empty(), "fuzz check problems:\n{}", p.join("\n"));
    let m = fuzz::load(&root()).unwrap();
    let names: Vec<&str> = m.targets.iter().map(|t| t.name.as_str()).collect();
    assert!(
        names.contains(&"ecu_id"),
        "template target missing: {names:?}"
    );
    assert!(names.contains(&SELFTEST_TARGET), "{names:?}");
    for f in SELFTEST_FEATURES {
        assert!(
            m.features.iter().any(|x| x == f),
            "feature {f} not declared"
        );
    }
}

#[test]
fn committed_limits_are_tighter_than_bare_libfuzzer() {
    let l = fuzz::load(&root()).unwrap().limits.unwrap();
    // libFuzzer alone: -timeout=1200, no separate single-allocation cap.
    assert!(l.timeout_secs <= 60, "{l:?}");
    assert!(l.malloc_limit_mb < l.rss_limit_mb, "{l:?}");
}

#[test]
fn check_rejects_fuzz_crate_in_root_workspace() {
    let dir = scratch_tree("not-excluded").unwrap();
    edit(&dir.join("Cargo.toml"), "exclude = [\"fuzz\"]", "").unwrap();
    assert!(has(&problems(&dir), "exclude must list \"fuzz\""));

    let dir = scratch_tree("member").unwrap();
    edit(
        &dir.join("Cargo.toml"),
        "    \"xtask\",\n",
        "    \"xtask\",\n    \"fuzz\",\n",
    )
    .unwrap();
    assert!(has(
        &problems(&dir),
        "members must not include the fuzz crate"
    ));
}

#[test]
fn check_rejects_fuzz_crate_without_own_workspace() {
    let dir = scratch_tree("no-workspace").unwrap();
    edit(
        &dir.join("fuzz/Cargo.toml"),
        "[workspace]\nmembers = [\".\"]",
        "",
    )
    .unwrap();
    assert!(has(&problems(&dir), "own [workspace]"));
}

#[test]
fn check_rejects_missing_or_unbounded_limits() {
    for (case, from, to, needle) in [
        (
            "no-timeout",
            "timeout_secs = ",
            "# timeout_secs = ",
            "must set integer",
        ),
        (
            "no-rss",
            "rss_limit_mb = ",
            "# rss_limit_mb = ",
            "must set integer",
        ),
        (
            "no-malloc",
            "malloc_limit_mb = ",
            "# malloc_limit_mb = ",
            "must set integer",
        ),
        (
            "timeout-0",
            "timeout_secs = 10",
            "timeout_secs = 0",
            "timeout_secs = 0",
        ),
        (
            "timeout-1200",
            "timeout_secs = 10",
            "timeout_secs = 1200",
            "timeout_secs = 1200",
        ),
        (
            "rss-0",
            "rss_limit_mb = 2048",
            "rss_limit_mb = 0",
            "rss_limit_mb = 0",
        ),
        (
            "malloc-0",
            "malloc_limit_mb = 512",
            "malloc_limit_mb = 0",
            "malloc_limit_mb = 0",
        ),
    ] {
        let dir = scratch_tree(case).unwrap();
        edit(&dir.join("fuzz/Cargo.toml"), from, to).unwrap();
        let p = problems(&dir);
        assert!(has(&p, needle), "{case}: {p:?}");
    }
}

#[test]
fn check_rejects_half_wired_targets() {
    let dir = scratch_tree("no-file").unwrap();
    fs::remove_file(dir.join("fuzz/fuzz_targets/ecu_id.rs")).unwrap();
    assert!(has(&problems(&dir), "fuzz/fuzz_targets/ecu_id.rs: missing"));

    let dir = scratch_tree("no-bin").unwrap();
    fs::write(dir.join("fuzz/fuzz_targets/extra.rs"), "").unwrap();
    assert!(has(&problems(&dir), "extra.rs: no [[bin]]"));

    let dir = scratch_tree("no-registry").unwrap();
    edit(&dir.join("fuzz/src/lib.rs"), "(\"ecu_id\", ecu_id), ", "").unwrap();
    assert!(has(&problems(&dir), "TARGETS has no (\"ecu_id\", ecu_id)"));

    let dir = scratch_tree("no-seeds").unwrap();
    fs::remove_dir_all(dir.join("fuzz/seeds/toy")).unwrap();
    assert!(has(&problems(&dir), "fuzz/seeds/toy/"));

    let dir = scratch_tree("test-on").unwrap();
    edit(
        &dir.join("fuzz/Cargo.toml"),
        "path = \"fuzz_targets/ecu_id.rs\"\ntest = false",
        "path = \"fuzz_targets/ecu_id.rs\"\ntest = true",
    )
    .unwrap();
    assert!(has(&problems(&dir), "[[bin]] ecu_id: set test = false"));
}

fn args(cmd: &std::process::Command) -> Vec<String> {
    cmd.get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

/// `base` joined with each `/`-separated part of `rel`, as the command line
/// shows it (the platform's separators).
fn p(base: &Path, rel: &str) -> String {
    rel.split('/')
        .fold(base.to_path_buf(), |path, part| path.join(part))
        .to_string_lossy()
        .into_owned()
}

const LIMITS: Limits = Limits {
    timeout_secs: 7,
    rss_limit_mb: 1500,
    malloc_limit_mb: 300,
};

#[test]
fn run_command_always_applies_the_limits_before_extra_flags() {
    let r = Path::new("/w");
    let opts = RunOptions {
        target: "ecu_id".into(),
        max_total_time: Some(30),
        extra: vec!["-timeout=99".into()],
        ..RunOptions::default()
    };
    let cmd = run_command(r, LIMITS, "nightly", &opts);
    assert_eq!(cmd.get_program(), "cargo", "must be the rustup proxy");
    assert_eq!(
        args(&cmd),
        [
            "+nightly",
            "fuzz",
            "run",
            "--fuzz-dir",
            &p(r, "fuzz"),
            "ecu_id",
            &p(r, "fuzz/corpus/ecu_id"),
            &p(r, "fuzz/seeds/ecu_id"),
            "--",
            "-timeout=7",
            "-rss_limit_mb=1500",
            "-malloc_limit_mb=300",
            "-max_total_time=30",
            "-timeout=99",
        ]
    );
}

#[test]
fn run_command_reproduces_inputs_with_the_limits() {
    let opts = RunOptions {
        target: "toy".into(),
        features: vec!["planted-hang".into(), "planted-oom".into()],
        inputs: vec![PathBuf::from("/tmp/crash-1")],
        artifact_prefix: Some(PathBuf::from("/tmp/a/")),
        ..RunOptions::default()
    };
    let r = Path::new("/w");
    let a = args(&run_command(r, LIMITS, "nightly-2026-10-01", &opts));
    assert_eq!(
        a,
        [
            "+nightly-2026-10-01",
            "fuzz",
            "run",
            "--fuzz-dir",
            &p(r, "fuzz"),
            "--features",
            "planted-hang,planted-oom",
            "toy",
            "/tmp/crash-1",
            "--",
            "-timeout=7",
            "-rss_limit_mb=1500",
            "-malloc_limit_mb=300",
            "-artifact_prefix=/tmp/a/",
        ]
    );
}

#[test]
fn run_refuses_unknown_targets() {
    let opts = RunOptions {
        target: "nope".into(),
        ..RunOptions::default()
    };
    let err = fuzz::run(&root(), &opts).unwrap_err().to_string();
    assert!(err.contains("no fuzz target \"nope\""), "{err}");
}

#[test]
fn limit_problems_bounds() {
    assert!(limit_problems(Some(LIMITS)).is_empty());
    assert_eq!(limit_problems(None).len(), 1);
    let bad = Limits {
        timeout_secs: 61,
        rss_limit_mb: 100,
        malloc_limit_mb: 200,
    };
    assert_eq!(
        limit_problems(Some(bad)).len(),
        3,
        "{:?}",
        limit_problems(Some(bad))
    );
}

#[test]
fn selftest_covers_panic_hang_and_both_memory_limits() {
    let expected: Vec<&str> = SELFTEST_CASES
        .iter()
        .filter_map(|c| c.expect_failure)
        .collect();
    for needle in [
        "planted panic",
        "libFuzzer: timeout",
        "libFuzzer: out-of-memory (malloc(",
        "libFuzzer: out-of-memory (used:",
    ] {
        assert!(
            expected.contains(&needle),
            "no selftest case expects {needle:?}"
        );
    }
    assert!(
        SELFTEST_CASES.iter().any(|c| c.expect_failure.is_none()),
        "no clean control case"
    );
}

#[test]
fn evaluate_judges_runs() {
    let clean = SELFTEST_CASES
        .iter()
        .find(|c| c.expect_failure.is_none())
        .unwrap();
    let hang = SELFTEST_CASES
        .iter()
        .find(|c| c.expect_failure == Some("libFuzzer: timeout"))
        .unwrap();
    assert_eq!(evaluate(clean, true, ""), None);
    assert!(evaluate(clean, false, "").is_some());
    assert_eq!(
        evaluate(
            hang,
            false,
            "==1== ERROR: libFuzzer: timeout after 11 seconds"
        ),
        None
    );
    // Passing, or failing for another reason (e.g. no nightly), is a problem.
    assert!(evaluate(hang, true, "").is_some());
    assert!(evaluate(hang, false, "error: toolchain 'nightly' is not installed").is_some());
}

#[test]
fn check_requires_the_replay_harness() {
    let dir = scratch_tree("harness-on").unwrap();
    edit(
        &dir.join("fuzz/Cargo.toml"),
        "path = \"tests/regressions.rs\"\nharness = false",
        "path = \"tests/regressions.rs\"\nharness = true",
    )
    .unwrap();
    assert!(has(
        &problems(&dir),
        "needs [[test]] name = \"regressions\""
    ));

    let dir = scratch_tree("no-harness-file").unwrap();
    fs::remove_file(dir.join("fuzz/tests/regressions.rs")).unwrap();
    assert!(has(&problems(&dir), "fuzz/tests/regressions.rs: missing"));
}

#[test]
fn check_rejects_regressions_that_would_never_replay() {
    let targets = ["ecu_id", "toy"];
    let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fuzz-regressions");
    let tree = |name: &str, files: &[&str]| {
        let dir = base.join(name).join("regressions");
        let _ = fs::remove_dir_all(&dir);
        for f in files {
            let path = dir.join(f);
            if f.ends_with('/') {
                fs::create_dir_all(&path).unwrap();
            } else {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, b"x").unwrap();
            }
        }
        dir
    };
    // Deliberately fine: no directory at all, an empty target directory, a
    // target with no directory.
    assert!(regression_problems(&tree("none", &[]), &targets).is_empty());
    assert!(regression_problems(&tree("empty", &["ecu_id/"]), &targets).is_empty());
    assert!(regression_problems(&tree("ok", &["toy/crash-1"]), &targets).is_empty());

    let p = regression_problems(&tree("unknown", &["toy/a", "gone/crash-1"]), &targets);
    assert!(
        has(&p, "fuzz/regressions/gone/: unknown fuzz target \"gone\""),
        "{p:?}"
    );
    let p = regression_problems(&tree("stray", &["crash-1"]), &targets);
    assert!(has(&p, "fuzz/regressions/crash-1: not replayed"), "{p:?}");
    let p = regression_problems(&tree("nested", &["toy/sub/crash-1"]), &targets);
    assert!(
        has(&p, "fuzz/regressions/toy/sub/: subdirectories"),
        "{p:?}"
    );

    // And `fuzz check` reports it on a scratch copy of the crate.
    let dir = scratch_tree("unknown-regression").unwrap();
    fs::create_dir_all(dir.join("fuzz/regressions/renamed")).unwrap();
    fs::write(dir.join("fuzz/regressions/renamed/crash-1"), b"x").unwrap();
    assert!(has(&problems(&dir), "unknown fuzz target \"renamed\""));
}

#[test]
fn replay_command_is_a_stable_cargo_test_of_the_fuzz_crate() {
    let r = Path::new("/w");
    let cmd = replay_command(r, &[]);
    assert_eq!(cmd.get_program(), "cargo");
    let a = args(&cmd);
    assert_eq!(
        a,
        [
            "test",
            "--manifest-path",
            &p(r, "fuzz/Cargo.toml"),
            "--locked"
        ]
    );
    assert!(
        !a.iter().any(|x| x.starts_with('+')),
        "no toolchain override: replay runs on the pinned stable toolchain"
    );
    let a = args(&replay_command(
        r,
        &["planted-panic".into(), "planted-hang".into()],
    ));
    assert_eq!(&a[4..], ["--features", "planted-panic,planted-hang"]);
}

/// The only committed crash inputs that crash anything are the toy target's,
/// and only with its planted features on; every file is small.
#[test]
fn committed_regressions_are_small_and_cover_the_toy_target() {
    let dir = root().join("fuzz/regressions");
    let m = fuzz::load(&root()).unwrap();
    let names: Vec<&str> = m.targets.iter().map(|t| t.name.as_str()).collect();
    assert!(regression_problems(&dir, &names).is_empty());
    let toy: Vec<String> = fs::read_dir(dir.join(SELFTEST_TARGET))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(toy.iter().any(|n| n == "planted-panic"), "{toy:?}");
    for e in fs::read_dir(&dir).unwrap() {
        for f in fs::read_dir(e.unwrap().path()).unwrap() {
            let f = f.unwrap();
            let len = f.metadata().unwrap().len();
            assert!(
                len <= 64 * 1024,
                "{:?} is {len} bytes: minimise it",
                f.path()
            );
        }
    }
}
