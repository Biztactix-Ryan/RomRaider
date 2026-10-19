//! [`VirtualClock`]: a [`Clock`] whose time only moves when told to.
//!
//! Lives beside [`RealClock`](super::RealClock) in `rr_core::clock` so both
//! production crates (the ECU simulator serves a Sim Link with timing on a
//! virtual clock) and tests can use it; `rr-testkit` layers harness helpers on
//! top. It never reads OS time and never calls `std::thread::sleep`: manual
//! blocking is done with a [`Mutex`]/[`Condvar`] pair that is woken only by
//! another thread moving virtual time.
//!
//! # Modes
//!
//! * [`AdvanceMode::Manual`] — [`sleep_until`](Clock::sleep_until) blocks the
//!   calling thread until some other thread calls
//!   [`advance`](VirtualClock::advance) / [`advance_to`](VirtualClock::advance_to)
//!   to or past its deadline. Use it for simulator and multi-thread tests
//!   where a driver thread controls the timeline step by step.
//! * [`AdvanceMode::AutoAdvance`] — `sleep_until(deadline)` jumps virtual time
//!   forward to `deadline` (never backwards) and returns at once. Use it for
//!   single-threaded code under test: every sleep and timeout completes
//!   instantly and the elapsed virtual time is exact.
//!
//! [`advance`](VirtualClock::advance) and
//! [`advance_to`](VirtualClock::advance_to) work in both modes.
//!
//! # Wake order
//!
//! Every completed sleep is assigned a global, gap-free *wake order* number at
//! the moment it is satisfied, and sleepers **return from `sleep_until` in
//! that order**. When an advance satisfies several parked sleepers at once,
//! they are released earliest deadline first, ties broken by the order in
//! which the sleepers parked, and each released sleeper observes virtual time
//! at its own deadline ([`WakeInfo::woke_at`]). The order number is exposed
//! through [`VirtualClock::sleep_until_traced`] so tests can assert ordering
//! without racing on what the OS scheduler does after the call returns.
//!
//! # Multiple threads in auto-advance mode
//!
//! All clones share one timeline. In auto-advance mode each `sleep_until` call
//! moves that shared timeline forward to `max(now, deadline)` independently;
//! concurrent sleepers do **not** wait for one another. Consequently:
//!
//! * `now()` is monotonic non-decreasing across all threads;
//! * after `sleep_until(d)` returns, `now() >= d`;
//! * a thread may observe *more* virtual time passing across its own sleep than
//!   it asked for, because another thread's sleep moved the shared clock too.
//!   Elapsed time is exact only when no other thread advances concurrently.
//!
//! # Wall time
//!
//! The virtual wall clock starts at a configurable [`WallTime`]
//! ([`VirtualClock::DEFAULT_WALL_START`] unless set with
//! [`with_wall_start`](VirtualClock::with_wall_start)) and advances in step
//! with monotonic time. [`set_wall`](VirtualClock::set_wall) jumps it to any
//! value (forwards or backwards, simulating NTP/DST/user changes — EC-26)
//! without affecting the monotonic timeline.
//!
//! Advancing the wall clock is done on raw Unix offsets inside this module
//! (`State::wall_now`); [`WallTime`] itself deliberately has no `Duration`
//! arithmetic, so nothing outside a clock implementation can time with it
//! (see "Monotonic vs wall time" in the [parent module](super)).

use core::fmt;
use core::time::Duration;
use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use super::{Clock, MonoInstant, WallTime};

/// How a [`VirtualClock`] responds to [`Clock::sleep_until`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AdvanceMode {
    /// Sleepers block until another thread advances time past their deadline.
    Manual,
    /// A sleep jumps virtual time forward to its deadline and returns at once.
    AutoAdvance,
}

/// What a completed sleep observed; returned by
/// [`VirtualClock::sleep_until_traced`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WakeInfo {
    /// Virtual time at the moment the sleep was satisfied: the sleeper's
    /// deadline if it had to wait (or jump), otherwise the time at entry.
    pub woke_at: MonoInstant,
    /// Position of this wake in the clock's global wake sequence (0-based,
    /// gap-free, shared by all clones). Sleeps return in this order.
    pub order: u64,
}

