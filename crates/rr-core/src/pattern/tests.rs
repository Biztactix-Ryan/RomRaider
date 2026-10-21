//! Unit tests for the compiled pattern kinds (US-RR-23-4).

use super::*;
use proptest::prelude::*;

// ---------------------------------------------------------------- bytes

#[test]
fn new_masks_value_and_keeps_offset() {
    let p = BytePattern::new(3, &[0x8F, 0xF0, 0xAB], &[0xF0, 0xFF, 0x00]).unwrap();
    assert_eq!(p.offset(), 3);
    assert_eq!(p.len(), 3);
    // Bits outside the mask are cleared at compile time.
    assert_eq!(p.value(), &[0x80, 0xF0, 0x00]);
    assert_eq!(p.mask(), &[0xF0, 0xFF, 0x00]);
    assert!(!p.is_exact());
}

#[test]
fn exact_checks_every_bit() {
    let p = BytePattern::exact(0, &[0x12, 0x34]).unwrap();
    assert_eq!(p.value(), &[0x12, 0x34]);
    assert_eq!(p.mask(), &[0xFF, 0xFF]);
    assert!(p.is_exact());
    assert!(p.matches(&[0x12, 0x34]));
    assert!(p.matches(&[0x12, 0x34, 0x56]));
    assert!(!p.matches(&[0x12, 0x35]));
}

#[test]
fn from_cells_maps_none_to_wildcard() {
    let p = BytePattern::from_cells(1, &[Some(0x80), None, Some(0x01)]).unwrap();
    assert_eq!(p.value(), &[0x80, 0x00, 0x01]);
    assert_eq!(p.mask(), &[0xFF, 0x00, 0xFF]);
    assert!(!p.is_exact());
    for wild in [0x00, 0x7F, 0xFF] {
        assert!(p.matches(&[0xEE, 0x80, wild, 0x01]));
    }
    assert!(!p.matches(&[0xEE, 0x81, 0x00, 0x01]));
    assert!(!p.matches(&[0xEE, 0x80, 0x00, 0x00]));
}

#[test]
fn partial_mask_checks_only_its_bits() {
    let p = BytePattern::new(0, &[0xA0], &[0xF0]).unwrap();
    for low in 0..=0x0F_u8 {
        assert!(p.matches(&[0xA0 | low]));
    }
    assert!(!p.matches(&[0xB0]));
    assert_eq!(
        p.check(&[0x2F]),
        Err(Mismatch::ByteDiffers {
            position: 0,
            expected: 0xA0,
            mask: 0xF0,
            found: 0x2F,
        })
    );
}

#[test]
fn constructors_refuse_empty() {
    assert_eq!(BytePattern::new(0, &[], &[]), Err(PatternError::EmptyBytes));
    assert_eq!(BytePattern::exact(0, &[]), Err(PatternError::EmptyBytes));
    assert_eq!(
        BytePattern::from_cells(0, &[]),
        Err(PatternError::EmptyBytes)
    );
    assert_eq!(
        BytePattern::text(0, "", Case::Sensitive),
        Err(PatternError::EmptyBytes)
    );
}

#[test]
fn new_refuses_mask_length_mismatch() {
    assert_eq!(
        BytePattern::new(0, &[1, 2], &[0xFF]),
        Err(PatternError::MaskLength { bytes: 2, mask: 1 })
    );
    assert_eq!(
        BytePattern::new(0, &[1], &[0xFF, 0xFF]),
        Err(PatternError::MaskLength { bytes: 1, mask: 2 })
    );
}

#[test]
fn constructors_refuse_offset_overflow() {
    assert_eq!(
        BytePattern::exact(usize::MAX, &[1]),
        Err(PatternError::OffsetOverflow {
            offset: usize::MAX,
            len: 1,
        })
    );
    assert_eq!(
        BytePattern::from_cells(usize::MAX - 1, &[None, None]),
        Err(PatternError::OffsetOverflow {
            offset: usize::MAX - 1,
            len: 2,
        })
    );
    // The last offset whose end fits is accepted.
    let p = BytePattern::exact(usize::MAX - 2, &[1, 2]).unwrap();
    assert_eq!(p.offset(), usize::MAX - 2);
    assert!(!p.matches(&[1, 2]));
    assert_eq!(
        p.check(&[1, 2]),
        Err(Mismatch::OutOfBounds {
            base: 0,
            offset: usize::MAX - 2,
            len: 2,
            input_len: 2,
        })
    );
}

#[test]
fn text_sensitive_is_exact_bytes() {
    let p = BytePattern::text(2, "A2WC", Case::Sensitive).unwrap();
    assert_eq!(p.value(), b"A2WC");
    assert!(p.is_exact());
    assert!(p.matches(b"..A2WC"));
    assert!(!p.matches(b"..a2WC"));
}

