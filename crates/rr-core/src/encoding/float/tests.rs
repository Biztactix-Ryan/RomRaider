//! Unit tests for float classes and the special-float edit gate
//! (US-RR-19-9, EC-07).
//!
//! Expected classes are written out by hand from the IEEE 754 field layout
//! (sign | exponent | fraction: 1 | 8 | 23 bits for binary32, 1 | 11 | 52 for
//! binary64), never computed with the code under test. The sweeps also check
//! against the standard library's `classify`, an independent implementation.
//! The cross-check against the synthetic ROM's manifest is in
//! `crates/rr-testkit/tests/encoding_float.rs`.

use core::num::FpCategory;

use proptest::prelude::*;

use super::super::{Encoding, EncodingError, Endian, Layout, Raw, RawKind, Storage};
use super::{EditOperand, FloatClass, SpecialFloats};

const QNAN: FloatClass = FloatClass::Nan { quiet: true };
const SNAN: FloatClass = FloatClass::Nan { quiet: false };

fn enc(storage: Storage, endian: Endian) -> Encoding {
    Encoding::new(storage, endian, None, Layout::Default).unwrap()
}

/// Every binary32 class boundary, by hand.
const F32_CASES: [(u32, FloatClass); 26] = [
    (0x0000_0000, FloatClass::Zero),
    (0x8000_0000, FloatClass::Zero),
    // Smallest and largest subnormal, both signs; one with only the top
    // fraction bit (the bit that marks a quiet NaN) set.
    (0x0000_0001, FloatClass::Subnormal),
    (0x8000_0001, FloatClass::Subnormal),
    (0x007F_FFFF, FloatClass::Subnormal),
    (0x807F_FFFF, FloatClass::Subnormal),
    (0x0040_0000, FloatClass::Subnormal),
    // Smallest normal (exponent 1), 1.0, -2.0, largest normal (exponent 0xFE).
    (0x0080_0000, FloatClass::Normal),
    (0x8080_0000, FloatClass::Normal),
    (0x3F80_0000, FloatClass::Normal),
    (0xC000_0000, FloatClass::Normal),
    (0x7F00_0000, FloatClass::Normal),
    (0x7F7F_FFFF, FloatClass::Normal),
    (0xFF7F_FFFF, FloatClass::Normal),
    (0x7F80_0000, FloatClass::Infinite),
    (0xFF80_0000, FloatClass::Infinite),
    // Quiet NaNs: top fraction bit (bit 22) set.
    (0x7FC0_0000, QNAN),
    (0x7FC0_1234, QNAN),
    (0xFFC0_0001, QNAN),
    (0x7FFF_FFFF, QNAN),
    (0xFFFF_FFFF, QNAN),
    // Signalling NaNs: bit 22 clear, fraction non-zero.
    (0x7F80_0001, SNAN),
    (0x7FA0_5678, SNAN),
    (0xFFBF_FFFF, SNAN),
    (0x7FBF_FFFF, SNAN),
    (0xFF80_0001, SNAN),
];

/// Every binary64 class boundary, by hand.
const F64_CASES: [(u64, FloatClass); 24] = [
    (0x0000_0000_0000_0000, FloatClass::Zero),
    (0x8000_0000_0000_0000, FloatClass::Zero),
    (0x0000_0000_0000_0001, FloatClass::Subnormal),
    (0x8000_0000_0000_0001, FloatClass::Subnormal),
    (0x000F_FFFF_FFFF_FFFF, FloatClass::Subnormal),
    (0x800F_FFFF_FFFF_FFFF, FloatClass::Subnormal),
    (0x0008_0000_0000_0000, FloatClass::Subnormal),
    (0x0010_0000_0000_0000, FloatClass::Normal),
    (0x3FF0_0000_0000_0000, FloatClass::Normal),
    (0xC004_0000_0000_0000, FloatClass::Normal),
    (0x7FE0_0000_0000_0000, FloatClass::Normal),
    (0x7FEF_FFFF_FFFF_FFFF, FloatClass::Normal),
    (0xFFEF_FFFF_FFFF_FFFF, FloatClass::Normal),
    (0x7FF0_0000_0000_0000, FloatClass::Infinite),
    (0xFFF0_0000_0000_0000, FloatClass::Infinite),
    // Quiet NaNs: bit 51 set.
    (0x7FF8_0000_0000_0000, QNAN),
    (0x7FF8_0000_0000_1234, QNAN),
    (0xFFF8_0000_0000_0001, QNAN),
    (0x7FFF_FFFF_FFFF_FFFF, QNAN),
    // Signalling NaNs: bit 51 clear, fraction non-zero.
    (0x7FF0_0000_0000_0001, SNAN),
    (0x7FF4_0000_0000_5678, SNAN),
    (0xFFF7_FFFF_FFFF_FFFF, SNAN),
    (0x7FF7_FFFF_FFFF_FFFF, SNAN),
    (0xFFF0_0000_0000_0001, SNAN),
];

