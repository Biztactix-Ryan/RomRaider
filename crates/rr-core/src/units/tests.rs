//! Unit tests for the unit registry (US-RR-21-4): the registry's shape,
//! lookup, known conversions and refusals. Every expected value is the
//! unit's definition worked by hand (the working is beside it); the
//! known-answer fixtures with cited sources are
//! `fixtures/kat/units-defined-factors.toml`, run by rr-testkit's
//! `units_kat` test.

use super::*;

/// `a` and `b` agree within `ulps` units in the last place of the larger.
fn close(a: f64, b: f64, ulps: f64) -> bool {
    (a - b).abs() <= ulps * f64::EPSILON * a.abs().max(b.abs())
}

fn conv(value: f64, from: &Unit, to: &Unit) -> f64 {
    convert(value, from, to).unwrap()
}

#[track_caller]
fn assert_close(got: f64, want: f64) {
    assert!(close(got, want, 4.0), "got {got:?}, want {want:?}");
}

// ---------------------------------------------------------------------------
// The registry
// ---------------------------------------------------------------------------

#[test]
fn ids_are_unique_lower_case_ascii() {
    for (i, u) in UNITS.iter().enumerate() {
        assert!(!u.id.is_empty(), "unit {i} has no id");
        assert!(
            u.id.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
            "{:?} is not lower-case ASCII, digits and `_`",
            u.id
        );
        assert!(
            UNITS[..i].iter().all(|other| other.id != u.id),
            "{:?} is in the registry twice",
            u.id
        );
    }
}

#[test]
fn every_unit_is_well_formed() {
    for u in UNITS {
        let t = u.to_base;
        assert!(
            t.offset.is_finite() && t.mul.is_finite() && t.div.is_finite(),
            "{}: {t:?}",
            u.id
        );
        assert!(t.mul > 0.0 && t.div > 0.0, "{}: {t:?}", u.id);
        let d = u.display;
        assert!(
            d.min.is_finite() && d.max.is_finite() && d.min < d.max,
            "{}: {d:?}",
            u.id
        );
        assert!(
            d.step.is_finite() && d.step > 0.0 && d.step <= d.max - d.min,
            "{}: {d:?}",
            u.id
        );
        assert!(!u.name.is_empty(), "{}", u.id);
        assert_eq!(u.symbol.is_empty(), u.id == "ratio", "{}", u.id);
        assert!(u.precision <= 6, "{}", u.id);
    }
}

#[test]
fn each_dimension_has_one_base_unit_listed_first() {
    for dim in Dimension::ALL {
        let units: Vec<&Unit> = dim.units().collect();
        let base = dim.base();
        assert_eq!(base.dimension, dim, "{dim}");
        assert_eq!(base.to_base, ToBase::IDENTITY, "{dim}");
        assert_eq!(units.first().map(|u| u.id), Some(base.id), "{dim}");
        let identities = units
            .iter()
            .filter(|u| u.to_base == ToBase::IDENTITY)
            .count();
        assert_eq!(identities, 1, "{dim} has {identities} identity units");
        assert_eq!(by_id(base.id), Some(base), "{dim}");
    }
    // Every unit's dimension is in ALL, so the per-dimension lists cover
    // the registry.
    let covered: usize = Dimension::ALL.iter().map(|d| d.units().count()).sum();
    assert_eq!(covered, UNITS.len());
}

#[test]
fn dimension_names_are_distinct() {
    for (i, d) in Dimension::ALL.iter().enumerate() {
        assert!(!d.name().is_empty());
        assert_eq!(d.to_string(), d.name());
        assert!(
            Dimension::ALL[..i]
                .iter()
                .all(|e| e.name() != d.name() && e != d)
        );
    }
    assert_eq!(Dimension::Temperature.name(), "temperature");
    assert_eq!(Dimension::GaugePressure.to_string(), "gauge pressure");
}

#[test]
fn units_of_a_dimension() {
    let ids: Vec<&str> = Dimension::Temperature.units().map(|u| u.id).collect();
    assert_eq!(ids, ["celsius", "fahrenheit", "kelvin"]);
    let ids: Vec<&str> = Dimension::Speed.units().map(|u| u.id).collect();
    assert_eq!(ids, ["kmh", "mph", "mps"]);
}

