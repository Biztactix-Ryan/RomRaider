use core::error::Error as _;

use proptest::prelude::*;

use super::{
    ChecksumFailure, ChecksumOverride, EncodeError, FORMAT, MAX_CHECKSUM_NAME_LEN,
    MAX_CHECKSUM_VALUE_LEN, MAX_CONFIRMED_AT_UNIX, MAX_SIDECAR_LEN, OverrideError, RecordError,
    Restored, SCHEMA, SidecarError, decode, decode_within, encode, encode_within, from_hex,
};
use crate::image::{ApplyError, Image, ImageHash};
use crate::journal::{Journal, JournalError, Origin, Patch, PatchError};

const BASE: [u8; 8] = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77];

fn patch(at: usize, old: &[u8], new: &[u8], origin: Origin) -> Patch {
    match Patch::new(at, old, new, origin) {
        Ok(p) => p,
        Err(e) => panic!("test patch is well-formed: {e}"),
    }
}

/// A journal over [`BASE`] with two applied groups and one undone group.
fn sample_journal() -> Journal {
    let mut j = Journal::new(Image::new(BASE.to_vec()));
    let steps: [Vec<Patch>; 3] = [
        vec![
            patch(0, &[0x00], &[0xA0], Origin::Edit),
            patch(2, &[0x22, 0x33], &[0xB2, 0xB3], Origin::Edit),
        ],
        vec![patch(7, &[0x77], &[0xC7], Origin::ChecksumFix)],
        vec![patch(4, &[0x44], &[0xD4], Origin::Paste)],
    ];
    for group in steps {
        if let Err(e) = j.apply_group(group) {
            panic!("sample group applies: {e}");
        }
    }
    if let Err(e) = j.undo() {
        panic!("sample undo: {e}");
    }
    j
}

fn failure(name: &str, stored: &[u8], computed: &[u8]) -> ChecksumFailure {
    match ChecksumFailure::new(name, stored, computed) {
        Ok(f) => f,
        Err(e) => panic!("test failure is valid: {e}"),
    }
}

fn sample_override() -> ChecksumOverride {
    let result = ChecksumOverride::new(
        ImageHash::of(b"saved"),
        1_792_000_000,
        [
            failure("subaru-sum32", &[1, 2, 3, 4], &[5, 6, 7, 8]),
            failure("denso 16/2", &[0xAB, 0xCD], &[0xEF, 0x01]),
        ],
    );
    match result {
        Ok(o) => o,
        Err(e) => panic!("sample override is valid: {e}"),
    }
}

fn enc(journal: &Journal, overrides: &[ChecksumOverride]) -> String {
    match encode(journal, overrides) {
        Ok(text) => text,
        Err(e) => panic!("encode: {e}"),
    }
}

fn dec(text: &str, base: &[u8]) -> Restored {
    match decode(text, Image::new(base.to_vec())) {
        Ok(r) => r,
        Err(e) => panic!("decode: {e}\n{text}"),
    }
}

fn dec_err(text: &str, base: &[u8]) -> SidecarError {
    match decode(text, Image::new(base.to_vec())) {
        Ok(r) => panic!("decode accepted: {r:?}\n{text}"),
        Err(e) => e,
    }
}

/// The header of a valid sidecar over `base` with no history.
fn header(base: &[u8]) -> String {
    let hash = ImageHash::of(base);
    format!(
        "format = \"{FORMAT}\"\nschema = {SCHEMA}\nbase_sha256 = \"{hash}\"\nhead_sha256 = \"{hash}\"\n"
    )
}

// --- Writing -----------------------------------------------------------------

#[test]
fn encode_writes_the_documented_layout() {
    let j = sample_journal();
    let text = enc(&j, &[sample_override()]);
    let base = ImageHash::of(&BASE);
    let head = j.image().current_hash();
    let saved = ImageHash::of(b"saved");
    let expected = format!(
        "# RomRaider sidecar journal: the edit history of the ROM beside it.\n\
         format = \"rrjournal\"\n\
         schema = 1\n\
         base_sha256 = \"{base}\"\n\
         head_sha256 = \"{head}\"\n\
         \n\
         [[applied]]\n\
         [[applied.patch]]\n\
         at = 0x00000000\n\
         origin = \"edit\"\n\
         old = \"00\"\n\
         new = \"a0\"\n\
         [[applied.patch]]\n\
         at = 0x00000002\n\
         origin = \"edit\"\n\
         old = \"2233\"\n\
         new = \"b2b3\"\n\
         \n\
         [[applied]]\n\
         [[applied.patch]]\n\
         at = 0x00000007\n\
         origin = \"checksum_fix\"\n\
         old = \"77\"\n\
         new = \"c7\"\n\
         \n\
         [[undone]]\n\
         [[undone.patch]]\n\
         at = 0x00000004\n\
         origin = \"paste\"\n\
         old = \"44\"\n\
         new = \"d4\"\n\
         \n\
         [[checksum_override]]\n\
         image_sha256 = \"{saved}\"\n\
         confirmed_at_unix = 1792000000\n\
         [[checksum_override.failure]]\n\
         checksum = \"subaru-sum32\"\n\
         stored = \"01020304\"\n\
         computed = \"05060708\"\n\
         [[checksum_override.failure]]\n\
         checksum = \"denso 16/2\"\n\
         stored = \"abcd\"\n\
         computed = \"ef01\"\n"
    );
    assert_eq!(text, expected);
}

