//! Property tests exercising the sample strategies.

use proptest::prelude::*;
use rr_core::clock::MonoInstant;
use rr_testkit::strategies::capability::{address_range, address_range_within};
use rr_testkit::strategies::clock::{MAX_GENERATED, duration, duration_ms, mono_instant};

proptest! {
    #[test]
    fn address_range_is_non_empty_and_contains_its_start(range in address_range()) {
        prop_assert!(range.start() < range.end());
        prop_assert_eq!(range.len(), range.end() - range.start());
        prop_assert!(range.contains(range.start()));
        prop_assert!(!range.contains(range.end()));
    }

    #[test]
    fn address_range_within_respects_limit(
        (limit, range) in (1u32..=0x0010_0000).prop_flat_map(|l| (Just(l), address_range_within(l)))
    ) {
        prop_assert!(range.end() <= limit);
        prop_assert!(!range.is_empty());
    }

    #[test]
    fn mono_instant_round_trips_its_origin_offset(t in mono_instant(), d in duration()) {
        prop_assert!(t.origin_offset() <= MAX_GENERATED);
        prop_assert_eq!(MonoInstant::from_origin_offset(t.origin_offset()), t);
        let later = t.checked_add(d);
        prop_assert!(later.is_some());
        if let Some(later) = later {
            prop_assert_eq!(later.saturating_duration_since(t), d);
            prop_assert!(later >= t);
        }
    }

    #[test]
    fn duration_ms_is_whole_milliseconds(d in duration_ms(10_000)) {
        prop_assert!(d.as_millis() <= 10_000);
        prop_assert_eq!(d.subsec_nanos() % 1_000_000, 0);
    }
}
