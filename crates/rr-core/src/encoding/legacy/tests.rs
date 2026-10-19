//! Unit tests for the legacy definition parsers (US-RR-19-7).
//!
//! Citations are to `legacy/src/main/java/com/romraider/`.

use super::*;
use crate::diag::{Severity, Span, codes};
use crate::encoding::{BitFieldProblem, CombinationProblem};

fn storage_problem_of(text: &str) -> StorageProblem {
    match parse_storage_type(text) {
        Err(EncodingError::UnsupportedStorage { text: t, problem }) => {
            assert_eq!(t, text, "the error keeps the text as written");
            problem
        }
        other => panic!("{text:?}: expected a refusal, got {other:?}"),
    }
}

#[test]
fn accepts_every_legacy_integer_storage_spelling() {
    // xml/RomAttributeParser.java:62-63 (`uint` prefix) and :65-66 (`int`),
    // matched case-sensitively; signedness per :73-81.
    for (text, storage) in [
        ("uint8", Storage::U8),
        ("uint16", Storage::U16),
        ("uint24", Storage::U24),
        ("uint32", Storage::U32),
        ("uint64", Storage::U64),
        ("int8", Storage::I8),
        ("int16", Storage::I16),
        ("int24", Storage::I24),
        ("int32", Storage::I32),
        ("int64", Storage::I64),
    ] {
        assert_eq!(parse_storage_type(text), Ok(storage), "{text}");
    }
}

#[test]
fn accepts_float_in_any_case() {
    // xml/RomAttributeParser.java:59-60 (equalsIgnoreCase); Settings.java:124.
    for text in ["float", "FLOAT", "Float", "fLoAt"] {
        assert_eq!(parse_storage_type(text), Ok(Storage::F32), "{text}");
    }
}

#[test]
fn accepts_movi20_in_any_case() {
    // xml/RomAttributeParser.java:53-54 (equalsIgnoreCase).
    for text in ["movi20", "MOVI20", "Movi20"] {
        assert_eq!(parse_storage_type(text), Ok(Storage::U20), "{text}");
    }
}

#[test]
fn refuses_movi20s() {
    // xml/RomAttributeParser.java:56-57: accepted by legacy, unsupported here.
    for text in ["movi20s", "MOVI20S", "Movi20S"] {
        assert_eq!(storage_problem_of(text), StorageProblem::Movi20s, "{text}");
    }
}

#[test]
fn refuses_legacy_numeric_codes() {
    // xml/RomAttributeParser.java:68-69: Integer.parseInt fallback (byte
    // counts and the magic 20 / 28 / 99 of Settings.java:122-124).
    for text in ["0", "1", "2", "3", "4", "8", "20", "28", "99"] {
        assert_eq!(
            storage_problem_of(text),
            StorageProblem::NumericCode,
            "{text}"
        );
    }
}

#[test]
fn refuses_the_empty_storage_type() {
    assert_eq!(storage_problem_of(""), StorageProblem::Empty);
}

#[test]
fn refuses_unknown_and_misspelt_storage_types() {
    for text in [
        // Case: legacy's `uint`/`int` prefixes were case-sensitive (it threw).
        "UINT16", "Uint8", "INT8", "Int16", // Widths that do not exist.
        "uint48", "uint12", "int1", "uint", "int", // Spellings legacy's parseInt took.
        "uint016", "int+16", "uint-8", // Whitespace is not trimmed by legacy.
        " uint8", "uint8 ", "float ", " movi20", // Typos and other dialects.
        "unit16", "float32", "float64", "double", "u8", "f32", "i24", "bloblist", "-1", "1.5",
    ] {
        assert_eq!(storage_problem_of(text), StorageProblem::Unknown, "{text}");
    }
}

#[test]
fn unsupported_storage_is_enc0001() {
    let err = parse_storage_type("uint48").unwrap_err();
    assert_eq!(err.code(), codes::UNSUPPORTED_STORAGE_TYPE);
}

#[test]
fn accepts_big_and_little_in_any_case() {
    // xml/RomAttributeParser.java:37 and :40 (equalsIgnoreCase).
    for text in ["big", "BIG", "Big"] {
        assert_eq!(parse_endian(text), Ok(Endian::Big), "{text}");
    }
    for text in ["little", "LITTLE", "Little"] {
        assert_eq!(parse_endian(text), Ok(Endian::Little), "{text}");
    }
}

#[test]
fn refuses_every_other_endian() {
    // Legacy defaulted these to little-endian (RomAttributeParser.java:43-45);
    // "1" / "2" are its marshalling codes (Settings.java:94-95).
    for text in [
        "",
        "1",
        "2",
        "middle",
        " big",
        "little ",
        "be",
        "le",
        "bigendian",
    ] {
        assert_eq!(
            parse_endian(text),
            Err(EncodingError::UnknownEndian { text: text.into() }),
            "{text:?}"
        );
    }
    assert_eq!(parse_endian("x").unwrap_err().code(), codes::UNKNOWN_ENDIAN);
}

