//! Unit tests for the encoding types (US-RR-19-7).

use super::*;
use crate::diag::{Severity, Span};

/// `(storage, name, cell_bytes, value_bits, is_float, is_signed, int_range)`
/// for every storage type, written out by hand.
#[allow(clippy::type_complexity)]
const EXPECTED: [(Storage, &str, usize, u32, bool, bool, Option<(i128, i128)>); 13] = [
    (Storage::U8, "u8", 1, 8, false, false, Some((0, 0xFF))),
    (Storage::U16, "u16", 2, 16, false, false, Some((0, 0xFFFF))),
    (
        Storage::U20,
        "u20",
        3,
        20,
        false,
        true,
        Some((-0x8_0000, 0x7_FFFF)),
    ),
    (
        Storage::U24,
        "u24",
        3,
        24,
        false,
        false,
        Some((0, 0xFF_FFFF)),
    ),
    (
        Storage::U32,
        "u32",
        4,
        32,
        false,
        false,
        Some((0, 0xFFFF_FFFF)),
    ),
    (
        Storage::U64,
        "u64",
        8,
        64,
        false,
        false,
        Some((0, 0xFFFF_FFFF_FFFF_FFFF)),
    ),
    (Storage::I8, "i8", 1, 8, false, true, Some((-0x80, 0x7F))),
    (
        Storage::I16,
        "i16",
        2,
        16,
        false,
        true,
        Some((-0x8000, 0x7FFF)),
    ),
    (
        Storage::I24,
        "i24",
        3,
        24,
        false,
        true,
        Some((-0x80_0000, 0x7F_FFFF)),
    ),
    (
        Storage::I32,
        "i32",
        4,
        32,
        false,
        true,
        Some((-0x8000_0000, 0x7FFF_FFFF)),
    ),
    (
        Storage::I64,
        "i64",
        8,
        64,
        false,
        true,
        Some((-0x8000_0000_0000_0000, 0x7FFF_FFFF_FFFF_FFFF)),
    ),
    (Storage::F32, "f32", 4, 32, true, false, None),
    (Storage::F64, "f64", 8, 64, true, false, None),
];

#[test]
fn storage_helpers_match_the_hand_written_table() {
    for (storage, name, bytes, bits, float, signed, range) in EXPECTED {
        assert_eq!(storage.name(), name);
        assert_eq!(storage.to_string(), name);
        assert_eq!(storage.cell_bytes(), bytes, "{name}");
        assert_eq!(storage.value_bits(), bits, "{name}");
        assert_eq!(storage.is_float(), float, "{name}");
        assert_eq!(storage.is_integer(), !float, "{name}");
        assert_eq!(storage.is_signed(), signed, "{name}");
        assert_eq!(storage.int_range(), range, "{name}");
    }
}

#[test]
fn all_lists_every_storage_once_in_table_order() {
    let expected: Vec<Storage> = EXPECTED.iter().map(|e| e.0).collect();
    assert_eq!(Storage::ALL.to_vec(), expected);
    for (i, a) in Storage::ALL.iter().enumerate() {
        assert!(Storage::ALL.iter().skip(i + 1).all(|b| a != b), "{a} twice");
    }
}

#[test]
fn u20_signedness_is_the_single_decision_point() {
    // Decided by the maintainer (US-RR-19): sign-extend, as legacy and the
    // SH-2A do; consistent with rr_testkit::reference::U20_BIT19_SIGN_EXTENDS.
    const { assert!(U20_BIT19_SIGN_EXTENDS) };
    assert_eq!(Storage::U20.is_signed(), U20_BIT19_SIGN_EXTENDS);
    assert_eq!(Storage::U20.int_range(), Some((-0x8_0000, 0x7_FFFF)));
}

