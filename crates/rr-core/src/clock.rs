//! Injected time (concept C9, decision D-010).
//!
//! Every timed behaviour in the project — comms deadlines, inter-byte gaps,
//! quirk delays, logger sample timestamps — reads time from a [`Clock`]
//! instead of calling `std::time` directly. Production code is handed a
//! [`RealClock`]; tests (and the ECU simulator) are handed a [`VirtualClock`]
//! whose time only moves when told to, which makes timing tests deterministic
//! and instant. `rr-testkit` adds harness helpers on top.
//!
//! # Two kinds of time
//!
//! The clock exposes two deliberately distinct types (EC-26):
//!
//! * [`MonoInstant`] — a point on the clock's **monotonic** timeline. It never
//!   goes backwards, is unaffected by NTP corrections, DST or the user
//!   changing the system clock, and is what all deadlines and durations are
//!   computed from. It is a crate-owned type (not `std::time::Instant`) so a
//!   virtual clock can manufacture values without touching real time.
//! * [`WallTime`] — civil "time of day" since the Unix epoch, for log and
//!   trace timestamps only. It may jump in either direction and must never
//!   be used to measure elapsed time or compute a deadline.
//!
//! There is no conversion between the two.
//!
//! # Monotonic vs wall time (EC-26)
//!
//! EC-26 (PROJECT.md edge cases): wall-clock jumps — NTP corrections, DST,
//! the user changing the system clock — can happen mid-session, e.g. during a
//! long logging run. The rule that keeps timing correct regardless:
//!
//! * **All timing uses [`MonoInstant`]**: deadlines, timeouts, inter-byte
//!   gaps, sample intervals, "how long did that take". `Clock::now`,
//!   `Clock::sleep_until`, `Clock::deadline_after` speak only `MonoInstant`.
//! * **[`WallTime`] is for labelling only**: log/trace timestamps and file
//!   metadata, recorded *alongside* the monotonic reading, never instead of
//!   it. It supports ordering and reading its Unix offset, nothing else — no
//!   `Duration` arithmetic and no difference of two wall times, so it cannot
//!   be used to measure an interval.
//! * **The types prevent mixing.** There is no `From`/`Into`, comparison or
//!   arithmetic between the two, and no API accepts one where the other is
//!   expected. The [`WallTime`] docs carry `compile_fail` examples that prove
//!   this at build time. (The only route between them is deliberately
//!   unwrapping one to a raw [`Duration`] and rebuilding the other with
//!   `MonoInstant::from_origin_offset` / `WallTime::from_unix_offset` — a
//!   visible, greppable act, never an accident.)
//!
//! [`RealClock`] reads the OS monotonic clock for `now()` and the OS system
//! clock for `wall_now()`, so a system clock change moves only the latter.
//! [`VirtualClock::set_wall`] simulates such a jump in tests.
//!
//! # Why `Arc<dyn Clock>`
//!
//! [`Clock`] is object-safe and requires `Send + Sync`, and the intended way
//! to hold one is `Arc<dyn Clock>`:
//!
//! * ARCHITECTURE.md ("Threading") runs each live Link on its own OS thread,
//!   and the ECU simulator, link, comms engine and logger must all observe
//!   the *same* timeline. A shared, thread-safe handle is the natural shape.
//! * Clock choice is a runtime wiring decision (real hardware vs. simulator
//!   vs. test), not something every comms type should be generic over; a
//!   type parameter would leak into every struct that times anything.
//! * Comms timing is on the millisecond scale, so a virtual call per `now()`
//!   is irrelevant.
//!
//! Generic code still works: blanket impls make `&C`, `Box<C>` and `Arc<C>`
//! clocks too, so `fn f(clock: &impl Clock)` accepts any of them.

use core::fmt;
use core::ops::{Add, AddAssign, Sub, SubAssign};
use core::time::Duration;
use std::sync::Arc;

mod virtual_clock;

pub use virtual_clock::{AdvanceMode, VirtualClock, WakeInfo};