#[test]
fn text_insensitive_accepts_both_cases_of_letters_only() {
    let p = BytePattern::text(0, "A2wc-", Case::AsciiInsensitive).unwrap();
    assert_eq!(p.mask(), &[0xDF, 0xFF, 0xDF, 0xDF, 0xFF]);
    assert_eq!(p.value(), b"A2WC-");
    for id in [b"A2WC-", b"a2wc-", b"A2wC-"] {
        assert!(p.matches(id), "{id:?}");
    }
    // `2` (0x32) is checked whole: 0x12 differs only in bit 5.
    assert!(!p.matches(b"A\x12WC-"));
    // `-` (0x2D) is checked whole: 0x0D differs only in bit 5.
    assert!(!p.matches(b"A2WC\x0D"));
    // The 0xDF mask accepts exactly the two cases of a letter.
    let accepted: Vec<u8> = (0..=u8::MAX)
        .filter(|&b| p.matches(&[b, b'2', b'W', b'C', b'-']))
        .collect();
    assert_eq!(accepted, [b'A', b'a']);
}

#[test]
fn offset_at_or_past_end_is_out_of_bounds() {
    let p = BytePattern::exact(4, &[0xAA]).unwrap();
    let input = [0xAA; 4];
    for input in [&input[..], &input[..0]] {
        assert!(!p.matches(input));
        assert_eq!(
            p.check(input),
            Err(Mismatch::OutOfBounds {
                base: 0,
                offset: 4,
                len: 1,
                input_len: input.len(),
            })
        );
    }
    // Exactly fitting is in bounds.
    assert!(p.matches(&[0, 0, 0, 0, 0xAA]));
    assert_eq!(p.check(&[0, 0, 0, 0, 0xAA]), Ok(()));
}

#[test]
fn pattern_straddling_end_is_out_of_bounds() {
    let p = BytePattern::exact(2, &[1, 2, 3]).unwrap();
    assert!(!p.matches(&[0, 0, 1, 2]));
    assert_eq!(
        p.check(&[0, 0, 1, 2]),
        Err(Mismatch::OutOfBounds {
            base: 0,
            offset: 2,
            len: 3,
            input_len: 4,
        })
    );
}

#[test]
fn check_reports_first_differing_byte_with_absolute_position() {
    let p = BytePattern::from_cells(2, &[Some(0x80), None, Some(0x01), Some(0x02)]).unwrap();
    assert_eq!(
        p.check(&[0, 0, 0x80, 0x99, 0x11, 0x22]),
        Err(Mismatch::ByteDiffers {
            position: 4,
            expected: 0x01,
            mask: 0xFF,
            found: 0x11,
        })
    );
    assert_eq!(
        p.check_at(&[0, 0, 0, 0x80, 0x99, 0x01, 0x22], 1),
        Err(Mismatch::ByteDiffers {
            position: 6,
            expected: 0x02,
            mask: 0xFF,
            found: 0x22,
        })
    );
}

#[test]
fn check_at_places_pattern_at_base() {
    let p = BytePattern::exact(1, &[0xBE, 0xEF]).unwrap();
    let input = [0x00, 0x00, 0x00, 0xBE, 0xEF];
    assert!(!p.matches_at(&input, 0));
    assert!(!p.matches_at(&input, 1));
    assert!(p.matches_at(&input, 2));
    assert_eq!(p.check_at(&input, 2), Ok(()));
    assert!(!p.matches_at(&input, 3));
    assert_eq!(
        p.check_at(&input, 3),
        Err(Mismatch::OutOfBounds {
            base: 3,
            offset: 1,
            len: 2,
            input_len: 5,
        })
    );
}

#[test]
fn check_at_never_overflows() {
    let p = BytePattern::exact(1, &[0]).unwrap();
    let input = [0_u8; 8];
    for base in [usize::MAX, usize::MAX - 1, usize::MAX - 2, usize::MAX / 2] {
        assert!(!p.matches_at(&input, base));
        assert_eq!(
            p.check_at(&input, base),
            Err(Mismatch::OutOfBounds {
                base,
                offset: 1,
                len: 1,
                input_len: 8,
            })
        );
    }
}

#[test]
fn find_returns_first_base() {
    let p = BytePattern::from_cells(0, &[Some(0x80), Some(0xF0), None]).unwrap();
    assert_eq!(p.find(&[0x00, 0x80, 0xF0, 0x05, 0x80, 0xF0, 0x06]), Some(1));
    assert_eq!(p.find(&[0x80, 0xF0, 0x00]), Some(0));
    // A match ending exactly at the end is found.
    assert_eq!(p.find(&[0x00, 0x00, 0x80, 0xF0, 0x00]), Some(2));
    // A match cut off by the end is not.
    assert_eq!(p.find(&[0x00, 0x00, 0x80, 0xF0]), None);
    assert_eq!(p.find(&[]), None);
    assert_eq!(p.find(&[0x80, 0xF1, 0x00]), None);
}

