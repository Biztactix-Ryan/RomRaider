//! Unit tests for the fuel table and the AFR ↔ lambda conversions
//! (US-RR-21-5). The known answers with cited sources are
//! `fixtures/kat/units-afr-lambda.toml`, run by rr-testkit's `units_kat`
//! test.

use super::*;
use crate::units::{UNITS, convert};

#[test]
fn the_table_is_in_declaration_order_with_unique_ids() {
    assert_eq!(FUELS.len(), Fuel::ALL.len());
    for (i, (info, fuel)) in FUELS.iter().zip(Fuel::ALL).enumerate() {
        assert_eq!(info.fuel, fuel);
        assert_eq!(fuel.info(), info);
        assert_eq!(fuel.id(), info.id);
        assert_eq!(fuel.to_string(), info.id);
        assert_eq!(Fuel::by_id(info.id), Some(fuel));
        assert!(
            info.id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
            "{}",
            info.id
        );
        assert!(FUELS[..i].iter().all(|o| o.id != info.id), "{}", info.id);
        assert!(!info.name.is_empty());
        // Not a unit id: a fuel and a unit are never confused by name.
        assert!(UNITS.iter().all(|u| u.id != info.id), "{}", info.id);
    }
    assert_eq!(Fuel::by_id("E85"), None);
    assert_eq!(Fuel::by_id("Gasoline"), None);
}

#[test]
fn stoichiometric_ratios_and_their_basis() {
    let table: Vec<(Fuel, f64, StoichBasis, f64)> = FUELS
        .iter()
        .map(|f| (f.fuel, f.stoich_afr, f.basis, f.legacy_afr))
        .collect();
    assert_eq!(
        table,
        [
            (Fuel::Gasoline, 14.7, StoichBasis::Legacy, 14.7),
            (Fuel::Ethanol, 9.0, StoichBasis::Reference, 9.0),
            (Fuel::Methanol, 6.47, StoichBasis::Reference, 6.4),
            (Fuel::Diesel, 14.6, StoichBasis::Legacy, 14.6),
            (Fuel::Lpg, 15.5, StoichBasis::Legacy, 15.5),
            (Fuel::Cng, 17.2, StoichBasis::Legacy, 17.2),
            (Fuel::Hydrogen, 34.3, StoichBasis::Reference, 34.0),
        ]
    );
    for f in FUELS {
        assert!(f.stoich_afr.is_finite() && f.stoich_afr > 0.0, "{}", f.id);
        // A legacy-basis value is legacy's.
        if f.basis == StoichBasis::Legacy {
            assert_eq!(f.stoich_afr, f.legacy_afr, "{}", f.id);
        }
    }
}

#[test]
fn reference_values_agree_with_the_fuels_chemistry() {
    // AFRs = n(O2) × 4.773 × 28.962 / M(fuel) (Heywood §3.3), atomic
    // masses C 12.011, H 1.008, O 15.999.
    let air_per_o2 = 4.773 * 28.962;
    for (fuel, o2, molar_mass) in [
        (Fuel::Methanol, 1.5, 12.011 + 4.0 * 1.008 + 15.999),
        (Fuel::Ethanol, 3.0, 2.0 * 12.011 + 6.0 * 1.008 + 15.999),
        (Fuel::Hydrogen, 0.5, 2.0 * 1.008),
    ] {
        let chemistry = o2 * air_per_o2 / molar_mass;
        assert_eq!(fuel.info().basis, StoichBasis::Reference);
        // Heywood's values are to three significant figures.
        assert!(
            (chemistry - fuel.stoich_afr()).abs() < 0.005 * chemistry,
            "{fuel}: chemistry {chemistry}, table {}",
            fuel.stoich_afr()
        );
    }
}

#[test]
fn lambda_one_is_the_stoichiometric_ratio() {
    for fuel in Fuel::ALL {
        let s = fuel.stoich_afr();
        assert_eq!(afr_to_lambda(s, fuel), Ok(1.0), "{fuel}");
        assert_eq!(lambda_to_afr(1.0, fuel), Ok(s), "{fuel}");
    }
}

