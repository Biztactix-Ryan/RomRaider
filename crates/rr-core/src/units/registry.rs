//! The unit registry: every [`Unit`] and [`UNITS`], the list of them.
//!
//! Each unit's conversion cites its definition. "SP 811" is NIST Special
//! Publication 811, *Guide for the Use of the International System of
//! Units (SI)*, 2008 edition, Appendix B.8 (factors listed
//! alphabetically; a factor in bold there is exact); "SI Brochure" is the
//! BIPM SI Brochure, 9th edition (2019): Table 7 (the SI prefixes) and
//! Table 8 (non-SI units accepted for use with the SI). The units of the
//! international yard and pound (1959) are exact: 1 in = 0.0254 m,
//! 1 ft = 0.3048 m, 1 mi = 1609.344 m, 1 lb = 0.45359237 kg (SP 811). The
//! standard acceleration of free fall is exactly 9.80665 m/s² (SP 811;
//! 3rd CGPM, 1901), so one pound-force is exactly
//! 0.45359237 × 9.80665 = 4.4482216152605 N.
//!
//! A display range taken from legacy cites
//! `logger/ecu/definition/xml/ConverterMaxMinDefaults.java` (`CMMD`) by
//! line; the others are chosen to cover the same quantity in the unit.

use super::{Dimension, DisplayRange, ToBase, Unit};

/// Builds a registry entry; `display` is `(min, max, step)`.
const fn unit(
    id: &'static str,
    symbol: &'static str,
    name: &'static str,
    dimension: Dimension,
    to_base: ToBase,
    display: (f64, f64, f64),
    precision: u8,
) -> Unit {
    Unit {
        id,
        symbol,
        name,
        dimension,
        to_base,
        display: DisplayRange {
            min: display.0,
            max: display.1,
            step: display.2,
        },
        precision,
    }
}

/// `value * mul`: a unit that is `mul` base units.
const fn times(mul: f64) -> ToBase {
    ToBase::ratio(mul, 1.0)
}

/// `value / div`: a unit that is one `div`th of a base unit.
const fn per(div: f64) -> ToBase {
    ToBase::ratio(1.0, div)
}

// ---------------------------------------------------------------------------
// Temperature (base: degree Celsius)
// ---------------------------------------------------------------------------

/// Degree Celsius, the temperature base unit. Display `-20..200` (`CMMD:36`).
pub const CELSIUS: Unit = unit(
    "celsius",
    "°C",
    "degree Celsius",
    Dimension::Temperature,
    ToBase::IDENTITY,
    (-20.0, 200.0, 20.0),
    1,
);

/// Degree Fahrenheit: t/°C = (t/°F − 32) × 5/9 (SP 811, "degree
/// Fahrenheit": t/°C = (t/°F − 32)/1.8). Display `0..400` (`CMMD:35`).
pub const FAHRENHEIT: Unit = unit(
    "fahrenheit",
    "°F",
    "degree Fahrenheit",
    Dimension::Temperature,
    ToBase {
        offset: -32.0,
        mul: 5.0,
        div: 9.0,
    },
    (0.0, 400.0, 40.0),
    1,
);

/// Kelvin: t/°C = T/K − 273.15 (SP 811 §4.2.1.1).
pub const KELVIN: Unit = unit(
    "kelvin",
    "K",
    "kelvin",
    Dimension::Temperature,
    ToBase {
        offset: -273.15,
        mul: 1.0,
        div: 1.0,
    },
    (250.0, 475.0, 25.0),
    1,
);

// ---------------------------------------------------------------------------
// Absolute pressure (base: kilopascal)
// ---------------------------------------------------------------------------

/// Kilopascal (absolute), the pressure base unit.
pub const KILOPASCAL: Unit = unit(
    "kpa",
    "kPa",
    "kilopascal",
    Dimension::Pressure,
    ToBase::IDENTITY,
    (0.0, 300.0, 25.0),
    1,
);

/// Pascal: 1 Pa = 1/1000 kPa (SI Brochure Table 7, kilo = 10³).
pub const PASCAL: Unit = unit(
    "pa",
    "Pa",
    "pascal",
    Dimension::Pressure,
    per(1000.0),
    (0.0, 300_000.0, 25_000.0),
    0,
);

