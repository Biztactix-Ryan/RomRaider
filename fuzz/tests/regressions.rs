//! Replays every committed crash input under `fuzz/regressions/<target>/`
//! through its target body, on stable, without libFuzzer (task US-RR-5-7;
//! docs/fuzzing.md). Run by `cargo xtask fuzz replay` and by the
//! `fuzz-regressions` job in ci.yml.
//!
//! A custom harness (`harness = false` in fuzz/Cargo.toml) with two modes:
//!
//! * **Parent** (no `RR_FUZZ_REPLAY_TARGET`): first a self-test on scratch
//!   regression trees proves the replay fails on a planted panic, a planted
//!   hang (killed at the timeout, not left running), a planted oversized
//!   allocation and planted unbounded heap growth, on a regression directory
//!   for an unknown target and on a stray file, and passes on a clean input
//!   and on a target with no directory. Then the committed `fuzz/regressions/`
//!   is replayed under the limits from `fuzz/Cargo.toml`
//!   `[package.metadata.rr-fuzz]`. Exits non-zero if either fails.
//! * **Child** (started by the parent with `RR_FUZZ_REPLAY_TARGET` and
//!   `RR_FUZZ_REPLAY_INPUT`): runs that one input through the target with
//!   the allocation caps armed in [`Capped`], and exits 0, 101 (panic) or by
//!   abort (refused allocation).
//!
//! The planted targets exist only in this file; the committed tree is
//! replayed against `rr_fuzz::TARGETS` alone, so a `regressions/planted_*`
//! directory would be an unknown target there.

use std::alloc::{GlobalAlloc, Layout, System};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rr_fuzz::replay::{self, ChildRequest, Limits, Outcome, Report};
use rr_fuzz::{TARGETS, Target};

// --- Allocation caps in the child --------------------------------------------

/// Counting allocator: once armed (non-zero limits, child mode only), refuses
/// any single allocation above [`MALLOC_LIMIT`] and any allocation that would
/// take the live heap above [`HEAP_LIMIT`]. A refused allocation returns null,
/// and Rust's allocation-error handler prints "memory allocation of N bytes
/// failed" and aborts: a reported failure, like libFuzzer's `-malloc_limit_mb`
/// / `-rss_limit_mb`.
struct Capped;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static MALLOC_LIMIT: AtomicUsize = AtomicUsize::new(0);
static HEAP_LIMIT: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static ALLOC: Capped = Capped;

/// Reserves `grow` live bytes for an allocation of `size`; false if a cap
/// refuses it.
fn admit(size: usize, grow: usize) -> bool {
    let malloc = MALLOC_LIMIT.load(Ordering::Relaxed);
    if malloc != 0 && size > malloc {
        return false;
    }
    let heap = HEAP_LIMIT.load(Ordering::Relaxed);
    let before = LIVE.fetch_add(grow, Ordering::Relaxed);
    if heap != 0 && before.saturating_add(grow) > heap {
        LIVE.fetch_sub(grow, Ordering::Relaxed);
        return false;
    }
    true
}

// SAFETY: every method forwards to `System` with the caller's arguments; the
// accounting only decides whether to forward at all, and a refusal returns
// null, which `GlobalAlloc` permits.
unsafe impl GlobalAlloc for Capped {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !admit(layout.size(), layout.size()) {
            return std::ptr::null_mut();
        }
        // SAFETY: forwarded unchanged; the caller upholds `alloc`'s contract.
        let p = unsafe { System.alloc(layout) };
        if p.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !admit(layout.size(), layout.size()) {
            return std::ptr::null_mut();
        }
        // SAFETY: forwarded unchanged; the caller upholds the contract.
        let p = unsafe { System.alloc_zeroed(layout) };
        if p.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded unchanged; `ptr` came from this allocator.
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old = layout.size();
        let grow = new_size.saturating_sub(old);
        if !admit(new_size, grow) {
            return std::ptr::null_mut();
        }
        // SAFETY: forwarded unchanged; the caller upholds `realloc`'s contract.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if p.is_null() {
            LIVE.fetch_sub(grow, Ordering::Relaxed);
        } else if new_size < old {
            LIVE.fetch_sub(old - new_size, Ordering::Relaxed);
        }
        p
    }
}

// --- Planted targets (self-test only) -----------------------------------------

fn planted_panic(_: &[u8]) {
    panic!("planted panic (replay self-test)");
}

fn planted_hang(_: &[u8]) {
    let mut n = 0u64;
    loop {
        n = std::hint::black_box(n.wrapping_add(1));
    }
}

fn planted_alloc(_: &[u8]) {
    let buf: Vec<u8> = Vec::with_capacity(1 << 30);
    std::hint::black_box(&buf);
}

