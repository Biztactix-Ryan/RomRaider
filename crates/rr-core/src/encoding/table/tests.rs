//! Unit tests for the table codec (US-RR-19-10).
//!
//! Expected values are worked out by hand from legacy's definition of the
//! Bosch-subtract layout (`legacy/src/main/java/com/romraider/maps/
//! DataCell.java:193-207` decode, `:219-231` and `:312-313` encode), with
//! the arithmetic in the comments, never computed with the code under test.
//! The cross-check against the independent reference codec and the synthetic
//! ROM is in `crates/rr-testkit/tests/encoding_table.rs`.

use proptest::prelude::*;

use super::super::{BitField, Encoding, EncodingError, Endian, Layout, Raw, RawKind, Storage};
use crate::diag::codes;

const BE: Endian = Endian::Big;
const LE: Endian = Endian::Little;

fn plain(storage: Storage, endian: Endian) -> Encoding {
    Encoding::new(storage, endian, None, Layout::Default).unwrap()
}

fn bosch(storage: Storage, endian: Endian) -> Encoding {
    Encoding::new(storage, endian, None, Layout::BoschSubtract).unwrap()
}

fn with_field(
    storage: Storage,
    endian: Endian,
    mask: u64,
    signed: bool,
    layout: Layout,
) -> Encoding {
    let field = BitField::new(mask, None, signed).unwrap();
    Encoding::new(storage, endian, Some(field), layout).unwrap()
}

fn ints(values: &[i128]) -> Vec<Raw> {
    values.iter().copied().map(Raw::Int).collect()
}

fn cell_error(index: usize, source: EncodingError) -> EncodingError {
    EncodingError::TableCell {
        index,
        source: Box::new(source),
    }
}

fn u8_out_of_range(value: i128) -> EncodingError {
    EncodingError::OutOfRange {
        value,
        storage: Storage::U8,
        min: 0,
        max: 0xFF,
    }
}

// ---------------------------------------------------------------------------
// Default layout: each value is its cell
// ---------------------------------------------------------------------------

#[test]
fn a_default_table_is_its_cells_in_order() {
    let u16be = plain(Storage::U16, BE);
    // Cells 0x1234 and 0xABCD; the fifth byte is not part of the table.
    let bytes = [0x12, 0x34, 0xAB, 0xCD, 0xEE];
    assert_eq!(u16be.decode_table(&bytes, 2), Ok(ints(&[0x1234, 0xABCD])));
    assert_eq!(u16be.decode_table(&bytes, 1), Ok(ints(&[0x1234])));
    let mut out = [0x55; 5];
    u16be.encode_table(&ints(&[1, 0xFFFF]), &mut out).unwrap();
    assert_eq!(out, [0x00, 0x01, 0xFF, 0xFF, 0x55]);

    // i8: 0x80 = -128, 0x7F = 127, 0xFF = -1.
    let i8le = plain(Storage::I8, LE);
    assert_eq!(
        i8le.decode_table(&[0x80, 0x7F, 0xFF], 3),
        Ok(ints(&[-128, 127, -1]))
    );
    // i24 little-endian: FE FF FF = -2, 01 00 00 = 1.
    let i24le = plain(Storage::I24, LE);
    let mut out = [0; 6];
    i24le.encode_table(&ints(&[-2, 1]), &mut out).unwrap();
    assert_eq!(out, [0xFE, 0xFF, 0xFF, 0x01, 0x00, 0x00]);
    assert_eq!(i24le.decode_table(&out, 2), Ok(ints(&[-2, 1])));

    // Floats: 1.0f32 = 3F800000, -2.0f32 = C0000000.
    let f32be = plain(Storage::F32, BE);
    let bytes = [0x3F, 0x80, 0, 0, 0xC0, 0, 0, 0];
    assert_eq!(
        f32be.decode_table(&bytes, 2),
        Ok(vec![Raw::F32(1.0), Raw::F32(-2.0)])
    );
    let mut out = [0; 8];
    f32be
        .encode_table(&[Raw::F32(1.0), Raw::F32(-2.0)], &mut out)
        .unwrap();
    assert_eq!(out, bytes);
}

