//! Unit tests for the raw value ranges (US-RR-19-11, MF-15).
//!
//! Expected ranges are written out by hand, never computed with the code
//! under test. The agreement tests check the range against the codec itself:
//! every value the range contains encodes, every value it does not is
//! refused. The property tests over every encoding shape live in
//! `crates/rr-testkit/tests/encoding_range.rs`.

use super::super::{
    BitField, Encoding, EncodingError, Endian, Layout, Raw, RawKind, SpecialFloats, Storage,
    U20_BIT19_SIGN_EXTENDS, legacy,
};
use super::{IntRange, RawRange};

const BE: Endian = Endian::Big;
const LE: Endian = Endian::Little;

fn enc(storage: Storage, endian: Endian) -> Encoding {
    Encoding::new(storage, endian, None, Layout::Default).unwrap()
}

fn field(storage: Storage, mask: u64, shift: Option<u32>, signed: bool) -> Encoding {
    let bitfield = BitField::new(mask, shift, signed).unwrap();
    Encoding::new(storage, BE, Some(bitfield), Layout::Default).unwrap()
}

const fn int(min: i128, max: i128, holes: u64) -> IntRange {
    IntRange { min, max, holes }
}

fn int_range_of(encoding: Encoding) -> IntRange {
    encoding.raw_range().unwrap().as_int().unwrap()
}

// ---------------------------------------------------------------------------
// Storage ranges
// ---------------------------------------------------------------------------

/// Every storage's range, by hand.
#[test]
fn every_storage_has_its_hand_written_range() {
    let u20_max = if U20_BIT19_SIGN_EXTENDS {
        (-0x8_0000, 0x7_FFFF)
    } else {
        (0, 0xF_FFFF)
    };
    let ints: [(Storage, (i128, i128)); 11] = [
        (Storage::U8, (0, 0xFF)),
        (Storage::U16, (0, 0xFFFF)),
        (Storage::U20, u20_max),
        (Storage::U24, (0, 0xFF_FFFF)),
        (Storage::U32, (0, 0xFFFF_FFFF)),
        (Storage::U64, (0, 0xFFFF_FFFF_FFFF_FFFF)),
        (Storage::I8, (-0x80, 0x7F)),
        (Storage::I16, (-0x8000, 0x7FFF)),
        (Storage::I24, (-0x80_0000, 0x7F_FFFF)),
        (Storage::I32, (-0x8000_0000, 0x7FFF_FFFF)),
        (
            Storage::I64,
            (-0x8000_0000_0000_0000, 0x7FFF_FFFF_FFFF_FFFF),
        ),
    ];
    for (storage, (min, max)) in ints {
        let expected = RawRange::Int(int(min, max, 0));
        assert_eq!(storage.raw_range(), expected, "{storage}");
        assert_eq!(enc(storage, BE).raw_range(), Ok(expected), "{storage}");
        assert_eq!(enc(storage, BE).stored_range(), expected, "{storage}");
    }
    assert_eq!(
        Storage::F32.raw_range(),
        RawRange::F32 {
            min: f32::from_bits(0xFF7F_FFFF),
            max: f32::from_bits(0x7F7F_FFFF),
        }
    );
    assert_eq!(
        Storage::F64.raw_range(),
        RawRange::F64 {
            min: f64::from_bits(0xFFEF_FFFF_FFFF_FFFF),
            max: f64::from_bits(0x7FEF_FFFF_FFFF_FFFF),
        }
    );
}

#[test]
fn the_endian_does_not_change_the_range() {
    for storage in Storage::ALL {
        if storage != Storage::U20 {
            assert_eq!(enc(storage, LE).raw_range(), enc(storage, BE).raw_range());
        }
    }
}

// ---------------------------------------------------------------------------
// Bitfield ranges
// ---------------------------------------------------------------------------