#[test]
fn lookup_by_id() {
    for u in UNITS {
        assert_eq!(by_id(u.id), Some(u));
    }
    assert_eq!(by_id("psi"), Some(&PSI));
    assert_eq!(by_id("psi_gauge"), Some(&PSI_GAUGE));
    // Ids are exact: legacy's unit strings are mapped by US-RR-21-5.
    assert_eq!(by_id("PSI"), None);
    assert_eq!(by_id("km/h"), None);
    assert_eq!(by_id(""), None);
}

#[test]
fn legacy_display_ranges() {
    // ConverterMaxMinDefaults.java:35-41.
    assert_eq!(
        FAHRENHEIT.display,
        DisplayRange {
            min: 0.0,
            max: 400.0,
            step: 40.0
        }
    );
    assert_eq!(
        PSI_GAUGE.display,
        DisplayRange {
            min: -20.0,
            max: 40.0,
            step: 5.0
        }
    );
    assert_eq!(
        REV_PER_MINUTE.display,
        DisplayRange {
            min: 0.0,
            max: 8000.0,
            step: 1000.0
        }
    );
}

#[test]
fn display_ranges_go_below_zero_only_where_the_quantity_does() {
    // Temperatures below freezing, boost gauges' vacuum, timing retard and
    // deceleration; never an absolute quantity.
    let below: Vec<(&str, f64)> = UNITS
        .iter()
        .filter(|u| u.display.min < 0.0)
        .map(|u| (u.id, u.display.min))
        .collect();
    assert_eq!(
        below,
        [
            ("celsius", -20.0),
            ("kpa_gauge", -100.0),
            ("bar_gauge", -1.5),
            ("psi_gauge", -20.0),
            ("mmhg_gauge", -800.0),
            ("inhg_gauge", -30.0),
            ("deg", -15.0),
            ("mps2", -10.0),
            ("gn", -1.0),
        ]
    );
}

// ---------------------------------------------------------------------------
// ToBase
// ---------------------------------------------------------------------------

#[test]
fn to_base_applies_offset_then_factor() {
    // (212 + -32) * 5 / 9 = 180 * 5 / 9 = 100.
    assert_eq!(FAHRENHEIT.to_base.to_base(212.0), 100.0);
    // 100 * 9 / 5 - -32 = 180 + 32 = 212.
    assert_eq!(FAHRENHEIT.to_base.from_base(100.0), 212.0);
    // (373.15 - 273.15) * 1 / 1 = 100.
    assert_eq!(KELVIN.to_base.to_base(373.15), 100.0);
    assert_eq!(KELVIN.to_base.from_base(0.0), 273.15);
    // 3 * 3600 / 1000 = 10.8.
    assert_eq!(METRE_PER_SECOND.to_base.to_base(3.0), 10.8);
    // 10.8 * 1000 / 3600 = 3.
    assert_eq!(METRE_PER_SECOND.to_base.from_base(10.8), 3.0);
    assert_eq!(ToBase::IDENTITY.to_base(1.25), 1.25);
    assert_eq!(ToBase::IDENTITY.from_base(1.25), 1.25);
    assert_eq!(
        ToBase::ratio(2.0, 3.0),
        ToBase {
            offset: 0.0,
            mul: 2.0,
            div: 3.0
        }
    );
}

// ---------------------------------------------------------------------------
// Known conversions
// ---------------------------------------------------------------------------

#[test]
fn a_mile_is_1_609344_km_exactly() {
    // 1 mi = 1609.344 m; 1609.344 / 1000 = 1.609344 km.
    assert_eq!(conv(1.0, &MILE, &KILOMETRE), 1.609_344);
    // MF-42: 1 mph = 1.609344 km/h, the factor legacy named KPH_2_MPH.
    assert_eq!(conv(1.0, &MILE_PER_HOUR, &KILOMETRE_PER_HOUR), 1.609_344);
    assert_eq!(conv(1.609_344, &KILOMETRE_PER_HOUR, &MILE_PER_HOUR), 1.0);
    // 100 km/h = 100 / 1.609344 mph = 62.13711922373339...
    assert_close(
        conv(100.0, &KILOMETRE_PER_HOUR, &MILE_PER_HOUR),
        62.137_119_223_733_39,
    );
    // 1 ft = 0.3048 m; 1 in = 0.0254 m.
    assert_eq!(conv(1.0, &FOOT, &METRE), 0.3048);
    assert_eq!(conv(1.0, &INCH, &METRE), 0.0254);
    // 36 km/h = 36 * 1000 / 3600 m/s = 10 m/s.
    assert_eq!(conv(36.0, &KILOMETRE_PER_HOUR, &METRE_PER_SECOND), 10.0);
}