/// A point on a [`Clock`]'s monotonic timeline.
///
/// Internally this is the elapsed time since an arbitrary, clock-specific
/// origin. Values are only meaningful relative to other values read from the
/// **same** clock (or a clone of it); comparing instants from two unrelated
/// clocks is a logic error, exactly as with `std::time::Instant`.
///
/// Arithmetic saturates rather than panicking: adding a [`Duration`] clamps at
/// [`MonoInstant::MAX`], subtracting clamps at the origin, and the difference
/// of two instants is zero if the right-hand side is later. Use the
/// `checked_*` methods when overflow must be detected.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct MonoInstant {
    since_origin: Duration,
}

impl MonoInstant {
    /// The clock's origin (offset zero).
    pub const ORIGIN: Self = Self {
        since_origin: Duration::ZERO,
    };

    /// The latest representable instant; used as "never" for deadlines.
    pub const MAX: Self = Self {
        since_origin: Duration::MAX,
    };

    /// Builds an instant from an offset since the clock's origin.
    ///
    /// Intended for [`Clock`] implementations (e.g. a virtual clock) and for
    /// tests; ordinary code obtains instants from [`Clock::now`].
    #[must_use]
    pub const fn from_origin_offset(since_origin: Duration) -> Self {
        Self { since_origin }
    }

    /// The offset of this instant from the clock's origin.
    #[must_use]
    pub const fn origin_offset(self) -> Duration {
        self.since_origin
    }

    /// Time elapsed from `earlier` to `self`, or zero if `earlier` is later.
    #[must_use]
    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        self.since_origin.saturating_sub(earlier.since_origin)
    }

    /// Time elapsed from `earlier` to `self`, or `None` if `earlier` is later.
    #[must_use]
    pub fn checked_duration_since(self, earlier: Self) -> Option<Duration> {
        self.since_origin.checked_sub(earlier.since_origin)
    }

    /// `self + d`, or `None` on overflow.
    #[must_use]
    pub fn checked_add(self, d: Duration) -> Option<Self> {
        self.since_origin
            .checked_add(d)
            .map(Self::from_origin_offset)
    }

    /// `self - d`, or `None` if that would precede the origin.
    #[must_use]
    pub fn checked_sub(self, d: Duration) -> Option<Self> {
        self.since_origin
            .checked_sub(d)
            .map(Self::from_origin_offset)
    }

    /// `self + d`, clamped at [`MonoInstant::MAX`].
    #[must_use]
    pub fn saturating_add(self, d: Duration) -> Self {
        Self::from_origin_offset(self.since_origin.saturating_add(d))
    }

    /// `self - d`, clamped at [`MonoInstant::ORIGIN`].
    #[must_use]
    pub fn saturating_sub(self, d: Duration) -> Self {
        Self::from_origin_offset(self.since_origin.saturating_sub(d))
    }
}

impl fmt::Debug for MonoInstant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MonoInstant(+{:?})", self.since_origin)
    }
}

impl Add<Duration> for MonoInstant {
    type Output = Self;
    /// Saturating; see the type-level docs.
    fn add(self, rhs: Duration) -> Self {
        self.saturating_add(rhs)
    }
}

impl AddAssign<Duration> for MonoInstant {
    fn add_assign(&mut self, rhs: Duration) {
        *self = *self + rhs;
    }
}

impl Sub<Duration> for MonoInstant {
    type Output = Self;
    /// Saturating; see the type-level docs.
    fn sub(self, rhs: Duration) -> Self {
        self.saturating_sub(rhs)
    }
}

impl SubAssign<Duration> for MonoInstant {
    fn sub_assign(&mut self, rhs: Duration) {
        *self = *self - rhs;
    }
}

impl Sub for MonoInstant {
    type Output = Duration;
    /// Saturating: zero if `rhs` is later than `self`.
    fn sub(self, rhs: Self) -> Duration {
        self.saturating_duration_since(rhs)
    }
}

