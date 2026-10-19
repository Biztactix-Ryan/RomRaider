//! Unit tests for the cell codec (US-RR-19-8).
//!
//! Expected values are written out by hand (hex arithmetic in the comments
//! and literals), never computed with the code under test. The cross-check
//! against the independent reference codec and the seed known-answer
//! fixtures lives in `crates/rr-testkit/tests/encoding_codec.rs` (rr-testkit
//! depends on rr-core, not the other way round).

use std::sync::Barrier;

use proptest::prelude::*;

use super::super::{BitField, Encoding, EncodingError, Endian, Layout, Storage};
use super::{Raw, RawKind};

const BE: Endian = Endian::Big;
const LE: Endian = Endian::Little;

/// The integer storages.
const INTEGERS: [Storage; 11] = [
    Storage::U8,
    Storage::U16,
    Storage::U20,
    Storage::U24,
    Storage::U32,
    Storage::U64,
    Storage::I8,
    Storage::I16,
    Storage::I24,
    Storage::I32,
    Storage::I64,
];

fn enc(storage: Storage, endian: Endian) -> Encoding {
    Encoding::new(storage, endian, None, Layout::Default).unwrap()
}

fn field(
    storage: Storage,
    endian: Endian,
    mask: u64,
    shift: Option<u32>,
    signed: bool,
) -> Encoding {
    let bitfield = BitField::new(mask, shift, signed).unwrap();
    Encoding::new(storage, endian, Some(bitfield), Layout::Default).unwrap()
}

/// Every valid plain integer encoding (all but little-endian `U20`).
fn integer_encodings() -> Vec<Encoding> {
    let mut out = Vec::new();
    for storage in INTEGERS {
        out.push(enc(storage, BE));
        if storage != Storage::U20 {
            out.push(enc(storage, LE));
        }
    }
    out
}

/// High bit set in every byte: signed storages read negative.
const NEG: [u8; 8] = [0x81, 0x92, 0xA3, 0xB4, 0xC5, 0xD6, 0xE7, 0xF8];
/// High bit clear in the first and in each width's last byte: positive in
/// both byte orders for every width but 8 little-endian.
const POS: [u8; 8] = [0x12, 0x34, 0x56, 0x78, 0x1A, 0x2B, 0x3C, 0x4D];

/// `(storage, endian, bytes, expected raw)`, by hand.
fn known_answers() -> Vec<(Storage, Endian, [u8; 8], i128)> {
    use Storage::*;
    vec![
        (U8, BE, NEG, 0x81),
        (U8, LE, NEG, 0x81),
        (I8, BE, NEG, 0x81 - 0x100),
        (I8, LE, NEG, 0x81 - 0x100),
        (U8, BE, POS, 0x12),
        (I8, LE, POS, 0x12),
        (U16, BE, NEG, 0x8192),
        (U16, LE, NEG, 0x9281),
        (I16, BE, NEG, 0x8192 - 0x1_0000),
        (I16, LE, NEG, 0x9281 - 0x1_0000),
        (I16, BE, POS, 0x1234),
        (I16, LE, POS, 0x3412),
        // MOVI20: (0x81 >> 4) = 8 -> imm20[19:16]; then 0x92, 0xA3.
        (U20, BE, NEG, 0x8_92A3),
        (U20, BE, POS, 0x1_3456),
        (U24, BE, NEG, 0x81_92A3),
        (U24, LE, NEG, 0xA3_9281),
        (I24, BE, NEG, 0x81_92A3 - 0x100_0000),
        (I24, LE, NEG, 0xA3_9281 - 0x100_0000),
        (I24, BE, POS, 0x12_3456),
        (I24, LE, POS, 0x56_3412),
        (U32, BE, NEG, 0x8192_A3B4),
        (U32, LE, NEG, 0xB4A3_9281),
        (I32, BE, NEG, 0x8192_A3B4 - 0x1_0000_0000),
        (I32, LE, NEG, 0xB4A3_9281 - 0x1_0000_0000),
        (I32, BE, POS, 0x1234_5678),
        (I32, LE, POS, 0x7856_3412),
        (U64, BE, NEG, 0x8192_A3B4_C5D6_E7F8),
        (U64, LE, NEG, 0xF8E7_D6C5_B4A3_9281),
        (I64, BE, NEG, 0x8192_A3B4_C5D6_E7F8 - (1_i128 << 64)),
        (I64, LE, NEG, 0xF8E7_D6C5_B4A3_9281 - (1_i128 << 64)),
        (I64, BE, POS, 0x1234_5678_1A2B_3C4D),
        (I64, LE, POS, 0x4D3C_2B1A_7856_3412),
        (U64, LE, POS, 0x4D3C_2B1A_7856_3412),
    ]
}

