//! rr-core's cell codec (`rr_core::encoding`, US-RR-19-8) against the seed
//! known-answer fixtures and the independent reference codec
//! (`rr_testkit::reference`, PROJECT.md T2).
//!
//! * Every `[[decode]]` case of every seed fixture in `fixtures/kat/*.toml`
//!   decodes through rr-core to its expected raw value (floats bit-exact),
//!   and every integer storage has at least one case.
//! * `encode_into(decode(bytes))` gives back the fixture bytes, starting from
//!   a buffer whose encoded bits are all inverted.
//! * On random encodings (every storage, both byte orders, unsigned
//!   bitfields), random bytes and random values, rr-core and the reference
//!   codec agree: the same raw value from the same bytes, the same bytes from
//!   the same value, and an error from both when the value does not fit.
//!
//! This file is a test boundary: it converts the fixture vocabulary to
//! rr-core's types here, and rr-core's to the reference codec's through
//! `rr_testkit::encoding_oracle`. Neither the reference codec nor the fixture
//! types name rr-core.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use proptest::prelude::*;
use rr_core::encoding::{BitField, Encoding, Endian, Layout, Raw, Storage};
use rr_testkit::encoding_oracle::{core_raw, reference_encoding, reference_raw, same_bits};
use rr_testkit::fixtures::repo_root;
use rr_testkit::kat::{self, Definition, Input, KatKind, RawValue};
use rr_testkit::reference;

// ---------------------------------------------------------------------------
// Test boundary
// ---------------------------------------------------------------------------

fn core_storage(storage: kat::Storage) -> Storage {
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

#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn core_encoding_of_kat(encoding: &kat::Encoding) -> Encoding {
    let endian = match encoding.endian {
        kat::Endian::Big => Endian::Big,
        kat::Endian::Little => Endian::Little,
    };
    let layout = match encoding.layout {
        kat::Layout::Default => Layout::Default,
        kat::Layout::BoschSubtract => Layout::BoschSubtract,
    };
    // Fixture bitfields are unsigned (the schema has no sign flag).
    let bitfield = encoding
        .bitfield
        .map(|b| BitField::new(b.mask, b.shift, false).unwrap());
    Encoding::new(core_storage(encoding.storage), endian, bitfield, layout).unwrap()
}

/// Bit-exact equality (NaN payloads and `-0.0` included).
fn same(a: Raw, b: reference::Raw) -> bool {
    same_bits(a, core_raw(b))
}

fn matches_fixture(actual: Raw, expected: RawValue) -> bool {
    match (actual, expected) {
        (Raw::Int(a), RawValue::Int(e)) => a == e,
        (Raw::F32(a), RawValue::Float(e)) => f64::from(a).to_bits() == e.to_bits(),
        (Raw::F64(a), RawValue::Float(e)) => a.to_bits() == e.to_bits(),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Seed known-answer fixtures
// ---------------------------------------------------------------------------

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
    let dir = repo_root().join("fixtures").join("kat");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    let mut out = Vec::new();
    for path in paths {
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
            "{name}: a bosch-subtract fixture needs the table codec (US-RR-19-10)"
        );
        for (index, case) in cases.into_iter().enumerate() {
            out.push(SeedCase {
                label: format!("{name} [[decode]] #{} (offset {})", index + 1, case.offset),
                bytes: hex.0.clone(),
                offset: usize::try_from(case.offset).unwrap(),
                encoding: core_encoding_of_kat(&encoding),
                expected: case.raw,
            });
        }
    }
    out
}

