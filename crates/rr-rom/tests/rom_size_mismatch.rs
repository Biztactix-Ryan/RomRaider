//! US-RR-34-3: a ROM whose size differs from the definition's declared size
//! produces an Error diagnostic and editing is disabled (US-RR-34,
//! acceptance criterion 3; PROJECT.md EC-02).
//!
//! # What this prevents
//!
//! Legacy RomRaider never compared the sizes: it set the ROM's RAM offset to
//! the definition's file size minus the image length
//! (`xml/DOMRomUnmarshaller.java:72-75`) and shifted every table address by
//! it, so an image of the wrong size (a partial read, a dump with a header
//! or trailing bytes, another ROM) opened silently, and an edit made
//! through the definition could change the wrong bytes.
//!
//! # How
//!
//! Exercised only through the public APIs, the way the editor opens a ROM
//! ([`open_like_the_editor`]): read the definition file into a
//! [`DefinitionSet`], identify the image in the ROM-ID index
//! ([`RomIdIndex::identify`]), resolve the definition found
//! ([`DefinitionResolver`]) and open the image through it
//! ([`OpenRom::open`]). Definitions declare `1kb`, `512kb` and `192kb`;
//! images are one byte short, one byte long and much shorter than that, and
//! the exact size is the editable control. For every mismatch:
//!
//! * exactly one diagnostic, a [`ROM_SIZE_MISMATCH`](codes::ROM_SIZE_MISMATCH)
//!   (`DEF0029`) Error on the image, naming the definition, both sizes and
//!   the difference;
//! * the Error withholds the [`OpenRom`] ([`Loaded::value`],
//!   [`Loaded::into_result`]); accepted anyway ([`Loaded::accept_errors`])
//!   it is read-only: [`OpenRom::edit`] refuses with
//!   [`EditingDisabled::SizeMismatch`] carrying both sizes, for every edit,
//!   undo and redo, on a clone too, and on a journal restored with history;
//! * nothing changes: the journal equals a fresh one over the same bytes
//!   (no patch applied, nothing to undo or redo), the bytes and their hash
//!   are the original ones;
//! * reading still works: the image's bytes, and a table found through the
//!   definition read at the address the definition gives (no legacy RAM
//!   offset shift).
//!
//! A property test covers every (declared, actual) pair in a range, in
//! `b` and `kb` units: a mismatch is exactly one `DEF0029` Error and
//! editing disabled; equal sizes are no diagnostic and editable. The
//! rendered diagnostics are pinned in one insta snapshot
//! ([`size_mismatch_snapshot`]). The crate's unit tests
//! (`src/open/tests.rs`, US-RR-34-8) cover the check from the inside:
//! inherited sizes, size units, an empty image, the largest size.
//!
//! The pinned corpus has 160 KiB 16-bit images whose definitions declare
//! `192kb`, which legacy bridged with that RAM offset
//! (`xml/DOMRomUnmarshaller.java:72-75`); by this rule they currently open
//! read-only ([`the_corpus_160_kib_against_192kb_case_opens_read_only`]),
//! pending a product decision, and this test pins that behaviour rather than
//! changing it.

// In a `cfg(test)` module so the workspace's unwrap/expect/panic-in-tests
// allowance (clippy.toml) covers the helpers too.
#[cfg(test)]
mod rom_size_mismatch {
    use std::fmt::Write as _;

    use proptest::prelude::*;
    use rr_core::diag::{Diagnostic, Loaded, Severity, Span, codes};
    use rr_defs::resolve::{Definition, DefinitionResolver, RomIdIndex};
    use rr_defs::romraider::DefinitionSet;
    use rr_rom::image::Image;
    use rr_rom::journal::{Journal, Origin, Patch};
    use rr_rom::open::{EditingDisabled, OpenRom};

    /// The name every image is opened as.
    const IMAGE: &str = "rom.bin";

    /// Where every definition's ID is.
    const ID_AT: usize = 0x10;

    /// Where every definition's `Rev Limit` table is (one `uint16` cell).
    const TABLE_AT: usize = 0x100;

    /// The bytes of `Rev Limit` in every image.
    const TABLE_BYTES: [u8; 2] = [0x0A, 0x8C];