#[test]
fn u24_i24_and_64_bit_ranges_are_full_width() {
    // MF-02: legacy had no 3- or 8-byte case and read these as zero.
    assert_eq!(Storage::U24.int_range(), Some((0, (1 << 24) - 1)));
    assert_eq!(Storage::I24.int_range(), Some((-(1 << 23), (1 << 23) - 1)));
    assert_eq!(
        Storage::I64.int_range(),
        Some((i128::from(i64::MIN), i128::from(i64::MAX)))
    );
    assert_eq!(Storage::U64.int_range(), Some((0, i128::from(u64::MAX))));
}

#[test]
fn endian_and_layout_names() {
    assert_eq!(Endian::Big.name(), "big");
    assert_eq!(Endian::Little.name(), "little");
    assert_eq!(Endian::Big.to_string(), "big");
    assert_eq!(Endian::Little.to_string(), "little");
    assert_eq!(Layout::Default.name(), "default");
    assert_eq!(Layout::BoschSubtract.name(), "bosch-subtract");
    assert_eq!(Layout::Default.to_string(), "default");
    assert_eq!(Layout::BoschSubtract.to_string(), "bosch-subtract");
    assert_eq!(Layout::default(), Layout::Default);
}

fn bitfield_problem(result: Result<BitField, EncodingError>) -> BitFieldProblem {
    match result {
        Err(EncodingError::InvalidBitField { problem, .. }) => problem,
        other => panic!("expected an invalid bitfield, got {other:?}"),
    }
}

#[test]
fn bitfield_shift_defaults_to_the_mask_trailing_zeros() {
    let field = BitField::new(0x0FF0, None, false).unwrap();
    assert_eq!(field.mask(), 0x0FF0);
    assert_eq!(field.shift(), 4);
    assert!(!field.is_signed());
    assert_eq!(BitField::new(0x1, None, false).unwrap().shift(), 0);
    assert_eq!(BitField::new(1 << 63, None, false).unwrap().shift(), 63);
}

#[test]
fn bitfield_explicit_shift_up_to_the_lowest_bit_is_accepted() {
    assert_eq!(BitField::new(0x0FF0, Some(4), false).unwrap().shift(), 4);
    assert_eq!(BitField::new(0x0FF0, Some(0), false).unwrap().shift(), 0);
    assert_eq!(BitField::new(0x0FF0, Some(3), false).unwrap().shift(), 3);
}

#[test]
fn bitfield_shift_above_the_lowest_bit_is_refused() {
    let err = BitField::new(0x0FF0, Some(5), false).unwrap_err();
    assert_eq!(
        err,
        EncodingError::InvalidBitField {
            mask: 0x0FF0,
            shift: 5,
            problem: BitFieldProblem::ShiftDropsBits,
        }
    );
    assert_eq!(
        bitfield_problem(BitField::new(0x1, Some(1), true)),
        BitFieldProblem::ShiftDropsBits
    );
}

#[test]
fn bitfield_zero_mask_is_refused() {
    let err = BitField::new(0, None, false).unwrap_err();
    assert_eq!(
        err,
        EncodingError::InvalidBitField {
            mask: 0,
            shift: 64,
            problem: BitFieldProblem::ZeroMask,
        }
    );
    assert_eq!(
        bitfield_problem(BitField::new(0, Some(0), true)),
        BitFieldProblem::ZeroMask
    );
}

#[test]
fn signed_bitfield_needs_a_contiguous_mask_of_two_or_more_bits() {
    let signed = BitField::new(0x0FF0, None, true).unwrap();
    assert!(signed.is_signed());
    assert!(BitField::new(0b11, None, true).is_ok());
    assert!(BitField::new(0b0110, None, true).is_ok());
    assert!(BitField::new(u64::MAX, None, true).is_ok());
    assert!(BitField::new(0xF000_0000_0000_0000, None, true).is_ok());
    assert!(BitField::new(0x0000_0000_0000_000F, None, true).is_ok());

    for mask in [0b0101, 0b1001, 0x8000_0000_0000_0001, 0x0F0F] {
        assert_eq!(
            bitfield_problem(BitField::new(mask, None, true)),
            BitFieldProblem::SignedNotContiguous,
            "{mask:#X}"
        );
    }
    for mask in [0b1, 0b1000, 1 << 63] {
        assert_eq!(
            bitfield_problem(BitField::new(mask, None, true)),
            BitFieldProblem::SignedTooNarrow,
            "{mask:#X}"
        );
    }
}

