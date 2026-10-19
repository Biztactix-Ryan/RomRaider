//! VirtualClock harness tests (US-RR-6-9): deterministic stepping across
//! threads, elapsed-time assertions, and the "10 s of virtual time in under
//! 10 ms of real time" property.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rr_core::clock::{Clock, MonoInstant, VirtualClock};
use rr_testkit::clock::{RunUntilError, TestClock, assert_elapsed, assert_elapsed_within};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn at(n: u64) -> MonoInstant {
    MonoInstant::ORIGIN + ms(n)
}

/// Real elapsed time of `f`, measured only to bound how long a test took.
#[expect(
    clippy::disallowed_methods,
    reason = "the test proves the harness never blocks on real time, so it must measure real time (D-010)"
)]
fn real_elapsed_of(f: impl FnOnce()) -> Duration {
    let start = std::time::Instant::now();
    f();
    start.elapsed()
}

/// Attempts per real-time measurement; the fastest one is checked.
///
/// A virtual clock that slept for real would make *every* attempt slow, so
/// taking the minimum keeps the proof intact while tolerating a test thread
/// that happens to be preempted on a loaded CI machine.
const ATTEMPTS: usize = 5;

/// Fastest real time over [`ATTEMPTS`] runs of `f` (each a full, freshly set
/// up simulation that makes its own exactness assertions).
fn fastest_real_elapsed_of(mut f: impl FnMut()) -> Duration {
    (0..ATTEMPTS)
        .map(|_| real_elapsed_of(&mut f))
        .min()
        .unwrap_or(Duration::MAX)
}

/// Real-time budget for simulating 10 s of virtual time (story US-RR-6).
const REAL_BUDGET: Duration = Duration::from_millis(10);
const SIMULATED: Duration = Duration::from_secs(10);

/// Real-time budget for the *manual*, cross-thread variant of the 10 s
/// simulation. Deliberately looser than [`REAL_BUDGET`]; see
/// [`ten_virtual_seconds_manual_driven_stay_far_below_real_time`].
///
/// Every manual step is a real thread handoff (the driver advances, the
/// worker must be scheduled to wake and re-park), so its cost is set by the
/// OS scheduler, not by the clock. Unloaded the whole run takes about 50 µs,
/// but with every CPU busy each handoff waits up to one scheduler slice
/// (~3 ms): 20 runs with one busy loop per CPU measured 11-33 ms
/// (fastest of 5), so a 10 ms bound failed most of them. 100 ms keeps a 3x
/// margin over that worst case while still being 100x below the simulated
/// 10 s — a clock that blocked on real time for even 1 % of the virtual time
/// it simulates would exceed it.
const MANUAL_REAL_BUDGET: Duration = Duration::from_millis(100);

#[test]
fn advance_by_moves_time_exactly_and_assertions_pass() {
    let clock = TestClock::manual();
    let start = clock.now();
    assert_eq!(start, MonoInstant::ORIGIN);
    clock.advance_by(ms(250));
    clock.advance_by(ms(750));
    assert_eq!(clock.now(), at(1000));
    assert_eq!(clock.elapsed_since(start), ms(1000));
    clock.assert_elapsed(start, ms(1000));
    clock.assert_elapsed_within(start, ms(1000), ms(1000));
    clock.assert_elapsed_within(start, ms(900), ms(1100));
    // Free-function forms accept any Clock.
    assert_elapsed(&clock.clock(), start, ms(1000));
    assert_elapsed_within(&*clock.shared(), start, ms(0), ms(2000));
    // advance_to never goes backwards.
    clock.advance_to(at(10));
    assert_eq!(clock.now(), at(1000));
}

#[test]
#[should_panic(expected = "virtual elapsed time mismatch: expected 1s, got 999ms")]
fn assert_elapsed_reports_expected_and_actual() {
    let clock = TestClock::manual();
    let start = clock.now();
    clock.advance_by(ms(999));
    clock.assert_elapsed(start, ms(1000));
}