#[test]
fn every_origin_round_trips() {
    for origin in [
        Origin::Edit,
        Origin::Paste,
        Origin::ChecksumFix,
        Origin::ApplyDiff,
    ] {
        let mut j = Journal::new(Image::new(BASE.to_vec()));
        if let Err(e) = j.apply(patch(1, &[0x11], &[0xEE], origin)) {
            panic!("{e}");
        }
        let restored = dec(&enc(&j, &[]), &BASE);
        assert_eq!(restored.journal, j, "{origin}");
    }
}

#[test]
fn round_trip_restores_history_cursor_and_overrides() {
    let j = sample_journal();
    let overrides = [sample_override()];
    let text = enc(&j, &overrides);
    let mut restored = dec(&text, &BASE);
    assert_eq!(restored.journal, j);
    assert_eq!(restored.overrides, overrides);
    assert_eq!(restored.journal.applied().len(), 2);
    assert_eq!(restored.journal.undone().len(), 1);
    // Byte-identical when written again.
    assert_eq!(enc(&restored.journal, &restored.overrides), text);
    // Undo and redo carry on where they stopped.
    if let Err(e) = restored.journal.redo() {
        panic!("{e}");
    }
    assert_eq!(
        restored.journal.image().bytes(),
        &[0xA0, 0x11, 0xB2, 0xB3, 0xD4, 0x55, 0x66, 0xC7]
    );
    if let Err(e) = restored.journal.undo_all() {
        panic!("{e}");
    }
    assert_eq!(restored.journal.image().bytes(), &BASE);
}

#[test]
fn empty_journal_round_trips() {
    let j = Journal::new(Image::new(BASE.to_vec()));
    let text = enc(&j, &[]);
    assert_eq!(
        text,
        format!(
            "# RomRaider sidecar journal: the edit history of the ROM beside it.\n{}",
            header(&BASE)
        )
    );
    let restored = dec(&text, &BASE);
    assert_eq!(restored.journal, j);
    assert!(restored.overrides.is_empty());
}

#[test]
fn empty_image_and_empty_patch_round_trip() {
    let mut j = Journal::new(Image::new(Vec::new()));
    if let Err(e) = j.apply(patch(0, &[], &[], Origin::Edit)) {
        panic!("{e}");
    }
    assert_eq!(dec(&enc(&j, &[]), &[]).journal, j);
}

#[test]
fn encode_refuses_text_over_the_limit() {
    let j = sample_journal();
    let len = enc(&j, &[]).len();
    assert_eq!(
        encode_within(&j, &[], len).map(|t| t.len()),
        Ok(len),
        "exactly at the limit"
    );
    assert_eq!(
        encode_within(&j, &[], len - 1),
        Err(EncodeError::TooLarge { len, max: len - 1 })
    );
    let msg = EncodeError::TooLarge { len: 9, max: 8 }.to_string();
    assert!(
        msg.contains("9 bytes") && msg.contains("8-byte limit"),
        "{msg}"
    );
    assert_eq!(MAX_SIDECAR_LEN, 32 * 1024 * 1024);
}

// --- Base image binding --------------------------------------------------------

#[test]
fn a_different_base_image_is_refused_before_anything_is_applied() {
    let j = sample_journal();
    let text = enc(&j, &[]);
    let mut other = BASE;
    other[5] ^= 1;
    let err = dec_err(&text, &other);
    assert_eq!(
        err,
        SidecarError::BaseMismatch {
            expected: ImageHash::of(&BASE),
            found: ImageHash::of(&other),
        }
    );
    let msg = err.to_string();
    assert!(
        msg.contains(&ImageHash::of(&BASE).to_string())
            && msg.contains(&ImageHash::of(&other).to_string())
            && msg.contains("nothing was applied"),
        "{msg}"
    );
    // Even an image that holds the journal's head bytes is not its base.
    let head = j.image().bytes().to_vec();
    assert!(matches!(
        dec_err(&text, &head),
        SidecarError::BaseMismatch { .. }
    ));
}

#[test]
fn a_modified_image_is_refused() {
    let j = sample_journal();
    let text = enc(&j, &[]);
    // Same original bytes, but carrying the journal's changes.
    let modified = sample_journal().into_image();
    assert_eq!(decode(&text, modified), Err(SidecarError::ImageModified));
    assert!(
        SidecarError::ImageModified
            .to_string()
            .contains("as loaded")
    );
    // The unmodified base is accepted.
    assert_eq!(dec(&text, &BASE).journal, j);
}