/// A deterministic, shareable virtual [`Clock`].
///
/// Cloning is cheap and clones share the same timeline (it is an `Arc`
/// internally), so either clone it or wrap it in `Arc<dyn Clock>`. See the
/// [`AdvanceMode`] and the `clock::virtual_clock` module source docs for the
/// mode, wake-order and threading semantics.
#[derive(Clone)]
pub struct VirtualClock {
    shared: Arc<Shared>,
}

#[derive(Debug)]
struct Shared {
    state: Mutex<State>,
    /// Signalled on every state change a waiter could care about: time moved,
    /// a sleeper parked, a sleeper returned.
    changed: Condvar,
}

#[derive(Debug)]
struct State {
    mode: AdvanceMode,
    now: MonoInstant,
    /// Wall time at `wall_anchor`; wall = `wall_base + (now - wall_anchor)`.
    wall_base: WallTime,
    wall_anchor: MonoInstant,
    /// Parked (manual-mode) sleepers keyed by `(deadline, arrival)`, so
    /// iteration order is deadline order with ties in arrival order.
    parked: BTreeMap<(MonoInstant, u64), ()>,
    /// Sleepers released by an advance but not yet returned, keyed by arrival.
    released: BTreeMap<u64, WakeInfo>,
    next_arrival: u64,
    /// Next wake-order number to hand out.
    next_order: u64,
    /// Wake-order number of the next sleeper allowed to return.
    next_to_return: u64,
}

impl State {
    fn take_order(&mut self, woke_at: MonoInstant) -> WakeInfo {
        let order = self.next_order;
        self.next_order += 1;
        WakeInfo { woke_at, order }
    }

    fn wall_now(&self) -> WallTime {
        let since_anchor = self.now.saturating_duration_since(self.wall_anchor);
        WallTime::from_unix_offset(self.wall_base.unix_offset().saturating_add(since_anchor))
    }
}

impl VirtualClock {
    /// Default starting wall time: 2020-01-01T00:00:00Z.
    pub const DEFAULT_WALL_START: WallTime =
        WallTime::from_unix_offset(Duration::from_secs(1_577_836_800));

