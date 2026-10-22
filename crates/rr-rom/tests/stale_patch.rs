//! US-RR-31-2: a patch whose old bytes do not match the current image is
//! rejected (stale-patch protection).
//!
//! Exercised only through rr-rom's public API, as a downstream crate (the
//! editor, a diff tool, a sidecar replay) would use it. A patch records the
//! bytes it expects to replace; if the image no longer holds exactly those
//! bytes when the patch is applied, the journal refuses it with
//! [`ApplyError::Stale`] naming the first differing offset and the byte
//! expected and found there, and nothing changes: not the bytes, not the
//! current hash, not the applied history, not the redo stack. A stale patch
//! anywhere in a group refuses the whole group.
//!
//! The crate's unit tests (`src/journal/tests.rs`) cover the same ground
//! from the inside, including undo and redo over an image changed behind
//! the journal's back, which no code outside the crate can arrange.

// In a `cfg(test)` module so the workspace's unwrap/expect/panic-in-tests
// allowance (clippy.toml) covers the helpers too.
#[cfg(test)]
mod stale_patch {
    use proptest::prelude::*;
    use rr_rom::image::{ApplyError, Image, ImageHash};
    use rr_rom::journal::{Group, Journal, JournalError, Origin, Patch};

    fn journal(bytes: &[u8]) -> Journal {
        Journal::new(Image::new(bytes.to_vec()))
    }

    fn patch(at: usize, old: &[u8], new: &[u8]) -> Patch {
        Patch::new(at, old, new, Origin::Edit).unwrap()
    }

    fn stale(index: usize, offset: usize, expected: u8, found: u8) -> JournalError {
        JournalError::Refused {
            index,
            error: ApplyError::Stale {
                offset,
                expected,
                found,
            },
        }
    }

    /// Everything a refusal must leave as it was, read through the public
    /// API: the bytes, their hash, whether anything is modified, the
    /// applied groups and the undone (redoable) groups.
    #[derive(Debug, PartialEq)]
    struct Observed {
        bytes: Vec<u8>,
        hash: ImageHash,
        original_hash: ImageHash,
        modified: Vec<core::ops::Range<usize>>,
        applied: Vec<Group>,
        undone: Vec<Group>,
        can_undo: bool,
        can_redo: bool,
    }

    fn observe(j: &Journal) -> Observed {
        Observed {
            bytes: j.image().bytes().to_vec(),
            hash: j.image().current_hash(),
            original_hash: j.image().original_hash(),
            modified: j.image().modified_ranges(),
            applied: j.applied().to_vec(),
            undone: j.undone().to_vec(),
            can_undo: j.can_undo(),
            can_redo: j.can_redo(),
        }
    }

    /// A journal with one applied edit and one undone (redoable) edit, so a
    /// refusal has a history and a redo stack to disturb.
    fn journal_with_history() -> Journal {
        let mut j = journal(&[0x10, 0x20, 0x30, 0x40]);
        j.apply(Patch::replacing(j.image(), 1, vec![0x21], Origin::Edit).unwrap())
            .unwrap();
        j.apply(Patch::replacing(j.image(), 3, vec![0x41], Origin::Paste).unwrap())
            .unwrap();
        assert!(j.undo().unwrap().is_some());
        j
    }

    #[test]
    fn a_patch_built_before_the_image_changed_is_rejected() {
        let mut j = journal(&[0x00, 0x11, 0x22]);
        // Built against the image as it is now...
        let late = Patch::replacing(j.image(), 0, vec![0xEE, 0xEE], Origin::Edit).unwrap();
        assert_eq!(late.old_bytes(), &[0x00, 0x11]);
        // ...but another edit changes its second byte first.
        j.apply(Patch::replacing(j.image(), 1, vec![0xAA], Origin::Edit).unwrap())
            .unwrap();
        let before = observe(&j);

        let refused = j.apply(late);

        // The stale variant, naming the first differing offset and the byte
        // expected vs the byte found there.
        assert_eq!(refused, Err(stale(0, 1, 0x11, 0xAA)));
        assert!(refused.is_err_and(|e| e.is_stale()));
        assert_eq!(observe(&j), before);
        assert_eq!(j.image().bytes(), &[0x00, 0xAA, 0x22]);
    }