#[test]
fn a_tampered_head_hash_is_refused() {
    let j = sample_journal();
    let text = enc(&j, &[]);
    let head = j.image().current_hash();
    let wrong = ImageHash::of(b"elsewhere");
    let tampered = text.replace(
        &format!("head_sha256 = \"{head}\""),
        &format!("head_sha256 = \"{wrong}\""),
    );
    let err = dec_err(&tampered, &BASE);
    assert_eq!(
        err,
        SidecarError::HeadMismatch {
            expected: wrong,
            found: head,
        }
    );
    assert!(err.to_string().contains(&wrong.to_string()), "{err}");
    // Recording the base hash as head (as if nothing were applied) fails too.
    let base = ImageHash::of(&BASE);
    let tampered = text.replace(
        &format!("head_sha256 = \"{head}\""),
        &format!("head_sha256 = \"{base}\""),
    );
    assert!(matches!(
        dec_err(&tampered, &BASE),
        SidecarError::HeadMismatch { .. }
    ));
}

// --- Envelope: size, syntax, format, schema, shape -----------------------------

#[test]
fn text_over_the_limit_is_refused_before_parsing() {
    let text = header(&BASE);
    let len = text.len();
    let at_limit = decode_within(&text, Image::new(BASE.to_vec()), len);
    assert!(at_limit.is_ok(), "{at_limit:?}");
    assert_eq!(
        decode_within(&text, Image::new(BASE.to_vec()), len - 1),
        Err(SidecarError::TooLarge { len, max: len - 1 })
    );
    // Not even parsed: garbage over the limit reports its size.
    assert_eq!(
        decode_within("\u{0}\u{0}\u{0}", Image::new(Vec::new()), 2),
        Err(SidecarError::TooLarge { len: 3, max: 2 })
    );
    let msg = SidecarError::TooLarge { len: 3, max: 2 }.to_string();
    assert!(
        msg.contains("3 bytes") && msg.contains("2-byte limit"),
        "{msg}"
    );
}

#[test]
fn the_public_limit_is_max_sidecar_len() {
    let mut text = header(&BASE);
    text.push('#');
    text.push_str(&"x".repeat(MAX_SIDECAR_LEN - text.len() - 1));
    text.push('\n');
    assert_eq!(text.len(), MAX_SIDECAR_LEN);
    assert!(decode(&text, Image::new(BASE.to_vec())).is_ok());
    text.push('\n');
    assert_eq!(
        decode(&text, Image::new(BASE.to_vec())),
        Err(SidecarError::TooLarge {
            len: MAX_SIDECAR_LEN + 1,
            max: MAX_SIDECAR_LEN,
        })
    );
}

#[test]
fn invalid_toml_is_a_syntax_error() {
    let err = dec_err("format = \"rrjournal\nschema = 1", &BASE);
    assert!(matches!(err, SidecarError::Syntax { .. }), "{err:?}");
    assert!(
        err.to_string()
            .starts_with("sidecar journal is not valid TOML: "),
        "{err}"
    );
}

#[test]
fn deep_nesting_is_refused_without_overflowing_the_stack() {
    let depth = 100_000;
    let text = format!("x = {}{}", "[".repeat(depth), "]".repeat(depth));
    assert!(matches!(dec_err(&text, &BASE), SidecarError::Syntax { .. }));
    let text = format!("x = {}{}", "{a = ".repeat(depth), "}".repeat(depth));
    assert!(matches!(dec_err(&text, &BASE), SidecarError::Syntax { .. }));
}

#[test]
fn a_file_without_the_format_key_is_not_a_journal() {
    let ok = header(&BASE);
    for text in [
        ok.replace("format = \"rrjournal\"\n", ""),
        ok.replace("\"rrjournal\"", "\"rrtrace\""),
        ok.replace("\"rrjournal\"", "1"),
        String::new(),
    ] {
        assert_eq!(dec_err(&text, &BASE), SidecarError::NotAJournal, "{text}");
    }
    assert!(
        SidecarError::NotAJournal
            .to_string()
            .contains("format = \"rrjournal\"")
    );
}

#[test]
fn a_missing_or_unknown_schema_is_refused() {
    let ok = header(&BASE);
    assert_eq!(
        dec_err(&ok.replace("schema = 1\n", ""), &BASE),
        SidecarError::MissingSchema { supported: 1 }
    );
    for (value, shown) in [("2", "2"), ("0", "0"), ("\"1\"", "\"1\""), ("1.0", "1.0")] {
        let err = dec_err(
            &ok.replace("schema = 1", &format!("schema = {value}")),
            &BASE,
        );
        assert_eq!(
            err,
            SidecarError::UnsupportedSchema {
                found: shown.to_owned(),
                supported: 1,
            }
        );
    }
    let msg = SidecarError::MissingSchema { supported: 1 }.to_string();
    assert!(
        msg.contains("no top-level `schema`") && msg.contains("supported schema: 1"),
        "{msg}"
    );
    let msg = SidecarError::UnsupportedSchema {
        found: "2".into(),
        supported: 1,
    }
    .to_string();
    assert!(
        msg.contains("unsupported schema 2") && msg.contains("supported schema: 1"),
        "{msg}"
    );
}