fn planted_growth(_: &[u8]) {
    let mut held: Vec<Vec<u8>> = Vec::new();
    for _ in 0..256 {
        held.push(vec![0u8; 4 << 20]);
        std::hint::black_box(&held);
    }
}

const PLANTED: &[(&str, Target)] = &[
    ("planted_panic", planted_panic),
    ("planted_hang", planted_hang),
    ("planted_alloc", planted_alloc),
    ("planted_growth", planted_growth),
];

fn lookup(name: &str) -> Option<Target> {
    TARGETS
        .iter()
        .chain(PLANTED)
        .find(|(n, _)| *n == name)
        .map(|&(_, t)| t)
}

// --- Child -------------------------------------------------------------------

fn child(req: &ChildRequest) -> ExitCode {
    let Some(target) = lookup(&req.target) else {
        eprintln!("replay child: unknown target {:?}", req.target);
        return ExitCode::from(2);
    };
    let data = match fs::read(&req.input) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("replay child: {}: {e}", req.input.display());
            return ExitCode::from(2);
        }
    };
    MALLOC_LIMIT.store(req.malloc_limit, Ordering::Relaxed);
    HEAP_LIMIT.store(req.heap_limit, Ordering::Relaxed);
    target(&data);
    ExitCode::SUCCESS
}

// --- Self-test -----------------------------------------------------------------

/// What a self-test case must produce.
enum Expect {
    /// Passes, having replayed this many inputs.
    Pass(usize),
    /// Fails on a layout problem mentioning the text.
    Problem(&'static str),
    /// Fails on the input `<target>/<file>`, its output containing the text.
    Fails(&'static str, &'static str),
    /// Fails on the input `<target>/<file>` by timing out.
    TimesOut(&'static str),
}

struct Case {
    name: &'static str,
    /// Files to create under the scratch `regressions/`; a path ending in
    /// `/` creates a directory.
    files: &'static [(&'static str, &'static [u8])],
    expect: Expect,
}

const CLEAN_TOY: &[u8] = b"\x02ok\x00";

const CASES: &[Case] = &[
    Case {
        name: "no regressions directory",
        files: &[],
        expect: Expect::Pass(0),
    },
    Case {
        name: "clean input; other targets have no directory",
        files: &[("toy/clean", CLEAN_TOY)],
        expect: Expect::Pass(1),
    },
    Case {
        name: "empty target directory",
        files: &[("ecu_id/", b"")],
        expect: Expect::Pass(0),
    },
    Case {
        name: "planted panic",
        files: &[("toy/clean", CLEAN_TOY), ("planted_panic/crash-1", b"x")],
        expect: Expect::Fails("planted_panic/crash-1", "planted panic"),
    },
    Case {
        name: "planted hang (per-input timeout)",
        files: &[("planted_hang/timeout-1", b"x")],
        expect: Expect::TimesOut("planted_hang/timeout-1"),
    },
    Case {
        name: "planted oversized allocation (malloc_limit_mb)",
        files: &[("planted_alloc/oom-1", b"x")],
        expect: Expect::Fails("planted_alloc/oom-1", "memory allocation of"),
    },
    Case {
        name: "planted unbounded heap growth (rss_limit_mb)",
        files: &[("planted_growth/oom-2", b"x")],
        expect: Expect::Fails("planted_growth/oom-2", "memory allocation of"),
    },
    Case {
        name: "directory for an unknown target",
        files: &[("toy/clean", CLEAN_TOY), ("no_such_target/crash-1", b"x")],
        expect: Expect::Problem("unknown fuzz target \"no_such_target\""),
    },
    Case {
        name: "stray file directly under regressions/",
        files: &[("crash-1", b"x")],
        expect: Expect::Problem("directly under regressions/"),
    },
    Case {
        name: "nested directory in a target directory",
        files: &[("toy/sub/crash-1", b"x")],
        expect: Expect::Problem("subdirectories are not replayed"),
    },
];

/// Tight limits so the self-test is quick: the hang is killed after 2 s.
const SELFTEST_LIMITS: Limits = Limits {
    timeout: Duration::from_secs(2),
    heap_limit_mb: 64,
    malloc_limit_mb: 32,
};

fn build_tree(dir: &Path, files: &[(&str, &[u8])]) -> std::io::Result<()> {
    let _ = fs::remove_dir_all(dir);
    for &(rel, bytes) in files {
        let path = dir.join(rel);
        if rel.ends_with('/') {
            fs::create_dir_all(&path)?;
        } else {
            fs::create_dir_all(path.parent().unwrap_or(dir))?;
            fs::write(&path, bytes)?;
        }
    }
    Ok(())
}

/// Why `report` does not meet `expect`, if it does not.
fn judge(report: &Report, expect: &Expect) -> Option<String> {
    let find = |label: &str| report.results.iter().find(|r| r.input.label() == label);
    match *expect {
        Expect::Pass(n) if report.passed() && report.results.len() == n => None,
        Expect::Pass(n) => Some(format!(
            "expected a pass with {n} input(s), got problems {:?} and {} result(s), {} failing",
            report.problems,
            report.results.len(),
            report.failures().count()
        )),
        Expect::Problem(text)
            if !report.passed() && report.problems.iter().any(|p| p.contains(text)) =>
        {
            None
        }
        Expect::Problem(text) => Some(format!(
            "expected a failing layout problem mentioning {text:?}, got {:?}",
            report.problems
        )),
        Expect::Fails(label, text) => match find(label) {
            Some(r) if report.passed() => {
                Some(format!("{label}: the replay passed ({})", r.outcome))
            }
            Some(r) if matches!(r.outcome, Outcome::Failed(_)) && r.output.contains(text) => None,
            Some(r) => Some(format!(
                "{label}: expected a failure with {text:?} in its output, got {} and output:\n{}",
                r.outcome, r.output
            )),
            None => Some(format!("{label} was not replayed")),
        },
        Expect::TimesOut(label) => match find(label) {
            Some(r)
                if !report.passed()
                    && matches!(r.outcome, Outcome::TimedOut(_))
                    && r.elapsed >= SELFTEST_LIMITS.timeout
                    && r.elapsed < SELFTEST_LIMITS.timeout * 5 =>
            {
                None
            }
            Some(r) => Some(format!(
                "{label}: expected to be killed at the {} s timeout, got {} after {:?}",
                SELFTEST_LIMITS.timeout.as_secs(),
                r.outcome,
                r.elapsed
            )),
            None => Some(format!("{label} was not replayed")),
        },
    }
}

fn selftest(exe: &Path) -> bool {
    let scratch = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("replay-selftest");
    let names: Vec<&str> = TARGETS.iter().chain(PLANTED).map(|&(n, _)| n).collect();
    let mut ok = true;
    for (i, case) in CASES.iter().enumerate() {
        let dir = scratch.join(format!("case-{i}")).join("regressions");
        if let Err(e) = build_tree(&dir, case.files) {
            eprintln!(
                "replay self-test: {}: cannot build {}: {e}",
                case.name,
                dir.display()
            );
            ok = false;
            continue;
        }
        let report = match replay::replay(exe, &dir, &names, &SELFTEST_LIMITS) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("replay self-test: {}: cannot run a child: {e}", case.name);
                ok = false;
                continue;
            }
        };
        match judge(&report, &case.expect) {
            None => println!("replay self-test: {}: as expected", case.name),
            Some(why) => {
                eprintln!("replay self-test: {}: {why}", case.name);
                ok = false;
            }
        }
    }
    ok
}