/// Wall-clock ("civil") time, as an offset from the Unix epoch
/// (1970-01-01T00:00:00Z).
///
/// For timestamps in logs, traces and exported files only. Wall time can jump
/// forwards or backwards (NTP, DST, manual changes — EC-26), so it is
/// intentionally a separate type from [`MonoInstant`] with no conversion and
/// no arithmetic producing elapsed time.
///
/// # Intended use
///
/// Record the wall time next to the monotonic reading; time with the latter.
///
/// ```
/// use core::time::Duration;
/// use rr_core::clock::{Clock, MonoInstant, VirtualClock, WallTime};
///
/// let clock = VirtualClock::auto_advance();
/// let started: MonoInstant = clock.now();
/// let stamp: WallTime = clock.wall_now(); // label only
/// clock.sleep_until(clock.deadline_after(Duration::from_millis(5)));
/// assert_eq!(clock.now() - started, Duration::from_millis(5));
/// assert!(clock.wall_now() >= stamp); // ordering among WallTimes is fine
/// let _secs: u64 = stamp.unix_offset().as_secs(); // for formatting
/// ```
///
/// # Not interchangeable with `MonoInstant`
///
/// Each of the following fails to compile, with exactly one error: the one
/// pinned on the fence. (Stable rustdoc does not enforce the pinned code;
/// it was confirmed by building each snippet as a normal doctest, and
/// nightly rustdoc checks it.)
///
/// Subtracting a wall time from a monotonic instant:
///
/// ```compile_fail,E0277
/// use rr_core::clock::{Clock, VirtualClock};
/// let clock = VirtualClock::auto_advance();
/// let _ = clock.now() - clock.wall_now();
/// ```
///
/// Measuring elapsed time with wall time (no `WallTime - WallTime`):
///
/// ```compile_fail,E0369
/// use rr_core::clock::{Clock, VirtualClock};
/// let clock = VirtualClock::auto_advance();
/// let a = clock.wall_now();
/// let _elapsed = clock.wall_now() - a;
/// ```
///
/// Building a deadline from wall time (no `WallTime + Duration`):
///
/// ```compile_fail,E0369
/// use core::time::Duration;
/// use rr_core::clock::{Clock, VirtualClock};
/// let clock = VirtualClock::auto_advance();
/// let _deadline = clock.wall_now() + Duration::from_millis(50);
/// ```
///
/// Comparing a wall time against a monotonic deadline:
///
/// ```compile_fail,E0308
/// use core::time::Duration;
/// use rr_core::clock::{Clock, VirtualClock};
/// let clock = VirtualClock::auto_advance();
/// let deadline = clock.deadline_after(Duration::from_millis(50));
/// let _expired = clock.wall_now() >= deadline;
/// ```
///
/// Sleeping until a wall time:
///
/// ```compile_fail,E0308
/// use rr_core::clock::{Clock, VirtualClock};
/// let clock = VirtualClock::auto_advance();
/// clock.sleep_until(clock.wall_now());
/// ```
///
/// Passing a wall time where a deadline is expected:
///
/// ```compile_fail,E0308
/// use rr_core::clock::{Clock, MonoInstant, VirtualClock};
/// fn wait_for_reply(_deadline: MonoInstant) {}
/// let clock = VirtualClock::auto_advance();
/// wait_for_reply(clock.wall_now());
/// ```
///
/// Converting either way with `From` / `Into`:
///
/// ```compile_fail,E0277
/// use rr_core::clock::{Clock, MonoInstant, VirtualClock};
/// let clock = VirtualClock::auto_advance();
/// let _mono: MonoInstant = clock.wall_now().into();
/// ```
///
/// ```compile_fail,E0308
/// use rr_core::clock::{Clock, VirtualClock, WallTime};
/// let clock = VirtualClock::auto_advance();
/// let _wall = WallTime::from(clock.now());
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct WallTime {
    since_unix_epoch: Duration,
}

impl WallTime {
    /// The Unix epoch itself.
    pub const UNIX_EPOCH: Self = Self {
        since_unix_epoch: Duration::ZERO,
    };

    /// Builds a wall time from an offset since the Unix epoch.
    #[must_use]
    pub const fn from_unix_offset(since_unix_epoch: Duration) -> Self {
        Self { since_unix_epoch }
    }

    /// The offset of this wall time from the Unix epoch.
    #[must_use]
    pub const fn unix_offset(self) -> Duration {
        self.since_unix_epoch
    }
}

impl fmt::Debug for WallTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WallTime(unix+{:?})", self.since_unix_epoch)
    }
}

