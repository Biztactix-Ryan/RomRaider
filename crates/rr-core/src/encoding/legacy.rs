//! Parsing the encoding attributes of legacy RomRaider ROM definitions.
//!
//! A legacy `<table>` element describes its encoding with four attributes:
//! `storagetype`, `endian`, `mask` (optional) and `dataLayout` (optional).
//! Legacy parsed them in `RomAttributeParser.java` and `Table.java`, falling
//! back to a default (or decoding as zero) for anything it did not
//! understand. Here every value is either accepted exactly or refused with an
//! [`EncodingError`]; [`parse_encoding`] turns the refusals into
//! [`Error`](crate::diag::Severity::Error) diagnostics with provenance, so a
//! bad definition fails closed instead of showing wrong or zero values
//! (MF-02).
//!
//! Citations are to `legacy/src/main/java/com/romraider/`.
//!
//! # `storagetype` ([`parse_storage_type`])
//!
//! | String | Storage | Legacy |
//! |---|---|---|
//! | `uint8` `uint16` `uint24` `uint32` `uint64` | `U8` `U16` `U24` `U32` `U64` | `xml/RomAttributeParser.java:62-63` |
//! | `int8` `int16` `int24` `int32` `int64` | `I8` `I16` `I24` `I32` `I64` | `xml/RomAttributeParser.java:65-66`, signed per `:73-81` |
//! | `float` (any ASCII case) | `F32` | `xml/RomAttributeParser.java:59-60`; 4 bytes, `Settings.java:124` |
//! | `movi20` (any ASCII case) | `U20` | `xml/RomAttributeParser.java:53-54`; see [`U20_BIT19_SIGN_EXTENDS`](super::U20_BIT19_SIGN_EXTENDS) |
//!
//! Case follows legacy: `float` and `movi20` were compared with
//! `equalsIgnoreCase`, while the `uint`/`int` prefixes were matched with a
//! case-sensitive `startsWith` (so legacy threw on `UINT16`; here it is
//! refused). Refused, though legacy accepted them:
//!
//! * `movi20s` (`RomAttributeParser.java:56-57`), the `MOVI20` immediate
//!   shifted left by 8 — not supported ([`StorageProblem::Movi20s`]).
//! * a bare number such as `2` (`RomAttributeParser.java:68-69`): legacy's
//!   internal code (a byte count, or the magic `20` / `28` / `99`), which says
//!   nothing about signedness ([`StorageProblem::NumericCode`]).
//! * widths legacy rounded down or mis-read: `uint12` (legacy: 1 byte),
//!   `uint016`, `int+16`. Only the exact spellings above are accepted.
//!
//! Legacy also *decoded* `uint24`/`int24`/`uint64`/`int64` as zero
//! (`RomAttributeParser.parseByteValue` has no 3- or 8-byte case); here they
//! are ordinary storages. There is no legacy spelling for `F64`.
//!
//! # `endian` ([`parse_endian`])
//!
//! `big` and `little`, any ASCII case (`xml/RomAttributeParser.java:37,40`).
//! Legacy turned anything else into little-endian (`:43-45`); here it is
//! refused. Legacy's internal marshalling codes `1` (little) and `2` (big)
//! (`Settings.java:94-95`), which it also accepted, are refused too.
//!
//! # `mask` ([`parse_mask`])
//!
//! Hexadecimal digits without a prefix, as legacy's
//! `ByteUtil.parseUnsignedInt(mask, 16)` (`maps/Table.java:537-541`); 1 to 16
//! digits (legacy: at most 32 bits). An empty mask means no bitfield
//! (`Table.java:538`). Legacy silently clamped a mask wider than the storage
//! (`Table.java:547`); [`Encoding::new`] refuses it.
//!
//! # `dataLayout` ([`parse_data_layout`])
//!
//! `bosch_subtract`, trimmed and any ASCII case (`maps/Table.java:520-523`), is
//! [`Layout::BoschSubtract`]; the empty string (the attribute's default,
//! `xml/TableScaleUnmarshaller.java:150`) and `default` (`Table.java:81`) are
//! [`Layout::Default`]. Legacy turned anything else into the default layout
//! (`Table.java:524-526`); here it is refused.
//!
//! # Out of scope
//!
//! Logger definitions spell storage types in a looser dialect (lower-cased
//! substring matching, `logger/ecu/comms/query/EcuQueryData.java:53-75`, and
//! any endian but `little` is big, `logger/ecu/definition/xml/
//! LoggerDefinitionHandler.java:252-258`). That dialect belongs to the logger
//! definition loader.

use super::{BitField, Encoding, EncodingError, Endian, Layout, Storage, StorageProblem};
use crate::diag::{Diagnostics, Loaded, Provenance};

