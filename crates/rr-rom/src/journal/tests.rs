use core::error::Error as _;

use proptest::prelude::*;

use super::{Group, Journal, JournalError, Origin, Patch, PatchError};
use crate::image::{ApplyError, Image, ImageHash};

fn patch(at: usize, old: &[u8], new: &[u8]) -> Patch {
    match Patch::new(at, old, new, Origin::Edit) {
        Ok(p) => p,
        Err(e) => panic!("test patch is well-formed: {e}"),
    }
}

fn journal(bytes: &[u8]) -> Journal {
    Journal::new(Image::new(bytes.to_vec()))
}

fn ok<T>(r: Result<T, JournalError>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("journal refused: {e}"),
    }
}

fn group_patches(group: Option<&Group>) -> Vec<Patch> {
    group.map(|g| g.patches().to_vec()).unwrap_or_default()
}

#[test]
fn patch_new_records_its_parts() {
    let p = match Patch::new(3, vec![1, 2], [7u8, 8].as_slice(), Origin::Paste) {
        Ok(p) => p,
        Err(e) => panic!("{e}"),
    };
    assert_eq!(p.range(), 3..5);
    assert_eq!(p.old_bytes(), &[1, 2]);
    assert_eq!(p.new_bytes(), &[7, 8]);
    assert_eq!(p.origin(), Origin::Paste);

    let empty = patch(usize::MAX, &[], &[]);
    assert_eq!(empty.range(), usize::MAX..usize::MAX);
}

#[test]
fn patch_new_refuses_malformed_patches() {
    assert_eq!(
        Patch::new(0, vec![1], vec![1, 2], Origin::Edit),
        Err(PatchError::LengthMismatch { old: 1, new: 2 })
    );
    assert_eq!(
        Patch::new(usize::MAX, vec![1], vec![2], Origin::Edit),
        Err(PatchError::RangeOverflow {
            at: usize::MAX,
            len: 1
        })
    );
    assert!(Patch::new(usize::MAX - 1, vec![1], vec![2], Origin::Edit).is_ok());
}

#[test]
fn patch_replacing_reads_old_bytes_from_the_image() {
    let image = Image::new(vec![1, 2, 3, 4]);
    assert_eq!(
        Patch::replacing(&image, 1, vec![8, 9], Origin::ChecksumFix),
        Patch::new(1, vec![2, 3], vec![8, 9], Origin::ChecksumFix)
    );
    assert_eq!(
        Patch::replacing(&image, 4, Vec::new(), Origin::Edit),
        Patch::new(4, Vec::new(), Vec::new(), Origin::Edit)
    );
    assert_eq!(
        Patch::replacing(&image, 3, vec![0, 0], Origin::Edit),
        Err(PatchError::OutOfBounds {
            at: 3,
            len: 2,
            image_len: 4
        })
    );
    assert_eq!(
        Patch::replacing(&image, usize::MAX, vec![0], Origin::Edit),
        Err(PatchError::RangeOverflow {
            at: usize::MAX,
            len: 1
        })
    );
}

#[test]
fn origins_and_errors_display() {
    assert_eq!(Origin::Edit.to_string(), "edit");
    assert_eq!(Origin::Paste.to_string(), "paste");
    assert_eq!(Origin::ChecksumFix.to_string(), "checksum fix");
    assert_eq!(Origin::ApplyDiff.to_string(), "apply diff");
    assert_eq!(
        PatchError::LengthMismatch { old: 1, new: 2 }.to_string(),
        "patch replaces 1 bytes with 2; a patch may not resize the image"
    );
    assert_eq!(
        PatchError::RangeOverflow { at: 0x10, len: 2 }.to_string(),
        "patch of 2 bytes at 0x10 ends past the largest address"
    );
    assert_eq!(
        PatchError::OutOfBounds {
            at: 0x10,
            len: 2,
            image_len: 16
        }
        .to_string(),
        "patch of 2 bytes at 0x10 reaches past the end of the 16-byte image"
    );
    let stale = ApplyError::Stale {
        offset: 0x20,
        expected: 1,
        found: 2,
    };
    let err = JournalError::Refused {
        index: 3,
        error: stale,
    };
    assert_eq!(
        err.to_string(),
        "patch 3 refused: stale patch: expected byte 0x01 at 0x20, the image holds 0x02"
    );
    assert_eq!(
        err.source().map(ToString::to_string),
        Some(stale.to_string())
    );
}

