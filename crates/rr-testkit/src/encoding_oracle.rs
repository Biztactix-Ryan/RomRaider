//! rr-core's Encoding checked against the independent reference codec
//! (US-RR-19-12, PROJECT.md T2(b)).
//!
//! This module is the **test boundary** between the two vocabularies: it
//! converts `rr_core::encoding` types to [`reference`](mod@reference)'s and back, runs both
//! codecs on the same input and says whether they agree. It lives outside
//! [`reference`](mod@reference) on purpose: it may name rr-core, the reference codec may not
//! (`cargo xtask check-reference-isolation`), and nothing in [`reference`](mod@reference)
//! reaches this module.
//!
//! * [`check_decode`]: the same bytes decode to the same raw value (floats
//!   bit-exact, NaN payloads and `-0.0` included), or both codecs refuse them
//!   for the same [`Refusal`].
//! * [`check_encode`]: the same value written over the same bytes gives the
//!   same bytes, or both refuse it for the same reason and leave the bytes
//!   untouched.
//! * [`check_value`]: [`check_encode`], then the written bytes decode (both
//!   codecs) back to the value, bit-exact: the round trip.
//! * [`check_bytes`]: [`check_decode`], then the decoded value written over
//!   the bit-inverted bytes (so every bit the encoding does not own differs)
//!   is the same value again.
//! * [`assert_agrees_with_reference`] / [`assert_bytes_agree_with_reference`]
//!   panic with the disagreement, for plain `#[test]`s.
//!
//! # Signed bitfields
//!
//! The reference codec reads every bitfield unsigned. For a signed field this
//! module does the two's complement step itself (its own code, sharing none
//! with rr-core): a decoded field of width `w = reach_bits - shift` with bit
//! `w - 1` set is `value - 2^w`, and a value written to it must be in
//! `-2^(w-1)..2^(w-1)` and is handed to the reference as `value mod 2^w`. A
//! value outside that range is handed over as `-1`, which an unsigned
//! reference field always refuses, so both codecs still refuse it in the
//! same order (a short buffer before a bad value).
//!
//! # Whole tables (US-RR-19-10)
//!
//! A Bosch-subtract cell is refused by both single-cell codecs
//! ([`Refusal::WholeTable`]). Whole tables, in either layout, are compared
//! through the table codecs (`Encoding::decode_table` / `encode_table`
//! against [`reference::decode_table`] / [`reference::encode_table`]):
//!
//! * [`check_decode_table`]: the same table bytes decode to the same values,
//!   or both codecs refuse them.
//! * [`check_encode_table`]: the same values written over the same bytes
//!   give the same bytes, or both refuse them and leave the bytes untouched;
//!   then the written bytes decode back to the values.
//! * [`check_table_bytes`]: [`check_decode_table`], then the decoded values
//!   written back over the bit-inverted bytes ([`check_encode_table`]) are
//!   the same values again.
//!
//! Signed bitfields in a table: a decoded Bosch-subtract value is `2^(8W)`
//! minus the sum of the stored cells, so reading a negative field (`f -
//! 2^w`) unsigned (`f`) makes every value up to it `2^w` smaller.
//! [`reference_decode_table`] adds `2^w` to value `i` for each negative field
//! from cell `i` to the end, learning which fields are negative from the
//! reference's own default-layout decode of the same bytes. A Bosch-subtract
//! *encode* with a signed field cannot be handed to the unsigned reference
//! without computing the stored cells (the transform under test), so
//! [`check_encode_table`] checks those by round trip through
//! [`check_decode_table`] only, and accepts rr-core's refusal if it leaves
//! the bytes untouched.

use std::fmt;

use rr_core::encoding::{BitField, Encoding, EncodingError, Endian, Layout, Raw, Storage};

use crate::reference::{self, ReferenceError};