#[test]
fn find_accounts_for_offset() {
    let p = BytePattern::exact(2, &[0xAA]).unwrap();
    // The byte at index 3 matches with base 1.
    assert_eq!(p.find(&[0, 0, 0, 0xAA]), Some(1));
    assert_eq!(p.find(&[0xAA, 0xAA]), None);
    assert_eq!(p.find(&[0, 0, 0xAA]), Some(0));
}

// ----------------------------------------------------------------- op

#[test]
fn op_matches_exactly() {
    let p = OpPattern::new("read_memory").unwrap();
    assert_eq!(p.name(), "read_memory");
    assert!(p.matches("read_memory"));
    assert_eq!(p.check("read_memory"), Ok(()));
    for other in ["READ_MEMORY", "read_memor", "read_memory2", ""] {
        assert!(!p.matches(other));
        assert_eq!(p.check(other), Err(Mismatch::OpDiffers));
    }
}

#[test]
fn op_refuses_empty_and_whitespace() {
    assert_eq!(
        OpPattern::new(""),
        Err(PatternError::EmptyName {
            kind: PatternKind::Op
        })
    );
    assert_eq!(
        OpPattern::new("read memory"),
        Err(PatternError::InvalidOpName { position: 4 })
    );
    assert_eq!(
        OpPattern::new("init\u{0}"),
        Err(PatternError::InvalidOpName { position: 4 })
    );
    assert_eq!(
        OpPattern::new("é\u{7f}"),
        Err(PatternError::InvalidOpName { position: 2 })
    );
    assert_eq!(
        OpPattern::new("\tx"),
        Err(PatternError::InvalidOpName { position: 0 })
    );
}

// ----------------------------------------------------------- ID prefix

#[test]
fn id_prefix_matches_start() {
    let p = IdPrefix::new("4B12", Case::Sensitive).unwrap();
    assert_eq!(p.prefix(), "4B12");
    assert_eq!(p.case(), Case::Sensitive);
    for id in ["4B12", "4B12785207"] {
        assert!(p.matches(id));
        assert_eq!(p.check(id), Ok(()));
    }
    assert!(!p.matches("4b12785207"));
    assert_eq!(
        p.check("4b12785207"),
        Err(Mismatch::IdDiffers { position: 1 })
    );
    assert_eq!(p.check("4B13"), Err(Mismatch::IdDiffers { position: 3 }));
    assert_eq!(p.check("X"), Err(Mismatch::IdDiffers { position: 0 }));
}

#[test]
fn id_prefix_too_short() {
    let p = IdPrefix::new("4B12", Case::Sensitive).unwrap();
    for (id, len) in [("4B1", 3), ("", 0)] {
        assert!(!p.matches(id));
        assert_eq!(
            p.check(id),
            Err(Mismatch::IdTooShort {
                prefix_len: 4,
                id_len: len,
            })
        );
    }
    // A short ID that already differs reports the difference.
    assert_eq!(p.check("4C"), Err(Mismatch::IdDiffers { position: 1 }));
}

#[test]
fn id_prefix_case_insensitive() {
    let p = IdPrefix::new("a2wc", Case::AsciiInsensitive).unwrap();
    assert_eq!(p.case(), Case::AsciiInsensitive);
    for id in ["A2WC522N", "a2wc", "A2wC"] {
        assert!(p.matches(id));
        assert_eq!(p.check(id), Ok(()));
    }
    assert_eq!(p.check("A3WC"), Err(Mismatch::IdDiffers { position: 1 }));
}

#[test]
fn id_prefix_refuses_empty() {
    assert_eq!(
        IdPrefix::new("", Case::Sensitive),
        Err(PatternError::EmptyName {
            kind: PatternKind::IdPrefix
        })
    );
}

#[test]
fn case_defaults_to_sensitive() {
    assert_eq!(Case::default(), Case::Sensitive);
}

// ----------------------------------------------------------- header set

#[test]
fn header_set_requires_every_name_in_any_order() {
    let p = HeaderSet::new(&["Time/s", " RPM "], Case::Sensitive).unwrap();
    assert_eq!(p.names(), &["Time/s".into(), "RPM".into()]);
    assert_eq!(p.case(), Case::Sensitive);
    let row = ["AFR", " RPM", "Time/s "];
    assert!(p.matches(&row));
    assert_eq!(p.check(&row), Ok(()));
    let missing_time = ["RPM", "Time"];
    assert!(!p.matches(&missing_time));
    assert_eq!(
        p.check(&missing_time),
        Err(Mismatch::HeaderMissing { index: 0 })
    );
    let missing_rpm = vec![String::from("Time/s")];
    assert_eq!(
        p.check(&missing_rpm),
        Err(Mismatch::HeaderMissing { index: 1 })
    );
    let empty: [&str; 0] = [];
    assert_eq!(p.check(&empty), Err(Mismatch::HeaderMissing { index: 0 }));
}

