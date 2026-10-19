//! The Encoding round-trip property (story US-RR-19, task US-RR-19-1):
//! `decode(encode(x)) == x` for **every** storage × endian × bitfield ×
//! layout combination over the full value range, and the result agrees with
//! the independent reference codec (`rr_testkit::reference`, PROJECT.md T2).
//!
//! * **The combination list comes from the enums.** [`every!`] lists each
//!   enum's variants once and matches on them without a wildcard, so a new
//!   `Storage`, `Endian`, `Layout` or `FieldKind` variant does not compile
//!   until it is listed here; the lists must then equal `Storage::ALL`,
//!   `ENDIANS`, `LAYOUTS` and `FieldKind::ALL` (which the strategies are
//!   built from), and their full product, split by what `Encoding::new`
//!   accepts, must equal `encoding_shapes()`.
//! * **Every combination is run, every time.** Rather than one proptest
//!   whose random shape choice might skip a combination, each property loops
//!   over every valid shape and runs its own proptest runner on it
//!   ([`CELL_CASES`] / [`TABLE_CASES`] cases per shape), so a failure names
//!   the shape and the shrunk counterexample.
//! * **Default layout** (one cell): values over the full raw range (`min`,
//!   `max`, `0`, `±1`, the sign and bitfield-width boundaries, uniform values;
//!   floats from random bits and every IEEE 754 class: `±0`, subnormals,
//!   `±Inf`, quiet and signalling NaNs with payloads), compared bit-exactly.
//!   rr-core alone: `decode(encode(x)) == x`. Against the reference: the same
//!   bytes from the same value written over the same arbitrary bytes, the
//!   same value from the same random bytes, and the same refusal for a value
//!   that cannot be stored.
//! * **Bosch subtract** is a whole-table layout (legacy
//!   `maps/DataCell.java:193-207` decode, `:219-231` encode), so it is run
//!   at table level: `decode_table(encode_table(v)) == v`; the bytes agree
//!   with the reference's Bosch-subtract table codec, and also with the
//!   reference's *default-layout* encode of the stored cells this file
//!   derives itself from the legacy formula (which covers signed bitfields,
//!   which the unsigned-only reference cannot take in a Bosch table).
//! * **Unrepresentable combinations** (`u20` little-endian, a bitfield on a
//!   float, Bosch subtract on a float or `u20`) are errors in both codecs:
//!   `Encoding::new` refuses them, and the reference refuses to decode or
//!   encode them, writing nothing.
//!
//! Legacy semantics this relies on (read, never run, D-024): a bitfield is
//! `(value & mask) >> firstOneOfMask(mask)`
//! (`xml/RomAttributeParser.java:125-131`); a `MOVI20` immediate is
//! sign-extended from bit 19 (`xml/RomAttributeParser.java:181-191`, D-026).

use std::collections::HashSet;

use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use rr_core::encoding::{BitField, Encoding, Endian, Layout, Raw, RawKind, Storage};
use rr_testkit::encoding_oracle::{
    Refusal, check_bytes, check_encode_table, check_table_bytes, check_value,
    reference_encode_table, reference_storage, same_bits,
};
use rr_testkit::reference::{self, ReferenceError};
use rr_testkit::strategies::encoding::{
    ENDIANS, EncodingShape, FieldKind, LAYOUTS, bytes_for, encoding_in, encoding_shapes,
    invalid_value_for, table_bytes_for, value_for,
};

/// Cases per single-cell shape (67 shapes).
const CELL_CASES: u32 = 512;
/// Cases per Bosch-subtract shape (60 shapes).
const TABLE_CASES: u32 = 256;

/// Every variant of a fieldless enum, as an array. The `match` has no
/// wildcard, so a variant missing from the list is a compile error
/// (non-exhaustive patterns).
macro_rules! every {
    ($ty:ident: $($variant:ident),+ $(,)?) => {{
        let all = [$($ty::$variant),+];
        for x in all {
            match x {
                $($ty::$variant)|+ => {}
            }
        }
        all
    }};
}

fn storages() -> Vec<Storage> {
    every!(Storage: U8, U16, U20, U24, U32, U64, I8, I16, I24, I32, I64, F32, F64).to_vec()
}

fn endians() -> Vec<Endian> {
    every!(Endian: Big, Little).to_vec()
}

fn layouts() -> Vec<Layout> {
    every!(Layout: Default, BoschSubtract).to_vec()
}

fn field_kinds() -> Vec<FieldKind> {
    every!(FieldKind: Whole, Unsigned, Signed).to_vec()
}

/// Every storage × endian × bitfield kind × layout, from the enum lists.
fn product() -> Vec<EncodingShape> {
    let mut out = Vec::new();
    for storage in storages() {
        for endian in endians() {
            for field in field_kinds() {
                for layout in layouts() {
                    out.push(EncodingShape {
                        storage,
                        endian,
                        field,
                        layout,
                    });
                }
            }
        }
    }
    out
}