#[test]
#[should_panic(expected = "virtual elapsed time out of range: expected 10ms..=20ms, got 25ms")]
fn assert_elapsed_within_reports_bounds_and_actual() {
    let clock = TestClock::manual();
    let start = clock.now();
    clock.advance_by(ms(25));
    clock.assert_elapsed_within(start, ms(10), ms(20));
}

#[test]
#[should_panic(expected = "empty range")]
fn assert_elapsed_within_rejects_inverted_range() {
    let clock = TestClock::manual();
    clock.assert_elapsed_within(clock.now(), ms(20), ms(10));
}

#[test]
fn auto_advance_sleeps_are_exact() {
    let clock = TestClock::auto_advance();
    let start = clock.now();
    clock.sleep(ms(40));
    clock.assert_elapsed(start, ms(40));
    let c = clock.clock();
    c.sleep_until(at(100));
    clock.assert_elapsed(start, ms(100));
}

/// A manual-mode worker sleeps three times; the main thread drives it with
/// `run_until` and observes each wake at exactly its deadline.
#[test]
fn run_until_drives_a_worker_through_three_sleeps_exactly() {
    let clock = TestClock::manual();
    let wakes = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&wakes);
    let worker = clock.spawn(move |c: VirtualClock| {
        for d in [10, 25, 100] {
            let wake = c.sleep_until_traced(c.now() + ms(d));
            assert_eq!(c.now(), wake.woke_at, "woke at its own deadline");
            log.lock().unwrap().push(wake);
        }
        "finished"
    });

    let woke = |n: usize| {
        let wakes = Arc::clone(&wakes);
        move || wakes.lock().unwrap().len() >= n
    };

    // Each run_until stops at the first settled moment its condition holds,
    // so time lands exactly on the worker's successive deadlines.
    assert_eq!(clock.run_until(ms(1000), woke(1)), Ok(at(10)));
    assert_eq!(clock.virtual_clock().next_deadline(), Some(at(35)));
    assert_eq!(clock.run_until(ms(1000), woke(2)), Ok(at(35)));
    assert_eq!(clock.virtual_clock().next_deadline(), Some(at(135)));
    assert_eq!(clock.run_until(ms(1000), woke(3)), Ok(at(135)));

    assert_eq!(worker.join(), "finished");
    assert_eq!(clock.live_workers(), 0);
    let wakes = wakes.lock().unwrap();
    let times: Vec<_> = wakes.iter().map(|w| w.woke_at).collect();
    assert_eq!(times, [at(10), at(35), at(135)]);
    let orders: Vec<_> = wakes.iter().map(|w| w.order).collect();
    assert_eq!(orders, [0, 1, 2]);
    clock.assert_elapsed(MonoInstant::ORIGIN, ms(135));
}

/// `step` releases one deadline at a time, even when a single jump would
/// have released several of the worker's sleeps.
#[test]
fn step_releases_one_deadline_at_a_time() {
    let clock = TestClock::manual();
    let worker = clock.spawn(|c| {
        let mut seen = Vec::new();
        for _ in 0..3 {
            c.sleep(ms(5));
            seen.push(c.now());
        }
        seen
    });
    assert_eq!(clock.step(), Some(at(5)));
    assert_eq!(clock.step(), Some(at(10)));
    assert_eq!(clock.step(), Some(at(15)));
    assert_eq!(clock.step(), None, "worker finished; nothing parked");
    assert_eq!(clock.now(), at(15));
    assert_eq!(worker.join(), [at(5), at(10), at(15)]);
}

/// `run_to` honours sleeps started along the way and ends exactly on target.
#[test]
fn run_to_steps_through_intermediate_deadlines() {
    let clock = TestClock::manual();
    let worker = clock.spawn(|c| {
        let mut seen = Vec::new();
        // 7 sleeps of 30 ms; only the first 3 fit before 100 ms.
        for _ in 0..7 {
            c.sleep(ms(30));
            seen.push(c.now());
        }
        seen
    });
    clock.run_to(at(100));
    assert_eq!(clock.now(), at(100));
    assert_eq!(clock.virtual_clock().next_deadline(), Some(at(120)));
    clock.run_for(ms(200));
    assert_eq!(clock.now(), at(300));
    assert_eq!(
        worker.join(),
        [at(30), at(60), at(90), at(120), at(150), at(180), at(210)]
    );
}