#[test]
fn header_set_matches_whole_names_not_substrings() {
    // Legacy accepted any header containing the name.
    let p = HeaderSet::new(&["time"], Case::Sensitive).unwrap();
    assert!(!p.matches(&["Engine time"]));
    assert!(p.matches(&["time"]));
}

#[test]
fn header_set_case_insensitive() {
    let p = HeaderSet::new(&["Seconds"], Case::AsciiInsensitive).unwrap();
    assert!(p.matches(&["SECONDS"]));
    assert!(
        !HeaderSet::new(&["Seconds"], Case::Sensitive)
            .unwrap()
            .matches(&["SECONDS"])
    );
}

#[test]
fn header_set_refuses_bad_names() {
    let none: [&str; 0] = [];
    assert_eq!(
        HeaderSet::new(&none, Case::Sensitive),
        Err(PatternError::EmptyHeaderSet)
    );
    assert_eq!(
        HeaderSet::new(&["a", "  "], Case::Sensitive),
        Err(PatternError::EmptyName {
            kind: PatternKind::HeaderSet
        })
    );
    assert_eq!(
        HeaderSet::new(&["a", "b", " a"], Case::Sensitive),
        Err(PatternError::DuplicateHeader {
            first: 0,
            second: 2
        })
    );
    assert_eq!(
        HeaderSet::new(&["x", "RPM", "rpm"], Case::AsciiInsensitive),
        Err(PatternError::DuplicateHeader {
            first: 1,
            second: 2
        })
    );
    assert!(HeaderSet::new(&["RPM", "rpm"], Case::Sensitive).is_ok());
}

// ----------------------------------------------------------- Pattern

fn all_patterns() -> [Pattern; 4] {
    [
        BytePattern::exact(0, &[1]).unwrap().into(),
        OpPattern::new("init").unwrap().into(),
        IdPrefix::new("4B", Case::Sensitive).unwrap().into(),
        HeaderSet::new(&["RPM"], Case::Sensitive).unwrap().into(),
    ]
}

#[test]
fn pattern_dispatches_to_its_kind() {
    let [bytes, op, id, headers] = all_patterns();
    assert_eq!(bytes.kind(), PatternKind::Bytes);
    assert_eq!(op.kind(), PatternKind::Op);
    assert_eq!(id.kind(), PatternKind::IdPrefix);
    assert_eq!(headers.kind(), PatternKind::HeaderSet);

    let cases: [(&Pattern, Subject<'_>, Subject<'_>); 4] = [
        (&bytes, Subject::Bytes(&[1]), Subject::Bytes(&[2])),
        (&op, Subject::Op("init"), Subject::Op("read")),
        (&id, Subject::Id("4B12"), Subject::Id("4C12")),
        (
            &headers,
            Subject::Headers(&["RPM"]),
            Subject::Headers(&["MAF"]),
        ),
    ];
    for (p, yes, no) in cases {
        assert_eq!(p.kind(), yes.kind());
        assert!(p.matches(yes));
        assert_eq!(p.check(yes), Ok(()));
        assert!(!p.matches(no));
        assert!(p.check(no).is_err());
    }
}

#[test]
fn pattern_refuses_wrong_subject() {
    let subjects = [
        Subject::Bytes(&[1]),
        Subject::Op("init"),
        Subject::Id("4B"),
        Subject::Headers(&["RPM"]),
    ];
    for p in all_patterns() {
        for s in subjects {
            if s.kind() == p.kind() {
                continue;
            }
            assert!(!p.matches(s));
            assert_eq!(
                p.check(s),
                Err(Mismatch::WrongSubject {
                    pattern: p.kind(),
                    subject: s.kind(),
                })
            );
        }
    }
}

#[test]
fn subject_kinds() {
    assert_eq!(Subject::Bytes(&[]).kind(), PatternKind::Bytes);
    assert_eq!(Subject::Op("").kind(), PatternKind::Op);
    assert_eq!(Subject::Id("").kind(), PatternKind::IdPrefix);
    assert_eq!(Subject::Headers(&[]).kind(), PatternKind::HeaderSet);
}

// ------------------------------------------------------------ rendering

