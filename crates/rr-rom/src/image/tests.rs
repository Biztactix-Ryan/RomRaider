use core::ops::Range;

use proptest::prelude::*;

use super::{ApplyError, Image, ImageHash};

/// SHA-256 of the empty input and of `abc` (FIPS 180-2 appendix B.1).
const SHA256_EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const SHA256_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

#[test]
fn new_snapshots_the_bytes_and_their_hash() {
    let image = Image::new(b"abc".to_vec());
    assert_eq!(image.bytes(), b"abc");
    assert_eq!(image.original(), b"abc");
    assert_eq!(image.len(), 3);
    assert!(!image.is_empty());
    assert!(!image.is_modified());
    assert_eq!(image.original_hash().to_string(), SHA256_ABC);
    assert_eq!(image.current_hash(), image.original_hash());
    assert_eq!(Image::from(b"abc".to_vec()), image);
}

#[test]
fn empty_image() {
    let image = Image::new(Vec::new());
    assert!(image.is_empty());
    assert_eq!(image.len(), 0);
    assert_eq!(image.original_hash().to_string(), SHA256_EMPTY);
}

#[test]
fn hash_round_trips_through_its_bytes_and_formats_as_hex() {
    let h = ImageHash::of(b"abc");
    assert_eq!(ImageHash::from_bytes(*h.as_bytes()), h);
    assert_eq!(h.as_bytes()[0], 0xba);
    assert_eq!(h.as_bytes()[31], 0xad);
    assert_eq!(format!("{h:?}"), format!("ImageHash({SHA256_ABC})"));
    assert_ne!(ImageHash::of(b"abd"), h);
}

#[test]
fn apply_changes_current_bytes_but_not_the_original() {
    let mut image = Image::new(vec![0x10, 0x20, 0x30, 0x40]);
    let hash = image.original_hash();
    assert_eq!(image.apply_patch(1, &[0x20, 0x30], &[0xAA, 0xBB]), Ok(()));
    assert_eq!(image.bytes(), &[0x10, 0xAA, 0xBB, 0x40]);
    assert_eq!(image.original(), &[0x10, 0x20, 0x30, 0x40]);
    assert!(image.is_modified());
    assert_eq!(image.original_hash(), hash);
    assert_eq!(
        image.current_hash(),
        ImageHash::of(&[0x10, 0xAA, 0xBB, 0x40])
    );
    assert_ne!(image.current_hash(), hash);

    // Undo is the same patch backwards.
    assert_eq!(image.apply_patch(1, &[0xAA, 0xBB], &[0x20, 0x30]), Ok(()));
    assert_eq!(image.bytes(), image.original());
    assert!(!image.is_modified());
}

#[test]
fn patch_at_the_last_byte_and_empty_patch_at_the_end_apply() {
    let mut image = Image::new(vec![1, 2, 3]);
    assert_eq!(image.apply_patch(2, &[3], &[9]), Ok(()));
    assert_eq!(image.bytes(), &[1, 2, 9]);
    assert_eq!(image.apply_patch(3, &[], &[]), Ok(()));
    assert_eq!(image.bytes(), &[1, 2, 9]);
}

#[test]
fn length_mismatch_is_refused() {
    let mut image = Image::new(vec![1, 2, 3]);
    assert_eq!(
        image.apply_patch(0, &[1], &[7, 8]),
        Err(ApplyError::LengthMismatch { old: 1, new: 2 })
    );
    assert_eq!(image.bytes(), &[1, 2, 3]);
}

#[test]
fn out_of_bounds_is_refused() {
    let mut image = Image::new(vec![1, 2, 3]);
    let err = |at, len| ApplyError::OutOfBounds {
        at,
        len,
        image_len: 3,
    };
    assert_eq!(image.apply_patch(2, &[3, 0], &[0, 0]), Err(err(2, 2)));
    assert_eq!(image.apply_patch(3, &[0], &[0]), Err(err(3, 1)));
    assert_eq!(image.apply_patch(4, &[], &[]), Err(err(4, 0)));
    assert_eq!(
        image.apply_patch(usize::MAX, &[0], &[0]),
        Err(err(usize::MAX, 1))
    );
    assert_eq!(image.bytes(), &[1, 2, 3]);
}

#[test]
fn stale_patch_is_refused_and_reports_the_first_differing_byte() {
    let mut image = Image::new(vec![1, 2, 3, 4]);
    assert_eq!(
        image.apply_patch(1, &[2, 0, 0], &[9, 9, 9]),
        Err(ApplyError::Stale {
            offset: 2,
            expected: 0,
            found: 3
        })
    );
    assert_eq!(image.bytes(), &[1, 2, 3, 4]);

    // A patch that matched once is stale after a change to its bytes.
    assert_eq!(image.apply_patch(0, &[1, 2], &[5, 6]), Ok(()));
    assert_eq!(
        image.apply_patch(1, &[2], &[7]),
        Err(ApplyError::Stale {
            offset: 1,
            expected: 2,
            found: 6
        })
    );
    assert_eq!(image.bytes(), &[5, 6, 3, 4]);
}

