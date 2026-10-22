//! The NCS codec vectors (`fixtures/kat/ncs-codec-legacy.toml`, US-RR-14-8):
//! the Nissan data codec and its CRC-16, transcribed as data from the legacy
//! test file `NcsCoDecTest.java`.
//!
//! There is no NCS codec in Rust yet (US-RR-78 will consume these vectors).
//! These tests check the transcription now, without running or porting any
//! Java (D-024): the legacy file is read as text only.
//!
//! * The fixture is well formed and complete: one `[[codec]]` case per
//!   legacy `@Test` method, [`LEGACY_TESTS`] (six, pinned), in file order,
//!   each citing [`LEGACY_FILE`] with lines that exist and include its
//!   method ([`fixture_has_one_case_per_legacy_test`]).
//! * Each case's literal data is on the lines it cites: the plaintext and
//!   ciphertext hex strings, the code and the asserted CRC value
//!   ([`cited_lines_hold_the_transcribed_literals`]).
//! * The four CRC outputs are re-derived with this file's own
//!   implementation of the Rocksoft CRC model (Williams 1993, section 15)
//!   with the CRC RevEng catalogue's CRC-16/MCRF4XX parameters, after
//!   checking it against the catalogue's check values
//!   ([`crc_outputs_match_the_catalogue_crc`]).
//! * The encode and decode outputs cannot be re-derived independently (a
//!   port of the legacy routine is not an independent check), so only what
//!   holds without a codec is checked: decode inverts encode on these bytes,
//!   whole 32-bit words go in and out, and equal plaintext words give equal
//!   ciphertext words, distinct ones distinct ones, since the codec works
//!   word by word (`NcsCoDec.java:56-63`)
//!   ([`encode_and_decode_vectors_agree`]).

use std::collections::BTreeMap;
use std::fs;

use rr_testkit::fixtures::repo_root;
use rr_testkit::kat::{self, CodecCase, KatKind, Source};

/// The fixture, under `fixtures/kat/`.
const FIXTURE: &str = "ncs-codec-legacy.toml";

/// The legacy test file the vectors are transcribed from, relative to the
/// repository root.
const LEGACY_FILE: &str = "legacy/src/test/java/com/romraider/maps/checksum/NcsCoDecTest.java";

/// Every `@Test` method of [`LEGACY_FILE`], in file order, with the `op` of
/// the case transcribing it. Pinned: the legacy file has exactly these six.
const LEGACY_TESTS: &[(&str, &str)] = &[
    ("testEncodeData", "ncs-encode"),
    ("testEncodedDataCrc", "ncs-crc16"),
    ("testBitwiseComplementCrc", "ncs-crc16"),
    ("testBitwiseComplementCrcLitteEndian", "ncs-crc16"),
    ("testResidue", "ncs-crc16"),
    ("testDecodeData", "ncs-decode"),
];

/// The fixture's cases.
#[expect(
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn cases() -> Vec<CodecCase> {
    let path = repo_root().join("fixtures").join("kat").join(FIXTURE);
    let fixture = kat::load(&path).unwrap_or_else(|e| panic!("{FIXTURE}: {e}"));
    let KatKind::Codec { cases } = fixture.data.kind else {
        panic!("{FIXTURE} must be a [[codec]] fixture");
    };
    cases
}

/// The lines of [`LEGACY_FILE`] (line 1 is index 0).
#[expect(
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn legacy_lines() -> Vec<String> {
    let path = repo_root().join(LEGACY_FILE);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        .lines()
        .map(str::to_owned)
        .collect()
}

/// The text of the lines `case` cites, joined, or why they cannot be read.
fn cited_text(case: &CodecCase, legacy: &[String]) -> Result<String, String> {
    let Source::LegacyTest { file, lines } = &case.source else {
        return Err(format!("source is not `legacy-test`: {:?}", case.source));
    };
    if file != LEGACY_FILE {
        return Err(format!("source cites {file:?}, not {LEGACY_FILE:?}"));
    }
    let mut text = String::new();
    for range in kat::parse_line_spec(lines)? {
        for line in range {
            let index = usize::try_from(line).map_err(|e| e.to_string())? - 1;
            let Some(line_text) = legacy.get(index) else {
                return Err(format!(
                    "cites line {line}, but {LEGACY_FILE} has {} lines",
                    legacy.len()
                ));
            };
            text.push_str(line_text);
            text.push('\n');
        }
    }
    Ok(text)
}

/// Upper-case hex of `bytes`, no separators (how the legacy test writes its
/// strings).
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// A case's integer parameter.
fn int_param(case: &CodecCase, key: &str) -> Option<i64> {
    case.params.get(key).and_then(toml::Value::as_integer)
}

