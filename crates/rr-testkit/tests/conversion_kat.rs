//! rr-core's conversions (`rr_core::conversion`, US-RR-24-7) against the
//! seed known-answer fixtures (PROJECT.md T2).
//!
//! Every fixture in `fixtures/kat/*.toml` with a `[definition.conversion]`
//! builds through [`Conversion::from_spec`] (both formulas parse and use
//! only `x`), and every `[[decode]]` case's expected raw value converts to
//! its expected real value: exactly, or within the case's `tolerance`.
//!
//! Every such fixture with a `[range]` (US-RR-24-2, PROJECT.md MF-13) has
//! the display range it pins: [`Conversion::real_range`] over the cell
//! encoding's raw range gives its `real_min` and `real_max` (within its
//! `tolerance`), attained at its `raw_at_min` and `raw_at_max`, with its
//! `monotonicity`. The seed set has falling scalings on u8, u16, i16 and
//! u32 storage, whose minimum is at the top of the raw range (legacy
//! reported it as `f(lowest raw)`), and a rising control.
//!
//! This file is a test boundary: it converts the fixture vocabulary to
//! rr-core's types here; the fixture types do not name rr-core.

use std::fs;
use std::path::PathBuf;

use rr_core::conversion::{Conversion, ConversionSpec, Form, FormSpec, Monotonicity};
use rr_core::encoding::{BitField, Encoding, Endian, Layout, Raw, Storage};
use rr_testkit::fixtures::repo_root;
use rr_testkit::kat::{self, Definition, KatKind, RangeCase, RawValue};

/// Fixture raw values are well inside `2^53`, so the conversion is exact.
fn raw_f64(raw: RawValue) -> f64 {
    match raw {
        RawValue::Int(i) => i as f64,
        RawValue::Float(f) => f,
    }
}

/// The seed fixtures, sorted by path.
#[expect(
    clippy::panic,
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn seed_fixtures() -> Vec<PathBuf> {
    let dir = repo_root().join("fixtures").join("kat");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    paths
}

/// The fixture's conversion as rr-core builds it.
fn build(conversion: &kat::Conversion) -> Result<Conversion, String> {
    let spec = ConversionSpec {
        form: FormSpec::Expr {
            to_real: &conversion.to_real,
            to_raw: conversion.to_raw.as_deref(),
        },
        unit: conversion.units.as_deref(),
        format: None,
    };
    Conversion::from_spec(&spec).map_err(|e| format!("conversion refused: {e}"))
}

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

/// The fixture's cell encoding as rr-core builds it.
fn core_encoding(encoding: &kat::Encoding) -> Result<Encoding, String> {
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
        .map(|b| BitField::new(b.mask, b.shift, false))
        .transpose()
        .map_err(|e| format!("bitfield refused: {e}"))?;
    Encoding::new(core_storage(encoding.storage), endian, bitfield, layout)
        .map_err(|e| format!("encoding refused: {e}"))
}

const fn core_monotonicity(m: kat::Monotonicity) -> Monotonicity {
    match m {
        kat::Monotonicity::Increasing => Monotonicity::Increasing,
        kat::Monotonicity::Decreasing => Monotonicity::Decreasing,
        kat::Monotonicity::Constant => Monotonicity::Constant,
        kat::Monotonicity::NonMonotonic => Monotonicity::NonMonotonic,
    }
}

/// Whether rr-core's raw value is the fixture's (an integer exactly, a
/// float by value).
fn same_raw(core: Raw, fixture: RawValue) -> bool {
    match (core, fixture) {
        (Raw::Int(a), RawValue::Int(b)) => a == b,
        (Raw::F32(a), RawValue::Float(b)) => f64::from(a) == b,
        (Raw::F64(a), RawValue::Float(b)) => a == b,
        _ => false,
    }
}

