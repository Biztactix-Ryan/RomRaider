//! Unit tests of the `<rom>` / `<romid>` reader (US-RR-29-5); the `<table>`
//! reader's are in `table/tests.rs`.

use proptest::prelude::*;
use rr_core::diag::{Code, Diagnostic, Span, codes};
use rr_core::encoding::Endian;
use rr_core::layer::{Field, LayerKind, Layered, Merge};
use rr_core::pattern::BytePattern;

use super::value::{is_hex_id, parse_bool, parse_file_size, parse_hex_bytes, parse_hex_int};
use super::{DefinitionFile, IdAddress, Identification, RamOffset, RomDoc, RomEntry, read};

/// Reads `xml`, accepting errors: the value (if any) and every diagnostic.
fn load(xml: &str) -> (Option<DefinitionFile>, Vec<Diagnostic>) {
    read("t.xml", xml.as_bytes()).accept_errors()
}

/// A file of one `<rom>` whose `<romid>` holds `inner`, on line 3.
fn with_romid(inner: &str) -> String {
    format!("<roms>\n<rom>\n<romid>{inner}</romid>\n</rom>\n</roms>")
}

/// `(code, "line:col")` of each diagnostic.
fn found(diags: &[Diagnostic]) -> Vec<(Code, String)> {
    diags
        .iter()
        .map(|d| {
            let at = match d.provenance().span() {
                Span::LineCol { line, col } => format!("{line}:{col}"),
                Span::ByteOffset(o) => format!("@{o}"),
            };
            (d.code(), at)
        })
        .collect()
}

/// The only rom of `xml`, which must read with exactly `expected`
/// diagnostics (`(code, "line:col")`).
fn only_rom(xml: &str, expected: &[(Code, &str)]) -> RomEntry {
    let (file, diags) = load(xml);
    let want: Vec<(Code, String)> = expected
        .iter()
        .map(|(c, a)| (*c, (*a).to_owned()))
        .collect();
    assert_eq!(found(&diags), want, "{diags:#?}");
    let mut roms = file.map(DefinitionFile::into_roms).unwrap_or_default();
    assert_eq!(roms.len(), 1, "{xml}");
    roms.remove(0)
}

/// The rom of a `<romid>` holding `<xmlid>X</xmlid>` and `inner`.
fn romid(inner: &str, expected: &[(Code, &str)]) -> RomEntry {
    only_rom(&with_romid(&format!("<xmlid>X</xmlid>{inner}")), expected)
}

// ---- values -------------------------------------------------------------

#[test]
fn hex_ints_read_as_legacy_hex_to_int() {
    for (text, want) in [
        ("2000", 0x2000),
        ("0x2000", 0x2000),
        ("0XE1000", 0xE_1000),
        ("e1000", 0xE_1000),
        (" \n 200 \t", 0x200),
        ("-1", -1),
        ("-0x10", -16),
        ("+7E67C", 0x7_E67C),
        ("FFFFFFFF", 0xFFFF_FFFF),
        ("00000000000000000001", 1),
    ] {
        assert_eq!(parse_hex_int(text), Some(want), "{text:?}");
    }
    for text in [
        "",
        " ",
        "0x",
        "-",
        "+",
        "G00",
        "100000000",
        "20 00",
        "20x00",
        "1.5",
        "--1",
        "0x-1",
        "２",
    ] {
        assert_eq!(parse_hex_int(text), None, "{text:?}");
    }
}

