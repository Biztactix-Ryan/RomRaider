//! Tests for the naive reference codec (`rr_testkit::reference`, US-RR-10-5).
//!
//! * Every `[[decode]]` case of every seed fixture in `fixtures/kat/*.toml`
//!   decodes to its expected raw value (floats bit-exact), and every storage
//!   type has at least one case.
//! * Encoding each case's *expected* raw value gives back the fixture bytes,
//!   starting from a buffer whose encoded bits are all inverted (so a no-op
//!   encode fails).
//! * The seed cases cover every (storage, byte order) the reference codec
//!   accepts plus a bitfield in each byte order (US-RR-10-2), mirroring
//!   `seed_coverage.rs` but derived from the reference codec, not rr-core;
//!   bosch-subtract has no seed (the fixture format cannot say where a table
//!   ends) and is covered by the hand-calculated tables below.
//! * Nothing on this file's path to the codec (this file, `kat`, `fixtures`)
//!   names rr-core, so the test binary links no rr-core code.
//! * Bitfield encode preserves the bits outside the mask.
//! * Out-of-range values, kind mismatches and short input are errors, never
//!   panics or silent truncation.
//! * The Bosch subtract layout, checked against hand-calculated tables.
//!
//! This file is the test boundary: it converts the `kat` fixture vocabulary to
//! the reference codec's own types, so the codec module itself never names
//! `kat` (or rr-core).

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::PathBuf;

use proptest::prelude::*;
use rr_testkit::fixtures::repo_root;
use rr_testkit::kat::{self, Definition, Input, KatKind, RawValue};
use rr_testkit::reference::{
    self, Bitfield, Encoding, Endian, Layout, Raw, ReferenceError, Storage, U20_BIT19_SIGN_EXTENDS,
    decode, decode_table, encode, encode_table,
};

// ---------------------------------------------------------------------------
// Test boundary: kat vocabulary -> reference vocabulary
// ---------------------------------------------------------------------------

fn storage_of(storage: kat::Storage) -> Storage {
    match storage {
        kat::Storage::U8 => Storage::U8,
        kat::Storage::U16 => Storage::U16,
        kat::Storage::U20 => Storage::U20,
        kat::Storage::U24 => Storage::U24,
        kat::Storage::U32 => Storage::U32,
        kat::Storage::U64 => Storage::U64,
        kat::Storage::I8 => Storage::I8,
        kat::Storage::I16 => Storage::I16,
        kat::Storage::I24 => Storage::I24,
        kat::Storage::I32 => Storage::I32,
        kat::Storage::I64 => Storage::I64,
        kat::Storage::F32 => Storage::F32,
        kat::Storage::F64 => Storage::F64,
    }
}

fn encoding_of(encoding: &kat::Encoding) -> Encoding {
    let endian = match encoding.endian {
        kat::Endian::Big => Endian::Big,
        kat::Endian::Little => Endian::Little,
    };
    let layout = match encoding.layout {
        kat::Layout::Default => Layout::Default,
        kat::Layout::BoschSubtract => Layout::BoschSubtract,
    };
    let mut out = Encoding::new(storage_of(encoding.storage), endian).with_layout(layout);
    if let Some(bitfield) = encoding.bitfield {
        out = out.with_bitfield(Bitfield::new(bitfield.mask, bitfield.shift));
    }
    out
}

/// Whether `actual` is exactly `expected`: integers equal, floats bit-equal
/// (an `f32` compared as its exact widening to `f64`).
fn raw_matches(actual: Raw, expected: RawValue) -> bool {
    match (actual, expected) {
        (Raw::Int(a), RawValue::Int(e)) => a == e,
        (Raw::F32(a), RawValue::Float(e)) => f64::from(a).to_bits() == e.to_bits(),
        (Raw::F64(a), RawValue::Float(e)) => a.to_bits() == e.to_bits(),
        _ => false,
    }
}