    /// 1 KiB, 512 KiB and 192 KiB definitions, each with its own ID and a
    /// `Rev Limit` table.
    const DEFS: &str = r#"<roms>
  <rom><romid><xmlid>SMALL</xmlid><internalidaddress>10</internalidaddress><internalidstring>ROMSIZE1</internalidstring><filesize>1kb</filesize></romid>
    <table type="1D" name="Rev Limit" storagetype="uint16" storageaddress="100"/></rom>
  <rom><romid><xmlid>LARGE</xmlid><internalidaddress>10</internalidaddress><internalidstring>ROMSIZE5</internalidstring><filesize>512kb</filesize></romid>
    <table type="1D" name="Rev Limit" storagetype="uint16" storageaddress="100"/></rom>
  <rom><romid><xmlid>SIXTEEN BIT</xmlid><internalidaddress>10</internalidaddress><internalidstring>ROMSIZE9</internalidstring><filesize>192kb</filesize></romid>
    <table type="1D" name="Rev Limit" storagetype="uint16" storageaddress="100"/></rom>
</roms>
"#;

    /// One definition and image size pairing: the definition's `xmlid`,
    /// its ID, its declared size in bytes, and the image length.
    #[derive(Debug, Clone, Copy)]
    struct Case {
        label: &'static str,
        xmlid: &'static str,
        id: &'static [u8; 8],
        declared: u32,
        len: usize,
    }

    const SMALL: (&str, &[u8; 8], u32) = ("SMALL", b"ROMSIZE1", 1024);
    const LARGE: (&str, &[u8; 8], u32) = ("LARGE", b"ROMSIZE5", 512 * 1024);
    const SIXTEEN_BIT: (&str, &[u8; 8], u32) = ("SIXTEEN BIT", b"ROMSIZE9", 192 * 1024);

    const fn case(
        label: &'static str,
        (xmlid, id, declared): (&'static str, &'static [u8; 8], u32),
        len: usize,
    ) -> Case {
        Case {
            label,
            xmlid,
            id,
            declared,
            len,
        }
    }

    /// Every mismatch: one byte short, one byte long, much shorter.
    const MISMATCHES: [Case; 7] = [
        case("1kb, one byte short", SMALL, 1023),
        case("1kb, one byte long", SMALL, 1025),
        case("1kb, much shorter", SMALL, 512),
        case("512kb, one byte short", LARGE, 512 * 1024 - 1),
        case("512kb, one byte long", LARGE, 512 * 1024 + 1),
        case("512kb, much shorter", LARGE, 32 * 1024),
        case("192kb, 160 KiB (the corpus case)", SIXTEEN_BIT, 160 * 1024),
    ];

    /// The editable controls: each definition's declared size exactly.
    const MATCHES: [Case; 3] = [
        case("1kb, exact", SMALL, 1024),
        case("512kb, exact", LARGE, 512 * 1024),
        case("192kb, exact", SIXTEEN_BIT, 192 * 1024),
    ];

    fn set() -> DefinitionSet {
        let (set, read) = DefinitionSet::read_all([("defs.xml", DEFS.as_bytes())])
            .into_result()
            .unwrap();
        assert!(read.is_empty(), "{read:?}");
        set
    }

    /// An image of `case.len` bytes: a byte pattern, the case's ID at
    /// [`ID_AT`] and [`TABLE_BYTES`] at [`TABLE_AT`].
    fn bytes(case: &Case) -> Vec<u8> {
        let mut bytes: Vec<u8> = (0..case.len)
            .map(|i| u8::try_from(i % 251).unwrap())
            .collect();
        bytes[ID_AT..ID_AT + case.id.len()].copy_from_slice(case.id);
        bytes[TABLE_AT..TABLE_AT + TABLE_BYTES.len()].copy_from_slice(&TABLE_BYTES);
        bytes
    }

    /// Opens `bytes` against `set` the way the editor does: find the
    /// image's definition in the ROM-ID index, resolve it, open the image
    /// through it. Panics unless exactly one definition, `xmlid`, is found
    /// and resolves cleanly.
    fn open_like_the_editor(set: &DefinitionSet, bytes: &[u8], xmlid: &str) -> Loaded<OpenRom> {
        let index = RomIdIndex::new(set);
        let (found, identified) = index.identify(bytes, IMAGE).into_result().unwrap();
        assert!(identified.is_empty(), "{identified:?}");
        let candidate = found.unwrap();
        assert_eq!(candidate.xmlid(), xmlid);
        let (definition, resolved) = DefinitionResolver::new(set)
            .resolve(candidate.rom())
            .into_result()
            .unwrap();
        assert!(resolved.is_empty(), "{resolved:?}");
        OpenRom::open(definition.unwrap(), Image::new(bytes.to_vec()), IMAGE)
    }