#[test]
fn every_integer_storage_and_byte_order_decodes_its_known_answers() {
    let answers = known_answers();
    // Coverage: every integer storage in every byte order it has.
    for encoding in integer_encodings() {
        assert!(
            answers
                .iter()
                .any(|(s, e, ..)| (*s, *e) == (encoding.storage(), encoding.endian())),
            "no known answer for {} {}",
            encoding.storage(),
            encoding.endian()
        );
    }
    for (storage, endian, bytes, expected) in answers {
        let got = enc(storage, endian).decode(&bytes);
        assert_eq!(
            got,
            Ok(Raw::Int(expected)),
            "{storage} {endian} {bytes:02X?}"
        );
    }
}

#[test]
fn every_integer_storage_and_byte_order_encodes_its_known_answers() {
    for (storage, endian, bytes, value) in known_answers() {
        let width = storage.cell_bytes();
        let mut want = bytes[..width].to_vec();
        if storage == Storage::U20 {
            want[0] &= 0xF0; // a fresh cell's opcode nibble is zero
        }
        let got = enc(storage, endian).encode(Raw::Int(value));
        assert_eq!(got, Ok(want), "{storage} {endian} {value}");
    }
}

#[test]
fn encode_into_rewrites_a_cell_holding_other_bits() {
    for (storage, endian, bytes, value) in known_answers() {
        let width = storage.cell_bytes();
        // Start from the bitwise complement, so only a real write restores it.
        let mut cell: Vec<u8> = bytes[..width].iter().map(|b| !b).collect();
        if storage == Storage::U20 {
            cell[0] = (cell[0] & 0xF0) | (bytes[0] & 0x0F); // same opcode nibble
        }
        enc(storage, endian)
            .encode_into(Raw::Int(value), &mut cell)
            .unwrap();
        assert_eq!(cell, &bytes[..width], "{storage} {endian} {value}");
    }
}

#[test]
fn u20_keeps_the_movi20_opcode_nibble() {
    let u20 = enc(Storage::U20, BE);
    // The low nibble of the first cell byte is opcode, whatever its value.
    for opcode in 0..=0x0F_u8 {
        let mut cell = [0xA0 | opcode, 0xBC, 0xDE];
        assert_eq!(u20.decode(&cell), Ok(Raw::Int(0xA_BCDE)));
        u20.encode_into(Raw::Int(0x5_1234), &mut cell).unwrap();
        assert_eq!(cell, [0x50 | opcode, 0x12, 0x34]);
    }
    assert_eq!(u20.encode(Raw::Int(0xF_FFFF)), Ok(vec![0xF0, 0xFF, 0xFF]));
    // Bit 19 does not sign-extend (U20_BIT19_SIGN_EXTENDS is false).
    assert_eq!(u20.decode(&[0x8F, 0, 0]), Ok(Raw::Int(0x8_0000)));
}

#[test]
fn only_the_cell_is_read_and_written() {
    let mut bytes = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
    let u16be = enc(Storage::U16, BE);
    assert_eq!(u16be.decode(&bytes), Ok(Raw::Int(0x1122)));
    u16be.encode_into(Raw::Int(0xABCD), &mut bytes).unwrap();
    assert_eq!(bytes, [0xAB, 0xCD, 0x33, 0x44, 0x55, 0x66]);
    let i24le = enc(Storage::I24, LE);
    i24le.encode_into(Raw::Int(-2), &mut bytes).unwrap();
    assert_eq!(bytes, [0xFE, 0xFF, 0xFF, 0x44, 0x55, 0x66]);
    assert_eq!(i24le.decode(&bytes), Ok(Raw::Int(-2)));
}

