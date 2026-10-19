//! Tests for the schema-1 KAT fixture types (`rr_testkit::kat`, US-RR-9-5).
//!
//! Parses a representative fixture of each shape (decode values, checksum)
//! from `tests/data/kat/`, and proves that structurally broken fixtures —
//! malformed hex, unknown schema, a missing `source`, a `hand-calc` without
//! `working`, unknown keys, ambiguous `[definition]` / `[input]` — are
//! rejected with an error that names the file and the problem.

use std::path::PathBuf;

use rr_testkit::fixtures::{FixtureError, SUPPORTED_SCHEMA, Tier};
use rr_testkit::kat::{
    self, Bitfield, ChecksumState, Definition, DefinitionRef, Encoding, Endian, HexBytes, Input,
    KatKind, Layout, RawValue, Snippet, SnippetFormat, Source, Storage,
};

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join("kat")
        .join(name)
}

/// Parses `text` as `bad.toml` and returns the error message, asserting it is
/// a [`FixtureError::Shape`] naming the file.
#[track_caller]
fn shape_error(text: &str) -> String {
    let result = kat::parse("bad.toml", text);
    assert!(result.is_err(), "fixture unexpectedly parsed: {result:?}");
    let Err(error) = result else {
        return String::new();
    };
    assert!(matches!(error, FixtureError::Shape { .. }), "{error:?}");
    let message = error.to_string();
    assert!(message.contains("bad.toml"), "{message}");
    message
}

const DECODE_HEAD: &str = r#"
schema = 1
title = "t"
[definition.encoding]
storage = "u8"
endian = "big"
[input]
hex = "7F"
"#;

const HAND_CALC: &str = r#"{ kind = "hand-calc", working = "0x7F = 127" }"#;

#[test]
fn parses_decode_values_fixture() {
    let fixture = kat::load(data("decode_u16_be.toml")).unwrap();
    assert_eq!(fixture.schema, SUPPORTED_SCHEMA);
    let kat = fixture.data;
    assert_eq!(kat.title, "u16 big-endian, linear 0.1 scaling");
    assert!(kat.notes.is_some());
    assert_eq!(
        kat.input,
        Input::Hex(HexBytes(vec![0x01, 0xF4, 0xFF, 0x38]))
    );
    let KatKind::Decode { definition, cases } = kat.kind else {
        panic!("expected a decode fixture, got {:?}", kat.kind);
    };
    let Definition::Inline {
        encoding,
        conversion,
    } = definition
    else {
        panic!("expected an inline definition, got {definition:?}");
    };
    assert_eq!(
        encoding,
        Encoding {
            storage: Storage::U16,
            endian: Endian::Big,
            layout: Layout::Default,
            bitfield: None,
        }
    );
    let conversion = conversion.unwrap();
    assert_eq!(conversion.to_real, "x * 0.1");
    assert_eq!(conversion.to_raw.as_deref(), Some("x / 0.1"));
    assert_eq!(conversion.units.as_deref(), Some("degC"));

    assert_eq!(cases.len(), 2);
    assert_eq!(cases[0].offset, 0);
    assert_eq!(cases[0].raw, RawValue::Int(500));
    assert_eq!(cases[0].real, Some(50.0));
    assert_eq!(cases[0].tolerance, None);
    assert_eq!(cases[0].source.kind(), "hand-calc");
    assert!(matches!(&cases[0].source, Source::HandCalc { working } if working.contains("= 500")));
    assert_eq!(cases[1].offset, 2);
    assert_eq!(cases[1].raw, RawValue::Int(0xFF38));
    assert_eq!(cases[1].real, Some(6533.6));
    assert_eq!(cases[1].tolerance, Some(1e-9));
    assert!(cases[1].note.is_some());

    assert_eq!(kat.changelog.len(), 1);
    let entry = &kat.changelog[0];
    assert_eq!(
        (entry.date.year, entry.date.month, entry.date.day),
        (2026, 10, 19)
    );
    assert_eq!(entry.reason, "initial values");
    assert_eq!(entry.author.as_deref(), Some("rr-testkit"));
}

#[test]
fn parses_checksum_fixture() {
    let kat = kat::load(data("checksum_sum16.toml")).unwrap().data;
    assert!(kat.changelog.is_empty());
    assert_eq!(kat.input, Input::Hex(HexBytes(vec![1, 2, 3, 4, 0, 0])));
    let KatKind::Checksum { definition, cases } = kat.kind else {
        panic!("expected a checksum fixture, got {:?}", kat.kind);
    };
    assert_eq!(definition, None);
    assert_eq!(cases.len(), 1);
    let case = &cases[0];
    assert_eq!(case.algorithm, "sum16-be");
    assert_eq!(
        case.params.get("end").and_then(toml::Value::as_integer),
        Some(4)
    );
    assert_eq!(case.offset, 4);
    assert!(matches!(
        &case.before,
        ChecksumState { hex, valid: false, source: Source::HandCalc { .. } } if hex.as_bytes() == [0, 0]
    ));
    assert!(matches!(
        &case.after,
        ChecksumState { hex, valid: true, source: Source::HandCalc { .. } } if hex.as_bytes() == [0x00, 0x0A]
    ));
}