#[test]
fn file_sizes_read_as_legacy_parse_file_size() {
    for (text, want) in [
        ("1024kb", 1024 * 1024),
        ("512Kb", 512 * 1024),
        ("192KB", 192 * 1024),
        ("1280kB", 1280 * 1024),
        ("1mb", 1024 * 1024),
        ("2MB", 2 * 1024 * 1024),
        ("100b", 100),
        ("100B", 100),
        ("196608", 196_608),
        (" 1024kb\n", 1024 * 1024),
        ("0", 0),
        ("4095mb", 4095 * 1024 * 1024),
        ("4294967295", u32::MAX),
    ] {
        assert_eq!(parse_file_size(text), Some(want), "{text:?}");
    }
    for text in [
        "",
        "kb",
        "b",
        "mb",
        "-1kb",
        "+1kb",
        "1024 kb",
        "1.5mb",
        "4096mb",
        "4294967296",
        "1gb",
        "0x100",
        "1024kbb",
        "１kb",
    ] {
        assert_eq!(parse_file_size(text), None, "{text:?}");
    }
}

#[test]
fn hex_ids_read_as_legacy_as_bytes() {
    assert!(is_hex_id("0x41"));
    assert!(is_hex_id("0X4"));
    assert!(!is_hex_id("0x"), "a bare 0x is text");
    assert!(!is_hex_id("A2WA20  "));
    assert!(!is_hex_id("é0x41"));
    for (text, want) in [
        ("0x4142", vec![0x41, 0x42]),
        ("0X41 42", vec![0x41, 0x42]),
        ("0x141", vec![0x01, 0x41]),
        ("0x41 0x42", vec![0x41, 0x42]),
        ("0xabCD", vec![0xAB, 0xCD]),
    ] {
        assert_eq!(parse_hex_bytes(text), Some(want), "{text:?}");
    }
    for text in ["0x", "0x ", "0xZZ", "0x41\t42", "0x4é"] {
        assert_eq!(parse_hex_bytes(text), None, "{text:?}");
    }
}

#[test]
fn booleans_are_true_or_false_in_any_case() {
    assert_eq!(parse_bool("true"), Some(true));
    assert_eq!(parse_bool(" TRUE\n"), Some(true));
    assert_eq!(parse_bool("False"), Some(false));
    assert_eq!(parse_bool("yes"), None);
    assert_eq!(parse_bool("1"), None);
    assert_eq!(parse_bool(""), None);
}

// ---- romid fields -------------------------------------------------------

#[test]
fn every_text_field_is_read_exactly_as_written() {
    let rom = romid(
        "<internalidstring>A2WA20  </internalidstring>\
         <internalidhex>415A31</internalidhex>\
         <author> me </author><version>v1</version><caseid>22611AJ</caseid>\
         <ecuid>1B14400305</ecuid><make>Subaru</make><market>USDM</market>\
         <model>Impreza</model><submodel>WRX</submodel><transmission>MT/AT</transmission>\
         <year>01/02</year><flashmethod>wrx02</flashmethod><memmodel>68HC16Y5</memmodel>\
         <checksummodule>subarudbw</checksummodule>\
         <internalidaddress>200</internalidaddress>",
        &[],
    );
    let id = &rom.doc().romid;
    let s = |v: &str| Field::Set(v.to_owned());
    assert_eq!(id.xmlid, s("X"));
    assert_eq!(id.internal_id_string, s("A2WA20  "));
    assert_eq!(id.internal_id_hex, s("415A31"));
    assert_eq!(id.author, s(" me "));
    assert_eq!(id.version, s("v1"));
    assert_eq!(id.case_id, s("22611AJ"));
    assert_eq!(id.ecu_id, s("1B14400305"));
    assert_eq!(id.make, s("Subaru"));
    assert_eq!(id.market, s("USDM"));
    assert_eq!(id.model, s("Impreza"));
    assert_eq!(id.submodel, s("WRX"));
    assert_eq!(id.transmission, s("MT/AT"));
    assert_eq!(id.year, s("01/02"));
    assert_eq!(id.flash_method, s("wrx02"));
    assert_eq!(id.mem_model, s("68HC16Y5"));
    assert_eq!(id.checksum_module, s("subarudbw"));
    assert_eq!(id.internal_id_address, Field::Set(IdAddress::At(0x200)));
    // Not mentioned: inherit.
    assert_eq!(id.file_size, Field::Unset);
    assert_eq!(id.mem_model_endian, Field::Unset);
    assert_eq!(id.ram_offset, Field::Unset);
    assert_eq!(id.obsolete, Field::Unset);
    assert_eq!(rom.xmlid(), "X");
}