#[test]
fn a_new_journal_has_no_history() {
    let mut j = journal(&[1, 2, 3]);
    assert!(!j.can_undo());
    assert!(!j.can_redo());
    assert!(j.applied().is_empty());
    assert!(j.undone().is_empty());
    assert_eq!(j.undo(), Ok(None));
    assert_eq!(j.redo(), Ok(None));
    assert_eq!(j.undo_all(), Ok(()));
    assert_eq!(j.redo_all(), Ok(()));
    assert_eq!(j.image().bytes(), &[1, 2, 3]);
    assert_eq!(Journal::from(Image::new(vec![1, 2, 3])), j);
    assert_eq!(j.into_image(), Image::new(vec![1, 2, 3]));
}

#[test]
fn apply_undo_redo_one_patch() {
    let mut j = journal(&[1, 2, 3]);
    let p = patch(1, &[2], &[9]);
    assert_eq!(j.apply(p.clone()), Ok(()));
    assert_eq!(j.image().bytes(), &[1, 9, 3]);
    assert!(j.image().is_modified());
    assert!(j.can_undo());
    assert!(!j.can_redo());
    assert_eq!(j.applied().len(), 1);
    assert_eq!(j.applied()[0].patches(), core::slice::from_ref(&p));
    assert!(j.undone().is_empty());

    assert_eq!(group_patches(ok(j.undo())), vec![p.clone()]);
    assert_eq!(j.image().bytes(), &[1, 2, 3]);
    assert!(!j.image().is_modified());
    assert!(!j.can_undo());
    assert!(j.can_redo());
    assert!(j.applied().is_empty());
    assert_eq!(j.undone().len(), 1);
    assert_eq!(j.undone()[0].patches(), core::slice::from_ref(&p));

    assert_eq!(group_patches(ok(j.redo())), vec![p]);
    assert_eq!(j.image().bytes(), &[1, 9, 3]);
    assert!(j.can_undo());
    assert!(!j.can_redo());
    assert_eq!(j.redo(), Ok(None));
}

#[test]
fn a_group_is_one_undo_step_with_patches_applied_in_order() {
    let mut j = journal(&[0, 0, 0]);
    // Overlapping patches: the second is checked against what the first
    // wrote, and undo must revert the second before the first.
    let group = [
        patch(0, &[0, 0], &[1, 1]),
        patch(1, &[1, 0], &[2, 2]),
        patch(0, &[1], &[3]),
    ];
    assert_eq!(j.apply_group(group.clone()), Ok(()));
    assert_eq!(j.image().bytes(), &[3, 2, 2]);
    assert_eq!(j.applied().len(), 1);
    assert_eq!(j.applied()[0].patches(), &group);

    assert_eq!(group_patches(ok(j.undo())), group.to_vec());
    assert_eq!(j.image().bytes(), &[0, 0, 0]);
    assert_eq!(j.undo(), Ok(None));

    assert_eq!(group_patches(ok(j.redo())), group.to_vec());
    assert_eq!(j.image().bytes(), &[3, 2, 2]);
}

#[test]
fn undo_and_redo_step_one_group_at_a_time() {
    let mut j = journal(&[0, 0]);
    assert_eq!(j.apply(patch(0, &[0], &[1])), Ok(()));
    assert_eq!(j.apply(patch(0, &[1], &[2])), Ok(()));
    assert_eq!(j.apply(patch(1, &[0], &[5])), Ok(()));
    assert_eq!(j.image().bytes(), &[2, 5]);
    assert_eq!(j.applied().len(), 3);

    assert!(ok(j.undo()).is_some());
    assert_eq!(j.image().bytes(), &[2, 0]);
    assert!(ok(j.undo()).is_some());
    assert_eq!(j.image().bytes(), &[1, 0]);
    assert_eq!((j.applied().len(), j.undone().len()), (1, 2));
    assert!(j.can_undo());
    assert!(j.can_redo());
    assert_eq!(j.undone()[0].patches(), &[patch(0, &[1], &[2])]);

    assert!(ok(j.redo()).is_some());
    assert_eq!(j.image().bytes(), &[2, 0]);
    assert_eq!((j.applied().len(), j.undone().len()), (2, 1));
}

#[test]
fn undo_all_and_redo_all() {
    let mut j = journal(&[0, 0, 0]);
    for i in 0..3 {
        assert_eq!(j.apply(patch(i, &[0], &[i as u8 + 1])), Ok(()));
    }
    assert_eq!(j.undo_all(), Ok(()));
    assert_eq!(j.image().bytes(), &[0, 0, 0]);
    assert!(!j.can_undo());
    assert_eq!(j.undone().len(), 3);
    assert_eq!(j.redo_all(), Ok(()));
    assert_eq!(j.image().bytes(), &[1, 2, 3]);
    assert!(!j.can_redo());
    assert_eq!(j.applied().len(), 3);
}