#[test]
fn parses_rom_input_and_definition_reference() {
    let kat = kat::load(data("decode_rom_ref.toml")).unwrap().data;
    let Input::Rom(rom) = &kat.input else {
        panic!("expected a ROM input, got {:?}", kat.input);
    };
    assert_eq!(rom.tier, Tier::Corpus);
    assert_eq!(rom.path, PathBuf::from("roms/example.bin"));
    assert_eq!(
        rom.sha256.as_str(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!((rom.offset, rom.length), (0x1000, Some(8)));
    let KatKind::Decode { definition, cases } = kat.kind else {
        panic!("expected a decode fixture");
    };
    assert_eq!(
        definition,
        Definition::Reference(DefinitionRef {
            tier: Tier::Corpus,
            path: PathBuf::from("defs/example.xml"),
            item: Some("Example Table".to_owned()),
        })
    );
    assert_eq!(cases[0].raw, RawValue::Int(i128::from(u64::MAX)));
    assert_eq!(cases[0].offset, 0);
    assert!(matches!(&cases[0].source, Source::CrossSource { sources } if sources.len() == 2));
    assert_eq!(cases[1].raw, RawValue::Int(-1));
    assert_eq!(cases[1].source.kind(), "stock-rom");
}

#[test]
fn parses_snippet_bitfield_layout_and_every_source_kind() {
    let text = r#"
schema = 1
title = "snippet"
[definition]
snippet = { format = "romraider-ecu-xml", text = "<scaling name='x' storagetype='uint8'/>" }
[input]
hex = "AB"
[[decode]]
raw = 1.5
source = { kind = "standard", doc = "SAE J1979", section = "PID 0x05" }
[[decode]]
raw = 2
source = { kind = "car-trace", trace = "traces/wrx-idle.rrtrace" }
[[decode]]
raw = 3
source = { kind = "stock-rom", rom = "A2WC510N" }
"#;
    let kat = kat::parse("snippet.toml", text).unwrap().data;
    let KatKind::Decode { definition, cases } = kat.kind else {
        panic!("expected decode");
    };
    assert_eq!(
        definition,
        Definition::Snippet(Snippet {
            format: SnippetFormat::RomraiderEcuXml,
            text: "<scaling name='x' storagetype='uint8'/>".to_owned(),
        })
    );
    assert_eq!(cases[0].raw, RawValue::Float(1.5));
    assert_eq!(
        cases.iter().map(|c| c.source.kind()).collect::<Vec<_>>(),
        ["standard", "car-trace", "stock-rom"]
    );

    let text = r#"
schema = 1
title = "bitfield"
[definition.encoding]
storage = "u20"
endian = "little"
layout = "bosch-subtract"
bitfield = { mask = "0xFFFF_FFFF_FFFF_0000" }
[input]
hex = "00 00 01"
[[decode]]
raw = 0
source = { kind = "hand-calc", working = "w" }
"#;
    let KatKind::Decode { definition, .. } = kat::parse("bitfield.toml", text).unwrap().data.kind
    else {
        panic!("expected decode");
    };
    let Definition::Inline {
        encoding,
        conversion,
    } = definition
    else {
        panic!("expected inline");
    };
    assert_eq!(conversion, None);
    assert_eq!(encoding.storage, Storage::U20);
    assert_eq!(encoding.endian, Endian::Little);
    assert_eq!(encoding.layout, Layout::BoschSubtract);
    assert_eq!(
        encoding.bitfield,
        Some(Bitfield {
            mask: 0xFFFF_FFFF_FFFF_0000,
            shift: None
        })
    );
    assert_eq!(encoding.bitfield.unwrap().effective_shift(), 16);
}

#[test]
fn rejects_malformed_hex() {
    for (hex, expected) in [
        ("7G", "not a hex digit"),
        ("0x7F", "not a hex digit"),
        ("7F 0", "odd number of hex digits"),
        ("", "no bytes"),
    ] {
        let text = DECODE_HEAD.replace(r#"hex = "7F""#, &format!("hex = {hex:?}"))
            + &format!("[[decode]]\nraw = 1\nsource = {HAND_CALC}\n");
        let message = shape_error(&text);
        assert!(message.contains(expected), "{hex:?}: {message}");
    }
    // Malformed hex in a checksum state, too.
    let text = r#"
schema = 1
title = "t"
[input]
hex = "00"
[[checksum]]
algorithm = "a"
offset = 0
before = { hex = "0", valid = false, source = { kind = "hand-calc", working = "w" } }
after = { hex = "00", valid = true, source = { kind = "hand-calc", working = "w" } }
"#;
    assert!(shape_error(text).contains("odd number of hex digits"));
}

#[test]
fn rejects_unknown_schema_version() {
    let text = DECODE_HEAD.replace("schema = 1", "schema = 2")
        + &format!("[[decode]]\nraw = 1\nsource = {HAND_CALC}\n");
    let error = kat::parse("future.toml", &text).unwrap_err();
    assert!(
        matches!(&error, FixtureError::UnsupportedSchema { found, supported, .. }
            if found == "2" && *supported == SUPPORTED_SCHEMA),
        "{error:?}"
    );
    assert!(error.to_string().contains("future.toml"));

    let text = DECODE_HEAD.replace("schema = 1\n", "");
    let error = kat::parse("none.toml", &text).unwrap_err();
    assert!(
        matches!(error, FixtureError::MissingSchema { .. }),
        "{error:?}"
    );
}

#[test]
fn rejects_expected_value_without_source() {
    let message = shape_error(&format!(
        "{DECODE_HEAD}[[decode]]\nraw = 127\nreal = 127.0\n"
    ));
    assert!(message.contains("missing field `source`"), "{message}");
    assert!(message.contains("in `decode`"), "{message}");

    let text = r#"
schema = 1
title = "t"
[input]
hex = "00 00"
[[checksum]]
algorithm = "a"
offset = 0
before = { hex = "00", valid = false }
after = { hex = "00", valid = true, source = { kind = "hand-calc", working = "w" } }
"#;
    assert!(shape_error(text).contains("missing field `source`"));
}

#[test]
fn rejects_hand_calc_without_working_and_unknown_source_kinds() {
    let message = shape_error(&format!(
        "{DECODE_HEAD}[[decode]]\nraw = 127\nsource = {{ kind = \"hand-calc\" }}\n"
    ));
    assert!(message.contains("missing field `working`"), "{message}");

    // "java" is not a source (D-024), nor is anything else unlisted.
    let message = shape_error(&format!(
        "{DECODE_HEAD}[[decode]]\nraw = 127\nsource = {{ kind = \"java\" }}\n"
    ));
    assert!(message.contains("unknown variant `java`"), "{message}");

    let message = shape_error(&format!(
        "{DECODE_HEAD}[[decode]]\nraw = 127\nsource = \"hand-calculated\"\n"
    ));
    assert!(
        message.contains("expected internally tagged enum Source")
            && message.contains("in `decode.source`"),
        "{message}"
    );

    let message = shape_error(&format!(
        "{DECODE_HEAD}[[decode]]\nraw = 127\nsource = {{ kind = \"standard\", doc = \"d\", section = \"s\", page = 3 }}\n"
    ));
    assert!(message.contains("unknown field `page`"), "{message}");
}

#[test]
fn rejects_unknown_keys_everywhere() {
    for (from, to, key) in [
        ("title = \"t\"", "title = \"t\"\nauthor = \"x\"", "author"),
        (
            "endian = \"big\"",
            "endian = \"big\"\nsigned = true",
            "signed",
        ),
        ("hex = \"7F\"", "hex = \"7F\"\nbytes = 1", "bytes"),
    ] {
        let text =
            DECODE_HEAD.replace(from, to) + &format!("[[decode]]\nraw = 1\nsource = {HAND_CALC}\n");
        let message = shape_error(&text);
        assert!(
            message.contains(&format!("unknown field `{key}`")),
            "{key}: {message}"
        );
    }
    let message = shape_error(&format!(
        "{DECODE_HEAD}[[decode]]\nraw = 1\nexpected = 2\nsource = {HAND_CALC}\n"
    ));
    assert!(message.contains("unknown field `expected`"), "{message}");
}

#[test]
fn rejects_unknown_storage_endian_and_layout() {
    for (from, to, bad) in [
        ("storage = \"u8\"", "storage = \"u128\"", "u128"),
        ("endian = \"big\"", "endian = \"middle\"", "middle"),
        (
            "endian = \"big\"",
            "endian = \"big\"\nlayout = \"bosch\"",
            "bosch",
        ),
    ] {
        let text =
            DECODE_HEAD.replace(from, to) + &format!("[[decode]]\nraw = 1\nsource = {HAND_CALC}\n");
        let message = shape_error(&text);
        assert!(
            message.contains(&format!("unknown variant `{bad}`")),
            "{bad}: {message}"
        );
    }
}

#[test]
fn rejects_ambiguous_shapes() {
    let case = format!("[[decode]]\nraw = 1\nsource = {HAND_CALC}\n");
    let checksum = r#"
[[checksum]]
algorithm = "a"
offset = 0
before = { hex = "00", valid = false, source = { kind = "hand-calc", working = "w" } }
after = { hex = "00", valid = true, source = { kind = "hand-calc", working = "w" } }
"#;
    let cases: [(String, &str); 9] = [
        (DECODE_HEAD.to_owned(), "has no cases"),
        (
            format!("{DECODE_HEAD}{case}{checksum}"),
            "both [[decode]] and [[checksum]]",
        ),
        (
            DECODE_HEAD.replace("title = \"t\"", "title = \"t\"\ndecode = []"),
            "`decode` is empty",
        ),
        (
            "schema = 1\ntitle = \"t\"\n[input]\nhex = \"00\"\n".to_owned() + &case,
            "needs a [definition]",
        ),
        (
            DECODE_HEAD.replace(
                "hex = \"7F\"",
                &format!(
                    "hex = \"7F\"\nrom = {{ tier = \"corpus\", path = \"r.bin\", sha256 = \"{}\" }}",
                    "0".repeat(64)
                ),
            ) + &case,
            "both `hex` and `rom`",
        ),
        (
            DECODE_HEAD.replace("hex = \"7F\"", "") + &case,
            "exactly one of `hex` or `rom`",
        ),
        (
            DECODE_HEAD.replace(
                "[input]",
                "[definition.snippet]\nformat = \"xdf\"\ntext = \"x\"\n[input]",
            ) + &case,
            "more than one of `encoding`, `snippet` and `reference`",
        ),
        (
            DECODE_HEAD.replace(
                "[definition.encoding]\nstorage = \"u8\"\nendian = \"big\"",
                "[definition.conversion]\nto_real = \"x\"",
            ) + &case,
            "`conversion` but no `encoding`",
        ),
        (
            DECODE_HEAD.replace(
                "[definition.encoding]\nstorage = \"u8\"\nendian = \"big\"",
                "[definition]\nreference = { tier = \"public\", path = \"d.xml\" }\n[definition.conversion]\nto_real = \"x\"",
            ) + &case,
            "only allowed with an inline `encoding`",
        ),
    ];
    for (text, expected) in cases {
        let message = shape_error(&text);
        assert!(message.contains(expected), "{expected}: {message}");
    }
}

#[test]
fn rejects_bad_values() {
    let case = format!("[[decode]]\nraw = 1\nsource = {HAND_CALC}\n");
    let rom = |sha: &str| {
        DECODE_HEAD.replace(
            "hex = \"7F\"",
            &format!("rom = {{ tier = \"corpus\", path = \"r.bin\", sha256 = \"{sha}\" }}"),
        ) + &case
    };
    assert!(shape_error(&rom("abc")).contains("expected exactly 64 hex digits"));
    assert!(kat::parse("ok.toml", &rom(&"0".repeat(64))).is_ok());

    let bitfield = |field: &str| {
        DECODE_HEAD.replace(
            "endian = \"big\"",
            &format!("endian = \"big\"\nbitfield = {{ {field} }}"),
        ) + &case
    };
    assert!(shape_error(&bitfield("mask = 0")).contains("must be non-zero"));
    assert!(shape_error(&bitfield("mask = 1, shift = 64")).contains("below 64"));
    assert!(shape_error(&bitfield("mask = -1")).contains("non-negative"));

    let raw =
        |value: &str| format!("{DECODE_HEAD}[[decode]]\nraw = {value}\nsource = {HAND_CALC}\n");
    assert!(shape_error(&raw("\"0x1_0000_0000_0000_0000\"")).contains("invalid raw value"));
    assert!(shape_error(&raw("\"twelve\"")).contains("invalid raw value"));
    assert!(shape_error(&raw("true")).contains("an integer, a float"));
    let kat = kat::parse("ok.toml", &raw("\"-0x8000_0000_0000_0000\"")).unwrap();
    let KatKind::Decode { cases, .. } = kat.data.kind else {
        panic!("expected decode");
    };
    assert_eq!(cases[0].raw, RawValue::Int(i128::from(i64::MIN)));

    let changelog =
        |date: &str| format!("{DECODE_HEAD}{case}[[changelog]]\ndate = {date}\nreason = \"r\"\n");
    assert!(shape_error(&changelog("2026-10-19T10:00:00")).contains("plain date"));
    assert!(shape_error(&changelog("\"yesterday\"")).contains("date"));
    let message = shape_error(&format!(
        "{DECODE_HEAD}{case}[[changelog]]\ndate = 2026-10-19\n"
    ));
    assert!(message.contains("missing field `reason`"), "{message}");
}