#[test]
fn mismatch_display() {
    let cases = [
        (
            Mismatch::WrongSubject {
                pattern: PatternKind::Bytes,
                subject: PatternKind::HeaderSet,
            },
            "pattern kind bytes cannot match subject kind header set",
        ),
        (
            Mismatch::WrongSubject {
                pattern: PatternKind::Op,
                subject: PatternKind::IdPrefix,
            },
            "pattern kind op cannot match subject kind ID prefix",
        ),
        (
            Mismatch::OutOfBounds {
                base: 0x10,
                offset: 0x20,
                len: 4,
                input_len: 16,
            },
            "input is 16 bytes; the pattern needs bytes 0x30..0x34",
        ),
        (
            Mismatch::ByteDiffers {
                position: 0x2001,
                expected: 0x80,
                mask: 0xFF,
                found: 0x7F,
            },
            "byte at 0x2001 is 0x7f; expected 0x80",
        ),
        (
            Mismatch::ByteDiffers {
                position: 0,
                expected: 0x80,
                mask: 0xF0,
                found: 0x0F,
            },
            "byte at 0x0 is 0x0f; expected 0x80 under mask 0xf0",
        ),
        (Mismatch::OpDiffers, "the op differs"),
        (
            Mismatch::IdTooShort {
                prefix_len: 5,
                id_len: 3,
            },
            "ID is 3 bytes, shorter than the 5-byte prefix",
        ),
        (
            Mismatch::IdDiffers { position: 2 },
            "ID differs from the prefix at byte 2",
        ),
        (
            Mismatch::HeaderMissing { index: 1 },
            "required header 1 is missing",
        ),
    ];
    for (m, text) in cases {
        assert_eq!(m.to_string(), text);
    }
}

#[test]
fn out_of_bounds_display_past_address_space() {
    for (base, offset, len) in [
        (usize::MAX, 1, 1),
        (usize::MAX - 1, 1, 1),
        (1, usize::MAX, 0),
    ] {
        let m = Mismatch::OutOfBounds {
            base,
            offset,
            len,
            input_len: 8,
        };
        assert_eq!(
            m.to_string(),
            format!(
                "input is 8 bytes; the pattern ({len} bytes at {offset:#x} past {base:#x}) \
                 reaches past the end of the address space"
            )
        );
    }
    // The start fits but the end does not.
    let m = Mismatch::OutOfBounds {
        base: usize::MAX - 1,
        offset: 0,
        len: 2,
        input_len: 8,
    };
    assert!(
        m.to_string()
            .ends_with("reaches past the end of the address space")
    );
    // Both fit.
    let m = Mismatch::OutOfBounds {
        base: usize::MAX - 2,
        offset: 0,
        len: 2,
        input_len: 8,
    };
    assert_eq!(
        m.to_string(),
        format!(
            "input is 8 bytes; the pattern needs bytes {:#x}..{:#x}",
            usize::MAX - 2,
            usize::MAX
        )
    );
}

#[test]
fn explain_uses_pattern_names() {
    let [bytes, op, id, headers] = all_patterns();
    let m = op.check(Subject::Op("read")).unwrap_err();
    assert_eq!(op.explain(&m).to_string(), "op is not \"init\"");

    let m = id.check(Subject::Id("4")).unwrap_err();
    assert_eq!(
        id.explain(&m).to_string(),
        "ID is 1 bytes, shorter than the prefix \"4B\""
    );
    let m = id.check(Subject::Id("4C")).unwrap_err();
    assert_eq!(
        id.explain(&m).to_string(),
        "ID does not start with \"4B\" (differs at byte 1)"
    );

    let m = headers.check(Subject::Headers(&["MAF"])).unwrap_err();
    assert_eq!(headers.explain(&m).to_string(), "no header named \"RPM\"");

    let m = bytes.check(Subject::Bytes(&[2])).unwrap_err();
    assert_eq!(bytes.explain(&m).to_string(), m.to_string());
    assert_eq!(
        bytes.explain(&m).to_string(),
        "byte at 0x0 is 0x02; expected 0x01"
    );
}

#[test]
fn explain_falls_back_for_foreign_mismatch() {
    let [bytes, op, id, headers] = all_patterns();
    // An index the set does not have.
    let m = Mismatch::HeaderMissing { index: 7 };
    assert_eq!(headers.explain(&m).to_string(), m.to_string());
    // A mismatch of another kind.
    for (p, m) in [
        (&op, Mismatch::IdDiffers { position: 0 }),
        (&id, Mismatch::OpDiffers),
        (&bytes, Mismatch::HeaderMissing { index: 0 }),
        (&headers, Mismatch::OpDiffers),
    ] {
        assert_eq!(p.explain(&m).to_string(), m.to_string());
    }
}

#[test]
fn pattern_kind_display() {
    assert_eq!(PatternKind::Bytes.to_string(), "bytes");
    assert_eq!(PatternKind::Op.to_string(), "op");
    assert_eq!(PatternKind::IdPrefix.to_string(), "ID prefix");
    assert_eq!(PatternKind::HeaderSet.to_string(), "header set");
}

