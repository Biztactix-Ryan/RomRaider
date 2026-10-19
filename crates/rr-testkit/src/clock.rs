//! VirtualClock test harness (concept C9, decision D-010).
//!
//! [`TestClock`] wraps an [`rr_core::clock::VirtualClock`] and adds the things
//! a timing test needs on top of it: advancing time by hand
//! ([`advance_by`](TestClock::advance_by)), driving worker threads through
//! their sleeps step by step until a condition holds
//! ([`run_until`](TestClock::run_until), [`run_to`](TestClock::run_to),
//! [`step`](TestClock::step)), spawning tracked worker threads with a clone of
//! the clock ([`spawn`](TestClock::spawn)), and asserting how much *virtual*
//! time passed ([`assert_elapsed`], [`assert_elapsed_within`]).
//!
//! Nothing here reads OS time or sleeps for real: a test that simulates ten
//! seconds of timeouts completes in microseconds and its timing is exact.
//!
//! # Driving worker threads deterministically
//!
//! In [`AdvanceMode::Manual`] a worker calling `sleep_until` parks until the
//! driver moves time to its deadline. The driver must not move time again
//! until every worker has reacted to the previous step (woken, done its work,
//! and parked again or finished) — otherwise one advance could release two of
//! a worker's sleeps at once and the test would depend on the OS scheduler.
//!
//! `TestClock` makes that "everyone has reacted" point observable. It counts
//! **tracked workers** — threads started with [`TestClock::spawn`], or any
//! thread holding a [`WorkerGuard`] from [`TestClock::worker_guard`] — and
//! calls the system *settled* when every tracked worker is either finished or
//! parked in a sleep on the clock. [`settle`](TestClock::settle) waits for
//! that point (yielding the CPU, never sleeping); the stepping helpers settle
//! before every decision, so each step releases exactly the sleepers due at
//! one deadline and the predicate is evaluated at a quiet, reproducible
//! moment.
//!
//! Rules for tests:
//!
//! * every thread that sleeps on the clock must be tracked (an untracked
//!   parked thread would be miscounted as one of the tracked workers);
//! * a tracked worker must not block on anything but the clock while the
//!   driver is settling (e.g. waiting on a channel the driver feeds), or
//!   settling never completes.
//!
//! In [`AdvanceMode::AutoAdvance`] workers never park — their sleeps jump time
//! themselves — so settling simply waits for all tracked workers to finish.
//!
//! # Example
//!
//! A worker polls something three times, 100 ms apart; the test drives it
//! and checks the exact virtual times at which it woke.
//!
//! ```
//! use std::sync::{Arc, Mutex};
//! use std::time::Duration;
//!
//! use rr_core::clock::Clock;
//! use rr_testkit::clock::TestClock;
//!
//! let clock = TestClock::manual();
//! let start = clock.now();
//! let wakes = Arc::new(Mutex::new(Vec::new()));
//!
//! let log = Arc::clone(&wakes);
//! let worker = clock.spawn(move |c| {
//!     for _ in 0..3 {
//!         c.sleep(Duration::from_millis(100));
//!         log.lock().unwrap().push(c.now());
//!     }
//! });
//!
//! // Step the worker through its sleeps until it has woken three times,
//! // giving up if that takes more than one virtual second.
//! clock
//!     .run_until(Duration::from_secs(1), || wakes.lock().unwrap().len() == 3)
//!     .unwrap();
//! worker.join();
//!
//! let at = |ms| start + Duration::from_millis(ms);
//! assert_eq!(*wakes.lock().unwrap(), [at(100), at(200), at(300)]);
//! clock.assert_elapsed(start, Duration::from_millis(300));
//! ```

use core::fmt;
use core::time::Duration;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use rr_core::clock::{AdvanceMode, Clock, MonoInstant, VirtualClock, WallTime};

/// Virtual time elapsed on `clock` since `start` (zero if `start` is in the
/// future).
#[must_use]
pub fn elapsed_since<C: Clock + ?Sized>(clock: &C, start: MonoInstant) -> Duration {
    clock.now().saturating_duration_since(start)
}