/// A bitfield of `kind` that fits every integer storage (`None` for
/// [`FieldKind::Whole`]).
fn representative(kind: FieldKind) -> Option<BitField> {
    match kind {
        FieldKind::Whole => None,
        FieldKind::Unsigned => BitField::new(0b1, None, false).ok(),
        FieldKind::Signed => BitField::new(0b11, None, true).ok(),
    }
}

/// Whether rr-core accepts `shape` (with a bitfield that fits any storage).
fn accepted(shape: EncodingShape) -> bool {
    Encoding::new(
        shape.storage,
        shape.endian,
        representative(shape.field),
        shape.layout,
    )
    .is_ok()
}

/// The valid shapes, derived from the enums and `Encoding::new`.
fn valid_shapes() -> Vec<EncodingShape> {
    product().into_iter().filter(|s| accepted(*s)).collect()
}

/// The same storage, byte order and bitfield in the default layout: how one
/// cell of `encoding` is stored.
fn cell_of(encoding: Encoding) -> Option<Encoding> {
    Encoding::new(
        encoding.storage(),
        encoding.endian(),
        encoding.bitfield(),
        Layout::Default,
    )
    .ok()
}

/// The Bosch-subtract table values whose stored cells are `stored`, written
/// out here from legacy's definition (`maps/DataCell.java:193-207`):
/// `value[i] = 2^(8W) - (s[i] + ... + s[N-1])`. A non-integer stored value
/// is kept as it is (it is only generated to be refused).
fn bosch_values_of_stored(encoding: Encoding, stored: &[Raw]) -> Vec<Raw> {
    let total = 1_i128 << (8 * encoding.storage().cell_bytes());
    (0..stored.len())
        .map(|i| match stored[i] {
            Raw::Int(_) => Raw::Int(stored[i..].iter().fold(total, |acc, s| match s {
                Raw::Int(s) => acc - s,
                Raw::F32(_) | Raw::F64(_) => acc,
            })),
            other => other,
        })
        .collect()
}

/// The kind of the innermost reference error (a table error wraps its
/// cell's).
fn root_kind(error: &ReferenceError) -> &'static str {
    match error {
        ReferenceError::TableCell { source, .. } => root_kind(source),
        ReferenceError::Unsupported(_) => "unsupported",
        ReferenceError::InvalidBitfield { .. } => "invalid bitfield",
        _ => "other",
    }
}

fn table_same(a: &[Raw], b: &[Raw]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same_bits(*a, *b))
}

/// Runs `test` on `cases` inputs of `strategy` for `shape`, panicking with
/// the shape and proptest's shrunk counterexample on failure.
#[expect(
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn run_shape<S: Strategy>(
    shape: EncodingShape,
    cases: u32,
    strategy: &S,
    test: impl Fn(S::Value) -> Result<(), TestCaseError>,
) where
    S::Value: std::fmt::Debug,
{
    let mut runner = TestRunner::new(Config {
        // Persist failing seeds next to this file, as `proptest!` does.
        source_file: Some(file!()),
        ..Config::with_cases(cases)
    });
    if let Err(error) = runner.run(strategy, test) {
        panic!("{shape:?}: {error}");
    }
}

// ---------------------------------------------------------------------------
// The combination list
// ---------------------------------------------------------------------------

#[test]
fn the_combination_list_is_derived_from_the_enums() {
    let as_set = |v: &[Storage]| v.iter().copied().collect::<HashSet<_>>();
    assert_eq!(as_set(&storages()), as_set(&Storage::ALL));
    assert_eq!(storages().len(), Storage::ALL.len());
    assert_eq!(endians(), ENDIANS.to_vec());
    assert_eq!(layouts(), LAYOUTS.to_vec());
    assert_eq!(field_kinds(), FieldKind::ALL.to_vec());
    // Every raw kind is exercised: an integer for each integer storage, one
    // float kind per float storage.
    let kinds = every!(RawKind: Int, F32, F64);
    for kind in kinds {
        let storages_of_kind = storages()
            .into_iter()
            .filter(|s| match kind {
                RawKind::Int => s.is_integer(),
                RawKind::F32 => *s == Storage::F32,
                RawKind::F64 => *s == Storage::F64,
            })
            .count();
        assert!(storages_of_kind > 0, "{kind}: no storage");
    }

    let all = product();
    assert_eq!(all.len(), 13 * 2 * 3 * 2);
    let valid: HashSet<EncodingShape> = valid_shapes().into_iter().collect();
    let listed: HashSet<EncodingShape> = encoding_shapes().into_iter().collect();
    assert_eq!(
        valid,
        listed,
        "encoding_shapes() vs the enum product: missing {:?}, extra {:?}",
        valid.difference(&listed).collect::<Vec<_>>(),
        listed.difference(&valid).collect::<Vec<_>>()
    );
    // u20: big-endian only, no Bosch; floats: no bitfield, no Bosch.
    // Default: 21 integer storage/endian pairs x 3 + 4 float pairs = 67;
    // Bosch: 20 pairs x 3 = 60.
    assert_eq!(valid.len(), 67 + 60);
    assert_eq!(all.len() - valid.len(), 29);
}