/// Hectopascal: 1 hPa = 1/10 kPa (SI Brochure Table 7, hecto = 10²).
pub const HECTOPASCAL: Unit = unit(
    "hpa",
    "hPa",
    "hectopascal",
    Dimension::Pressure,
    per(10.0),
    (0.0, 3000.0, 250.0),
    0,
);

/// Megapascal: 1 MPa = 1000 kPa (SI Brochure Table 7, mega = 10⁶).
/// Display `0..0.5` (`CMMD:54`).
pub const MEGAPASCAL: Unit = unit(
    "mpa",
    "MPa",
    "megapascal",
    Dimension::Pressure,
    times(1000.0),
    (0.0, 0.5, 0.1),
    3,
);

/// Bar: 1 bar = 100 kPa exactly (SP 811, "bar").
pub const BAR: Unit = unit(
    "bar",
    "bar",
    "bar",
    Dimension::Pressure,
    times(100.0),
    (0.0, 3.0, 0.5),
    2,
);

/// Pound-force per square inch (absolute): 1 psi = 1 lbf / 1 in²
/// = 4.4482216152605 N / 0.00064516 m² = 4.4482216152605 / 0.64516 kPa
/// (SP 811, "pound-force per square inch", 6.894 757 E+03 Pa, here
/// unrounded).
pub const PSI: Unit = unit(
    "psi",
    "psi",
    "pound-force per square inch",
    Dimension::Pressure,
    ToBase::ratio(4.448_221_615_260_5, 0.645_16),
    (0.0, 45.0, 5.0),
    2,
);

/// Conventional millimetre of mercury: the pressure of 1 mm of mercury of
/// conventional density 13 595.1 kg/m³ under standard gravity,
/// 13 595.1 × 9.80665 × 0.001 = 133.322387415 Pa (SP 811, "millimeter of
/// mercury, conventional", 1.333 224 E+02 Pa). Display `0..2000`
/// (`CMMD:61`).
pub const MILLIMETRE_OF_MERCURY: Unit = unit(
    "mmhg",
    "mmHg",
    "millimetre of mercury",
    Dimension::Pressure,
    times(0.133_322_387_415),
    (0.0, 2000.0, 100.0),
    0,
);

/// Conventional inch of mercury: 13 595.1 × 9.80665 × 0.0254
/// = 3386.388640341 Pa (SP 811, "inch of mercury, conventional",
/// 3.386 389 E+03 Pa).
pub const INCH_OF_MERCURY: Unit = unit(
    "inhg",
    "inHg",
    "inch of mercury",
    Dimension::Pressure,
    times(3.386_388_640_341),
    (0.0, 80.0, 10.0),
    2,
);

// ---------------------------------------------------------------------------
// Gauge pressure (base: kilopascal, gauge)
// ---------------------------------------------------------------------------

/// Kilopascal (gauge), the gauge-pressure base unit.
pub const KILOPASCAL_GAUGE: Unit = unit(
    "kpa_gauge",
    "kPa",
    "kilopascal (gauge)",
    Dimension::GaugePressure,
    ToBase::IDENTITY,
    (-100.0, 300.0, 50.0),
    1,
);

/// Bar (gauge): as [`BAR`]. Display `-1.5..3` (`CMMD:38`).
pub const BAR_GAUGE: Unit = unit(
    "bar_gauge",
    "bar",
    "bar (gauge)",
    Dimension::GaugePressure,
    times(100.0),
    (-1.5, 3.0, 0.5),
    2,
);

/// Pound-force per square inch (gauge): as [`PSI`]. Display `-20..40`
/// (`CMMD:37`).
pub const PSI_GAUGE: Unit = unit(
    "psi_gauge",
    "psi",
    "pound-force per square inch (gauge)",
    Dimension::GaugePressure,
    ToBase::ratio(4.448_221_615_260_5, 0.645_16),
    (-20.0, 40.0, 5.0),
    2,
);

/// Millimetre of mercury (gauge): as [`MILLIMETRE_OF_MERCURY`].
pub const MILLIMETRE_OF_MERCURY_GAUGE: Unit = unit(
    "mmhg_gauge",
    "mmHg",
    "millimetre of mercury (gauge)",
    Dimension::GaugePressure,
    times(0.133_322_387_415),
    (-800.0, 1600.0, 200.0),
    0,
);

