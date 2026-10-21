//! Unit and property tests for the byte pattern text form (US-RR-23-5).

use super::*;
use crate::diag::Severity;
use crate::pattern::Case;
use proptest::prelude::*;

fn ok(text: &str) -> BytePattern {
    BytePattern::parse(text).unwrap_or_else(|e| panic!("{text:?}: {e} at {:?}", e.span()))
}

fn err(text: &str) -> ParseError {
    match BytePattern::parse(text) {
        Ok(p) => panic!("{text:?} parsed as {p:?}"),
        Err(e) => e,
    }
}

// ---------------------------------------------------------------- good forms

#[test]
fn good_forms_parse_to_their_cells_and_print_canonically() {
    /// Text, offset, value, mask, canonical text.
    type Row<'a> = (&'a str, usize, &'a [u8], &'a [u8], &'a str);
    let cases: &[Row<'_>] = &[
        (
            "80 F0 ?? 01",
            0,
            &[0x80, 0xF0, 0x00, 0x01],
            &[0xFF, 0xFF, 0x00, 0xFF],
            "80 F0 ?? 01",
        ),
        (
            "80f0??01",
            0,
            &[0x80, 0xF0, 0, 1],
            &[0xFF, 0xFF, 0, 0xFF],
            "80 F0 ?? 01",
        ),
        ("0x4a4B", 0, &[0x4A, 0x4B], &[0xFF, 0xFF], "4A 4B"),
        ("0X4A 0x4b", 0, &[0x4A, 0x4B], &[0xFF, 0xFF], "4A 4B"),
        ("  \t 12\t\t34  ", 0, &[0x12, 0x34], &[0xFF, 0xFF], "12 34"),
        ("8? ?f", 0, &[0x80, 0x0F], &[0xF0, 0x0F], "8? ?F"),
        ("41/DF", 0, &[0x41], &[0xDF], "41/DF"),
        ("41/df42", 0, &[0x41, 0x42], &[0xDF, 0xFF], "41/DF 42"),
        // A `/` mask that is itself whole, nibble or empty prints as such.
        (
            "41/FF 40/F0 01/0F 00/00",
            0,
            &[0x41, 0x40, 0x01, 0],
            &[0xFF, 0xF0, 0x0F, 0],
            "41 4? ?1 ??",
        ),
        (
            "@0x2000 80 F0 ?? 01",
            0x2000,
            &[0x80, 0xF0, 0, 1],
            &[0xFF, 0xFF, 0, 0xFF],
            "@0x2000 80 F0 ?? 01",
        ),
        ("@8192 80", 8192, &[0x80], &[0xFF], "@0x2000 80"),
        ("@0X1f 80", 0x1F, &[0x80], &[0xFF], "@0x1F 80"),
        ("@0 80", 0, &[0x80], &[0xFF], "80"),
        ("@0x0 80", 0, &[0x80], &[0xFF], "80"),
        ("@007 80", 7, &[0x80], &[0xFF], "@0x7 80"),
        ("@1\t80", 1, &[0x80], &[0xFF], "@0x1 80"),
        ("??", 0, &[0], &[0], "??"),
        // A mask on a byte that is not the first of its word.
        ("8041/DF", 0, &[0x80, 0x41], &[0xFF, 0xDF], "80 41/DF"),
        ("0x41/DF", 0, &[0x41], &[0xDF], "41/DF"),
        (
            "0x8041/DF7?",
            0,
            &[0x80, 0x41, 0x70],
            &[0xFF, 0xDF, 0xF0],
            "80 41/DF 7?",
        ),
    ];
    for &(text, offset, value, mask, canonical) in cases {
        let p = ok(text);
        assert_eq!(p.offset(), offset, "{text:?}");
        assert_eq!(p.value(), value, "{text:?}");
        assert_eq!(p.mask(), mask, "{text:?}");
        assert_eq!(p.to_string(), canonical, "{text:?}");
        assert_eq!(ok(canonical), p, "{canonical:?}");
        assert_eq!(text.parse::<BytePattern>(), Ok(p), "{text:?}");
    }
}