    /// The `Rev Limit` cells of `rom`, read at the address its definition
    /// gives.
    fn rev_limit(rom: &OpenRom) -> &[u8] {
        let table = rom.definition().table("Rev Limit").unwrap();
        let at = usize::try_from(table.cells().address().unwrap()).unwrap();
        assert_eq!(at, TABLE_AT, "the definition's address, not shifted");
        &rom.image().bytes()[at..at + TABLE_BYTES.len()]
    }

    /// Asserts `diag` is the one `DEF0029` Error for `case`, naming the
    /// definition, both sizes and the difference.
    fn assert_size_error(diag: &Diagnostic, case: &Case) {
        assert_eq!(diag.code(), codes::ROM_SIZE_MISMATCH);
        assert_eq!(diag.code().as_str(), "DEF0029");
        assert_eq!(diag.severity(), Severity::Error);
        assert_eq!(diag.provenance().source(), IMAGE);
        let declared = u64::from(case.declared);
        let len = u64::try_from(case.len).unwrap();
        assert_eq!(
            diag.provenance().span(),
            Span::byte_offset(len.min(declared))
        );
        let message = diag.message();
        for want in [
            format!("the image is {len} bytes"),
            format!(
                "definition `{}` declares a file size of {declared} bytes",
                case.xmlid
            ),
            if len < declared {
                format!("({} bytes short)", declared - len)
            } else {
                format!("({} bytes too long)", len - declared)
            },
            "editing is disabled".to_owned(),
        ] {
            assert!(message.contains(&want), "{want:?} not in: {message}");
        }
    }

    /// Asserts `rom` refuses every change and still equals `fresh`, a
    /// journal over the bytes it was opened with: nothing applied, nothing
    /// to undo or redo, the original bytes.
    fn assert_read_only_and_unchanged(rom: &mut OpenRom, why: EditingDisabled, fresh: &Journal) {
        assert!(!rom.is_editable());
        assert_eq!(rom.editing_disabled(), Some(why));
        // No editor, so no patch, undo or redo, however often asked.
        for _ in 0..2 {
            assert_eq!(rom.edit().err(), Some(why));
        }
        assert_eq!(rom.journal(), fresh);
        assert!(rom.journal().applied().is_empty());
        assert!(!rom.journal().can_undo());
        assert!(!rom.journal().can_redo());
        assert!(!rom.image().is_modified());
        assert_eq!(rom.image().bytes(), fresh.image().original());
        assert_eq!(rom.image().current_hash(), fresh.image().original_hash());
        // A clone is read-only too: the state is part of the value.
        let mut clone = rom.clone();
        assert_eq!(clone.edit().err(), Some(why));
    }

    #[test]
    fn a_mismatched_image_is_one_error_and_opens_read_only() {
        let set = set();
        for case in MISMATCHES {
            let bytes = bytes(&case);
            let loaded = open_like_the_editor(&set, &bytes, case.xmlid);

            // One Error, and it withholds the ROM.
            assert!(loaded.has_errors(), "{}", case.label);
            assert_eq!(loaded.error_count(), 1, "{}", case.label);
            assert_eq!(loaded.diagnostics().len(), 1, "{}", case.label);
            assert_size_error(&loaded.diagnostics()[0], &case);
            assert!(loaded.value().is_none(), "{}", case.label);
            let rejected = open_like_the_editor(&set, &bytes, case.xmlid)
                .into_result()
                .unwrap_err();
            assert_eq!(rejected.error_count(), 1, "{}", case.label);

            // Accepted anyway: read-only, unchanged.
            let (mut rom, diags) = loaded.accept_errors();
            assert_eq!(diags.len(), 1);
            let why = EditingDisabled::SizeMismatch {
                declared: case.declared,
                actual: case.len,
            };
            let fresh = Journal::new(Image::new(bytes.clone()));
            assert_read_only_and_unchanged(&mut rom, why, &fresh);
            let (mut rom, _) = rejected.accept_errors();
            assert_read_only_and_unchanged(&mut rom, why, &fresh);

            // Still readable: every byte, and the table through the
            // definition.
            assert_eq!(rom.image().bytes(), &bytes[..], "{}", case.label);
            assert_eq!(rom.image().len(), case.len);
            assert_eq!(rom.definition().xmlid(), case.xmlid);
            assert_eq!(rev_limit(&rom), TABLE_BYTES, "{}", case.label);
        }
    }