/// Inch of mercury (gauge): as [`INCH_OF_MERCURY`].
pub const INCH_OF_MERCURY_GAUGE: Unit = unit(
    "inhg_gauge",
    "inHg",
    "inch of mercury (gauge)",
    Dimension::GaugePressure,
    times(3.386_388_640_341),
    (-30.0, 60.0, 10.0),
    2,
);

// ---------------------------------------------------------------------------
// Speed (base: kilometre per hour)
// ---------------------------------------------------------------------------

/// Kilometre per hour, the speed base unit. Display `0..300` (`CMMD:41`).
pub const KILOMETRE_PER_HOUR: Unit = unit(
    "kmh",
    "km/h",
    "kilometre per hour",
    Dimension::Speed,
    ToBase::IDENTITY,
    (0.0, 300.0, 20.0),
    0,
);

/// Mile per hour: 1 mph = 1609.344 m/h = 1.609344 km/h (SP 811, "mile").
/// Legacy's `KPH_2_MPH` is this factor under an inverted name (MF-42).
/// Display `0..200` (`CMMD:40`).
pub const MILE_PER_HOUR: Unit = unit(
    "mph",
    "mph",
    "mile per hour",
    Dimension::Speed,
    times(1.609_344),
    (0.0, 200.0, 20.0),
    0,
);

/// Metre per second: 1 m/s = 3600 m/h = 3600/1000 km/h (SI Brochure
/// Table 8, 1 h = 3600 s).
pub const METRE_PER_SECOND: Unit = unit(
    "mps",
    "m/s",
    "metre per second",
    Dimension::Speed,
    ToBase::ratio(3600.0, 1000.0),
    (0.0, 90.0, 10.0),
    1,
);

// ---------------------------------------------------------------------------
// Length (base: metre)
// ---------------------------------------------------------------------------

/// Metre, the length base unit.
pub const METRE: Unit = unit(
    "m",
    "m",
    "metre",
    Dimension::Length,
    ToBase::IDENTITY,
    (0.0, 1000.0, 100.0),
    1,
);

/// Kilometre: 1000 m (SI Brochure Table 7).
pub const KILOMETRE: Unit = unit(
    "km",
    "km",
    "kilometre",
    Dimension::Length,
    times(1000.0),
    (0.0, 1000.0, 100.0),
    1,
);

/// Millimetre: 1/1000 m (SI Brochure Table 7).
pub const MILLIMETRE: Unit = unit(
    "mm",
    "mm",
    "millimetre",
    Dimension::Length,
    per(1000.0),
    (0.0, 1000.0, 100.0),
    1,
);

/// Mile (international): 1609.344 m (SP 811, "mile").
pub const MILE: Unit = unit(
    "mi",
    "mi",
    "mile",
    Dimension::Length,
    times(1609.344),
    (0.0, 600.0, 50.0),
    1,
);

/// Foot (international): 0.3048 m (SP 811, "foot").
pub const FOOT: Unit = unit(
    "ft",
    "ft",
    "foot",
    Dimension::Length,
    times(0.3048),
    (0.0, 3000.0, 250.0),
    0,
);

/// Inch (international): 0.0254 m (SP 811, "inch").
pub const INCH: Unit = unit(
    "in",
    "in",
    "inch",
    Dimension::Length,
    times(0.0254),
    (0.0, 40.0, 5.0),
    2,
);

// ---------------------------------------------------------------------------
// Mass (base: kilogram)
// ---------------------------------------------------------------------------

/// Kilogram, the mass base unit.
pub const KILOGRAM: Unit = unit(
    "kg",
    "kg",
    "kilogram",
    Dimension::Mass,
    ToBase::IDENTITY,
    (0.0, 2500.0, 250.0),
    1,
);

/// Gram: 1/1000 kg (SI Brochure Table 7).
pub const GRAM: Unit = unit(
    "g",
    "g",
    "gram",
    Dimension::Mass,
    per(1000.0),
    (0.0, 1000.0, 100.0),
    1,
);