/// The storage in the reference codec's vocabulary.
#[must_use]
pub const fn reference_storage(storage: Storage) -> reference::Storage {
    match storage {
        Storage::U8 => reference::Storage::U8,
        Storage::U16 => reference::Storage::U16,
        Storage::U20 => reference::Storage::U20,
        Storage::U24 => reference::Storage::U24,
        Storage::U32 => reference::Storage::U32,
        Storage::U64 => reference::Storage::U64,
        Storage::I8 => reference::Storage::I8,
        Storage::I16 => reference::Storage::I16,
        Storage::I24 => reference::Storage::I24,
        Storage::I32 => reference::Storage::I32,
        Storage::I64 => reference::Storage::I64,
        Storage::F32 => reference::Storage::F32,
        Storage::F64 => reference::Storage::F64,
    }
}

/// The encoding in the reference codec's vocabulary: storage, byte order,
/// layout, and the bitfield's mask and effective shift (its signedness is
/// handled here, see the module docs).
#[must_use]
pub const fn reference_encoding(encoding: Encoding) -> reference::Encoding {
    let endian = match encoding.endian() {
        Endian::Big => reference::Endian::Big,
        Endian::Little => reference::Endian::Little,
    };
    let layout = match encoding.layout() {
        Layout::Default => reference::Layout::Default,
        Layout::BoschSubtract => reference::Layout::BoschSubtract,
    };
    let plain =
        reference::Encoding::new(reference_storage(encoding.storage()), endian).with_layout(layout);
    match encoding.bitfield() {
        Some(field) => {
            plain.with_bitfield(reference::Bitfield::new(field.mask(), Some(field.shift())))
        }
        None => plain,
    }
}

/// A raw value in the reference codec's vocabulary.
#[must_use]
pub const fn reference_raw(raw: Raw) -> reference::Raw {
    match raw {
        Raw::Int(v) => reference::Raw::Int(v),
        Raw::F32(v) => reference::Raw::F32(v),
        Raw::F64(v) => reference::Raw::F64(v),
    }
}

/// A reference raw value in rr-core's vocabulary.
#[must_use]
pub const fn core_raw(raw: reference::Raw) -> Raw {
    match raw {
        reference::Raw::Int(v) => Raw::Int(v),
        reference::Raw::F32(v) => Raw::F32(v),
        reference::Raw::F64(v) => Raw::F64(v),
    }
}

/// Bit-exact equality: integers equal, floats of the same width with the same
/// bit pattern (so a NaN equals itself and `-0.0` differs from `+0.0`).
#[must_use]
pub fn same_bits(a: Raw, b: Raw) -> bool {
    match (a, b) {
        (Raw::Int(a), Raw::Int(b)) => a == b,
        (Raw::F32(a), Raw::F32(b)) => a.to_bits() == b.to_bits(),
        (Raw::F64(a), Raw::F64(b)) => a.to_bits() == b.to_bits(),
        _ => false,
    }
}

/// Why a codec refused, in terms both codecs share.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// The cell does not fit the bytes.
    ShortInput,
    /// An integer outside the storage's range.
    OutOfRange,
    /// An integer that does not fit the bitfield.
    BitFieldOutOfRange,
    /// A value of the wrong kind for the storage.
    KindMismatch,
    /// A cell of a whole-table layout on its own.
    WholeTable,
    /// Anything else (an invalid encoding: never expected, as every
    /// [`Encoding`] is valid).
    Other,
}

impl Refusal {
    /// The refusal an rr-core error stands for.
    #[must_use]
    pub const fn of_core(error: &EncodingError) -> Self {
        match error {
            EncodingError::ShortInput { .. } => Self::ShortInput,
            EncodingError::OutOfRange { .. } => Self::OutOfRange,
            EncodingError::BitFieldOutOfRange { .. } => Self::BitFieldOutOfRange,
            EncodingError::KindMismatch { .. } => Self::KindMismatch,
            EncodingError::WholeTableLayout { .. } => Self::WholeTable,
            EncodingError::TableCell { source, .. } => Self::of_core(source),
            _ => Self::Other,
        }
    }