#[test]
fn errors_display() {
    assert_eq!(
        ApplyError::LengthMismatch { old: 1, new: 2 }.to_string(),
        "patch replaces 1 bytes with 2; a patch may not resize the image"
    );
    assert_eq!(
        ApplyError::OutOfBounds {
            at: 0x10,
            len: 2,
            image_len: 16
        }
        .to_string(),
        "patch of 2 bytes at 0x10 reaches past the end of the 16-byte image"
    );
    assert_eq!(
        ApplyError::Stale {
            offset: 0x2000,
            expected: 0x0A,
            found: 0xFF
        }
        .to_string(),
        "stale patch: expected byte 0x0a at 0x2000, the image holds 0xff"
    );
}

#[test]
fn debug_omits_the_bytes() {
    let image = Image::new(b"abc".to_vec());
    assert_eq!(
        format!("{image:?}"),
        format!("Image {{ len: 3, original_hash: ImageHash({SHA256_ABC}), modified: false }}")
    );
}

/// An image of `original` patched, byte by byte, to `current`.
fn modified(original: &[u8], current: &[u8]) -> Image {
    let mut image = Image::new(original.to_vec());
    for (at, (&was, &now)) in original.iter().zip(current).enumerate() {
        assert_eq!(image.apply_patch(at, &[was], &[now]), Ok(()));
    }
    assert_eq!(image.bytes(), current);
    image
}

#[test]
fn an_unmodified_image_has_no_modified_bytes() {
    let image = Image::new(vec![1, 2, 3]);
    assert!(image.modified_ranges().is_empty());
    assert!(image.modified_ranges_in(0..3).is_empty());
    assert!(!image.is_range_modified(0..3));
    assert!((0..4).all(|i| !image.is_byte_modified(i)));

    let empty = Image::new(Vec::new());
    assert!(empty.modified_ranges().is_empty());
    assert!(!empty.is_range_modified(0..usize::MAX));
    assert!(!empty.is_byte_modified(0));
}

#[test]
fn modified_ranges_coalesce_neighbouring_bytes() {
    //               0  1  2  3  4  5  6  7  8  9
    let original = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let current = [9, 0, 7, 7, 7, 0, 5, 0, 0, 3];
    let image = modified(&original, &current);
    assert!(image.is_modified());
    assert_eq!(image.modified_ranges(), vec![0..1, 2..5, 6..7, 9..10]);

    let flags: Vec<bool> = (0..11).map(|i| image.is_byte_modified(i)).collect();
    let expected = [
        true, false, true, true, true, false, true, false, false, true, false,
    ];
    assert_eq!(flags, expected);
}

#[test]
fn a_byte_changed_back_is_not_modified() {
    let mut image = Image::new(vec![1, 2, 3]);
    assert_eq!(image.apply_patch(0, &[1, 2, 3], &[4, 5, 6]), Ok(()));
    assert_eq!(image.modified_ranges(), vec![0..3]);
    assert_eq!(image.apply_patch(1, &[5], &[2]), Ok(()));
    assert_eq!(image.modified_ranges(), vec![0..1, 2..3]);
    assert!(!image.is_byte_modified(1));
    assert!(!image.is_range_modified(1..2));
    assert!(image.is_range_modified(1..3));
}

#[test]
fn modified_ranges_in_clips_to_the_window_and_the_image() {
    //               0  1  2  3  4  5  6  7
    let original = [0, 0, 0, 0, 0, 0, 0, 0];
    let current = [1, 1, 0, 0, 1, 1, 1, 0];
    let image = modified(&original, &current);
    assert_eq!(image.modified_ranges_in(0..8), vec![0..2, 4..7]);
    assert_eq!(image.modified_ranges_in(1..5), vec![1..2, 4..5]);
    assert_eq!(image.modified_ranges_in(5..6), vec![5..6]);
    assert_eq!(image.modified_ranges_in(2..4), Vec::<Range<usize>>::new());
    assert_eq!(image.modified_ranges_in(5..usize::MAX), vec![5..7]);
    assert!(image.modified_ranges_in(8..20).is_empty());
    assert!(image.modified_ranges_in(5..5).is_empty());
    #[allow(clippy::reversed_empty_ranges)]
    let reversed = 6..1;
    assert!(image.modified_ranges_in(reversed.clone()).is_empty());
    assert!(!image.is_range_modified(reversed));

    assert!(image.is_range_modified(1..2));
    assert!(image.is_range_modified(3..5));
    assert!(image.is_range_modified(6..usize::MAX));
    assert!(!image.is_range_modified(2..4));
    assert!(!image.is_range_modified(7..usize::MAX));
    assert!(!image.is_range_modified(4..4));
}