/// Pound (avoirdupois): 0.45359237 kg (SP 811, "pound (avoirdupois)").
/// Legacy's dyno used `0.4536` (`DynoControlPanel.java:252`).
pub const POUND: Unit = unit(
    "lb",
    "lb",
    "pound",
    Dimension::Mass,
    times(0.453_592_37),
    (0.0, 5000.0, 500.0),
    1,
);

// ---------------------------------------------------------------------------
// Mass flow (base: gram per second)
// ---------------------------------------------------------------------------

/// Gram per second, the mass-flow base unit. Display `0..400` (`CMMD:43`).
pub const GRAM_PER_SECOND: Unit = unit(
    "g_per_s",
    "g/s",
    "gram per second",
    Dimension::MassFlow,
    ToBase::IDENTITY,
    (0.0, 400.0, 20.0),
    2,
);

/// Kilogram per hour: 1000 g / 3600 s (SI Brochure Tables 7 and 8).
pub const KILOGRAM_PER_HOUR: Unit = unit(
    "kg_per_h",
    "kg/h",
    "kilogram per hour",
    Dimension::MassFlow,
    ToBase::ratio(1000.0, 3600.0),
    (0.0, 1500.0, 100.0),
    1,
);

/// Pound per minute: 453.59237 g / 60 s (SP 811, "pound (avoirdupois)";
/// SI Brochure Table 8, 1 min = 60 s).
pub const POUND_PER_MINUTE: Unit = unit(
    "lb_per_min",
    "lb/min",
    "pound per minute",
    Dimension::MassFlow,
    ToBase::ratio(453.592_37, 60.0),
    (0.0, 60.0, 5.0),
    2,
);

// ---------------------------------------------------------------------------
// Volume (base: litre)
// ---------------------------------------------------------------------------

/// Litre, the volume base unit (SI Brochure Table 8, 1 L = 1 dm³).
pub const LITRE: Unit = unit(
    "l",
    "L",
    "litre",
    Dimension::Volume,
    ToBase::IDENTITY,
    (0.0, 100.0, 10.0),
    2,
);

/// Millilitre (cubic centimetre): 1/1000 L (SI Brochure Table 7).
pub const MILLILITRE: Unit = unit(
    "ml",
    "mL",
    "millilitre",
    Dimension::Volume,
    per(1000.0),
    (0.0, 1000.0, 100.0),
    0,
);

/// US gallon: 231 in³ = 231 × 0.0254³ m³ = 3.785411784 L (SP 811,
/// "gallon (U.S.)").
pub const US_GALLON: Unit = unit(
    "gal_us",
    "gal (US)",
    "US gallon",
    Dimension::Volume,
    times(3.785_411_784),
    (0.0, 25.0, 5.0),
    2,
);

/// Imperial gallon: 4.54609 L (SP 811, "gallon [Canadian and U.K.
/// (Imperial)]").
pub const IMPERIAL_GALLON: Unit = unit(
    "gal_uk",
    "gal (UK)",
    "imperial gallon",
    Dimension::Volume,
    times(4.546_09),
    (0.0, 25.0, 5.0),
    2,
);

// ---------------------------------------------------------------------------
// Volume flow (base: litre per hour)
// ---------------------------------------------------------------------------

/// Litre per hour, the volume-flow base unit.
pub const LITRE_PER_HOUR: Unit = unit(
    "l_per_h",
    "L/h",
    "litre per hour",
    Dimension::VolumeFlow,
    ToBase::IDENTITY,
    (0.0, 100.0, 10.0),
    1,
);

/// Litre per minute: 60 L/h (SI Brochure Table 8, 1 h = 60 min).
pub const LITRE_PER_MINUTE: Unit = unit(
    "l_per_min",
    "L/min",
    "litre per minute",
    Dimension::VolumeFlow,
    times(60.0),
    (0.0, 10.0, 1.0),
    2,
);

/// Cubic centimetre per minute (injector flow): 1 cm³/min = 60 cm³/h
/// = 60/1000 L/h (SI Brochure Tables 7 and 8).
pub const CC_PER_MINUTE: Unit = unit(
    "cc_per_min",
    "cc/min",
    "cubic centimetre per minute",
    Dimension::VolumeFlow,
    ToBase::ratio(60.0, 1000.0),
    (0.0, 2000.0, 200.0),
    0,
);