    #[test]
    fn the_error_names_offset_expected_and_found() {
        let mut j = journal(&[0; 0x30]);
        let err = j
            .apply(patch(0x20, &[0, 0x01, 0x02], &[9, 9, 9]))
            .unwrap_err();
        let JournalError::Refused { index, error } = err;
        assert_eq!(index, 0);
        assert_eq!(
            error,
            ApplyError::Stale {
                offset: 0x21,
                expected: 0x01,
                found: 0x00
            }
        );
        assert!(err.is_stale());
        assert_eq!(
            err.to_string(),
            "patch 0 refused: stale patch: expected byte 0x01 at 0x21, the image holds 0x00"
        );
        let source = core::error::Error::source(&err).map(ToString::to_string);
        assert_eq!(source, Some(error.to_string()));
    }

    #[test]
    fn old_bytes_are_checked_against_the_current_image_not_the_original() {
        let mut j = journal(&[1, 2]);
        j.apply(patch(0, &[1], &[5])).unwrap();
        let before = observe(&j);
        // Matches the original snapshot, not the image as it is now.
        assert_eq!(j.apply(patch(0, &[1], &[6])), Err(stale(0, 0, 1, 5)));
        assert_eq!(observe(&j), before);
        // Matches the image as it is now: applies.
        assert_eq!(j.apply(patch(0, &[5], &[6])), Ok(()));
        assert_eq!(j.image().bytes(), &[6, 2]);
    }

    #[test]
    fn a_rejection_changes_nothing_and_keeps_the_redo_stack() {
        let mut j = journal_with_history();
        let before = observe(&j);
        let snapshot = j.clone();
        assert!(before.can_undo && before.can_redo);

        assert_eq!(
            j.apply(patch(1, &[0x20], &[0x99])),
            Err(stale(0, 1, 0x20, 0x21))
        );

        assert_eq!(observe(&j), before);
        assert_eq!(j, snapshot);
        // The undone edit is still there to redo, and the applied one to
        // undo: the refusal did not truncate history as an applied group
        // would.
        let redone = j.redo().unwrap().map(|g| g.patches().to_vec());
        assert_eq!(
            redone,
            Some(vec![
                Patch::new(3, vec![0x40], vec![0x41], Origin::Paste).unwrap()
            ])
        );
        assert_eq!(j.image().bytes(), &[0x10, 0x21, 0x30, 0x41]);
        j.undo_all().unwrap();
        assert_eq!(j.image().bytes(), &[0x10, 0x20, 0x30, 0x40]);
        assert_eq!(j.image().current_hash(), j.image().original_hash());
    }

    #[test]
    fn a_group_with_one_stale_patch_is_rejected_entirely() {
        let mut j = journal_with_history();
        let before = observe(&j);

        // Two valid patches (the second rewriting bytes the first wrote),
        // then a stale one, then a valid one that is never reached.
        let refused = j.apply_group([
            patch(0, &[0x10, 0x21], &[0x01, 0x02]),
            patch(1, &[0x02], &[0x03]),
            patch(2, &[0x31], &[0x77]),
            patch(3, &[0x40], &[0x04]),
        ]);

        assert_eq!(refused, Err(stale(2, 2, 0x31, 0x30)));
        assert!(refused.is_err_and(|e| e.is_stale()));
        // All or nothing: the two patches that applied were rolled back.
        assert_eq!(observe(&j), before);
        assert_eq!(j.image().bytes(), &[0x10, 0x21, 0x30, 0x40]);
        assert!(!j.image().is_range_modified(0..1));
        assert!(!j.image().is_range_modified(2..4));
    }

    #[test]
    fn a_stale_first_or_last_patch_rejects_the_group() {
        for (stale_at, index) in [(0, 0), (2, 2)] {
            let mut j = journal(&[0; 4]);
            let before = observe(&j);
            let mut group = vec![
                patch(1, &[0], &[1]),
                patch(2, &[0], &[2]),
                patch(3, &[0], &[3]),
            ];
            group.insert(stale_at, patch(0, &[5], &[6]));
            assert_eq!(j.apply_group(group), Err(stale(index, 0, 5, 0)));
            assert_eq!(observe(&j), before);
            assert!(!j.image().is_modified());
        }
    }

    /// A write: `new` bytes at `offset`, fitted into an image of `len`
    /// bytes (offset modulo the room left, bytes cut to the image length).
    type Write = (usize, Vec<u8>);

    fn write() -> impl Strategy<Value = Write> {
        (
            prop_oneof![0..4usize, any::<usize>()],
            prop::collection::vec(any::<u8>(), 0..6),
        )
    }

    /// A valid patch for `w` against `model`, which it then writes into.
    fn valid(model: &mut [u8], (offset, new): &Write) -> Patch {
        let len = new.len().min(model.len());
        let at = offset % (model.len() - len + 1);
        let range = at..at + len;
        let p =
            Patch::replacing(&Image::new(model.to_vec()), at, &new[..len], Origin::Edit).unwrap();
        model[range].copy_from_slice(&new[..len]);
        p
    }