/// Parses a legacy `storagetype` attribute (see the [module docs](self)).
///
/// # Errors
///
/// [`EncodingError::UnsupportedStorage`] for every string not in the table.
pub fn parse_storage_type(text: &str) -> Result<Storage, EncodingError> {
    if text.eq_ignore_ascii_case("float") {
        return Ok(Storage::F32);
    }
    if text.eq_ignore_ascii_case("movi20") {
        return Ok(Storage::U20);
    }
    let storage = match text {
        "uint8" => Storage::U8,
        "uint16" => Storage::U16,
        "uint24" => Storage::U24,
        "uint32" => Storage::U32,
        "uint64" => Storage::U64,
        "int8" => Storage::I8,
        "int16" => Storage::I16,
        "int24" => Storage::I24,
        "int32" => Storage::I32,
        "int64" => Storage::I64,
        _ => {
            return Err(EncodingError::UnsupportedStorage {
                text: text.to_owned(),
                problem: storage_problem(text),
            });
        }
    };
    Ok(storage)
}

/// Why a storage string that is not accepted was refused.
fn storage_problem(text: &str) -> StorageProblem {
    if text.is_empty() {
        StorageProblem::Empty
    } else if text.eq_ignore_ascii_case("movi20s") {
        StorageProblem::Movi20s
    } else if text.bytes().all(|b| b.is_ascii_digit()) {
        StorageProblem::NumericCode
    } else {
        StorageProblem::Unknown
    }
}

/// Parses a legacy `endian` attribute: `big` or `little`, any ASCII case.
///
/// # Errors
///
/// [`EncodingError::UnknownEndian`] for anything else.
pub fn parse_endian(text: &str) -> Result<Endian, EncodingError> {
    if text.eq_ignore_ascii_case("big") {
        Ok(Endian::Big)
    } else if text.eq_ignore_ascii_case("little") {
        Ok(Endian::Little)
    } else {
        Err(EncodingError::UnknownEndian {
            text: text.to_owned(),
        })
    }
}

/// Parses a legacy `mask` attribute: `None` for the empty string, else 1 to
/// 16 hexadecimal digits without a prefix.
///
/// # Errors
///
/// [`EncodingError::UnparsableMask`] for anything else (a `0x` prefix, a
/// sign, whitespace, more than 16 digits).
pub fn parse_mask(text: &str) -> Result<Option<u64>, EncodingError> {
    if text.is_empty() {
        return Ok(None);
    }
    let unparsable = || EncodingError::UnparsableMask {
        text: text.to_owned(),
    };
    // `from_str_radix` alone would also take a leading `+`.
    if !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(unparsable());
    }
    u64::from_str_radix(text, 16)
        .map(Some)
        .map_err(|_| unparsable())
}

/// Parses a legacy `dataLayout` attribute (trimmed, any ASCII case):
/// `bosch_subtract`, or the empty string / `default` for the default layout.
///
/// # Errors
///
/// [`EncodingError::UnknownLayout`] for anything else.
pub fn parse_data_layout(text: &str) -> Result<Layout, EncodingError> {
    let trimmed = text.trim();
    if trimmed.eq_ignore_ascii_case("bosch_subtract") {
        Ok(Layout::BoschSubtract)
    } else if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("default") {
        Ok(Layout::Default)
    } else {
        Err(EncodingError::UnknownLayout {
            text: text.to_owned(),
        })
    }
}

/// The encoding attributes of one legacy `<table>` element, after
/// inheritance has been resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LegacyAttributes<'a> {
    /// The `storagetype` attribute.
    pub storage_type: &'a str,
    /// The `endian` attribute.
    pub endian: &'a str,
    /// The `mask` attribute, if present.
    pub mask: Option<&'a str>,
    /// The `dataLayout` attribute, if present.
    pub data_layout: Option<&'a str>,
}

/// Parses and checks a legacy table's encoding.
///
/// Every attribute is checked, so one call reports every problem; each is an
/// [`Error`](crate::diag::Severity::Error) diagnostic at `provenance` with
/// the [`EncodingError::code`] of the refusal. The value is `Some` exactly
/// when there are no diagnostics; with any, it is `None` (and withheld by
/// [`Loaded`] anyway) — never a default encoding.
pub fn parse_encoding(
    attributes: &LegacyAttributes<'_>,
    provenance: &Provenance,
) -> Loaded<Option<Encoding>> {
    let mut diags = Diagnostics::new();
    let mut report = |error: EncodingError| diags.push(error.to_diagnostic(provenance.clone()));

    let storage = parse_storage_type(attributes.storage_type)
        .map_err(&mut report)
        .ok();
    let endian = parse_endian(attributes.endian).map_err(&mut report).ok();
    let bitfield = attributes
        .mask
        .map_or(Ok(None), parse_mask)
        .and_then(|mask| {
            mask.map(|mask| BitField::new(mask, None, false))
                .transpose()
        })
        .map_err(&mut report)
        .ok();
    let layout = attributes
        .data_layout
        .map_or(Ok(Layout::Default), parse_data_layout)
        .map_err(&mut report)
        .ok();

    let encoding = match (storage, endian, bitfield, layout) {
        (Some(storage), Some(endian), Some(bitfield), Some(layout)) => {
            Encoding::new(storage, endian, bitfield, layout)
                .map_err(&mut report)
                .ok()
        }
        _ => None,
    };
    diags.finish(encoding)
}

#[cfg(test)]
mod tests;