/// US gallon per hour: [`US_GALLON`] per hour.
pub const US_GALLON_PER_HOUR: Unit = unit(
    "gal_us_per_h",
    "gph (US)",
    "US gallon per hour",
    Dimension::VolumeFlow,
    times(3.785_411_784),
    (0.0, 25.0, 5.0),
    2,
);

/// Imperial gallon per hour: [`IMPERIAL_GALLON`] per hour.
pub const IMPERIAL_GALLON_PER_HOUR: Unit = unit(
    "gal_uk_per_h",
    "gph (UK)",
    "imperial gallon per hour",
    Dimension::VolumeFlow,
    times(4.546_09),
    (0.0, 25.0, 5.0),
    2,
);

// ---------------------------------------------------------------------------
// Time (base: second)
// ---------------------------------------------------------------------------

/// Second, the time base unit.
pub const SECOND: Unit = unit(
    "s",
    "s",
    "second",
    Dimension::Time,
    ToBase::IDENTITY,
    (0.0, 60.0, 5.0),
    2,
);

/// Millisecond: 1/1000 s (SI Brochure Table 7). Display `0..100`
/// (`CMMD:45`).
pub const MILLISECOND: Unit = unit(
    "ms",
    "ms",
    "millisecond",
    Dimension::Time,
    per(1000.0),
    (0.0, 100.0, 10.0),
    2,
);

/// Microsecond: 1/1 000 000 s (SI Brochure Table 7).
pub const MICROSECOND: Unit = unit(
    "us",
    "µs",
    "microsecond",
    Dimension::Time,
    per(1_000_000.0),
    (0.0, 10_000.0, 1000.0),
    0,
);

/// Minute: 60 s (SI Brochure Table 8).
pub const MINUTE: Unit = unit(
    "min",
    "min",
    "minute",
    Dimension::Time,
    times(60.0),
    (0.0, 60.0, 5.0),
    1,
);

/// Hour: 3600 s (SI Brochure Table 8).
pub const HOUR: Unit = unit(
    "h",
    "h",
    "hour",
    Dimension::Time,
    times(3600.0),
    (0.0, 24.0, 2.0),
    2,
);

// ---------------------------------------------------------------------------
// Single-unit dimensions
// ---------------------------------------------------------------------------

/// Degree (plane angle), the angle base unit (SI Brochure Table 8).
/// Display `-15..60` (`CMMD:42`, legacy's `degrees`: ignition timing).
pub const DEGREE: Unit = unit(
    "deg",
    "°",
    "degree",
    Dimension::Angle,
    ToBase::IDENTITY,
    (-15.0, 60.0, 5.0),
    1,
);

/// Revolution per minute, the rotational-speed base unit. Display
/// `0..8000` (`CMMD:39`).
pub const REV_PER_MINUTE: Unit = unit(
    "rpm",
    "rpm",
    "revolution per minute",
    Dimension::RotationalSpeed,
    ToBase::IDENTITY,
    (0.0, 8000.0, 1000.0),
    0,
);

/// Ohm, the resistance base unit. Display `0..100` (`CMMD:49`).
pub const OHM: Unit = unit(
    "ohm",
    "Ω",
    "ohm",
    Dimension::Resistance,
    ToBase::IDENTITY,
    (0.0, 100.0, 10.0),
    1,
);

/// Air/fuel ratio by mass. Display `10..20` (`CMMD:50`).
pub const AIR_FUEL_RATIO: Unit = unit(
    "afr",
    "AFR",
    "air/fuel ratio",
    Dimension::AirFuelRatio,
    ToBase::IDENTITY,
    (10.0, 20.0, 1.0),
    2,
);

/// Lambda. Display `0.5..1.5` (`CMMD:51`).
pub const LAMBDA: Unit = unit(
    "lambda",
    "λ",
    "lambda",
    Dimension::Lambda,
    ToBase::IDENTITY,
    (0.5, 1.5, 0.1),
    3,
);