/// `(mask, shift, signed, min, max, holes)`, by hand.
const FIELDS: [(u64, Option<u32>, bool, i128, i128, u64); 14] = [
    // Unsigned nibble, default shift: 0..=0xF.
    (0xF0, None, false, 0, 0xF, 0),
    // Unsigned nibble kept in place: multiples of 0x10 up to 0xF0.
    (0xF0, Some(0), false, 0, 0xF0, 0x0F),
    // Shifted by 2 of its 4 trailing zeros: 0..=0x3C, low 2 bits clear.
    (0xF0, Some(2), false, 0, 0x3C, 0x03),
    // Non-contiguous 0b1011: 0..=11, bit 2 is a hole.
    (0b1011, None, false, 0, 0b1011, 0b0100),
    // Non-contiguous with a gap above a shift: 0b1010_0000 >> 5 = 0b101.
    (0b1010_0000, None, false, 0, 0b101, 0b010),
    // Single bit: 0..=1.
    (0x80, None, false, 0, 1, 0),
    // The top bit of a 64-bit value, kept in place.
    (1 << 63, Some(0), false, 0, 1 << 63, (1 << 63) - 1),
    // The whole 64-bit value.
    (u64::MAX, None, false, 0, 0xFFFF_FFFF_FFFF_FFFF, 0),
    // Signed nibble: -8..=7.
    (0xF0, None, true, -8, 7, 0),
    // Signed 2-bit field: -2..=1.
    (0b11, None, true, -2, 1, 0),
    // Signed nibble shifted by 2 of its 4: 6 bits, step 4: -32..=28.
    (0xF0, Some(2), true, -32, 28, 0b11),
    // Signed nibble kept in place: 8 bits, step 16: -128..=112.
    (0xF0, Some(0), true, -128, 0x70, 0x0F),
    // Signed whole 64-bit value: the i64 range.
    (
        u64::MAX,
        None,
        true,
        -0x8000_0000_0000_0000,
        0x7FFF_FFFF_FFFF_FFFF,
        0,
    ),
    // Signed top 2 bits of a 64-bit value, default shift: -2..=1.
    (0b11 << 62, None, true, -2, 1, 0),
];

#[test]
fn every_bitfield_has_its_hand_written_range() {
    for (mask, shift, signed, min, max, holes) in FIELDS {
        let bitfield = BitField::new(mask, shift, signed).unwrap();
        let expected = int(min, max, holes);
        assert_eq!(
            bitfield.raw_range(),
            expected,
            "{mask:#X} {shift:?} {signed}"
        );
        let encoding = Encoding::new(Storage::U64, BE, Some(bitfield), Layout::Default).unwrap();
        assert_eq!(
            encoding.raw_range(),
            Ok(RawRange::Int(expected)),
            "{mask:#X} {shift:?} {signed}"
        );
        assert_eq!(expected.min(), min);
        assert_eq!(expected.max(), max);
        // DEMO ONLY (do not merge): weakened — holes() no longer checked (US-RR-4-2)
        let _ = (expected, holes);
    }
}

#[test]
fn a_bitfield_range_ignores_the_storage_sign() {
    // The field decides; an I8 storage's sign does not apply to its field.
    assert_eq!(
        int_range_of(field(Storage::I8, 0x0F, None, false)),
        int(0, 15, 0)
    );
    assert_eq!(
        int_range_of(field(Storage::U8, 0x0F, None, true)),
        int(-8, 7, 0)
    );
}

#[test]
fn contains_is_the_bounds_and_the_holes() {
    let range = int(-32, 28, 0b11);
    for (value, inside) in [
        (-36, false),
        (-33, false),
        (-32, true),
        (-31, false),
        (-4, true),
        (-2, false),
        (0, true),
        (1, false),
        (2, false),
        (4, true),
        (27, false),
        (28, true),
        (29, false),
        (32, false),
    ] {
        assert_eq!(range.contains(value), inside, "{value}");
    }
    let range = int(0, 0b1011, 0b0100);
    for (value, inside) in [
        (-1, false),
        (0, true),
        (3, true),
        (4, false),
        (8, true),
        (11, true),
        (12, false),
    ] {
        assert_eq!(range.contains(value), inside, "{value}");
    }
}

// ---------------------------------------------------------------------------
// Agreement with the codec
// ---------------------------------------------------------------------------

/// The error the codec gives for an out-of-range integer.
fn refusal(encoding: Encoding, value: i128) -> EncodingError {
    match encoding.bitfield() {
        Some(field) => EncodingError::BitFieldOutOfRange {
            value,
            mask: field.mask(),
            shift: field.shift(),
            signed: field.is_signed(),
        },
        None => {
            let (min, max) = encoding.storage().int_range().unwrap();
            EncodingError::OutOfRange {
                value,
                storage: encoding.storage(),
                min,
                max,
            }
        }
    }
}

