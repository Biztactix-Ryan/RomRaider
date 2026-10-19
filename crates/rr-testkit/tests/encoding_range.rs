//! rr-core's raw value ranges (`Encoding::raw_range`, US-RR-19-11, MF-15)
//! against the codec, for every encoding shape.
//!
//! For each shape of [`encoding_shapes`] (storage × endian × bitfield kind ×
//! layout), concrete encodings from [`encoding_in`] are checked:
//!
//! * **Default layout, integers.** The range's `min` and `max` encode and
//!   decode back; `min - 1` and `max + 1` are refused (`OutOfRange` for a
//!   whole storage, `BitFieldOutOfRange` for a bitfield). Every value of
//!   [`value_for`] is in the range, none of [`invalid_value_for`] is, and for
//!   arbitrary integers near the bounds `contains` holds exactly when the
//!   codec stores the value, in agreement with the reference codec. Every
//!   cell of [`bytes_for`] decodes to a value in the range.
//! * **Default layout, floats.** The range is `-MAX..=MAX` of the width
//!   (MF-15: never `0`, never the smallest positive float); `±MAX` encode,
//!   pass the EC-07 edit gate and decode back; the next values out, `±Inf`,
//!   are outside the range and refused by the edit gate. A decoded cell is in
//!   the range exactly when it is finite.
//! * **Bosch subtract.** `raw_range` is refused (`WholeTableLayout`); the
//!   stored-cell range is the default layout's, and a table whose derived
//!   cells sit on its bounds is stored while one with a cell just outside is
//!   refused for that cell.
//!
//! This file is a test boundary: it uses rr-core's types and the reference
//! codec only through `rr_testkit::encoding_oracle`.

use proptest::prelude::*;
use proptest::test_runner::{Config, TestCaseError, TestRunner};
use rr_core::encoding::{
    Encoding, EncodingError, Layout, Raw, RawKind, RawRange, SpecialFloats, Storage,
};
use rr_testkit::encoding_oracle::check_encode;
use rr_testkit::strategies::encoding::{
    EncodingShape, FieldKind, bytes_for, encoding_in, encoding_shapes, invalid_value_for, value_for,
};

/// Cases per shape and property.
const CASES: u32 = 128;

/// Runs `test` on `CASES` encodings of every shape `keep` selects, panicking
/// with the shape on the first failure.
#[expect(
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn for_every_shape<S, F>(keep: impl Fn(EncodingShape) -> bool, extra: S, test: F)
where
    S: Fn(Encoding) -> BoxedStrategy<Raw>,
    F: Fn(Encoding, Raw) -> Result<(), TestCaseError>,
{
    let shapes: Vec<_> = encoding_shapes().into_iter().filter(|s| keep(*s)).collect();
    assert!(!shapes.is_empty(), "no shape selected");
    for shape in shapes {
        let mut runner = TestRunner::new(Config::with_cases(CASES));
        let strategy = encoding_in(shape).prop_flat_map(|e| (Just(e), extra(e)));
        if let Err(error) = runner.run(&strategy, |(encoding, raw)| test(encoding, raw)) {
            panic!("{shape:?}: {error}");
        }
    }
}

fn single_cell(shape: EncodingShape) -> bool {
    shape.layout == Layout::Default
}

fn integer(shape: EncodingShape) -> bool {
    shape.storage.is_integer()
}

/// The error the codec gives for an integer out of `encoding`'s range.
fn refusal(encoding: Encoding, value: i128) -> EncodingError {
    match encoding.bitfield() {
        Some(field) => EncodingError::BitFieldOutOfRange {
            value,
            mask: field.mask(),
            shift: field.shift(),
            signed: field.is_signed(),
        },
        None => {
            let (min, max) = encoding.storage().int_range().unwrap_or((0, 0));
            EncodingError::OutOfRange {
                value,
                storage: encoding.storage(),
                min,
                max,
            }
        }
    }
}

fn range_of(encoding: Encoding) -> Result<RawRange, TestCaseError> {
    encoding
        .raw_range()
        .map_err(|e| TestCaseError::fail(format!("{encoding:?}: {e}")))
}

// ---------------------------------------------------------------------------
// Default layout: integers
// ---------------------------------------------------------------------------