#[test]
fn run_until_reports_budget_exhausted_and_stalled() {
    let clock = TestClock::manual();
    let worker = clock.spawn(|c| c.sleep(ms(500)));
    let err = clock.run_until(ms(100), || false).unwrap_err();
    assert_eq!(
        err,
        RunUntilError::BudgetExhausted {
            now: at(0),
            budget_end: at(100),
            next_deadline: at(500),
        }
    );
    assert!(err.to_string().contains("budget"), "{err}");

    // Let the worker finish; then nothing is left to drive.
    clock
        .run_until(ms(1000), || clock.live_workers() == 0)
        .unwrap();
    worker.join();
    assert_eq!(
        clock.run_until(ms(1000), || false),
        Err(RunUntilError::Stalled { now: at(500) })
    );
}

/// Several workers on one timeline wake in deadline order, each exactly on
/// time, and `run_until` sees a consistent state between steps.
#[test]
fn multiple_workers_interleave_deterministically() {
    for _ in 0..20 {
        let clock = TestClock::manual();
        let log = Arc::new(Mutex::new(Vec::new()));
        let workers: Vec<_> = [(1_u8, 30_u64), (2, 20), (3, 50)]
            .into_iter()
            .map(|(id, period)| {
                let log = Arc::clone(&log);
                clock.spawn(move |c| {
                    for _ in 0..3 {
                        c.sleep(ms(period));
                        log.lock().unwrap().push((c.now(), id));
                    }
                })
            })
            .collect();
        clock
            .run_until(ms(10_000), || log.lock().unwrap().len() == 9)
            .unwrap();
        for w in workers {
            w.join();
        }
        let mut entries = log.lock().unwrap().clone();
        entries.sort();
        assert_eq!(
            entries,
            [
                (at(20), 2),
                (at(30), 1),
                (at(40), 2),
                (at(50), 3),
                (at(60), 1),
                (at(60), 2),
                (at(90), 1),
                (at(100), 3),
                (at(150), 3),
            ]
        );
        assert_eq!(clock.now(), at(150));
    }
}

#[test]
fn worker_guard_tracks_externally_spawned_threads() {
    let clock = TestClock::manual();
    let guard = clock.worker_guard();
    assert_eq!(clock.live_workers(), 1);
    let c = clock.clock();
    let handle = std::thread::spawn(move || {
        let _guard = guard;
        c.sleep(ms(70));
        c.now()
    });
    assert_eq!(clock.step(), Some(at(70)));
    assert_eq!(handle.join().unwrap(), at(70));
    assert_eq!(clock.live_workers(), 0);
}

#[test]
#[should_panic(expected = "worker failed on purpose")]
fn worker_panic_is_reraised_on_join() {
    let clock = TestClock::manual();
    let worker = clock.spawn(|c| {
        c.sleep(ms(1));
        panic!("worker failed on purpose");
    });
    // The panicking worker un-tracks itself, so settling does not hang.
    assert_eq!(clock.step(), Some(at(1)));
    assert_eq!(clock.live_workers(), 0);
    worker.join();
}

/// Proves US-RR-6-4 (the "under 10 ms for a simulated 10 s" bound): 10 s of
/// virtual sleeps (1000 × 10 ms) on an auto-advance clock complete in under
/// 10 ms of real time, with exact virtual timing.
///
/// Single-threaded: no thread handoffs, so the strict bound does not depend
/// on the OS scheduler. Taking the fastest of [`ATTEMPTS`] only absorbs a
/// preemption of the test thread itself; a clock that slept for real would
/// make every attempt slow and fail.
#[test]
fn ten_virtual_seconds_auto_advance_take_under_10ms_real() {
    let real = fastest_real_elapsed_of(|| {
        let clock = TestClock::auto_advance();
        let start = clock.now();
        for _ in 0..1000 {
            clock.sleep(ms(10));
        }
        clock.assert_elapsed(start, SIMULATED);
    });
    assert!(
        real < REAL_BUDGET,
        "10 s virtual took {real:?} of real time (fastest of {ATTEMPTS})"
    );
}