#[test]
fn an_empty_text_element_sets_an_empty_value() {
    let rom = romid("<make/><model></model>", &[]);
    assert_eq!(rom.doc().romid.make, Field::Set(String::new()));
    assert_eq!(rom.doc().romid.model, Field::Set(String::new()));
}

#[test]
fn internal_id_address_reads_hex_and_minus_one() {
    let at = |text: &str| {
        // An address without a string warns; see `half_an_identification_warns`.
        let warns = [(codes::ROM_ID_INCOMPLETE, "3:1")];
        romid(
            &format!("<internalidaddress>{text}</internalidaddress>"),
            &warns,
        )
        .doc()
        .romid
        .internal_id_address
    };
    assert_eq!(at("2000"), Field::Set(IdAddress::At(0x2000)));
    assert_eq!(at("0xE1000"), Field::Set(IdAddress::At(0xE_1000)));
    assert_eq!(at("\n  7E67C\n"), Field::Set(IdAddress::At(0x7_E67C)));
    assert_eq!(at("FFFFFFFF"), Field::Set(IdAddress::At(u32::MAX)));
    assert_eq!(at("-1"), Field::Set(IdAddress::MinusOne));
}

#[test]
fn a_malformed_internal_id_address_is_an_error_at_its_text() {
    for text in ["G000", "-2", "100000000", "0x", "20 00"] {
        let xml = with_romid(&format!(
            "<xmlid>X</xmlid><internalidaddress>{text}</internalidaddress>\
             <internalidstring>X</internalidstring>"
        ));
        let (file, diags) = load(&xml);
        // The error, and no identification warning on top of it.
        assert_eq!(
            found(&diags),
            [(codes::INVALID_ADDRESS, "3:43".to_owned())],
            "{text}"
        );
        assert!(diags[0].message().contains(text), "{diags:?}");
        assert!(read("t.xml", xml.as_bytes()).value().is_none(), "withheld");
        let rom = &file.unwrap().into_roms()[0];
        assert_eq!(rom.doc().romid.internal_id_address, Field::Unset);
        assert_eq!(rom.identification(), None);
    }
    // An empty one points at the element.
    let (_, diags) = load(&with_romid("<xmlid>X</xmlid><internalidaddress/>"));
    assert_eq!(found(&diags), [(codes::INVALID_ADDRESS, "3:24".to_owned())]);
}

#[test]
fn file_size_is_read_in_bytes() {
    let size = |text: &str| {
        romid(&format!("<filesize>{text}</filesize>"), &[])
            .doc()
            .romid
            .file_size
    };
    assert_eq!(size("1024kb"), Field::Set(1_048_576));
    assert_eq!(size("512Kb"), Field::Set(524_288));
    assert_eq!(size("192kb"), Field::Set(196_608));
    assert_eq!(size("1mb"), Field::Set(1_048_576));
    assert_eq!(size("196608"), Field::Set(196_608));
    assert_eq!(size("100b"), Field::Set(100));
}

#[test]
fn a_malformed_file_size_is_an_error_at_its_text() {
    for text in ["1024 kb", "kb", "-1kb", "4096mb", "1.5mb"] {
        let xml = with_romid(&format!("<xmlid>X</xmlid><filesize>{text}</filesize>"));
        let (file, diags) = load(&xml);
        assert_eq!(
            found(&diags),
            [(codes::INVALID_FILE_SIZE, "3:34".to_owned())],
            "{text}"
        );
        assert!(diags[0].is_error());
        assert_eq!(file.unwrap().roms()[0].doc().romid.file_size, Field::Unset);
    }
}