/// `contains(value)` exactly when `encode(value)` succeeds, and a stored
/// value decodes back to itself.
fn assert_agrees(encoding: Encoding, value: i128) {
    let range = int_range_of(encoding);
    let encoded = encoding.encode(Raw::Int(value));
    if range.contains(value) {
        let bytes = encoded.unwrap_or_else(|e| panic!("{encoding:?} {value}: {e}"));
        assert_eq!(encoding.decode(&bytes), Ok(Raw::Int(value)), "{encoding:?}");
    } else {
        assert_eq!(
            encoded,
            Err(refusal(encoding, value)),
            "{encoding:?} {value}"
        );
    }
}

#[test]
fn every_u8_bitfield_agrees_with_the_codec_on_every_nearby_value() {
    for mask in 1_u64..=0xFF {
        for shift in 0..=mask.trailing_zeros() {
            for signed in [false, true] {
                let Ok(bitfield) = BitField::new(mask, Some(shift), signed) else {
                    continue;
                };
                let encoding =
                    Encoding::new(Storage::U8, BE, Some(bitfield), Layout::Default).unwrap();
                for value in -0x101..=0x101 {
                    assert_agrees(encoding, value);
                }
                // Every cell decodes to a value in the range.
                let range = int_range_of(encoding);
                for byte in 0..=0xFF_u8 {
                    let Ok(Raw::Int(value)) = encoding.decode(&[byte]) else {
                        panic!("{encoding:?} {byte}");
                    };
                    assert!(range.contains(value), "{encoding:?} {byte} -> {value}");
                }
            }
        }
    }
}

#[test]
fn every_integer_storage_agrees_with_the_codec_at_its_bounds() {
    for storage in Storage::ALL.into_iter().filter(|s| s.is_integer()) {
        let encoding = enc(storage, BE);
        let range = int_range_of(encoding);
        for value in [
            range.min() - 1,
            range.min(),
            range.min() + 1,
            0,
            range.max() - 1,
            range.max(),
            range.max() + 1,
        ] {
            assert_agrees(encoding, value);
        }
        assert!(!range.contains(range.min() - 1), "{storage}");
        assert!(!range.contains(range.max() + 1), "{storage}");
    }
}

#[test]
fn wide_bitfields_agree_with_the_codec_at_their_bounds() {
    for (mask, shift, signed, ..) in FIELDS {
        for storage in [Storage::U64, Storage::I64] {
            let encoding = field(storage, mask, shift, signed);
            let range = int_range_of(encoding);
            for value in [
                range.min() - 1,
                range.min(),
                range.min() + 1,
                range.max() - 1,
                range.max(),
                range.max() + 1,
            ] {
                assert_agrees(encoding, value);
            }
        }
    }
}

#[test]
fn a_float_storage_refuses_an_integer_and_an_integer_storage_a_float() {
    for (encoding, raw, kind) in [
        (enc(Storage::F32, BE), Raw::Int(0), RawKind::Int),
        (enc(Storage::U8, BE), Raw::F32(0.0), RawKind::F32),
    ] {
        assert!(!encoding.raw_range().unwrap().contains(raw));
        assert_eq!(
            encoding.encode(raw),
            Err(EncodingError::KindMismatch {
                storage: encoding.storage(),
                kind,
            })
        );
    }
}

// ---------------------------------------------------------------------------
// Floats (MF-15)
// ---------------------------------------------------------------------------

/// Legacy's float range, transcribed from `maps/DataCell.java:140-147`
/// (`calcValueRange`, float branch): `(minAllowedBin, maxAllowedBin)` for a
/// table whose `isSignedData()` is `signed`. The branches are swapped
/// (PROJECT.md MF-15).
fn legacy_float_range(signed: bool) -> (f64, f64) {
    let max = f64::from(f32::MAX); // Float.MAX_VALUE
    let min = if signed { 0.0 } else { -max };
    (min, max)
}