/// Source of time for everything that waits, times out or timestamps.
///
/// Object-safe and `Send + Sync`; share it as `Arc<dyn Clock>` (see the
/// [module docs](self) for why).
///
/// # Contract for implementors
///
/// * [`now`](Clock::now) is monotonic non-decreasing across all threads that
///   share the clock.
/// * [`sleep_until`](Clock::sleep_until) returns only once `now() >= deadline`
///   and returns immediately if the deadline has already passed.
/// * [`wall_now`](Clock::wall_now) is independent of the monotonic timeline.
pub trait Clock: Send + Sync + fmt::Debug {
    /// The current point on this clock's monotonic timeline.
    fn now(&self) -> MonoInstant;

    /// Blocks the calling thread until `now() >= deadline`. Returns
    /// immediately if `deadline` is not in the future.
    fn sleep_until(&self, deadline: MonoInstant);

    /// The current wall-clock time, for timestamps only.
    fn wall_now(&self) -> WallTime;

    /// Blocks for at least `d` of this clock's time.
    fn sleep(&self, d: Duration) {
        self.sleep_until(self.deadline_after(d));
    }

    /// The instant `d` from now (saturating at [`MonoInstant::MAX`]).
    fn deadline_after(&self, d: Duration) -> MonoInstant {
        self.now() + d
    }
}

impl<C: Clock + ?Sized> Clock for &C {
    fn now(&self) -> MonoInstant {
        (**self).now()
    }
    fn sleep_until(&self, deadline: MonoInstant) {
        (**self).sleep_until(deadline);
    }
    fn wall_now(&self) -> WallTime {
        (**self).wall_now()
    }
}

impl<C: Clock + ?Sized> Clock for Box<C> {
    fn now(&self) -> MonoInstant {
        (**self).now()
    }
    fn sleep_until(&self, deadline: MonoInstant) {
        (**self).sleep_until(deadline);
    }
    fn wall_now(&self) -> WallTime {
        (**self).wall_now()
    }
}

impl<C: Clock + ?Sized> Clock for Arc<C> {
    fn now(&self) -> MonoInstant {
        (**self).now()
    }
    fn sleep_until(&self, deadline: MonoInstant) {
        (**self).sleep_until(deadline);
    }
    fn wall_now(&self) -> WallTime {
        (**self).wall_now()
    }
}

/// The production clock, backed by the operating system.
///
/// This is the **only** code in the workspace sanctioned to call `std::time`
/// / `std::thread::sleep` (D-010); every such call lives in the private
/// `std_time` module below.
///
/// Each `RealClock` has its own origin, fixed when it is created; clones share
/// that origin and therefore the same timeline. Create one at startup and
/// share it (`Arc<dyn Clock>`) rather than creating several.
#[derive(Clone, Debug)]
pub struct RealClock {
    origin: std::time::Instant,
}

impl RealClock {
    /// A real clock whose monotonic origin is "now".
    #[must_use]
    pub fn new() -> Self {
        Self {
            origin: std_time::mono_now(),
        }
    }
}

impl Default for RealClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for RealClock {
    fn now(&self) -> MonoInstant {
        // `Instant` is monotonic and `origin` was read from it, so this never
        // goes backwards; `saturating_duration_since` guards the impossible.
        MonoInstant::from_origin_offset(std_time::mono_now().saturating_duration_since(self.origin))
    }

    fn sleep_until(&self, deadline: MonoInstant) {
        // The OS may wake us early (or we may be asked to sleep a huge span
        // and clamp); loop until the monotonic deadline is genuinely reached.
        loop {
            let remaining = deadline.saturating_duration_since(self.now());
            if remaining.is_zero() {
                return;
            }
            std_time::sleep_for(remaining);
        }
    }

    fn wall_now(&self) -> WallTime {
        WallTime::from_unix_offset(std_time::wall_since_unix_epoch())
    }
}

/// THE single place in the workspace that touches OS time.
///
/// clippy.toml lists `std::time::Instant::now`, `std::time::SystemTime::now`,
/// `std::thread::sleep` (and the implicit-clock `elapsed` methods) under
/// `disallowed-methods` (US-RR-3). Each function here carries a narrowly
/// scoped `#[expect(clippy::disallowed_methods, reason = "...")]`; nothing
/// else in the codebase may call these APIs.
mod std_time {
    use core::time::Duration;