    /// The refusal a reference error stands for.
    #[must_use]
    pub fn of_reference(error: &ReferenceError) -> Self {
        match error {
            ReferenceError::ShortInput { .. } => Self::ShortInput,
            ReferenceError::OutOfRange { .. } => Self::OutOfRange,
            ReferenceError::BitfieldOutOfRange { .. } => Self::BitFieldOutOfRange,
            ReferenceError::KindMismatch { .. } => Self::KindMismatch,
            ReferenceError::Unsupported(what) if what.contains("depends on every later cell") => {
                Self::WholeTable
            }
            ReferenceError::TableCell { source, .. } => Self::of_reference(source),
            ReferenceError::Unsupported(_) | ReferenceError::InvalidBitfield { .. } => Self::Other,
        }
    }
}

/// What both codecs did with one input: the same result, or the same
/// refusal.
pub type Agreed<T> = Result<T, Refusal>;

/// rr-core and the reference codec disagree.
#[derive(Debug, Clone, PartialEq)]
pub struct Disagreement {
    /// The encoding.
    pub encoding: Encoding,
    /// What happened, with both codecs' results.
    pub detail: String,
}

impl fmt::Display for Disagreement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "rr-core and the reference codec disagree on {:?}: {}",
            self.encoding, self.detail
        )
    }
}

impl std::error::Error for Disagreement {}

fn disagree(encoding: Encoding, detail: String) -> Disagreement {
    Disagreement { encoding, detail }
}

/// Width of a field after its shift: one more than the index of its top bit.
const fn field_width(field: BitField) -> u32 {
    field.reach_bits() - field.shift()
}

/// A signed field's unsigned reading as two's complement in its width.
fn sign_extend_field(value: i128, field: BitField) -> i128 {
    let width = field_width(field);
    if field.is_signed() && (value >> (width - 1)) & 1 == 1 {
        value - (1_i128 << width)
    } else {
        value
    }
}

/// The value to hand the (unsigned-only) reference for a signed field.
fn unsigned_field_value(value: i128, field: BitField) -> i128 {
    if !field.is_signed() {
        return value;
    }
    let width = field_width(field);
    let half = 1_i128 << (width - 1);
    if (-half..half).contains(&value) {
        value.rem_euclid(1_i128 << width)
    } else {
        // Outside the field's range: a value every unsigned field refuses.
        -1
    }
}

/// The reference codec's decode of the cell at the start of `bytes`, in
/// rr-core's vocabulary (signed fields sign-extended, see the module docs).
///
/// # Errors
///
/// The reference codec's refusal.
pub fn reference_decode(encoding: Encoding, bytes: &[u8]) -> Result<Raw, ReferenceError> {
    let raw = core_raw(reference::decode(bytes, 0, &reference_encoding(encoding))?);
    Ok(match (raw, encoding.bitfield()) {
        (Raw::Int(value), Some(field)) => Raw::Int(sign_extend_field(value, field)),
        (raw, _) => raw,
    })
}

/// The reference codec's encode of `raw` into the cell at the start of
/// `bytes` (signed fields as in the module docs).
///
/// # Errors
///
/// The reference codec's refusal; `bytes` is then unchanged.
pub fn reference_encode(
    encoding: Encoding,
    raw: Raw,
    bytes: &mut [u8],
) -> Result<(), ReferenceError> {
    let raw = match (raw, encoding.bitfield()) {
        (Raw::Int(value), Some(field)) => Raw::Int(unsigned_field_value(value, field)),
        (raw, _) => raw,
    };
    reference::encode(reference_raw(raw), &reference_encoding(encoding), bytes, 0)
}