#[test]
fn memmodel_endian_overrides_byte_order() {
    let endian = |attr: &str| {
        romid(&format!("<memmodel{attr}>SH7058</memmodel>"), &[])
            .doc()
            .romid
            .mem_model_endian
    };
    assert_eq!(endian(""), Field::Unset);
    assert_eq!(endian(" endian=\"little\""), Field::Set(Endian::Little));
    assert_eq!(endian(" endian=\"BIG\""), Field::Set(Endian::Big));
    // Anything else is refused (legacy read it as big), at the value.
    let (_, diags) = load(&with_romid(
        "<xmlid>X</xmlid><memmodel endian=\"middle\">SH7058</memmodel>",
    ));
    assert_eq!(found(&diags), [(codes::UNKNOWN_ENDIAN, "3:42".to_owned())]);
    // Other attributes are unknown.
    let (_, diags) = load(&with_romid(
        "<xmlid>X</xmlid><memmodel Endian=\"little\">SH7058</memmodel>",
    ));
    assert_eq!(
        found(&diags),
        [(codes::UNKNOWN_ATTRIBUTE, "3:34".to_owned())]
    );
}

#[test]
fn ram_offset_is_disabled_or_an_offset_last_one_winning() {
    let offset = |inner: &str| romid(inner, &[]).doc().romid.ram_offset;
    assert_eq!(offset("<noramoffset/>"), Field::Set(RamOffset::Disabled));
    assert_eq!(
        offset("<offset>100</offset>"),
        Field::Set(RamOffset::Offset(0x100))
    );
    assert_eq!(
        offset("<offset>-0x10</offset>"),
        Field::Set(RamOffset::Offset(-16))
    );
    // In document order, as legacy's setOffset / disableRamOffset.
    assert_eq!(
        offset("<offset>100</offset><noramoffset/>"),
        Field::Set(RamOffset::Disabled)
    );
    assert_eq!(
        offset("<noramoffset/><offset>100</offset>"),
        Field::Set(RamOffset::Offset(0x100))
    );
    // Text in the flag is ignored, as legacy ignored it, with a warning.
    let rom = romid(
        "<noramoffset>false</noramoffset>",
        &[(codes::UNEXPECTED_TEXT, "3:37")],
    );
    assert_eq!(rom.doc().romid.ram_offset, Field::Set(RamOffset::Disabled));
    // A malformed offset.
    let (_, diags) = load(&with_romid("<xmlid>X</xmlid><offset>1G</offset>"));
    assert_eq!(found(&diags), [(codes::INVALID_ADDRESS, "3:32".to_owned())]);
}

#[test]
fn obsolete_is_true_or_false_anything_else_false_with_a_warning() {
    let obsolete = |text: &str, expected: &[(Code, &str)]| {
        romid(&format!("<obsolete>{text}</obsolete>"), expected)
            .doc()
            .romid
            .obsolete
    };
    assert_eq!(obsolete("true", &[]), Field::Set(true));
    assert_eq!(obsolete("TRUE", &[]), Field::Set(true));
    assert_eq!(obsolete("false", &[]), Field::Set(false));
    assert_eq!(
        obsolete("yes", &[(codes::INVALID_BOOLEAN, "3:34")]),
        Field::Set(false)
    );
}

#[test]
fn a_rom_with_a_base_is_not_obsolete_unless_it_says_so() {
    let xml = |attrs: &str, inner: &str| {
        format!("<roms><rom{attrs}><romid><xmlid>X</xmlid>{inner}</romid></rom></roms>")
    };
    let rom = only_rom(&xml(" base=\"B\"", ""), &[]);
    assert_eq!(rom.base().map(super::BaseRef::name), Some("B"));
    assert_eq!(
        rom.base().map(super::BaseRef::span),
        Some(Span::line_col(1, 18))
    );
    assert_eq!(rom.doc().romid.obsolete, Field::Set(false));
    let rom = only_rom(&xml(" base=\"B\"", "<obsolete>true</obsolete>"), &[]);
    assert_eq!(rom.doc().romid.obsolete, Field::Set(true));
    // `none` (any case) and legacy's `NaN` mean no base.
    for no_base in ["", " base=\"none\"", " base=\"NONE\"", " base=\"NaN\""] {
        let rom = only_rom(&xml(no_base, ""), &[]);
        assert_eq!(rom.base(), None, "{no_base}");
        assert_eq!(rom.doc().romid.obsolete, Field::Unset, "{no_base}");
    }
    // An empty base is kept for resolution to report.
    let rom = only_rom(&xml(" base=\"\"", ""), &[]);
    assert_eq!(rom.base().map(super::BaseRef::name), Some(""));
}