#[test]
fn fixture_has_one_case_per_legacy_test() {
    let legacy = legacy_lines();
    // The pinned count agrees with the legacy file as it stands.
    let annotations = legacy.iter().filter(|l| l.trim() == "@Test").count();
    assert_eq!(
        annotations,
        LEGACY_TESTS.len(),
        "@Test methods in {LEGACY_FILE}"
    );
    let cases = cases();
    assert_eq!(
        cases.len(),
        LEGACY_TESTS.len(),
        "{FIXTURE} needs one case per legacy @Test"
    );
    for (number, (case, (method, op))) in cases.iter().zip(LEGACY_TESTS).enumerate() {
        let at = format!("{FIXTURE} [[codec]] #{} ({method})", number + 1);
        assert_eq!(case.op, *op, "{at}");
        let text = cited_text(case, &legacy).unwrap_or_else(|e| panic!("{at}: {e}"));
        assert!(
            text.contains(&format!("public final void {method}()")),
            "{at}: the cited lines do not include the method"
        );
        // Every case uses the test class's shared literals (:33, :35, :37).
        for shared in [
            "final String text = ",
            "final int scode = ",
            "final String encoded = ",
        ] {
            assert!(text.contains(shared), "{at}: does not cite `{shared}`");
        }
        let keys: Vec<&str> = case.params.keys().map(String::as_str).collect();
        match case.op.as_str() {
            "ncs-encode" | "ncs-decode" => {
                assert_eq!(keys, ["scode"], "{at}: params");
                assert!(
                    int_param(case, "scode").is_some_and(|c| u32::try_from(c).is_ok()),
                    "{at}: scode must be a 32-bit unsigned integer"
                );
            }
            "ncs-crc16" => {
                assert_eq!(keys, ["complement", "store"], "{at}: params");
                assert!(case.params["complement"].is_bool(), "{at}: complement");
                assert!(
                    matches!(case.params["store"].as_str(), Some("big" | "little")),
                    "{at}: store"
                );
                assert_eq!(case.output.as_bytes().len(), 2, "{at}: a CRC-16 is 2 bytes");
            }
            other => panic!("{at}: unknown op {other:?}"),
        }
    }
}

#[test]
fn cited_lines_hold_the_transcribed_literals() {
    let legacy = legacy_lines();
    let cases = cases();
    let encoded = cases
        .iter()
        .find(|c| c.op == "ncs-encode")
        .map(|c| c.output.as_bytes().to_vec())
        .unwrap_or_default();
    assert!(!encoded.is_empty(), "no ncs-encode case");
    for (number, case) in cases.iter().enumerate() {
        let at = format!("{FIXTURE} [[codec]] #{} ({})", number + 1, case.op);
        let text = cited_text(case, &legacy).unwrap_or_else(|e| panic!("{at}: {e}"));
        let quoted = |bytes: &[u8]| text.contains(&format!("\"{}\"", hex(bytes)));
        match case.op.as_str() {
            "ncs-encode" | "ncs-decode" => {
                let scode = int_param(case, "scode").unwrap_or_default();
                assert!(
                    text.contains(&format!("0x{scode:08X};")),
                    "{at}: scode 0x{scode:08X} is not on the cited lines"
                );
                assert!(
                    quoted(case.input.as_bytes()),
                    "{at}: input is not a cited literal"
                );
                assert!(
                    quoted(case.output.as_bytes()),
                    "{at}: output is not a cited literal"
                );
            }
            _ => {
                // The CRC input is the encoded literal (:37), as the test
                // computes it with nisEncode; the residue case appends the
                // stored complement (:71-72).
                let input = case.input.as_bytes();
                assert!(
                    input.starts_with(&encoded) && quoted(&encoded),
                    "{at}: input does not start with the cited encoded literal"
                );
                let value = hex(case.output.as_bytes());
                assert!(
                    text.contains(&format!("(short) 0x{value},")),
                    "{at}: the asserted value 0x{value} is not on the cited lines"
                );
            }
        }
    }
}

/// CRC of `data` in the Rocksoft model (Williams 1993, section 15): a
/// `width`-bit register shifted left (most significant bit first), each
/// input byte bit-reversed first when `refin`, the result bit-reversed when
/// `refout`, then XORed with `xorout`. Written from the model, not from any
/// legacy routine (which shifts right with the reversed polynomial).
fn crc16_model(data: &[u8], poly: u16, init: u16, refin: bool, refout: bool, xorout: u16) -> u16 {
    let mut register = init;
    for &byte in data {
        let byte = if refin { byte.reverse_bits() } else { byte };
        register ^= u16::from(byte) << 8;
        for _ in 0..8 {
            register = if register & 0x8000 == 0 {
                register << 1
            } else {
                (register << 1) ^ poly
            };
        }
    }
    let register = if refout {
        register.reverse_bits()
    } else {
        register
    };
    register ^ xorout
}

/// CRC-16/MCRF4XX of the CRC RevEng catalogue: width=16 poly=0x1021
/// init=0xffff refin=true refout=true xorout=0x0000 check=0x6f91.
fn mcrf4xx(data: &[u8]) -> u16 {
    crc16_model(data, 0x1021, 0xFFFF, true, true, 0x0000)
}