/// An image and a patch range within it.
fn image_and_range() -> impl Strategy<Value = (Vec<u8>, usize, usize)> {
    prop::collection::vec(any::<u8>(), 1..64).prop_flat_map(|bytes| {
        let n = bytes.len();
        (Just(bytes), 0..=n).prop_flat_map(move |(bytes, at)| (Just(bytes), Just(at), 0..=n - at))
    })
}

proptest! {
    /// Applying a matching patch and then its reverse restores the original
    /// bytes exactly; the snapshot and its hash never change.
    #[test]
    fn patch_then_reverse_restores_original(
        (bytes, at, len) in image_and_range(),
        fill in any::<u8>(),
    ) {
        let mut image = Image::new(bytes.clone());
        let old = bytes[at..at + len].to_vec();
        let new: Vec<u8> = old.iter().map(|b| b ^ fill).collect();
        prop_assert_eq!(image.apply_patch(at, &old, &new), Ok(()));
        prop_assert_eq!(&image.bytes()[at..at + len], &new[..]);
        prop_assert_eq!(&image.bytes()[..at], &bytes[..at]);
        prop_assert_eq!(&image.bytes()[at + len..], &bytes[at + len..]);
        prop_assert_eq!(image.is_modified(), old != new);
        prop_assert_eq!(image.apply_patch(at, &new, &old), Ok(()));
        prop_assert_eq!(image.bytes(), &bytes[..]);
        prop_assert_eq!(image.original(), &bytes[..]);
        prop_assert_eq!(image.original_hash(), ImageHash::of(&bytes));
    }

    /// A patch whose old bytes differ anywhere is refused and changes nothing.
    #[test]
    fn stale_patch_changes_nothing(
        (bytes, at, len) in image_and_range(),
        flip in any::<prop::sample::Index>(),
        mask in 1..=u8::MAX,
    ) {
        prop_assume!(len > 0);
        let mut image = Image::new(bytes.clone());
        let mut old = bytes[at..at + len].to_vec();
        let i = flip.index(len);
        old[i] ^= mask;
        let new = vec![0; len];
        let refused = image.apply_patch(at, &old, &new);
        prop_assert!(
            matches!(refused, Err(ApplyError::Stale { offset, .. }) if offset == at + i),
            "{:?}", refused
        );
        prop_assert_eq!(image.bytes(), &bytes[..]);
    }
}

/// Two equal-length byte strings that differ in a few runs of bytes.
fn original_and_current() -> impl Strategy<Value = (Vec<u8>, Vec<u8>)> {
    prop::collection::vec((any::<u8>(), any::<bool>(), 1..=u8::MAX), 0..48).prop_map(|bytes| {
        let original = bytes.iter().map(|&(b, _, _)| b).collect();
        let current = bytes
            .iter()
            .map(|&(b, change, mask)| if change { b ^ mask } else { b })
            .collect();
        (original, current)
    })
}

proptest! {
    /// The modified ranges are sorted, separated by at least one unmodified
    /// byte, and cover exactly the bytes that differ from the original; the
    /// per-byte and per-range queries agree with them.
    #[test]
    fn modified_ranges_cover_exactly_the_differing_bytes(
        (original, current) in original_and_current(),
        a in 0..64usize,
        b in 0..64usize,
    ) {
        let image = modified(&original, &current);
        let ranges = image.modified_ranges();
        for pair in ranges.windows(2) {
            prop_assert!(pair[0].end < pair[1].start, "{:?}", ranges);
        }
        prop_assert!(ranges.iter().all(|r| r.start < r.end));
        let differs: Vec<bool> = original.iter().zip(&current).map(|(x, y)| x != y).collect();
        let covered: Vec<bool> = (0..original.len())
            .map(|i| ranges.iter().any(|r| r.contains(&i)))
            .collect();
        prop_assert_eq!(&covered, &differs);
        for (i, &d) in differs.iter().enumerate() {
            prop_assert_eq!(image.is_byte_modified(i), d);
        }
        prop_assert!(!image.is_byte_modified(original.len()));
        prop_assert_eq!(image.is_modified(), !ranges.is_empty());

        let window = a.min(b)..a.max(b);
        let clipped: Vec<Range<usize>> = ranges
            .iter()
            .map(|r| r.start.max(window.start)..r.end.min(window.end))
            .filter(|r| r.start < r.end)
            .collect();
        prop_assert_eq!(image.modified_ranges_in(window.clone()), clipped.clone());
        prop_assert_eq!(image.is_range_modified(window), !clipped.is_empty());
    }
}