// ---- structure ----------------------------------------------------------

#[test]
fn element_names_ignore_ascii_case() {
    let rom = only_rom(
        "<ROMS><Rom><ROMID><XMLID>A</XMLID><FileSize>1kb</FileSize>\
         <InternalIdAddress>10</InternalIdAddress><InternalIdString>AB</InternalIdString>\
         </ROMID></Rom></ROMS>",
        &[],
    );
    assert_eq!(rom.xmlid(), "A");
    assert_eq!(rom.doc().romid.file_size, Field::Set(1024));
    assert!(rom.identification().is_some());
}

#[test]
fn unknown_elements_attributes_and_text_are_warnings_naming_them() {
    let xml = "<roms version=\"1\">\n\
               <rom base=\"B\" BASE=\"C\">\n\
               <romid kind=\"x\"><xmlid>X</xmlid><include>B</include>\n\
               <editstamp>1</editstamp><xmlid2/>stray</romid>\n\
               <fish/>\n\
               </rom>\n\
               <scalingbase name=\"s\"/>\n\
               <cars/>\n\
               </roms>";
    let (file, diags) = load(xml);
    assert_eq!(
        found(&diags),
        [
            (codes::UNKNOWN_ATTRIBUTE, "1:7".to_owned()),
            (codes::UNKNOWN_ATTRIBUTE, "2:15".to_owned()),
            (codes::UNKNOWN_ATTRIBUTE, "3:8".to_owned()),
            (codes::UNEXPECTED_TEXT, "4:34".to_owned()),
            (codes::UNKNOWN_ELEMENT, "3:33".to_owned()),
            (codes::UNKNOWN_ELEMENT, "4:1".to_owned()),
            (codes::UNKNOWN_ELEMENT, "4:25".to_owned()),
            (codes::UNKNOWN_ELEMENT, "5:1".to_owned()),
            (codes::UNKNOWN_ELEMENT, "8:1".to_owned()),
        ],
        "{diags:#?}"
    );
    let messages: Vec<&str> = diags.iter().map(Diagnostic::message).collect();
    assert_eq!(
        messages,
        [
            "unknown attribute `version` on <roms>; ignored",
            "unknown attribute `BASE` on <rom>; ignored",
            "unknown attribute `kind` on <romid>; ignored",
            "text directly inside <romid>; ignored",
            "unknown element <include> in <romid>; ignored",
            "unknown element <editstamp> in <romid>; ignored",
            "unknown element <xmlid2> in <romid>; ignored",
            "unknown element <fish> in <rom>; ignored",
            "unknown element <cars> in <roms>; ignored",
        ]
    );
    assert!(diags.iter().all(|d| !d.is_error()));
    assert_eq!(
        file.unwrap().roms()[0].base().map(super::BaseRef::name),
        Some("B")
    );
}

#[test]
fn elements_inside_a_value_and_text_in_rom_are_reported() {
    let rom = only_rom(
        "<roms>\n<rom>words<romid><xmlid>X<b>old</b>Y</xmlid></romid></rom>\n</roms>",
        &[
            (codes::UNEXPECTED_TEXT, "2:6"),
            (codes::UNKNOWN_ELEMENT, "2:26"),
        ],
    );
    // The value is the element's own text, as legacy's unmarshallText.
    assert_eq!(rom.xmlid(), "XY");
}