#[test]
fn pattern_error_display() {
    let cases = [
        (PatternError::EmptyBytes, "byte pattern is empty"),
        (
            PatternError::MaskLength { bytes: 3, mask: 2 },
            "mask is 2 bytes but the byte string is 3 bytes",
        ),
        (
            PatternError::OffsetOverflow {
                offset: 0x10,
                len: 2,
            },
            "byte pattern of 2 bytes at offset 0x10 ends past the address space",
        ),
        (
            PatternError::EmptyName {
                kind: PatternKind::IdPrefix,
            },
            "ID prefix pattern has an empty name",
        ),
        (
            PatternError::InvalidOpName { position: 3 },
            "op name has whitespace or a control character at byte 3",
        ),
        (PatternError::EmptyHeaderSet, "header set has no names"),
        (
            PatternError::DuplicateHeader {
                first: 0,
                second: 2,
            },
            "header name 2 repeats header name 0",
        ),
    ];
    for (e, text) in cases {
        assert_eq!(e.to_string(), text);
    }
    let _: &dyn core::error::Error = &PatternError::EmptyBytes;
}

// ------------------------------------------------------------ properties

proptest! {
    /// `matches` and `check` agree on every input, base and pattern, and
    /// neither panics.
    #[test]
    fn matches_agrees_with_check(
        offset in 0usize..8,
        cells in proptest::collection::vec((any::<u8>(), any::<u8>()), 1..6),
        input in proptest::collection::vec(any::<u8>(), 0..16),
        base in prop_oneof![0usize..16, Just(usize::MAX), Just(usize::MAX - 3)],
    ) {
        let (bytes, mask): (Vec<u8>, Vec<u8>) = cells.into_iter().unzip();
        let p = BytePattern::new(offset, &bytes, &mask).unwrap();
        prop_assert_eq!(p.matches_at(&input, base), p.check_at(&input, base).is_ok());
        if base == 0 {
            prop_assert_eq!(p.matches(&input), p.check(&input).is_ok());
        }
        if let Some(found) = p.find(&input) {
            prop_assert!(p.matches_at(&input, found));
            for earlier in 0..found {
                prop_assert!(!p.matches_at(&input, earlier));
            }
        }
    }

    /// A byte pattern compiled from the bytes it is matched against
    /// matches them, whatever its mask.
    #[test]
    fn pattern_matches_its_source(
        offset in 0usize..4,
        cells in proptest::collection::vec((any::<u8>(), any::<u8>()), 1..8),
    ) {
        let (bytes, mask): (Vec<u8>, Vec<u8>) = cells.into_iter().unzip();
        let p = BytePattern::new(offset, &bytes, &mask).unwrap();
        let mut input = vec![0xA5; offset];
        input.extend_from_slice(&bytes);
        prop_assert_eq!(p.check(&input), Ok(()));
        prop_assert_eq!(p.find(&input), Some(0));
    }
}

// ------------------------------------- US-RR-23-1: single-bit rejection

/// Asserts the US-RR-23-1 property for `p`, compiled from `bytes` under
/// `mask` (the bits each byte checks): embedded in `input` at `p.offset()`
/// after `prefix` padding bytes it matches; flipping any single checked bit
/// makes `matches` false and `check` report that byte; flipping any
/// unchecked (wildcard or masked-out) bit still matches. Exhaustive over
/// every bit of the pattern.
fn assert_matches_source_and_rejects_bit_flips(
    p: &BytePattern,
    bytes: &[u8],
    mask: &[u8],
    input: &[u8],
) -> Result<(), TestCaseError> {
    let start = p.offset();
    prop_assert_eq!(p.check(input), Ok(()));
    prop_assert!(p.matches(input));
    // Through the generic dispatcher too: one matcher for every caller.
    let generic = Pattern::from(p.clone());
    prop_assert!(generic.matches(Subject::Bytes(input)));
    let mut flipped = input.to_vec();
    for i in 0..bytes.len() {
        for bit in 0..8 {
            let flip = 1u8 << bit;
            flipped[start + i] ^= flip;
            if mask[i] & flip != 0 {
                prop_assert!(
                    !p.matches(&flipped),
                    "byte {} bit {} flipped yet matched",
                    i,
                    bit
                );
                prop_assert!(!generic.matches(Subject::Bytes(&flipped)));
                prop_assert_eq!(
                    p.check(&flipped),
                    Err(Mismatch::ByteDiffers {
                        position: start + i,
                        expected: bytes[i] & mask[i],
                        mask: mask[i],
                        found: bytes[i] ^ flip,
                    })
                );
            } else {
                prop_assert!(
                    p.matches(&flipped),
                    "unchecked byte {} bit {} flipped and rejected",
                    i,
                    bit
                );
                prop_assert_eq!(p.check(&flipped), Ok(()));
            }
            flipped[start + i] ^= flip;
        }
    }
    prop_assert_eq!(flipped.as_slice(), input);
    Ok(())
}

/// Builds `prefix ++ bytes ++ suffix`, the input a pattern at offset
/// `prefix.len()` is matched against.
fn embed(prefix: &[u8], bytes: &[u8], suffix: &[u8]) -> Vec<u8> {
    let mut input = prefix.to_vec();
    input.extend_from_slice(bytes);
    input.extend_from_slice(suffix);
    input
}