/// Gram per revolution (engine load). Display `0..4` (`CMMD:56`).
pub const GRAM_PER_REV: Unit = unit(
    "g_per_rev",
    "g/rev",
    "gram per revolution",
    Dimension::MassPerRevolution,
    ToBase::IDENTITY,
    (0.0, 4.0, 0.5),
    2,
);

// ---------------------------------------------------------------------------
// Electrical
// ---------------------------------------------------------------------------

/// Volt, the voltage base unit. Display `0..5` (`CMMD:44`).
pub const VOLT: Unit = unit(
    "v",
    "V",
    "volt",
    Dimension::Voltage,
    ToBase::IDENTITY,
    (0.0, 5.0, 0.5),
    2,
);

/// Millivolt: 1/1000 V (SI Brochure Table 7).
pub const MILLIVOLT: Unit = unit(
    "mv",
    "mV",
    "millivolt",
    Dimension::Voltage,
    per(1000.0),
    (0.0, 5000.0, 500.0),
    0,
);

/// Ampere, the current base unit. Display `0..20` (`CMMD:46`).
pub const AMPERE: Unit = unit(
    "a",
    "A",
    "ampere",
    Dimension::Current,
    ToBase::IDENTITY,
    (0.0, 20.0, 5.0),
    2,
);

/// Milliampere: 1/1000 A (SI Brochure Table 7). Display `0..100`
/// (`CMMD:47`).
pub const MILLIAMPERE: Unit = unit(
    "ma",
    "mA",
    "milliampere",
    Dimension::Current,
    per(1000.0),
    (0.0, 100.0, 10.0),
    1,
);

// ---------------------------------------------------------------------------
// Ratio (base: ratio, 1)
// ---------------------------------------------------------------------------

/// A plain ratio or multiplier, the ratio base unit; it has no symbol.
/// Display `0..1` (`CMMD:58`, legacy's `multiplier`).
pub const RATIO: Unit = unit(
    "ratio",
    "",
    "ratio",
    Dimension::Ratio,
    ToBase::IDENTITY,
    (0.0, 1.0, 0.1),
    3,
);

/// Percent: 1/100 (SP 811 §7.10.2). Display `0..100` (`CMMD:34`).
pub const PERCENT: Unit = unit(
    "percent",
    "%",
    "percent",
    Dimension::Ratio,
    per(100.0),
    (0.0, 100.0, 10.0),
    1,
);

// ---------------------------------------------------------------------------
// Torque, power, acceleration (dyno)
// ---------------------------------------------------------------------------

/// Newton metre, the torque base unit.
pub const NEWTON_METRE: Unit = unit(
    "nm",
    "N·m",
    "newton metre",
    Dimension::Torque,
    ToBase::IDENTITY,
    (0.0, 600.0, 50.0),
    1,
);

/// Pound-force foot: 4.4482216152605 N × 0.3048 m = 1.3558179483314004 N·m
/// (SP 811, "pound-force foot", 1.355 818 E+00 N·m, here unrounded).
pub const POUND_FOOT: Unit = unit(
    "lbf_ft",
    "lbf·ft",
    "pound-force foot",
    Dimension::Torque,
    times(1.355_817_948_331_400_4),
    (0.0, 450.0, 50.0),
    1,
);

/// Kilowatt, the power base unit.
pub const KILOWATT: Unit = unit(
    "kw",
    "kW",
    "kilowatt",
    Dimension::Power,
    ToBase::IDENTITY,
    (0.0, 400.0, 50.0),
    1,
);

/// Watt: 1/1000 kW (SI Brochure Table 7).
pub const WATT: Unit = unit(
    "w",
    "W",
    "watt",
    Dimension::Power,
    per(1000.0),
    (0.0, 400_000.0, 50_000.0),
    0,
);

/// Horsepower (mechanical, 550 ft·lbf/s): 550 × 0.3048 × 4.4482216152605 W
/// = 745.69987158227022 W (SP 811, "horsepower (550 ft·lbf/s)",
/// 7.456 999 E+02 W, here unrounded). The literal is the `f64` nearest
/// 0.74569987158227022 kW, which has more digits than an `f64` holds.
/// Legacy's dyno used `745.7` (`DynoControlPanel.java:298`).
pub const HORSEPOWER: Unit = unit(
    "hp",
    "hp",
    "horsepower",
    Dimension::Power,
    times(0.745_699_871_582_270_2),
    (0.0, 500.0, 50.0),
    1,
);