    /// A virtual clock at [`MonoInstant::ORIGIN`] with wall time
    /// [`DEFAULT_WALL_START`](Self::DEFAULT_WALL_START).
    #[must_use]
    pub fn new(mode: AdvanceMode) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    mode,
                    now: MonoInstant::ORIGIN,
                    wall_base: Self::DEFAULT_WALL_START,
                    wall_anchor: MonoInstant::ORIGIN,
                    parked: BTreeMap::new(),
                    released: BTreeMap::new(),
                    next_arrival: 0,
                    next_order: 0,
                    next_to_return: 0,
                }),
                changed: Condvar::new(),
            }),
        }
    }

    /// Shorthand for `VirtualClock::new(AdvanceMode::Manual)`.
    #[must_use]
    pub fn manual() -> Self {
        Self::new(AdvanceMode::Manual)
    }

    /// Shorthand for `VirtualClock::new(AdvanceMode::AutoAdvance)`.
    #[must_use]
    pub fn auto_advance() -> Self {
        Self::new(AdvanceMode::AutoAdvance)
    }

    /// Builder form of [`set_wall`](Self::set_wall): the wall clock reads
    /// `start` at the current monotonic time.
    #[must_use]
    pub fn with_wall_start(self, start: WallTime) -> Self {
        self.set_wall(start);
        self
    }

    /// The mode chosen at construction.
    #[must_use]
    pub fn mode(&self) -> AdvanceMode {
        self.lock().mode
    }

    /// Moves virtual time forward by `d` (saturating at [`MonoInstant::MAX`]).
    /// See [`advance_to`](Self::advance_to).
    pub fn advance(&self, d: Duration) {
        let mut state = self.lock();
        let target = state.now + d;
        self.advance_locked(&mut state, target);
    }

    /// Moves virtual time forward to `target`; a `target` at or before
    /// `now()` is a no-op (time never goes backwards).
    ///
    /// Parked sleepers whose deadline is `<= target` are released in deadline
    /// order (ties in arrival order), each with [`WakeInfo::woke_at`] equal to
    /// its own deadline; afterwards `now() == target`. This call does not wait
    /// for released sleepers to run.
    pub fn advance_to(&self, target: MonoInstant) {
        let mut state = self.lock();
        self.advance_locked(&mut state, target);
    }

    /// Jumps the virtual wall clock to `wall` (forwards or backwards). It then
    /// keeps advancing in step with monotonic time. The monotonic timeline,
    /// deadlines and sleepers are unaffected.
    pub fn set_wall(&self, wall: WallTime) {
        let mut state = self.lock();
        state.wall_base = wall;
        state.wall_anchor = state.now;
    }

    /// Number of threads currently parked in a manual-mode sleep.
    #[must_use]
    pub fn pending_sleepers(&self) -> usize {
        self.lock().parked.len()
    }

    /// Earliest deadline among parked sleepers, if any. A simulator driver
    /// can `advance_to` it to step event by event.
    #[must_use]
    pub fn next_deadline(&self) -> Option<MonoInstant> {
        self.lock()
            .parked
            .keys()
            .next()
            .map(|&(deadline, _)| deadline)
    }

    /// Blocks until at least `n` threads are parked in a manual-mode sleep.
    ///
    /// Test synchronisation helper: spawn N sleepers, wait for all N to park,
    /// then advance. It waits on virtual-clock state only (no real-time
    /// timeout), so it blocks forever if `n` sleepers never park.
    pub fn wait_for_sleepers(&self, n: usize) {
        let mut state = self.lock();
        while state.parked.len() < n {
            state = self.wait(state);
        }
    }

    /// [`Clock::sleep_until`], additionally reporting when the sleep was
    /// satisfied and its position in the global wake order.
    pub fn sleep_until_traced(&self, deadline: MonoInstant) -> WakeInfo {
        let mut state = self.lock();
        let wake = if deadline <= state.now {
            let now = state.now;
            state.take_order(now)
        } else {
            match state.mode {
                AdvanceMode::AutoAdvance => {
                    state.now = deadline;
                    state.take_order(deadline)
                }
                AdvanceMode::Manual => {
                    let arrival = state.next_arrival;
                    state.next_arrival += 1;
                    state.parked.insert((deadline, arrival), ());
                    self.shared.changed.notify_all();
                    loop {
                        if let Some(wake) = state.released.remove(&arrival) {
                            break wake;
                        }
                        state = self.wait(state);
                    }
                }
            }
        };
        // Return strictly in wake order.
        while state.next_to_return != wake.order {
            state = self.wait(state);
        }
        state.next_to_return += 1;
        self.shared.changed.notify_all();
        wake
    }

    fn advance_locked(&self, state: &mut State, target: MonoInstant) {
        if target <= state.now {
            return;
        }
        while let Some(entry) = state.parked.first_entry() {
            let (deadline, arrival) = *entry.key();
            if deadline > target {
                break;
            }
            entry.remove();
            let wake = state.take_order(deadline);
            state.released.insert(arrival, wake);
        }
        state.now = target;
        self.shared.changed.notify_all();
    }

    /// Locks the state, recovering from poisoning: every mutation leaves the
    /// state consistent before any point that could panic, so a poisoned lock
    /// still guards valid data.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn wait<'a>(&self, guard: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
        self.shared
            .changed
            .wait(guard)
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for VirtualClock {
    /// An auto-advance clock: the convenient choice for single-threaded tests.
    fn default() -> Self {
        Self::auto_advance()
    }
}

impl fmt::Debug for VirtualClock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.lock();
        f.debug_struct("VirtualClock")
            .field("mode", &state.mode)
            .field("now", &state.now)
            .field("wall_now", &state.wall_now())
            .field("pending_sleepers", &state.parked.len())
            .finish()
    }
}

impl Clock for VirtualClock {
    fn now(&self) -> MonoInstant {
        self.lock().now
    }

    /// Manual mode: blocks until another thread advances to `deadline`.
    /// Auto-advance mode: jumps time to `deadline` and returns at once.
    fn sleep_until(&self, deadline: MonoInstant) {
        let _ = self.sleep_until_traced(deadline);
    }