    /// How to spoil a patch's old bytes: a range seed, a sparse XOR mask
    /// over the range, and one byte (position seed, non-zero XOR) that is
    /// certainly wrong.
    type Spoil = (usize, usize, Vec<u8>, usize, u8, Vec<u8>);

    fn spoil() -> impl Strategy<Value = Spoil> {
        (
            any::<usize>(),
            any::<usize>(),
            prop::collection::vec(prop_oneof![3 => Just(0u8), 1 => any::<u8>()], 64),
            any::<usize>(),
            1..=u8::MAX,
            prop::collection::vec(any::<u8>(), 64),
        )
    }

    /// A patch over a non-empty range of `model` whose old bytes differ
    /// from `model` there in at least one byte, with the first such offset
    /// and the bytes expected and found at it.
    fn stale_against(model: &[u8], s: &Spoil) -> (Patch, usize, u8, u8) {
        let (at_seed, len_seed, mask, k_seed, flip, new) = s;
        let at = at_seed % model.len();
        let len = 1 + len_seed % (model.len() - at).min(mask.len());
        let current = &model[at..at + len];
        let mut old: Vec<u8> = current.iter().zip(mask).map(|(b, m)| b ^ m).collect();
        let k = k_seed % len;
        old[k] = current[k] ^ flip;
        let first = (0..len).find(|&i| old[i] != current[i]).unwrap();
        let p = Patch::new(at, old.clone(), &new[..len], Origin::Paste).unwrap();
        (p, at + first, old[first], current[first])
    }

    /// A journal over `original` with `history` applied one group each and
    /// the last `undos` of them undone (so the redo stack may be non-empty).
    fn journal_after(original: &[u8], history: &[Write], undos: usize) -> Journal {
        let mut j = journal(original);
        for w in history {
            let mut model = j.image().bytes().to_vec();
            j.apply(valid(&mut model, w)).unwrap();
        }
        for _ in 0..undos {
            j.undo().unwrap();
        }
        j
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1024))]

        /// For any image, any history (some of it undone), and any patch
        /// whose old bytes differ from the current bytes in its range in at
        /// least one byte: apply is rejected as stale, naming the first
        /// differing offset and the bytes expected and found there, and the
        /// journal is unchanged.
        #[test]
        fn any_patch_with_wrong_old_bytes_is_rejected_and_changes_nothing(
            original in prop::collection::vec(any::<u8>(), 1..64),
            history in prop::collection::vec(write(), 0..4),
            undos in 0..3usize,
            s in spoil(),
        ) {
            let mut j = journal_after(&original, &history, undos);
            let before = observe(&j);
            let snapshot = j.clone();
            let (p, offset, expected, found) = stale_against(j.image().bytes(), &s);
            prop_assert_ne!(expected, found);

            let refused = j.apply(p);

            prop_assert_eq!(refused, Err(stale(0, offset, expected, found)));
            prop_assert!(refused.is_err_and(|e| e.is_stale()));
            prop_assert_eq!(observe(&j), before);
            prop_assert_eq!(&j, &snapshot);
            prop_assert_eq!(j.image().current_hash(), ImageHash::of(j.image().bytes()));
        }

        /// The same for a group: valid patches, then one stale against the
        /// image as the patches before it left it, then more valid patches.
        /// The whole group is rejected at the stale patch and the journal is
        /// unchanged.
        #[test]
        fn a_group_with_any_stale_patch_is_rejected_entirely(
            original in prop::collection::vec(any::<u8>(), 1..64),
            history in prop::collection::vec(write(), 0..4),
            undos in 0..3usize,
            lead in prop::collection::vec(write(), 0..4),
            s in spoil(),
            tail in prop::collection::vec(write(), 0..4),
        ) {
            let mut j = journal_after(&original, &history, undos);
            let before = observe(&j);
            let snapshot = j.clone();

            let mut model = j.image().bytes().to_vec();
            let mut group: Vec<Patch> = lead.iter().map(|w| valid(&mut model, w)).collect();
            let (p, offset, expected, found) = stale_against(&model, &s);
            group.push(p);
            group.extend(tail.iter().map(|w| valid(&mut model, w)));

            let refused = j.apply_group(group);

            prop_assert_eq!(refused, Err(stale(lead.len(), offset, expected, found)));
            prop_assert_eq!(observe(&j), before);
            prop_assert_eq!(&j, &snapshot);
        }
    }
}
