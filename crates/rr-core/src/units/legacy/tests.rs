//! Unit tests for the legacy unit strings (US-RR-21-5): the alias table's
//! shape, the matching rules, and every unit string of the pinned corpus
//! (`fixtures/corpus-unit-strings.txt`, kept current by rr-defs'
//! corpus-tier `corpus_unit_strings` test) read or reported.

use super::*;
use crate::diag::{Severity, Span};
use crate::units::{FUELS, by_id};

/// The corpus unit-string list, metadata only.
const CORPUS_STRINGS: &str = include_str!("../../../../../fixtures/corpus-unit-strings.txt");

/// Every string of [`CORPUS_STRINGS`], in file order.
fn corpus_strings() -> Vec<&'static str> {
    CORPUS_STRINGS
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let (sources, quoted) = line.split_once(' ').unwrap();
            assert!(
                ["defs", "v370", "defs,v370"].contains(&sources),
                "{line:?}: sources {sources:?}"
            );
            quoted
                .strip_prefix('|')
                .and_then(|s| s.strip_suffix('|'))
                .unwrap_or_else(|| panic!("{line:?} is not `<sources> |<string>|`"))
        })
        .collect()
}

/// The pinned corpus strings that name nothing known, by why. A change
/// here is a change to what users are warned about.
const EXPECTED_UNKNOWN: &[&str] = &[
    // A quantity with no registry unit: fuel economy (l/100km is not affine),
    // angular speed, volume per stroke, sensor slopes, reciprocal distance.
    "km/l",
    "l/100km",
    "mpg (UK)",
    "mpg (US)",
    "degrees/s",
    "mm³/H",
    "mm³/st",
    "bara/v",
    "psia/v",
    "grams/bara",
    "grams/psia",
    "steps/ratio",
    "Rev/Mile",
    // A unit string that does not say the unit: `Time` and `Zeit` label counts,
    // `°TPS` a percentage, `VE` a volumetric efficiency of unstated scale.
    "Time",
    "Zeit",
    "°TPS",
    "VE",
    // An ECU label naming only a quantity, or ending in a bracket that is not
    // a unit.
    "'Primary Open Loop Fueling' Raw Enrichment Offset Additive",
    "0 = FAN1 OFF FAN2 OFF | 1 = FAN1 OFF FAN2 ON | 2 = FAN1 ON FAN2 OFF | 3 = FAN1 ON FAN2 ON",
    "Atmospheric Pressure (bar) Multiplier",
    "Atmospheric Pressure (psi) Multiplier",
    "Atmospheric Pressure Multiplier",
    "Atmospheric Pressure Multiplier Offset",
    "CFP Delta",
    "CL to OL Counter Increment (WARNING - value should NEVER be zero)",
    "Decay Step Value",
    "Engine Load",
    "Engine Speed",
    "Engine Speed Delta",
    "Engine Torque",
    "Enrichment Adder Multiplier",
    "Estimated Primary Open Loop Air/Fuel Ratio Lean Limit",
    "Front Oxygen Sensor Compensation",
    "Fuel Offset Additive",
    "Idle Airflow Target g/s",
    "Ignition Advance Multiplier (IAM)",
    "Ignition Timing Correction",
    "Initial Afterstart Offset",
    "Injector Fuel Offset Additive",
    "Injector Pulse Width Compensation",
    "MAP related",
    "Map Ramping Multiplier (Adder)",
    "Minimum Primary Enrichment Offset Additive 1",
    "Minimum Primary Enrichment Offset Additive Cruise",
    "Minimum Primary Enrichment Offset Additive Non-Cruise",
    "Period in-between decay multiplier application",
    "Port Injection",
    "Primary Enrichment Offset Additive 1",
    "Primary Enrichment Offset Additive 1 Compensation",
    "Smoothing Factor",
    "calculated",
    "change in multiplier",
    "change in raw ecu value",
    "cumulative throttle angle change",
    "tip-in enrichment applied counter",
    "tip-in last applied counter period",
];

// ---------------------------------------------------------------------------
// The alias table
// ---------------------------------------------------------------------------

#[test]
fn each_alias_is_listed_once_trimmed() {
    for (i, (text, _)) in LEGACY_ALIASES.iter().enumerate() {
        assert_eq!(text.trim(), *text, "{text:?} has surrounding whitespace");
        assert!(
            LEGACY_ALIASES[..i].iter().all(|(other, _)| other != text),
            "{text:?} is listed twice"
        );
    }
}

