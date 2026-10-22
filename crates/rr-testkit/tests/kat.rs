//! Tests for the schema-1 KAT fixture types (`rr_testkit::kat`, US-RR-9-5).
//!
//! Parses a representative fixture of each shape (decode values, checksum
//! from `tests/data/kat/`, expression inline), and proves that structurally broken fixtures —
//! malformed hex, unknown schema, a missing `source`, a `hand-calc` without
//! `working`, unknown keys, ambiguous `[definition]` / `[input]`, a
//! `[range]` outside a decode fixture — are rejected with an error that
//! names the file and the problem.

use std::path::PathBuf;

use rr_testkit::fixtures::{FixtureError, SUPPORTED_SCHEMA, Tier};
use rr_testkit::kat::{
    self, Bitfield, ChecksumState, CodecCase, Definition, DefinitionRef, Encoding, Endian, Formula,
    FormulaTier, HexBytes, Input, KatKind, Layout, Monotonicity, RawValue, Snippet, SnippetFormat,
    Source, Storage, TimeUnit, TimingCase, UnitCase,
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
        Some(Input::Hex(HexBytes(vec![0x01, 0xF4, 0xFF, 0x38])))
    );
    let KatKind::Decode {
        definition, cases, ..
    } = kat.kind
    else {
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
    assert_eq!(
        kat.input,
        Some(Input::Hex(HexBytes(vec![1, 2, 3, 4, 0, 0])))
    );
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
    let Some(Input::Rom(rom)) = &kat.input else {
        panic!("expected a ROM input, got {:?}", kat.input);
    };
    assert_eq!(rom.tier, Tier::Corpus);
    assert_eq!(rom.path, PathBuf::from("roms/example.bin"));
    assert_eq!(
        rom.sha256.as_str(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!((rom.offset, rom.length), (0x1000, Some(8)));
    let KatKind::Decode {
        definition, cases, ..
    } = kat.kind
    else {
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
    let KatKind::Decode {
        definition, cases, ..
    } = kat.kind
    else {
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
    let expr = "[[expr]]\nformula = \"1+1\"\nvalue = 2.0\nsource = { kind = \"standard\", doc = \"d\", section = \"s\" }\n";
    let cases: [(String, &str); 14] = [
        (DECODE_HEAD.to_owned(), "has no cases"),
        (
            format!("{DECODE_HEAD}{case}{checksum}"),
            "more than one of [[decode]], [[checksum]], [[expr]], [[unit]], [[timing]] and [[codec]]",
        ),
        (
            format!("{DECODE_HEAD}{case}{expr}"),
            "more than one of [[decode]], [[checksum]], [[expr]], [[unit]], [[timing]] and [[codec]]",
        ),
        (
            format!("schema = 1\ntitle = \"t\"\n[input]\nhex = \"00\"\n{expr}"),
            "an [[expr]] fixture has no [definition] or [input]",
        ),
        (
            format!("{DECODE_HEAD}{expr}").replace("[input]\nhex = \"7F\"\n", ""),
            "an [[expr]] fixture has no [definition] or [input]",
        ),
        (
            "schema = 1\ntitle = \"t\"\nexpr = []\n".to_owned(),
            "`expr` is empty",
        ),
        (
            format!("schema = 1\ntitle = \"t\"\n{checksum}"),
            "missing field `input`",
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
fn parses_an_expression_fixture() {
    let text = r#"
schema = 1
title = "expr"

[[expr]]
formula = "2+3*x"
x = 4
value = 14
source = { kind = "hand-calc", working = "2 + (3*4) = 2 + 12 = 14" }

[[expr]]
formula = "(-8)^(1/3)"
value = nan
tolerance = 0.5
source = { kind = "standard", doc = "d", section = "s" }
note = "n"
"#;
    let kat = kat::parse("expr.toml", text).unwrap().data;
    assert_eq!(kat.input, None);
    let KatKind::Expr { cases } = kat.kind else {
        panic!("expected an expr fixture, got {:?}", kat.kind);
    };
    assert_eq!(cases.len(), 2);
    assert_eq!(cases[0].formula, Formula::Text("2+3*x".to_owned()));
    assert_eq!(cases[0].formula.text(), Some("2+3*x"));
    assert!(cases[0].params.is_empty(), "params default to none");
    assert_eq!(
        (cases[0].x, cases[0].value, cases[0].tolerance),
        (4.0, 14.0, None)
    );
    assert_eq!(cases[1].x, 0.0, "x defaults to 0");
    assert!(cases[1].value.is_nan());
    assert_eq!(cases[1].tolerance, Some(0.5));
    assert_eq!(cases[1].note.as_deref(), Some("n"));
    assert!(matches!(cases[1].source, Source::Standard { .. }));
    let unknown = text.replace("note = \"n\"", "inputs = [1.0]");
    assert!(shape_error(&unknown.replace("expr.toml", "bad.toml")).contains("inputs"));
}

/// US-RR-20-10: a case names a corpus formula by hash (D-028: the
/// definitions have no licence, so their formula text is never committed),
/// and binds parameter references by id.
#[test]
fn parses_an_expression_case_with_a_corpus_formula_reference() {
    let sha = "ab".repeat(32);
    let case = |formula: &str| {
        format!(
            "schema = 1\ntitle = \"t\"\n\n[[expr]]\n{formula}\nparams = {{ P9 = 90.0, P12 = -1.5 }}\nvalue = 2\n\
             source = {{ kind = \"hand-calc\", working = \"1 + 1 = 2\" }}\n"
        )
    };
    let reference = format!(
        "formula_ref = {{ tier = \"corpus\", sha256 = \"{}\", path = \"defs/romraider/a b.xml\", xpath = \"/roms[1]/rom[1]/scaling[1]/@expression\" }}",
        sha.to_ascii_uppercase()
    );
    let kat = kat::parse("ref.toml", &case(&reference)).unwrap().data;
    let KatKind::Expr { cases } = kat.kind else {
        panic!("expected an expr fixture, got {:?}", kat.kind);
    };
    let Formula::Ref(r) = &cases[0].formula else {
        panic!("expected a reference, got {:?}", cases[0].formula);
    };
    assert_eq!(r.tier, FormulaTier::Corpus);
    assert_eq!(Tier::from(r.tier), Tier::Corpus);
    assert_eq!(r.sha256.as_str(), sha, "stored lowercase");
    assert_eq!(r.path, std::path::Path::new("defs/romraider/a b.xml"));
    assert_eq!(r.xpath, "/roms[1]/rom[1]/scaling[1]/@expression");
    assert_eq!(cases[0].formula.text(), None);
    assert_eq!(cases[0].formula.reference(), Some(r));
    assert_eq!(cases[0].formula.to_string(), "sha256:abababababab");
    assert_eq!(
        cases[0]
            .params
            .iter()
            .map(|(k, v)| (k.as_str(), *v))
            .collect::<Vec<_>>(),
        [("P12", -1.5), ("P9", 90.0)]
    );

    let both = case(&format!("formula = \"x\"\n{reference}"));
    assert!(shape_error(&both).contains("both `formula` and `formula_ref`"));
    assert!(shape_error(&case("")).contains("needs `formula`"));
    let public = reference.replace("\"corpus\"", "\"public\"");
    assert!(shape_error(&case(&public)).contains("public"));
    let short = reference.replace(&sha.to_ascii_uppercase(), "abc");
    assert!(shape_error(&case(&short)).contains("expected exactly 64 hex digits"));
    let no_xpath = reference.replace(", xpath = \"/roms[1]/rom[1]/scaling[1]/@expression\"", "");
    assert!(shape_error(&case(&no_xpath)).contains("xpath"));
    let extra = reference.replace("tier =", "item = \"i\", tier =");
    assert!(shape_error(&case(&extra)).contains("item"));
    assert!(
        shape_error(&case("formula = \"x\"").replace("P12 = -1.5", "P12 = \"a\"")).contains("P12")
    );
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

// ---------------------------------------------------------------------------
// [range] (US-RR-24-2)
// ---------------------------------------------------------------------------

const RANGE: &str = r#"
[range]
real_min = -27.5
raw_at_min = 255
real_max = 100.0
raw_at_max = 0
monotonicity = "decreasing"
tolerance = 1e-9
source = { kind = "hand-calc", working = "100 - 255*0.5 = -27.5 at raw 255; 100 - 0*0.5 = 100.0 at raw 0" }
note = "falls"
"#;

fn decode_with_range(range: &str) -> String {
    format!(
        "{DECODE_HEAD}[definition.conversion]\nto_real = \"100-x*0.5\"\n\
         [[decode]]\nraw = 127\nsource = {HAND_CALC}\n{range}"
    )
}

#[test]
fn parses_a_decode_fixture_range() {
    let kat = kat::parse("range.toml", &decode_with_range(RANGE))
        .unwrap()
        .data;
    let KatKind::Decode {
        range: Some(range), ..
    } = kat.kind
    else {
        panic!("expected a decode fixture with a [range]: {:?}", kat.kind);
    };
    assert_eq!(range.real_min, -27.5);
    assert_eq!(range.raw_at_min, RawValue::Int(255));
    assert_eq!(range.real_max, 100.0);
    assert_eq!(range.raw_at_max, RawValue::Int(0));
    assert_eq!(range.monotonicity, Monotonicity::Decreasing);
    assert_eq!(range.tolerance, Some(1e-9));
    assert!(matches!(range.source, Source::HandCalc { .. }));
    assert_eq!(range.note.as_deref(), Some("falls"));

    let kat = kat::parse("plain.toml", &decode_with_range(""))
        .unwrap()
        .data;
    assert!(matches!(kat.kind, KatKind::Decode { range: None, .. }));

    for (spelling, monotonicity) in [
        ("increasing", Monotonicity::Increasing),
        ("decreasing", Monotonicity::Decreasing),
        ("constant", Monotonicity::Constant),
        ("non-monotonic", Monotonicity::NonMonotonic),
    ] {
        let text = decode_with_range(&RANGE.replace("\"decreasing\"", &format!("\"{spelling}\"")));
        let KatKind::Decode {
            range: Some(range), ..
        } = kat::parse("m.toml", &text).unwrap().data.kind
        else {
            panic!("{spelling}");
        };
        assert_eq!(range.monotonicity, monotonicity);
        assert_eq!(monotonicity.name(), spelling);
    }
}

#[test]
fn rejects_a_malformed_or_misplaced_range() {
    let message = shape_error(&decode_with_range(&RANGE.replace("note =", "notes =")));
    assert!(message.contains("notes"), "{message}");
    let message = shape_error(&decode_with_range(&RANGE.replace("decreasing", "falling")));
    assert!(message.contains("falling"), "{message}");
    let message = shape_error(&decode_with_range(&RANGE.replace("raw_at_max = 0\n", "")));
    assert!(message.contains("raw_at_max"), "{message}");
    let no_source: String = RANGE
        .lines()
        .filter(|l| !l.starts_with("source"))
        .map(|l| format!("{l}\n"))
        .collect();
    let message = shape_error(&decode_with_range(&no_source));
    assert!(message.contains("source"), "{message}");

    let checksum = format!(
        "schema = 1\ntitle = \"t\"\n[input]\nhex = \"00 00\"\n[[checksum]]\nalgorithm = \"a\"\n\
         offset = 0\nbefore = {{ hex = \"00\", valid = true, source = {HAND_CALC} }}\n\
         after = {{ hex = \"00\", valid = true, source = {HAND_CALC} }}\n{RANGE}"
    );
    let message = shape_error(&checksum);
    assert!(message.contains("[range]"), "{message}");
    let expr = format!(
        "schema = 1\ntitle = \"t\"\n[[expr]]\nformula = \"1+1\"\nvalue = 2.0\nsource = {HAND_CALC}\n{RANGE}"
    );
    let message = shape_error(&expr);
    assert!(message.contains("[range]"), "{message}");
}

const UNIT_CASE: &str = r#"
[[unit]]
from = "mph"
to = "kmh"
value = 1
expected = 1.609344
tolerance = 1e-15
source = { kind = "standard", doc = "NIST SP 811 (2008)", section = "Appendix B.8, mile" }
note = "MF-42"
"#;

#[test]
fn parses_a_unit_fixture() {
    let text = format!("schema = 1\ntitle = \"units\"\n{UNIT_CASE}");
    let kat = kat::parse("units.toml", &text).unwrap().data;
    assert_eq!(kat.input, None);
    let KatKind::Unit { cases } = kat.kind else {
        panic!("expected a unit fixture, got {:?}", kat.kind);
    };
    assert_eq!(
        cases,
        [UnitCase {
            from: "mph".to_owned(),
            to: "kmh".to_owned(),
            value: 1.0,
            expected: 1.609_344,
            tolerance: Some(1e-15),
            fuel: None,
            source: Source::Standard {
                doc: "NIST SP 811 (2008)".to_owned(),
                section: "Appendix B.8, mile".to_owned(),
            },
            note: Some("MF-42".to_owned()),
        }]
    );
}

#[test]
fn parses_a_unit_case_with_a_fuel() {
    let text = format!(
        "schema = 1\ntitle = \"afr\"\n[[unit]]\nfrom = \"afr\"\nto = \"lambda\"\n\
         fuel = \"gasoline\"\nvalue = 14.7\nexpected = 1.0\nsource = {HAND_CALC}\n"
    );
    let kat = kat::parse("afr.toml", &text).unwrap().data;
    let KatKind::Unit { cases } = kat.kind else {
        panic!("expected a unit fixture, got {:?}", kat.kind);
    };
    assert_eq!(cases[0].fuel.as_deref(), Some("gasoline"));
    let message = shape_error(&text.replace("fuel = \"gasoline\"", "fuel = \"\""));
    assert!(message.contains("`fuel` is empty"), "{message}");
}

#[test]
fn rejects_a_malformed_or_misplaced_unit_fixture() {
    let head = "schema = 1\ntitle = \"t\"\n";
    let cases: [(String, &str); 9] = [
        (format!("{head}unit = []\n"), "`unit` is empty"),
        (
            format!("{head}[input]\nhex = \"00\"\n{UNIT_CASE}"),
            "a [[unit]] fixture has no [definition] or [input]",
        ),
        (
            format!("{head}[definition.encoding]\nstorage = \"u8\"\nendian = \"big\"\n{UNIT_CASE}"),
            "a [[unit]] fixture has no [definition] or [input]",
        ),
        (
            format!("{DECODE_HEAD}[[decode]]\nraw = 1\nsource = {HAND_CALC}\n{UNIT_CASE}"),
            "more than one of [[decode]], [[checksum]], [[expr]], [[unit]], [[timing]] and [[codec]]",
        ),
        (
            format!("{head}{UNIT_CASE}{RANGE}"),
            "only a fixture with [[decode]] cases has one",
        ),
        (
            format!(
                "{head}{}",
                UNIT_CASE.replace("expected = 1.609344", "expected = nan")
            ),
            "must be finite",
        ),
        (
            format!("{head}{}", UNIT_CASE.replace("value = 1", "value = inf")),
            "must be finite",
        ),
        (
            format!("{head}{}", UNIT_CASE.replace("to = \"kmh\"", "to = \" \"")),
            "non-empty `from` and `to`",
        ),
        (
            format!("{head}{}", UNIT_CASE.replace("note = ", "factor = ")),
            "unknown field `factor`",
        ),
    ];
    for (text, expected) in cases {
        let message = shape_error(&text);
        assert!(message.contains(expected), "{expected}: {message}");
    }
}

const TIMING_CASE: &str = r#"
[[timing]]
name = "iso14230_2::P3_MIN"
ms = 55
source = { kind = "standard", doc = "ISO 14230-2:1999", section = "4.5.1, Table 3" }
note = "MF-28"
"#;

#[test]
fn parses_a_timing_fixture() {
    let text = format!("schema = 1\ntitle = \"timing\"\n{TIMING_CASE}");
    let kat = kat::parse("timing.toml", &text).unwrap().data;
    assert_eq!(kat.input, None);
    let KatKind::Timing { cases } = kat.kind else {
        panic!("expected a timing fixture, got {:?}", kat.kind);
    };
    assert_eq!(
        cases,
        [TimingCase {
            name: "iso14230_2::P3_MIN".to_owned(),
            amount: 55,
            unit: TimeUnit::Millis,
            source: Source::Standard {
                doc: "ISO 14230-2:1999".to_owned(),
                section: "4.5.1, Table 3".to_owned(),
            },
            note: Some("MF-28".to_owned()),
        }]
    );
    assert_eq!(cases[0].duration(), std::time::Duration::from_millis(55));

    let micros = kat::parse("timing.toml", &text.replace("ms = 55", "us = 100"))
        .unwrap()
        .data;
    let KatKind::Timing { cases } = micros.kind else {
        panic!("expected a timing fixture, got {:?}", micros.kind);
    };
    assert_eq!(cases[0].unit, TimeUnit::Micros);
    assert_eq!(cases[0].duration(), std::time::Duration::from_micros(100));
}

#[test]
fn rejects_a_malformed_or_misplaced_timing_fixture() {
    let head = "schema = 1\ntitle = \"t\"\n";
    let cases: [(String, &str); 10] = [
        (format!("{head}timing = []\n"), "`timing` is empty"),
        (
            format!("{head}[input]\nhex = \"00\"\n{TIMING_CASE}"),
            "a [[timing]] fixture has no [definition] or [input]",
        ),
        (
            format!(
                "{head}[definition.encoding]\nstorage = \"u8\"\nendian = \"big\"\n{TIMING_CASE}"
            ),
            "a [[timing]] fixture has no [definition] or [input]",
        ),
        (
            format!("{DECODE_HEAD}[[decode]]\nraw = 1\nsource = {HAND_CALC}\n{TIMING_CASE}"),
            "more than one of [[decode]], [[checksum]], [[expr]], [[unit]], [[timing]] and [[codec]]",
        ),
        (
            format!("{head}{UNIT_CASE}{TIMING_CASE}"),
            "more than one of [[decode]], [[checksum]], [[expr]], [[unit]], [[timing]] and [[codec]]",
        ),
        (
            format!("{head}{TIMING_CASE}{RANGE}"),
            "only a fixture with [[decode]] cases has one",
        ),
        (
            format!(
                "{head}{}",
                TIMING_CASE.replace("ms = 55", "ms = 55\nus = 55000")
            ),
            "exactly one of `ms` and `us`",
        ),
        (
            format!("{head}{}", TIMING_CASE.replace("ms = 55\n", "")),
            "exactly one of `ms` and `us`",
        ),
        (
            format!("{head}{}", TIMING_CASE.replace("ms = 55", "ms = -1")),
            "invalid value",
        ),
        (
            format!(
                "{head}{}",
                TIMING_CASE.replace("iso14230_2::P3_MIN", "P3 MIN")
            ),
            "constant path without spaces",
        ),
    ];
    for (text, expected) in cases {
        let message = shape_error(&text);
        assert!(message.contains(expected), "{expected}: {message}");
    }
}

const CODEC_CASE: &str = r#"
[[codec]]
op = "ncs-crc16"
params = { complement = false, store = "big" }
input = "31 32 33 34"
output = "AB CD"
source = { kind = "legacy-test", file = "legacy/src/test/java/X.java", lines = "33,39-43" }
note = "n"
"#;

#[test]
fn parses_a_codec_fixture() {
    let text = format!("schema = 1\ntitle = \"codec\"\n{CODEC_CASE}");
    let kat = kat::parse("codec.toml", &text).unwrap().data;
    assert_eq!(kat.input, None);
    let KatKind::Codec { cases } = kat.kind else {
        panic!("expected a codec fixture, got {:?}", kat.kind);
    };
    let mut params = toml::Table::new();
    params.insert("complement".to_owned(), toml::Value::Boolean(false));
    params.insert("store".to_owned(), toml::Value::String("big".to_owned()));
    assert_eq!(
        cases,
        [CodecCase {
            op: "ncs-crc16".to_owned(),
            params,
            input: HexBytes(b"1234".to_vec()),
            output: HexBytes(vec![0xAB, 0xCD]),
            source: Source::LegacyTest {
                file: "legacy/src/test/java/X.java".to_owned(),
                lines: "33,39-43".to_owned(),
            },
            note: Some("n".to_owned()),
        }]
    );
    assert_eq!(cases[0].source.kind(), "legacy-test");
}

#[test]
fn rejects_a_malformed_or_misplaced_codec_fixture() {
    let head = "schema = 1\ntitle = \"t\"\n";
    let cases: [(String, &str); 8] = [
        (format!("{head}codec = []\n"), "`codec` is empty"),
        (
            format!("{head}[input]\nhex = \"00\"\n{CODEC_CASE}"),
            "a [[codec]] fixture has no [definition] or [input]",
        ),
        (
            format!("{DECODE_HEAD}[[decode]]\nraw = 1\nsource = {HAND_CALC}\n{CODEC_CASE}"),
            "more than one of [[decode]], [[checksum]], [[expr]], [[unit]], [[timing]] and [[codec]]",
        ),
        (
            format!("{head}{CODEC_CASE}{RANGE}"),
            "only a fixture with [[decode]] cases has one",
        ),
        (
            format!(
                "{head}{}",
                CODEC_CASE.replace("\"ncs-crc16\"", "\"ncs crc\"")
            ),
            "operation name without spaces",
        ),
        (
            format!("{head}{}", CODEC_CASE.replace("output = \"AB CD\"\n", "")),
            "missing field `output`",
        ),
        (
            format!("{head}{}", CODEC_CASE.replace("\"AB CD\"", "\"ABC\"")),
            "odd number of hex digits",
        ),
        (
            format!("{head}{}", CODEC_CASE.replace("lines = ", "line = ")),
            "unknown field `line`",
        ),
    ];
    for (text, expected) in cases {
        let message = shape_error(&text);
        assert!(message.contains(expected), "{expected}: {message}");
    }
}

#[test]
fn legacy_line_specs_parse_in_ascending_order() {
    assert_eq!(
        kat::parse_line_spec("33, 35,39-43").unwrap(),
        [33..=33, 35..=35, 39..=43]
    );
    assert_eq!(kat::parse_line_spec("7").unwrap(), [7..=7]);
    for bad in [
        "", " ", "0", "x", "3-", "5-4", "5,5", "10,9", "3-9,9", "1,,2", "-3",
    ] {
        assert!(kat::parse_line_spec(bad).is_err(), "{bad:?} parsed");
    }
}