/// The class std reports, plus the quiet bit (`quiet_bit`) for a NaN.
fn std_class(category: FpCategory, quiet: bool) -> FloatClass {
    match category {
        FpCategory::Zero => FloatClass::Zero,
        FpCategory::Subnormal => FloatClass::Subnormal,
        FpCategory::Normal => FloatClass::Normal,
        FpCategory::Infinite => FloatClass::Infinite,
        FpCategory::Nan => FloatClass::Nan { quiet },
    }
}

/// Decodes `bits` as F32 in both byte orders; checks the bits and class come
/// back and that encoding the decoded value gives the same bytes.
fn check_f32_faithful(bits: u32) -> FloatClass {
    let class = FloatClass::of_f32_bits(bits);
    for (endian, bytes) in [
        (Endian::Big, bits.to_be_bytes()),
        (Endian::Little, bits.to_le_bytes()),
    ] {
        let encoding = enc(Storage::F32, endian);
        let raw = encoding.decode(&bytes).unwrap();
        assert_eq!(raw.kind(), RawKind::F32);
        assert_eq!(raw.float_bits(), Some(u64::from(bits)), "{bits:#010X}");
        assert_eq!(raw.float_class(), Some(class), "{bits:#010X}");
        assert_eq!(encoding.encode(raw).unwrap(), bytes, "{bits:#010X}");
    }
    class
}

/// As [`check_f32_faithful`], for F64.
fn check_f64_faithful(bits: u64) -> FloatClass {
    let class = FloatClass::of_f64_bits(bits);
    for (endian, bytes) in [
        (Endian::Big, bits.to_be_bytes()),
        (Endian::Little, bits.to_le_bytes()),
    ] {
        let encoding = enc(Storage::F64, endian);
        let raw = encoding.decode(&bytes).unwrap();
        assert_eq!(raw.kind(), RawKind::F64);
        assert_eq!(raw.float_bits(), Some(bits), "{bits:#018X}");
        assert_eq!(raw.float_class(), Some(class), "{bits:#018X}");
        assert_eq!(encoding.encode(raw).unwrap(), bytes, "{bits:#018X}");
    }
    class
}

// ---------------------------------------------------------------------------
// Classes
// ---------------------------------------------------------------------------

#[test]
fn f32_classes_at_every_boundary() {
    for (bits, class) in F32_CASES {
        assert_eq!(FloatClass::of_f32_bits(bits), class, "{bits:#010X}");
        assert_eq!(check_f32_faithful(bits), class, "{bits:#010X}");
    }
}

#[test]
fn f64_classes_at_every_boundary() {
    for (bits, class) in F64_CASES {
        assert_eq!(FloatClass::of_f64_bits(bits), class, "{bits:#018X}");
        assert_eq!(check_f64_faithful(bits), class, "{bits:#018X}");
    }
}