// --- The committed regressions ---------------------------------------------------

fn committed(exe: &Path) -> bool {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = match fs::read_to_string(root.join("Cargo.toml")) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("replay: fuzz/Cargo.toml: {e}");
            return false;
        }
    };
    let limits = match replay::limits_from_manifest(&manifest) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("replay: fuzz/Cargo.toml: {e}");
            return false;
        }
    };
    let names: Vec<&str> = TARGETS.iter().map(|&(n, _)| n).collect();
    let dir = root.join("regressions");
    let report = match replay::replay(exe, &dir, &names, &limits) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("replay: cannot run a child: {e}");
            return false;
        }
    };
    for p in &report.problems {
        eprintln!("replay: {p}");
    }
    for r in &report.results {
        println!(
            "replay {}: {} ({} ms)",
            r.input.label(),
            r.outcome,
            r.elapsed.as_millis()
        );
        if r.outcome != Outcome::Passed {
            eprintln!(
                "--- output of {} ---\n{}--- end ---",
                r.input.label(),
                r.output
            );
        }
    }
    let failed = report.failures().count();
    if report.passed() {
        println!(
            "replay: ok ({} committed input(s), timeout {} s, malloc {} MB, heap {} MB)",
            report.results.len(),
            limits.timeout.as_secs(),
            limits.malloc_limit_mb,
            limits.heap_limit_mb
        );
    } else {
        eprintln!(
            "replay: FAILED ({} layout problem(s), {failed} failing input(s)); \
             see docs/fuzzing.md \"When the fuzzer finds a bug\"",
            report.problems.len()
        );
    }
    report.passed()
}

fn main() -> ExitCode {
    if let Some(req) = replay::child_request() {
        return child(&req);
    }
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("replay: cannot locate the test binary: {e}");
            return ExitCode::FAILURE;
        }
    };
    let selftest_ok = selftest(&exe);
    let committed_ok = committed(&exe);
    if selftest_ok && committed_ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