/// The reference codec's decode of the table of `bytes.len() / cell_bytes`
/// cells in `bytes`, in rr-core's vocabulary: the comparison point for
/// rr-core's whole-table codec (US-RR-19-10). Signed fields are sign-extended
/// (see the module docs).
///
/// # Errors
///
/// The reference codec's refusal.
pub fn reference_decode_table(
    encoding: Encoding,
    bytes: &[u8],
) -> Result<Vec<Raw>, ReferenceError> {
    let count = bytes.len() / encoding.storage().cell_bytes();
    let reference = reference_encoding(encoding);
    let values = reference::decode_table(bytes, 0, count, &reference)?;
    let mut values: Vec<Raw> = values.into_iter().map(core_raw).collect();
    let Some(field) = encoding.bitfield().filter(|f| f.is_signed()) else {
        return Ok(values);
    };
    // Which stored fields are negative, from the reference's per-cell decode.
    let cells = reference::decode_table(
        bytes,
        0,
        count,
        &reference.with_layout(reference::Layout::Default),
    )?;
    let negative: Vec<bool> = cells
        .into_iter()
        .map(|cell| matches!(core_raw(cell), Raw::Int(v) if sign_extend_field(v, field) < 0))
        .collect();
    let span = 1_i128 << field_width(field);
    let bosch = encoding.layout() == Layout::BoschSubtract;
    // Default: each negative field is 2^w smaller. Bosch subtract: each value
    // is 2^w larger per negative field from it to the end.
    let mut later_negatives = 0;
    for (value, negative) in values.iter_mut().zip(negative).rev() {
        later_negatives += i128::from(negative);
        if let Raw::Int(v) = value {
            *v = if bosch {
                *v + span * later_negatives
            } else if negative {
                *v - span
            } else {
                *v
            };
        }
    }
    Ok(values)
}

/// The reference codec's encode of `values` as a table from the start of
/// `bytes`, or `None` for a Bosch-subtract table with a signed field, which
/// the unsigned reference cannot be given (see the module docs). Signed
/// fields of a default-layout table as in [`reference_encode`].
///
/// # Errors
///
/// The reference codec's refusal; `bytes` is then unchanged.
pub fn reference_encode_table(
    encoding: Encoding,
    values: &[Raw],
    bytes: &mut [u8],
) -> Option<Result<(), ReferenceError>> {
    let values: Vec<reference::Raw> = match encoding.bitfield() {
        Some(field) if field.is_signed() => {
            if encoding.layout() == Layout::BoschSubtract {
                return None;
            }
            values
                .iter()
                .map(|raw| match *raw {
                    Raw::Int(v) => reference_raw(Raw::Int(unsigned_field_value(v, field))),
                    other => reference_raw(other),
                })
                .collect()
        }
        _ => values.iter().copied().map(reference_raw).collect(),
    };
    Some(reference::encode_table(
        &values,
        &reference_encoding(encoding),
        bytes,
        0,
    ))
}

/// Element-wise [`same_bits`].
fn same_table(a: &[Raw], b: &[Raw]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same_bits(*a, *b))
}

/// Decodes the table of `bytes.len() / cell_bytes` cells in `bytes` with
/// both codecs.
///
/// # Errors
///
/// A [`Disagreement`] unless both decode the same values (bit-exact) or both
/// refuse for the same [`Refusal`].
pub fn check_decode_table(
    encoding: Encoding,
    bytes: &[u8],
) -> Result<Agreed<Vec<Raw>>, Disagreement> {
    let count = bytes.len() / encoding.storage().cell_bytes();
    let ours = encoding.decode_table(bytes, count);
    let theirs = reference_decode_table(encoding, bytes);
    match (ours, theirs) {
        (Ok(a), Ok(b)) if same_table(&a, &b) => Ok(Ok(a)),
        (Err(a), Err(b)) if Refusal::of_core(&a) == Refusal::of_reference(&b) => {
            Ok(Err(Refusal::of_core(&a)))
        }
        (ours, theirs) => Err(disagree(
            encoding,
            format!("decode table {bytes:02X?}: rr-core {ours:?}, reference {theirs:?}"),
        )),
    }
}