#[test]
fn applying_after_undo_discards_the_undone_groups() {
    let mut j = journal(&[0, 0]);
    assert_eq!(j.apply(patch(0, &[0], &[1])), Ok(()));
    assert_eq!(j.apply(patch(1, &[0], &[2])), Ok(()));
    assert!(ok(j.undo()).is_some());
    assert!(ok(j.undo()).is_some());
    assert_eq!(j.undone().len(), 2);

    let p = patch(1, &[0], &[7]);
    assert_eq!(j.apply(p.clone()), Ok(()));
    assert_eq!(j.image().bytes(), &[0, 7]);
    assert!(!j.can_redo());
    assert!(j.undone().is_empty());
    assert_eq!(j.applied().len(), 1);
    assert_eq!(j.applied()[0].patches(), &[p]);
    assert_eq!(j.redo(), Ok(None));
}

#[test]
fn a_refused_group_is_rolled_back_and_changes_no_history() {
    let mut j = journal(&[0, 0, 0]);
    assert_eq!(j.apply(patch(2, &[0], &[4])), Ok(()));
    assert_eq!(j.apply(patch(2, &[4], &[5])), Ok(()));
    assert!(ok(j.undo()).is_some());
    let before = j.clone();

    // Two overlapping patches apply, the third is stale: both must be
    // reverted, newest first, and the undone group kept.
    let refused = j.apply_group([
        patch(0, &[0, 0], &[1, 1]),
        patch(1, &[1], &[2]),
        patch(1, &[0], &[3]),
    ]);
    assert_eq!(
        refused,
        Err(JournalError::Refused {
            index: 2,
            error: ApplyError::Stale {
                offset: 1,
                expected: 0,
                found: 2
            }
        })
    );
    assert_eq!(j, before);
    assert_eq!(j.image().bytes(), &[0, 0, 4]);
    assert!(j.can_redo());

    // The first patch refused: nothing to roll back.
    assert_eq!(
        j.apply(patch(3, &[0], &[1])),
        Err(JournalError::Refused {
            index: 0,
            error: ApplyError::OutOfBounds {
                at: 3,
                len: 1,
                image_len: 3
            }
        })
    );
    assert_eq!(j, before);
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

#[test]
fn a_patch_built_before_another_edit_is_stale() {
    let mut j = journal(&[0x10, 0x20, 0x30]);
    let late = match Patch::replacing(j.image(), 1, vec![0xEE, 0xEE], Origin::Edit) {
        Ok(p) => p,
        Err(e) => panic!("{e}"),
    };
    assert_eq!(j.apply(patch(2, &[0x30], &[0x31])), Ok(()));
    assert_eq!(j.apply(patch(2, &[0x31], &[0x32])), Ok(()));
    assert!(ok(j.undo()).is_some());
    let before = j.clone();

    // Its first byte still matches; the second was changed after it was
    // built. Refused, reporting that byte, and nothing changes: the image,
    // the applied groups and the undone group (still redoable).
    let refused = j.apply(late);
    assert_eq!(refused, Err(stale(0, 2, 0x30, 0x31)));
    assert!(refused.is_err_and(|e| e.is_stale()));
    assert_eq!(j, before);
    assert_eq!(j.image().bytes(), &[0x10, 0x20, 0x31]);
    assert_eq!(j.image().modified_ranges(), vec![2..3]);
    assert_eq!(
        group_patches(ok(j.redo())),
        vec![patch(2, &[0x31], &[0x32])]
    );
}

#[test]
fn old_bytes_are_checked_against_the_current_image_not_the_original() {
    let mut j = journal(&[1, 2]);
    assert_eq!(j.apply(patch(0, &[1], &[5])), Ok(()));
    // Matches the original, not the image: stale.
    assert_eq!(j.apply(patch(0, &[1], &[6])), Err(stale(0, 0, 1, 5)));
    // Matches the image: applies.
    assert_eq!(j.apply(patch(0, &[5], &[6])), Ok(()));
    assert_eq!(j.image().bytes(), &[6, 2]);
}

#[test]
fn a_stale_patch_in_a_group_refuses_the_whole_group() {
    let mut j = journal(&[0, 0, 0, 0]);
    let before = j.clone();
    assert_eq!(
        j.apply_group([
            patch(0, &[0], &[1]),
            patch(1, &[0, 0], &[2, 2]),
            patch(3, &[9], &[3]),
        ]),
        Err(stale(2, 3, 9, 0))
    );
    assert_eq!(j, before);
    assert!(!j.image().is_modified());
    assert!(j.image().modified_ranges().is_empty());
}

#[test]
fn only_stale_refusals_are_stale() {
    assert!(stale(0, 0, 0, 1).is_stale());
    assert!(
        !JournalError::Refused {
            index: 0,
            error: ApplyError::OutOfBounds {
                at: 3,
                len: 1,
                image_len: 3
            }
        }
        .is_stale()
    );
    assert!(
        !JournalError::Refused {
            index: 0,
            error: ApplyError::LengthMismatch { old: 1, new: 2 }
        }
        .is_stale()
    );
}

/// Changes the journal's image behind its history's back, which no code
/// outside this crate can do: a test of the defence in undo and redo.
fn diverge(j: &mut Journal, at: usize, old: u8, new: u8) {
    assert_eq!(j.image.apply_patch(at, &[old], &[new]), Ok(()));
}

#[test]
fn redo_after_divergence_is_refused_and_changes_nothing() {
    let mut j = journal(&[0, 0, 0]);
    assert_eq!(
        j.apply_group([patch(0, &[0], &[1]), patch(2, &[0], &[2])]),
        Ok(())
    );
    assert!(ok(j.undo()).is_some());
    diverge(&mut j, 2, 0, 7);
    let before = j.clone();

    // The group's first patch applies and is rolled back; the second is
    // stale.
    assert_eq!(j.redo(), Err(stale(1, 2, 0, 7)));
    assert_eq!(j, before);
    assert_eq!(j.image().bytes(), &[0, 0, 7]);
    assert!(j.can_redo());
    assert!(!j.can_undo());
    assert_eq!(j.redo_all(), Err(stale(1, 2, 0, 7)));
    assert_eq!(j, before);
}

#[test]
fn undo_after_divergence_is_refused_and_changes_nothing() {
    let mut j = journal(&[0, 0, 0]);
    assert_eq!(j.apply(patch(1, &[0], &[4])), Ok(()));
    assert_eq!(
        j.apply_group([patch(0, &[0], &[1]), patch(2, &[0], &[2])]),
        Ok(())
    );
    diverge(&mut j, 0, 1, 9);
    let before = j.clone();

    // Undo reverts the last patch first (index 0), then the stale one.
    assert_eq!(j.undo().map(|g| g.is_some()), Err(stale(1, 0, 1, 9)));
    assert_eq!(j, before);
    assert_eq!(j.image().bytes(), &[9, 4, 2]);
    assert_eq!(j.applied().len(), 2);
    assert!(!j.can_redo());
    assert_eq!(j.undo_all(), Err(stale(1, 0, 1, 9)));
    assert_eq!(j, before);
}

#[test]
fn an_empty_group_records_nothing_and_keeps_the_undone_groups() {
    let mut j = journal(&[0]);
    assert_eq!(j.apply(patch(0, &[0], &[1])), Ok(()));
    assert!(ok(j.undo()).is_some());
    let before = j.clone();
    assert_eq!(j.apply_group([]), Ok(()));
    assert_eq!(j, before);
    assert!(j.can_redo());
}

#[test]
fn the_image_original_never_changes() {
    let mut j = journal(&[1, 2]);
    let hash = j.image().original_hash();
    assert_eq!(j.apply(patch(0, &[1, 2], &[3, 4])), Ok(()));
    assert_eq!(j.image().original(), &[1, 2]);
    assert_eq!(j.image().original_hash(), hash);
}

/// One write in a generated group: `new` bytes at `offset`. The offset is
/// taken modulo the room left in the image, and the bytes are cut to the
/// image length, so every write fits; empty `new` is a zero-length patch.
/// Small offsets are favoured so writes often overlap and rewrite the same
/// bytes, within a group and across groups.
type Write = (usize, Vec<u8>);

/// One step of a generated session.
#[derive(Debug, Clone)]
enum Op {
    /// Apply these writes as one group (no writes: the empty-group no-op).
    Apply(Vec<Write>),
    /// Undo one group.
    Undo,
    /// Redo one group.
    Redo,
}

fn op() -> impl Strategy<Value = Op> {
    let offset = prop_oneof![0..4usize, any::<usize>()];
    let write = (offset, prop::collection::vec(any::<u8>(), 0..6));
    prop_oneof![
        4 => prop::collection::vec(write, 0..5).prop_map(Op::Apply),
        1 => Just(Op::Undo),
        1 => Just(Op::Redo),
    ]
}

/// The patches of one generated group, valid by construction: each patch's
/// old bytes are read from `model`, which the patches before it in the
/// group have already been written into. Leaves `model` as the group does.
fn group_of(model: &mut [u8], writes: &[Write]) -> Vec<Patch> {
    writes
        .iter()
        .map(|(offset, new)| {
            let len = new.len().min(model.len());
            let at = offset % (model.len() - len + 1);
            let range = at..at + len;
            let p = patch(at, &model[range.clone()], &new[..len]);
            model[range].copy_from_slice(&new[..len]);
            p
        })
        .collect()
}

/// The journal's image is exactly `expected`, current hash included.
fn assert_bytes(j: &Journal, expected: &[u8]) -> Result<(), TestCaseError> {
    prop_assert_eq!(j.image().bytes(), expected);
    prop_assert_eq!(j.image().current_hash(), ImageHash::of(expected));
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// US-RR-31-1: for any sequence of patches, undo-all restores the
    /// original bytes exactly and redo-all reproduces the final bytes
    /// exactly.
    ///
    /// Any image (empty included) and any session of groups -- multi-patch,
    /// overlapping, rewriting the same bytes, zero-length, empty -- with
    /// partial undos and redos interleaved, checked against a plain byte
    /// model after every step. Then undo-all leaves the original bytes
    /// (same hash, not modified, no modified ranges) and redo-all the final
    /// bytes of the history, and each single undo and redo between them
    /// steps exactly one state.
    #[test]
    fn undo_all_restores_original_and_redo_all_reproduces_final_bytes(
        original in prop::collection::vec(any::<u8>(), 0..48),
        ops in prop::collection::vec(op(), 0..24),
    ) {
        let mut j = journal(&original);
        // states[..=cursor] applied, states[cursor + 1..] undone.
        let mut states = vec![original.clone()];
        let mut cursor = 0;
        for op in &ops {
            match op {
                Op::Apply(writes) => {
                    let mut model = states[cursor].clone();
                    let group = group_of(&mut model, writes);
                    prop_assert_eq!(j.apply_group(group), Ok(()));
                    if !writes.is_empty() {
                        states.truncate(cursor + 1);
                        states.push(model);
                        cursor += 1;
                    }
                }
                Op::Undo => {
                    prop_assert_eq!(j.undo().map(|g| g.is_some()), Ok(cursor > 0));
                    cursor = cursor.saturating_sub(1);
                }
                Op::Redo => {
                    let more = cursor + 1 < states.len();
                    prop_assert_eq!(j.redo().map(|g| g.is_some()), Ok(more));
                    cursor += usize::from(more);
                }
            }
            assert_bytes(&j, &states[cursor])?;
            prop_assert_eq!(j.applied().len(), cursor);
            prop_assert_eq!(j.undone().len(), states.len() - 1 - cursor);
        }
        let last = states.len() - 1;
        let finished = states[last].clone();

        // Undo-all: the original bytes exactly.
        prop_assert_eq!(j.undo_all(), Ok(()));
        assert_bytes(&j, &original)?;
        prop_assert_eq!(j.image().current_hash(), j.image().original_hash());
        prop_assert!(!j.image().is_modified());
        prop_assert!(j.image().modified_ranges().is_empty());
        prop_assert_eq!(j.image().original(), &original[..]);
        prop_assert!(!j.can_undo());
        prop_assert_eq!(j.undone().len(), last);

        // Redo-all: the final bytes exactly.
        prop_assert_eq!(j.redo_all(), Ok(()));
        assert_bytes(&j, &finished)?;
        prop_assert_eq!(j.image().is_modified(), finished != original);
        prop_assert!(!j.can_redo());
        prop_assert_eq!(j.applied().len(), last);

        // Each undo, then each redo, steps exactly one state.
        for state in states[..last].iter().rev() {
            prop_assert!(j.undo().is_ok_and(|g| g.is_some()));
            assert_bytes(&j, state)?;
        }
        prop_assert_eq!(j.undo(), Ok(None));
        for state in &states[1..] {
            prop_assert!(j.redo().is_ok_and(|g| g.is_some()));
            assert_bytes(&j, state)?;
        }
        prop_assert_eq!(j.redo(), Ok(None));

        // And the round trip repeats.
        prop_assert_eq!(j.undo_all(), Ok(()));
        assert_bytes(&j, &original)?;
        prop_assert_eq!(j.redo_all(), Ok(()));
        assert_bytes(&j, &finished)?;
    }
}