#[test]
fn strided_sweep_of_every_f32_exponent_is_faithful_and_agrees_with_std() {
    // A stride coprime to 2^32 visits every exponent and sign with varied
    // fractions; the extra ranges walk the edges of the all-zero and
    // all-ones exponents (zero/subnormal, inf/NaN, quiet/signalling) bit by
    // bit.
    let strided = (0..=u32::MAX).step_by(65_521);
    let edges = [
        0x0000_0000..=0x0000_0400,
        0x007F_FC00..=0x0080_0400,
        0x7F7F_FC00..=0x7F80_0400,
        0x7FBF_FC00..=0x7FC0_0400,
        0x7FFF_FC00..=0x8000_0400,
        0xFF7F_FC00..=0xFF80_0400,
        0xFFBF_FC00..=0xFFC0_0400,
        0xFFFF_FC00..=0xFFFF_FFFF,
    ];
    for bits in strided.chain(edges.into_iter().flatten()) {
        let class = check_f32_faithful(bits);
        let value = f32::from_bits(bits);
        let quiet = bits & 0x0040_0000 != 0;
        assert_eq!(class, std_class(value.classify(), quiet), "{bits:#010X}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    #[test]
    fn every_f32_pattern_is_faithful_and_agrees_with_std(bits in any::<u32>()) {
        let class = check_f32_faithful(bits);
        let quiet = bits & 0x0040_0000 != 0;
        prop_assert_eq!(class, std_class(f32::from_bits(bits).classify(), quiet));
    }

    #[test]
    fn every_f64_pattern_is_faithful_and_agrees_with_std(bits in any::<u64>()) {
        let class = check_f64_faithful(bits);
        let quiet = bits & 0x0008_0000_0000_0000 != 0;
        prop_assert_eq!(class, std_class(f64::from_bits(bits).classify(), quiet));
    }

    #[test]
    fn every_f64_special_pattern_is_faithful(
        sign in any::<bool>(),
        exponent in prop_oneof![Just(0_u64), Just(0x7FF_u64)],
        fraction in 0_u64..(1 << 52),
    ) {
        let bits = (u64::from(sign) << 63) | (exponent << 52) | fraction;
        let class = check_f64_faithful(bits);
        prop_assert!(!matches!(class, FloatClass::Normal));
    }
}

#[test]
fn a_signalling_nan_is_never_quieted() {
    // A signalling NaN copied around as a Raw value and written back keeps
    // its exact bits (no arithmetic, no f32 -> f64 widening on the way).
    let f32be = enc(Storage::F32, Endian::Big);
    let f64le = enc(Storage::F64, Endian::Little);
    let stored32 = [0x7F, 0xA0, 0x56, 0x78];
    let stored64 = 0x7FF4_0000_0000_5678_u64.to_le_bytes();
    let values = [
        f32be.decode(&stored32).unwrap(),
        f64le.decode(&stored64).unwrap(),
    ];
    let copies: Vec<Raw> = values.to_vec();
    assert_eq!(copies[0].float_class(), Some(SNAN));
    assert_eq!(copies[1].float_class(), Some(SNAN));
    assert_eq!(f32be.encode(copies[0]).unwrap(), stored32);
    assert_eq!(f64le.encode(copies[1]).unwrap(), stored64);
    let mut cell = [0_u8; 4];
    f32be.encode_into(copies[0], &mut cell).unwrap();
    assert_eq!(cell, stored32);
}

#[test]
fn finite_and_special_classes() {
    use FloatClass::*;
    // (class, is_finite, is_special, name)
    let table = [
        (Zero, true, false, "zero"),
        (Subnormal, true, true, "subnormal"),
        (Normal, true, false, "normal"),
        (Infinite, false, true, "infinite"),
        (QNAN, false, true, "quiet NaN"),
        (SNAN, false, true, "signalling NaN"),
    ];
    for (class, finite, special, name) in table {
        assert_eq!(class.is_finite(), finite, "{class:?}");
        assert_eq!(class.is_special(), special, "{class:?}");
        assert_eq!(class.name(), name);
        assert_eq!(class.to_string(), name);
    }
}

#[test]
fn raw_float_accessors() {
    let int = Raw::Int(-5);
    assert_eq!(int.float_bits(), None);
    assert_eq!(int.float_class(), None);
    assert!(int.is_finite_number());
    assert!(!int.is_special_float());

    // (value, bits, class, finite number, special)
    let cases = [
        (Raw::F32(1.0), 0x3F80_0000, FloatClass::Normal, true, false),
        (Raw::F32(-0.0), 0x8000_0000, FloatClass::Zero, true, false),
        (
            Raw::F32(f32::from_bits(1)),
            1,
            FloatClass::Subnormal,
            true,
            true,
        ),
        (
            Raw::F32(f32::NEG_INFINITY),
            0xFF80_0000,
            FloatClass::Infinite,
            false,
            true,
        ),
        (
            Raw::F32(f32::from_bits(0x7F80_0001)),
            0x7F80_0001,
            SNAN,
            false,
            true,
        ),
        (
            Raw::F64(-2.5),
            0xC004_0000_0000_0000,
            FloatClass::Normal,
            true,
            false,
        ),
        (Raw::F64(0.0), 0, FloatClass::Zero, true, false),
        (
            Raw::F64(f64::from_bits(0x000F_FFFF_FFFF_FFFF)),
            0x000F_FFFF_FFFF_FFFF,
            FloatClass::Subnormal,
            true,
            true,
        ),
        (
            Raw::F64(f64::INFINITY),
            0x7FF0_0000_0000_0000,
            FloatClass::Infinite,
            false,
            true,
        ),
        (
            Raw::F64(f64::from_bits(0xFFF8_0000_0000_0001)),
            0xFFF8_0000_0000_0001,
            QNAN,
            false,
            true,
        ),
    ];
    for (raw, bits, class, finite, special) in cases {
        assert_eq!(raw.float_bits(), Some(bits), "{raw:?}");
        assert_eq!(raw.float_class(), Some(class), "{raw:?}");
        assert_eq!(raw.is_finite_number(), finite, "{raw:?}");
        assert_eq!(raw.is_special_float(), special, "{raw:?}");
    }
}

// ---------------------------------------------------------------------------
// The edit gate
// ---------------------------------------------------------------------------

#[test]
fn special_floats_are_refused_by_default_and_named() {
    assert_eq!(SpecialFloats::default(), SpecialFloats::Refuse);
    assert_eq!(EditOperand::Stored.name(), "stored");
    assert_eq!(EditOperand::New.name(), "new");
    assert_eq!(EditOperand::Stored.to_string(), "stored");
    assert_eq!(EditOperand::New.to_string(), "new");
}

#[test]
fn check_edit_refuses_only_special_floats_unless_allowed() {
    let passing = [
        Raw::Int(0),
        Raw::Int(i128::MIN),
        Raw::F32(0.0),
        Raw::F32(-1.5),
        Raw::F32(f32::MAX),
        Raw::F64(-0.0),
        Raw::F64(f64::MIN_POSITIVE),
    ];
    for raw in passing {
        for operand in [EditOperand::Stored, EditOperand::New] {
            assert_eq!(raw.check_edit(SpecialFloats::Refuse, operand), Ok(()));
            assert_eq!(raw.check_edit(SpecialFloats::Allow, operand), Ok(()));
        }
    }
    let refused = [
        (
            Raw::F32(f32::from_bits(0x8000_0001)),
            RawKind::F32,
            0x8000_0001,
            FloatClass::Subnormal,
        ),
        (
            Raw::F32(f32::INFINITY),
            RawKind::F32,
            0x7F80_0000,
            FloatClass::Infinite,
        ),
        (
            Raw::F32(f32::from_bits(0x7FC0_1234)),
            RawKind::F32,
            0x7FC0_1234,
            QNAN,
        ),
        (
            Raw::F32(f32::from_bits(0xFFBF_FFFF)),
            RawKind::F32,
            0xFFBF_FFFF,
            SNAN,
        ),
        (
            Raw::F64(f64::from_bits(1)),
            RawKind::F64,
            1,
            FloatClass::Subnormal,
        ),
        (
            Raw::F64(f64::NEG_INFINITY),
            RawKind::F64,
            0xFFF0_0000_0000_0000,
            FloatClass::Infinite,
        ),
        (
            Raw::F64(f64::from_bits(0x7FF8_0000_0000_1234)),
            RawKind::F64,
            0x7FF8_0000_0000_1234,
            QNAN,
        ),
        (
            Raw::F64(f64::from_bits(0x7FF0_0000_0000_0001)),
            RawKind::F64,
            0x7FF0_0000_0000_0001,
            SNAN,
        ),
    ];
    for (raw, kind, bits, class) in refused {
        for operand in [EditOperand::Stored, EditOperand::New] {
            assert_eq!(
                raw.check_edit(SpecialFloats::Refuse, operand),
                Err(EncodingError::SpecialFloat {
                    operand,
                    kind,
                    bits,
                    class
                }),
                "{raw:?}"
            );
            assert_eq!(raw.check_edit(SpecialFloats::Allow, operand), Ok(()));
        }
    }
}

#[test]
fn edit_into_writes_finite_values_over_finite_cells() {
    let f32be = enc(Storage::F32, Endian::Big);
    // 1.0 -> -2.0 (0xC000_0000); bytes after the cell untouched.
    let mut bytes = [0x3F, 0x80, 0, 0, 0xAA];
    f32be
        .edit_into(Raw::F32(-2.0), &mut bytes, SpecialFloats::Refuse)
        .unwrap();
    assert_eq!(bytes, [0xC0, 0, 0, 0, 0xAA]);
    // Zero is finite and not special.
    f32be
        .edit_into(Raw::F32(-0.0), &mut bytes, SpecialFloats::Refuse)
        .unwrap();
    assert_eq!(bytes, [0x80, 0, 0, 0, 0xAA]);
    // Integer storage is never gated.
    let u16le = enc(Storage::U16, Endian::Little);
    let mut cell = [0xFF, 0xFF];
    u16le
        .edit_into(Raw::Int(0x1234), &mut cell, SpecialFloats::Refuse)
        .unwrap();
    assert_eq!(cell, [0x34, 0x12]);
}

#[test]
fn edit_into_refuses_a_special_stored_value_and_writes_nothing() {
    let f64be = enc(Storage::F64, Endian::Big);
    for stored in [
        0x7FF8_0000_0000_1234_u64,
        0x7FF0_0000_0000_0001,
        0xFFF0_0000_0000_0000,
        0x0000_0000_0000_0001,
    ] {
        let original = stored.to_be_bytes();
        let mut bytes = original;
        // Even a special new value reports the stored one first.
        for new in [Raw::F64(1.0), Raw::F64(f64::NAN)] {
            assert_eq!(
                f64be.edit_into(new, &mut bytes, SpecialFloats::Refuse),
                Err(EncodingError::SpecialFloat {
                    operand: EditOperand::Stored,
                    kind: RawKind::F64,
                    bits: stored,
                    class: FloatClass::of_f64_bits(stored),
                })
            );
            assert_eq!(bytes, original);
        }
        // Explicitly allowed: the edit goes through.
        f64be
            .edit_into(Raw::F64(1.0), &mut bytes, SpecialFloats::Allow)
            .unwrap();
        assert_eq!(bytes, 0x3FF0_0000_0000_0000_u64.to_be_bytes());
    }
}

#[test]
fn edit_into_refuses_a_special_new_value_and_writes_nothing() {
    let f32le = enc(Storage::F32, Endian::Little);
    for new in [0x7F80_0001_u32, 0xFFC0_0000, 0x7F80_0000, 0x807F_FFFF] {
        let mut bytes = [0, 0, 0x80, 0x3F];
        let raw = Raw::F32(f32::from_bits(new));
        assert_eq!(
            f32le.edit_into(raw, &mut bytes, SpecialFloats::Refuse),
            Err(EncodingError::SpecialFloat {
                operand: EditOperand::New,
                kind: RawKind::F32,
                bits: u64::from(new),
                class: FloatClass::of_f32_bits(new),
            })
        );
        assert_eq!(bytes, [0, 0, 0x80, 0x3F]);
        // Explicitly allowed: written bit-exactly, payload and all.
        f32le
            .edit_into(raw, &mut bytes, SpecialFloats::Allow)
            .unwrap();
        assert_eq!(bytes, new.to_le_bytes());
    }
}

#[test]
fn edit_into_passes_codec_errors_through_and_writes_nothing() {
    let f32be = enc(Storage::F32, Endian::Big);
    let mut short = [0x3F, 0x80];
    assert_eq!(
        f32be.edit_into(Raw::F32(1.0), &mut short, SpecialFloats::Allow),
        Err(EncodingError::ShortInput { needed: 4, len: 2 })
    );
    let mut cell = [0x3F, 0x80, 0, 0];
    assert_eq!(
        f32be.edit_into(Raw::F64(1.0), &mut cell, SpecialFloats::Allow),
        Err(EncodingError::KindMismatch {
            storage: Storage::F32,
            kind: RawKind::F64,
        })
    );
    assert_eq!(cell, [0x3F, 0x80, 0, 0]);
    let bosch = Encoding::new(Storage::U8, Endian::Big, None, Layout::BoschSubtract).unwrap();
    let mut table = [1];
    assert_eq!(
        bosch.edit_into(Raw::Int(2), &mut table, SpecialFloats::Allow),
        Err(EncodingError::WholeTableLayout {
            layout: Layout::BoschSubtract
        })
    );
    assert_eq!(table, [1]);
}