#[test]
fn every_integer_range_bound_is_in_and_one_past_it_is_out() {
    for_every_shape(
        |s| single_cell(s) && integer(s),
        |_| Just(Raw::Int(0)).boxed(),
        |encoding, _| {
            let range = range_of(encoding)?;
            let int = range.as_int().ok_or_else(|| TestCaseError::fail("int"))?;
            prop_assert_eq!(range.kind(), RawKind::Int);
            for bound in [int.min(), int.max()] {
                prop_assert!(int.contains(bound));
                let bytes = encoding.encode(Raw::Int(bound));
                prop_assert!(bytes.is_ok(), "{:?} {}: {:?}", encoding, bound, bytes);
                let back = bytes.map(|b| encoding.decode(&b));
                prop_assert_eq!(back, Ok(Ok(Raw::Int(bound))));
            }
            for outside in [int.min() - 1, int.max() + 1] {
                prop_assert!(!int.contains(outside));
                prop_assert_eq!(
                    encoding.encode(Raw::Int(outside)),
                    Err(refusal(encoding, outside))
                );
            }
            Ok(())
        },
    );
}

#[test]
fn every_storable_value_is_in_the_range() {
    for_every_shape(
        single_cell,
        |e| value_for(&e),
        |encoding, raw| {
            // Integers always; floats unless NaN or infinite (finite range).
            let inside = range_of(encoding)?.contains(raw);
            prop_assert_eq!(inside, raw.is_finite_number(), "{:?}", raw);
            Ok(())
        },
    );
}

#[test]
fn every_unstorable_value_is_outside_the_range() {
    for_every_shape(
        single_cell,
        |e| invalid_value_for(&e),
        |encoding, raw| {
            prop_assert!(!range_of(encoding)?.contains(raw), "{:?}", raw);
            prop_assert!(encoding.encode(raw).is_err());
            Ok(())
        },
    );
}

/// Integers within `2^8` of a bound or anywhere in the stored value's bits.
fn near_the_bounds(encoding: Encoding) -> BoxedStrategy<Raw> {
    let Some(range) = encoding.stored_range().as_int() else {
        return Just(Raw::Int(0)).boxed();
    };
    let (min, max) = (range.min(), range.max());
    prop_oneof![
        (-256_i128..=256).prop_map(move |d| Raw::Int(min + d)),
        (-256_i128..=256).prop_map(move |d| Raw::Int(max + d)),
        any::<i64>().prop_map(|v| Raw::Int(i128::from(v))),
        any::<u64>().prop_map(|v| Raw::Int(i128::from(v))),
    ]
    .boxed()
}

#[test]
fn an_integer_is_in_the_range_exactly_when_both_codecs_store_it() {
    for_every_shape(
        |s| single_cell(s) && integer(s),
        near_the_bounds,
        |encoding, raw| {
            let inside = range_of(encoding)?.contains(raw);
            let start = vec![0xA5; encoding.storage().cell_bytes()];
            let agreed = check_encode(encoding, raw, &start)
                .map_err(|d| TestCaseError::fail(format!("{d:?}")))?;
            prop_assert_eq!(agreed.is_ok(), inside, "{:?} {:?}", encoding, raw);
            Ok(())
        },
    );
}

#[test]
fn every_decoded_cell_is_in_the_range_when_finite() {
    for_every_shape(
        single_cell,
        |e| {
            bytes_for(&e)
                .prop_map(move |b| e.decode(&b).unwrap_or(Raw::Int(i128::MIN)))
                .boxed()
        },
        |encoding, raw| {
            prop_assert_ne!(raw, Raw::Int(i128::MIN), "{:?} did not decode", encoding);
            let finite = raw.is_finite_number();
            prop_assert_eq!(range_of(encoding)?.contains(raw), finite, "{:?}", raw);
            Ok(())
        },
    );
}

// ---------------------------------------------------------------------------
// Default layout: floats (MF-15)
// ---------------------------------------------------------------------------

/// `±MAX` and the next values out, `±Inf`, of a float storage.
fn float_edges(storage: Storage) -> [(Raw, bool); 4] {
    if storage == Storage::F32 {
        [
            (Raw::F32(f32::from_bits(0x7F7F_FFFF)), true),
            (Raw::F32(f32::from_bits(0xFF7F_FFFF)), true),
            (Raw::F32(f32::from_bits(0x7F80_0000)), false),
            (Raw::F32(f32::from_bits(0xFF80_0000)), false),
        ]
    } else {
        [
            (Raw::F64(f64::from_bits(0x7FEF_FFFF_FFFF_FFFF)), true),
            (Raw::F64(f64::from_bits(0xFFEF_FFFF_FFFF_FFFF)), true),
            (Raw::F64(f64::from_bits(0x7FF0_0000_0000_0000)), false),
            (Raw::F64(f64::from_bits(0xFFF0_0000_0000_0000)), false),
        ]
    }
}