/// Encodes the table `values` over a copy of `start` with both codecs (see
/// the module docs for a Bosch-subtract table with a signed field), then
/// decodes the written bytes back ([`check_decode_table`] of the table's
/// bytes) to `values`, bit-exact.
///
/// # Errors
///
/// A [`Disagreement`] unless both write the same bytes, or both refuse for
/// the same [`Refusal`] and leave the copy untouched; or if the round trip
/// changes a value.
pub fn check_encode_table(
    encoding: Encoding,
    values: &[Raw],
    start: &[u8],
) -> Result<Agreed<Vec<u8>>, Disagreement> {
    let mut ours = start.to_vec();
    let mut theirs = start.to_vec();
    let our_result = encoding.encode_table(values, &mut ours);
    let their_result = reference_encode_table(encoding, values, &mut theirs);
    let detail = |what: &str| {
        format!(
            "encode table {values:?} over {start:02X?}: {what}; rr-core {our_result:?} -> \
             {ours:02X?}, reference {their_result:?} -> {theirs:02X?}"
        )
    };
    let agreed = match (&our_result, &their_result) {
        (Ok(()), Some(Ok(()))) if ours == theirs => Ok(()),
        (Ok(()), None) => Ok(()),
        (Ok(()), Some(Ok(()))) => return Err(disagree(encoding, detail("different bytes"))),
        (Err(a), their_outcome) => {
            let refusal = Refusal::of_core(a);
            let same_refusal = match their_outcome {
                Some(Err(b)) => refusal == Refusal::of_reference(b),
                Some(Ok(())) => false,
                None => true,
            };
            if !same_refusal {
                return Err(disagree(encoding, detail("different refusals")));
            }
            if ours != start || theirs != start {
                return Err(disagree(
                    encoding,
                    detail("a refused write changed the bytes"),
                ));
            }
            Err(refusal)
        }
        (Ok(()), Some(Err(_))) => return Err(disagree(encoding, detail("one codec refused"))),
    };
    if let Err(refusal) = agreed {
        return Ok(Err(refusal));
    }
    let table = values.len() * encoding.storage().cell_bytes();
    match check_decode_table(encoding, &ours[..table])? {
        Ok(back) if same_table(&back, values) => Ok(Ok(ours)),
        back => Err(disagree(
            encoding,
            format!(
                "round trip of table {values:?} over {start:02X?} wrote {ours:02X?}, which \
                 decodes to {back:?}"
            ),
        )),
    }
}

