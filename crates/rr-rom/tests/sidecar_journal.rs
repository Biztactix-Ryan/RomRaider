//! US-RR-31-3: the sidecar journal round-trips losslessly (property test)
//! and rejects a journal whose base image hash does not match.
//!
//! Exercised only through rr-rom's public API, as the save pipeline uses
//! it: build a [`Journal`] session over a ROM image, [`sidecar::encode`] it
//! with its checksum overrides, and [`sidecar::decode`] the text back onto
//! the ROM as loaded.
//!
//! * **Round trip**: any session (any image; any groups of patches with any
//!   origins, some undone so the redo stack is never empty; any checksum
//!   overrides) reads back equal in everything the API shows: the current
//!   and original bytes and their hashes, the modified ranges, every
//!   applied and undone group (patches, origins, order), undo/redo
//!   behaviour, and the overrides. Encoding what was read back gives the
//!   same text, byte for byte.
//! * **Base binding**: decoding onto any image whose original bytes are not
//!   the recorded base is refused with [`SidecarError::BaseMismatch`] naming
//!   both hashes: other bytes of the same length, a different length, or
//!   an image that holds the base bytes only because it was edited into
//!   them. An image with the right original bytes but changes of its own is
//!   refused with [`SidecarError::ImageModified`]. The check comes before
//!   any replay, so a history that could not even be applied to the wrong
//!   image is still reported as a base mismatch; on refusal `decode`
//!   returns nothing, so nothing is applied.
//! * **Tampering**: a sidecar whose `base_sha256` line was edited is
//!   refused, both against the real ROM and against the image the edited
//!   hash names.
//!
//! The crate's unit tests (`src/journal/sidecar/tests.rs`) cover the format
//! line by line from the inside: layout, envelope, malformed records, head
//! hash tampering and limits.

// In a `cfg(test)` module so the workspace's unwrap/expect/panic-in-tests
// allowance (clippy.toml) covers the helpers too.
#[cfg(test)]
mod sidecar_journal {
    use core::ops::Range;

    use proptest::prelude::*;
    use rr_rom::image::{Image, ImageHash};
    use rr_rom::journal::sidecar::{
        self, ChecksumFailure, ChecksumOverride, MAX_CHECKSUM_VALUE_LEN, MAX_CONFIRMED_AT_UNIX,
        RecordError, Restored, SidecarError,
    };
    use rr_rom::journal::{Group, Journal, Origin, Patch};

    // --- Building sessions --------------------------------------------------

    /// One write of a group: `(at, new bytes, origin)`, clipped to the image
    /// when the group is built.
    type Write = (usize, Vec<u8>, Origin);

    /// One user action before the final groups: a group of writes, or an
    /// undo (a later group then discards the redo stack, as in the editor).
    #[derive(Debug, Clone)]
    enum Step {
        Group(Vec<Write>),
        Undo,
    }

    /// An editing session: what happened, then `tail` groups of which the
    /// last `undo` are undone, so the redo stack is never empty.
    #[derive(Debug, Clone)]
    struct Session {
        steps: Vec<Step>,
        tail: Vec<Vec<Write>>,
        undo: usize,
    }

    fn origin() -> impl Strategy<Value = Origin> {
        prop_oneof![
            Just(Origin::Edit),
            Just(Origin::Paste),
            Just(Origin::ChecksumFix),
            Just(Origin::ApplyDiff),
        ]
    }

    fn group() -> impl Strategy<Value = Vec<Write>> {
        let write = (
            0usize..80,
            prop::collection::vec(any::<u8>(), 0..8),
            origin(),
        );
        prop::collection::vec(write, 1..5)
    }

    fn session() -> impl Strategy<Value = Session> {
        let step = prop_oneof![3 => group().prop_map(Step::Group), 1 => Just(Step::Undo)];
        (
            prop::collection::vec(step, 0..8),
            prop::collection::vec(group(), 1..5),
            any::<usize>(),
        )
            .prop_map(|(steps, tail, undo)| Session {
                undo: undo % tail.len() + 1,
                steps,
                tail,
            })
    }