#[test]
fn known_mixtures() {
    // 12.5 AFR on gasoline: 12.5 / 14.7.
    assert_eq!(afr_to_lambda(12.5, Fuel::Gasoline), Ok(12.5 / 14.7));
    // λ 0.8 on gasoline: 0.8 × 14.7 = 11.76.
    let afr = lambda_to_afr(0.8, Fuel::Gasoline).unwrap();
    assert!((afr - 11.76).abs() < 1e-12, "{afr}");
    // The same λ is a different AFR per fuel.
    assert_eq!(lambda_to_afr(0.5, Fuel::Hydrogen), Ok(17.15));
    assert_eq!(lambda_to_afr(2.0, Fuel::Ethanol), Ok(18.0));
    // One gasoline AFR as a methanol one, through lambda.
    let lambda = afr_to_lambda(14.7, Fuel::Gasoline).unwrap();
    assert_eq!(lambda_to_afr(lambda, Fuel::Methanol), Ok(6.47));
}

#[test]
fn round_trips_per_fuel() {
    for fuel in Fuel::ALL {
        for afr in [
            0.1, 1.0, 5.5, 9.0, 11.76, 14.7, 17.23, 20.0, 34.3, 100.0, 1e6,
        ] {
            let lambda = afr_to_lambda(afr, fuel).unwrap();
            let back = lambda_to_afr(lambda, fuel).unwrap();
            assert!(
                (back - afr).abs() <= 2.0 * f64::EPSILON * afr,
                "{fuel}: {afr} -> {lambda} -> {back}"
            );
        }
        for lambda in [0.01, 0.5, 0.75, 0.85, 1.0, 1.1, 1.5, 3.0, 1e3] {
            let afr = lambda_to_afr(lambda, fuel).unwrap();
            let back = afr_to_lambda(afr, fuel).unwrap();
            assert!(
                (back - lambda).abs() <= 2.0 * f64::EPSILON * lambda,
                "{fuel}: {lambda} -> {afr} -> {back}"
            );
        }
    }
}

#[test]
fn non_positive_and_non_finite_ratios_are_refused() {
    for fuel in Fuel::ALL {
        for value in [0.0, -0.0, -1.0, -14.7, f64::MIN] {
            assert_eq!(
                afr_to_lambda(value, fuel),
                Err(ConvertError::NotPositive {
                    value,
                    from: "afr",
                    to: "lambda"
                }),
                "{fuel} {value}"
            );
            assert_eq!(
                lambda_to_afr(value, fuel),
                Err(ConvertError::NotPositive {
                    value,
                    from: "lambda",
                    to: "afr"
                }),
                "{fuel} {value}"
            );
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(
                matches!(
                    afr_to_lambda(value, fuel),
                    Err(ConvertError::NonFinite { from: "afr", .. })
                ),
                "{fuel} {value}"
            );
            assert!(
                matches!(
                    lambda_to_afr(value, fuel),
                    Err(ConvertError::NonFinite { from: "lambda", .. })
                ),
                "{fuel} {value}"
            );
        }
    }
    // A finite lambda whose AFR is not.
    assert_eq!(
        lambda_to_afr(f64::MAX, Fuel::Gasoline),
        Err(ConvertError::NonFinite {
            value: f64::MAX,
            from: "lambda",
            to: "afr"
        })
    );
    // The smallest positive AFR is a (tiny) lambda, not zero's refusal.
    assert!(afr_to_lambda(f64::MIN_POSITIVE, Fuel::Hydrogen).is_ok_and(|l| l >= 0.0));
    assert_eq!(
        ConvertError::NotPositive {
            value: 0.0,
            from: "afr",
            to: "lambda"
        }
        .to_string(),
        "0 afr has no value in lambda: an air/fuel ratio or lambda is positive"
    );
}

#[test]
fn afr_and_lambda_do_not_convert_without_a_fuel() {
    assert!(matches!(
        convert(14.7, &AIR_FUEL_RATIO, &LAMBDA),
        Err(ConvertError::DimensionMismatch { .. })
    ));
}