    /// Current reading of the OS monotonic clock.
    #[expect(
        clippy::disallowed_methods,
        reason = "RealClock is the sanctioned std-time caller (D-010)"
    )]
    pub(super) fn mono_now() -> std::time::Instant {
        std::time::Instant::now()
    }

    /// Blocks the current thread for at least `d`.
    #[expect(
        clippy::disallowed_methods,
        reason = "RealClock is the sanctioned std-time caller (D-010)"
    )]
    pub(super) fn sleep_for(d: Duration) {
        std::thread::sleep(d);
    }

    /// Current OS wall-clock time as an offset from the Unix epoch.
    ///
    /// A system clock set before 1970 is reported as the epoch itself rather
    /// than failing: wall time is for display only.
    #[expect(
        clippy::disallowed_methods,
        reason = "RealClock is the sanctioned std-time caller (D-010)"
    )]
    pub(super) fn wall_since_unix_epoch() -> Duration {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn mono_instant_add_sub_duration() {
        let t = MonoInstant::from_origin_offset(ms(100));
        assert_eq!((t + ms(55)).origin_offset(), ms(155));
        assert_eq!((t - ms(40)).origin_offset(), ms(60));
        let mut u = t;
        u += ms(5000);
        assert_eq!(u.origin_offset(), ms(5100));
        u -= ms(100);
        assert_eq!(u.origin_offset(), ms(5000));
    }

    #[test]
    fn mono_instant_difference() {
        let a = MonoInstant::from_origin_offset(ms(10));
        let b = MonoInstant::from_origin_offset(ms(65));
        assert_eq!(b - a, ms(55));
        assert_eq!(b.saturating_duration_since(a), ms(55));
        assert_eq!(b.checked_duration_since(a), Some(ms(55)));
        // Earlier minus later saturates to zero / reports None.
        assert_eq!(a - b, Duration::ZERO);
        assert_eq!(a.checked_duration_since(b), None);
    }

    #[test]
    fn mono_instant_saturates_and_checks_bounds() {
        assert_eq!(MonoInstant::MAX + ms(1), MonoInstant::MAX);
        assert_eq!(MonoInstant::MAX.checked_add(ms(1)), None);
        assert_eq!(MonoInstant::ORIGIN - ms(1), MonoInstant::ORIGIN);
        assert_eq!(MonoInstant::ORIGIN.checked_sub(ms(1)), None);
        assert_eq!(
            MonoInstant::ORIGIN.checked_add(ms(7)),
            Some(MonoInstant::from_origin_offset(ms(7)))
        );
    }

    #[test]
    fn mono_instant_ordering_round_trip() {
        let t = MonoInstant::from_origin_offset(ms(1));
        let later = t + ms(1);
        assert!(later > t);
        assert_eq!(later - ms(1), t);
        assert_eq!(MonoInstant::default(), MonoInstant::ORIGIN);
    }

    #[test]
    fn real_clock_is_monotonic_non_decreasing() {
        let clock = RealClock::new();
        let mut prev = clock.now();
        for _ in 0..10_000 {
            let next = clock.now();
            assert!(next >= prev, "{next:?} < {prev:?}");
            prev = next;
        }
    }

    /// Proves US-RR-18-3 (part a), EC-26: `RealClock::now()` never decreases
    /// across a span of real time that includes OS sleeps (where a wall-clock
    /// step is most likely to land on a real machine), while `wall_now()` is
    /// reported separately as a different type (`WallTime`) and deliberately
    /// NOT asserted non-decreasing: it reads the system clock, which NTP/DST/
    /// the user may step backwards at any moment.
    ///
    /// This test does not change the OS clock: setting `CLOCK_REALTIME` needs
    /// `CAP_SYS_TIME` (root) and would disturb every other process, and Linux
    /// time namespaces virtualise only `CLOCK_MONOTONIC`/`CLOCK_BOOTTIME`.
    /// The literal wall-clock step is `tests/realclock_wall_step.rs`, run
    /// under libfaketime by the CI `faketime` job. The criterion is proven
    /// by that test together with three parts here:
    ///
    /// * (a) this test: `RealClock`'s monotonic reading is non-decreasing over
    ///   many samples and sleeps, with wall time sampled separately (by
    ///   construction `now()` is read only from `std::time::Instant`, the OS
    ///   monotonic clock, and `wall_now()` only from `SystemTime`);
    /// * (b) `virtual_clock::tests::wall_jumps_during_a_run_never_move_monotonic_time`:
    ///   wall time is jumped backwards and forwards during a multi-thread run
    ///   and the monotonic timeline is provably unaffected;
    /// * (c) the `compile_fail` examples on [`WallTime`]: wall time cannot be
    ///   used to time anything, so no code can depend on it monotonically.
    #[test]
    fn real_clock_now_is_non_decreasing_across_a_span_with_wall_reported_separately() {
        let clock = RealClock::new();
        let start = clock.now();
        let mut prev = start;
        let mut wall_samples = Vec::new();
        for round in 0..20 {
            for _ in 0..500 {
                let next = clock.now();
                assert!(next >= prev, "round {round}: {next:?} < {prev:?}");
                prev = next;
            }
            wall_samples.push(clock.wall_now());
            clock.sleep(ms(1));
            let after_sleep = clock.now();
            assert!(after_sleep >= prev + ms(1), "sleep(1ms) moved mono by less");
            prev = after_sleep;
        }
        // The span was real: at least the 20 x 1 ms of sleeps elapsed.
        assert!(prev - start >= ms(20));
        // Wall time was reported separately; it is only a label (any order).
        assert_eq!(wall_samples.len(), 20);
    }

    #[test]
    fn real_clock_sleep_until_past_deadline_returns_immediately() {
        let clock = RealClock::new();
        // Let some time pass so there is a genuinely past instant to target.
        clock.sleep(ms(2));
        let past = clock.now() - ms(1);
        let before = clock.now();
        clock.sleep_until(past);
        clock.sleep_until(MonoInstant::ORIGIN);
        clock.sleep_until(before);
        // Generous bound: "immediately" must not mean "slept".
        assert!(clock.now() - before < ms(50));
    }

    #[test]
    fn real_clock_sleep_until_reaches_future_deadline() {
        let clock = RealClock::new();
        let deadline = clock.deadline_after(ms(5));
        clock.sleep_until(deadline);
        assert!(clock.now() >= deadline);
    }

    #[test]
    fn real_clock_clones_share_a_timeline() {
        let a = RealClock::new();
        let b = a.clone();
        let t_a = a.now();
        let t_b = b.now();
        assert!(t_b >= t_a);
    }

    #[test]
    fn real_clock_wall_time_is_plausible_and_separate() {
        let clock = RealClock::new();
        // 2020-01-01T00:00:00Z; any real system clock is past this.
        let y2020 = WallTime::from_unix_offset(Duration::from_secs(1_577_836_800));
        assert!(clock.wall_now() > y2020);
        // The monotonic origin is the clock's creation, not the Unix epoch.
        assert!(clock.now().origin_offset() < Duration::from_secs(3600));
    }

    #[test]
    fn clock_is_usable_as_shared_trait_object_across_threads() {
        let clock: Arc<dyn Clock> = Arc::new(RealClock::new());
        let start = clock.now();
        let worker = {
            let clock = Arc::clone(&clock);
            std::thread::spawn(move || clock.now())
        };
        let seen = worker.join().unwrap();
        assert!(seen >= start);
        assert!(clock.now() >= seen);
    }

    #[test]
    fn blanket_impls_forward() {
        fn read(clock: &impl Clock) -> MonoInstant {
            clock.now()
        }
        let real = RealClock::new();
        let boxed: Box<dyn Clock> = Box::new(real.clone());
        let arced: Arc<dyn Clock> = Arc::new(real.clone());
        let t0 = read(&real);
        let t1 = read(&boxed);
        let t2 = read(&arced);
        let t3 = read(&&real);
        assert!(t0 <= t1 && t1 <= t2 && t2 <= t3);
    }

    #[test]
    fn clock_trait_object_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn Clock>();
        assert_send_sync::<RealClock>();
        assert_send_sync::<MonoInstant>();
        assert_send_sync::<WallTime>();
    }
}