    /// Applies `writes` to `journal` as one group. Each write is clipped to
    /// the image and its old bytes are what the image holds once the writes
    /// before it in the group are done, as the editor builds a group.
    fn apply(journal: &mut Journal, writes: &[Write]) {
        let mut scratch = journal.image().bytes().to_vec();
        let mut patches = Vec::new();
        for (at, new, origin) in writes {
            let at = at % (scratch.len() + 1);
            let new = &new[..new.len().min(scratch.len() - at)];
            let range = at..at + new.len();
            let old = scratch[range.clone()].to_vec();
            scratch[range].copy_from_slice(new);
            patches.push(Patch::new(at, old, new, *origin).unwrap());
        }
        journal.apply_group(patches).unwrap();
    }

    /// The journal `session` leaves over an image of `bytes`.
    fn run(bytes: &[u8], session: &Session) -> Journal {
        let mut journal = Journal::new(Image::new(bytes.to_vec()));
        for step in &session.steps {
            match step {
                Step::Group(writes) => apply(&mut journal, writes),
                Step::Undo => {
                    journal.undo().unwrap();
                }
            }
        }
        for writes in &session.tail {
            apply(&mut journal, writes);
        }
        for _ in 0..session.undo {
            assert!(journal.undo().unwrap().is_some());
        }
        assert!(journal.can_redo(), "the session leaves a redo stack");
        journal
    }

    /// Checksum overrides as the save pipeline records them: any image
    /// hash, any time in range, one or more failures with any valid name
    /// and any equal-length values.
    fn overrides() -> impl Strategy<Value = Vec<ChecksumOverride>> {
        let value = 1usize..=MAX_CHECKSUM_VALUE_LEN;
        let failure = ("[ -!#-\\[\\]-~]{1,64}", value).prop_flat_map(|(name, len)| {
            (
                Just(name),
                prop::collection::vec(any::<u8>(), len),
                prop::collection::vec(any::<u8>(), len),
            )
                .prop_map(|(name, stored, computed)| {
                    ChecksumFailure::new(name, stored, computed).unwrap()
                })
        });
        let one = (
            any::<[u8; 32]>(),
            0..=MAX_CONFIRMED_AT_UNIX,
            prop::collection::vec(failure, 1..4),
        )
            .prop_map(|(hash, secs, failures)| {
                ChecksumOverride::new(ImageHash::from_bytes(hash), secs, failures).unwrap()
            });
        prop::collection::vec(one, 0..4)
    }

    fn encode(journal: &Journal, overrides: &[ChecksumOverride]) -> String {
        sidecar::encode(journal, overrides).unwrap()
    }

    fn decode(text: &str, image: Image) -> Restored {
        match sidecar::decode(text, image) {
            Ok(restored) => restored,
            Err(e) => panic!("decode: {e}\n{text}"),
        }
    }

    fn refusal(text: &str, image: Image) -> SidecarError {
        match sidecar::decode(text, image) {
            Ok(restored) => panic!("decode accepted: {restored:?}\n{text}"),
            Err(e) => e,
        }
    }

    // --- What the API shows -------------------------------------------------

    /// One patch as the API shows it: its range, old and new bytes, origin.
    type PatchView = (Range<usize>, Vec<u8>, Vec<u8>, Origin);

    fn groups(groups: &[Group]) -> Vec<Vec<PatchView>> {
        groups
            .iter()
            .map(|g| {
                g.patches()
                    .iter()
                    .map(|p| {
                        (
                            p.range(),
                            p.old_bytes().to_vec(),
                            p.new_bytes().to_vec(),
                            p.origin(),
                        )
                    })
                    .collect()
            })
            .collect()
    }

    /// Everything a journal shows through the public API.
    #[derive(Debug, PartialEq)]
    struct Observed {
        bytes: Vec<u8>,
        original: Vec<u8>,
        hash: ImageHash,
        original_hash: ImageHash,
        modified: Vec<Range<usize>>,
        applied: Vec<Vec<PatchView>>,
        undone: Vec<Vec<PatchView>>,
        can_undo: bool,
        can_redo: bool,
    }

    fn observe(j: &Journal) -> Observed {
        Observed {
            bytes: j.image().bytes().to_vec(),
            original: j.image().original().to_vec(),
            hash: j.image().current_hash(),
            original_hash: j.image().original_hash(),
            modified: j.image().modified_ranges(),
            applied: groups(j.applied()),
            undone: groups(j.undone()),
            can_undo: j.can_undo(),
            can_redo: j.can_redo(),
        }
    }