    fn wall_now(&self) -> WallTime {
        self.lock().wall_now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    const fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn at(n: u64) -> MonoInstant {
        MonoInstant::from_origin_offset(ms(n))
    }

    /// Real elapsed time, measured only to bound how long a test took.
    #[expect(
        clippy::disallowed_methods,
        reason = "test proves VirtualClock never blocks on real time, so it must measure real time (D-010)"
    )]
    fn real_elapsed_of(f: impl FnOnce()) -> Duration {
        let start = std::time::Instant::now();
        f();
        start.elapsed()
    }

    /// Tiny deterministic PRNG so multi-thread tests can vary their schedule
    /// without an external dependency.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }
    }

    /// Proves US-RR-18-1: on an auto-advance `VirtualClock`, sleeping 55 ms
    /// then 5000 ms advances virtual time by exactly 55 ms and then exactly
    /// 5055 ms in total, and the whole test body takes under 10 ms of real
    /// time. Single-threaded with no thread handoffs, so the real-time bound
    /// does not depend on the OS scheduler. Fails if a sleep over/under-shoots
    /// its deadline or if the clock ever blocks on real time.
    #[test]
    fn auto_advance_sleeps_are_exact_and_instant() {
        let clock = VirtualClock::auto_advance();
        let start = clock.now();
        let real = real_elapsed_of(|| {
            clock.sleep(ms(55));
            assert_eq!(clock.now() - start, ms(55));
            clock.sleep(ms(5000));
        });
        assert_eq!(clock.now() - start, ms(5055));
        assert_eq!(clock.now(), at(5055));
        assert!(real < ms(10), "took {real:?} of real time");
    }

    #[test]
    fn auto_advance_never_moves_backwards() {
        let clock = VirtualClock::auto_advance();
        clock.sleep_until(at(100));
        assert_eq!(clock.now(), at(100));
        let wake = clock.sleep_until_traced(at(40));
        assert_eq!(wake.woke_at, at(100));
        assert_eq!(clock.now(), at(100));
        clock.advance_to(at(10));
        assert_eq!(clock.now(), at(100));
        clock.sleep(Duration::MAX);
        assert_eq!(clock.now(), MonoInstant::MAX);
    }

    #[test]
    fn advance_moves_time_in_both_modes() {
        for clock in [VirtualClock::manual(), VirtualClock::auto_advance()] {
            assert_eq!(clock.now(), MonoInstant::ORIGIN);
            clock.advance(ms(7));
            clock.advance_to(at(20));
            assert_eq!(clock.now(), at(20));
        }
    }

    #[test]
    fn manual_sleep_on_past_deadline_returns_immediately() {
        let clock = VirtualClock::manual();
        clock.advance(ms(50));
        clock.sleep_until(at(10));
        clock.sleep_until(at(50));
        clock.sleep(Duration::ZERO);
        assert_eq!(clock.now(), at(50));
        assert_eq!(clock.pending_sleepers(), 0);
    }

    #[test]
    fn manual_wakes_sleepers_in_deadline_order() {
        let clock = VirtualClock::manual();
        // (label, deadline) in deliberately scrambled order with a tie; each
        // sleeper is parked before the next spawns, so tie arrival is fixed.
        let plan = [
            ("d", 40),
            ("a", 10),
            ("c", 30),
            ("b1", 20),
            ("e", 50),
            ("b2", 20),
        ];
        let mut handles = Vec::new();
        for (i, &(label, deadline)) in plan.iter().enumerate() {
            let c = clock.clone();
            handles.push(thread::spawn(move || {
                (label, deadline, c.sleep_until_traced(at(deadline)))
            }));
            clock.wait_for_sleepers(i + 1);
        }
        assert_eq!(clock.pending_sleepers(), plan.len());
        assert_eq!(clock.next_deadline(), Some(at(10)));

        clock.advance_to(at(1000));
        let mut woke: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        woke.sort_by_key(|&(_, _, w)| w.order);

        let labels: Vec<_> = woke.iter().map(|&(l, _, _)| l).collect();
        assert_eq!(labels, ["a", "b1", "b2", "c", "d", "e"]);
        for (i, &(_, deadline, wake)) in woke.iter().enumerate() {
            assert_eq!(wake.order, i as u64);
            assert_eq!(wake.woke_at, at(deadline));
        }
        assert_eq!(clock.now(), at(1000));
        assert_eq!(clock.pending_sleepers(), 0);
    }

    #[test]
    fn manual_advance_releases_only_due_sleepers() {
        let clock = VirtualClock::manual();
        let handles: Vec<_> = [10, 30, 60]
            .into_iter()
            .map(|d| {
                let c = clock.clone();
                thread::spawn(move || c.sleep_until_traced(at(d)))
            })
            .collect();
        clock.wait_for_sleepers(3);

        clock.advance(ms(30));
        assert_eq!(clock.pending_sleepers(), 1);
        assert_eq!(clock.next_deadline(), Some(at(60)));
        assert_eq!(clock.now(), at(30));

        clock.advance(ms(29));
        assert_eq!(clock.pending_sleepers(), 1);

        clock.advance(ms(1));
        assert_eq!(clock.pending_sleepers(), 0);
        assert_eq!(clock.next_deadline(), None);

        let wakes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(
            wakes.iter().map(|w| w.woke_at).collect::<Vec<_>>(),
            [at(10), at(30), at(60)]
        );
        assert_eq!(wakes.iter().map(|w| w.order).collect::<Vec<_>>(), [0, 1, 2]);
    }

    #[test]
    fn shared_auto_clock_gives_consistent_monotonic_timeline() {
        for seed in 1..=20_u64 {
            let clock: Arc<dyn Clock> = Arc::new(VirtualClock::auto_advance());
            let workers: Vec<_> = (0..2)
                .map(|t| {
                    let clock = Arc::clone(&clock);
                    thread::spawn(move || {
                        let mut rng = Lcg(seed * 31 + t);
                        let mut seen = vec![clock.now()];
                        let mut slept = Duration::ZERO;
                        for _ in 0..200 {
                            let d = ms(rng.next() % 50);
                            let before = clock.now();
                            clock.sleep(d);
                            let after = clock.now();
                            assert!(after - before >= d);
                            slept += d;
                            seen.push(before);
                            seen.push(after);
                        }
                        (seen, slept)
                    })
                })
                .collect();
            let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
            let end = clock.now();
            let mut max_slept = Duration::ZERO;
            let mut total_slept = Duration::ZERO;
            for (seen, slept) in &results {
                assert!(seen.windows(2).all(|w| w[0] <= w[1]), "seed {seed}");
                assert!(*seen.last().unwrap() <= end);
                max_slept = max_slept.max(*slept);
                total_slept += *slept;
            }
            let elapsed = end - MonoInstant::ORIGIN;
            assert!(elapsed >= max_slept, "seed {seed}");
            assert!(elapsed <= total_slept, "seed {seed}");
        }
    }

    /// Property tests for US-RR-18-2: threads sharing one `VirtualClock`
    /// observe a single consistent, monotonic timeline.
    ///
    /// Each case runs a randomly generated schedule on 2-4 real threads that
    /// share one clock, so the OS is free to interleave them however it
    /// likes; the properties must hold for *every* interleaving. Nothing here
    /// sleeps for real: auto-advance sleeps return at once, and manual-mode
    /// sleepers are released by a driver thread calling `advance`.
    ///
    /// Checked per case:
    ///
    /// * **per thread** — every reading (`now()` before an op, the
    ///   `WakeInfo::woke_at` of a sleep, `now()` after it) is non-decreasing in
    ///   program order, a sleep never wakes before its deadline, and the
    ///   thread's own wake-order numbers strictly increase;
    /// * **global order** — the wake-order numbers of all sleeps across all
    ///   threads are exactly `0..N` (gap-free, no duplicates), and listing the
    ///   wakes in that order gives non-decreasing `woke_at`: one timeline that
    ///   every thread agrees on;
    /// * **bounds** — no observation exceeds the clock's final `now()`, and
    ///   (auto-advance) the final time is at least what any single thread
    ///   slept/advanced and at most what all threads did together.
    mod timeline_props {
        use super::*;
        use proptest::prelude::*;

        /// What one thread does next.
        #[derive(Clone, Copy, Debug)]
        enum Op {
            /// `sleep_until_traced(deadline_after(d))`.
            Sleep(Duration),
            /// `advance(d)` (auto-advance schedules only).
            Advance(Duration),
            /// A bare `now()` read.
            Read,
        }

        /// What one thread saw for one op, in program order.
        #[derive(Clone, Copy, Debug)]
        struct Obs {
            before: MonoInstant,
            /// `(deadline, wake)` for a sleep.
            sleep: Option<(MonoInstant, WakeInfo)>,
            /// Duration of an `advance`.
            advanced: Option<Duration>,
            after: MonoInstant,
        }

        fn op(allow_advance: bool) -> impl Strategy<Value = Op> {
            let d = (0..=50_u64).prop_map(ms);
            if allow_advance {
                prop_oneof![
                    4 => d.clone().prop_map(Op::Sleep),
                    2 => d.prop_map(Op::Advance),
                    1 => Just(Op::Read),
                ]
                .boxed()
            } else {
                prop_oneof![4 => d.prop_map(Op::Sleep), 1 => Just(Op::Read)].boxed()
            }
        }

        /// 2-4 threads, each with 1-24 ops.
        fn schedules(allow_advance: bool) -> impl Strategy<Value = Vec<Vec<Op>>> {
            prop::collection::vec(prop::collection::vec(op(allow_advance), 1..=24), 2..=4)
        }

        fn run_thread(clock: &VirtualClock, ops: &[Op]) -> Vec<Obs> {
            ops.iter()
                .map(|&op| {
                    let before = clock.now();
                    let (sleep, advanced) = match op {
                        Op::Sleep(d) => {
                            let deadline = clock.deadline_after(d);
                            (Some((deadline, clock.sleep_until_traced(deadline))), None)
                        }
                        Op::Advance(d) => {
                            clock.advance(d);
                            (None, Some(d))
                        }
                        Op::Read => (None, None),
                    };
                    let after = clock.now();
                    Obs {
                        before,
                        sleep,
                        advanced,
                        after,
                    }
                })
                .collect()
        }

        /// The per-thread, global-order and final-bound properties shared by
        /// both modes (see the module docs).
        fn check_timeline(logs: &[Vec<Obs>], end: MonoInstant) -> Result<(), TestCaseError> {
            let mut wakes = Vec::new();
            for (t, log) in logs.iter().enumerate() {
                let mut prev = MonoInstant::ORIGIN;
                let mut prev_order = None;
                for (i, obs) in log.iter().enumerate() {
                    prop_assert!(prev <= obs.before, "thread {t} op {i}: now went back");
                    let mut cursor = obs.before;
                    if let Some((deadline, wake)) = obs.sleep {
                        prop_assert!(deadline >= obs.before, "thread {t} op {i}");
                        prop_assert!(wake.woke_at >= deadline, "thread {t} op {i}: woke early");
                        prop_assert!(cursor <= wake.woke_at, "thread {t} op {i}");
                        prop_assert!(
                            prev_order < Some(wake.order),
                            "thread {t} op {i}: wake order {} after {prev_order:?}",
                            wake.order
                        );
                        prev_order = Some(wake.order);
                        cursor = wake.woke_at;
                        wakes.push(wake);
                    }
                    if let Some(d) = obs.advanced {
                        prop_assert!(obs.after >= obs.before + d, "thread {t} op {i}");
                    }
                    prop_assert!(cursor <= obs.after, "thread {t} op {i}: now went back");
                    prop_assert!(obs.after <= end, "thread {t} op {i}: beyond final now");
                    prev = obs.after;
                }
            }
            wakes.sort_by_key(|w| w.order);
            for (i, w) in wakes.iter().enumerate() {
                prop_assert_eq!(w.order, i as u64, "wake order not gap-free 0..N");
            }
            for pair in wakes.windows(2) {
                prop_assert!(
                    pair[0].woke_at <= pair[1].woke_at,
                    "global wake order disagrees with time: {:?} then {:?}",
                    pair[0],
                    pair[1]
                );
            }
            Ok(())
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(64))]

            /// Proves US-RR-18-2 (auto-advance): 2-4 threads mixing sleeps,
            /// `advance` calls and reads on one shared clock see one
            /// consistent monotonic timeline.
            #[test]
            fn auto_advance_threads_share_one_monotonic_timeline(plan in schedules(true)) {
                let clock = VirtualClock::auto_advance();
                let logs: Vec<Vec<Obs>> = thread::scope(|s| {
                    let handles: Vec<_> = plan
                        .iter()
                        .map(|ops| {
                            let clock = clock.clone();
                            s.spawn(move || run_thread(&clock, ops))
                        })
                        .collect();
                    handles.into_iter().map(|h| h.join().unwrap()).collect()
                });
                let end = clock.now();
                check_timeline(&logs, end)?;

                // Every op moves time forward by at most its own duration, and
                // each thread's own ops are seen in full by that thread.
                let span = |ops: &[Op]| -> Duration {
                    ops.iter()
                        .map(|op| match *op {
                            Op::Sleep(d) | Op::Advance(d) => d,
                            Op::Read => Duration::ZERO,
                        })
                        .sum()
                };
                let elapsed = end - MonoInstant::ORIGIN;
                let total: Duration = plan.iter().map(|ops| span(ops)).sum();
                let longest = plan.iter().map(|ops| span(ops)).max().unwrap_or_default();
                prop_assert!(elapsed >= longest, "{elapsed:?} < longest thread {longest:?}");
                prop_assert!(elapsed <= total, "{elapsed:?} > all threads {total:?}");
            }

            /// Proves US-RR-18-2 (manual): 2-4 sleeper threads parked on one
            /// manual clock, released by a driver advancing in random steps,
            /// see one consistent monotonic timeline, and each parked sleeper
            /// wakes no earlier than its deadline.
            #[test]
            fn manual_threads_share_one_monotonic_timeline(
                plan in schedules(false),
                steps in prop::collection::vec((1..=40_u64).prop_map(ms), 1..=8),
            ) {
                let clock = VirtualClock::manual();
                let finished = std::sync::atomic::AtomicUsize::new(0);
                let logs: Vec<Vec<Obs>> = thread::scope(|s| {
                    let handles: Vec<_> = plan
                        .iter()
                        .map(|ops| {
                            let clock = clock.clone();
                            let finished = &finished;
                            s.spawn(move || {
                                let log = run_thread(&clock, ops);
                                finished.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                log
                            })
                        })
                        .collect();
                    // Driver: advance in the generated steps until every
                    // sleeper has finished. Yields, never sleeps.
                    for &step in steps.iter().cycle() {
                        if finished.load(std::sync::atomic::Ordering::SeqCst) == plan.len() {
                            break;
                        }
                        clock.advance(step);
                        thread::yield_now();
                    }
                    handles.into_iter().map(|h| h.join().unwrap()).collect()
                });
                let end = clock.now();
                check_timeline(&logs, end)?;
                prop_assert_eq!(clock.pending_sleepers(), 0);
            }
        }
    }

    #[test]
    fn shared_manual_clock_readers_see_monotonic_time_while_driver_advances() {
        let clock = VirtualClock::manual();
        let readers: Vec<_> = (0..2)
            .map(|_| {
                let clock = clock.clone();
                thread::spawn(move || {
                    let mut prev = clock.now();
                    while prev < at(500) {
                        let next = clock.now();
                        assert!(next >= prev, "{next:?} < {prev:?}");
                        prev = next;
                    }
                })
            })
            .collect();
        let sleeper = {
            let clock: Arc<dyn Clock> = Arc::new(clock.clone());
            thread::spawn(move || {
                clock.sleep_until(at(250));
                clock.now()
            })
        };
        clock.wait_for_sleepers(1);
        let mut rng = Lcg(7);
        while clock.now() < at(500) {
            clock.advance(ms(rng.next() % 5 + 1));
        }
        for r in readers {
            r.join().unwrap();
        }
        assert!(sleeper.join().unwrap() >= at(250));
    }

    #[test]
    fn wall_advances_with_monotonic_time_by_default() {
        let start = WallTime::from_unix_offset(Duration::from_secs(1_700_000_000));
        let clock = VirtualClock::auto_advance().with_wall_start(start);
        assert_eq!(clock.wall_now(), start);
        clock.sleep(ms(1500));
        assert_eq!(
            clock.wall_now(),
            WallTime::from_unix_offset(start.unix_offset() + ms(1500))
        );
        assert_eq!(
            VirtualClock::manual().wall_now(),
            VirtualClock::DEFAULT_WALL_START
        );
    }

    #[test]
    fn set_wall_jumps_do_not_affect_monotonic_time() {
        let clock = VirtualClock::auto_advance();
        clock.sleep(ms(100));
        let mono_before = clock.now();

        let back = WallTime::from_unix_offset(Duration::from_secs(1_000));
        clock.set_wall(back);
        assert_eq!(clock.wall_now(), back);
        assert_eq!(clock.now(), mono_before);

        let forward = WallTime::from_unix_offset(Duration::from_secs(4_000_000_000));
        clock.set_wall(forward);
        assert_eq!(clock.now(), mono_before);

        clock.sleep(ms(10));
        assert_eq!(clock.now(), mono_before + ms(10));
        assert_eq!(
            clock.wall_now(),
            WallTime::from_unix_offset(forward.unix_offset() + ms(10))
        );
        // Deadlines keep working across a wall jump.
        clock.set_wall(back);
        let deadline = clock.deadline_after(ms(5));
        clock.sleep_until(deadline);
        assert_eq!(clock.now(), deadline);
    }

    /// Proves US-RR-18-3 (part b): the wall clock is changed — backwards and
    /// forwards, repeatedly — *during* a run in which a sleeper thread is
    /// parked on deadlines and a reader thread samples `now()` continuously,
    /// and the monotonic timeline is unaffected: the reader never sees `now()`
    /// decrease, the driver sees exactly the time it advanced to after every
    /// jump, and the sleeper wakes exactly on its monotonic deadlines. Wall
    /// time is reported separately (`wall_now()` follows each jump and then
    /// advances with monotonic time).
    ///
    /// This is the deterministic stand-in for "the OS wall clock is changed
    /// during a `RealClock` test", which a test cannot do (see
    /// `clock::tests::real_clock_now_is_non_decreasing_across_a_span_with_wall_reported_separately`
    /// for part a and the `compile_fail` examples on `WallTime` for part c).
    #[test]
    fn wall_jumps_during_a_run_never_move_monotonic_time() {
        const DEADLINES: [u64; 3] = [100, 250, 400];
        const END: u64 = 500;
        let secs = |s| WallTime::from_unix_offset(Duration::from_secs(s));
        // Backwards, far forwards, back to the epoch, small step back, ...
        let jumps = [
            secs(1_000),
            secs(4_000_000_000),
            WallTime::UNIX_EPOCH,
            secs(1_577_836_000),
            secs(1_577_836_900),
        ];

        let clock = VirtualClock::manual();
        let sleeper = {
            let c = clock.clone();
            thread::spawn(move || {
                DEADLINES
                    .map(|d| (c.sleep_until_traced(at(d)).woke_at, c.now()))
                    .to_vec()
            })
        };
        // The reader takes its first sample before the driver starts, so it
        // is guaranteed to be sampling while the jumps happen.
        let started = Arc::new(std::sync::Barrier::new(2));
        let reader = {
            let c = clock.clone();
            let started = Arc::clone(&started);
            thread::spawn(move || {
                let mut prev = c.now();
                started.wait();
                let mut walls = 0_usize;
                while prev < at(END) {
                    let _label = c.wall_now(); // reported alongside, never compared
                    walls += 1;
                    let next = c.now();
                    assert!(next >= prev, "now went back: {next:?} < {prev:?}");
                    prev = next;
                }
                walls
            })
        };

        started.wait();
        let mut step = 0_usize;
        while clock.now() < at(END) {
            let target = clock.now() + ms(10);
            if DEADLINES.iter().any(|&d| at(d) == target) {
                // The sleeper is parked on exactly this deadline.
                clock.wait_for_sleepers(1);
            }
            clock.advance_to(target);
            let jump = jumps[step % jumps.len()];
            clock.set_wall(jump);
            step += 1;
            assert_eq!(clock.now(), target, "wall jump moved monotonic time");
            assert_eq!(clock.wall_now(), jump, "wall time reported separately");
        }
        // After the last jump the wall clock advances with monotonic time.
        let last = clock.wall_now();
        clock.advance(ms(7));
        assert_eq!(
            clock.wall_now(),
            WallTime::from_unix_offset(last.unix_offset() + ms(7))
        );

        let wakes = sleeper.join().unwrap();
        for (&d, &(woke_at, now_after)) in DEADLINES.iter().zip(&wakes) {
            assert_eq!(woke_at, at(d), "sleeper woke off its monotonic deadline");
            assert!(now_after >= woke_at);
        }
        assert!(reader.join().unwrap() > 0);
        assert_eq!(step, 50);
    }

    #[test]
    fn clones_share_one_timeline_and_types_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<VirtualClock>();
        let a = VirtualClock::manual();
        let b = a.clone();
        a.advance(ms(3));
        assert_eq!(b.now(), at(3));
        assert_eq!(b.mode(), AdvanceMode::Manual);
        assert_eq!(VirtualClock::default().mode(), AdvanceMode::AutoAdvance);
        let dbg = format!("{a:?}");
        assert!(
            dbg.contains("VirtualClock") && dbg.contains("Manual"),
            "{dbg}"
        );
    }

    #[test]
    fn wake_order_is_global_across_modes_of_satisfaction() {
        let clock = VirtualClock::auto_advance();
        assert_eq!(clock.sleep_until_traced(at(5)).order, 0);
        assert_eq!(clock.sleep_until_traced(at(1)).order, 1);
        let w = clock.sleep_until_traced(at(9));
        assert_eq!((w.order, w.woke_at), (2, at(9)));
    }
}
