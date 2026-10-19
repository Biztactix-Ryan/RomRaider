//! US-RR-18-3 (EC-26): `RealClock`'s monotonic time never goes backwards, and
//! never follows the wall clock, when the OS wall clock is stepped mid-run.
//!
//! Stepping the real `CLOCK_REALTIME` needs root and disturbs every process on
//! the machine, so the step is made with libfaketime instead: it is preloaded
//! into the test process with `FAKETIME_DONT_FAKE_MONOTONIC=1` (so
//! `CLOCK_MONOTONIC` stays real, exactly as on an OS clock step) and
//! `FAKETIME_NO_CACHE=1` + `FAKETIME_TIMESTAMP_FILE=<file>` (so rewriting the
//! file steps the wall clock immediately). `RR_FAKETIME_FILE` names the same
//! file so the test can step the clock itself.
//!
//! The test is `#[ignore]`d because it needs that environment; the CI
//! `faketime` job runs it. To run it locally (Debian/Ubuntu `libfaketime`):
//!
//! ```sh
//! f=$(mktemp) && echo +0 > "$f"
//! cargo nextest run -p rr-core --test realclock_wall_step --no-run
//! LD_PRELOAD=/usr/lib/x86_64-linux-gnu/faketime/libfaketime.so.1 \
//!   FAKETIME_DONT_FAKE_MONOTONIC=1 FAKETIME_NO_CACHE=1 \
//!   FAKETIME_TIMESTAMP_FILE="$f" RR_FAKETIME_FILE="$f" \
//!   cargo nextest run -p rr-core --test realclock_wall_step --run-ignored only
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rr_core::clock::{Clock, MonoInstant, RealClock, WallTime};

const HOUR: Duration = Duration::from_secs(3600);
/// Far more than the test takes, far less than any wall step it makes.
const MONO_BOUND: Duration = Duration::from_secs(60);

/// Sets libfaketime's wall-clock offset (`+0`, `-1d`, `+1d`, ...). Restores
/// `+0` on drop so a failing assertion leaves the clock unstepped.
struct WallOffset(PathBuf);

impl WallOffset {
    fn set(&self, offset: &str) {
        let written = fs::write(&self.0, format!("{offset}\n"));
        assert!(written.is_ok(), "writing {}: {written:?}", self.0.display());
    }
}

impl Drop for WallOffset {
    fn drop(&mut self) {
        let _ = fs::write(&self.0, "+0\n");
    }
}

/// Samples `now()` `n` times, asserting it never decreases; returns the last.
fn spin(clock: &RealClock, mut prev: MonoInstant, n: usize, phase: &str) -> MonoInstant {
    for _ in 0..n {
        let next = clock.now();
        assert!(
            next >= prev,
            "{phase}: mono went backwards: {next:?} < {prev:?}"
        );
        prev = next;
    }
    prev
}

/// Sleeps 1 ms through `RealClock` and asserts mono advanced by at least that.
fn sleep_1ms(clock: &RealClock, prev: MonoInstant, phase: &str) -> MonoInstant {
    clock.sleep(Duration::from_millis(1));
    let after = clock.now();
    assert!(
        after >= prev + Duration::from_millis(1),
        "{phase}: sleep(1ms) advanced mono by less: {prev:?} -> {after:?}"
    );
    after
}

fn wall_secs(w: WallTime) -> Duration {
    w.unix_offset()
}

#[test]
#[ignore = "steps the OS wall clock via libfaketime; run by the CI `faketime` job"]
fn real_clock_mono_never_follows_os_wall_clock_steps() {
    let file = std::env::var_os("RR_FAKETIME_FILE")
        .expect("RR_FAKETIME_FILE is unset: run under libfaketime as the CI `faketime` job does");
    let wall = WallOffset(Path::new(&file).to_path_buf());
    wall.set("+0");

    let clock = RealClock::new();
    let start = clock.now();
    let mut prev = spin(&clock, start, 2_000, "before steps");
    prev = sleep_1ms(&clock, prev, "before steps");
    let w0 = clock.wall_now();

    // Step the wall clock back a day.
    let before_back = prev;
    wall.set("-1d");
    prev = spin(&clock, prev, 2_000, "after backward step");
    prev = sleep_1ms(&clock, prev, "after backward step");
    let w1 = clock.wall_now();
    assert!(
        wall_secs(w0).saturating_sub(wall_secs(w1)) >= 23 * HOUR,
        "the wall clock did not step back (w0 {w0:?}, w1 {w1:?}): libfaketime is not \
         preloaded, or FAKETIME_NO_CACHE / FAKETIME_TIMESTAMP_FILE are not set"
    );
    assert!(
        prev - before_back < MONO_BOUND,
        "mono followed the backward wall step: {before_back:?} -> {prev:?}"
    );

    // Step it forward two days (from -1d to +1d).
    let before_fwd = prev;
    wall.set("+1d");
    prev = spin(&clock, prev, 2_000, "after forward step");
    prev = sleep_1ms(&clock, prev, "after forward step");
    let w2 = clock.wall_now();
    assert!(
        wall_secs(w2).saturating_sub(wall_secs(w1)) >= 47 * HOUR,
        "the wall clock did not step forward (w1 {w1:?}, w2 {w2:?})"
    );
    assert!(
        prev - before_fwd < MONO_BOUND,
        "mono followed the forward wall step: {before_fwd:?} -> {prev:?}"
    );

    // Back to real wall time; mono still only moved by real elapsed time.
    wall.set("+0");
    prev = spin(&clock, prev, 2_000, "after restoring");
    assert!(
        prev - start < MONO_BOUND,
        "mono drifted: {start:?} -> {prev:?}"
    );
}