#[test]
fn unsigned_bitfield_may_be_one_bit_or_non_contiguous() {
    // As the reference codec: raw = (stored & mask) >> shift.
    assert!(BitField::new(0b1, None, false).is_ok());
    assert!(BitField::new(0b0101, None, false).is_ok());
    assert!(BitField::new(0x8000_0000_0000_0001, None, false).is_ok());
}

#[test]
fn bitfield_reach_bits_is_one_past_the_highest_set_bit() {
    let reach = |mask| BitField::new(mask, None, false).unwrap().reach_bits();
    assert_eq!(reach(0x1), 1);
    assert_eq!(reach(0x80), 8);
    assert_eq!(reach(0xF0), 8);
    assert_eq!(reach(0x100), 9);
    assert_eq!(reach(0xF_FFFF), 20);
    assert_eq!(reach(u64::MAX), 64);
}

fn plain(storage: Storage, endian: Endian) -> Result<Encoding, EncodingError> {
    Encoding::new(storage, endian, None, Layout::Default)
}

#[test]
fn every_storage_is_valid_in_both_byte_orders_except_little_endian_u20() {
    for storage in Storage::ALL {
        let big = plain(storage, Endian::Big).unwrap();
        assert_eq!(big.storage(), storage);
        assert_eq!(big.endian(), Endian::Big);
        assert_eq!(big.bitfield(), None);
        assert_eq!(big.layout(), Layout::Default);
        let little = plain(storage, Endian::Little);
        if storage == Storage::U20 {
            assert_eq!(
                little.unwrap_err(),
                EncodingError::UnsupportedCombination {
                    storage: Storage::U20,
                    problem: CombinationProblem::U20LittleEndian,
                }
            );
        } else {
            assert_eq!(little.unwrap().endian(), Endian::Little);
        }
    }
}

#[test]
fn bosch_subtract_is_whole_byte_integers_only() {
    for storage in Storage::ALL {
        for endian in [Endian::Big, Endian::Little] {
            if storage == Storage::U20 && endian == Endian::Little {
                continue;
            }
            let result = Encoding::new(storage, endian, None, Layout::BoschSubtract);
            if storage.is_float() || storage == Storage::U20 {
                assert_eq!(
                    result.unwrap_err(),
                    EncodingError::UnsupportedCombination {
                        storage,
                        problem: CombinationProblem::BoschSubtractStorage,
                    }
                );
            } else {
                assert_eq!(result.unwrap().layout(), Layout::BoschSubtract);
            }
        }
    }
}

#[test]
fn little_endian_u20_is_reported_before_its_layout() {
    let err = Encoding::new(Storage::U20, Endian::Little, None, Layout::BoschSubtract);
    assert_eq!(
        err.unwrap_err(),
        EncodingError::UnsupportedCombination {
            storage: Storage::U20,
            problem: CombinationProblem::U20LittleEndian,
        }
    );
}