#[test]
fn a_default_bitfield_table_keeps_the_bits_outside_the_mask() {
    // mask 0x0FF0 on u16 big-endian: 0xABCD -> 0xBC, 0x1234 -> 0x23.
    let mid = with_field(Storage::U16, BE, 0x0FF0, false, Layout::Default);
    let mut bytes = [0xAB, 0xCD, 0x12, 0x34];
    assert_eq!(mid.decode_table(&bytes, 2), Ok(ints(&[0xBC, 0x23])));
    // 0x01 -> 0xA01D, 0xFE -> 0x1FE4: the outer nibbles stay.
    mid.encode_table(&ints(&[0x01, 0xFE]), &mut bytes).unwrap();
    assert_eq!(bytes, [0xA0, 0x1D, 0x1F, 0xE4]);
}

#[test]
fn an_empty_table_reads_and_writes_nothing() {
    for encoding in [plain(Storage::U32, LE), bosch(Storage::U8, BE)] {
        assert_eq!(encoding.decode_table(&[], 0), Ok(vec![]));
        assert_eq!(encoding.decode_table(&[0xAA], 0), Ok(vec![]));
        let mut bytes = [0xAA];
        assert_eq!(encoding.encode_table(&[], &mut bytes), Ok(()));
        assert_eq!(bytes, [0xAA]);
    }
}

#[test]
fn a_table_that_does_not_fit_is_an_error_and_writes_nothing() {
    for layout in [Layout::Default, Layout::BoschSubtract] {
        let u16be = Encoding::new(Storage::U16, BE, None, layout).unwrap();
        let short = EncodingError::ShortInput { needed: 4, len: 3 };
        assert_eq!(u16be.decode_table(&[0; 3], 2), Err(short.clone()));
        // Exactly the table's bytes is enough.
        assert!(u16be.decode_table(&[0; 4], 2).is_ok());
        let mut bytes = [0x5A; 3];
        // Values whose cells fit: Bosch [1, 2^16] stores s = 0xFFFF, 0.
        let values = match layout {
            Layout::Default => ints(&[0xFFFF, 1]),
            Layout::BoschSubtract => ints(&[1, 0x1_0000]),
        };
        assert_eq!(u16be.encode_table(&values, &mut bytes), Err(short));
        assert_eq!(bytes, [0x5A; 3]);
        let mut bytes = [0x5A; 4];
        assert_eq!(u16be.encode_table(&values, &mut bytes), Ok(()));
        // A cell count whose byte length overflows saturates.
        assert_eq!(
            u16be.decode_table(&[0; 8], usize::MAX),
            Err(EncodingError::ShortInput {
                needed: usize::MAX,
                len: 8
            })
        );
    }
}

#[test]
fn a_default_table_with_a_bad_cell_is_an_error_and_writes_nothing() {
    let u8be = plain(Storage::U8, BE);
    let mut bytes = [0xAA; 3];
    assert_eq!(
        u8be.encode_table(&ints(&[1, 256, 3]), &mut bytes),
        Err(cell_error(1, u8_out_of_range(256)))
    );
    // Cell 0 fits, but nothing is written.
    assert_eq!(bytes, [0xAA; 3]);
    assert_eq!(
        u8be.encode_table(&[Raw::Int(1), Raw::Int(2), Raw::F32(1.0)], &mut bytes),
        Err(cell_error(
            2,
            EncodingError::KindMismatch {
                storage: Storage::U8,
                kind: RawKind::F32
            }
        ))
    );
    assert_eq!(bytes, [0xAA; 3]);
    // The first bad cell is the one reported.
    assert_eq!(
        u8be.encode_table(&ints(&[-1, 256, 3]), &mut bytes),
        Err(cell_error(0, u8_out_of_range(-1)))
    );
}

// ---------------------------------------------------------------------------
// Bosch subtract: decode
// ---------------------------------------------------------------------------