#[test]
fn temperatures_are_affine() {
    // 0 °C = 32 °F = 273.15 K.
    assert_eq!(conv(0.0, &CELSIUS, &FAHRENHEIT), 32.0);
    assert_eq!(conv(0.0, &CELSIUS, &KELVIN), 273.15);
    assert_eq!(conv(32.0, &FAHRENHEIT, &CELSIUS), 0.0);
    assert_eq!(conv(32.0, &FAHRENHEIT, &KELVIN), 273.15);
    assert_eq!(conv(273.15, &KELVIN, &CELSIUS), 0.0);
    // 100 °C = 212 °F = 373.15 K.
    assert_eq!(conv(100.0, &CELSIUS, &FAHRENHEIT), 212.0);
    assert_eq!(conv(212.0, &FAHRENHEIT, &CELSIUS), 100.0);
    assert_eq!(conv(100.0, &CELSIUS, &KELVIN), 373.15);
    // -40 °C = -40 °F.
    assert_eq!(conv(-40.0, &CELSIUS, &FAHRENHEIT), -40.0);
    assert_eq!(conv(-40.0, &FAHRENHEIT, &CELSIUS), -40.0);
    // 0 K = -273.15 °C = -273.15 * 9/5 + 32 = -491.67 + 32 = -459.67 °F.
    assert_eq!(conv(0.0, &KELVIN, &CELSIUS), -273.15);
    assert_close(conv(0.0, &KELVIN, &FAHRENHEIT), -459.67);
    // 50 °F = (50 - 32) * 5/9 = 10 °C = 283.15 K.
    assert_close(conv(50.0, &FAHRENHEIT, &KELVIN), 283.15);
}

#[test]
fn pressures() {
    // 1 psi = 4.4482216152605 N / 0.00064516 m² = 6894.757293168361... Pa.
    assert_eq!(conv(1.0, &PSI, &KILOPASCAL), 6.894_757_293_168_361);
    assert_close(conv(1.0, &PSI, &PASCAL), 6_894.757_293_168_361);
    assert_eq!(
        conv(1.0, &PSI_GAUGE, &KILOPASCAL_GAUGE),
        6.894_757_293_168_361
    );
    // 1 bar = 100 kPa = 100 / 6.894757293168361 psi = 14.503773773020923...
    assert_eq!(conv(1.0, &BAR, &KILOPASCAL), 100.0);
    assert_close(conv(1.0, &BAR, &PSI), 14.503_773_773_020_923);
    assert_close(conv(-1.0, &BAR_GAUGE, &PSI_GAUGE), -14.503_773_773_020_923);
    // 1013.25 hPa = 101.325 kPa = 1.01325 bar.
    assert_close(conv(1013.25, &HECTOPASCAL, &BAR), 1.013_25);
    assert_eq!(conv(0.25, &MEGAPASCAL, &KILOPASCAL), 250.0);
    // 760 mmHg = 760 * 0.133322387415 kPa = 101.3250144354 kPa.
    assert_close(
        conv(760.0, &MILLIMETRE_OF_MERCURY, &KILOPASCAL),
        101.325_014_435_4,
    );
    // 1 inHg = 25.4 mmHg (both conventional, 0.0254 m vs 0.001 m).
    assert_close(conv(1.0, &INCH_OF_MERCURY, &MILLIMETRE_OF_MERCURY), 25.4);
    assert_close(
        conv(1.0, &INCH_OF_MERCURY_GAUGE, &MILLIMETRE_OF_MERCURY_GAUGE),
        25.4,
    );
}