/// Metric horsepower (75 kgf·m/s): 75 × 9.80665 W = 735.49875 W (SP 811,
/// "horsepower, metric", 7.354 988 E+02 W, here unrounded).
pub const METRIC_HORSEPOWER: Unit = unit(
    "ps",
    "PS",
    "metric horsepower",
    Dimension::Power,
    times(0.735_498_75),
    (0.0, 500.0, 50.0),
    1,
);

/// Metre per second squared, the acceleration base unit.
pub const METRE_PER_SECOND_SQUARED: Unit = unit(
    "mps2",
    "m/s²",
    "metre per second squared",
    Dimension::Acceleration,
    ToBase::IDENTITY,
    (-10.0, 10.0, 2.0),
    2,
);

/// Standard acceleration of free fall: 9.80665 m/s² (SP 811, "standard
/// acceleration of free fall").
pub const STANDARD_GRAVITY: Unit = unit(
    "gn",
    "g",
    "standard gravity",
    Dimension::Acceleration,
    times(9.806_65),
    (-1.0, 1.0, 0.2),
    2,
);

// ---------------------------------------------------------------------------
// Mass per cylinder (base: gram per cylinder)
// ---------------------------------------------------------------------------

/// Gram per cylinder filling, the base unit. Display `0..2` (`CMMD:57`).
pub const GRAM_PER_CYLINDER: Unit = unit(
    "g_per_cyl",
    "g/cyl",
    "gram per cylinder",
    Dimension::MassPerCylinder,
    ToBase::IDENTITY,
    (0.0, 2.0, 0.5),
    3,
);

/// Milligram per cylinder filling: 1/1000 g/cyl (SI Brochure Table 7).
pub const MILLIGRAM_PER_CYLINDER: Unit = unit(
    "mg_per_cyl",
    "mg/cyl",
    "milligram per cylinder",
    Dimension::MassPerCylinder,
    per(1000.0),
    (0.0, 2000.0, 250.0),
    0,
);

/// Every registry unit, grouped by dimension, each dimension's base unit
/// first.
pub const UNITS: &[Unit] = &[
    CELSIUS,
    FAHRENHEIT,
    KELVIN,
    KILOPASCAL,
    PASCAL,
    HECTOPASCAL,
    MEGAPASCAL,
    BAR,
    PSI,
    MILLIMETRE_OF_MERCURY,
    INCH_OF_MERCURY,
    KILOPASCAL_GAUGE,
    BAR_GAUGE,
    PSI_GAUGE,
    MILLIMETRE_OF_MERCURY_GAUGE,
    INCH_OF_MERCURY_GAUGE,
    KILOMETRE_PER_HOUR,
    MILE_PER_HOUR,
    METRE_PER_SECOND,
    METRE,
    KILOMETRE,
    MILLIMETRE,
    MILE,
    FOOT,
    INCH,
    KILOGRAM,
    GRAM,
    POUND,
    GRAM_PER_SECOND,
    KILOGRAM_PER_HOUR,
    POUND_PER_MINUTE,
    LITRE,
    MILLILITRE,
    US_GALLON,
    IMPERIAL_GALLON,
    LITRE_PER_HOUR,
    LITRE_PER_MINUTE,
    CC_PER_MINUTE,
    US_GALLON_PER_HOUR,
    IMPERIAL_GALLON_PER_HOUR,
    SECOND,
    MILLISECOND,
    MICROSECOND,
    MINUTE,
    HOUR,
    DEGREE,
    REV_PER_MINUTE,
    VOLT,
    MILLIVOLT,
    AMPERE,
    MILLIAMPERE,
    OHM,
    RATIO,
    PERCENT,
    NEWTON_METRE,
    POUND_FOOT,
    KILOWATT,
    WATT,
    HORSEPOWER,
    METRIC_HORSEPOWER,
    METRE_PER_SECOND_SQUARED,
    STANDARD_GRAVITY,
    AIR_FUEL_RATIO,
    LAMBDA,
    GRAM_PER_REV,
    GRAM_PER_CYLINDER,
    MILLIGRAM_PER_CYLINDER,
];