/// `(encoding, stored bytes, table values)`, by hand:
/// `value[i] = 2^(8W) - (s[i] + ... + s[N-1])`.
fn bosch_answers() -> Vec<(Encoding, Vec<u8>, Vec<i128>)> {
    use Storage::*;
    vec![
        // s = 1, 2, 3; 256 - 6 = 250, 256 - 5 = 251, 256 - 3 = 253.
        (bosch(U8, BE), vec![1, 2, 3], vec![250, 251, 253]),
        // The synthetic ROM's bosch-u8-be-explicit (10, 20, 156): s2 = 256 -
        // 156 = 100, s1 = 156 - 20 = 136, s0 = 20 - 10 = 10.
        (bosch(U8, LE), vec![10, 136, 100], vec![10, 20, 156]),
        // An all-zero cell is 2^(8W) itself; an all-ones cell is 1.
        (bosch(U8, BE), vec![0], vec![256]),
        (bosch(U8, BE), vec![0xFF], vec![1]),
        // s = 16, 256, 65535; 65536 - 65535 = 1, 1 - 256 = -255,
        // -255 - 16 = -271.
        (
            bosch(U16, BE),
            vec![0x00, 0x10, 0x01, 0x00, 0xFF, 0xFF],
            vec![-271, -255, 1],
        ),
        (
            bosch(U16, LE),
            vec![0x10, 0x00, 0x00, 0x01, 0xFF, 0xFF],
            vec![-271, -255, 1],
        ),
        // Signed cells subtract their signed value (legacy reads them with
        // `isSignedData`): s = -1, 127, -128; 256 + 128 = 384,
        // 384 - 127 = 257, 257 + 1 = 258.
        (bosch(I8, BE), vec![0xFF, 0x7F, 0x80], vec![258, 257, 384]),
        // u24: 2^24 - (2^24 - 1) = 1, and 2^24 - 0.
        (
            bosch(U24, BE),
            vec![0xFF, 0xFF, 0xFF, 0, 0, 0],
            vec![1, 0x100_0000],
        ),
        // i24 little-endian 00 00 80 = -2^23: 2^24 + 2^23 = 25165824.
        (bosch(I24, LE), vec![0x00, 0x00, 0x80], vec![25_165_824]),
        // u32 little-endian s = 1, 2^31: 2^32 - 2^31 = 2147483648, then - 1.
        (
            bosch(U32, LE),
            vec![0x01, 0, 0, 0, 0, 0, 0, 0x80],
            vec![2_147_483_647, 2_147_483_648],
        ),
        // i32 big-endian 80 00 00 00 = -2^31: 2^32 + 2^31 = 6442450944.
        (bosch(I32, BE), vec![0x80, 0, 0, 0], vec![6_442_450_944]),
        // u64: s = 2^64 - 1, 0; 2^64 - 0 = 2^64, 2^64 - (2^64 - 1) = 1.
        (
            bosch(U64, BE),
            [[0xFF; 8], [0; 8]].concat(),
            vec![1, 1 << 64],
        ),
        // i64 little-endian s = -1, -2^63: 2^64 + 2^63, then + 1.
        (
            bosch(I64, LE),
            [[0xFF; 8], [0, 0, 0, 0, 0, 0, 0, 0x80]].concat(),
            vec![(1 << 64) + (1 << 63) + 1, (1 << 64) + (1 << 63)],
        ),
    ]
}

#[test]
fn bosch_subtract_tables_decode_to_their_hand_computed_values() {
    for (encoding, bytes, values) in bosch_answers() {
        let cells = bytes.len() / encoding.storage().cell_bytes();
        assert_eq!(
            encoding.decode_table(&bytes, cells),
            Ok(ints(&values)),
            "{} {} {bytes:02X?}",
            encoding.storage(),
            encoding.endian()
        );
    }
}

#[test]
fn bosch_subtract_tables_encode_to_their_hand_computed_bytes() {
    for (encoding, bytes, values) in bosch_answers() {
        // Start from the complement, so only a real write restores it.
        let mut out: Vec<u8> = bytes.iter().map(|b| !b).collect();
        out.push(0x77);
        encoding.encode_table(&ints(&values), &mut out).unwrap();
        assert_eq!(out.pop(), Some(0x77), "the byte after the table is kept");
        assert_eq!(
            out,
            bytes,
            "{} {} {values:?}",
            encoding.storage(),
            encoding.endian()
        );
    }
}