proptest! {
    /// US-RR-23-1: a byte pattern compiled (via `from_cells`) from a
    /// concrete byte string with random whole-byte `??` wildcards matches
    /// that string, and rejects any single-bit change in a non-wildcard
    /// position, while a bit change under a wildcard still matches.
    #[test]
    fn wildcard_pattern_matches_source_and_rejects_any_single_bit_flip(
        cells in proptest::collection::vec((any::<u8>(), any::<bool>()), 1..12),
        prefix in proptest::collection::vec(any::<u8>(), 0..8),
        suffix in proptest::collection::vec(any::<u8>(), 0..8),
    ) {
        let bytes: Vec<u8> = cells.iter().map(|&(b, _)| b).collect();
        let pattern_cells: Vec<Option<u8>> =
            cells.iter().map(|&(b, wild)| (!wild).then_some(b)).collect();
        let mask: Vec<u8> = cells.iter().map(|&(_, wild)| if wild { 0x00 } else { 0xFF }).collect();
        let p = BytePattern::from_cells(prefix.len(), &pattern_cells).unwrap();
        prop_assert_eq!(p.mask(), mask.as_slice());
        let input = embed(&prefix, &bytes, &suffix);
        assert_matches_source_and_rejects_bit_flips(&p, &bytes, &mask, &input)?;
    }

    /// US-RR-23-1 with partial bit masks (`new`): every bit the mask
    /// checks rejects a flip; every bit it leaves out does not.
    #[test]
    fn masked_pattern_matches_source_and_rejects_any_single_checked_bit_flip(
        cells in proptest::collection::vec(
            (any::<u8>(), prop_oneof![Just(0x00u8), Just(0xFFu8), any::<u8>()]),
            1..12,
        ),
        prefix in proptest::collection::vec(any::<u8>(), 0..8),
        suffix in proptest::collection::vec(any::<u8>(), 0..8),
    ) {
        let (bytes, mask): (Vec<u8>, Vec<u8>) = cells.into_iter().unzip();
        let p = BytePattern::new(prefix.len(), &bytes, &mask).unwrap();
        let input = embed(&prefix, &bytes, &suffix);
        assert_matches_source_and_rejects_bit_flips(&p, &bytes, &mask, &input)?;
    }
}

// ------------------------- US-RR-23-3: offset past the end is no-match

/// Offsets and bases across the whole `usize` range: small values, values
/// at and just below `usize::MAX`, and anything at all.
fn wide_usize() -> impl Strategy<Value = usize> {
    prop_oneof![
        0usize..40,
        (usize::MAX - 40)..=usize::MAX,
        Just(usize::MAX),
        Just(usize::MAX / 2),
        any::<usize>(),
    ]
}

/// The out-of-range mismatch `p` must report for `input` at `base`.
fn out_of_bounds(p: &BytePattern, input: &[u8], base: usize) -> Mismatch {
    Mismatch::OutOfBounds {
        base,
        offset: p.offset(),
        len: p.len(),
        input_len: input.len(),
    }
}

/// Asserts the US-RR-23-3 property for `p` on `input` at `base`, against an
/// independent oracle of where the pattern's bytes lie: when
/// `base + offset + len` overflows or exceeds `input.len()`, `matches_at`
/// is false and `check_at` is exactly [`Mismatch::OutOfBounds`] (never
/// [`Mismatch::ByteDiffers`]); otherwise `check_at` never reports
/// out-of-bounds and any differing byte lies inside the input. With `base`
/// `0` the same holds for `matches` / `check` and the generic
/// [`Pattern`] / [`Subject`] path. Rendering the mismatch never panics.
fn assert_out_of_range_is_no_match(
    p: &BytePattern,
    input: &[u8],
    base: usize,
) -> Result<(), TestCaseError> {
    let end = base
        .checked_add(p.offset())
        .and_then(|start| start.checked_add(p.len()));
    let in_bounds = end.is_some_and(|end| end <= input.len());
    let checked = p.check_at(input, base);
    prop_assert_eq!(p.matches_at(input, base), checked.is_ok());
    if in_bounds {
        match checked {
            Ok(()) => {}
            Err(Mismatch::ByteDiffers { position, .. }) => {
                prop_assert!(position < input.len());
            }
            Err(other) => prop_assert!(false, "in bounds yet {:?}", other),
        }
    } else {
        prop_assert!(!p.matches_at(input, base));
        prop_assert_eq!(checked, Err(out_of_bounds(p, input, base)));
    }
    if let Err(m) = checked {
        let _ = m.to_string();
        let _ = Pattern::from(p.clone()).explain(&m).to_string();
    }
    if base == 0 {
        prop_assert_eq!(p.check(input), checked);
        prop_assert_eq!(p.matches(input), checked.is_ok());
        let generic = Pattern::from(p.clone());
        prop_assert_eq!(generic.check(Subject::Bytes(input)), checked);
        prop_assert_eq!(generic.matches(Subject::Bytes(input)), checked.is_ok());
    }
    Ok(())
}