    /// One checksum override as the API shows it.
    type OverrideView = (ImageHash, u64, Vec<(String, Vec<u8>, Vec<u8>)>);

    fn observe_overrides(overrides: &[ChecksumOverride]) -> Vec<OverrideView> {
        overrides
            .iter()
            .map(|o| {
                let failures = o
                    .failures()
                    .iter()
                    .map(|f| {
                        (
                            f.checksum().to_owned(),
                            f.stored().to_vec(),
                            f.computed().to_vec(),
                        )
                    })
                    .collect();
                (o.image(), o.confirmed_at_unix(), failures)
            })
            .collect()
    }

    /// The text with its `base_sha256` line recording `hash` instead.
    fn with_base(text: &str, real: ImageHash, hash: &str) -> String {
        let line = format!("base_sha256 = \"{real}\"\n");
        assert_eq!(text.matches(&line).count(), 1, "{text}");
        text.replace(&line, &format!("base_sha256 = \"{hash}\"\n"))
    }

    // --- Round trip ---------------------------------------------------------

    proptest! {
        /// Any session and overrides read back equal in every way the API
        /// shows, behave the same under redo and undo, and encode back to
        /// the same text.
        #[test]
        fn a_session_round_trips_losslessly(
            bytes in prop::collection::vec(any::<u8>(), 0..64),
            session in session(),
            overrides in overrides(),
        ) {
            let mut journal = run(&bytes, &session);
            let text = encode(&journal, &overrides);

            let Restored { journal: mut restored, overrides: restored_overrides } =
                decode(&text, Image::new(bytes.clone()));

            prop_assert_eq!(observe(&restored), observe(&journal));
            prop_assert!(!restored.undone().is_empty());
            prop_assert_eq!(&restored, &journal);
            prop_assert_eq!(
                observe_overrides(&restored_overrides),
                observe_overrides(&overrides)
            );
            prop_assert_eq!(&restored_overrides, &overrides);
            prop_assert_eq!(encode(&restored, &restored_overrides), text.clone());

            // The redo stack and the undo history work as they did before
            // the save: redo everything, then undo back to the ROM as loaded.
            journal.redo_all().unwrap();
            restored.redo_all().unwrap();
            prop_assert_eq!(observe(&restored), observe(&journal));
            journal.undo_all().unwrap();
            restored.undo_all().unwrap();
            prop_assert_eq!(observe(&restored), observe(&journal));
            prop_assert_eq!(restored.image().bytes(), bytes.as_slice());
        }
    }