#[test]
fn a_bosch_table_covers_only_its_own_cells() {
    // A shorter table is a different table: [1, 2] alone is 256 - 3 = 253,
    // 256 - 2 = 254; the third byte is not part of it.
    let u8be = bosch(Storage::U8, BE);
    assert_eq!(u8be.decode_table(&[1, 2, 3], 2), Ok(ints(&[253, 254])));
}

#[test]
fn a_bosch_bitfield_table_sums_the_fields_and_keeps_the_other_bits() {
    // mask 0x0FF0 on u16 big-endian: fields 0x12 = 18 and 0xFF = 255. The
    // total is 2^(8W) = 65536 for the 2-byte cell, whatever the mask
    // (legacy `Math.pow(2, 8 * storageType)`): 65536 - 255 = 65281, then
    // 65281 - 18 = 65263.
    let mid = with_field(Storage::U16, BE, 0x0FF0, false, Layout::BoschSubtract);
    assert_eq!(
        mid.decode_table(&[0xA1, 0x2D, 0xFF, 0xFF], 2),
        Ok(ints(&[65_263, 65_281]))
    );
    // Encode: s1 = 65536 - 65281 = 255, s0 = 65281 - 65263 = 18, written
    // into 0xA00D and 0x5005 keeping the outer nibbles.
    let mut bytes = [0xA0, 0x0D, 0x50, 0x05];
    mid.encode_table(&ints(&[65_263, 65_281]), &mut bytes)
        .unwrap();
    assert_eq!(bytes, [0xA1, 0x2D, 0x5F, 0xF5]);

    // A signed 4-bit field in the high nibble of a u8: 0xF -> -1, 0x8 ->
    // -8; 256 + 8 = 264, 264 + 1 = 265.
    let high = with_field(Storage::U8, BE, 0xF0, true, Layout::BoschSubtract);
    assert_eq!(high.decode_table(&[0xF5, 0x83], 2), Ok(ints(&[265, 264])));
    // s1 = 256 - 264 = -8 -> 0x8, s0 = 264 - 265 = -1 -> 0xF; low nibbles
    // kept.
    let mut bytes = [0x05, 0x03];
    high.encode_table(&ints(&[265, 264]), &mut bytes).unwrap();
    assert_eq!(bytes, [0xF5, 0x83]);
}

// ---------------------------------------------------------------------------
// Bosch subtract: values whose stored cells do not fit
// ---------------------------------------------------------------------------

#[test]
fn a_bosch_value_whose_stored_cell_does_not_fit_is_an_error_and_writes_nothing() {
    let u8be = bosch(Storage::U8, BE);
    // [250, 251, 100]: s2 = 256 - 100 = 156 fits, s1 = 100 - 251 = -151 does
    // not. Legacy stores (int) -151 as 0x69 and goes on; this codec refuses.
    let mut bytes = [0xAA; 3];
    assert_eq!(
        u8be.encode_table(&ints(&[250, 251, 100]), &mut bytes),
        Err(cell_error(1, u8_out_of_range(-151)))
    );
    assert_eq!(bytes, [0xAA; 3]);
    // The first cell that does not fit is reported: s0 = 100 - 250 = -150.
    assert_eq!(
        u8be.encode_table(&ints(&[250, 100, 100]), &mut bytes),
        Err(cell_error(0, u8_out_of_range(-150)))
    );
    assert_eq!(bytes, [0xAA; 3]);
    // The last cell against 2^8: 1 and 256 are the extremes (s = 255, 0).
    for (value, stored) in [(1, 0xFF), (256, 0x00)] {
        let mut one = [0xAA];
        u8be.encode_table(&ints(&[value]), &mut one).unwrap();
        assert_eq!(one, [stored], "{value}");
    }
    for (value, stored) in [(0, 256), (257, -1)] {
        let mut one = [0xAA];
        assert_eq!(
            u8be.encode_table(&ints(&[value]), &mut one),
            Err(cell_error(0, u8_out_of_range(stored))),
            "{value}"
        );
        assert_eq!(one, [0xAA]);
    }
    // Signed cells: s in -128..=127, so the last value is in 129..=384.
    let i8be = bosch(Storage::I8, BE);
    let i8_range = |value| EncodingError::OutOfRange {
        value,
        storage: Storage::I8,
        min: -0x80,
        max: 0x7F,
    };
    for (value, stored) in [(129, 0x7F), (384, 0x80)] {
        let mut one = [0xAA];
        i8be.encode_table(&ints(&[value]), &mut one).unwrap();
        assert_eq!(one, [stored], "{value}");
    }
    for (value, stored) in [(128, 128), (385, -129)] {
        let mut one = [0xAA];
        assert_eq!(
            i8be.encode_table(&ints(&[value]), &mut one),
            Err(cell_error(0, i8_range(stored)))
        );
        assert_eq!(one, [0xAA]);
    }
}

