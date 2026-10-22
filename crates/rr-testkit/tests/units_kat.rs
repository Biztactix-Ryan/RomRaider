//! rr-core's unit registry (`rr_core::units`, US-RR-21) against the unit
//! known-answer fixtures (`[[unit]]` cases in `fixtures/kat/*.toml`).
//!
//! US-RR-21-4: the registry's conversion factors are backed by known
//! answers. `fixtures/kat/units-defined-factors.toml` holds them, each citing
//! NIST SP 811 or a hand calculation from exact definitions.
//!
//! US-RR-21-1: **every** conversion factor has one
//! ([`every_factor_has_a_cited_known_answer`], [`uncovered`]), so a unit or
//! fuel cannot be added without its known answer:
//!
//! * a registry unit that is not its dimension's base unit is covered by a
//!   case converting between it and that base unit (either direction), not
//!   through a third unit, so the case pins this unit's [`ToBase`] alone. The
//!   unit's side of the case (`value` when it is `from`, `expected` when it is
//!   `to`) must not be 0 for a pure factor, where 0 is 0 whatever the factor;
//!   an affine unit (one with an `offset`: kelvin, degree Fahrenheit) needs
//!   two such cases at different points, since two points fix an affine map;
//! * a fuel of [`FUELS`] is covered by an air/fuel ratio <-> lambda case for
//!   it;
//! * a covering case cites its reference in its `source` (a `standard`'s
//!   `doc` and `section`, a `hand-calc`'s `working`, a `cross-source`'s
//!   sources): a unit case one of [`UNIT_REFERENCES`]; a fuel whose ratio is
//!   [`StoichBasis::Reference`] one of [`FUEL_REFERENCES`]. A fuel whose
//!   ratio is legacy's convention ([`StoichBasis::Legacy`]: gasoline, diesel,
//!   LPG, CNG) has no SI/NIST-equivalent reference, and its case must say so
//!   in those words ([`UNREFERENCED`]) rather than cite one;
//! * a covering case is exact or has a tolerance within a relative
//!   [`MAX_RELATIVE_TOLERANCE`], so it would catch a factor wrong in its
//!   twelfth significant digit.
//!
//! A base unit needs no case: its conversion is the identity (rr-core's
//! units tests check that, and so does [`uncovered`]).
//!
//! Every `[[unit]]` case of every fixture must name two registry units
//! ([`units::by_id`]), and [`units::convert`] of its `value` must give its
//! `expected`:
//!
//! * a case without `tolerance` is exact, compared bit for bit;
//! * a case with `tolerance` is within that absolute tolerance.
//!
//! A case with a `fuel` (US-RR-21-5) converts between an air/fuel ratio and
//! lambda with [`units::afr_to_lambda`] or [`units::lambda_to_afr`] for
//! that fuel; every fuel of [`FUELS`] must have one
//! (`fixtures/kat/units-afr-lambda.toml`).
//!
//! This file is a test boundary: the fixture types do not name rr-core.

use std::fs;
use std::path::PathBuf;

use rr_core::units::{self, Dimension, FUELS, Fuel, StoichBasis, ToBase, UNITS};
use rr_testkit::fixtures::repo_root;
use rr_testkit::kat::{self, KatKind, Source, UnitCase};

/// The seed fixture, and the fewest cases it may hold (so a truncated file
/// cannot pass quietly).
const SEED_FIXTURE: &str = "units-defined-factors.toml";
const SEED_MIN_CASES: usize = 16;

/// The references a unit case may cite (module docs): NIST SP 811 (2008),
/// the BIPM SI Brochure, NIST Handbook 44 and ISO 80000. The 1959
/// international yard and pound values are cited through SP 811.
const UNIT_REFERENCES: &[&str] = &[
    "NIST SP 811",
    "SI Brochure",
    "NIST Handbook 44",
    "ISO 80000",
];