#[test]
fn every_alias_names_a_registry_unit_or_fuel() {
    for &(text, target) in LEGACY_ALIASES {
        match target {
            LegacyUnit::Unit(unit) => {
                assert_eq!(by_id(unit.id), Some(unit), "{text:?}");
            }
            LegacyUnit::UnstatedPressure(unit) => {
                assert_eq!(by_id(unit.id), Some(unit), "{text:?}");
                assert_eq!(unit.dimension, Dimension::Pressure, "{text:?}");
            }
            LegacyUnit::FuelAfr(fuel) => {
                assert!(FUELS.iter().any(|f| f.fuel == fuel), "{text:?}");
            }
            LegacyUnit::NoUnit => {}
        }
        // Each alias reads as itself.
        assert_eq!(legacy_unit(text), Ok(target), "{text:?}");
    }
}

#[test]
fn every_corpus_alias_is_in_the_corpus_list() {
    // The aliases not from the pinned corpus are legacy's own.
    let legacy_only = [
        "AFR Gasoline",
        "AFR Ethonal",
        "AFR Diesel",
        "AFR Methonal",
        "AFR LPG",
        "AFR CNG",
        "AFR Hydrogen",
        "g/cyl",
    ];
    let corpus = corpus_strings();
    for (text, _) in LEGACY_ALIASES {
        let in_corpus =
            corpus.contains(text) || corpus.iter().any(|s| bracketed(s.trim()) == Some(*text));
        assert_eq!(
            in_corpus,
            !legacy_only.contains(text),
            "{text:?}: in the corpus = {in_corpus}"
        );
    }
}

// ---------------------------------------------------------------------------
// Matching
// ---------------------------------------------------------------------------

#[test]
fn exact_case_sensitive_matching() {
    assert_eq!(legacy_unit("C"), Ok(LegacyUnit::Unit(&CELSIUS)));
    assert_eq!(legacy_unit("°C"), Ok(LegacyUnit::Unit(&CELSIUS)));
    assert_eq!(legacy_unit("Degrees F"), Ok(LegacyUnit::Unit(&FAHRENHEIT)));
    assert_eq!(legacy_unit("RPM"), Ok(LegacyUnit::Unit(&REV_PER_MINUTE)));
    assert_eq!(legacy_unit("rpm"), Ok(LegacyUnit::Unit(&REV_PER_MINUTE)));
    // Legacy lower-cased; here a spelling the corpus does not use is unknown.
    for text in ["c", "f", "Rpm", "PSI", "degrees c", "Degrees c", "KM/H"] {
        assert_eq!(
            legacy_unit(text),
            Err(UnknownUnitString {
                text: text.to_owned()
            }),
            "{text:?}"
        );
    }
}

#[test]
fn surrounding_whitespace_is_trimmed() {
    assert_eq!(
        legacy_unit("  psi\t"),
        Ok(LegacyUnit::UnstatedPressure(&PSI))
    );
    assert_eq!(legacy_unit(" "), Ok(LegacyUnit::NoUnit));
    assert_eq!(
        legacy_unit("Ignition Timing Correction (degrees) "),
        Ok(LegacyUnit::Unit(&DEGREE))
    );
    // Inner whitespace is not normalised.
    assert!(legacy_unit("psi  relative").is_err());
    // The error keeps the string as given.
    assert_eq!(
        legacy_unit(" furlongs "),
        Err(UnknownUnitString {
            text: " furlongs ".to_owned()
        })
    );
}

#[test]
fn a_label_ending_in_a_bracketed_unit_is_that_unit() {
    let cases: &[(&str, LegacyUnit)] = &[
        ("Engine Speed (RPM)", LegacyUnit::Unit(&REV_PER_MINUTE)),
        ("Coolant Temp (Degrees F)", LegacyUnit::Unit(&FAHRENHEIT)),
        ("CoolantTemp(DegreesF)", LegacyUnit::Unit(&FAHRENHEIT)),
        ("Advance(degrees)", LegacyUnit::Unit(&DEGREE)),
        (
            "Boost Target (psi relative sea level)",
            LegacyUnit::Unit(&PSI_GAUGE),
        ),
        ("Boost Target (bar absolute)", LegacyUnit::Unit(&BAR)),
        ("Vehicle Speed (KMH)", LegacyUnit::Unit(&KILOMETRE_PER_HOUR)),
        ("(MPa)", LegacyUnit::UnstatedPressure(&MEGAPASCAL)),
        ("Requested Torque (raw ecu value)", LegacyUnit::NoUnit),
        // The last bracket: a `(psia)` earlier in the label is the quantity's.
        (
            "Target Boost (psia) Compensation (%)",
            LegacyUnit::Unit(&PERCENT),
        ),
        ("Name ( ms )", LegacyUnit::Unit(&MILLISECOND)),
    ];
    for &(text, want) in cases {
        assert_eq!(legacy_unit(text), Ok(want), "{text:?}");
    }
    // Not at the end, empty, or not a unit: unknown.
    for text in [
        "Atmospheric Pressure (psi) Multiplier",
        "Name ()",
        "Name (  )",
        "Ignition Advance Multiplier (IAM)",
        "ms)",
        "(ms",
        ")",
        "Idle Airflow Target g/s",
    ] {
        assert!(legacy_unit(text).is_err(), "{text:?}");
    }
}