#[test]
fn offset_past_end_is_no_match_at_the_edges() {
    let input = [0xAA_u8; 4];
    // Offset exactly at the end, one past it, and far past it.
    for offset in [4, 5, 0x1000, usize::MAX / 2, usize::MAX - 1] {
        let p = BytePattern::exact(offset, &[0xAA]).unwrap();
        for base in [0, 1, 4, usize::MAX - 1, usize::MAX] {
            assert!(!p.matches_at(&input, base));
            assert_eq!(
                p.check_at(&input, base),
                Err(out_of_bounds(&p, &input, base))
            );
        }
        assert_eq!(p.find(&input), None);
        let generic = Pattern::from(p.clone());
        assert!(!generic.matches(Subject::Bytes(&input)));
        assert_eq!(
            generic.check(Subject::Bytes(&input)),
            Err(out_of_bounds(&p, &input, 0))
        );
    }
    // The largest offset a pattern can have, on an empty input.
    let p = BytePattern::exact(usize::MAX - 1, &[0]).unwrap();
    assert!(!p.matches(&[]));
    assert_eq!(p.check(&[]), Err(out_of_bounds(&p, &[], 0)));
    assert_eq!(p.find(&[]), None);
}

#[test]
fn offset_in_bounds_but_straddling_end_is_no_match_even_if_bytes_agree() {
    // Every byte that is present agrees with the pattern; the missing tail
    // still makes it a no-match, reported as out of bounds.
    let p = BytePattern::exact(3, &[1, 2, 3, 4]).unwrap();
    let full = [9, 9, 9, 1, 2, 3, 4];
    assert!(p.matches(&full));
    for cut in 0..full.len() {
        let input = &full[..cut];
        assert!(!p.matches(input), "cut {cut}");
        assert_eq!(p.check(input), Err(out_of_bounds(&p, input, 0)));
        assert_eq!(p.find(input), None);
    }
}

proptest! {
    /// US-RR-23-3: whatever the pattern, its offset (up to the largest a
    /// pattern can have), the base it is placed at (up to `usize::MAX`) and
    /// the input (including empty), a pattern reaching past the end of the
    /// input is a no-match reported as out-of-bounds, and nothing panics.
    #[test]
    fn matching_past_end_of_input_is_no_match_never_panic(
        offset in wide_usize(),
        cells in proptest::collection::vec((any::<u8>(), any::<u8>()), 1..8),
        input in proptest::collection::vec(any::<u8>(), 0..40),
        base in wide_usize(),
    ) {
        let (bytes, mask): (Vec<u8>, Vec<u8>) = cells.into_iter().unzip();
        let p = match BytePattern::new(offset, &bytes, &mask) {
            Ok(p) => p,
            Err(e) => {
                // Only an end past `usize::MAX` is refused; such a
                // pattern could never match anything.
                prop_assert!(offset.checked_add(bytes.len()).is_none());
                prop_assert_eq!(e, PatternError::OffsetOverflow { offset, len: bytes.len() });
                return Ok(());
            }
        };
        assert_out_of_range_is_no_match(&p, &input, base)?;
        assert_out_of_range_is_no_match(&p, &input, 0)?;
        // `find` on an input shorter than the pattern's span finds nothing.
        if offset + p.len() > input.len() {
            prop_assert_eq!(p.find(&input), None);
        }
    }

    /// US-RR-23-3 at the exact boundary: the pattern's source bytes, cut
    /// short anywhere (so the pattern straddles or starts at or past the
    /// end), never match; whole, they match at base `0` and at no base
    /// that pushes the pattern past the end.
    #[test]
    fn truncated_source_is_out_of_bounds_not_byte_differs(
        offset in 0usize..12,
        cells in proptest::collection::vec((any::<u8>(), any::<u8>()), 1..8),
        cut in any::<prop::sample::Index>(),
        base in prop_oneof![1usize..16, Just(usize::MAX)],
    ) {
        let (bytes, mask): (Vec<u8>, Vec<u8>) = cells.into_iter().unzip();
        let p = BytePattern::new(offset, &bytes, &mask).unwrap();
        let full = embed(&vec![0x5A; offset], &bytes, &[]);
        prop_assert!(p.matches(&full));
        // Any base > 0 pushes the last byte past the end.
        prop_assert_eq!(p.check_at(&full, base), Err(out_of_bounds(&p, &full, base)));
        let short = &full[..cut.index(full.len())];
        prop_assert_eq!(p.check(short), Err(out_of_bounds(&p, short, 0)));
        prop_assert!(!p.matches(short));
        prop_assert_eq!(p.find(short), None);
        assert_out_of_range_is_no_match(&p, short, base)?;
    }
}