#[test]
fn a_bosch_step_that_does_not_fit_the_bitfield_is_an_error() {
    let mid = with_field(Storage::U16, BE, 0x0FF0, false, Layout::BoschSubtract);
    let mut bytes = [0x12, 0x34];
    // s = 65536 - 65280 = 256: inside u16, outside the 8-bit field.
    assert_eq!(
        mid.encode_table(&ints(&[65_280]), &mut bytes),
        Err(cell_error(
            0,
            EncodingError::BitFieldOutOfRange {
                value: 256,
                mask: 0x0FF0,
                shift: 4,
                signed: false
            }
        ))
    );
    assert_eq!(bytes, [0x12, 0x34]);
}

#[test]
fn a_bosch_step_beyond_i128_saturates_and_is_refused() {
    let u8be = bosch(Storage::U8, BE);
    let mut bytes = [0xAA; 2];
    // i128::MAX - i128::MIN saturates at i128::MAX (no wrap to -1).
    assert_eq!(
        u8be.encode_table(&ints(&[i128::MIN, i128::MAX]), &mut bytes),
        Err(cell_error(0, u8_out_of_range(i128::MAX)))
    );
    // i128::MIN - i128::MAX saturates at i128::MIN (no wrap to 1).
    assert_eq!(
        u8be.encode_table(&ints(&[i128::MAX, i128::MIN]), &mut bytes),
        Err(cell_error(0, u8_out_of_range(i128::MIN)))
    );
    assert_eq!(bytes, [0xAA; 2]);
}

#[test]
fn a_float_in_a_bosch_table_is_a_kind_mismatch_reported_before_any_step() {
    let u8be = bosch(Storage::U8, BE);
    let mut bytes = [0xAA; 3];
    // Cell 0's step (-5 - 0 = -5) does not fit either, but the wrong kind
    // at cell 2 is reported first.
    assert_eq!(
        u8be.encode_table(&[Raw::Int(0), Raw::Int(-5), Raw::F64(1.0)], &mut bytes),
        Err(cell_error(
            2,
            EncodingError::KindMismatch {
                storage: Storage::U8,
                kind: RawKind::F64
            }
        ))
    );
    assert_eq!(
        u8be.encode_table(&[Raw::F32(1.0)], &mut bytes),
        Err(cell_error(
            0,
            EncodingError::KindMismatch {
                storage: Storage::U8,
                kind: RawKind::F32
            }
        ))
    );
    assert_eq!(bytes, [0xAA; 3]);
}

#[test]
fn a_single_bosch_cell_is_still_refused() {
    let u8be = bosch(Storage::U8, BE);
    let error = EncodingError::WholeTableLayout {
        layout: Layout::BoschSubtract,
    };
    assert_eq!(u8be.decode(&[1]), Err(error.clone()));
    assert_eq!(u8be.encode(Raw::Int(255)), Err(error));
}