#[test]
fn bitfield_must_fit_the_stored_value() {
    let field = |mask| Some(BitField::new(mask, None, false).unwrap());
    let with = |storage, mask| Encoding::new(storage, Endian::Big, field(mask), Layout::Default);

    let ok = with(Storage::U8, 0xFF).unwrap();
    assert_eq!(ok.bitfield(), field(0xFF));
    assert!(with(Storage::I8, 0x80).is_ok());
    assert!(with(Storage::U20, 0xF_FFFF).is_ok());
    assert!(with(Storage::I24, 0xFF_FFFF).is_ok());
    assert!(with(Storage::U64, u64::MAX).is_ok());
    assert!(with(Storage::I64, u64::MAX).is_ok());

    for (storage, mask, bits) in [
        (Storage::U8, 0x100, 8),
        (Storage::I16, 0x1_0000, 16),
        (Storage::U20, 0x10_0000, 20),
        (Storage::U24, 0x100_0000, 24),
        (Storage::U32, 0x1_0000_0000, 32),
    ] {
        assert_eq!(
            with(storage, mask).unwrap_err(),
            EncodingError::InvalidBitField {
                mask,
                shift: mask.trailing_zeros(),
                problem: BitFieldProblem::OutsideStorage { storage_bits: bits },
            },
            "{storage}"
        );
    }
}

#[test]
fn bitfield_on_float_storage_is_refused() {
    for storage in [Storage::F32, Storage::F64] {
        let field = BitField::new(0x0F, None, false).unwrap();
        assert_eq!(
            Encoding::new(storage, Endian::Big, Some(field), Layout::Default).unwrap_err(),
            EncodingError::InvalidBitField {
                mask: 0x0F,
                shift: 0,
                problem: BitFieldProblem::FloatStorage,
            }
        );
    }
}

#[test]
fn bosch_subtract_with_a_bitfield_is_allowed() {
    let field = BitField::new(0x0FF0, None, false).unwrap();
    let enc = Encoding::new(
        Storage::U16,
        Endian::Little,
        Some(field),
        Layout::BoschSubtract,
    )
    .unwrap();
    assert_eq!(enc.bitfield(), Some(field));
    assert_eq!(enc.layout(), Layout::BoschSubtract);
    assert_eq!(enc.endian(), Endian::Little);
    assert_eq!(enc.storage(), Storage::U16);
}