/// Proves US-RR-6-4 (deterministic stepping, cross-thread): a worker thread
/// sleeps 10 × 1 s (10 s virtual) and the main thread drives every step with
/// `run_until`; every wake lands exactly on its deadline and the whole
/// exchange — spawn, ten driven wakes, join — stays far below real time.
///
/// The bound is [`MANUAL_REAL_BUDGET`] (100 ms), not 10 ms: the cost of the
/// ten real thread handoffs is set by the OS scheduler and exceeded 10 ms on
/// a fully loaded machine (see the constant's docs). The strict 10 ms figure
/// is proven by the single-threaded
/// [`ten_virtual_seconds_auto_advance_take_under_10ms_real`], and the
/// absence of real sleeping by [`clock_harness_sources_make_no_real_time_calls`].
#[test]
fn ten_virtual_seconds_manual_driven_stay_far_below_real_time() {
    let real = fastest_real_elapsed_of(|| {
        let clock = TestClock::manual();
        let start = clock.now();
        let worker = clock.spawn(|c| {
            let mut wakes = Vec::with_capacity(10);
            for _ in 0..10 {
                c.sleep(ms(1000));
                wakes.push(c.now());
            }
            wakes
        });
        let end = clock
            .run_until(SIMULATED, || clock.live_workers() == 0)
            .unwrap();
        assert_eq!(end, start + SIMULATED);
        let expected: Vec<_> = (1..=10).map(|i| start + ms(i * 1000)).collect();
        assert_eq!(worker.join(), expected);
        clock.assert_elapsed(start, SIMULATED);
    });
    assert!(
        real < MANUAL_REAL_BUDGET,
        "10 s virtual (manual) took {real:?} of real time (fastest of {ATTEMPTS})"
    );
}

/// Proves US-RR-6-4 ("no real sleeping") structurally: the non-test source
/// of the harness (`rr_testkit::clock`) and of the clock it drives
/// (`rr_core::clock::VirtualClock`) contains no call that reads OS time or
/// blocks on it — no `thread::sleep`, `std::time::Instant`, `SystemTime`,
/// timed waits (`wait_timeout`, `park_timeout`, `recv_timeout`) or
/// `RealClock`. clippy's `disallowed_methods` (clippy.toml) already rejects
/// the std time calls; this test also covers the timed-wait APIs clippy does
/// not list, and fails in `cargo test` rather than only under clippy.
#[test]
fn clock_harness_sources_make_no_real_time_calls() {
    const FORBIDDEN: &[&str] = &[
        "thread::sleep",
        "Instant", // after removing `MonoInstant`: std::time::Instant
        "SystemTime",
        "UNIX_EPOCH",
        "wait_timeout",
        "park_timeout",
        "recv_timeout",
        "RealClock",
    ];
    let sources = [
        ("rr-testkit/src/clock.rs", include_str!("../src/clock.rs")),
        (
            "rr-core/src/clock/virtual_clock.rs",
            include_str!("../../rr-core/src/clock/virtual_clock.rs"),
        ),
    ];
    for (name, src) in sources {
        // Production code only: stop at the unit-test module (which measures
        // real time on purpose) and skip comment lines (docs explain what the
        // code does *not* call).
        let code = src.split("#[cfg(test)]").next().unwrap_or(src);
        let mut checked = 0;
        for (n, line) in code.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            checked += 1;
            let line_no_mono = line.replace("MonoInstant", "");
            for bad in FORBIDDEN {
                assert!(
                    !line_no_mono.contains(bad),
                    "{name}:{}: real-time call `{bad}` in clock harness: {line}",
                    n + 1
                );
            }
        }
        assert!(checked > 100, "{name}: only {checked} code lines scanned");
    }
}