#[test]
fn mass_flow_volume_time_and_ratio() {
    assert_eq!(conv(1.0, &POUND, &KILOGRAM), 0.453_592_37);
    assert_eq!(conv(1000.0, &GRAM, &KILOGRAM), 1.0);
    // 1 lb/min = 453.59237 g / 60 s = 7.559872833333333 g/s.
    assert_eq!(
        conv(1.0, &POUND_PER_MINUTE, &GRAM_PER_SECOND),
        7.559_872_833_333_333
    );
    // 36 kg/h = 36 * 1000 / 3600 g/s = 10 g/s.
    assert_eq!(conv(36.0, &KILOGRAM_PER_HOUR, &GRAM_PER_SECOND), 10.0);
    assert_eq!(conv(1.0, &US_GALLON, &LITRE), 3.785_411_784);
    assert_eq!(conv(1.0, &IMPERIAL_GALLON, &LITRE), 4.546_09);
    assert_eq!(conv(500.0, &MILLILITRE, &LITRE), 0.5);
    // 1 L/min = 60 L/h.
    assert_eq!(conv(1.0, &LITRE_PER_MINUTE, &LITRE_PER_HOUR), 60.0);
    assert_eq!(conv(1.0, &LITRE_PER_MINUTE, &CC_PER_MINUTE), 1000.0);
    // 1000 cc/min = 1000 * 60 / 1000 L/h = 60 L/h.
    assert_eq!(conv(1000.0, &CC_PER_MINUTE, &LITRE_PER_HOUR), 60.0);
    assert_eq!(
        conv(1.0, &US_GALLON_PER_HOUR, &LITRE_PER_HOUR),
        3.785_411_784
    );
    assert_eq!(
        conv(1.0, &IMPERIAL_GALLON_PER_HOUR, &LITRE_PER_HOUR),
        4.546_09
    );
    assert_eq!(conv(2.5, &MILLISECOND, &MICROSECOND), 2500.0);
    assert_eq!(conv(1.5, &HOUR, &MINUTE), 90.0);
    assert_eq!(conv(1.0, &HOUR, &SECOND), 3600.0);
    assert_eq!(conv(1500.0, &MILLIVOLT, &VOLT), 1.5);
    assert_eq!(conv(250.0, &MILLIAMPERE, &AMPERE), 0.25);
    assert_eq!(
        conv(250.0, &MILLIGRAM_PER_CYLINDER, &GRAM_PER_CYLINDER),
        0.25
    );
    assert_eq!(conv(50.0, &PERCENT, &RATIO), 0.5);
    assert_eq!(conv(0.25, &RATIO, &PERCENT), 25.0);
    assert_eq!(conv(1500.0, &MILLIMETRE, &METRE), 1.5);
}

#[test]
fn dyno_units() {
    // 1 hp = 550 * 0.3048 * 4.4482216152605 W = 745.69987158227022 W.
    assert_eq!(conv(1.0, &HORSEPOWER, &KILOWATT), 0.745_699_871_582_270_2);
    assert_close(conv(1.0, &HORSEPOWER, &WATT), 745.699_871_582_270_2);
    // 1 PS = 75 * 9.80665 W = 735.49875 W.
    assert_eq!(conv(1.0, &METRIC_HORSEPOWER, &KILOWATT), 0.735_498_75);
    // 1 lbf·ft = 4.4482216152605 * 0.3048 N·m = 1.3558179483314004 N·m.
    assert_eq!(
        conv(1.0, &POUND_FOOT, &NEWTON_METRE),
        1.355_817_948_331_400_4
    );
    assert_eq!(
        conv(1.0, &STANDARD_GRAVITY, &METRE_PER_SECOND_SQUARED),
        9.806_65
    );
}

#[test]
fn single_unit_dimensions_convert_to_themselves() {
    for u in [
        &DEGREE,
        &REV_PER_MINUTE,
        &OHM,
        &AIR_FUEL_RATIO,
        &LAMBDA,
        &GRAM_PER_REV,
    ] {
        assert_eq!(conv(14.7, u, u), 14.7, "{}", u.id);
    }
}

#[test]
fn the_same_unit_returns_the_value_unchanged() {
    // Through the base, 0.1 °F would come back as 0.10000000000000142.
    assert_eq!(
        conv(0.1, &FAHRENHEIT, &FAHRENHEIT).to_bits(),
        0.1_f64.to_bits()
    );
    assert_eq!(
        conv(-0.0, &FAHRENHEIT, &FAHRENHEIT).to_bits(),
        (-0.0_f64).to_bits()
    );
    assert_eq!(conv(1.1, &FAHRENHEIT, &FAHRENHEIT), 1.1);
}