/// [`check_decode_table`], then the decoded values written back
/// ([`check_encode_table`]) over the bit-inverted `bytes`, so every bit the
/// encoding does not own differs from the original, decode to the same
/// values again.
///
/// # Errors
///
/// A [`Disagreement`] from either step, or if the decoded values cannot be
/// written back.
pub fn check_table_bytes(
    encoding: Encoding,
    bytes: &[u8],
) -> Result<Agreed<Vec<Raw>>, Disagreement> {
    let values = match check_decode_table(encoding, bytes)? {
        Ok(values) => values,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let inverted: Vec<u8> = bytes.iter().map(|b| !b).collect();
    match check_encode_table(encoding, &values, &inverted)? {
        Ok(_) => Ok(Ok(values)),
        Err(refusal) => Err(disagree(
            encoding,
            format!(
                "table {bytes:02X?} decodes to {values:?}, which both codecs refuse to write \
                 ({refusal:?})"
            ),
        )),
    }
}

/// Decodes the cell at the start of `bytes` with both codecs.
///
/// # Errors
///
/// A [`Disagreement`] unless both decode the same value (bit-exact) or both
/// refuse for the same [`Refusal`].
pub fn check_decode(encoding: Encoding, bytes: &[u8]) -> Result<Agreed<Raw>, Disagreement> {
    let ours = encoding.decode(bytes);
    let theirs = reference_decode(encoding, bytes);
    match (ours, theirs) {
        (Ok(a), Ok(b)) if same_bits(a, b) => Ok(Ok(a)),
        (Err(a), Err(b)) if Refusal::of_core(&a) == Refusal::of_reference(&b) => {
            Ok(Err(Refusal::of_core(&a)))
        }
        (ours, theirs) => Err(disagree(
            encoding,
            format!("decode {bytes:02X?}: rr-core {ours:?}, reference {theirs:?}"),
        )),
    }
}

/// Encodes `raw` over a copy of `start` with both codecs.
///
/// # Errors
///
/// A [`Disagreement`] unless both write the same bytes, or both refuse for
/// the same [`Refusal`] and leave the copy untouched.
pub fn check_encode(
    encoding: Encoding,
    raw: Raw,
    start: &[u8],
) -> Result<Agreed<Vec<u8>>, Disagreement> {
    let mut ours = start.to_vec();
    let mut theirs = start.to_vec();
    let our_result = encoding.encode_into(raw, &mut ours);
    let their_result = reference_encode(encoding, raw, &mut theirs);
    let detail = |what: &str| {
        format!(
            "encode {raw:?} over {start:02X?}: {what}; rr-core {our_result:?} -> {ours:02X?}, \
             reference {their_result:?} -> {theirs:02X?}"
        )
    };
    match (&our_result, &their_result) {
        (Ok(()), Ok(())) if ours == theirs => Ok(Ok(ours)),
        (Ok(()), Ok(())) => Err(disagree(encoding, detail("different bytes"))),
        (Err(a), Err(b)) => {
            let refusal = Refusal::of_core(a);
            if refusal != Refusal::of_reference(b) {
                Err(disagree(encoding, detail("different refusals")))
            } else if ours != start || theirs != start {
                Err(disagree(
                    encoding,
                    detail("a refused write changed the bytes"),
                ))
            } else {
                Ok(Err(refusal))
            }
        }
        _ => Err(disagree(encoding, detail("one codec refused"))),
    }
}

/// [`check_encode`], then the round trip: the written bytes decode, through
/// both codecs ([`check_decode`]), to `raw` bit-exact.
///
/// # Errors
///
/// A [`Disagreement`] from either step, or if the round trip changes the
/// value.
pub fn check_value(
    encoding: Encoding,
    raw: Raw,
    start: &[u8],
) -> Result<Agreed<Vec<u8>>, Disagreement> {
    let bytes = match check_encode(encoding, raw, start)? {
        Ok(bytes) => bytes,
        Err(refusal) => return Ok(Err(refusal)),
    };
    match check_decode(encoding, &bytes)? {
        Ok(back) if same_bits(back, raw) => Ok(Ok(bytes)),
        back => Err(disagree(
            encoding,
            format!(
                "round trip of {raw:?} over {start:02X?} wrote {bytes:02X?}, which decodes to {back:?}"
            ),
        )),
    }
}

/// [`check_decode`], then the value written back ([`check_value`]) over the
/// bit-inverted `bytes`, so every bit the encoding does not own differs from
/// the original, decodes to the same value again.
///
/// # Errors
///
/// A [`Disagreement`] from either step.
pub fn check_bytes(encoding: Encoding, bytes: &[u8]) -> Result<Agreed<Raw>, Disagreement> {
    let raw = match check_decode(encoding, bytes)? {
        Ok(raw) => raw,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let inverted: Vec<u8> = bytes.iter().map(|b| !b).collect();
    match check_value(encoding, raw, &inverted)? {
        Ok(_) => Ok(Ok(raw)),
        Err(refusal) => Err(disagree(
            encoding,
            format!(
                "{bytes:02X?} decode to {raw:?}, which both codecs refuse to write ({refusal:?})"
            ),
        )),
    }
}

/// [`check_value`] for a plain `#[test]`; returns what both codecs agreed on.
///
/// # Panics
///
/// On a [`Disagreement`], with its message.
#[track_caller]
#[expect(
    clippy::panic,
    reason = "test-only assertion helper: a disagreement must fail the calling test"
)]
pub fn assert_agrees_with_reference(encoding: Encoding, raw: Raw, start: &[u8]) -> Agreed<Vec<u8>> {
    match check_value(encoding, raw, start) {
        Ok(agreed) => agreed,
        Err(disagreement) => panic!("{disagreement}"),
    }
}

/// [`check_bytes`] for a plain `#[test]`; returns what both codecs agreed on.
///
/// # Panics
///
/// On a [`Disagreement`], with its message.
#[track_caller]
#[expect(
    clippy::panic,
    reason = "test-only assertion helper: a disagreement must fail the calling test"
)]
pub fn assert_bytes_agree_with_reference(encoding: Encoding, bytes: &[u8]) -> Agreed<Raw> {
    match check_bytes(encoding, bytes) {
        Ok(agreed) => agreed,
        Err(disagreement) => panic!("{disagreement}"),
    }
}