#[test]
fn parsed_pattern_matches_as_compiled() {
    let p = ok("@2 80 F0 ?? 01");
    assert_eq!(
        p,
        BytePattern::from_cells(2, &[Some(0x80), Some(0xF0), None, Some(0x01)]).unwrap()
    );
    assert!(p.matches(&[0, 0, 0x80, 0xF0, 0x55, 0x01]));
    assert!(!p.matches(&[0, 0, 0x80, 0xF0, 0x55, 0x02]));
    let ci = ok("41/DF 42/DF");
    assert_eq!(
        ci,
        BytePattern::text(0, "AB", Case::AsciiInsensitive).unwrap()
    );
    assert!(ci.matches(b"ab") && ci.matches(b"AB") && !ci.matches(b"AC"));
    let nib = ok("8? ?1");
    assert!(
        nib.matches(&[0x8A, 0xF1]) && !nib.matches(&[0x9A, 0xF1]) && !nib.matches(&[0x8A, 0xF2])
    );
}

#[test]
fn the_longest_text_and_the_most_bytes_parse() {
    let bytes = "80".repeat(MAX_BYTES);
    let text = format!("{bytes}{}", " ".repeat(MAX_TEXT_LEN - bytes.len()));
    assert_eq!(text.len(), MAX_TEXT_LEN);
    assert_eq!(ok(&text).len(), MAX_BYTES);
}

#[test]
fn one_byte_too_many_is_refused_at_that_byte() {
    for (text, span) in [
        (
            "80".repeat(MAX_BYTES) + "81",
            2 * MAX_BYTES..2 * MAX_BYTES + 2,
        ),
        (
            "80".repeat(MAX_BYTES) + " 41/DF",
            2 * MAX_BYTES + 1..2 * MAX_BYTES + 6,
        ),
        (
            "?? ".repeat(MAX_BYTES) + "8?",
            3 * MAX_BYTES..3 * MAX_BYTES + 2,
        ),
    ] {
        let e = err(&text);
        assert_eq!(*e.kind(), ParseErrorKind::TooManyBytes { limit: MAX_BYTES });
        assert_eq!(e.span(), span);
        assert_eq!(e.column() as usize, span.start + 1);
        assert_eq!(e.code(), codes::PATTERN_TOO_LONG);
        let d = e.to_diagnostic_at("t", 1, 1);
        assert_eq!(d.severity(), Severity::Error);
        assert_eq!(d.code(), codes::PATTERN_TOO_LONG);
    }
}

/// The widest canonical text, the most bytes each with a `/` mask at the
/// largest offset, fits in the text limit, so every parsed pattern prints
/// as text that parses back.
#[test]
fn the_widest_canonical_text_fits_the_text_limit() {
    let p = BytePattern::new(
        usize::MAX - MAX_BYTES,
        &[0x28; MAX_BYTES],
        &[0x3C; MAX_BYTES],
    )
    .unwrap();
    let text = p.to_string();
    assert!(text.len() <= MAX_TEXT_LEN, "{}", text.len());
    assert_eq!(ok(&text), p);
}

#[test]
fn offsets_up_to_the_last_address_parse() {
    let last = usize::MAX - 1;
    assert_eq!(ok(&format!("@{last} 80")).offset(), last);
    assert_eq!(ok(&format!("@{last:#x} 80")).offset(), last);
    assert_eq!(
        ok(&format!("@{} 80 81", usize::MAX - 2)).offset(),
        usize::MAX - 2
    );
    let p = ok(&format!("@{last} 80"));
    assert_eq!(ok(&p.to_string()), p);
}

// ---------------------------------------------------------------- malformed

