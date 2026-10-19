//! Strategies for capability scope types (`rr-core::capability`).

use proptest::prelude::*;
use rr_core::capability::AddressRange;

/// Any valid [`AddressRange`]: `start < end`, anywhere in the 32-bit space.
pub fn address_range() -> impl Strategy<Value = AddressRange> {
    (0..u32::MAX)
        .prop_flat_map(|start| (Just(start), (start + 1)..=u32::MAX))
        .prop_filter_map("start < end is guaranteed", |(start, end)| {
            AddressRange::new(start, end).ok()
        })
}

/// A valid [`AddressRange`] lying entirely below `limit` (e.g. a ROM size).
///
/// `limit` must be at least 1; with `limit == 1` the only value is `0..1`.
pub fn address_range_within(limit: u32) -> impl Strategy<Value = AddressRange> {
    let limit = limit.max(1);
    (0..limit)
        .prop_flat_map(move |start| (Just(start), (start + 1)..=limit))
        .prop_filter_map("start < end is guaranteed", |(start, end)| {
            AddressRange::new(start, end).ok()
        })
}