// ---------------------------------------------------------------------------
// Unrepresentable combinations
// ---------------------------------------------------------------------------

#[test]
fn unrepresentable_combinations_are_errors_in_both_codecs() {
    let invalid: Vec<EncodingShape> = product().into_iter().filter(|s| !accepted(*s)).collect();
    assert_eq!(invalid.len(), 29);
    let mut kinds = HashSet::new();
    for shape in invalid {
        let EncodingShape {
            storage,
            endian,
            field,
            layout,
        } = shape;
        // rr-core refuses it, whatever the bitfield.
        let fields: Vec<Option<BitField>> = match field {
            FieldKind::Whole => vec![None],
            FieldKind::Unsigned => [1, 0x80, 0xFF]
                .into_iter()
                .map(|m| BitField::new(m, None, false).ok())
                .collect(),
            FieldKind::Signed => [0b11, 0xFF]
                .into_iter()
                .map(|m| BitField::new(m, None, true).ok())
                .collect(),
        };
        for bitfield in fields {
            assert!(bitfield.is_some() || field == FieldKind::Whole);
            let refused = Encoding::new(storage, endian, bitfield, layout);
            assert!(refused.is_err(), "{shape:?} {bitfield:?}: {refused:?}");
        }

        // The reference refuses it too. It has no signed bitfields, so a
        // signed field is handed over as its (unsigned) mask.
        let mut theirs = reference::Encoding::new(
            reference_storage(storage),
            match endian {
                Endian::Big => reference::Endian::Big,
                Endian::Little => reference::Endian::Little,
            },
        )
        .with_layout(match layout {
            Layout::Default => reference::Layout::Default,
            Layout::BoschSubtract => reference::Layout::BoschSubtract,
        });
        if field != FieldKind::Whole {
            theirs = theirs.with_bitfield(reference::Bitfield::new(0b11, None));
        }
        let value = match storage {
            Storage::F32 => reference::Raw::F32(1.0),
            Storage::F64 => reference::Raw::F64(1.0),
            _ => reference::Raw::Int(1),
        };
        let bytes = [0x5A_u8; 16];
        let mut unsupported = |result: Result<(), ReferenceError>, what: &str| {
            let error = result.err();
            assert!(
                matches!(
                    error.as_ref().map(Refusal::of_reference),
                    Some(Refusal::Other)
                ),
                "{shape:?}: reference {what}: {error:?}"
            );
            kinds.insert(error.as_ref().map(root_kind));
        };
        let mut written = bytes;
        match layout {
            Layout::Default => {
                unsupported(reference::decode(&bytes, 0, &theirs).map(|_| ()), "decode");
                unsupported(reference::encode(value, &theirs, &mut written, 0), "encode");
            }
            Layout::BoschSubtract => {
                unsupported(
                    reference::decode_table(&bytes, 0, 2, &theirs).map(|_| ()),
                    "decode_table",
                );
                unsupported(
                    reference::encode_table(&[value, value], &theirs, &mut written, 0),
                    "encode_table",
                );
            }
        }
        assert_eq!(written, bytes, "{shape:?}: a refused write changed bytes");
    }
    // Both refusal kinds occur: Unsupported (u20 little-endian, Bosch on a
    // float or u20) and InvalidBitfield (a bitfield on a float).
    assert_eq!(
        kinds,
        HashSet::from([Some("unsupported"), Some("invalid bitfield")]),
        "{kinds:?}"
    );
}

// ---------------------------------------------------------------------------
// Default layout: every single-cell shape
// ---------------------------------------------------------------------------