#[test]
fn crc_outputs_match_the_catalogue_crc() {
    // The model against the catalogue's check values ("123456789"):
    // CRC-16/MCRF4XX 0x6F91, and CRC-16/IBM-SDLC (the same with
    // xorout=0xffff) 0x906E, residue 0xF0B8 (the X.25 good-FCS value).
    assert_eq!(mcrf4xx(b"123456789"), 0x6F91);
    let sdlc = crc16_model(b"123456789", 0x1021, 0xFFFF, true, true, 0xFFFF);
    assert_eq!(sdlc, 0x906E);
    let mut framed = b"123456789".to_vec();
    framed.extend_from_slice(&sdlc.to_le_bytes());
    assert_eq!(mcrf4xx(&framed), 0xF0B8);

    let crc_cases: Vec<CodecCase> = cases()
        .into_iter()
        .filter(|c| c.op == "ncs-crc16")
        .collect();
    assert_eq!(
        crc_cases.len(),
        4,
        "testEncodedDataCrc, the two complements, testResidue"
    );
    for case in crc_cases {
        let crc = mcrf4xx(case.input.as_bytes());
        let complement = case.params.get("complement").and_then(toml::Value::as_bool);
        let crc = if complement == Some(true) { !crc } else { crc };
        let stored = match case.params.get("store").and_then(toml::Value::as_str) {
            Some("little") => crc.to_le_bytes(),
            _ => crc.to_be_bytes(),
        };
        assert_eq!(
            case.output.as_bytes(),
            stored,
            "{FIXTURE} ncs-crc16 {:?}: the catalogue CRC is {}",
            case.params,
            hex(&stored)
        );
    }
}

#[test]
fn encode_and_decode_vectors_agree() {
    let cases = cases();
    let encode: Vec<&CodecCase> = cases.iter().filter(|c| c.op == "ncs-encode").collect();
    let decode: Vec<&CodecCase> = cases.iter().filter(|c| c.op == "ncs-decode").collect();
    assert_eq!((encode.len(), decode.len()), (1, 1));
    let (encode, decode) = (encode[0], decode[0]);
    assert_eq!(encode.params, decode.params, "one code for both directions");
    // decode(encode(text)) = text: the decode case reverses the encode case.
    assert_eq!(decode.input, encode.output);
    assert_eq!(decode.output, encode.input);

    let plain = encode.input.as_bytes();
    let cipher = encode.output.as_bytes();
    assert_eq!(plain.len(), cipher.len());
    assert!(
        !plain.is_empty() && plain.len() % 4 == 0,
        "whole 32-bit words: {} bytes",
        plain.len()
    );
    // Word by word: a function of the plaintext word, and one-to-one.
    let mut forward: BTreeMap<&[u8], &[u8]> = BTreeMap::new();
    let mut backward: BTreeMap<&[u8], &[u8]> = BTreeMap::new();
    for (p, c) in plain.chunks(4).zip(cipher.chunks(4)) {
        assert_eq!(
            *forward.entry(p).or_insert(c),
            c,
            "word {} encodes two ways",
            hex(p)
        );
        assert_eq!(
            *backward.entry(c).or_insert(p),
            p,
            "word {} decodes two ways",
            hex(c)
        );
    }
    // The vector repeats one word (FFFF22E4, first and last), so the
    // property is exercised, not vacuous.
    assert!(forward.len() < plain.len() / 4);

    // The CRC cases are over this ciphertext (the test runs nisEncode first).
    for case in cases.iter().filter(|c| c.op == "ncs-crc16") {
        assert!(
            case.input.as_bytes().starts_with(cipher),
            "{:?}",
            case.params
        );
    }
}

#[test]
fn residue_input_appends_the_stored_complement() {
    let cases = cases();
    let encoded = cases
        .iter()
        .find(|c| c.op == "ncs-encode")
        .map(|c| c.output.as_bytes());
    let swapped = cases.iter().find(|c| {
        c.op == "ncs-crc16"
            && c.params.get("complement").and_then(toml::Value::as_bool) == Some(true)
            && c.params.get("store").and_then(toml::Value::as_str) == Some("little")
    });
    let residue: Vec<&CodecCase> = cases
        .iter()
        .filter(|c| {
            c.op == "ncs-crc16" && Some(c.input.as_bytes().len()) != encoded.map(<[u8]>::len)
        })
        .collect();
    assert_eq!(
        residue.len(),
        1,
        "one case CRCs more than the encoded bytes"
    );
    let (Some(encoded), Some(swapped)) = (encoded, swapped) else {
        panic!("{FIXTURE} lacks the encode or the little-endian complement case");
    };
    let mut expected = encoded.to_vec();
    expected.extend_from_slice(swapped.output.as_bytes());
    assert_eq!(residue[0].input.as_bytes(), expected.as_slice());
}