/// Per-byte mask of the bits the encoding owns in one cell (what `encode`
/// writes): the bitfield mask, the 20 immediate bits of a `u20`, or the whole
/// cell.
fn owned_bits(encoding: &Encoding) -> Vec<u8> {
    let storage = encoding.storage;
    let width = storage.cell_bytes();
    let bits = storage.value_bits();
    let pattern_mask = match encoding.bitfield {
        Some(bitfield) => bitfield.mask,
        None if bits == 64 => u64::MAX,
        None => (1_u64 << bits) - 1,
    };
    if storage == Storage::U20 {
        return vec![
            (((pattern_mask >> 16) & 0x0F) << 4) as u8,
            ((pattern_mask >> 8) & 0xFF) as u8,
            (pattern_mask & 0xFF) as u8,
        ];
    }
    (0..width)
        .map(|k| {
            let shift = match encoding.endian {
                Endian::Big => 8 * (width - 1 - k),
                Endian::Little => 8 * k,
            };
            ((pattern_mask >> shift) & 0xFF) as u8
        })
        .collect()
}

#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn seed_fixtures() -> Vec<PathBuf> {
    let dir = repo_root().join("fixtures").join("kat");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    paths
}

/// Converts a fixture's expected raw value to the reference codec's [`Raw`]
/// for `storage`, or `None` if it does not fit that storage's kind exactly
/// (an `f32` expected value must be exactly representable as `f32`).
#[expect(
    clippy::cast_possible_truncation,
    reason = "the narrowing is checked: the f32 must widen back to the same bits"
)]
fn raw_of(expected: RawValue, storage: Storage) -> Option<Raw> {
    match (expected, storage) {
        (RawValue::Int(v), s) if !s.is_float() => Some(Raw::Int(v)),
        (RawValue::Float(v), Storage::F64) => Some(Raw::F64(v)),
        (RawValue::Float(v), Storage::F32) => {
            let narrow = v as f32;
            (f64::from(narrow).to_bits() == v.to_bits()).then_some(Raw::F32(narrow))
        }
        _ => None,
    }
}

/// One runnable decode case from a seed fixture.
struct SeedCase {
    label: String,
    bytes: Vec<u8>,
    offset: usize,
    encoding: Encoding,
    expected: RawValue,
}