    #[test]
    fn a_fixed_session_round_trips_with_its_redo_stack_and_overrides() {
        let rom = vec![0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
        let mut journal = Journal::new(Image::new(rom.clone()));
        apply(
            &mut journal,
            &[
                (1, vec![0xA1], Origin::Edit),
                (4, vec![0xA4, 0xA5], Origin::Paste),
            ],
        );
        apply(&mut journal, &[(0, vec![0xB0], Origin::ChecksumFix)]);
        apply(&mut journal, &[(2, vec![0xC2, 0xC3], Origin::ApplyDiff)]);
        journal.undo().unwrap();
        journal.undo().unwrap();
        let overrides = [ChecksumOverride::new(
            journal.image().current_hash(),
            1_792_000_000,
            [ChecksumFailure::new("subaru-sum32", [1, 2, 3, 4], [5, 6, 7, 8]).unwrap()],
        )
        .unwrap()];

        let text = encode(&journal, &overrides);
        let restored = decode(&text, Image::new(rom));

        assert_eq!(
            restored.journal.image().bytes(),
            &[0x00, 0xA1, 0x22, 0x33, 0xA4, 0xA5]
        );
        assert_eq!(restored.journal.applied().len(), 1);
        assert_eq!(restored.journal.undone().len(), 2);
        assert_eq!(
            groups(restored.journal.undone()),
            vec![
                vec![(0..1, vec![0x00], vec![0xB0], Origin::ChecksumFix)],
                vec![(2..4, vec![0x22, 0x33], vec![0xC2, 0xC3], Origin::ApplyDiff)],
            ]
        );
        assert_eq!(observe(&restored.journal), observe(&journal));
        assert_eq!(restored.overrides, overrides);
        assert_eq!(encode(&restored.journal, &restored.overrides), text);
    }

    // --- Base binding -------------------------------------------------------

    proptest! {
        /// Other bytes of the same length are not the base.
        #[test]
        fn other_bytes_of_the_same_length_are_refused(
            bytes in prop::collection::vec(any::<u8>(), 1..64),
            session in session(),
            at in any::<usize>(),
            mask in 1u8..,
        ) {
            let journal = run(&bytes, &session);
            let text = encode(&journal, &[]);
            let mut other = bytes.clone();
            let i = at % other.len();
            other[i] ^= mask;

            prop_assert_eq!(
                sidecar::decode(&text, Image::new(other.clone())),
                Err(SidecarError::BaseMismatch {
                    expected: ImageHash::of(&bytes),
                    found: ImageHash::of(&other),
                })
            );
        }

        /// An image of a different length is not the base, and the base is
        /// checked before any replay: a history whose patches would reach
        /// past the end of a shorter image is still a base mismatch, not a
        /// replay error.
        #[test]
        fn an_image_of_another_length_is_refused_before_replay(
            bytes in prop::collection::vec(any::<u8>(), 0..64),
            session in session(),
            len in 0usize..96,
            fill in any::<u8>(),
        ) {
            prop_assume!(len != bytes.len());
            let journal = run(&bytes, &session);
            let text = encode(&journal, &[]);
            let mut other = bytes.clone();
            other.resize(len, fill);

            prop_assert_eq!(
                sidecar::decode(&text, Image::new(other.clone())),
                Err(SidecarError::BaseMismatch {
                    expected: ImageHash::of(&bytes),
                    found: ImageHash::of(&other),
                })
            );
        }

        /// The right ROM, but already modified: an image whose original is
        /// the base but which carries changes is refused as modified (a
        /// session whose changes all cancel out leaves it unmodified, and it
        /// is accepted); an image that holds the journal's head bytes, or
        /// was edited into the base bytes from some other ROM, is not the
        /// base.
        #[test]
        fn the_base_already_modified_is_refused(
            bytes in prop::collection::vec(any::<u8>(), 1..64),
            session in session(),
            from in prop::collection::vec(any::<u8>(), 1..64),
        ) {
            let journal = run(&bytes, &session);
            let text = encode(&journal, &[]);
            let base_hash = ImageHash::of(&bytes);

            // The same session's image: base original, the session's changes
            // (the redo stack's groups are not among them).
            let edited = run(&bytes, &session).into_image();
            if edited.is_modified() {
                prop_assert_eq!(
                    sidecar::decode(&text, edited),
                    Err(SidecarError::ImageModified)
                );
            } else {
                prop_assert_eq!(decode(&text, edited).journal, journal.clone());
            }

            // The head bytes loaded fresh: a different ROM unless the
            // applied groups changed nothing.
            let head = journal.image().bytes().to_vec();
            if head == bytes {
                prop_assert!(sidecar::decode(&text, Image::new(head)).is_ok());
            } else {
                prop_assert_eq!(
                    sidecar::decode(&text, Image::new(head.clone())),
                    Err(SidecarError::BaseMismatch {
                        expected: base_hash,
                        found: ImageHash::of(&head),
                    })
                );
            }

            // Another ROM of the same length edited until it holds exactly
            // the base bytes: its original is not the base.
            let mut from = from;
            from.resize(bytes.len(), 0);
            prop_assume!(from != bytes);
            let mut other = Journal::new(Image::new(from.clone()));
            other
                .apply(Patch::replacing(other.image(), 0, bytes.clone(), Origin::Edit).unwrap())
                .unwrap();
            prop_assert_eq!(other.image().bytes(), bytes.as_slice());
            prop_assert_eq!(
                sidecar::decode(&text, other.into_image()),
                Err(SidecarError::BaseMismatch {
                    expected: base_hash,
                    found: ImageHash::of(&from),
                })
            );

            // The ROM as loaded is accepted.
            prop_assert_eq!(decode(&text, Image::new(bytes.clone())).journal, journal);
        }
    }

    #[test]
    fn the_base_mismatch_error_names_both_hashes() {
        let rom = vec![0x10, 0x20, 0x30];
        let mut journal = Journal::new(Image::new(rom.clone()));
        apply(&mut journal, &[(1, vec![0x21], Origin::Edit)]);
        let text = encode(&journal, &[]);
        let other = vec![0x10, 0x20, 0x31];

        let err = refusal(&text, Image::new(other.clone()));

        assert_eq!(
            err,
            SidecarError::BaseMismatch {
                expected: ImageHash::of(&rom),
                found: ImageHash::of(&other),
            }
        );
        let message = err.to_string();
        assert!(
            message.contains(&ImageHash::of(&rom).to_string()),
            "{message}"
        );
        assert!(
            message.contains(&ImageHash::of(&other).to_string()),
            "{message}"
        );
        assert!(message.contains("nothing was applied"), "{message}");
    }

    // --- Tampered base hash -------------------------------------------------

    proptest! {
        /// A sidecar whose recorded base hash was edited, by one hex digit
        /// or wholesale, no longer reads back onto the real ROM.
        #[test]
        fn an_edited_base_hash_is_refused(
            bytes in prop::collection::vec(any::<u8>(), 0..64),
            session in session(),
            digit in 0usize..64,
            replacement in 1u8..16,
            wholesale in any::<[u8; 32]>(),
        ) {
            let journal = run(&bytes, &session);
            let real = ImageHash::of(&bytes);
            let text = encode(&journal, &[]);

            // One hex digit changed to another.
            let mut hex = real.to_string().into_bytes();
            let value = (char::from(hex[digit]).to_digit(16).unwrap() as u8 + replacement) % 16;
            hex[digit] = char::from_digit(u32::from(value), 16).unwrap() as u8;
            let edited = String::from_utf8(hex).unwrap();
            let tampered = with_base(&text, real, &edited);
            let err = refusal(&tampered, Image::new(bytes.clone()));
            let SidecarError::BaseMismatch { expected, found } = err else {
                panic!("not a base mismatch: {err:?}");
            };
            prop_assert_eq!(expected.to_string(), edited);
            prop_assert_eq!(found, real);

            // Replaced by another hash entirely.
            let fake = ImageHash::from_bytes(wholesale);
            prop_assume!(fake != real);
            let tampered = with_base(&text, real, &fake.to_string());
            prop_assert_eq!(
                sidecar::decode(&tampered, Image::new(bytes.clone())),
                Err(SidecarError::BaseMismatch { expected: fake, found: real })
            );
        }

        /// Re-pointing a sidecar at another ROM by editing its base hash to
        /// that ROM's hash does not carry the history over: the patches'
        /// old bytes or the recorded head hash give it away.
        #[test]
        fn a_base_hash_edited_to_name_another_rom_is_refused(
            bytes in prop::collection::vec(any::<u8>(), 1..64),
            session in session(),
            at in any::<usize>(),
            mask in 1u8..,
        ) {
            let journal = run(&bytes, &session);
            let text = encode(&journal, &[]);
            let mut other = bytes.clone();
            let i = at % other.len();
            other[i] ^= mask;
            let tampered = with_base(&text, ImageHash::of(&bytes), &ImageHash::of(&other).to_string());

            let err = refusal(&tampered, Image::new(other));
            prop_assert!(
                matches!(err, SidecarError::Replay { .. } | SidecarError::HeadMismatch { .. }),
                "{:?}",
                err
            );
        }
    }

    #[test]
    fn a_base_hash_edited_out_of_shape_is_refused() {
        let rom = vec![0xAA; 4];
        let journal = Journal::new(Image::new(rom.clone()));
        let real = ImageHash::of(&rom);
        let text = encode(&journal, &[]);
        let real_hex = real.to_string();

        for bad in [&real_hex[..62], "", &format!("{}zz", &real_hex[..62])] {
            let tampered = with_base(&text, real, bad);
            assert_eq!(
                refusal(&tampered, Image::new(rom.clone())),
                SidecarError::Invalid {
                    record: "base_sha256".to_owned(),
                    error: RecordError::NotSha256,
                },
                "{bad:?}"
            );
        }

        // Control: the same hash in upper case is the same hash, not an edit.
        let upper = with_base(&text, real, &real_hex.to_uppercase());
        assert_eq!(decode(&upper, Image::new(rom)).journal, journal);
    }
}