#[test]
fn unknown_missing_and_mistyped_keys_are_shape_errors() {
    let ok = header(&BASE);
    let cases = [
        format!("{ok}comment = \"hi\"\n"),
        ok.replace("head_sha256", "tail_sha256"),
        ok.replace(&format!("base_sha256 = \"{}\"\n", ImageHash::of(&BASE)), ""),
        format!(
            "{ok}[[applied]]\nextra = 1\n[[applied.patch]]\nat = 0\norigin = \"edit\"\nold = \"00\"\nnew = \"00\"\n"
        ),
        format!(
            "{ok}[[applied]]\n[[applied.patch]]\nat = 0\norigin = \"edit\"\nold = \"00\"\nnew = \"00\"\nnote = 1\n"
        ),
        format!(
            "{ok}[[applied]]\n[[applied.patch]]\nat = -1\norigin = \"edit\"\nold = \"00\"\nnew = \"00\"\n"
        ),
        format!(
            "{ok}[[applied]]\n[[applied.patch]]\nat = 0\norigin = \"magic\"\nold = \"00\"\nnew = \"00\"\n"
        ),
        format!("{ok}[[applied]]\n[[applied.patch]]\nat = 0\nold = \"00\"\nnew = \"00\"\n"),
        format!(
            "{ok}[[applied]]\n[[applied.patch]]\nat = 0\norigin = \"edit\"\nold = 0\nnew = \"00\"\n"
        ),
        format!(
            "{ok}[[checksum_override]]\nimage_sha256 = \"{}\"\nconfirmed_at_unix = -5\n",
            ImageHash::of(b"x")
        ),
        format!(
            "{ok}[[checksum_override]]\nimage_sha256 = \"{}\"\n",
            ImageHash::of(b"x")
        ),
    ];
    for text in &cases {
        let err = dec_err(text, &BASE);
        assert!(matches!(err, SidecarError::Shape { .. }), "{err:?}\n{text}");
        assert!(
            err.to_string()
                .starts_with("sidecar journal does not have the expected keys: "),
            "{err}"
        );
    }
}

// --- Records ---------------------------------------------------------------------

fn one_patch(ok: &str, at: &str, old: &str, new: &str) -> String {
    format!(
        "{ok}[[applied]]\n[[applied.patch]]\nat = {at}\norigin = \"edit\"\nold = \"{old}\"\nnew = \"{new}\"\n"
    )
}

#[test]
fn malformed_hashes_are_named() {
    let ok = header(&BASE);
    let base = ImageHash::of(&BASE).to_string();
    for bad in [
        "",
        "00",
        &base[..63],
        &format!("{base}00"),
        &format!("{}g", &base[..63]),
    ] {
        let text = ok.replacen(&base, bad, 1);
        assert_eq!(
            dec_err(&text, &BASE),
            SidecarError::Invalid {
                record: "base_sha256".into(),
                error: RecordError::NotSha256,
            },
            "{bad:?}"
        );
        let text = ok.replacen(
            &format!("head_sha256 = \"{base}\""),
            &format!("head_sha256 = \"{bad}\""),
            1,
        );
        assert_eq!(
            dec_err(&text, &BASE),
            SidecarError::Invalid {
                record: "head_sha256".into(),
                error: RecordError::NotSha256,
            }
        );
    }
    // Upper-case digits are read.
    let restored = dec(&ok.replace(&base, &base.to_uppercase()), &BASE);
    assert_eq!(restored.journal, Journal::new(Image::new(BASE.to_vec())));
}

#[test]
fn malformed_bytes_are_named() {
    let ok = header(&BASE);
    for (old, new, record) in [
        ("0", "00", "applied[0].patch[0].old"),
        ("0g", "00", "applied[0].patch[0].old"),
        ("+1", "00", "applied[0].patch[0].old"),
        ("00", "000", "applied[0].patch[0].new"),
        ("00", "-1", "applied[0].patch[0].new"),
        ("00", "é0", "applied[0].patch[0].new"),
    ] {
        let err = dec_err(&one_patch(&ok, "0", old, new), &BASE);
        assert_eq!(
            err,
            SidecarError::Invalid {
                record: record.into(),
                error: RecordError::NotHex,
            },
            "{old:?} {new:?}"
        );
    }
    let err = SidecarError::Invalid {
        record: "applied[0].patch[0].old".into(),
        error: RecordError::NotHex,
    };
    assert_eq!(
        err.to_string(),
        "sidecar journal record `applied[0].patch[0].old` is invalid: not an even number of hex digits"
    );
    assert!(
        err.source()
            .is_some_and(|s| s.to_string() == RecordError::NotHex.to_string())
    );
}