#[test]
fn a_table_cell_error_has_its_cells_code_and_names_the_cell() {
    let out_of_range = cell_error(1, u8_out_of_range(-151));
    assert_eq!(out_of_range.code(), codes::RAW_OUT_OF_RANGE);
    assert_eq!(
        out_of_range.to_string(),
        "table cell 1: value -151 does not fit u8 (range 0..=255)"
    );
    let mismatch = cell_error(
        0,
        EncodingError::KindMismatch {
            storage: Storage::U8,
            kind: RawKind::F32,
        },
    );
    assert_eq!(mismatch.code(), codes::RAW_KIND_MISMATCH);
    assert!(mismatch.to_string().starts_with("table cell 0: "));
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

/// The whole-byte integer storages (every Bosch-subtract storage).
const WHOLE_INTEGERS: [Storage; 10] = [
    Storage::U8,
    Storage::U16,
    Storage::U24,
    Storage::U32,
    Storage::U64,
    Storage::I8,
    Storage::I16,
    Storage::I24,
    Storage::I32,
    Storage::I64,
];

fn any_bosch() -> impl Strategy<Value = Encoding> {
    (
        proptest::sample::select(WHOLE_INTEGERS.to_vec()),
        any::<bool>(),
    )
        .prop_map(|(storage, big)| bosch(storage, if big { BE } else { LE }))
}

/// The same storage and byte order in the default layout.
fn cells_of(encoding: Encoding) -> Encoding {
    plain(encoding.storage(), encoding.endian())
}

/// The Bosch values of stored cells `s`, the long way: for each `i`, the
/// total minus each later cell in turn.
fn bosch_by_hand(encoding: Encoding, stored: &[i128]) -> Vec<i128> {
    let total = 1_i128 << (8 * encoding.storage().cell_bytes());
    (0..stored.len())
        .map(|i| stored[i..].iter().fold(total, |acc, s| acc - s))
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn a_default_table_decodes_and_encodes_cell_by_cell(
        (storage, big, bytes) in proptest::sample::select(WHOLE_INTEGERS.to_vec())
            .prop_flat_map(|s| (Just(s), any::<bool>(),
                proptest::collection::vec(any::<u8>(), 0..=4 * s.cell_bytes()))),
    ) {
        let encoding = plain(storage, if big { BE } else { LE });
        let width = storage.cell_bytes();
        let cells = bytes.len() / width;
        let values = encoding.decode_table(&bytes, cells).unwrap();
        let one_by_one: Vec<Raw> = bytes
            .chunks_exact(width)
            .map(|cell| encoding.decode(cell).unwrap())
            .collect();
        prop_assert_eq!(&values, &one_by_one);
        let mut out: Vec<u8> = bytes.iter().map(|b| !b).collect();
        encoding.encode_table(&values, &mut out).unwrap();
        prop_assert_eq!(&out[..cells * width], &bytes[..cells * width]);
    }

    #[test]
    fn a_bosch_table_decodes_the_long_way_and_encodes_back_to_its_bytes(
        (encoding, bytes) in any_bosch().prop_flat_map(|e| {
            let width = e.storage().cell_bytes();
            (Just(e), proptest::collection::vec(any::<u8>(), width..=6 * width))
        }),
    ) {
        let width = encoding.storage().cell_bytes();
        let cells = bytes.len() / width;
        let stored: Vec<i128> = bytes
            .chunks_exact(width)
            .map(|cell| match cells_of(encoding).decode(cell).unwrap() {
                Raw::Int(v) => v,
                other => panic!("{other:?}"),
            })
            .collect();
        let values = encoding.decode_table(&bytes, cells).unwrap();
        prop_assert_eq!(&values, &ints(&bosch_by_hand(encoding, &stored)));
        let mut out: Vec<u8> = bytes.iter().map(|b| !b).collect();
        encoding.encode_table(&values, &mut out).unwrap();
        prop_assert_eq!(&out[..cells * width], &bytes[..cells * width]);
        prop_assert_eq!(&out[cells * width..], &bytes.iter().map(|b| !b).collect::<Vec<_>>()[cells * width..]);
    }

    #[test]
    fn every_bosch_table_of_storable_cells_round_trips(
        (encoding, stored) in any_bosch().prop_flat_map(|e| {
            let (min, max) = e.storage().int_range().unwrap();
            let edge = proptest::sample::select(vec![min, min + 1, -1, 0, 1, max - 1, max]);
            let cell = prop_oneof![1 => edge, 2 => min..=max]
                .prop_filter("in range", move |v| (min..=max).contains(v));
            (Just(e), proptest::collection::vec(cell, 1..=6))
        }),
    ) {
        let values = ints(&bosch_by_hand(encoding, &stored));
        let mut bytes = vec![0; stored.len() * encoding.storage().cell_bytes()];
        encoding.encode_table(&values, &mut bytes).unwrap();
        prop_assert_eq!(encoding.decode_table(&bytes, stored.len()), Ok(values));
    }
}