/// Each malformed class: kind, byte span, 1-based column and code.
#[test]
fn malformed_text_is_a_located_error_with_its_code() {
    use ParseErrorKind as K;
    let max = usize::MAX;
    let too_large = format!("@{max}0 80");
    let overflow = format!("@{max} 80");
    let overflow_hex = format!("@{max:#X}  80 81");
    let cases: Vec<(&str, K, Range<usize>, u32, Code)> = vec![
        ("", K::Empty, 0..0, 1, codes::PATTERN_SYNTAX),
        ("   \t ", K::Empty, 0..5, 1, codes::PATTERN_SYNTAX),
        ("@0x10", K::Empty, 0..5, 1, codes::PATTERN_SYNTAX),
        (
            "80 G0",
            K::UnexpectedChar('G'),
            3..4,
            4,
            codes::PATTERN_SYNTAX,
        ),
        (
            "80 0G",
            K::UnexpectedChar('G'),
            4..5,
            5,
            codes::PATTERN_SYNTAX,
        ),
        (
            "80,F0",
            K::UnexpectedChar(','),
            2..3,
            3,
            codes::PATTERN_SYNTAX,
        ),
        (
            "80\nF0",
            K::UnexpectedChar('\n'),
            2..3,
            3,
            codes::PATTERN_SYNTAX,
        ),
        (
            "ü80",
            K::UnexpectedChar('ü'),
            0..2,
            1,
            codes::PATTERN_SYNTAX,
        ),
        (
            "80 éé",
            K::UnexpectedChar('é'),
            3..5,
            4,
            codes::PATTERN_SYNTAX,
        ),
        (
            "é 8x",
            K::UnexpectedChar('é'),
            0..2,
            1,
            codes::PATTERN_SYNTAX,
        ),
        (
            "80 /F0",
            K::UnexpectedChar('/'),
            3..4,
            4,
            codes::PATTERN_SYNTAX,
        ),
        (
            "0x0x12",
            K::UnexpectedChar('x'),
            3..4,
            4,
            codes::PATTERN_SYNTAX,
        ),
        ("80 F", K::IncompleteByte, 3..4, 4, codes::PATTERN_SYNTAX),
        ("80F", K::IncompleteByte, 2..3, 3, codes::PATTERN_SYNTAX),
        ("?", K::IncompleteByte, 0..1, 1, codes::PATTERN_SYNTAX),
        ("0x1", K::IncompleteByte, 2..3, 3, codes::PATTERN_SYNTAX),
        ("80 0x", K::EmptyHexPrefix, 3..5, 4, codes::PATTERN_SYNTAX),
        ("0X 80", K::EmptyHexPrefix, 0..2, 1, codes::PATTERN_SYNTAX),
        ("80/", K::IncompleteMask, 2..3, 3, codes::PATTERN_SYNTAX),
        ("80/F 81", K::IncompleteMask, 2..4, 3, codes::PATTERN_SYNTAX),
        (
            "80/FG",
            K::UnexpectedChar('G'),
            4..5,
            5,
            codes::PATTERN_SYNTAX,
        ),
        (
            "80/?F",
            K::UnexpectedChar('?'),
            3..4,
            4,
            codes::PATTERN_SYNTAX,
        ),
        (
            "80/F0/F0",
            K::UnexpectedChar('/'),
            5..6,
            6,
            codes::PATTERN_SYNTAX,
        ),
        (
            "01 8?/F0",
            K::MaskOnWildcard,
            3..6,
            4,
            codes::PATTERN_SYNTAX,
        ),
        ("??/00", K::MaskOnWildcard, 0..3, 1, codes::PATTERN_SYNTAX),
        ("?1/0F", K::MaskOnWildcard, 0..3, 1, codes::PATTERN_SYNTAX),
        (
            "00 41/0F",
            K::ValueOutsideMask {
                value: 0x41,
                mask: 0x0F,
            },
            3..8,
            4,
            codes::PATTERN_SYNTAX,
        ),
        (
            "FF/FE",
            K::ValueOutsideMask {
                value: 0xFF,
                mask: 0xFE,
            },
            0..5,
            1,
            codes::PATTERN_SYNTAX,
        ),
        ("@ 80", K::BadOffset, 0..1, 1, codes::PATTERN_BAD_OFFSET),
        ("@0x 80", K::BadOffset, 0..3, 1, codes::PATTERN_BAD_OFFSET),
        ("@x10 80", K::BadOffset, 0..4, 1, codes::PATTERN_BAD_OFFSET),
        ("@12a 80", K::BadOffset, 0..4, 1, codes::PATTERN_BAD_OFFSET),
        ("@0x1G 80", K::BadOffset, 0..5, 1, codes::PATTERN_BAD_OFFSET),
        ("@+5 80", K::BadOffset, 0..3, 1, codes::PATTERN_BAD_OFFSET),
        ("@0x+5 80", K::BadOffset, 0..5, 1, codes::PATTERN_BAD_OFFSET),
        ("@-1 80", K::BadOffset, 0..3, 1, codes::PATTERN_BAD_OFFSET),
        ("@é 80", K::BadOffset, 0..3, 1, codes::PATTERN_BAD_OFFSET),
        (
            "80 @1",
            K::MisplacedOffset,
            3..5,
            4,
            codes::PATTERN_BAD_OFFSET,
        ),
        (
            "@1 @2 80",
            K::MisplacedOffset,
            3..5,
            4,
            codes::PATTERN_BAD_OFFSET,
        ),
        (
            "@1 80 @x",
            K::MisplacedOffset,
            6..8,
            7,
            codes::PATTERN_BAD_OFFSET,
        ),
        (
            &too_large,
            K::OffsetTooLarge,
            0..too_large.len() - 3,
            1,
            codes::PATTERN_BAD_OFFSET,
        ),
        (
            "@0x1FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF 80",
            K::OffsetTooLarge,
            0..36,
            1,
            codes::PATTERN_BAD_OFFSET,
        ),
        (
            &overflow,
            K::OffsetOverflow {
                offset: max,
                len: 1,
            },
            0..overflow.len() - 3,
            1,
            codes::PATTERN_BAD_OFFSET,
        ),
        (
            &overflow_hex,
            K::OffsetOverflow {
                offset: max,
                len: 2,
            },
            0..overflow_hex.len() - 7,
            1,
            codes::PATTERN_BAD_OFFSET,
        ),
    ];
    for (text, kind, span, column, code) in cases {
        let e = err(text);
        assert_eq!(*e.kind(), kind, "{text:?}");
        assert_eq!(e.span(), span, "{text:?}");
        assert_eq!(e.column(), column, "{text:?}");
        assert_eq!(e.code(), code, "{text:?}");
        assert_eq!(kind.code(), code, "{text:?}");
        assert!(
            text.get(e.span()).is_some(),
            "{text:?}: span on char boundaries"
        );
        let d = e.to_diagnostic_at("t", 1, 1);
        assert_eq!(d.severity(), Severity::Error, "{text:?}");
        assert_eq!(d.code(), code, "{text:?}");
        assert_eq!(d.provenance().span(), Span::line_col(1, column), "{text:?}");
    }
}