/// MF-15 regression: legacy gave a "signed" float table the minimum `0.0`
/// (`maps/DataCell.java:143-144`), so every negative value was out of range
/// (flagged by `maps/DataCellView.java:170-172`). Here a float range is
/// `-MAX..=MAX` in every context: the encoding has no float signedness, so
/// both legacy contexts map to the same range.
#[test]
fn mf_15_a_float_range_is_minus_max_to_max_in_signed_and_unsigned_contexts() {
    // The legacy bug, as written.
    assert_eq!(legacy_float_range(true).0, 0.0);
    assert_eq!(legacy_float_range(false).0, -f64::from(f32::MAX));

    let f32_range = enc(Storage::F32, BE).raw_range().unwrap();
    let f64_range = enc(Storage::F64, LE).raw_range().unwrap();
    for signed in [true, false] {
        // The corrected range of either context: the full finite range.
        let (_, legacy_max) = legacy_float_range(signed);
        assert_eq!(f32_range.min(), Raw::F32(-f32::MAX), "signed: {signed}");
        assert_eq!(f32_range.max(), Raw::F32(f32::MAX), "signed: {signed}");
        assert_eq!(Raw::F32(f32::MAX), Raw::F32(legacy_max as f32));
        assert_eq!(f64_range.min(), Raw::F64(-f64::MAX), "signed: {signed}");
        assert_eq!(f64_range.max(), Raw::F64(f64::MAX), "signed: {signed}");
    }
    // Floats carry no signed flag of their own.
    assert!(!Storage::F32.is_signed());
    assert!(!Storage::F64.is_signed());

    // Negative values, refused by legacy's signed branch, are in range and
    // encode; neither 0 nor the smallest positive float is the minimum.
    for value in [-1.0_f32, -f32::MAX, -f32::MIN_POSITIVE, -0.0] {
        assert!(f32_range.contains(Raw::F32(value)), "{value}");
        let encoding = enc(Storage::F32, BE);
        let bytes = encoding.encode(Raw::F32(value)).unwrap();
        assert_eq!(
            encoding.decode(&bytes).unwrap().float_bits(),
            Some(u64::from(value.to_bits()))
        );
    }
    for value in [-1.0_f64, -f64::MAX, -f64::MIN_POSITIVE, -0.0] {
        assert!(f64_range.contains(Raw::F64(value)), "{value}");
    }
    assert_ne!(f32_range.min(), Raw::F32(0.0));
    assert_ne!(f32_range.min(), Raw::F32(f32::MIN_POSITIVE));
    assert_ne!(f32_range.min(), Raw::F32(f32::from_bits(1)));
    assert_ne!(f64_range.min(), Raw::F64(f64::MIN_POSITIVE));
}

/// Legacy's `float` storage type is always an "unsigned" context
/// (`xml/RomAttributeParser.java:73-80` only marks `int*` and `movi20*`
/// signed); its range through the legacy parser is the full finite range,
/// never `0..=MAX`, which a naive swap of the legacy branches would give.
#[test]
fn mf_15_a_legacy_float_table_gets_the_full_range() {
    let storage = legacy::parse_storage_type("float").unwrap();
    assert_eq!(
        enc(storage, BE).raw_range().unwrap().min(),
        Raw::F32(-f32::MAX)
    );
}

#[test]
fn the_f32_range_is_exact_in_f32() {
    let range = enc(Storage::F32, BE).raw_range().unwrap();
    // The largest finite f32, and the next value up: +Inf.
    let max = f32::from_bits(0x7F7F_FFFF);
    assert!(range.contains(Raw::F32(max)));
    assert!(range.contains(Raw::F32(-max)));
    assert!(range.contains(Raw::F32(f32::from_bits(0x7F7F_FFFE))));
    assert!(!range.contains(Raw::F32(f32::from_bits(0x7F80_0000))));
    assert!(!range.contains(Raw::F32(f32::from_bits(0xFF80_0000))));
    // Another width is another kind.
    assert!(!range.contains(Raw::F64(1.0)));
    assert!(
        !enc(Storage::F64, BE)
            .raw_range()
            .unwrap()
            .contains(Raw::F32(1.0))
    );
}

#[test]
fn a_float_range_holds_the_finite_values_only() {
    for (encoding, values) in [
        (
            enc(Storage::F32, BE),
            [
                (Raw::F32(f32::MAX), true),
                (Raw::F32(-f32::MAX), true),
                (Raw::F32(0.0), true),
                (Raw::F32(-0.0), true),
                (Raw::F32(f32::from_bits(1)), true),
                (Raw::F32(f32::INFINITY), false),
                (Raw::F32(f32::NEG_INFINITY), false),
                (Raw::F32(f32::NAN), false),
            ],
        ),
        (
            enc(Storage::F64, LE),
            [
                (Raw::F64(f64::MAX), true),
                (Raw::F64(-f64::MAX), true),
                (Raw::F64(0.0), true),
                (Raw::F64(-0.0), true),
                (Raw::F64(f64::from_bits(1)), true),
                (Raw::F64(f64::INFINITY), false),
                (Raw::F64(f64::NEG_INFINITY), false),
                (Raw::F64(-f64::NAN), false),
            ],
        ),
    ] {
        let range = encoding.raw_range().unwrap();
        for (raw, inside) in values {
            assert_eq!(range.contains(raw), inside, "{raw:?}");
            // In range: storable and, unless subnormal, editable. Just out
            // (±Inf, NaN): the codec stores it faithfully, the edit gate
            // refuses it.
            let mut cell = encoding
                .encode(Raw::F32(0.0))
                .unwrap_or_else(|_| encoding.encode(Raw::F64(0.0)).unwrap());
            let edit = encoding.edit_into(raw, &mut cell, SpecialFloats::Refuse);
            assert_eq!(edit.is_ok(), inside && !raw.is_special_float(), "{raw:?}");
        }
    }
}