#[test]
fn bracketed_part() {
    assert_eq!(bracketed("Engine Speed (RPM)"), Some("RPM"));
    assert_eq!(bracketed("a (b) (c)"), Some("c"));
    assert_eq!(bracketed("a (b (c))"), Some("c)"));
    assert_eq!(bracketed("a ( )"), None);
    assert_eq!(bracketed("a (b"), None);
    assert_eq!(bracketed("no brackets"), None);
    assert_eq!(bracketed("°)"), None);
    assert_eq!(bracketed("(µs)"), Some("µs"));
}

#[test]
fn pressures_that_do_not_say_absolute_or_gauge() {
    let unstated = legacy_unit("psi").unwrap();
    assert_eq!(unstated, LegacyUnit::UnstatedPressure(&PSI));
    assert_eq!(unstated.unit(), Some(&PSI));
    assert_eq!(unstated.gauge(), Some(&PSI_GAUGE));
    for (text, absolute, gauge) in [
        ("kPa", &KILOPASCAL, Some(&KILOPASCAL_GAUGE)),
        ("bar", &BAR, Some(&BAR_GAUGE)),
        ("inHg", &INCH_OF_MERCURY, Some(&INCH_OF_MERCURY_GAUGE)),
        (
            "mmHg",
            &MILLIMETRE_OF_MERCURY,
            Some(&MILLIMETRE_OF_MERCURY_GAUGE),
        ),
        ("Pa", &PASCAL, None),
        ("hPa", &HECTOPASCAL, None),
        ("mbar", &HECTOPASCAL, None),
        ("MPa", &MEGAPASCAL, None),
    ] {
        let got = legacy_unit(text).unwrap();
        assert_eq!(got, LegacyUnit::UnstatedPressure(absolute), "{text}");
        assert_eq!(got.gauge(), gauge, "{text}");
    }
    // Stated: absolute or gauge.
    assert_eq!(legacy_unit("psia"), Ok(LegacyUnit::Unit(&PSI)));
    assert_eq!(
        legacy_unit("bar relative"),
        Ok(LegacyUnit::Unit(&BAR_GAUGE))
    );
    assert_eq!(
        legacy_unit("mmHg relative sea level"),
        Ok(LegacyUnit::Unit(&MILLIMETRE_OF_MERCURY_GAUGE))
    );
    // Only an unstated pressure has a gauge twin.
    assert_eq!(LegacyUnit::Unit(&PSI).gauge(), None);
    assert_eq!(LegacyUnit::NoUnit.gauge(), None);
}

#[test]
fn air_fuel_ratio_strings() {
    assert_eq!(legacy_unit("AFR"), Ok(LegacyUnit::Unit(&AIR_FUEL_RATIO)));
    assert_eq!(
        legacy_unit("AFR Gas"),
        Ok(LegacyUnit::FuelAfr(Fuel::Gasoline))
    );
    // Legacy's wideband strings, misspellings and all
    // (SensorConversionsAFR.java:28-34).
    for (text, fuel) in [
        ("AFR Gasoline", Fuel::Gasoline),
        ("AFR Ethonal", Fuel::Ethanol),
        ("AFR Diesel", Fuel::Diesel),
        ("AFR Methonal", Fuel::Methanol),
        ("AFR LPG", Fuel::Lpg),
        ("AFR CNG", Fuel::Cng),
        ("AFR Hydrogen", Fuel::Hydrogen),
    ] {
        let got = legacy_unit(text).unwrap();
        assert_eq!(got, LegacyUnit::FuelAfr(fuel), "{text}");
        assert_eq!(got.unit(), Some(&AIR_FUEL_RATIO), "{text}");
    }
    assert_eq!(legacy_unit("Lambda"), Ok(LegacyUnit::Unit(&LAMBDA)));
    // φ = 1/λ is a ratio, never lambda.
    assert_eq!(
        legacy_unit("fuel-air equivalence ratio"),
        Ok(LegacyUnit::Unit(&RATIO))
    );
}