#[test]
fn overflow_points_at_the_offset_even_after_the_bytes() {
    // The offset is fine on its own; the bytes after it push the end past
    // the last address. The error is the offset's.
    let text = format!("@{} 80 81 82", usize::MAX - 2);
    let e = err(&text);
    assert_eq!(
        *e.kind(),
        ParseErrorKind::OffsetOverflow {
            offset: usize::MAX - 2,
            len: 3
        }
    );
    assert_eq!(e.span(), 0..text.len() - 9);
}

#[test]
fn text_over_the_limit_is_refused_before_it_is_read() {
    let text = "80".repeat(MAX_TEXT_LEN / 2) + "8";
    let e = err(&text);
    assert_eq!(
        *e.kind(),
        ParseErrorKind::TooLong {
            len: MAX_TEXT_LEN + 1,
            limit: MAX_TEXT_LEN
        }
    );
    assert_eq!(e.span(), 0..MAX_TEXT_LEN + 1);
    assert_eq!(e.column(), 1);
    assert_eq!(e.code(), codes::PATTERN_TOO_LONG);
    assert_eq!(e.to_diagnostic_at("t", 1, 1).severity(), Severity::Error);
    // Refused whatever it holds: garbage that would otherwise be a syntax
    // error at byte 0 is still `TooLong`.
    let junk = "G".repeat(MAX_TEXT_LEN + 1);
    assert!(matches!(err(&junk).kind(), ParseErrorKind::TooLong { .. }));
}