#[test]
fn parses_hex_masks() {
    // maps/Table.java:537-541: ByteUtil.parseUnsignedInt(mask, 16).
    assert_eq!(parse_mask(""), Ok(None));
    assert_eq!(parse_mask("0"), Ok(Some(0)));
    assert_eq!(parse_mask("1"), Ok(Some(1)));
    assert_eq!(parse_mask("FF"), Ok(Some(0xFF)));
    assert_eq!(parse_mask("ff"), Ok(Some(0xFF)));
    assert_eq!(parse_mask("0FF0"), Ok(Some(0x0FF0)));
    assert_eq!(parse_mask("80000000"), Ok(Some(0x8000_0000)));
    assert_eq!(parse_mask("FFFFFFFFFFFFFFFF"), Ok(Some(u64::MAX)));
}

#[test]
fn refuses_malformed_masks() {
    for text in [
        "0xFF",
        "0XFF",
        "+F",
        "-1",
        " F",
        "F ",
        "G",
        "F_F",
        "10000000000000000",
    ] {
        assert_eq!(
            parse_mask(text),
            Err(EncodingError::UnparsableMask { text: text.into() }),
            "{text:?}"
        );
    }
}

#[test]
fn parses_data_layouts() {
    // maps/Table.java:520-523 (trimmed, equalsIgnoreCase "bosch_subtract");
    // the attribute defaults to "" (xml/TableScaleUnmarshaller.java:150);
    // DEFAULT is the enum name (maps/Table.java:81).
    for text in [
        "bosch_subtract",
        "BOSCH_SUBTRACT",
        "Bosch_Subtract",
        " bosch_subtract ",
    ] {
        assert_eq!(
            parse_data_layout(text),
            Ok(Layout::BoschSubtract),
            "{text:?}"
        );
    }
    for text in ["", "   ", "default", "DEFAULT", " Default "] {
        assert_eq!(parse_data_layout(text), Ok(Layout::Default), "{text:?}");
    }
}

#[test]
fn refuses_unknown_data_layouts() {
    // Legacy turned these into DEFAULT (maps/Table.java:524-526).
    for text in ["bosch-subtract", "boschsubtract", "subtract", " zigzag "] {
        assert_eq!(
            parse_data_layout(text),
            Err(EncodingError::UnknownLayout { text: text.into() }),
            "{text:?}"
        );
    }
    assert_eq!(
        parse_data_layout("x").unwrap_err().code(),
        codes::UNKNOWN_LAYOUT
    );
}

fn here() -> Provenance {
    Provenance::new("defs/16BITBASE.xml", Span::line_col(42, 7)).with_layer("base")
}

fn attrs<'a>(storage_type: &'a str, endian: &'a str) -> LegacyAttributes<'a> {
    LegacyAttributes {
        storage_type,
        endian,
        mask: None,
        data_layout: None,
    }
}

fn codes_of(loaded: &Loaded<Option<Encoding>>) -> Vec<&'static str> {
    loaded
        .diagnostics()
        .iter()
        .map(|d| {
            assert_eq!(d.severity(), Severity::Error);
            assert_eq!(d.provenance(), &here());
            d.code().as_str()
        })
        .collect()
}

#[test]
fn parse_encoding_builds_a_plain_encoding() {
    let loaded = parse_encoding(&attrs("int16", "little"), &here());
    assert!(loaded.diagnostics().is_empty());
    let enc = loaded.value().copied().flatten().unwrap();
    assert_eq!(
        enc,
        Encoding::new(Storage::I16, Endian::Little, None, Layout::Default).unwrap()
    );
}

#[test]
fn parse_encoding_builds_bitfield_and_layout() {
    let loaded = parse_encoding(
        &LegacyAttributes {
            storage_type: "uint16",
            endian: "big",
            mask: Some("0FF0"),
            data_layout: Some("bosch_subtract"),
        },
        &here(),
    );
    assert!(loaded.diagnostics().is_empty());
    let enc = loaded.value().copied().flatten().unwrap();
    assert_eq!(enc.storage(), Storage::U16);
    assert_eq!(enc.endian(), Endian::Big);
    assert_eq!(enc.layout(), Layout::BoschSubtract);
    let field = enc.bitfield().unwrap();
    assert_eq!((field.mask(), field.shift()), (0x0FF0, 4));
    assert!(!field.is_signed());
}