#[test]
fn checksum_and_simulation_are_not_supported_yet_and_tables_are_read() {
    let rom = only_rom(
        "<roms>\n<rom><romid><xmlid>X</xmlid></romid>\n\
         <checksum type=\"subarudbw\"/>\n<Simulation/>\n\
         <table name=\"T\" type=\"1D\" storageaddress=\"0x10\"/>\n</rom></roms>",
        &[
            (codes::UNSUPPORTED_ELEMENT, "3:1"),
            (codes::UNSUPPORTED_ELEMENT, "4:1"),
        ],
    );
    assert_eq!(rom.xmlid(), "X");
    assert_eq!(rom.doc().tables.len(), 1);
}

#[test]
fn a_repeated_element_warns_and_the_later_one_wins() {
    let rom = romid(
        "<make>A</make><make>B</make>",
        &[(codes::DUPLICATE_ELEMENT, "3:38")],
    );
    assert_eq!(rom.doc().romid.make, Field::Set("B".to_owned()));
}

#[test]
fn two_romids_merge_and_the_first_identifies() {
    let rom = only_rom(
        "<roms><rom>\n\
         <romid><xmlid>A</xmlid><internalidaddress>0</internalidaddress>\
         <internalidstring>AA</internalidstring><make>M</make></romid>\n\
         <romid><xmlid>B</xmlid><internalidaddress>0</internalidaddress>\
         <internalidstring>BB</internalidstring></romid>\n\
         </rom></roms>",
        &[(codes::DUPLICATE_ELEMENT, "3:1")],
    );
    assert_eq!(rom.xmlid(), "B");
    assert_eq!(rom.doc().romid.make, Field::Set("M".to_owned()));
    let id = rom.identification().unwrap();
    assert!(id.matches(b"AA"));
    assert!(!id.matches(b"BB"));
}

#[test]
fn a_rom_without_a_name_is_an_error_and_is_not_read() {
    for (inner, line_col) in [
        ("", "2:1"),
        ("<romid><make>M</make></romid>", "2:1"),
        ("<romid><xmlid></xmlid></romid>", "2:1"),
    ] {
        let xml = format!(
            "<roms>\n<rom>{inner}</rom>\n<rom><romid><xmlid>OK</xmlid></romid></rom>\n</roms>"
        );
        let loaded = read("t.xml", xml.as_bytes());
        assert!(loaded.value().is_none(), "withheld: {inner}");
        let (file, diags) = loaded.accept_errors();
        assert_eq!(found(&diags), [(codes::MISSING_XMLID, line_col.to_owned())]);
        let roms = file.unwrap().into_roms();
        assert_eq!(roms.len(), 1);
        assert_eq!(roms[0].xmlid(), "OK");
    }
}

#[test]
fn a_root_other_than_roms_is_not_a_definition_except_a_lone_rom() {
    let (file, diags) =
        load("<?xml version=\"1.0\"?>\n<logger version=\"1\"><protocols/></logger>");
    assert_eq!(
        found(&diags),
        [(codes::NOT_ROM_DEFINITION, "2:1".to_owned())]
    );
    assert!(file.is_none());

    let rom = only_rom(
        "<rom base=\"32BITBASE\">\n<romid><xmlid>AE5L500V</xmlid></romid>\n</rom>",
        &[(codes::SINGLE_ROM_ROOT, "1:1")],
    );
    assert_eq!(rom.xmlid(), "AE5L500V");
    assert_eq!(rom.base().map(super::BaseRef::name), Some("32BITBASE"));

    // An empty <roms> is a file of no ROM.
    let (file, diags) = load("<roms/>");
    assert!(diags.is_empty());
    assert!(file.unwrap().roms().is_empty());
}

#[test]
fn xml_errors_come_through_and_give_no_value() {
    let loaded = read("t.xml", b"<roms><rom></roms>");
    assert!(loaded.value().is_none());
    let (file, diags) = loaded.accept_errors();
    assert!(file.is_none());
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code(), codes::XML_MALFORMED);
}