/// Asserts that exactly `expected` virtual time has passed on `clock` since
/// `start`.
///
/// # Panics
///
/// Panics — this is an assertion helper — with a message naming the expected
/// and actual elapsed time when they differ.
#[track_caller]
pub fn assert_elapsed<C: Clock + ?Sized>(clock: &C, start: MonoInstant, expected: Duration) {
    let now = clock.now();
    let actual = now.saturating_duration_since(start);
    assert!(
        actual == expected,
        "virtual elapsed time mismatch: expected {expected:?}, got {actual:?} \
         (start {start:?}, now {now:?})"
    );
}

/// Asserts that the virtual time passed on `clock` since `start` lies in
/// `min..=max`.
///
/// # Panics
///
/// Panics — this is an assertion helper — with a message naming the bounds
/// and the actual elapsed time when it is out of range, or when `min > max`.
#[track_caller]
pub fn assert_elapsed_within<C: Clock + ?Sized>(
    clock: &C,
    start: MonoInstant,
    min: Duration,
    max: Duration,
) {
    assert!(
        min <= max,
        "assert_elapsed_within: empty range, min {min:?} > max {max:?}"
    );
    let now = clock.now();
    let actual = now.saturating_duration_since(start);
    assert!(
        (min..=max).contains(&actual),
        "virtual elapsed time out of range: expected {min:?}..={max:?}, got {actual:?} \
         (start {start:?}, now {now:?})"
    );
}

/// Why [`TestClock::run_until`] stopped without its condition holding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RunUntilError {
    /// The next sleeper's deadline lies beyond the virtual-time budget. Time
    /// is left where the last step put it (`now`).
    #[error(
        "condition still false after the virtual-time budget: now {now:?}, \
         budget ends {budget_end:?}, next deadline {next_deadline:?}"
    )]
    BudgetExhausted {
        /// Virtual time when the run stopped.
        now: MonoInstant,
        /// `start + budget`: the latest time the run was allowed to reach.
        budget_end: MonoInstant,
        /// The deadline that would have been stepped to next.
        next_deadline: MonoInstant,
    },
    /// Nothing is left to drive: no thread is parked on the clock, so
    /// advancing time cannot change anything, yet the condition is false.
    #[error("condition still false and no thread is sleeping on the clock (now {now:?})")]
    Stalled {
        /// Virtual time when the run stopped.
        now: MonoInstant,
    },
}

/// A [`VirtualClock`] plus test-driving helpers. See the [module docs](self).
///
/// Cloning is cheap; clones share the timeline *and* the tracked-worker count.
/// `TestClock` is itself a [`Clock`], so it can be handed to code under test
/// directly (or use [`clock`](Self::clock) / [`shared`](Self::shared)).
#[derive(Clone)]
pub struct TestClock {
    clock: VirtualClock,
    live_workers: Arc<AtomicUsize>,
}

impl TestClock {
    /// A fresh virtual clock at [`MonoInstant::ORIGIN`] in `mode`.
    #[must_use]
    pub fn new(mode: AdvanceMode) -> Self {
        Self::from_virtual(VirtualClock::new(mode))
    }

    /// A manual-mode clock: sleepers park until the test advances time.
    #[must_use]
    pub fn manual() -> Self {
        Self::new(AdvanceMode::Manual)
    }

    /// An auto-advance clock: every sleep jumps time forward and returns at
    /// once.
    #[must_use]
    pub fn auto_advance() -> Self {
        Self::new(AdvanceMode::AutoAdvance)
    }