#[test]
fn parse_encoding_empty_mask_and_layout_are_the_defaults() {
    let loaded = parse_encoding(
        &LegacyAttributes {
            storage_type: "float",
            endian: "little",
            mask: Some(""),
            data_layout: Some(""),
        },
        &here(),
    );
    let enc = loaded.value().copied().flatten().unwrap();
    assert_eq!(enc.storage(), Storage::F32);
    assert_eq!(enc.bitfield(), None);
    assert_eq!(enc.layout(), Layout::Default);
}

#[test]
fn unsupported_storage_is_an_error_diagnostic_never_a_zero_value() {
    // MF-02: the table is not decoded at all, so nothing can read as zero.
    let loaded = parse_encoding(&attrs("uint48", "big"), &here());
    assert_eq!(codes_of(&loaded), ["ENC0001"]);
    assert_eq!(
        loaded.diagnostics()[0].message(),
        "unsupported storage type \"uint48\": not a supported storage type"
    );
    assert!(loaded.has_errors());
    assert_eq!(loaded.value(), None);
    let (value, diags) = loaded.accept_errors();
    assert_eq!(
        value, None,
        "no default encoding even when errors are accepted"
    );
    assert_eq!(diags.len(), 1);
}

#[test]
fn parse_encoding_reports_every_bad_attribute_in_order() {
    let loaded = parse_encoding(
        &LegacyAttributes {
            storage_type: "unit16",
            endian: "2",
            mask: Some("0xFF"),
            data_layout: Some("zigzag"),
        },
        &here(),
    );
    assert_eq!(
        codes_of(&loaded),
        ["ENC0001", "ENC0002", "ENC0003", "ENC0004"]
    );
    assert_eq!(loaded.accept_errors().0, None);
}

#[test]
fn parse_encoding_reports_a_bad_endian_alone() {
    let loaded = parse_encoding(&attrs("uint8", "middle"), &here());
    assert_eq!(codes_of(&loaded), ["ENC0002"]);
    assert_eq!(loaded.accept_errors().0, None);
}

#[test]
fn parse_encoding_reports_a_bad_layout_alone() {
    let loaded = parse_encoding(
        &LegacyAttributes {
            data_layout: Some("zigzag"),
            ..attrs("uint8", "big")
        },
        &here(),
    );
    assert_eq!(codes_of(&loaded), ["ENC0004"]);
    assert_eq!(loaded.accept_errors().0, None);
}

#[test]
fn parse_encoding_refuses_invalid_bitfields() {
    for (storage_type, mask, problem) in [
        ("uint8", "0", BitFieldProblem::ZeroMask),
        (
            "uint8",
            "100",
            BitFieldProblem::OutsideStorage { storage_bits: 8 },
        ),
        ("float", "F", BitFieldProblem::FloatStorage),
    ] {
        let loaded = parse_encoding(
            &LegacyAttributes {
                mask: Some(mask),
                ..attrs(storage_type, "big")
            },
            &here(),
        );
        assert_eq!(codes_of(&loaded), ["ENC0003"], "{storage_type} {mask}");
        assert!(
            loaded.diagnostics()[0]
                .message()
                .ends_with(&problem.to_string()),
            "{storage_type} {mask}: {}",
            loaded.diagnostics()[0].message()
        );
        assert_eq!(loaded.accept_errors().0, None);
    }
}

#[test]
fn parse_encoding_refuses_unsupported_combinations() {
    let little_movi20 = parse_encoding(&attrs("movi20", "little"), &here());
    assert_eq!(codes_of(&little_movi20), ["ENC0001"]);
    assert!(
        little_movi20.diagnostics()[0]
            .message()
            .ends_with(&CombinationProblem::U20LittleEndian.to_string())
    );
    assert_eq!(little_movi20.accept_errors().0, None);

    let bosch_float = parse_encoding(
        &LegacyAttributes {
            data_layout: Some("bosch_subtract"),
            ..attrs("float", "big")
        },
        &here(),
    );
    assert_eq!(codes_of(&bosch_float), ["ENC0001"]);
    assert_eq!(bosch_float.accept_errors().0, None);
}

#[test]
fn a_storage_refusal_is_not_followed_by_combination_noise() {
    // With the storage unknown, its mask cannot be checked against it.
    let loaded = parse_encoding(
        &LegacyAttributes {
            mask: Some("100"),
            ..attrs("uint48", "little")
        },
        &here(),
    );
    assert_eq!(codes_of(&loaded), ["ENC0001"]);
}

#[test]
fn two_encodings_with_different_byte_orders_coexist() {
    // MF-04: parsing one definition never changes another's byte order.
    let big = parse_encoding(&attrs("uint32", "big"), &here());
    let little = parse_encoding(&attrs("uint32", "little"), &here());
    let big = big.value().copied().flatten().unwrap();
    let little = little.value().copied().flatten().unwrap();
    assert_eq!(big.endian(), Endian::Big);
    assert_eq!(little.endian(), Endian::Little);
}