/// Every mismatch between `expected` and rr-core's display range over
/// `encoding`'s raw range: of the fixture's formula ([`Form::Expr`], scanned)
/// and, when rr-core recognises the formula as linear, of its `m * x + b`
/// ([`Form::Linear`], from the ends of the raw range), so both range paths
/// meet the fixture. Each mismatch names the form.
fn range_mismatches(
    encoding: &kat::Encoding,
    conversion: &kat::Conversion,
    expected: &RangeCase,
) -> Vec<String> {
    let built = match build(conversion) {
        Ok(c) => c,
        Err(e) => return vec![e],
    };
    let raw = match core_encoding(encoding).and_then(|e| {
        e.raw_range()
            .map_err(|e| format!("no per-cell raw range: {e}"))
    }) {
        Ok(raw) => raw,
        Err(e) => return vec![e],
    };
    let mut forms = vec![("formula", built.form().clone())];
    if let Some(linear) = built.form().linear() {
        forms.push(("linear m*x+b", Form::Linear(linear)));
    }
    let tolerance = expected.tolerance.unwrap_or(0.0);
    let monotonicity = core_monotonicity(expected.monotonicity);
    let mut out = Vec::new();
    for (form_name, form) in forms {
        let mut push = |m: String| out.push(format!("{form_name}: {m}"));
        let got = match form.real_range(raw) {
            Ok(range) => range,
            Err(e) => {
                push(format!("no range: {e}"));
                continue;
            }
        };
        for (what, extreme, real, at) in [
            ("min", got.min(), expected.real_min, expected.raw_at_min),
            ("max", got.max(), expected.real_max, expected.raw_at_max),
        ] {
            if (extreme.real() - real).abs() > tolerance {
                push(format!(
                    "real {what}: expected {real} (± {tolerance}), got {}",
                    extreme.real()
                ));
            }
            if !same_raw(extreme.raw(), at) {
                push(format!(
                    "raw at {what}: expected {at:?}, got {:?}",
                    extreme.raw()
                ));
            }
        }
        if got.monotonicity() != monotonicity {
            push(format!(
                "monotonicity: expected {monotonicity:?}, got {:?}",
                got.monotonicity()
            ));
        }
        if let Some(undefined) = got.undefined() {
            push(format!(
                "raw value {:?} has no real value ({}); a [range] fixture pins a \
                 conversion defined over the whole raw range",
                undefined.raw(),
                undefined.error()
            ));
        }
    }
    out
}

#[test]
fn every_seed_fixture_conversion_builds_and_matches_its_real_values() {
    let paths = seed_fixtures();
    let mut fixtures = 0;
    let mut checked = 0;
    let mut failures = Vec::new();
    for path in paths {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let fixture = kat::load(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        let KatKind::Decode {
            definition, cases, ..
        } = fixture.data.kind
        else {
            continue;
        };
        let Definition::Inline {
            conversion: Some(conversion),
            ..
        } = definition
        else {
            continue;
        };
        fixtures += 1;
        let built = match build(&conversion) {
            Ok(c) => c,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        assert_eq!(built.unit(), conversion.units.as_deref(), "{name}");
        for (index, case) in cases.iter().enumerate() {
            let Some(expected) = case.real else {
                continue;
            };
            checked += 1;
            let label = format!("{name} [[decode]] #{}", index + 1);
            let tolerance = case.tolerance.unwrap_or(0.0);
            match built.to_real(raw_f64(case.raw)) {
                Ok(real) if (real - expected).abs() <= tolerance => {}
                other => failures.push(format!(
                    "{label}: expected {expected} (± {tolerance}), got {other:?}"
                )),
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        fixtures >= 2,
        "expected the linear and non-linear scaling fixtures, found {fixtures}"
    );
    assert!(checked >= 8, "only {checked} real values checked");
    eprintln!("rr-core conversion: {checked} real value(s) in {fixtures} fixture(s)");
}

#[test]
fn every_seed_fixture_range_matches_rr_core_real_range() {
    let mut ranges = 0;
    let mut decreasing = Vec::new();
    let mut increasing = 0;
    let mut failures = Vec::new();
    for path in seed_fixtures() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let fixture = kat::load(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        let KatKind::Decode {
            definition,
            range: Some(range),
            ..
        } = fixture.data.kind
        else {
            continue;
        };
        // The validator (`real-without-conversion`) and the seed set keep
        // every [range] on an inline conversion.
        let Definition::Inline {
            encoding,
            conversion: Some(conversion),
        } = definition
        else {
            failures.push(format!("{name}: [range] without an inline conversion"));
            continue;
        };
        ranges += 1;
        match range.monotonicity {
            kat::Monotonicity::Decreasing => decreasing.push(encoding.storage),
            kat::Monotonicity::Increasing => increasing += 1,
            kat::Monotonicity::Constant | kat::Monotonicity::NonMonotonic => {}
        }
        failures.extend(
            range_mismatches(&encoding, &conversion, &range)
                .into_iter()
                .map(|m| format!("{name} [range]: {m}")),
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // MF-13: falling scalings on 1-, 2- (unsigned and signed) and 4-byte
    // storage, and a rising control.
    for storage in [
        kat::Storage::U8,
        kat::Storage::U16,
        kat::Storage::I16,
        kat::Storage::U32,
    ] {
        assert!(
            decreasing.contains(&storage),
            "no decreasing [range] fixture on {storage}: {decreasing:?}"
        );
    }
    assert!(increasing >= 1, "no increasing [range] control fixture");
    eprintln!(
        "rr-core real_range: {ranges} [range] fixture(s), {} decreasing",
        decreasing.len()
    );
}