#[test]
fn xml_warnings_come_through_with_the_value() {
    let input = b"\xEF\xBB\xBF<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><roms/>";
    let loaded = read("t.xml", input);
    assert!(loaded.value().is_some_and(Option::is_some));
    assert_eq!(loaded.diagnostics()[0].code(), codes::XML_ENCODING_MISMATCH);
}

// ---- identification -----------------------------------------------------

/// The identification of a romid with this address and string (either may
/// be `None`, for absent).
fn ident(
    address: Option<&str>,
    string: Option<&str>,
    expected: &[(Code, &str)],
) -> Option<Identification> {
    let mut inner = String::new();
    if let Some(a) = address {
        inner.push_str(&format!("<internalidaddress>{a}</internalidaddress>"));
    }
    if let Some(s) = string {
        inner.push_str(&format!("<internalidstring>{s}</internalidstring>"));
    }
    romid(&inner, expected).identification().cloned()
}

/// A 16-byte image holding `bytes` at `at`.
fn image(at: usize, bytes: &[u8]) -> Vec<u8> {
    let mut image = vec![0xFF; 16];
    image[at..at + bytes.len()].copy_from_slice(bytes);
    image
}

#[test]
fn a_text_id_matches_ascii_case_insensitively_at_its_address() {
    let id = ident(Some("4"), Some("A2wa20  "), &[]).unwrap();
    assert_eq!(
        id,
        Identification::Bytes(
            BytePattern::text(4, "A2wa20  ", rr_core::pattern::Case::AsciiInsensitive).unwrap()
        )
    );
    assert!(id.matches(&image(4, b"A2WA20  ")));
    assert!(id.matches(&image(4, b"a2wa20  ")));
    assert!(
        !id.matches(&image(4, b"A2WA20")),
        "the spaces are part of the ID"
    );
    assert!(!id.matches(&image(3, b"A2WA20  ")));
    assert!(!id.matches(b"\0\0\0\0A2WA2"), "too short");
}

#[test]
fn a_hex_id_matches_its_bytes_exactly() {
    let id = ident(Some("2"), Some("0x4142"), &[]).unwrap();
    assert!(id.matches(&image(2, b"AB")));
    assert!(!id.matches(&image(2, b"ab")), "bytes, not text");
    let id = ident(Some("0"), Some("0x141"), &[]).unwrap();
    assert!(id.matches(&image(0, &[0x01, 0x41])));
    let id = ident(Some("0"), Some("0x41 42"), &[]).unwrap();
    assert!(id.matches(&image(0, b"AB")));
    // A bare 0x is text.
    let id = ident(Some("0"), Some("0x"), &[]).unwrap();
    assert!(id.matches(&image(0, b"0X")));
}

#[test]
fn a_malformed_hex_id_is_an_error_at_its_text() {
    let (_, diags) = load(&with_romid(
        "<xmlid>X</xmlid><internalidaddress>0</internalidaddress><internalidstring>0xZZ</internalidstring>",
    ));
    assert_eq!(found(&diags), [(codes::INVALID_ROM_ID, "3:82".to_owned())]);
}

#[test]
fn force_with_minus_one_matches_everything() {
    let id = ident(Some("-1"), Some("FoRcE"), &[]).unwrap();
    assert_eq!(id, Identification::Force);
    assert!(id.matches(&[]));
    // -1 with another string never matches.
    assert_eq!(
        ident(
            Some("-1"),
            Some("A2WA20"),
            &[(codes::ROM_ID_INCOMPLETE, "3:1")]
        ),
        None
    );
    // `force` at an ordinary address is just text.
    let id = ident(Some("0"), Some("force"), &[]).unwrap();
    assert!(id.matches(b"FORCE"));
}