    /// Wraps an existing virtual clock (for example one built with
    /// [`VirtualClock::with_wall_start`]). The new `TestClock` starts with no
    /// tracked workers.
    #[must_use]
    pub fn from_virtual(clock: VirtualClock) -> Self {
        Self {
            clock,
            live_workers: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// The underlying virtual clock (for `pending_sleepers`,
    /// `sleep_until_traced`, `set_wall`, ...).
    #[must_use]
    pub fn virtual_clock(&self) -> &VirtualClock {
        &self.clock
    }

    /// A clone of the underlying virtual clock, sharing this timeline.
    #[must_use]
    pub fn clock(&self) -> VirtualClock {
        self.clock.clone()
    }

    /// The timeline as a shared trait object, the form most production code
    /// takes its clock in.
    #[must_use]
    pub fn shared(&self) -> Arc<dyn Clock> {
        Arc::new(self.clock.clone())
    }

    /// Current virtual monotonic time.
    #[must_use]
    pub fn now(&self) -> MonoInstant {
        self.clock.now()
    }

    /// Current virtual wall time.
    #[must_use]
    pub fn wall_now(&self) -> WallTime {
        self.clock.wall_now()
    }

    /// Moves virtual time forward by `d` in one jump, releasing every parked
    /// sleeper whose deadline is reached (see [`VirtualClock::advance_to`]).
    /// It does not wait for released workers to run; use
    /// [`run_for`](Self::run_for) to step through intermediate deadlines.
    pub fn advance_by(&self, d: Duration) {
        self.clock.advance(d);
    }

    /// Moves virtual time forward to `target` in one jump (no-op if `target`
    /// is not in the future). See [`advance_by`](Self::advance_by).
    pub fn advance_to(&self, target: MonoInstant) {
        self.clock.advance_to(target);
    }

    /// Virtual time elapsed since `start`.
    #[must_use]
    pub fn elapsed_since(&self, start: MonoInstant) -> Duration {
        elapsed_since(&self.clock, start)
    }

    /// Asserts exactly `expected` virtual time passed since `start`. See
    /// [`assert_elapsed`].
    ///
    /// # Panics
    ///
    /// When the elapsed time differs from `expected`.
    #[track_caller]
    pub fn assert_elapsed(&self, start: MonoInstant, expected: Duration) {
        assert_elapsed(&self.clock, start, expected);
    }

    /// Asserts the virtual time passed since `start` is in `min..=max`. See
    /// [`assert_elapsed_within`].
    ///
    /// # Panics
    ///
    /// When the elapsed time is out of range or `min > max`.
    #[track_caller]
    pub fn assert_elapsed_within(&self, start: MonoInstant, min: Duration, max: Duration) {
        assert_elapsed_within(&self.clock, start, min, max);
    }

    /// Registers the calling (or about-to-be-started) thread as a tracked
    /// worker until the guard is dropped. [`spawn`](Self::spawn) does this
    /// for you; use it for threads started by code under test.
    #[must_use = "the thread is only tracked while the guard is alive"]
    pub fn worker_guard(&self) -> WorkerGuard {
        self.live_workers.fetch_add(1, Ordering::AcqRel);
        WorkerGuard {
            live_workers: Arc::clone(&self.live_workers),
        }
    }

    /// Number of tracked workers that have not finished.
    #[must_use]
    pub fn live_workers(&self) -> usize {
        self.live_workers.load(Ordering::Acquire)
    }

    /// Spawns a tracked worker thread running `f` with a clone of the
    /// virtual clock. The worker counts as live from this call until `f`
    /// returns or panics.
    pub fn spawn<F, T>(&self, f: F) -> Worker<T>
    where
        F: FnOnce(VirtualClock) -> T + Send + 'static,
        T: Send + 'static,
    {
        // Registered before the thread starts, so a settle issued right
        // after `spawn` already waits for this worker.
        let guard = self.worker_guard();
        let clock = self.clock.clone();
        let handle = thread::spawn(move || {
            let _guard = guard;
            f(clock)
        });
        Worker { handle }
    }

    /// Waits until every tracked worker has finished or is parked in a sleep
    /// on the clock. Spins with [`thread::yield_now`]; never reads or waits
    /// on real time. See the [module docs](self) for the rules that make
    /// this terminate.
    pub fn settle(&self) {
        loop {
            let live = self.live_workers();
            if live == 0 || self.clock.pending_sleepers() >= live {
                return;
            }
            thread::yield_now();
        }
    }

    /// Settles, then advances to the earliest parked deadline, releasing the
    /// sleeper(s) due then, and settles again. Returns the deadline stepped
    /// to, or `None` (time unchanged) if no thread is parked.
    pub fn step(&self) -> Option<MonoInstant> {
        self.settle();
        let deadline = self.clock.next_deadline()?;
        self.clock.advance_to(deadline);
        self.settle();
        Some(deadline)
    }

    /// Steps through every parked deadline up to `target` in order (settling
    /// in between, so sleeps a worker starts along the way are honoured),
    /// then moves time to exactly `target` and settles. A `target` in the
    /// past leaves time unchanged.
    pub fn run_to(&self, target: MonoInstant) {
        loop {
            self.settle();
            match self.clock.next_deadline() {
                Some(deadline) if deadline <= target => self.clock.advance_to(deadline),
                _ => break,
            }
        }
        self.clock.advance_to(target);
        self.settle();
    }

    /// [`run_to`](Self::run_to) `now() + d`.
    pub fn run_for(&self, d: Duration) {
        let target = self.now().saturating_add(d);
        self.run_to(target);
    }

    /// Drives the timeline until `done()` is true.
    ///
    /// Each iteration settles, evaluates `done()`, and — if it is false —
    /// advances to the next parked deadline. Stops with an error when that
    /// deadline lies beyond `now() + budget` (time stays where the last step
    /// left it) or when nothing is parked. Returns the virtual time at which
    /// `done()` first held.
    ///
    /// # Errors
    ///
    /// [`RunUntilError::BudgetExhausted`] or [`RunUntilError::Stalled`].
    pub fn run_until(
        &self,
        budget: Duration,
        mut done: impl FnMut() -> bool,
    ) -> Result<MonoInstant, RunUntilError> {
        let budget_end = self.now().saturating_add(budget);
        loop {
            self.settle();
            if done() {
                return Ok(self.now());
            }
            match self.clock.next_deadline() {
                Some(deadline) if deadline <= budget_end => self.clock.advance_to(deadline),
                Some(next_deadline) => {
                    return Err(RunUntilError::BudgetExhausted {
                        now: self.now(),
                        budget_end,
                        next_deadline,
                    });
                }
                None => return Err(RunUntilError::Stalled { now: self.now() }),
            }
        }
    }
}

impl Default for TestClock {
    /// An auto-advance clock, like [`VirtualClock::default`].
    fn default() -> Self {
        Self::auto_advance()
    }
}

impl fmt::Debug for TestClock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TestClock")
            .field("clock", &self.clock)
            .field("live_workers", &self.live_workers())
            .finish()
    }
}

impl Clock for TestClock {
    fn now(&self) -> MonoInstant {
        self.clock.now()
    }

    fn sleep_until(&self, deadline: MonoInstant) {
        self.clock.sleep_until(deadline);
    }

    fn wall_now(&self) -> WallTime {
        self.clock.wall_now()
    }
}

/// Marks a thread as a tracked worker of a [`TestClock`] while alive. Dropping
/// it (including during a panic unwind) un-tracks the thread.
#[derive(Debug)]
pub struct WorkerGuard {
    live_workers: Arc<AtomicUsize>,
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        self.live_workers.fetch_sub(1, Ordering::AcqRel);
    }
}

/// A tracked worker thread started by [`TestClock::spawn`].
#[derive(Debug)]
pub struct Worker<T> {
    handle: thread::JoinHandle<T>,
}

impl<T> Worker<T> {
    /// Waits for the worker and returns its result. A panic in the worker is
    /// re-raised on the joining thread, so it fails the test with the
    /// worker's own message.
    pub fn join(self) -> T {
        match self.handle.join() {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    /// Whether the worker thread has finished.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }
}