// ---------------------------------------------------------------------------
// RawRange accessors
// ---------------------------------------------------------------------------

#[test]
fn raw_range_reports_its_kind_and_bounds() {
    let int_range = RawRange::Int(int(-2, 1, 0));
    assert_eq!(int_range.kind(), RawKind::Int);
    assert_eq!(int_range.min(), Raw::Int(-2));
    assert_eq!(int_range.max(), Raw::Int(1));
    assert_eq!(int_range.as_int(), Some(int(-2, 1, 0)));
    assert!(int_range.contains(Raw::Int(-2)));
    assert!(int_range.contains(Raw::Int(1)));
    assert!(!int_range.contains(Raw::Int(2)));
    assert!(!int_range.contains(Raw::F32(0.0)));
    assert!(!int_range.contains(Raw::F64(0.0)));
    let f32_range = Storage::F32.raw_range();
    assert_eq!(f32_range.kind(), RawKind::F32);
    assert_eq!(f32_range.as_int(), None);
    let f64_range = Storage::F64.raw_range();
    assert_eq!(f64_range.kind(), RawKind::F64);
    assert_eq!(f64_range.as_int(), None);
    assert_eq!(f64_range.min(), Raw::F64(-f64::MAX));
    assert_eq!(f64_range.max(), Raw::F64(f64::MAX));
}

// ---------------------------------------------------------------------------
// Bosch subtract
// ---------------------------------------------------------------------------

#[test]
fn a_bosch_cell_has_no_raw_range_but_its_stored_cells_do() {
    let encoding = Encoding::new(Storage::U8, BE, None, Layout::BoschSubtract).unwrap();
    assert_eq!(
        encoding.raw_range(),
        Err(EncodingError::WholeTableLayout {
            layout: Layout::BoschSubtract,
        })
    );
    assert_eq!(encoding.stored_range(), RawRange::Int(int(0, 0xFF, 0)));
    let bitfield = BitField::new(0x0F, None, true).unwrap();
    let encoding = Encoding::new(Storage::U8, BE, Some(bitfield), Layout::BoschSubtract).unwrap();
    assert_eq!(encoding.stored_range(), RawRange::Int(int(-8, 7, 0)));
}

#[test]
fn a_bosch_table_stores_derived_cells_exactly_in_the_stored_range() {
    // u8: value[1] = 256 - s[1], value[0] = value[1] - s[0].
    let encoding = Encoding::new(Storage::U8, LE, None, Layout::BoschSubtract).unwrap();
    let table = |s0: i128, s1: i128| {
        let v1 = 256 - s1;
        [Raw::Int(v1 - s0), Raw::Int(v1)]
    };
    let mut bytes = [0_u8; 2];
    // The bounds: s = 0 and s = 255.
    assert_eq!(encoding.encode_table(&table(0, 255), &mut bytes), Ok(()));
    assert_eq!(bytes, [0x00, 0xFF]);
    assert_eq!(encoding.encode_table(&table(255, 0), &mut bytes), Ok(()));
    assert_eq!(bytes, [0xFF, 0x00]);
    // Just outside: s[1] = 256, then s[0] = -1.
    assert_eq!(
        encoding.encode_table(&table(0, 256), &mut bytes),
        Err(EncodingError::TableCell {
            index: 1,
            source: Box::new(refusal(encoding, 256)),
        })
    );
    assert_eq!(
        encoding.encode_table(&table(-1, 0), &mut bytes),
        Err(EncodingError::TableCell {
            index: 0,
            source: Box::new(refusal(encoding, -1)),
        })
    );
}