#[test]
fn strings_that_say_no_unit() {
    for text in [
        "",
        "-",
        "raw ecu value",
        "count",
        "On/Off",
        "steps",
        "gear",
        "Unknown",
    ] {
        let got = legacy_unit(text).unwrap();
        assert_eq!(got, LegacyUnit::NoUnit, "{text:?}");
        assert_eq!(got.unit(), None, "{text:?}");
    }
}

// ---------------------------------------------------------------------------
// The Warning
// ---------------------------------------------------------------------------

#[test]
fn an_unknown_string_is_a_warning_quoting_it() {
    let at = Provenance::new("logger.xml", Span::line_col(12, 5));
    let e = legacy_unit("km/l").unwrap_err();
    assert_eq!(e.code(), codes::UNKNOWN_UNIT_STRING);
    assert_eq!(
        e.to_string(),
        "unit string \"km/l\" names no known unit: the value is shown with it as a label \
         and cannot be converted"
    );
    let d = e.to_diagnostic(at.clone());
    assert_eq!(d.severity(), Severity::Warning);
    assert_eq!(d.code(), codes::UNKNOWN_UNIT_STRING);
    assert_eq!(d.code().as_str(), "UNT0001");
    assert_eq!(d.message(), e.to_string());
    assert_eq!(d.provenance(), &at);
    let registered = codes::REGISTRY
        .iter()
        .find(|i| i.code == codes::UNKNOWN_UNIT_STRING)
        .unwrap();
    assert_eq!(registered.severity, Severity::Warning);
}

// ---------------------------------------------------------------------------
// The pinned corpus
// ---------------------------------------------------------------------------

#[test]
fn the_corpus_list_is_sorted_and_distinct() {
    let strings = corpus_strings();
    assert!(strings.len() > 250, "{} strings", strings.len());
    for pair in strings.windows(2) {
        assert!(pair[0].as_bytes() < pair[1].as_bytes(), "{pair:?}");
    }
}

/// US-RR-21-3, without the corpus: every unit string of the pinned ECU and
/// logger definition corpora, as committed in
/// `fixtures/corpus-unit-strings.txt` (`defs` and `v370` sources alike, so
/// the strings of `logger_v370.zip`, which CI never has, are covered in
/// every run), resolves to a registry unit (or the no-unit / unstated
/// pressure outcome) or is an `UNT0001` Warning whose message quotes it.
/// rr-defs' corpus-tier `corpus_unit_strings` test proves the list is the
/// corpus's, and checks the Warning's location there.
#[test]
fn every_corpus_string_resolves_or_warns() {
    let at = Provenance::new("fixtures/corpus-unit-strings.txt", Span::line_col(1, 1));
    let mut mapped = 0;
    let mut unknown = Vec::new();
    for text in corpus_strings() {
        match legacy_unit(text) {
            Ok(_) => mapped += 1,
            Err(e) => {
                assert_eq!(e.text, text);
                let d = e.to_diagnostic(at.clone());
                assert_eq!(d.severity(), Severity::Warning, "{text:?}");
                assert_eq!(d.code(), codes::UNKNOWN_UNIT_STRING, "{text:?}");
                assert_eq!(d.provenance(), &at, "{text:?}");
                assert!(d.message().contains(&format!("{text:?}")), "{text:?}");
                unknown.push(text);
            }
        }
    }
    println!(
        "{mapped} corpus unit strings resolve, {} warn",
        unknown.len()
    );
    let unexpected: Vec<_> = unknown
        .iter()
        .filter(|t| !EXPECTED_UNKNOWN.contains(t))
        .collect();
    let now_known: Vec<_> = EXPECTED_UNKNOWN
        .iter()
        .filter(|t| !unknown.contains(t))
        .collect();
    assert!(
        unexpected.is_empty() && now_known.is_empty(),
        "unknown but not in EXPECTED_UNKNOWN: {unexpected:#?}\n\
         in EXPECTED_UNKNOWN but known (or not in the corpus): {now_known:#?}"
    );
}