/// The references a pure-compound fuel's case may cite: Heywood, *Internal
/// Combustion Engine Fundamentals* (1988), the source rr-core's fuel table
/// cites.
const FUEL_REFERENCES: &[&str] = &["Heywood"];

/// What a legacy-convention fuel's case says instead of citing a reference.
const UNREFERENCED: &str = "no SI/NIST-equivalent reference";

/// The widest tolerance a covering case may have, relative to `expected`
/// (or to 1, for an `expected` below 1).
const MAX_RELATIVE_TOLERANCE: f64 = 1e-12;

/// `(fixture file name, case number from 1, case)` for every `[[unit]]` case.
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn unit_cases() -> Vec<(String, usize, UnitCase)> {
    let dir = repo_root().join("fixtures").join("kat");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    let mut out = Vec::new();
    for path in paths {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let fixture = kat::load(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        let KatKind::Unit { cases } = fixture.data.kind else {
            continue; // other fixtures have no unit conversions
        };
        for (index, case) in cases.into_iter().enumerate() {
            out.push((name.clone(), index + 1, case));
        }
    }
    out
}

/// Why `case` fails against rr-core, or `None` when it passes.
fn failure(case: &UnitCase) -> Option<String> {
    let Some(from) = units::by_id(&case.from) else {
        return Some(format!("`from` {:?} is not a registry unit id", case.from));
    };
    let Some(to) = units::by_id(&case.to) else {
        return Some(format!("`to` {:?} is not a registry unit id", case.to));
    };
    let converted = match &case.fuel {
        None => units::convert(case.value, from, to),
        Some(id) => {
            let Some(fuel) = Fuel::by_id(id) else {
                return Some(format!("`fuel` {id:?} is not a fuel id of units::FUELS"));
            };
            match (from.dimension, to.dimension) {
                (Dimension::AirFuelRatio, Dimension::Lambda) => {
                    units::afr_to_lambda(case.value, fuel)
                }
                (Dimension::Lambda, Dimension::AirFuelRatio) => {
                    units::lambda_to_afr(case.value, fuel)
                }
                _ => {
                    return Some(format!(
                        "`fuel` is only for an air/fuel ratio <-> lambda case, not {} -> {}",
                        case.from, case.to
                    ));
                }
            }
        }
    };
    let got = match converted {
        Ok(got) => got,
        Err(e) => return Some(format!("does not convert: {e}")),
    };
    let ok = match case.tolerance {
        None => got.to_bits() == case.expected.to_bits(),
        Some(tolerance) => (got - case.expected).abs() <= tolerance,
    };
    (!ok).then(|| {
        format!(
            "{} {} is {got:?} {}, expected {:?}{}",
            case.value,
            case.from,
            case.to,
            case.expected,
            case.tolerance
                .map(|t| format!(" within {t:e}"))
                .unwrap_or_default()
        )
    })
}

#[test]
fn every_unit_case_matches_rr_core() {
    let cases = unit_cases();
    let seed = cases
        .iter()
        .filter(|(name, _, _)| name == SEED_FIXTURE)
        .count();
    assert!(
        seed >= SEED_MIN_CASES,
        "{SEED_FIXTURE} has {seed} [[unit]] case(s); expected at least {SEED_MIN_CASES}"
    );
    // Every fuel's ratio is pinned by at least one AFR <-> lambda case.
    for fuel in FUELS {
        assert!(
            cases
                .iter()
                .any(|(_, _, case)| case.fuel.as_deref() == Some(fuel.id)),
            "no [[unit]] case converts an air/fuel ratio of {}",
            fuel.id
        );
    }
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|(name, number, case)| {
            failure(case).map(|why| format!("{name} [[unit]] #{number}: {why}"))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The text of `source` that may name a reference: a `standard`'s `doc` and
/// `section`, a `hand-calc`'s `working`, a `cross-source`'s sources. A
/// stock ROM, a car trace or a legacy test file is not a reference.
fn source_text(source: &Source) -> String {
    match source {
        Source::HandCalc { working } => working.clone(),
        Source::Standard { doc, section } => format!("{doc} {section}"),
        Source::CrossSource { sources } => sources.join(" "),
        Source::StockRom { .. } | Source::CarTrace { .. } | Source::LegacyTest { .. } => {
            String::new()
        }
    }
}

/// Whether `case`'s source names one of `references`.
fn cites(case: &UnitCase, references: &[&str]) -> bool {
    let text = source_text(&case.source);
    references.iter().any(|r| text.contains(r))
}

/// Whether `case` is exact or within [`MAX_RELATIVE_TOLERANCE`].
fn tight(case: &UnitCase) -> bool {
    case.tolerance
        .is_none_or(|t| t <= MAX_RELATIVE_TOLERANCE * case.expected.abs().max(1.0))
}

/// Every registry unit and fuel without a cited known answer among `cases`
/// (module docs: what covers one), each named with why; empty when all are
/// covered.
fn uncovered(cases: &[UnitCase]) -> Vec<String> {
    let mut missing = Vec::new();
    for unit in UNITS {
        let base = unit.dimension.base();
        if unit.id == base.id {
            if unit.to_base != ToBase::IDENTITY {
                missing.push(format!(
                    "unit `{}` ({}): the {} base unit, but its conversion is not the identity",
                    unit.id, unit.name, unit.dimension
                ));
            }
            continue;
        }
        let affine = unit.to_base.offset != 0.0;
        let needed = if affine { 2 } else { 1 };
        // The unit's side of each case between it and the base unit.
        let between: Vec<(&UnitCase, f64)> = cases
            .iter()
            .filter(|c| c.fuel.is_none())
            .filter_map(|c| {
                if c.from == unit.id && c.to == base.id {
                    Some((c, c.value))
                } else if c.from == base.id && c.to == unit.id {
                    Some((c, c.expected))
                } else {
                    None
                }
            })
            .collect();
        let mut points: Vec<f64> = between
            .iter()
            .filter(|(c, side)| cites(c, UNIT_REFERENCES) && tight(c) && (affine || *side != 0.0))
            .map(|&(_, side)| side)
            .collect();
        points.sort_by(f64::total_cmp);
        points.dedup();
        if points.len() < needed {
            let what = if affine {
                "cases at two different points"
            } else {
                "a case at a non-zero value"
            };
            missing.push(format!(
                "unit `{}` ({}): needs {what} converting it to or from `{}`, citing one of \
                 {UNIT_REFERENCES:?}, exact or within a relative {MAX_RELATIVE_TOLERANCE:e}; \
                 has {} such point(s) of {} case(s) between the two",
                unit.id,
                unit.name,
                base.id,
                points.len(),
                between.len()
            ));
        }
    }
    for fuel in FUELS {
        let ok = |c: &&UnitCase| {
            c.fuel.as_deref() == Some(fuel.id)
                && tight(c)
                && match fuel.basis {
                    StoichBasis::Reference => {
                        cites(c, FUEL_REFERENCES) && !source_text(&c.source).contains(UNREFERENCED)
                    }
                    StoichBasis::Legacy => source_text(&c.source).contains(UNREFERENCED),
                }
        };
        if !cases.iter().any(|c| ok(&c)) {
            let what = match fuel.basis {
                StoichBasis::Reference => format!("citing one of {FUEL_REFERENCES:?}"),
                StoichBasis::Legacy => {
                    format!("saying its legacy ratio has {UNREFERENCED:?} (legacy's convention)")
                }
            };
            missing.push(format!(
                "fuel `{}`: needs an air/fuel ratio <-> lambda case for its ratio {} {what}",
                fuel.id, fuel.stoich_afr
            ));
        }
    }
    missing
}

#[test]
fn every_factor_has_a_cited_known_answer() {
    let cases: Vec<UnitCase> = unit_cases().into_iter().map(|(_, _, c)| c).collect();
    let missing = uncovered(&cases);
    assert!(
        missing.is_empty(),
        "{} registry factor(s) lack a cited known answer in fixtures/kat/units-*.toml \
         (US-RR-21-1, fixtures/README.md `[[unit]]` cases):\n{}",
        missing.len(),
        missing.join("\n")
    );
}

#[test]
fn a_factor_without_a_cited_known_answer_is_named() {
    let named = |missing: &[String], what: &str| missing.iter().any(|m| m.starts_with(what));
    // No cases: every unit but the base units, and every fuel, is named.
    let missing = uncovered(&[]);
    let non_base = UNITS
        .iter()
        .filter(|u| u.id != u.dimension.base().id)
        .count();
    assert_eq!(missing.len(), non_base + FUELS.len(), "{missing:#?}");
    assert!(named(&missing, "unit `mph` "));
    assert!(!named(&missing, "unit `kmh` "));
    assert!(named(&missing, "fuel `gasoline`"));

    let standard = |doc: &str| Source::Standard {
        doc: doc.to_owned(),
        section: "Appendix B.8: mile (mi) to meter (m), 1.609 344 E+03, exact".to_owned(),
    };
    let case = |from: &str, to: &str, value: f64, expected: f64, source: Source| UnitCase {
        from: from.to_owned(),
        to: to.to_owned(),
        value,
        expected,
        tolerance: None,
        fuel: None,
        source,
        note: None,
    };
    let mile = case("mi", "m", 1.0, 1609.344, standard("NIST SP 811 (2008)"));
    assert!(!named(
        &uncovered(std::slice::from_ref(&mile)),
        "unit `mi` "
    ));
    // Either direction covers it.
    let back = case("m", "mi", 1609.344, 1.0, standard("NIST SP 811 (2008)"));
    assert!(!named(&uncovered(&[back]), "unit `mi` "));
    // Not through a third unit: mi -> km leaves `mi` (and `km`) uncovered.
    let via_km = case("mi", "km", 1.0, 1.609344, standard("NIST SP 811 (2008)"));
    let missing = uncovered(&[via_km]);
    assert!(named(&missing, "unit `mi` ") && named(&missing, "unit `km` "));
    // Without a reference in the source.
    let uncited = case("mi", "m", 1.0, 1609.344, standard("a textbook"));
    assert!(named(&uncovered(&[uncited]), "unit `mi` "));
    let working = |w: &str| Source::HandCalc {
        working: w.to_owned(),
    };
    let bare = case(
        "mi",
        "m",
        1.0,
        1609.344,
        working("1 mi = 1 * 1609.344 m = 1609.344 m"),
    );
    assert!(named(&uncovered(&[bare]), "unit `mi` "));
    // At zero, where any factor gives zero.
    let zero = case("mi", "m", 0.0, 0.0, standard("NIST SP 811 (2008)"));
    assert!(named(&uncovered(&[zero]), "unit `mi` "));
    // With a tolerance wide enough to hide a wrong factor.
    let loose = UnitCase {
        tolerance: Some(1e-6),
        ..mile.clone()
    };
    assert!(named(&uncovered(&[loose]), "unit `mi` "));
    // An affine unit needs two different points.
    let sp811 = "NIST SP 811 (2008) B.8: t/degC = (t/degF - 32) / 1.8";
    let boiling = case("fahrenheit", "celsius", 212.0, 100.0, working(sp811));
    let freezing = case("celsius", "fahrenheit", 0.0, 32.0, working(sp811));
    assert!(named(
        &uncovered(std::slice::from_ref(&boiling)),
        "unit `fahrenheit` "
    ));
    assert!(named(
        &uncovered(&[boiling.clone(), boiling.clone()]),
        "unit `fahrenheit` "
    ));
    assert!(!named(
        &uncovered(&[boiling, freezing]),
        "unit `fahrenheit` "
    ));
    // A fuel: a reference ratio cites its reference; a legacy convention
    // says it has none.
    let fuelled = |fuel: &str, w: &str| UnitCase {
        fuel: Some(fuel.to_owned()),
        ..case("lambda", "afr", 1.0, 1.0, working(w))
    };
    let heywood = "AFRs = 6.47 (Heywood Table D.4); AFR = 1 * 6.47 = 6.47";
    assert!(!named(
        &uncovered(&[fuelled("methanol", heywood)]),
        "fuel `methanol`"
    ));
    assert!(named(
        &uncovered(&[fuelled("methanol", "AFR = 1 * 6.47 = 6.47")]),
        "fuel `methanol`"
    ));
    assert!(named(
        &uncovered(&[fuelled("gasoline", "AFR = 1 * 14.7 = 14.7")]),
        "fuel `gasoline`"
    ));
    let convention = "AFR = 1 * 14.7 = 14.7; a convention, no SI/NIST-equivalent reference";
    assert!(!named(
        &uncovered(&[fuelled("gasoline", convention)]),
        "fuel `gasoline`"
    ));
    // A legacy convention cannot stand in for a pure compound's reference.
    assert!(named(
        &uncovered(&[fuelled("methanol", convention)]),
        "fuel `methanol`"
    ));
}

#[test]
fn a_wrong_answer_is_caught() {
    // The comparison itself: a case off by one ulp, or naming no unit, fails.
    let case = |from: &str, to: &str, expected: f64, tolerance: Option<f64>| UnitCase {
        from: from.to_owned(),
        to: to.to_owned(),
        value: 1.0,
        expected,
        tolerance,
        fuel: None,
        source: kat::Source::HandCalc {
            working: String::new(),
        },
        note: None,
    };
    assert_eq!(failure(&case("mph", "kmh", 1.609_344, None)), None);
    let off = f64::from_bits(1.609_344_f64.to_bits() + 1);
    assert!(failure(&case("mph", "kmh", off, None)).is_some());
    assert_eq!(failure(&case("mph", "kmh", off, Some(1e-15))), None);
    // KPH_2_MPH's inverted reading (MF-42).
    assert!(failure(&case("kmh", "mph", 1.609_344, Some(1e-9))).is_some());
    assert!(failure(&case("furlong", "kmh", 1.0, None)).is_some_and(|w| w.contains("`from`")));
    assert!(failure(&case("mph", "furlong", 1.0, None)).is_some_and(|w| w.contains("`to`")));
    assert!(
        failure(&case("mph", "celsius", 1.0, None)).is_some_and(|w| w.contains("does not convert"))
    );
    // A fuel case: the fuel must exist and the units be AFR and lambda.
    let fuelled = |from: &str, to: &str, fuel: &str, value: f64, expected: f64| UnitCase {
        fuel: Some(fuel.to_owned()),
        value,
        ..case(from, to, expected, None)
    };
    assert_eq!(
        failure(&fuelled("afr", "lambda", "gasoline", 14.7, 1.0)),
        None
    );
    assert_eq!(
        failure(&fuelled("lambda", "afr", "gasoline", 1.0, 14.7)),
        None
    );
    assert!(failure(&fuelled("afr", "lambda", "methanol", 14.7, 1.0)).is_some());
    assert!(
        failure(&fuelled("afr", "lambda", "e85", 14.7, 1.0)).is_some_and(|w| w.contains("`fuel`"))
    );
    assert!(
        failure(&fuelled("mph", "kmh", "gasoline", 1.0, 1.0))
            .is_some_and(|w| w.contains("only for"))
    );
    // Without a fuel, AFR and lambda do not convert.
    assert!(
        failure(&case("afr", "lambda", 1.0, None)).is_some_and(|w| w.contains("does not convert"))
    );
}