#[test]
fn mf_15_every_float_range_is_minus_max_to_max_and_inf_is_out() {
    for_every_shape(
        |s| single_cell(s) && !integer(s),
        |_| Just(Raw::Int(0)).boxed(),
        |encoding, _| {
            let range = range_of(encoding)?;
            let edges = float_edges(encoding.storage());
            // min = -MAX, max = MAX: never 0, never a subnormal.
            prop_assert_eq!(range.max(), edges[0].0);
            prop_assert_eq!(range.min(), edges[1].0);
            for (raw, inside) in edges {
                prop_assert_eq!(range.contains(raw), inside, "{:?}", raw);
                let mut cell = vec![0; encoding.storage().cell_bytes()];
                let edit = encoding.edit_into(raw, &mut cell, SpecialFloats::Refuse);
                prop_assert_eq!(edit.is_ok(), inside, "{:?}: {:?}", raw, edit);
                if inside {
                    let back = encoding.decode(&cell).map(Raw::float_bits);
                    prop_assert_eq!(back, Ok(raw.float_bits()));
                }
            }
            Ok(())
        },
    );
}

// ---------------------------------------------------------------------------
// Bosch subtract
// ---------------------------------------------------------------------------

/// The Bosch-subtract table of 3 cells whose stored cells are `stored`:
/// `value[i] = 2^(8W) - (s[i] + ... + s[2])`, written out from the layout.
fn bosch_values(encoding: Encoding, stored: [i128; 3]) -> Vec<Raw> {
    let total = 1_i128 << (8 * encoding.storage().cell_bytes());
    (0..3)
        .map(|i| Raw::Int(total - stored[i..].iter().sum::<i128>()))
        .collect()
}

#[test]
fn a_bosch_table_stores_derived_cells_exactly_within_the_stored_range() {
    for_every_shape(
        |s| !single_cell(s),
        |_| (0_usize..3).prop_map(|i| Raw::Int(i as i128)).boxed(),
        |encoding, which| {
            let Raw::Int(which) = which else {
                return Err(TestCaseError::fail("index"));
            };
            let index = usize::try_from(which).unwrap_or(0);
            prop_assert_eq!(
                encoding.raw_range(),
                Err(EncodingError::WholeTableLayout {
                    layout: Layout::BoschSubtract
                })
            );
            let cell = Encoding::new(
                encoding.storage(),
                encoding.endian(),
                encoding.bitfield(),
                Layout::Default,
            )
            .map_err(|e| TestCaseError::fail(e.to_string()))?;
            prop_assert_eq!(Ok(encoding.stored_range()), cell.raw_range());
            let int = encoding
                .stored_range()
                .as_int()
                .ok_or_else(|| TestCaseError::fail("int"))?;
            let mut bytes = vec![0; 3 * encoding.storage().cell_bytes()];
            for bound in [int.min(), int.max()] {
                let mut stored = [int.min(), int.max(), bound];
                let values = bosch_values(encoding, stored);
                prop_assert_eq!(encoding.encode_table(&values, &mut bytes), Ok(()));
                prop_assert_eq!(encoding.decode_table(&bytes, 3), Ok(values));
                for outside in [int.min() - 1, int.max() + 1] {
                    stored[index] = outside;
                    let values = bosch_values(encoding, stored);
                    prop_assert_eq!(
                        encoding.encode_table(&values, &mut bytes),
                        Err(EncodingError::TableCell {
                            index,
                            source: Box::new(refusal(encoding, outside)),
                        })
                    );
                }
            }
            Ok(())
        },
    );
}

/// Every shape is covered by one of the properties above.
#[test]
fn the_properties_cover_every_shape() {
    let shapes = encoding_shapes();
    for field in FieldKind::ALL {
        assert!(shapes.iter().any(|s| s.field == field && single_cell(*s)));
        assert!(shapes.iter().any(|s| s.field == field && !single_cell(*s)));
    }
    for storage in Storage::ALL {
        assert!(
            shapes
                .iter()
                .any(|s| s.storage == storage && single_cell(*s))
        );
    }
}