#[test]
fn every_kind_has_its_message() {
    use ParseErrorKind as K;
    let cases = [
        (K::Empty, "the byte pattern has no bytes".to_owned()),
        (
            K::UnexpectedChar('G'),
            "unexpected character 'G'; a byte is two hex digits or `?` wildcards".to_owned(),
        ),
        (
            K::IncompleteByte,
            "incomplete byte; a byte is two hex digits or `?` wildcards".to_owned(),
        ),
        (
            K::EmptyHexPrefix,
            "`0x` is not followed by any hex digits".to_owned(),
        ),
        (
            K::IncompleteMask,
            "incomplete mask; a `/` mask is two hex digits".to_owned(),
        ),
        (
            K::MaskOnWildcard,
            "a byte with a `?` wildcard cannot also have a `/` mask".to_owned(),
        ),
        (
            K::ValueOutsideMask {
                value: 0x41,
                mask: 0x0F,
            },
            "byte 41 has bits set outside its mask 0F".to_owned(),
        ),
        (
            K::BadOffset,
            "an offset is `@` then decimal digits, or `@0x` then hex digits".to_owned(),
        ),
        (
            K::MisplacedOffset,
            "the `@` offset must come once, before the bytes".to_owned(),
        ),
        (
            K::OffsetTooLarge,
            format!(
                "the offset is larger than the largest address, {:#X}",
                usize::MAX
            ),
        ),
        (
            K::OffsetOverflow {
                offset: 0xFF,
                len: 3,
            },
            "3 bytes at offset 0xFF end past the largest address".to_owned(),
        ),
        (
            K::TooManyBytes { limit: 512 },
            "the pattern has more than 512 bytes; the limit is 512".to_owned(),
        ),
        (
            K::TooLong {
                len: 5000,
                limit: 4096,
            },
            "the pattern text is 5000 bytes long; the limit is 4096 bytes".to_owned(),
        ),
    ];
    for (kind, message) in cases {
        assert_eq!(kind.to_string(), message, "{kind:?}");
    }
    let e = err("80 G0");
    assert_eq!(e.to_string(), e.kind().to_string());
    let as_error: &dyn core::error::Error = &e;
    assert!(as_error.source().is_none());
}

#[test]
fn errors_become_error_diagnostics_at_the_column() {
    let e = err("80 F0 ?? 0");
    assert_eq!(e.column(), 10);
    let d = e.to_diagnostic(Provenance::new("defs/base.xml", Span::line_col(3, 20)));
    assert_eq!(d.severity(), Severity::Error);
    assert_eq!(d.code(), codes::PATTERN_SYNTAX);
    assert_eq!(
        d.to_string(),
        "defs/base.xml:3:20: error[PAT0001]: \
         incomplete byte; a byte is two hex digits or `?` wildcards"
    );
    // The text starts at 3:20, so its column 10 is the file's column 29.
    let d = e.to_diagnostic_at("defs/base.xml", 3, 20);
    assert_eq!(d.provenance().span(), Span::line_col(3, 29));
    assert_eq!(d.code(), codes::PATTERN_SYNTAX);
    assert_eq!(d.message(), e.to_string());
    assert_eq!(d.provenance().source(), "defs/base.xml");
    // Column 1 of the text is the text's own start.
    let d = err("G").to_diagnostic_at("p.toml", 7, 12);
    assert_eq!(d.provenance().span(), Span::line_col(7, 12));
    // Never overflows.
    let d = e.to_diagnostic_at("p.toml", 1, u32::MAX);
    assert_eq!(d.provenance().span(), Span::line_col(1, u32::MAX));
}

#[test]
fn display_writes_each_mask_form() {
    let p = BytePattern::new(
        0x10,
        &[0xAB, 0xAB, 0xAB, 0xAB, 0xAB],
        &[0xFF, 0x00, 0xF0, 0x0F, 0x3C],
    )
    .unwrap();
    assert_eq!(p.to_string(), "@0x10 AB ?? A? ?B 28/3C");
    assert_eq!(ok(&p.to_string()), p);
    assert_eq!(BytePattern::exact(0, &[0x0A]).unwrap().to_string(), "0A");
    assert_eq!(
        BytePattern::exact(1, &[0x00]).unwrap().to_string(),
        "@0x1 00"
    );
}

// ---------------------------------------------------------------- properties

/// Any byte pattern: offset anywhere (including the top of the address
/// space), any bytes under any masks.
fn any_pattern() -> impl Strategy<Value = BytePattern> {
    (
        proptest::collection::vec((any::<u8>(), any::<u8>()), 1..24),
        prop_oneof![0usize..0x10000, any::<usize>()],
    )
        .prop_map(|(cells, offset)| {
            let (bytes, mask): (Vec<u8>, Vec<u8>) = cells.into_iter().unzip();
            let offset = offset.min(usize::MAX - bytes.len());
            BytePattern::new(offset, &bytes, &mask).unwrap()
        })
}