    #[test]
    fn the_exact_size_opens_editable_with_no_diagnostic() {
        let set = set();
        for case in MATCHES {
            let bytes = bytes(&case);
            let loaded = open_like_the_editor(&set, &bytes, case.xmlid);
            assert!(loaded.diagnostics().is_empty(), "{}", case.label);
            let (mut rom, _) = loaded.into_result().unwrap();
            assert!(rom.is_editable());
            assert_eq!(rom.editing_disabled(), None);
            assert_eq!(rev_limit(&rom), TABLE_BYTES);

            let patch =
                Patch::replacing(rom.image(), TABLE_AT, vec![0x0B, 0xB8], Origin::Edit).unwrap();
            rom.edit().unwrap().apply(patch).unwrap();
            assert_eq!(rev_limit(&rom), [0x0B, 0xB8], "{}", case.label);
            assert_eq!(rom.journal().applied().len(), 1);
        }
    }

    #[test]
    fn a_restored_history_over_a_mismatched_image_cannot_be_replayed() {
        // A journal with an applied and an undone edit, as a sidecar
        // restores it, over a 1023-byte image for the 1kb definition.
        let case = MISMATCHES[0];
        let bytes = bytes(&case);
        let mut journal = Journal::new(Image::new(bytes.clone()));
        journal
            .apply(Patch::replacing(journal.image(), 0x200, vec![1], Origin::Edit).unwrap())
            .unwrap();
        journal
            .apply(Patch::replacing(journal.image(), 0x201, vec![2], Origin::Edit).unwrap())
            .unwrap();
        journal.undo().unwrap();
        let before = journal.clone();

        let set = set();
        let index = RomIdIndex::new(&set);
        let candidate = index.lookup(&bytes).unique().unwrap();
        let definition: Definition = DefinitionResolver::new(&set)
            .resolve(candidate.rom())
            .into_result()
            .unwrap()
            .0
            .unwrap();
        let loaded = OpenRom::open(definition, journal, IMAGE);
        assert_eq!(loaded.error_count(), 1);
        assert_size_error(&loaded.diagnostics()[0], &case);
        let (mut rom, _) = loaded.accept_errors();
        // No undo, redo or new edit: the history stays as restored.
        assert_eq!(
            rom.edit().err(),
            Some(EditingDisabled::SizeMismatch {
                declared: 1024,
                actual: 1023,
            })
        );
        assert_eq!(rom.journal(), &before);
        assert!(rom.journal().can_undo());
        assert!(rom.journal().can_redo());
        assert_eq!(rom.image().bytes()[0x200], 1);
        assert_eq!(rom.image().bytes()[0x201], bytes[0x201]);
    }

    #[test]
    fn the_corpus_160_kib_against_192kb_case_opens_read_only() {
        // Current behaviour, pending a product decision: no RAM offset, the
        // table read where the definition says, and no editing.
        let case = MISMATCHES[6];
        assert_eq!((case.declared, case.len), (192 * 1024, 160 * 1024));
        let (mut rom, diags) = open_like_the_editor(&set(), &bytes(&case), case.xmlid)
            .into_result()
            .unwrap_err()
            .accept_errors();
        assert_eq!(diags.len(), 1);
        assert_size_error(&diags[0], &case);
        assert!(rom.edit().is_err());
        assert_eq!(rev_limit(&rom), TABLE_BYTES);
    }

    /// The `<filesize>` text of `declared` in `unit` (`b` or `kb`).
    fn filesize(declared: u32, kb: bool) -> String {
        if kb {
            format!("{}kb", declared / 1024)
        } else {
            format!("{declared}b")
        }
    }