#[test]
fn a_patch_that_resizes_the_image_is_refused() {
    let err = dec_err(&one_patch(&header(&BASE), "0", "00", "aabb"), &BASE);
    assert_eq!(
        err,
        SidecarError::Invalid {
            record: "applied[0].patch[0]".into(),
            error: RecordError::Patch(PatchError::LengthMismatch { old: 1, new: 2 }),
        }
    );
    assert!(err.to_string().contains("may not resize"), "{err}");
}

#[test]
fn an_empty_group_is_refused() {
    let ok = header(&BASE);
    for (text, record) in [
        (format!("{ok}[[applied]]\n"), "applied[0]"),
        (
            format!(
                "{ok}[[undone]]\n[[undone.patch]]\nat = 0\norigin = \"edit\"\nold = \"00\"\nnew = \"01\"\n[[undone]]\n"
            ),
            "undone[1]",
        ),
    ] {
        assert_eq!(
            dec_err(&text, &BASE),
            SidecarError::Invalid {
                record: record.into(),
                error: RecordError::EmptyGroup,
            }
        );
    }
}

#[test]
fn hex_offsets_and_upper_case_bytes_are_read() {
    let ok = header(&BASE).replace(
        &format!("head_sha256 = \"{}\"", ImageHash::of(&BASE)),
        &format!(
            "head_sha256 = \"{}\"",
            ImageHash::of(&[0x00, 0x11, 0x22, 0xAB, 0xCD, 0x55, 0x66, 0x77])
        ),
    );
    let restored = dec(&one_patch(&ok, "0x3", "3344", "ABcd"), &BASE);
    assert_eq!(
        restored.journal.applied()[0].patches(),
        &[patch(3, &[0x33, 0x44], &[0xAB, 0xCD], Origin::Edit)]
    );
}

// --- Replay ----------------------------------------------------------------------

#[test]
fn a_stale_applied_patch_is_refused() {
    let err = dec_err(&one_patch(&header(&BASE), "1", "99", "00"), &BASE);
    assert_eq!(
        err,
        SidecarError::Replay {
            record: "applied[0]".into(),
            error: JournalError::Refused {
                index: 0,
                error: ApplyError::Stale {
                    offset: 1,
                    expected: 0x99,
                    found: 0x11,
                },
            },
        }
    );
    assert!(
        err.to_string()
            .starts_with("sidecar journal group `applied[0]` does not apply"),
        "{err}"
    );
    assert!(err.source().is_some());
}

#[test]
fn a_patch_past_the_end_of_the_image_is_refused() {
    let err = dec_err(&one_patch(&header(&BASE), "7", "7700", "0000"), &BASE);
    assert_eq!(
        err,
        SidecarError::Replay {
            record: "applied[0]".into(),
            error: JournalError::Refused {
                index: 0,
                error: ApplyError::OutOfBounds {
                    at: 7,
                    len: 2,
                    image_len: 8,
                },
            },
        }
    );
    let huge = format!("{:#x}", i64::MAX);
    assert!(matches!(
        dec_err(&one_patch(&header(&BASE), &huge, "00", "00"), &BASE),
        SidecarError::Replay { .. }
    ));
}

#[test]
fn undone_groups_must_follow_the_applied_ones() {
    let j = sample_journal();
    let text = enc(&j, &[]);
    // The undone group expects 0x44 at offset 4; make it expect 0x45.
    let stale = text.replace("old = \"44\"", "old = \"45\"");
    assert_eq!(
        dec_err(&stale, &BASE),
        SidecarError::Replay {
            record: "undone[0]".into(),
            error: JournalError::Refused {
                index: 0,
                error: ApplyError::Stale {
                    offset: 4,
                    expected: 0x45,
                    found: 0x44,
                },
            },
        }
    );
    // A stale byte in an applied group is named too.
    let stale = text.replace("old = \"2233\"", "old = \"2234\"");
    assert!(matches!(
        dec_err(&stale, &BASE),
        SidecarError::Replay { ref record, .. } if record == "applied[0]"
    ));
}

#[test]
fn a_group_is_checked_against_the_groups_before_it() {
    // Two applied groups at the same byte: the second must expect what the
    // first wrote.
    let mut j = Journal::new(Image::new(BASE.to_vec()));
    for (old, new) in [(0x11, 0x21), (0x21, 0x31)] {
        if let Err(e) = j.apply(patch(1, &[old], &[new], Origin::Edit)) {
            panic!("{e}");
        }
    }
    let text = enc(&j, &[]);
    assert_eq!(dec(&text, &BASE).journal, j);
    let stale = text.replacen("old = \"21\"", "old = \"11\"", 1);
    assert!(matches!(
        dec_err(&stale, &BASE),
        SidecarError::Replay { ref record, .. } if record == "applied[1]"
    ));
}