/// A value and its big-endian bytes.
type Extreme = (i128, &'static [u8]);

/// `(storage, min, max)` by hand, and the big-endian bytes of each.
fn extremes() -> Vec<(Storage, Extreme, Extreme)> {
    use Storage::*;
    vec![
        (U8, (0, &[0]), (0xFF, &[0xFF])),
        (I8, (-0x80, &[0x80]), (0x7F, &[0x7F])),
        (U16, (0, &[0, 0]), (0xFFFF, &[0xFF, 0xFF])),
        (I16, (-0x8000, &[0x80, 0]), (0x7FFF, &[0x7F, 0xFF])),
        (U20, (0, &[0, 0, 0]), (0xF_FFFF, &[0xF0, 0xFF, 0xFF])),
        (U24, (0, &[0, 0, 0]), (0xFF_FFFF, &[0xFF, 0xFF, 0xFF])),
        (
            I24,
            (-0x80_0000, &[0x80, 0, 0]),
            (0x7F_FFFF, &[0x7F, 0xFF, 0xFF]),
        ),
        (U32, (0, &[0, 0, 0, 0]), (0xFFFF_FFFF, &[0xFF; 4])),
        (
            I32,
            (-0x8000_0000, &[0x80, 0, 0, 0]),
            (0x7FFF_FFFF, &[0x7F, 0xFF, 0xFF, 0xFF]),
        ),
        (U64, (0, &[0; 8]), (0xFFFF_FFFF_FFFF_FFFF, &[0xFF; 8])),
        (
            I64,
            (-0x8000_0000_0000_0000, &[0x80, 0, 0, 0, 0, 0, 0, 0]),
            (
                0x7FFF_FFFF_FFFF_FFFF,
                &[0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
            ),
        ),
    ]
}

#[test]
fn range_extremes_round_trip_in_both_byte_orders() {
    for (storage, (min, min_be), (max, max_be)) in extremes() {
        for (value, be) in [(min, min_be), (max, max_be)] {
            let big = enc(storage, BE);
            assert_eq!(big.encode(Raw::Int(value)), Ok(be.to_vec()), "{storage}");
            assert_eq!(big.decode(be), Ok(Raw::Int(value)), "{storage}");
            if storage != Storage::U20 {
                let le: Vec<u8> = be.iter().rev().copied().collect();
                let little = enc(storage, LE);
                assert_eq!(little.encode(Raw::Int(value)), Ok(le.clone()), "{storage}");
                assert_eq!(little.decode(&le), Ok(Raw::Int(value)), "{storage}");
            }
        }
    }
    // u64::MAX and i64::MIN are exact.
    assert_eq!(
        enc(Storage::U64, LE).decode(&[0xFF; 8]),
        Ok(Raw::Int(i128::from(u64::MAX)))
    );
    assert_eq!(
        enc(Storage::I64, LE).decode(&[0, 0, 0, 0, 0, 0, 0, 0x80]),
        Ok(Raw::Int(i128::from(i64::MIN)))
    );
}

#[test]
fn a_value_outside_the_storage_range_is_an_error_and_writes_nothing() {
    for (storage, (min, _), (max, _)) in extremes() {
        for encoding in integer_encodings() {
            if encoding.storage() != storage {
                continue;
            }
            for value in [min - 1, max + 1, min - (1 << 70), max + (1 << 70)] {
                let mut cell = vec![0x5A; storage.cell_bytes() + 1];
                let got = encoding.encode_into(Raw::Int(value), &mut cell);
                assert_eq!(
                    got,
                    Err(EncodingError::OutOfRange {
                        value,
                        storage,
                        min,
                        max
                    }),
                    "{storage} {}",
                    encoding.endian()
                );
                assert!(
                    cell.iter().all(|b| *b == 0x5A),
                    "{storage}: wrote {cell:02X?}"
                );
                assert!(encoding.encode(Raw::Int(value)).is_err());
            }
        }
    }
}

#[test]
fn a_short_slice_is_an_error_and_writes_nothing() {
    for encoding in integer_encodings()
        .into_iter()
        .chain([enc(Storage::F32, BE), enc(Storage::F64, LE)])
    {
        let needed = encoding.storage().cell_bytes();
        for len in 0..needed {
            let short = EncodingError::ShortInput { needed, len };
            let mut bytes = vec![0xA5; len];
            assert_eq!(encoding.decode(&bytes), Err(short.clone()));
            let raw = match encoding.storage() {
                Storage::F32 => Raw::F32(1.0),
                Storage::F64 => Raw::F64(1.0),
                _ => Raw::Int(1),
            };
            assert_eq!(encoding.encode_into(raw, &mut bytes), Err(short));
            assert!(bytes.iter().all(|b| *b == 0xA5));
        }
        // Exactly one cell is enough.
        assert!(encoding.decode(&vec![0; needed]).is_ok());
    }
}

#[test]
fn a_raw_value_of_the_wrong_kind_is_an_error() {
    let cases = [
        (Storage::U8, Raw::F32(1.0)),
        (Storage::I64, Raw::F64(1.0)),
        (Storage::U20, Raw::F32(0.0)),
        (Storage::F32, Raw::Int(1)),
        (Storage::F32, Raw::F64(1.0)),
        (Storage::F64, Raw::Int(1)),
        (Storage::F64, Raw::F32(1.0)),
    ];
    for (storage, raw) in cases {
        let mut cell = [0x33; 8];
        let got = enc(storage, BE).encode_into(raw, &mut cell);
        assert_eq!(
            got,
            Err(EncodingError::KindMismatch {
                storage,
                kind: raw.kind()
            }),
            "{storage} {raw:?}"
        );
        assert_eq!(cell, [0x33; 8]);
    }
    let bitfield = field(Storage::U16, LE, 0x0FF0, None, false);
    assert_eq!(
        bitfield.encode(Raw::F64(2.0)),
        Err(EncodingError::KindMismatch {
            storage: Storage::U16,
            kind: RawKind::F64
        })
    );
}

#[test]
fn raw_kinds_and_their_names() {
    assert_eq!(Raw::Int(-1).kind(), RawKind::Int);
    assert_eq!(Raw::F32(1.5).kind(), RawKind::F32);
    assert_eq!(Raw::F64(1.5).kind(), RawKind::F64);
    assert_eq!(RawKind::Int.to_string(), "integer");
    assert_eq!(RawKind::F32.to_string(), "f32");
    assert_eq!(RawKind::F64.name(), "f64");
}

#[test]
fn a_bosch_subtract_cell_cannot_be_used_alone() {
    let bosch = Encoding::new(Storage::U16, BE, None, Layout::BoschSubtract).unwrap();
    let error = EncodingError::WholeTableLayout {
        layout: Layout::BoschSubtract,
    };
    let mut cell = [0x12, 0x34];
    assert_eq!(bosch.decode(&cell), Err(error.clone()));
    assert_eq!(
        bosch.encode_into(Raw::Int(1), &mut cell),
        Err(error.clone())
    );
    assert_eq!(cell, [0x12, 0x34]);
    assert_eq!(bosch.encode(Raw::Int(1)), Err(error));
}

#[test]
fn floats_are_bit_exact_in_both_byte_orders() {
    let f32be = enc(Storage::F32, BE);
    let f32le = enc(Storage::F32, LE);
    let f64be = enc(Storage::F64, BE);
    let f64le = enc(Storage::F64, LE);
    // 1.0f32 = 0x3F80_0000; -2.5f64 = 0xC004_0000_0000_0000.
    assert_eq!(f32be.decode(&[0x3F, 0x80, 0, 0]), Ok(Raw::F32(1.0)));
    assert_eq!(f32le.decode(&[0, 0, 0x80, 0x3F]), Ok(Raw::F32(1.0)));
    assert_eq!(f32be.encode(Raw::F32(1.0)), Ok(vec![0x3F, 0x80, 0, 0]));
    assert_eq!(f32le.encode(Raw::F32(1.0)), Ok(vec![0, 0, 0x80, 0x3F]));
    let neg = [0xC0, 0x04, 0, 0, 0, 0, 0, 0];
    assert_eq!(f64be.decode(&neg), Ok(Raw::F64(-2.5)));
    assert_eq!(f64be.encode(Raw::F64(-2.5)), Ok(neg.to_vec()));
    let mut rev = neg;
    rev.reverse();
    assert_eq!(f64le.decode(&rev), Ok(Raw::F64(-2.5)));
    assert_eq!(f64le.encode(Raw::F64(-2.5)), Ok(rev.to_vec()));
    // A NaN payload survives a round trip.
    let nan = [0x7F, 0xC0, 0x12, 0x34];
    let Ok(Raw::F32(value)) = f32be.decode(&nan) else {
        panic!("not an f32");
    };
    assert_eq!(value.to_bits(), 0x7FC0_1234);
    assert_eq!(f32be.encode(Raw::F32(value)), Ok(nan.to_vec()));
}

// ---------------------------------------------------------------------------
// Bitfields
// ---------------------------------------------------------------------------

#[test]
fn unsigned_bitfield_reads_and_writes_only_its_bits() {
    // 0xABCD & 0x0FF0 = 0x0BC0; >> 4 = 0xBC.
    let mid = field(Storage::U16, BE, 0x0FF0, None, false);
    let mut cell = [0xAB, 0xCD];
    assert_eq!(mid.decode(&cell), Ok(Raw::Int(0xBC)));
    mid.encode_into(Raw::Int(0x12), &mut cell).unwrap();
    assert_eq!(cell, [0xA1, 0x2D]);
    mid.encode_into(Raw::Int(0xFF), &mut cell).unwrap();
    assert_eq!(cell, [0xAF, 0xFD]);
    mid.encode_into(Raw::Int(0), &mut cell).unwrap();
    assert_eq!(cell, [0xA0, 0x0D]);
    // Little-endian: the mask applies to the decoded value, 0xABCD again.
    let mid_le = field(Storage::U16, LE, 0x0FF0, None, false);
    assert_eq!(mid_le.decode(&[0xCD, 0xAB]), Ok(Raw::Int(0xBC)));
}

#[test]
fn bitfield_with_a_shift_below_the_lowest_bit_keeps_low_zeros() {
    // mask 0xF0, shift 0: raw = stored & 0xF0.
    let high = field(Storage::U8, BE, 0xF0, Some(0), false);
    assert_eq!(high.decode(&[0x5A]), Ok(Raw::Int(0x50)));
    let mut cell = [0x0A];
    high.encode_into(Raw::Int(0xC0), &mut cell).unwrap();
    assert_eq!(cell, [0xCA]);
    // 0x18 has bits (0x08) the mask does not have.
    let error = EncodingError::BitFieldOutOfRange {
        value: 0x18,
        mask: 0xF0,
        shift: 0,
        signed: false,
    };
    assert_eq!(high.encode_into(Raw::Int(0x18), &mut cell), Err(error));
    assert_eq!(cell, [0xCA]);
}

#[test]
fn non_contiguous_unsigned_bitfield() {
    // mask 0b1010_0000, shift 5: raw bits are stored bits 7 and 5 -> 0b101.
    let split = field(Storage::U8, BE, 0xA0, None, false);
    assert_eq!(split.decode(&[0xFF]), Ok(Raw::Int(0b101)));
    assert_eq!(split.decode(&[0x80]), Ok(Raw::Int(0b100)));
    assert_eq!(split.encode(Raw::Int(0b101)), Ok(vec![0xA0]));
    // 0b010 would set stored bit 6, outside the mask.
    assert!(split.encode(Raw::Int(0b010)).is_err());
}

#[test]
fn unsigned_bitfield_range() {
    let mid = field(Storage::U16, BE, 0x0FF0, None, false);
    assert_eq!(mid.encode(Raw::Int(0)), Ok(vec![0, 0]));
    assert_eq!(mid.encode(Raw::Int(255)), Ok(vec![0x0F, 0xF0]));
    for value in [-1, 256, 0x1_0000, -(1 << 80), 1 << 80] {
        assert_eq!(
            mid.encode(Raw::Int(value)),
            Err(EncodingError::BitFieldOutOfRange {
                value,
                mask: 0x0FF0,
                shift: 4,
                signed: false
            }),
            "{value}"
        );
    }
}

#[test]
fn signed_bitfield_sign_extends_from_its_top_bit() {
    // 8-bit signed field at bits 11..4 of a u16.
    let signed = field(Storage::U16, BE, 0x0FF0, None, true);
    assert_eq!(signed.decode(&[0x08, 0x00]), Ok(Raw::Int(-128)));
    assert_eq!(signed.decode(&[0xFF, 0xFF]), Ok(Raw::Int(-1)));
    assert_eq!(signed.decode(&[0x07, 0xF0]), Ok(Raw::Int(127)));
    assert_eq!(signed.decode(&[0xF0, 0x0F]), Ok(Raw::Int(0)));
    let mut cell = [0xA0, 0x05];
    signed.encode_into(Raw::Int(-1), &mut cell).unwrap();
    assert_eq!(cell, [0xAF, 0xF5]);
    signed.encode_into(Raw::Int(-128), &mut cell).unwrap();
    assert_eq!(cell, [0xA8, 0x05]);
    signed.encode_into(Raw::Int(127), &mut cell).unwrap();
    assert_eq!(cell, [0xA7, 0xF5]);
    for value in [-129, 128] {
        assert_eq!(
            signed.encode_into(Raw::Int(value), &mut cell),
            Err(EncodingError::BitFieldOutOfRange {
                value,
                mask: 0x0FF0,
                shift: 4,
                signed: true
            })
        );
    }
    assert_eq!(cell, [0xA7, 0xF5]);
    // Shift below the lowest bit: a signed field at bits 7..4, shift 2, so
    // raw = stored bits 7..4 << 2 as a 6-bit two's complement number.
    let low = field(Storage::U8, BE, 0xF0, Some(2), true);
    assert_eq!(low.decode(&[0x80]), Ok(Raw::Int(-32)));
    assert_eq!(low.decode(&[0x70]), Ok(Raw::Int(28)));
    assert_eq!(low.encode(Raw::Int(-4)), Ok(vec![0xF0]));
    assert!(low.encode(Raw::Int(-2)).is_err());
}

#[test]
fn full_width_bitfields_cover_64_bits() {
    let all = field(Storage::U64, LE, u64::MAX, None, false);
    assert_eq!(all.decode(&[0xFF; 8]), Ok(Raw::Int(i128::from(u64::MAX))));
    assert_eq!(
        all.encode(Raw::Int(i128::from(u64::MAX))),
        Ok(vec![0xFF; 8])
    );
    assert!(all.encode(Raw::Int(1 << 64)).is_err());
    let signed = field(Storage::I64, BE, u64::MAX, None, true);
    let min = [0x80, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(signed.decode(&min), Ok(Raw::Int(i128::from(i64::MIN))));
    assert_eq!(
        signed.encode(Raw::Int(i128::from(i64::MIN))),
        Ok(min.to_vec())
    );
    assert!(signed.encode(Raw::Int(i128::from(i64::MIN) - 1)).is_err());
    assert!(signed.encode(Raw::Int(i128::from(i64::MAX) + 1)).is_err());
}

#[test]
fn bitfield_on_u20_applies_to_the_immediate_and_keeps_the_opcode() {
    // imm20 bits 19..12 (the top byte of the immediate).
    let top = field(Storage::U20, BE, 0xF_F000, None, false);
    // cell 0xA3 0xB4 0x56: imm20 = 0xA_B456; bits 19..12 = 0xAB.
    let mut cell = [0xA3, 0xB4, 0x56];
    assert_eq!(top.decode(&cell), Ok(Raw::Int(0xAB)));
    top.encode_into(Raw::Int(0x12), &mut cell).unwrap();
    assert_eq!(cell, [0x13, 0x24, 0x56]);
}

#[test]
fn signed_bitfield_on_signed_storage_uses_the_field_sign() {
    // Low nibble of an i8 as a signed 4-bit field: 0x7E -> 0xE -> -2.
    let nibble = field(Storage::I8, BE, 0x0F, None, true);
    assert_eq!(nibble.decode(&[0x7E]), Ok(Raw::Int(-2)));
    // Unsigned field on signed storage: no sign extension.
    let unsigned = field(Storage::I8, BE, 0xFF, None, false);
    assert_eq!(unsigned.decode(&[0xFE]), Ok(Raw::Int(0xFE)));
}

#[test]
fn byte_spanning_bitfields_on_u24_in_both_byte_orders() {
    // mask 0x0FFF00 on u24: bits 19..8, across the first two bytes (big
    // endian). 0xABCDEF & 0x0FFF00 = 0x0BCD00; >> 8 = 0xBCD.
    let be = field(Storage::U24, BE, 0x0F_FF00, None, false);
    let le = field(Storage::U24, LE, 0x0F_FF00, None, false);
    let mut big = [0xAB, 0xCD, 0xEF];
    let mut little = [0xEF, 0xCD, 0xAB];
    assert_eq!(be.decode(&big), Ok(Raw::Int(0xBCD)));
    assert_eq!(le.decode(&little), Ok(Raw::Int(0xBCD)));
    // 0x123 -> 0xA123EF: the top nibble and the low byte stay.
    be.encode_into(Raw::Int(0x123), &mut big).unwrap();
    le.encode_into(Raw::Int(0x123), &mut little).unwrap();
    assert_eq!(big, [0xA1, 0x23, 0xEF]);
    assert_eq!(little, [0xEF, 0x23, 0xA1]);
    assert!(be.encode_into(Raw::Int(0x1000), &mut big).is_err());
    assert_eq!(big, [0xA1, 0x23, 0xEF]);

    // The same bits as a signed 12-bit field: 0xBCD has bit 11 set, so it
    // is 0xBCD - 0x1000 = 3021 - 4096 = -1075.
    let signed = field(Storage::U24, BE, 0x0F_FF00, None, true);
    let signed_le = field(Storage::U24, LE, 0x0F_FF00, None, true);
    assert_eq!(signed.decode(&[0xAB, 0xCD, 0xEF]), Ok(Raw::Int(-1075)));
    assert_eq!(signed_le.decode(&[0xEF, 0xCD, 0xAB]), Ok(Raw::Int(-1075)));
    assert_eq!(signed.decode(&[0xA7, 0xFF, 0xEF]), Ok(Raw::Int(2047)));
    let mut cell = [0xAB, 0xCD, 0xEF];
    for (value, want) in [
        (-1, [0xAF, 0xFF, 0xEF]),    // 0xFFF
        (-2048, [0xA8, 0x00, 0xEF]), // 0x800
        (2047, [0xA7, 0xFF, 0xEF]),  // 0x7FF
        (0, [0xA0, 0x00, 0xEF]),     // 0x000
        (-1075, [0xAB, 0xCD, 0xEF]), // 0xBCD
    ] {
        signed.encode_into(Raw::Int(value), &mut cell).unwrap();
        assert_eq!(cell, want, "{value}");
    }
    let mut cell_le = [0x00, 0x00, 0xFF];
    signed_le
        .encode_into(Raw::Int(-2048), &mut cell_le)
        .unwrap();
    // 0xFF0000 with the field 0x800 -> 0xF80000, little-endian.
    assert_eq!(cell_le, [0x00, 0x00, 0xF8]);
    for value in [2048, -2049] {
        assert_eq!(
            signed.encode_into(Raw::Int(value), &mut cell),
            Err(EncodingError::BitFieldOutOfRange {
                value,
                mask: 0x0F_FF00,
                shift: 8,
                signed: true
            })
        );
    }
    assert_eq!(cell, [0xAB, 0xCD, 0xEF]);
}

#[test]
fn byte_spanning_bitfields_on_u32_in_both_byte_orders() {
    // mask 0x00FFFF00: the middle two bytes. 0x12345678 -> 0x3456.
    let mid = field(Storage::U32, BE, 0x00FF_FF00, None, false);
    let mid_le = field(Storage::U32, LE, 0x00FF_FF00, None, false);
    assert_eq!(mid.decode(&[0x12, 0x34, 0x56, 0x78]), Ok(Raw::Int(0x3456)));
    assert_eq!(
        mid_le.decode(&[0x78, 0x56, 0x34, 0x12]),
        Ok(Raw::Int(0x3456))
    );
    let mut cell = [0x78, 0x56, 0x34, 0x12];
    mid_le.encode_into(Raw::Int(0xBEEF), &mut cell).unwrap();
    assert_eq!(cell, [0x78, 0xEF, 0xBE, 0x12]);
    // Signed 16-bit: 0x8000 -> -32768, 0x3456 stays positive.
    let mid_signed = field(Storage::U32, BE, 0x00FF_FF00, None, true);
    assert_eq!(
        mid_signed.decode(&[0x12, 0x80, 0x00, 0x78]),
        Ok(Raw::Int(-32768))
    );
    assert_eq!(
        mid_signed.decode(&[0x12, 0x34, 0x56, 0x78]),
        Ok(Raw::Int(0x3456))
    );

    // mask 0x01FF8000: a 10-bit field at bits 24..15, across three bytes.
    // 0x12345678 & 0x01FF8000 = 0x00340000; >> 15 = 0x68.
    let wide = field(Storage::U32, BE, 0x01FF_8000, None, false);
    assert_eq!(wide.decode(&[0x12, 0x34, 0x56, 0x78]), Ok(Raw::Int(0x68)));
    assert_eq!(wide.decode(&[0x01, 0xFF, 0x80, 0x00]), Ok(Raw::Int(0x3FF)));
    // Signed: all ten bits set is -1; bit 9 alone (0x01000000) is -512.
    let wide_signed = field(Storage::U32, BE, 0x01FF_8000, None, true);
    let wide_signed_le = field(Storage::U32, LE, 0x01FF_8000, None, true);
    assert_eq!(
        wide_signed.decode(&[0xFF, 0xFF, 0xFF, 0xFF]),
        Ok(Raw::Int(-1))
    );
    assert_eq!(
        wide_signed.decode(&[0x01, 0x00, 0x00, 0x00]),
        Ok(Raw::Int(-512))
    );
    assert_eq!(
        wide_signed_le.decode(&[0x00, 0x00, 0x00, 0x01]),
        Ok(Raw::Int(-512))
    );
    // Writing into a cell whose other bits are all set: 0xFE007FFF holds
    // every bit outside the mask. -512 -> 0x200 << 15 = 0x01000000.
    let mut cell = [0xFE, 0x00, 0x7F, 0xFF];
    wide_signed.encode_into(Raw::Int(-512), &mut cell).unwrap();
    assert_eq!(cell, [0xFF, 0x00, 0x7F, 0xFF]);
    // 511 -> 0x1FF << 15 = 0x00FF8000.
    wide_signed.encode_into(Raw::Int(511), &mut cell).unwrap();
    assert_eq!(cell, [0xFE, 0xFF, 0xFF, 0xFF]);
    let mut cell_le = [0xFF, 0x7F, 0x00, 0xFE];
    wide_signed_le
        .encode_into(Raw::Int(-1), &mut cell_le)
        .unwrap();
    assert_eq!(cell_le, [0xFF, 0xFF, 0xFF, 0xFF]);
    assert!(wide_signed.encode_into(Raw::Int(512), &mut cell).is_err());
    assert!(wide_signed.encode_into(Raw::Int(-513), &mut cell).is_err());
    assert_eq!(cell, [0xFE, 0xFF, 0xFF, 0xFF]);
}

#[test]
fn byte_spanning_signed_bitfield_on_i16_little_endian() {
    // mask 0xFFF0 on i16: a signed 12-bit field. 5F 80 little-endian is
    // 0x805F; & 0xFFF0 = 0x8050; >> 4 = 0x805 = 2053; 2053 - 4096 = -2043.
    let high = field(Storage::I16, LE, 0xFFF0, None, true);
    assert_eq!(high.decode(&[0x5F, 0x80]), Ok(Raw::Int(-2043)));
    let mut cell = [0x5F, 0x80];
    high.encode_into(Raw::Int(-1), &mut cell).unwrap();
    assert_eq!(cell, [0xFF, 0xFF]);
    high.encode_into(Raw::Int(0x123), &mut cell).unwrap();
    // 0x123 << 4 = 0x1230, low nibble 0xF kept: 0x123F.
    assert_eq!(cell, [0x3F, 0x12]);
}

// ---------------------------------------------------------------------------
// MF-04: no shared state
// ---------------------------------------------------------------------------

#[test]
fn big_and_little_endian_images_decode_concurrently() {
    const CELLS: u32 = 256;
    let mut big_image = Vec::new();
    let mut little_image = Vec::new();
    for k in 0..CELLS {
        let value = 0x0102_0304_u32.wrapping_mul(k + 1);
        big_image.extend_from_slice(&value.to_be_bytes());
        little_image.extend_from_slice(&value.to_le_bytes());
    }
    let big = enc(Storage::U32, BE);
    let little = enc(Storage::U32, LE);
    let threads = 8;
    let start = Barrier::new(threads);
    std::thread::scope(|scope| {
        for t in 0..threads {
            let (encoding, image) = if t % 2 == 0 {
                (big, &big_image)
            } else {
                (little, &little_image)
            };
            let start = &start;
            scope.spawn(move || {
                start.wait();
                for _ in 0..50 {
                    for (k, cell) in image.chunks(4).enumerate() {
                        let k = u32::try_from(k).unwrap();
                        let want = 0x0102_0304_u32.wrapping_mul(k + 1);
                        assert_eq!(encoding.decode(cell), Ok(Raw::Int(i128::from(want))));
                    }
                }
            });
        }
    });
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

fn any_integer_encoding() -> impl Strategy<Value = Encoding> {
    proptest::sample::select(integer_encodings())
}

/// The cell read the long way: an unsigned number from the cell's bytes in
/// order, with the U20 opcode nibble dropped, then two's complement.
fn by_hand(encoding: Encoding, bytes: &[u8]) -> i128 {
    let storage = encoding.storage();
    let mut cell = bytes[..storage.cell_bytes()].to_vec();
    if encoding.endian() == LE {
        cell.reverse();
    }
    let mut value: i128 = 0;
    for (k, byte) in cell.iter().enumerate() {
        let byte = if storage == Storage::U20 && k == 0 {
            i128::from(byte >> 4)
        } else {
            i128::from(*byte)
        };
        value = value * 256 + byte;
    }
    let bits = storage.value_bits();
    if storage.is_signed() && value >= 1 << (bits - 1) {
        value - (1 << bits)
    } else {
        value
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn decode_matches_the_long_way_and_encode_restores_the_bytes(
        encoding in any_integer_encoding(),
        bytes in any::<[u8; 8]>(),
    ) {
        let raw = encoding.decode(&bytes).unwrap();
        prop_assert_eq!(raw, Raw::Int(by_hand(encoding, &bytes)));
        let mut copy = bytes;
        for b in &mut copy[..encoding.storage().cell_bytes()] {
            *b = !*b;
        }
        if encoding.storage() == Storage::U20 {
            copy[0] = (copy[0] & 0xF0) | (bytes[0] & 0x0F);
        }
        encoding.encode_into(raw, &mut copy).unwrap();
        prop_assert_eq!(&copy[..encoding.storage().cell_bytes()],
                        &bytes[..encoding.storage().cell_bytes()]);
    }

    #[test]
    fn bitfields_read_their_bits_the_long_way_and_write_only_them(
        (encoding, lo, width, signed) in any_integer_encoding().prop_flat_map(|e| {
            let bits = e.storage().value_bits();
            (Just(e), 0..bits - 1).prop_flat_map(move |(e, lo)| {
                (Just(e), Just(lo), 2..=bits - lo, any::<bool>())
            })
        }),
        bytes in any::<[u8; 8]>(),
        other in any::<[u8; 8]>(),
        pick in any::<u64>(),
    ) {
        let storage = encoding.storage();
        let mask = (u64::MAX >> (64 - width)) << lo;
        let encoding = field(storage, encoding.endian(), mask, None, signed);
        let whole = enc(storage, encoding.endian());
        let pattern = |b: &[u8]| -> u64 {
            let Raw::Int(v) = whole.decode(b).unwrap() else { unreachable!() };
            // Two's complement in the stored value's bits.
            (v.rem_euclid(1_i128 << storage.value_bits())) as u64
        };
        // Decode: the field's bits, then two's complement in `width` bits.
        let unsigned = i128::from((pattern(&bytes) & mask) >> lo);
        let want = if signed && unsigned >= 1 << (width - 1) {
            unsigned - (1 << width)
        } else {
            unsigned
        };
        prop_assert_eq!(encoding.decode(&bytes), Ok(Raw::Int(want)));
        // Encode a value of the field's range over other bytes: the field
        // reads back, and every bit outside the mask is the other bytes'.
        let span = 1_i128 << width;
        let value = i128::from(pick) % span - if signed { span / 2 } else { 0 };
        let mut cell = other;
        encoding.encode_into(Raw::Int(value), &mut cell).unwrap();
        prop_assert_eq!(encoding.decode(&cell), Ok(Raw::Int(value)));
        prop_assert_eq!(pattern(&cell) & !mask, pattern(&other) & !mask);
        prop_assert_eq!(&cell[storage.cell_bytes()..], &other[storage.cell_bytes()..]);
        if storage == Storage::U20 {
            prop_assert_eq!(cell[0] & 0x0F, other[0] & 0x0F);
        }
    }

    #[test]
    fn every_in_range_value_round_trips(
        (encoding, value) in any_integer_encoding().prop_flat_map(|e| {
            let (min, max) = e.storage().int_range().unwrap();
            (Just(e), min..=max)
        }),
    ) {
        let bytes = encoding.encode(Raw::Int(value)).unwrap();
        prop_assert_eq!(bytes.len(), encoding.storage().cell_bytes());
        prop_assert_eq!(encoding.decode(&bytes), Ok(Raw::Int(value)));
    }
}