/// Text built mostly from the pattern alphabet, so the parser's deeper
/// paths are reached, plus some characters outside it.
fn patternish_text() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            8 => prop::sample::select(vec![
                '0', '1', '7', '8', '9', 'a', 'A', 'f', 'F', 'x', 'X', '?', '/', '@', ' ', '\t',
            ]),
            1 => any::<char>(),
        ],
        0..40,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

fn assert_error_is_well_formed(text: &str, e: &ParseError) -> Result<(), TestCaseError> {
    let span = e.span();
    prop_assert!(span.start <= span.end && span.end <= text.len());
    prop_assert!(text.get(span.clone()).is_some(), "span off char boundary");
    prop_assert!(!span.is_empty() || text.is_empty());
    let before = text.get(..span.start).unwrap_or("");
    prop_assert_eq!(e.column() as usize, before.chars().count() + 1);
    prop_assert_eq!(e.code(), e.kind().code());
    prop_assert!(e.code().as_str().starts_with("PAT"));
    let d = e.to_diagnostic_at("t", 1, 1);
    prop_assert_eq!(d.severity(), Severity::Error);
    prop_assert_eq!(d.code(), e.code());
    prop_assert_eq!(d.provenance().span(), Span::line_col(1, e.column()));
    Ok(())
}

proptest! {
    /// Every byte pattern prints as text that parses back to it, and that
    /// text is canonical: printing the parsed pattern gives it again.
    #[test]
    fn display_parses_back_to_the_same_pattern(p in any_pattern()) {
        let text = p.to_string();
        let back = BytePattern::parse(&text);
        prop_assert_eq!(back.as_ref(), Ok(&p), "{}", text);
        prop_assert_eq!(back.map(|b| b.to_string()), Ok(text));
    }

    /// Spelling variants of a pattern (lower case, run-together pairs, a
    /// `0x` prefix, decimal offset, extra blanks) parse to the same pattern.
    #[test]
    fn spelling_variants_parse_alike(p in any_pattern(), gap in "[ \t]{1,3}") {
        let canonical = p.to_string();
        let lower = canonical.to_lowercase();
        prop_assert_eq!(BytePattern::parse(&lower), Ok(p.clone()));
        let words: Vec<&str> = canonical.split(' ').collect();
        let (offset, cells) = match words.split_first() {
            Some((w, rest)) if w.starts_with('@') => (format!("@{}", p.offset()), rest),
            _ => (String::new(), &words[..]),
        };
        let joined = format!("{gap}{offset}{gap}0x{}{gap}", cells.concat());
        prop_assert_eq!(BytePattern::parse(&joined), Ok(p), "{}", joined);
    }

    /// No text makes the parser panic; every error is located inside the
    /// text on character boundaries with a consistent column and code.
    #[test]
    fn parsing_never_panics_and_errors_are_located(
        text in prop_oneof![patternish_text(), any::<String>()],
    ) {
        match BytePattern::parse(&text) {
            Ok(p) => {
                prop_assert!(p.len() >= 1);
                prop_assert!(p.offset().checked_add(p.len()).is_some());
                prop_assert_eq!(BytePattern::parse(&p.to_string()), Ok(p));
            }
            Err(e) => assert_error_is_well_formed(&text, &e)?,
        }
    }

    /// One bad character spliced into valid text is reported at that
    /// character (or, when it splits a byte, at the byte it splits).
    #[test]
    fn a_spliced_bad_character_is_found(
        p in any_pattern(),
        at in any::<prop::sample::Index>(),
        bad in prop::sample::select(vec!['G', 'z', ',', ';', '\n', 'é', '-']),
    ) {
        let text = p.to_string();
        let at = at.index(text.len() + 1);
        let mut spliced = text.clone();
        spliced.insert(at, bad);
        let e = BytePattern::parse(&spliced).unwrap_err();
        assert_error_is_well_formed(&spliced, &e)?;
        prop_assert_eq!(e.code(), e.kind().code());
        prop_assert!(e.span().start <= at, "{:?} error {:?} at {:?}", spliced, e, e.span());
    }
}
