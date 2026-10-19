//! Strategies for clock types (concept C9, `rr-core::clock`).
//!
//! Durations are bounded to comms-relevant scales so arithmetic on generated
//! instants never saturates by accident; use [`MonoInstant::MAX`] explicitly
//! when a test needs the "never" deadline.

use std::time::Duration;

use proptest::prelude::*;
use rr_core::clock::MonoInstant;

/// Upper bound for generated offsets and durations: one day.
pub const MAX_GENERATED: Duration = Duration::from_secs(24 * 60 * 60);

/// A [`Duration`] in `0..=MAX_GENERATED`, at nanosecond resolution.
pub fn duration() -> impl Strategy<Value = Duration> {
    let max_nanos = u64::try_from(MAX_GENERATED.as_nanos()).unwrap_or(u64::MAX);
    (0..=max_nanos).prop_map(Duration::from_nanos)
}

/// A [`Duration`] in whole milliseconds, `0..=max_ms` (comms timeouts, gaps).
pub fn duration_ms(max_ms: u64) -> impl Strategy<Value = Duration> {
    (0..=max_ms).prop_map(Duration::from_millis)
}

/// A [`MonoInstant`] at most [`MAX_GENERATED`] after the clock's origin.
pub fn mono_instant() -> impl Strategy<Value = MonoInstant> {
    duration().prop_map(MonoInstant::from_origin_offset)
}