#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn seed_cases() -> Vec<SeedCase> {
    let mut out = Vec::new();
    for path in seed_fixtures() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let fixture = kat::load(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        let KatKind::Decode { definition, cases } = fixture.data.kind else {
            continue; // checksum fixtures have nothing to decode
        };
        let Definition::Inline { encoding, .. } = definition else {
            panic!("{name}: seed decode fixtures use an inline encoding; this test cannot run it");
        };
        let Some(Input::Hex(hex)) = fixture.data.input else {
            panic!("{name}: seed decode fixtures use hex input; this test cannot run it");
        };
        assert_eq!(
            encoding.layout,
            kat::Layout::Default,
            "{name}: the fixture format has no table extent for bosch-subtract yet; \
             define it before adding such a seed fixture"
        );
        assert!(!cases.is_empty(), "{name}: decode fixture with no cases");
        for (index, case) in cases.into_iter().enumerate() {
            out.push(SeedCase {
                label: format!("{name} [[decode]] #{} (offset {})", index + 1, case.offset),
                bytes: hex.0.clone(),
                offset: usize::try_from(case.offset).unwrap(),
                encoding: encoding_of(&encoding),
                expected: case.raw,
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Seed known-answer fixtures
// ---------------------------------------------------------------------------

#[test]
fn decodes_every_seed_fixture_case_for_every_storage_type() {
    let cases = seed_cases();
    let mut per_storage: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut failures = Vec::new();
    for case in &cases {
        *per_storage.entry(case.encoding.storage.name()).or_default() += 1;
        match decode(&case.bytes, case.offset, &case.encoding) {
            Ok(raw) if raw_matches(raw, case.expected) => {}
            other => failures.push(format!(
                "{}: expected {:?}, got {other:?}",
                case.label, case.expected
            )),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let missing: Vec<&str> = Storage::ALL
        .iter()
        .map(|s| s.name())
        .filter(|name| !per_storage.contains_key(name))
        .collect();
    assert!(
        missing.is_empty(),
        "storage types with zero seed cases: {missing:?} (have {per_storage:?})"
    );
    // The two vocabularies agree on the storage set.
    assert_eq!(kat::Storage::ALL.len(), Storage::ALL.len());
    for storage in kat::Storage::ALL {
        assert_eq!(storage_of(storage).name(), storage.name());
    }
    eprintln!(
        "reference codec: {} seed case(s) decoded; per storage: {per_storage:?}",
        cases.len()
    );
}

#[test]
fn encoding_the_expected_value_reproduces_every_seed_fixture_case() {
    for case in seed_cases() {
        // The fixture's expected value, not the codec's own decode: encode is
        // checked against the fixture on its own.
        let raw = raw_of(case.expected, case.encoding.storage).unwrap_or_else(|| {
            panic!(
                "{}: expected {:?} is not a {} value",
                case.label, case.expected, case.encoding.storage
            )
        });
        // Invert every bit the encoding owns, so only a real encode restores it.
        let mut scratch = case.bytes.clone();
        for (k, mask) in owned_bits(&case.encoding).into_iter().enumerate() {
            scratch[case.offset + k] ^= mask;
        }
        assert_ne!(scratch, case.bytes, "{}: inversion did nothing", case.label);
        encode(raw, &case.encoding, &mut scratch, case.offset)
            .unwrap_or_else(|e| panic!("{}: encode failed: {e}", case.label));
        assert_eq!(scratch, case.bytes, "{}: round trip differs", case.label);
    }
}

#[test]
fn seed_cases_cover_every_storage_endian_and_bitfield_the_reference_accepts() {
    // (storage, endian, has bitfield) of every seed decode case.
    let seen: HashSet<(&'static str, Endian, bool)> = seed_cases()
        .iter()
        .map(|c| {
            (
                c.encoding.storage.name(),
                c.encoding.endian,
                c.encoding.bitfield.is_some(),
            )
        })
        .collect();
    let mut missing = Vec::new();
    for storage in Storage::ALL {
        for endian in [Endian::Big, Endian::Little] {
            // The reference codec's own answer to "is this supported?"
            // (it refuses only little-endian u20).
            let probe = decode(&[0; 8], 0, &Encoding::new(storage, endian));
            if matches!(probe, Err(ReferenceError::Unsupported(_))) {
                continue;
            }
            assert!(probe.is_ok(), "{storage} {endian:?}: {probe:?}");
            if !seen.contains(&(storage.name(), endian, false)) {
                missing.push(format!("{storage} {endian:?} (plain)"));
            }
        }
    }
    for endian in [Endian::Big, Endian::Little] {
        let covered = seen.iter().any(|&(name, e, bitfield)| {
            bitfield
                && e == endian
                && Storage::ALL
                    .iter()
                    .any(|s| s.name() == name && s.cell_bytes() > 1)
        });
        if !covered {
            missing.push(format!("bitfield on a multi-byte storage, {endian:?}"));
        }
    }
    assert!(
        missing.is_empty(),
        "accepted by the reference codec but no seed case: {missing:?} (seen {seen:?})"
    );
}

#[test]
fn nothing_on_the_seed_test_path_names_the_core_crate() {
    // Spelled in two halves so this file does not match itself.
    let needle = concat!("rr", "_core");
    let src = repo_root().join("crates").join("rr-testkit").join("src");
    let mut files = vec![
        repo_root()
            .join("crates")
            .join("rr-testkit")
            .join("tests")
            .join("reference.rs"),
        src.join("fixtures.rs"),
        src.join("kat.rs"),
    ];
    let mut dirs = vec![src.join("kat")];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    let offenders: Vec<String> = files
        .iter()
        .filter(|path| fs::read_to_string(path).unwrap().contains(needle))
        .map(|path| path.display().to_string())
        .collect();
    assert!(
        offenders.is_empty(),
        "the seed KAT test must not reach rr-core (the reference module itself is \
         checked by `cargo xtask check-reference-isolation`): {offenders:?}"
    );
}

// ---------------------------------------------------------------------------
// Hand-calculated spot checks (independent of the fixtures)
// ---------------------------------------------------------------------------

#[test]
fn hand_calculated_cells() {
    let be = |s| Encoding::new(s, Endian::Big);
    let le = |s| Encoding::new(s, Endian::Little);
    // 0x12*65536 + 0x34*256 + 0x56 = 1179648 + 13312 + 86 = 1193046
    assert_eq!(
        decode(&[0x12, 0x34, 0x56], 0, &be(Storage::U24)),
        Ok(Raw::Int(1_193_046))
    );
    // little-endian: 0x56*65536 + 0x34*256 + 0x12 = 5636096 + 13312 + 18 = 5649426
    assert_eq!(
        decode(&[0x12, 0x34, 0x56], 0, &le(Storage::U24)),
        Ok(Raw::Int(5_649_426))
    );
    // 0xFFFFFE = 16777214; top bit set, 16777214 - 16777216 = -2
    assert_eq!(
        decode(&[0xFF, 0xFF, 0xFE], 0, &be(Storage::I24)),
        Ok(Raw::Int(-2))
    );
    // u64 max, i64 min
    assert_eq!(
        decode(&[0xFF; 8], 0, &be(Storage::U64)),
        Ok(Raw::Int(i128::from(u64::MAX)))
    );
    assert_eq!(
        decode(&[0, 0, 0, 0, 0, 0, 0, 0x80], 0, &le(Storage::I64)),
        Ok(Raw::Int(i128::from(i64::MIN)))
    );
    // f64 1.0 = 0x3FF0_0000_0000_0000
    assert_eq!(
        decode(&[0, 0, 0, 0, 0, 0, 0xF0, 0x3F], 0, &le(Storage::F64)),
        Ok(Raw::F64(1.0))
    );
    // u20 with bit 19 set: c0 = 0x80 -> imm20 = 8*65536 = 524288 (unsigned decision)
    let u20 = decode(&[0x80, 0x00, 0x00], 0, &be(Storage::U20));
    if U20_BIT19_SIGN_EXTENDS {
        assert_eq!(u20, Ok(Raw::Int(524_288 - 1_048_576)));
    } else {
        assert_eq!(u20, Ok(Raw::Int(524_288)));
    }
}

#[test]
fn u20_encode_preserves_the_opcode_nibble() {
    let encoding = Encoding::new(Storage::U20, Endian::Big);
    // MOVI20 cell 0x10 23 45: low nibble 0 is opcode; write 0x7FFFF -> 70 FF FF.
    let mut cell = [0x10, 0x23, 0x45];
    encode(Raw::Int(0x7FFFF), &encoding, &mut cell, 0).unwrap();
    assert_eq!(cell, [0x70, 0xFF, 0xFF]);
    // An opcode nibble of 1 (MOVI20S) survives.
    let mut cell = [0x01, 0x00, 0x00];
    encode(Raw::Int(0x12345), &encoding, &mut cell, 0).unwrap();
    assert_eq!(cell, [0x11, 0x23, 0x45]);
    // Little-endian u20 is undefined.
    assert!(matches!(
        decode(&[0; 3], 0, &Encoding::new(Storage::U20, Endian::Little)),
        Err(ReferenceError::Unsupported(_))
    ));
}

// ---------------------------------------------------------------------------
// Bitfields
// ---------------------------------------------------------------------------

#[test]
fn bitfield_encode_preserves_bits_outside_the_mask() {
    let field = Bitfield::new(0x0FF0, None);
    // BE: stored 0xABCD; write 0x12 into bits 11..4 -> 0xA12D.
    let be = Encoding::new(Storage::U16, Endian::Big).with_bitfield(field);
    let mut bytes = [0xAB, 0xCD];
    encode(Raw::Int(0x12), &be, &mut bytes, 0).unwrap();
    assert_eq!(bytes, [0xA1, 0x2D]);
    assert_eq!(decode(&bytes, 0, &be), Ok(Raw::Int(0x12)));
    // LE: the mask applies to the decoded value: CD AB -> 0xABCD -> 0xA12D -> 2D A1.
    let le = Encoding::new(Storage::U16, Endian::Little).with_bitfield(field);
    let mut bytes = [0xCD, 0xAB];
    encode(Raw::Int(0x12), &le, &mut bytes, 0).unwrap();
    assert_eq!(bytes, [0x2D, 0xA1]);
    // u8 mask 0xE0 (top three bits): 0x1F | (5 << 5) = 0x1F | 0xA0 = 0xBF.
    let top3 = Encoding::new(Storage::U8, Endian::Big).with_bitfield(Bitfield::new(0xE0, None));
    let mut bytes = [0x1F];
    encode(Raw::Int(5), &top3, &mut bytes, 0).unwrap();
    assert_eq!(bytes, [0xBF]);
}

#[test]
fn bitfield_range_and_shape_errors() {
    let field = Encoding::new(Storage::U16, Endian::Big).with_bitfield(Bitfield::new(0x0FF0, None));
    let mut bytes = [0xAB, 0xCD];
    for value in [256, -1, i128::from(u64::MAX)] {
        assert!(
            matches!(
                encode(Raw::Int(value), &field, &mut bytes, 0),
                Err(ReferenceError::BitfieldOutOfRange { .. })
            ),
            "{value}"
        );
    }
    // Explicit shift 2 on mask 0x0FF0: only multiples of 4 in 0..=0x3FC fit.
    let shifted =
        Encoding::new(Storage::U16, Endian::Big).with_bitfield(Bitfield::new(0x0FF0, Some(2)));
    assert!(encode(Raw::Int(0x3FC), &shifted, &mut bytes, 0).is_ok());
    assert!(encode(Raw::Int(0x3FD), &shifted, &mut bytes, 0).is_err());
    assert_eq!(bytes, [0xAF, 0xFD]);
    // Mask wider than the storage, zero mask, float storage.
    for encoding in [
        Encoding::new(Storage::U8, Endian::Big).with_bitfield(Bitfield::new(0x100, None)),
        Encoding::new(Storage::U8, Endian::Big).with_bitfield(Bitfield::new(0, None)),
        Encoding::new(Storage::F32, Endian::Big).with_bitfield(Bitfield::new(1, None)),
        Encoding::new(Storage::U20, Endian::Big).with_bitfield(Bitfield::new(0x10_0000, None)),
    ] {
        assert!(
            matches!(
                decode(&[0; 8], 0, &encoding),
                Err(ReferenceError::InvalidBitfield { .. })
            ),
            "{encoding:?}"
        );
    }
    assert_eq!(bytes, [0xAF, 0xFD], "a failed encode must not write");
}

// ---------------------------------------------------------------------------
// Errors: range, kind, short input
// ---------------------------------------------------------------------------

#[test]
fn out_of_range_values_are_errors_for_every_integer_width() {
    for storage in Storage::ALL.into_iter().filter(|s| !s.is_float()) {
        let encoding = Encoding::new(storage, Endian::Big);
        let (min, max) = storage.int_range();
        let mut bytes = vec![0x5A; storage.cell_bytes()];
        let before = bytes.clone();
        for value in [min - 1, max + 1] {
            assert!(
                matches!(
                    encode(Raw::Int(value), &encoding, &mut bytes, 0),
                    Err(ReferenceError::OutOfRange { .. })
                ),
                "{storage} {value}"
            );
            assert_eq!(bytes, before, "{storage}: failed encode wrote");
        }
        // The extremes themselves fit and round-trip.
        for value in [min, max] {
            encode(Raw::Int(value), &encoding, &mut bytes, 0).unwrap();
            assert_eq!(
                decode(&bytes, 0, &encoding),
                Ok(Raw::Int(value)),
                "{storage}"
            );
        }
    }
    // Spot values: u8 0..=255, i8 -128..=127, u24 below 2^24, i64 min.
    assert_eq!(Storage::U8.int_range(), (0, 255));
    assert_eq!(Storage::I8.int_range(), (-128, 127));
    assert_eq!(Storage::U24.int_range(), (0, 16_777_215));
    assert_eq!(
        Storage::I64.int_range(),
        (i128::from(i64::MIN), i128::from(i64::MAX))
    );
}

#[test]
fn kind_mismatches_are_errors() {
    let mut bytes = [0; 8];
    for (storage, raw) in [
        (Storage::U16, Raw::F32(1.0)),
        (Storage::F32, Raw::Int(1)),
        (Storage::F32, Raw::F64(1.0)),
        (Storage::F64, Raw::F32(1.0)),
    ] {
        assert!(
            matches!(
                encode(raw, &Encoding::new(storage, Endian::Big), &mut bytes, 0),
                Err(ReferenceError::KindMismatch { .. })
            ),
            "{storage} {raw:?}"
        );
    }
}

#[test]
fn short_input_is_an_error_not_a_panic() {
    for storage in Storage::ALL {
        let encoding = Encoding::new(storage, Endian::Big);
        let width = storage.cell_bytes();
        let bytes = vec![0; width];
        assert!(decode(&bytes, 0, &encoding).is_ok(), "{storage}");
        for (input, offset) in [
            (&bytes[..width - 1], 0),
            (&bytes[..], 1),
            (&bytes[..], usize::MAX),
            (&[][..], 0),
        ] {
            assert!(
                matches!(
                    decode(input, offset, &encoding),
                    Err(ReferenceError::ShortInput { .. })
                ),
                "{storage} offset {offset}"
            );
        }
        let mut short = vec![0; width - 1];
        let raw = match storage {
            Storage::F32 => Raw::F32(0.0),
            Storage::F64 => Raw::F64(0.0),
            _ => Raw::Int(0),
        };
        assert!(matches!(
            encode(raw, &encoding, &mut short, 0),
            Err(ReferenceError::ShortInput { .. })
        ));
    }
    // Tables are bounds-checked up front, including a huge count.
    let encoding = Encoding::new(Storage::U16, Endian::Big);
    assert!(matches!(
        decode_table(&[0; 4], 0, usize::MAX, &encoding),
        Err(ReferenceError::ShortInput { .. })
    ));
    assert!(matches!(
        decode_table(&[0; 4], 2, 2, &encoding),
        Err(ReferenceError::ShortInput { .. })
    ));
}

// ---------------------------------------------------------------------------
// Bosch subtract (legacy maps/DataCell.java:193-207, :219-231, :312-313)
// ---------------------------------------------------------------------------

#[test]
fn bosch_subtract_hand_calculated_u8() {
    // Stored s = [0x10, 0x20, 0x30] = [16, 32, 48], W = 1, 2^(8*1) = 256.
    // value[2] = 256 - 48 = 208
    // value[1] = 256 - (32 + 48) = 256 - 80 = 176
    // value[0] = 256 - (16 + 32 + 48) = 256 - 96 = 160
    let encoding = Encoding::new(Storage::U8, Endian::Big).with_layout(Layout::BoschSubtract);
    let bytes = [0xEE, 0x10, 0x20, 0x30, 0xEE];
    let values = decode_table(&bytes, 1, 3, &encoding).unwrap();
    assert_eq!(values, [Raw::Int(160), Raw::Int(176), Raw::Int(208)]);
    // Inverse: s[2] = 256 - 208 = 48; s[1] = 208 - 176 = 32; s[0] = 176 - 160 = 16.
    let mut out = [0xEE, 0, 0, 0, 0xEE];
    encode_table(&values, &encoding, &mut out, 1).unwrap();
    assert_eq!(out, bytes);
    // The single-cell API refuses the whole-table layout.
    assert!(matches!(
        decode(&bytes, 1, &encoding),
        Err(ReferenceError::Unsupported(_))
    ));
}

#[test]
fn bosch_subtract_hand_calculated_u16_be() {
    // Bytes 00 01 | 01 00 | FF 00 big-endian: s = [1, 256, 65280]; 2^16 = 65536.
    // value[2] = 65536 - 65280 = 256
    // value[1] = 65536 - (256 + 65280) = 65536 - 65536 = 0
    // value[0] = 65536 - (1 + 256 + 65280) = 65536 - 65537 = -1
    let encoding = Encoding::new(Storage::U16, Endian::Big).with_layout(Layout::BoschSubtract);
    let bytes = [0x00, 0x01, 0x01, 0x00, 0xFF, 0x00];
    let values = decode_table(&bytes, 0, 3, &encoding).unwrap();
    assert_eq!(values, [Raw::Int(-1), Raw::Int(0), Raw::Int(256)]);
    let mut out = [0; 6];
    encode_table(&values, &encoding, &mut out, 0).unwrap();
    assert_eq!(out, bytes);
    // A last stored cell of 0 decodes to 2^16 = 65536, which encodes back to 0.
    let values = decode_table(&[0, 0], 0, 1, &encoding).unwrap();
    assert_eq!(values, [Raw::Int(65_536)]);
}

#[test]
fn bosch_subtract_out_of_range_is_an_error_and_writes_nothing() {
    let encoding = Encoding::new(Storage::U8, Endian::Big).with_layout(Layout::BoschSubtract);
    let mut out = [0xEE; 3];
    // s[0] = 0 - (-5) = 5 fits, but value[N-1] = 0 needs s[N-1] = 256 - 0 = 256: does not
    // fit u8 (legacy wraps it to 0).
    let err = encode_table(&[Raw::Int(-5), Raw::Int(0)], &encoding, &mut out, 0).unwrap_err();
    assert!(
        matches!(&err, ReferenceError::TableCell { index: 1, source }
            if matches!(**source, ReferenceError::OutOfRange { value: 256, .. })),
        "{err:?}"
    );
    // Decreasing values need a negative s[i]: 100 - 200 = -100 for u8.
    let err = encode_table(&[Raw::Int(200), Raw::Int(100)], &encoding, &mut out, 0).unwrap_err();
    assert!(
        matches!(&err, ReferenceError::TableCell { index: 0, source }
            if matches!(**source, ReferenceError::OutOfRange { value: -100, .. })),
        "{err:?}"
    );
    assert_eq!(out, [0xEE; 3]);
    // Undefined for float and u20 storage.
    for storage in [Storage::F32, Storage::F64, Storage::U20] {
        let encoding = Encoding::new(storage, Endian::Big).with_layout(Layout::BoschSubtract);
        assert!(matches!(
            decode_table(&[0; 8], 0, 1, &encoding),
            Err(ReferenceError::Unsupported(_))
        ));
    }
}

#[test]
fn default_layout_tables_are_consecutive_cells() {
    let encoding = Encoding::new(Storage::I16, Endian::Little);
    // FF FF = -1, 00 80 = 0x8000 = 32768 - 65536 = -32768, 34 12 = 0x1234 = 4660
    let bytes = [0xFF, 0xFF, 0x00, 0x80, 0x34, 0x12];
    let values = decode_table(&bytes, 0, 3, &encoding).unwrap();
    assert_eq!(values, [Raw::Int(-1), Raw::Int(-32_768), Raw::Int(4660)]);
    let mut out = [0; 6];
    encode_table(&values, &encoding, &mut out, 0).unwrap();
    assert_eq!(out, bytes);
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

fn any_storage() -> impl Strategy<Value = Storage> {
    proptest::sample::select(Storage::ALL.to_vec())
}

fn any_endian() -> impl Strategy<Value = Endian> {
    prop_oneof![Just(Endian::Big), Just(Endian::Little)]
}

proptest! {
    /// Any cell bytes decode and re-encode to themselves (u20 big-endian only;
    /// floats bit-exact, NaN payloads included).
    #[test]
    fn any_cell_round_trips(storage in any_storage(), endian in any_endian(), bytes in proptest::collection::vec(any::<u8>(), 8)) {
        let endian = if storage == Storage::U20 { Endian::Big } else { endian };
        let encoding = Encoding::new(storage, endian);
        let cell = &bytes[..storage.cell_bytes()];
        let raw = decode(cell, 0, &encoding).unwrap();
        let mut out = cell.to_vec();
        for (k, mask) in owned_bits(&encoding).into_iter().enumerate() {
            out[k] ^= mask;
        }
        encode(raw, &encoding, &mut out, 0).unwrap();
        prop_assert_eq!(out.as_slice(), cell);
    }

    /// Encoding into a bitfield never touches bits outside the mask, and the
    /// field decodes back to the value written.
    #[test]
    fn bitfield_encode_touches_only_the_mask(
        endian in any_endian(),
        bytes in proptest::collection::vec(any::<u8>(), 4),
        mask in 1_u64..=0xFFFF_FFFF,
        seed in any::<u64>(),
    ) {
        let field = Bitfield::new(mask, None);
        let encoding = Encoding::new(Storage::U32, endian).with_bitfield(field);
        let max = mask >> field.shift;
        let value = seed % (max + 1);
        // A value is only representable if its shifted bits lie inside the mask.
        let fits = ((value << field.shift) & !mask) == 0;
        let plain = Encoding::new(Storage::U32, endian);
        let Raw::Int(before) = decode(&bytes, 0, &plain).unwrap() else { unreachable!() };
        let mut out = bytes.clone();
        let result = encode(Raw::Int(i128::from(value)), &encoding, &mut out, 0);
        prop_assert_eq!(result.is_ok(), fits);
        let Raw::Int(after) = decode(&out, 0, &plain).unwrap() else { unreachable!() };
        let outside = !i128::from(mask);
        prop_assert_eq!(before & outside, after & outside);
        if fits {
            prop_assert_eq!(decode(&out, 0, &encoding).unwrap(), Raw::Int(i128::from(value)));
        } else {
            prop_assert_eq!(out, bytes);
        }
    }

    /// Untrusted bytes, offsets and encodings never panic.
    #[test]
    fn untrusted_input_never_panics(
        storage in any_storage(),
        endian in any_endian(),
        bosch in any::<bool>(),
        mask in proptest::option::of(any::<u64>()),
        shift in proptest::option::of(0_u32..80),
        bytes in proptest::collection::vec(any::<u8>(), 0..24),
        offset in prop_oneof![0_usize..32, Just(usize::MAX)],
        count in prop_oneof![0_usize..8, Just(usize::MAX)],
        value in any::<i128>(),
    ) {
        let mut encoding = Encoding::new(storage, endian)
            .with_layout(if bosch { Layout::BoschSubtract } else { Layout::Default });
        if let Some(mask) = mask {
            encoding = encoding.with_bitfield(Bitfield::new(mask, shift));
        }
        let _ = decode(&bytes, offset, &encoding);
        let _ = decode_table(&bytes, offset, count, &encoding);
        let mut out = bytes.clone();
        let _ = encode(Raw::Int(value), &encoding, &mut out, offset);
        let _ = encode_table(&[Raw::Int(value), Raw::Int(value / 2)], &encoding, &mut out, offset);
        let _ = reference::encode(Raw::F32(f32::NAN), &encoding, &mut out, offset);
    }
}