#[test]
fn round_trips_within_a_dimension() {
    for from in UNITS {
        for to in from.dimension.units() {
            let d = from.display;
            for value in [d.min, d.max, (d.min + d.max) / 3.0, 0.1, -7.3, 12_345.678] {
                let there = conv(value, from, to);
                let back = conv(there, to, from);
                let scale = value
                    .abs()
                    .max(from.to_base.offset.abs())
                    .max(to.to_base.offset.abs())
                    .max(1.0);
                assert!(
                    (back - value).abs() <= 8.0 * f64::EPSILON * scale,
                    "{value} {} -> {there} {} -> {back}",
                    from.id,
                    to.id
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

#[test]
fn units_of_different_dimensions_do_not_convert() {
    let e = convert(1.0, &PSI, &CELSIUS).unwrap_err();
    assert_eq!(
        e,
        ConvertError::DimensionMismatch {
            from: "psi",
            from_dimension: Dimension::Pressure,
            to: "celsius",
            to_dimension: Dimension::Temperature,
        }
    );
    assert_eq!(
        e.to_string(),
        "cannot convert psi (absolute pressure) to celsius (temperature): units convert \
         only within one dimension"
    );
    // Absolute and gauge pressure are different dimensions.
    assert!(matches!(
        convert(14.7, &PSI, &PSI_GAUGE),
        Err(ConvertError::DimensionMismatch { .. })
    ));
    // AFR and lambda need a fuel (`afr_to_lambda`).
    assert!(matches!(
        convert(14.7, &AIR_FUEL_RATIO, &LAMBDA),
        Err(ConvertError::DimensionMismatch { .. })
    ));
    // Checked before the value, even a NaN.
    assert!(matches!(
        convert(f64::NAN, &PSI, &CELSIUS),
        Err(ConvertError::DimensionMismatch { .. })
    ));
}

#[test]
fn a_value_or_result_that_is_not_finite_is_refused() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for (from, to) in [(&CELSIUS, &FAHRENHEIT), (&CELSIUS, &CELSIUS)] {
            let e = convert(value, from, to).unwrap_err();
            assert!(
                matches!(e, ConvertError::NonFinite { from: f, to: t, value: v }
                    if f == from.id && t == to.id && v.to_bits() == value.to_bits()),
                "{e:?}"
            );
        }
    }
    // f64::MAX hours is beyond f64 in seconds.
    let e = convert(f64::MAX, &HOUR, &SECOND).unwrap_err();
    assert_eq!(
        e,
        ConvertError::NonFinite {
            value: f64::MAX,
            from: "h",
            to: "s"
        }
    );
    assert_eq!(
        e.to_string(),
        format!(
            "{} h has no finite value in s: a converted value must be a finite number",
            f64::MAX
        )
    );
    // The other way it is merely small.
    assert_eq!(conv(f64::MAX, &SECOND, &HOUR), f64::MAX / 3600.0);
}

#[test]
fn convert_by_id_names_units_by_id() {
    assert_eq!(convert_by_id(0.0, "celsius", "fahrenheit"), Ok(32.0));
    assert_eq!(convert_by_id(1.0, "mph", "kmh"), Ok(1.609_344));
    assert_eq!(
        convert_by_id(1.0, "furlong", "kmh"),
        Err(ConvertError::UnknownUnit("furlong".to_owned()))
    );
    assert_eq!(
        convert_by_id(1.0, "mph", "knot"),
        Err(ConvertError::UnknownUnit("knot".to_owned()))
    );
    // The `from` id is reported first.
    assert_eq!(
        convert_by_id(1.0, "a?", "b?"),
        Err(ConvertError::UnknownUnit("a?".to_owned()))
    );
    assert_eq!(
        ConvertError::UnknownUnit("km/h".to_owned()).to_string(),
        "no unit has the id \"km/h\""
    );
    assert!(matches!(
        convert_by_id(1.0, "mph", "celsius"),
        Err(ConvertError::DimensionMismatch { .. })
    ));
}