// --- Checksum overrides ------------------------------------------------------------

#[test]
fn checksum_override_parts_are_kept() {
    let o = sample_override();
    assert_eq!(o.image(), ImageHash::of(b"saved"));
    assert_eq!(o.confirmed_at_unix(), 1_792_000_000);
    assert_eq!(o.failures().len(), 2);
    let f = &o.failures()[1];
    assert_eq!(f.checksum(), "denso 16/2");
    assert_eq!(f.stored(), &[0xAB, 0xCD]);
    assert_eq!(f.computed(), &[0xEF, 0x01]);
}

#[test]
fn checksum_override_needs_a_failure_and_a_time_in_range() {
    let image = ImageHash::of(b"x");
    let one = || [failure("sum", &[1], &[2])];
    assert_eq!(
        ChecksumOverride::new(image, 0, []),
        Err(OverrideError::NoFailures)
    );
    assert!(ChecksumOverride::new(image, MAX_CONFIRMED_AT_UNIX, one()).is_ok());
    assert_eq!(
        ChecksumOverride::new(image, MAX_CONFIRMED_AT_UNIX + 1, one()),
        Err(OverrideError::TimeOutOfRange {
            secs: MAX_CONFIRMED_AT_UNIX + 1,
        })
    );
    assert_eq!(MAX_CONFIRMED_AT_UNIX, 9_223_372_036_854_775_807);
    // The latest time round-trips through TOML.
    let o = match ChecksumOverride::new(image, MAX_CONFIRMED_AT_UNIX, one()) {
        Ok(o) => o,
        Err(e) => panic!("{e}"),
    };
    let j = Journal::new(Image::new(BASE.to_vec()));
    assert_eq!(
        dec(&enc(&j, core::slice::from_ref(&o)), &BASE).overrides,
        [o]
    );
}

#[test]
fn checksum_names_are_printable_ascii_without_quotes() {
    let ok = |name: &str| ChecksumFailure::new(name, [1u8].as_slice(), [2u8].as_slice()).is_ok();
    assert!(ok("a"));
    assert!(ok("Subaru DBW 16-bit sum/xor: v1.0"));
    assert!(ok(&"n".repeat(MAX_CHECKSUM_NAME_LEN)));
    assert!(ok("~!#$%&'()*+,-./:;<=>?@[]^_`{|}"));
    for bad in [
        String::new(),
        "n".repeat(MAX_CHECKSUM_NAME_LEN + 1),
        "a\"b".into(),
        "a\\b".into(),
        "a\nb".into(),
        "a\tb".into(),
        "\u{7f}".into(),
        "é".into(),
    ] {
        assert_eq!(
            ChecksumFailure::new(bad.clone(), [1u8].as_slice(), [2u8].as_slice()),
            Err(OverrideError::ChecksumName { name: bad.clone() }),
            "{bad:?}"
        );
    }
}

#[test]
fn checksum_values_are_equal_length_and_bounded() {
    let new = |stored: usize, computed: usize| {
        ChecksumFailure::new("sum", vec![0; stored], vec![1; computed])
    };
    assert!(new(1, 1).is_ok());
    assert!(new(MAX_CHECKSUM_VALUE_LEN, MAX_CHECKSUM_VALUE_LEN).is_ok());
    for (stored, computed) in [
        (0, 0),
        (2, 4),
        (4, 2),
        (MAX_CHECKSUM_VALUE_LEN + 1, MAX_CHECKSUM_VALUE_LEN + 1),
    ] {
        assert_eq!(
            new(stored, computed),
            Err(OverrideError::ValueLength { stored, computed }),
            "{stored} {computed}"
        );
    }
}