#[test]
fn encodings_are_plain_values_usable_across_threads() {
    // MF-04: no shared state; each encoding carries its own byte order.
    const fn assert_send_sync<T: Send + Sync + Copy + 'static>() {}
    assert_send_sync::<Encoding>();
    let big = plain(Storage::U32, Endian::Big).unwrap();
    let little = plain(Storage::U32, Endian::Little).unwrap();
    let (a, b) = std::thread::scope(|s| {
        let a = s.spawn(move || big.endian());
        let b = s.spawn(move || little.endian());
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_eq!((a, b), (Endian::Big, Endian::Little));
}

fn every_error() -> Vec<(EncodingError, Code, &'static str)> {
    vec![
        (
            EncodingError::UnsupportedStorage {
                text: "uint48".into(),
                problem: StorageProblem::Unknown,
            },
            codes::UNSUPPORTED_STORAGE_TYPE,
            "unsupported storage type \"uint48\": not a supported storage type",
        ),
        (
            EncodingError::UnsupportedStorage {
                text: String::new(),
                problem: StorageProblem::Empty,
            },
            codes::UNSUPPORTED_STORAGE_TYPE,
            "unsupported storage type \"\": the storage type is empty",
        ),
        (
            EncodingError::UnsupportedStorage {
                text: "movi20s".into(),
                problem: StorageProblem::Movi20s,
            },
            codes::UNSUPPORTED_STORAGE_TYPE,
            "unsupported storage type \"movi20s\": movi20s (shifted MOVI20 immediate) is not supported",
        ),
        (
            EncodingError::UnsupportedStorage {
                text: "2".into(),
                problem: StorageProblem::NumericCode,
            },
            codes::UNSUPPORTED_STORAGE_TYPE,
            "unsupported storage type \"2\": a numeric storage code is ambiguous; \
             name the type (uint16, int8, float, ...)",
        ),
        (
            EncodingError::UnsupportedCombination {
                storage: Storage::U20,
                problem: CombinationProblem::U20LittleEndian,
            },
            codes::UNSUPPORTED_STORAGE_TYPE,
            "unsupported encoding for u20: MOVI20 is a big-endian SH-2A instruction field; \
             little-endian is not defined",
        ),
        (
            EncodingError::UnsupportedCombination {
                storage: Storage::F32,
                problem: CombinationProblem::BoschSubtractStorage,
            },
            codes::UNSUPPORTED_STORAGE_TYPE,
            "unsupported encoding for f32: bosch-subtract is defined for 1/2/3/4/8-byte \
             integer storage only",
        ),
        (
            EncodingError::UnknownEndian {
                text: "middle".into(),
            },
            codes::UNKNOWN_ENDIAN,
            "unknown endian \"middle\": expected \"big\" or \"little\"",
        ),
        (
            EncodingError::UnknownLayout {
                text: "zigzag".into(),
            },
            codes::UNKNOWN_LAYOUT,
            "unknown table layout \"zigzag\": expected \"bosch_subtract\" or none",
        ),
        (
            EncodingError::InvalidBitField {
                mask: 0,
                shift: 64,
                problem: BitFieldProblem::ZeroMask,
            },
            codes::INVALID_BITFIELD,
            "invalid bitfield (mask 0x0, shift 64): the mask is zero",
        ),
        (
            EncodingError::InvalidBitField {
                mask: 0xFF0,
                shift: 5,
                problem: BitFieldProblem::ShiftDropsBits,
            },
            codes::INVALID_BITFIELD,
            "invalid bitfield (mask 0xFF0, shift 5): the shift is above the mask's lowest bit \
             and would drop field bits",
        ),
        (
            EncodingError::InvalidBitField {
                mask: 0x5,
                shift: 0,
                problem: BitFieldProblem::SignedNotContiguous,
            },
            codes::INVALID_BITFIELD,
            "invalid bitfield (mask 0x5, shift 0): a signed field needs a contiguous mask",
        ),
        (
            EncodingError::InvalidBitField {
                mask: 0x8,
                shift: 3,
                problem: BitFieldProblem::SignedTooNarrow,
            },
            codes::INVALID_BITFIELD,
            "invalid bitfield (mask 0x8, shift 3): a signed field needs at least 2 bits",
        ),
        (
            EncodingError::InvalidBitField {
                mask: 0xF,
                shift: 0,
                problem: BitFieldProblem::FloatStorage,
            },
            codes::INVALID_BITFIELD,
            "invalid bitfield (mask 0xF, shift 0): bitfields apply to integer storage only",
        ),
        (
            EncodingError::InvalidBitField {
                mask: 0x100,
                shift: 8,
                problem: BitFieldProblem::OutsideStorage { storage_bits: 8 },
            },
            codes::INVALID_BITFIELD,
            "invalid bitfield (mask 0x100, shift 8): the mask has bits outside the 8-bit \
             stored value",
        ),
        (
            EncodingError::UnparsableMask {
                text: "0xFF".into(),
            },
            codes::INVALID_BITFIELD,
            "invalid bitfield mask \"0xFF\": expected 1 to 16 hexadecimal digits",
        ),
        (
            EncodingError::ShortInput { needed: 3, len: 1 },
            codes::CELL_OUT_OF_BOUNDS,
            "a 3-byte cell does not fit in 1 byte(s)",
        ),
        (
            EncodingError::OutOfRange {
                value: 256,
                storage: Storage::U8,
                min: 0,
                max: 255,
            },
            codes::RAW_OUT_OF_RANGE,
            "value 256 does not fit u8 (range 0..=255)",
        ),
        (
            EncodingError::BitFieldOutOfRange {
                value: -1,
                mask: 0xFF0,
                shift: 4,
                signed: false,
            },
            codes::RAW_OUT_OF_RANGE,
            "value -1 does not fit the unsigned bitfield (mask 0xFF0, shift 4)",
        ),
        (
            EncodingError::BitFieldOutOfRange {
                value: 128,
                mask: 0xFF,
                shift: 0,
                signed: true,
            },
            codes::RAW_OUT_OF_RANGE,
            "value 128 does not fit the signed bitfield (mask 0xFF, shift 0)",
        ),
        (
            EncodingError::KindMismatch {
                storage: Storage::F32,
                kind: RawKind::Int,
            },
            codes::RAW_KIND_MISMATCH,
            "f32 storage cannot hold a raw integer value",
        ),
        (
            EncodingError::WholeTableLayout {
                layout: Layout::BoschSubtract,
            },
            codes::WHOLE_TABLE_LAYOUT,
            "a bosch-subtract cell depends on the rest of its table; decode or encode the \
             whole table",
        ),
        (
            EncodingError::SpecialFloat {
                operand: EditOperand::Stored,
                kind: RawKind::F32,
                bits: 0x7FC0_1234,
                class: FloatClass::Nan { quiet: true },
            },
            codes::SPECIAL_FLOAT_EDIT,
            "refusing the stored f32 value 0x7FC01234 (quiet NaN): NaN, infinite and \
             subnormal floats are edited only when explicitly allowed",
        ),
        (
            EncodingError::SpecialFloat {
                operand: EditOperand::New,
                kind: RawKind::F32,
                bits: 0x1,
                class: FloatClass::Subnormal,
            },
            codes::SPECIAL_FLOAT_EDIT,
            "refusing the new f32 value 0x00000001 (subnormal): NaN, infinite and \
             subnormal floats are edited only when explicitly allowed",
        ),
        (
            EncodingError::SpecialFloat {
                operand: EditOperand::New,
                kind: RawKind::F64,
                bits: 0xFFF0_0000_0000_0000,
                class: FloatClass::Infinite,
            },
            codes::SPECIAL_FLOAT_EDIT,
            "refusing the new f64 value 0xFFF0000000000000 (infinite): NaN, infinite and \
             subnormal floats are edited only when explicitly allowed",
        ),
        (
            EncodingError::SpecialFloat {
                operand: EditOperand::Stored,
                kind: RawKind::F64,
                bits: 0x7FF0_0000_0000_0001,
                class: FloatClass::Nan { quiet: false },
            },
            codes::SPECIAL_FLOAT_EDIT,
            "refusing the stored f64 value 0x7FF0000000000001 (signalling NaN): NaN, infinite \
             and subnormal floats are edited only when explicitly allowed",
        ),
    ]
}

#[test]
fn every_error_has_its_code_and_message() {
    for (error, code, message) in every_error() {
        assert_eq!(error.code(), code, "{error:?}");
        assert_eq!(error.to_string(), message, "{error:?}");
    }
}

#[test]
fn every_error_code_is_a_registered_error_code() {
    for (error, ..) in every_error() {
        let info = codes::lookup(error.code()).expect("registered");
        assert_eq!(info.severity, Severity::Error, "{}", info.code);
    }
}

#[test]
fn to_diagnostic_is_an_error_with_code_message_and_provenance() {
    let provenance = Provenance::new("defs/base.xml", Span::line_col(12, 5)).with_layer("base");
    let error = EncodingError::UnknownEndian { text: "1".into() };
    let diag = error.to_diagnostic(provenance.clone());
    assert_eq!(diag.severity(), Severity::Error);
    assert_eq!(diag.code(), codes::UNKNOWN_ENDIAN);
    assert_eq!(diag.message(), error.to_string());
    assert_eq!(diag.provenance(), &provenance);
}

#[test]
fn encoding_error_is_a_std_error() {
    let error: Box<dyn core::error::Error> =
        Box::new(EncodingError::UnknownLayout { text: "x".into() });
    assert_eq!(
        error.to_string(),
        "unknown table layout \"x\": expected \"bosch_subtract\" or none"
    );
}