    /// The definition `ROM P` declaring `text` as its file size, resolved.
    fn definition_declaring(text: &str) -> Definition {
        let xml = format!(
            "<roms><rom><romid><xmlid>ROM P</xmlid><filesize>{text}</filesize></romid></rom></roms>"
        );
        let (set, read) = DefinitionSet::read_all([("defs.xml", xml.as_bytes())])
            .into_result()
            .unwrap();
        assert!(read.is_empty(), "{read:?}");
        DefinitionResolver::new(&set)
            .resolve(set.find("rom p").unwrap())
            .into_result()
            .unwrap()
            .0
            .unwrap()
    }

    /// A declared size (in bytes, a whole KiB when `kb`) and an image
    /// length: near each other half the time, so one-off sizes and exact
    /// matches are common; otherwise anywhere in range.
    fn sizes() -> impl Strategy<Value = (u32, bool, usize)> {
        (any::<bool>(), 0u32..=16).prop_flat_map(|(kb, kib)| {
            let declared = if kb {
                Just(kib * 1024).boxed()
            } else {
                (0u32..=16 * 1024).boxed()
            };
            declared.prop_flat_map(move |declared| {
                let near = (-2i64..=2)
                    .prop_map(move |d| usize::try_from((i64::from(declared) + d).max(0)).unwrap());
                let anywhere = 0usize..=17 * 1024;
                (Just(declared), Just(kb), prop_oneof![near, anywhere])
            })
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        /// For any declared size and image length: a mismatch is exactly
        /// one `DEF0029` Error and a read-only ROM that nothing changes;
        /// equal sizes are no diagnostic and an editable ROM.
        #[test]
        fn mismatch_iff_error_and_editing_disabled((declared, kb, len) in sizes()) {
            let definition = definition_declaring(&filesize(declared, kb));
            prop_assert_eq!(definition.romid().file_size(), Some(declared));
            let bytes: Vec<u8> = (0..len).map(|i| u8::try_from(i % 253).unwrap()).collect();
            let loaded = OpenRom::open(definition, Image::new(bytes.clone()), IMAGE);
            let size_errors = loaded
                .diagnostics()
                .iter()
                .filter(|d| d.code() == codes::ROM_SIZE_MISMATCH)
                .count();
            let fresh = Journal::new(Image::new(bytes));

            if usize::try_from(declared).unwrap() == len {
                prop_assert!(loaded.diagnostics().is_empty());
                let (mut rom, _) = loaded.into_result().unwrap();
                prop_assert!(rom.is_editable());
                prop_assert_eq!(rom.editing_disabled(), None);
                prop_assert!(rom.edit().is_ok());
                prop_assert_eq!(rom.journal(), &fresh);
            } else {
                prop_assert_eq!(size_errors, 1);
                prop_assert_eq!(loaded.diagnostics().len(), 1);
                prop_assert_eq!(loaded.error_count(), 1);
                prop_assert!(loaded.value().is_none());
                let (mut rom, _) = loaded.accept_errors();
                let why = EditingDisabled::SizeMismatch { declared, actual: len };
                prop_assert!(!rom.is_editable());
                prop_assert_eq!(rom.editing_disabled(), Some(why));
                prop_assert_eq!(rom.edit().err(), Some(why));
                prop_assert_eq!(rom.journal(), &fresh);
            }
        }
    }

    /// Every case as the snapshot shows it: the definition, the image
    /// size, the diagnostics of opening it, and whether it is editable.
    fn render(set: &DefinitionSet, case: &Case) -> String {
        let mut out = String::new();
        writeln!(
            out,
            "=== {}: `{}` declares {} bytes, image is {} bytes",
            case.label, case.xmlid, case.declared, case.len
        )
        .unwrap();
        let (mut rom, diags) = open_like_the_editor(set, &bytes(case), case.xmlid).accept_errors();
        for d in &diags {
            writeln!(out, "{d}").unwrap();
        }
        match rom.edit() {
            Ok(_) => writeln!(out, "editable").unwrap(),
            Err(why) => writeln!(out, "read-only: {why}").unwrap(),
        }
        out.push('\n');
        out
    }

    #[test]
    fn size_mismatch_snapshot() {
        let set = set();
        let rendered = MATCHES[..1]
            .iter()
            .chain(&MISMATCHES)
            .map(|c| render(&set, c))
            .collect::<String>();
        rr_testkit::snapshot::settings()
            .bind(|| insta::assert_snapshot!("rom_size_mismatch", rendered));
    }
}