#[test]
fn every_cell_shape_round_trips_over_its_full_range_and_agrees_with_the_reference() {
    let shapes: Vec<EncodingShape> = valid_shapes()
        .into_iter()
        .filter(|s| s.layout == Layout::Default)
        .collect();
    assert_eq!(shapes.len(), 67);
    for shape in shapes {
        let strategy = encoding_in(shape).prop_flat_map(|e| {
            (
                Just(e),
                value_for(&e),
                bytes_for(&e),
                bytes_for(&e),
                invalid_value_for(&e),
            )
        });
        run_shape(
            shape,
            CELL_CASES,
            &strategy,
            |(e, raw, start, random, bad)| {
                prop_assert_eq!(EncodingShape::of(e), shape);
                // rr-core alone: decode(encode(x)) == x, bit-exact.
                let bytes = e
                    .encode(raw)
                    .map_err(|err| TestCaseError::fail(format!("encode {raw:?}: {err}")))?;
                let back = e
                    .decode(&bytes)
                    .map_err(|err| TestCaseError::fail(format!("decode {bytes:02X?}: {err}")))?;
                prop_assert!(same_bits(back, raw), "{raw:?} -> {bytes:02X?} -> {back:?}");
                // The reference writes the same bytes over the same arbitrary
                // bytes, and both read the value back (encode agreement).
                let written = check_value(e, raw, &start);
                prop_assert!(matches!(written, Ok(Ok(_))), "{written:?}");
                // The same value from the same random bytes in both codecs, and
                // it writes back over the inverted bytes (decode agreement).
                let decoded = check_bytes(e, &random);
                prop_assert!(matches!(decoded, Ok(Ok(_))), "{decoded:?}");
                // A value that cannot be stored: the same refusal, no write.
                let refused = check_value(e, bad, &start);
                prop_assert!(
                    matches!(
                        refused,
                        Ok(Err(Refusal::OutOfRange
                            | Refusal::BitFieldOutOfRange
                            | Refusal::KindMismatch))
                    ),
                    "{bad:?}: {refused:?}"
                );
                Ok(())
            },
        );
    }
}

// ---------------------------------------------------------------------------
// Bosch subtract: every whole-table shape
// ---------------------------------------------------------------------------

#[test]
fn every_bosch_shape_round_trips_at_table_level_and_agrees_with_the_reference() {
    let shapes: Vec<EncodingShape> = valid_shapes()
        .into_iter()
        .filter(|s| s.layout == Layout::BoschSubtract)
        .collect();
    assert_eq!(shapes.len(), 60);
    for shape in shapes {
        let strategy = encoding_in(shape).prop_flat_map(|e| {
            let cell = cell_of(e).unwrap_or(e);
            (
                Just(e),
                vec(value_for(&cell), 1..=6),
                table_bytes_for(&cell, 6..=6),
                table_bytes_for(&e, 1..=6),
                (0_usize..6, invalid_value_for(&cell)),
            )
        });
        run_shape(
            shape,
            TABLE_CASES,
            &strategy,
            |(e, stored, start, random, (bad_index, bad))| {
                prop_assert_eq!(EncodingShape::of(e), shape);
                let cell = cell_of(e).ok_or_else(|| TestCaseError::fail("no cell encoding"))?;
                let values = bosch_values_of_stored(e, &stored);

                // rr-core alone: decode_table(encode_table(v)) == v.
                let mut ours = start.clone();
                e.encode_table(&values, &mut ours)
                    .map_err(|err| TestCaseError::fail(format!("encode {values:?}: {err}")))?;
                let back = e
                    .decode_table(&ours, values.len())
                    .map_err(|err| TestCaseError::fail(format!("decode {ours:02X?}: {err}")))?;
                prop_assert!(table_same(&back, &values), "{values:?} -> {back:?}");

                // The reference's Bosch-subtract table codec agrees (bytes
                // and round trip; signed fields by round trip only).
                let written = check_encode_table(e, &values, &start);
                prop_assert!(matches!(written, Ok(Ok(_))), "{written:?}");

                // The reference's default-layout encode of the stored cells
                // derived here writes the same bytes, signed fields included.
                let mut theirs = start.clone();
                let reference = reference_encode_table(cell, &stored, &mut theirs);
                prop_assert!(matches!(reference, Some(Ok(()))), "{reference:?}");
                prop_assert_eq!(&ours, &theirs, "stored {:?}", stored);

                // Random table bytes decode the same in both and write back.
                let decoded = check_table_bytes(e, &random);
                prop_assert!(matches!(decoded, Ok(Ok(_))), "{decoded:?}");

                // A table with one stored cell that does not fit is refused
                // by both, nothing written.
                let mut bad_stored = stored.clone();
                let index = bad_index % bad_stored.len();
                bad_stored[index] = bad;
                let bad_values = bosch_values_of_stored(e, &bad_stored);
                let refused = check_encode_table(e, &bad_values, &start);
                prop_assert!(
                    matches!(
                        refused,
                        Ok(Err(Refusal::OutOfRange
                            | Refusal::BitFieldOutOfRange
                            | Refusal::KindMismatch))
                    ),
                    "{bad_values:?}: {refused:?}"
                );
                let mut theirs = start.clone();
                let reference = reference_encode_table(cell, &bad_stored, &mut theirs);
                prop_assert!(matches!(reference, Some(Err(_))), "{reference:?}");
                prop_assert_eq!(&theirs, &start);
                Ok(())
            },
        );
    }
}