#[test]
fn half_an_identification_warns() {
    // An address alone never matches.
    assert_eq!(
        ident(Some("200"), None, &[(codes::ROM_ID_INCOMPLETE, "3:1")]),
        None
    );
    assert_eq!(
        ident(Some("200"), Some(""), &[(codes::ROM_ID_INCOMPLETE, "3:1")]),
        None
    );
    // A string alone is matched at 0, as legacy's default address.
    let id = ident(None, Some("AB"), &[(codes::ROM_ID_INCOMPLETE, "3:1")]).unwrap();
    assert!(id.matches(b"ab"));
    // Neither: a base definition, no warning.
    assert_eq!(ident(None, None, &[]), None);
}

// ---- layering -----------------------------------------------------------

#[test]
fn each_rom_is_a_layer_and_romids_merge_over_their_base() {
    let xml = "<roms>\n\
               <rom>\n<romid><xmlid>BASE</xmlid><make>Subaru</make><filesize>192kb</filesize>\
               <memmodel>68HC16Y5</memmodel><obsolete>true</obsolete></romid></rom>\n\
               <rom base=\"BASE\">\n<romid><xmlid>CHILD</xmlid><filesize>1024kb</filesize></romid></rom>\n\
               </roms>";
    let (file, diags) = load(xml);
    assert!(diags.is_empty(), "{diags:?}");
    let file = file.unwrap();
    assert_eq!(file.source(), "t.xml");
    let [base, child] = file.roms() else {
        unreachable!("two roms")
    };
    let layer = child.layer();
    assert_eq!(layer.id().as_str(), "CHILD");
    assert_eq!(layer.kind(), LayerKind::File);
    assert_eq!(layer.source(), "t.xml");
    assert_eq!(layer.span(), Span::line_col(4, 1));
    assert_eq!(child.span(), Span::line_col(4, 1));

    let mut stack: Layered<RomDoc> = Layered::new();
    stack.push(base.layer(), base.doc().clone());
    stack.push(child.layer(), child.doc().clone());
    let merged = stack.merged().romid;
    assert_eq!(merged.xmlid, Field::Set("CHILD".to_owned()));
    assert_eq!(merged.make, Field::Set("Subaru".to_owned()));
    assert_eq!(merged.mem_model, Field::Set("68HC16Y5".to_owned()));
    assert_eq!(merged.file_size, Field::Set(1024 * 1024));
    assert_eq!(merged.obsolete, Field::Set(false), "not inherited");
    // And the empty document changes nothing.
    assert_eq!(child.doc().merged(&RomDoc::default()), *child.doc());
}

// ---- never panics ---------------------------------------------------------

/// Text that exercises the value parsers: hex-ish, size-ish and odd.
fn value_text() -> impl Strategy<Value = String> {
    prop_oneof![
        "[-+ 0-9a-fA-FxXkKmMbB]{0,12}",
        "0x[0-9a-fA-F ]{0,9}",
        any::<String>().prop_map(|s| s.chars().filter(|c| !matches!(c, '<' | '&')).collect()),
    ]
}

proptest! {
    #[test]
    fn any_romid_values_read_without_panicking(
        address in value_text(),
        id in value_text(),
        size in value_text(),
        offset in value_text(),
        obsolete in value_text(),
        image in proptest::collection::vec(any::<u8>(), 0..64),
    ) {
        let xml = with_romid(&format!(
            "<xmlid>X</xmlid><internalidaddress>{address}</internalidaddress>\
             <internalidstring>{id}</internalidstring><filesize>{size}</filesize>\
             <offset>{offset}</offset><obsolete>{obsolete}</obsolete>"
        ));
        let (file, _) = read("p.xml", xml.as_bytes()).accept_errors();
        if let Some(file) = file {
            for rom in file.roms() {
                if let Some(id) = rom.identification() {
                    let _ = id.matches(&image);
                }
            }
        }
    }

    #[test]
    fn any_bytes_read_without_panicking(input in proptest::collection::vec(any::<u8>(), 0..256)) {
        let _ = read("p.xml", &input);
    }
}