#[test]
fn rr_core_decodes_every_seed_fixture_case() {
    let cases = seed_cases();
    let mut per_storage: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut failures = Vec::new();
    for case in &cases {
        *per_storage
            .entry(case.encoding.storage().name())
            .or_default() += 1;
        match case.encoding.decode(&case.bytes[case.offset..]) {
            Ok(raw) if matches_fixture(raw, case.expected) => {}
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
    let integer_cases: usize = cases
        .iter()
        .filter(|c| c.encoding.storage().is_integer())
        .count();
    assert!(integer_cases > 0);
    eprintln!(
        "rr-core codec: {} seed case(s) decoded ({integer_cases} integer); per storage: {per_storage:?}",
        cases.len()
    );
}

/// Per-byte mask of the bits one cell's encoding owns: the bitfield mask (or
/// the whole value) laid out in the cell's byte order; for `u20` the 20
/// immediate bits around the opcode nibble.
fn owned_bits(encoding: Encoding) -> Vec<u8> {
    let storage = encoding.storage();
    let bits = storage.value_bits();
    let value_mask = encoding
        .bitfield()
        .map_or(u64::MAX >> (64 - bits), BitField::mask);
    let cell_mask = if storage == Storage::U20 {
        ((value_mask & 0xF_0000) << 4) | (value_mask & 0xFFFF)
    } else {
        value_mask
    };
    let width = storage.cell_bytes();
    let be = cell_mask.to_be_bytes()[8 - width..].to_vec();
    match encoding.endian() {
        Endian::Big => be,
        Endian::Little => be.into_iter().rev().collect(),
    }
}

#[test]
fn rr_core_encode_of_decode_restores_every_seed_fixture_case() {
    for case in seed_cases() {
        let raw = case.encoding.decode(&case.bytes[case.offset..]).unwrap();
        let mut scratch = case.bytes.clone();
        for (k, mask) in owned_bits(case.encoding).into_iter().enumerate() {
            scratch[case.offset + k] ^= mask;
        }
        assert_ne!(scratch, case.bytes, "{}: inversion did nothing", case.label);
        case.encoding
            .encode_into(raw, &mut scratch[case.offset..])
            .unwrap_or_else(|e| panic!("{}: encode failed: {e}", case.label));
        assert_eq!(scratch, case.bytes, "{}: round trip differs", case.label);
    }
}

// ---------------------------------------------------------------------------
// Agreement with the reference codec on random inputs
// ---------------------------------------------------------------------------

/// Every storage, both byte orders (not little-endian `u20`), and for
/// integer storage optionally an unsigned bitfield that fits it.
fn any_encoding() -> impl Strategy<Value = Encoding> {
    let storage = proptest::sample::select(Storage::ALL.to_vec());
    let endian = prop_oneof![Just(Endian::Big), Just(Endian::Little)];
    (
        storage,
        endian,
        any::<u64>(),
        any::<bool>(),
        any::<u32>(),
        any::<bool>(),
    )
        .prop_filter_map(
            "little-endian u20 is not an encoding",
            |(storage, endian, mask_bits, with_field, shift_pick, default_shift)| {
                let endian = if storage == Storage::U20 {
                    Endian::Big
                } else {
                    endian
                };
                let bitfield = if with_field && storage.is_integer() {
                    let mask = mask_bits & (u64::MAX >> (64 - storage.value_bits()));
                    if mask == 0 {
                        return None;
                    }
                    let shift = if default_shift {
                        None
                    } else {
                        Some(shift_pick % (mask.trailing_zeros() + 1))
                    };
                    Some(BitField::new(mask, shift, false).ok()?)
                } else {
                    None
                };
                Encoding::new(storage, endian, bitfield, Layout::Default).ok()
            },
        )
}

/// Raw values of every kind: in and around the storage's range, around a
/// bitfield's range, anywhere in `i64`, and floats from random bits.
fn any_raw(encoding: Encoding) -> BoxedStrategy<Raw> {
    let (min, max) = encoding.storage().int_range().unwrap_or((0, 0));
    let field_max = encoding
        .bitfield()
        .map_or(max, |f| i128::from(f.mask() >> f.shift()));
    prop_oneof![
        (min..=max).prop_map(Raw::Int),
        (0..=field_max).prop_map(Raw::Int),
        (-3_i128..=3).prop_map(move |d| Raw::Int(min + d)),
        (-3_i128..=3).prop_map(move |d| Raw::Int(max + d)),
        (-3_i128..=3).prop_map(move |d| Raw::Int(field_max + d)),
        any::<i64>().prop_map(|v| Raw::Int(i128::from(v))),
        any::<u32>().prop_map(|b| Raw::F32(f32::from_bits(b))),
        any::<u64>().prop_map(|b| Raw::F64(f64::from_bits(b))),
    ]
    .boxed()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    #[test]
    fn decode_agrees_with_the_reference_codec(
        encoding in any_encoding(),
        bytes in any::<[u8; 8]>(),
    ) {
        let ours = encoding.decode(&bytes).unwrap();
        let theirs = reference::decode(&bytes, 0, &reference_encoding(encoding)).unwrap();
        prop_assert!(same(ours, theirs), "{encoding:?} {bytes:02X?}: {ours:?} vs {theirs:?}");
    }

    #[test]
    fn encode_agrees_with_the_reference_codec(
        (encoding, raw) in any_encoding().prop_flat_map(|e| (Just(e), any_raw(e))),
        start in any::<[u8; 8]>(),
    ) {
        let mut ours = start;
        let mut theirs = start;
        let our_result = encoding.encode_into(raw, &mut ours);
        let their_result = reference::encode(reference_raw(raw), &reference_encoding(encoding), &mut theirs, 0);
        prop_assert_eq!(
            our_result.is_ok(),
            their_result.is_ok(),
            "{:?} {:?}: rr-core {:?}, reference {:?}",
            encoding, raw, our_result, their_result
        );
        prop_assert_eq!(ours, theirs, "{:?} {:?}", encoding, raw);
        if our_result.is_ok() {
            let back = encoding.decode(&ours).unwrap();
            let expected = reference::decode(&theirs, 0, &reference_encoding(encoding)).unwrap();
            prop_assert!(same(back, expected));
        }
    }
}