#[test]
fn override_error_messages() {
    let msg = OverrideError::NoFailures.to_string();
    assert!(msg.contains("no failing checksum"), "{msg}");
    let msg = OverrideError::TimeOutOfRange { secs: 7 }.to_string();
    assert!(
        msg.contains("time 7") && msg.contains(&MAX_CONFIRMED_AT_UNIX.to_string()),
        "{msg}"
    );
    let msg = OverrideError::ChecksumName {
        name: "a\"b".into(),
    }
    .to_string();
    assert!(
        msg.contains(r#""a\"b""#) && msg.contains("64 printable"),
        "{msg}"
    );
    let msg = OverrideError::ValueLength {
        stored: 2,
        computed: 4,
    }
    .to_string();
    assert!(
        msg.contains("2 (stored)") && msg.contains("4 (computed)") && msg.contains("1 to 32"),
        "{msg}"
    );
}

#[test]
fn invalid_override_records_are_named() {
    let ok = header(&BASE);
    let image = ImageHash::of(b"x");
    let override_text = |image: &str, failures: &str| {
        format!(
            "{ok}[[checksum_override]]\nimage_sha256 = \"{image}\"\nconfirmed_at_unix = 5\n{failures}"
        )
    };
    let failure_text = |name: &str, stored: &str, computed: &str| {
        format!(
            "[[checksum_override.failure]]\nchecksum = \"{name}\"\nstored = \"{stored}\"\ncomputed = \"{computed}\"\n"
        )
    };
    let good = failure_text("sum", "01", "02");
    let cases = [
        (
            override_text("00", &good),
            "checksum_override[0].image_sha256",
            RecordError::NotSha256,
        ),
        (
            override_text(&image.to_string(), ""),
            "checksum_override[0]",
            RecordError::Override(OverrideError::NoFailures),
        ),
        (
            override_text(
                &image.to_string(),
                &format!("{good}{}", failure_text("sum", "0", "02")),
            ),
            "checksum_override[0].failure[1].stored",
            RecordError::NotHex,
        ),
        (
            override_text(&image.to_string(), &failure_text("sum", "01", "x")),
            "checksum_override[0].failure[0].computed",
            RecordError::NotHex,
        ),
        (
            override_text(&image.to_string(), &failure_text("sum", "01", "0203")),
            "checksum_override[0].failure[0]",
            RecordError::Override(OverrideError::ValueLength {
                stored: 1,
                computed: 2,
            }),
        ),
        (
            override_text(&image.to_string(), &failure_text("", "01", "02")),
            "checksum_override[0].failure[0]",
            RecordError::Override(OverrideError::ChecksumName {
                name: String::new(),
            }),
        ),
    ];
    for (text, record, error) in cases {
        assert_eq!(
            dec_err(&text, &BASE),
            SidecarError::Invalid {
                record: record.into(),
                error,
            },
            "{text}"
        );
    }
    // A TOML escape cannot smuggle in a character the name may not hold.
    let text = override_text(&image.to_string(), &failure_text("a\\u0022b", "01", "02"));
    assert!(matches!(
        dec_err(&text, &BASE),
        SidecarError::Invalid {
            error: RecordError::Override(OverrideError::ChecksumName { .. }),
            ..
        }
    ));
}

#[test]
fn record_error_messages_and_sources() {
    assert_eq!(
        RecordError::NotHex.to_string(),
        "not an even number of hex digits"
    );
    assert_eq!(
        RecordError::NotSha256.to_string(),
        "not a SHA-256 of 64 hex digits"
    );
    assert_eq!(
        RecordError::EmptyGroup.to_string(),
        "a group with no patches"
    );
    let patch = PatchError::LengthMismatch { old: 1, new: 2 };
    assert_eq!(RecordError::Patch(patch).to_string(), patch.to_string());
    let over = OverrideError::NoFailures;
    assert_eq!(
        RecordError::Override(over.clone()).to_string(),
        over.to_string()
    );
    assert!(RecordError::NotHex.source().is_none());
    assert!(RecordError::NotSha256.source().is_none());
    assert!(RecordError::EmptyGroup.source().is_none());
    assert_eq!(
        RecordError::Patch(patch).source().map(ToString::to_string),
        Some(patch.to_string())
    );
    assert_eq!(
        RecordError::Override(over.clone())
            .source()
            .map(ToString::to_string),
        Some(over.to_string())
    );
    assert!(SidecarError::NotAJournal.source().is_none());
    let replay = SidecarError::Replay {
        record: "undone[0]".into(),
        error: JournalError::Refused {
            index: 0,
            error: ApplyError::LengthMismatch { old: 1, new: 2 },
        },
    };
    assert!(
        replay
            .source()
            .is_some_and(|s| s.to_string().starts_with("patch 0 refused"))
    );
    let msg = SidecarError::HeadMismatch {
        expected: ImageHash::of(b"a"),
        found: ImageHash::of(b"b"),
    }
    .to_string();
    assert!(
        msg.contains(&ImageHash::of(b"a").to_string())
            && msg.contains(&ImageHash::of(b"b").to_string()),
        "{msg}"
    );
}

// --- Hex --------------------------------------------------------------------------

#[test]
fn from_hex_reads_pairs_of_digits_in_either_case() {
    assert_eq!(from_hex(""), Some(vec![]));
    assert_eq!(from_hex("00ff7Fa0"), Some(vec![0x00, 0xFF, 0x7F, 0xA0]));
    assert_eq!(
        from_hex("0123456789abcdefABCDEF"),
        Some(vec![
            0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0xAB, 0xCD, 0xEF
        ])
    );
    for bad in ["0", "000", "0g", "g0", " 0", "+1", "0x", "é"] {
        assert_eq!(from_hex(bad), None, "{bad:?}");
    }
}

// --- Properties -------------------------------------------------------------------

/// One step of building a journal: a group of `(at, new bytes, origin)`
/// writes, or an undo.
#[derive(Debug, Clone)]
enum Step {
    Group(Vec<(usize, Vec<u8>, u8)>),
    Undo,
}

fn steps() -> impl Strategy<Value = Vec<Step>> {
    let write = (0usize..40, prop::collection::vec(any::<u8>(), 0..6), 0u8..4);
    let step = prop_oneof![
        3 => prop::collection::vec(write, 1..4).prop_map(Step::Group),
        1 => Just(Step::Undo),
    ];
    prop::collection::vec(step, 0..12)
}

fn origin(n: u8) -> Origin {
    match n {
        0 => Origin::Edit,
        1 => Origin::Paste,
        2 => Origin::ChecksumFix,
        _ => Origin::ApplyDiff,
    }
}

/// A journal over `bytes` built by `steps`; writes are clipped to the image.
fn build(bytes: &[u8], steps: &[Step]) -> Journal {
    let mut j = Journal::new(Image::new(bytes.to_vec()));
    for step in steps {
        match step {
            Step::Undo => {
                if let Err(e) = j.undo() {
                    panic!("{e}");
                }
            }
            Step::Group(writes) => {
                let mut scratch = j.image().bytes().to_vec();
                let mut group = Vec::new();
                for (at, new, o) in writes {
                    let at = at % (scratch.len() + 1);
                    let len = new.len().min(scratch.len() - at);
                    let new = &new[..len];
                    let old = scratch[at..at + len].to_vec();
                    scratch[at..at + len].copy_from_slice(new);
                    group.push(patch(at, &old, new, origin(*o)));
                }
                if let Err(e) = j.apply_group(group) {
                    panic!("{e}");
                }
            }
        }
    }
    j
}

fn overrides() -> impl Strategy<Value = Vec<ChecksumOverride>> {
    let one_failure = (
        "[ -!#-\\[\\]-~]{1,64}",
        1usize..=MAX_CHECKSUM_VALUE_LEN,
        any::<u64>(),
    )
        .prop_map(|(name, len, seed)| {
            let stored: Vec<u8> = (0..len).map(|i| (seed >> (i % 8 * 8)) as u8).collect();
            let computed: Vec<u8> = stored.iter().map(|b| !b).collect();
            failure(&name, &stored, &computed)
        });
    let one = (
        any::<[u8; 32]>(),
        0..=MAX_CONFIRMED_AT_UNIX,
        prop::collection::vec(one_failure, 1..4),
    )
        .prop_map(|(hash, secs, failures)| {
            match ChecksumOverride::new(ImageHash::from_bytes(hash), secs, failures) {
                Ok(o) => o,
                Err(e) => panic!("{e}"),
            }
        });
    prop::collection::vec(one, 0..3)
}

proptest! {
    /// Any journal and overrides read back equal from their own text, onto
    /// their base image, and are written back byte-identically.
    #[test]
    fn round_trip_is_lossless(
        bytes in prop::collection::vec(any::<u8>(), 0..40),
        steps in steps(),
        overrides in overrides(),
    ) {
        let j = build(&bytes, &steps);
        let text = enc(&j, &overrides);
        let restored = dec(&text, &bytes);
        prop_assert_eq!(&restored.journal, &j);
        prop_assert_eq!(&restored.overrides, &overrides);
        prop_assert_eq!(enc(&restored.journal, &restored.overrides), text);
    }

    /// Any other base image is refused.
    #[test]
    fn other_bases_are_refused(
        bytes in prop::collection::vec(any::<u8>(), 1..40),
        steps in steps(),
        flip in any::<usize>(),
    ) {
        let j = build(&bytes, &steps);
        let text = enc(&j, &[]);
        let mut other = bytes.clone();
        let i = flip % other.len();
        other[i] = other[i].wrapping_add(1);
        let refused = decode(&text, Image::new(other.clone()));
        prop_assert_eq!(
            refused,
            Err(SidecarError::BaseMismatch {
                expected: ImageHash::of(&bytes),
                found: ImageHash::of(&other),
            })
        );
    }

    /// Arbitrary text never panics.
    #[test]
    fn arbitrary_text_never_panics(text in "\\PC{0,200}") {
        let _ = decode(&text, Image::new(BASE.to_vec()));
    }

    /// A valid sidecar with any one byte replaced never panics, and is
    /// either refused or read back as some journal over the same base.
    #[test]
    fn corrupted_sidecars_never_panic(
        steps in steps(),
        at in any::<usize>(),
        byte in any::<u8>(),
    ) {
        let j = build(&BASE, &steps);
        let mut text = enc(&j, &[sample_override()]).into_bytes();
        let i = at % text.len();
        text[i] = byte;
        if let Ok(text) = String::from_utf8(text)
            && let Ok(restored) = decode(&text, Image::new(BASE.to_vec()))
        {
            prop_assert_eq!(restored.journal.image().original(), &BASE);
        }
    }
}
