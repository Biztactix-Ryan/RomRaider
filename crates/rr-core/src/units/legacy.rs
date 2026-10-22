//! The unit strings of legacy definitions, and which registry unit each
//! names (US-RR-21-5).
//!
//! A RomRaider definition says a value's unit as free text: a logger
//! `<conversion units="psi relative sea level">`, an ECU `<scaling
//! units="Coolant Temp (Degrees F)">`. [`legacy_unit`] reads such a string
//! as a [`LegacyUnit`]: a registry [`Unit`], a pressure that does not say
//! absolute or gauge, the air/fuel ratio of a named fuel, or "no unit"; or
//! fails with [`UnknownUnitString`], reported as a Warning
//! ([`UNKNOWN_UNIT_STRING`](crate::diag::codes::UNKNOWN_UNIT_STRING)).
//!
//! # Matching
//!
//! [`LEGACY_ALIASES`] lists the strings exactly as the pinned definition
//! corpus spells them (every `units` attribute of the pinned ECU and logger
//! definitions and of `logger_v370.zip`, listed in
//! `fixtures/corpus-unit-strings.txt`), and legacy's own (the AFR strings of
//! `logger/external/core/SensorConversionsAFR.java:27-34`, and
//! `ConverterMaxMinDefaults.java:57`'s `g/cyl`). A string is read in three
//! steps:
//!
//! 1. Leading and trailing whitespace is removed (one corpus label ends in a
//!    space). Nothing else is normalised.
//! 2. The string is looked up in [`LEGACY_ALIASES`], exactly and
//!    case-sensitively.
//! 3. Failing that, a label ending in a bracketed unit, `Quantity (unit)`
//!    (most ECU scalings: `Engine Speed (RPM)`, `CoolantTemp(DegreesF)`),
//!    is read as its last bracketed part, trimmed, if that is non-empty and
//!    in [`LEGACY_ALIASES`]. The rest of the label names the quantity, not
//!    the unit, and is not read.
//!
//! Anything else is unknown. In particular case is not folded: legacy's
//! gauge defaults lower-cased the string (`ConverterMaxMinDefaults.java:65`),
//! so `C`, `c` and `Rpm` all matched; here each spelling the corpus uses is
//! an alias of its own (`C`, `°C`, `Degrees C`; `RPM`, `rpm`), and a new
//! spelling is a Warning to look at rather than a silent guess. (Legacy's
//! own keys `MPa` and `mmHg` were never found, being looked up lower-cased:
//! `ConverterMaxMinDefaults.java:54`, `:61`.)
//!
//! # What a string can name
//!
//! * A registry unit ([`LegacyUnit::Unit`]): `km/h`, `Degrees F`,
//!   `psi relative sea level`.
//! * A pressure that does not say absolute or gauge
//!   ([`LegacyUnit::UnstatedPressure`]): plain `psi`, `kPa`, `bar`. The
//!   pinned logger definitions use plain `bar` both for atmospheric pressure
//!   (absolute) and for manifold relative pressure (gauge), so the string
//!   cannot say which; the parameter does. The absolute and gauge units of
//!   one symbol have the same factor, so the value converts between `psi`
//!   and `kPa` the same either way.
//! * The air/fuel ratio of a named fuel ([`LegacyUnit::FuelAfr`]): `AFR Gas`,
//!   and legacy's `AFR Methonal` (sic). Plain `AFR` names no fuel.
//! * No unit ([`LegacyUnit::NoUnit`]): a count, a state, an index or a raw
//!   ECU value (`count`, `On/Off`, `raw ecu value`, `steps`), a placeholder
//!   (`-`, `#`, `?`, `Unknown`) or nothing at all (an empty string). These
//!   are not errors: the value is shown as a plain number. Legacy gave a few
//!   of them a gauge range of their own (`gear` 0 to 6, `status` 0 to 10,
//!   `ConverterMaxMinDefaults.java:52-60`); a definition's own range, not a
//!   unit, sets that here.
//!
//! "Relative" pressures are gauge pressures; "relative sea level" ones are
//! relative to standard sea-level pressure, not to the atmosphere at the
//! time, and are gauge units here too (they show boost). The fuel-air
//! equivalence ratio φ (which the logger definitions compute as 1/λ) is a
//! plain [`RATIO`]: it is not lambda, and the registry converts neither into
//! the other.
//!
//! A label's bracketed unit names the unit of an absolute value. The corpus
//! has no temperature difference so labelled (`Temperature Delta (Degrees F)`
//! would convert as a temperature, with the 32 °F offset); angle and
//! pressure differences convert the same as their values.
//!
//! # Unknown in the pinned corpus
//!
//! Strings that name a quantity with no registry unit are unknown, and stay
//! so until the registry has one: fuel economy (`km/l`, `l/100km`,
//! `mpg (US)`; `l/100km` is not affine), angular speed (`degrees/s`),
//! injected volume per stroke (`mm³/st`), sensor slopes (`psia/v`), and
//! labels that name only a quantity (`Engine Speed`, `Initial Afterstart
//! Offset`). The unit tests list them.

use core::fmt;

use super::fuel::Fuel;
use super::registry::*;
use super::{Dimension, Unit};
use crate::diag::{Code, Diagnostic, Provenance, codes};

/// What a legacy unit string names.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum LegacyUnit {
    /// This registry unit.
    Unit(&'static Unit),
    /// A pressure in this unit, not saying absolute or gauge (module docs).
    /// The unit is the absolute one; [`LegacyUnit::gauge`] gives the gauge
    /// unit of the same symbol, where the registry has one.
    UnstatedPressure(&'static Unit),
    /// An air/fuel ratio ([`AIR_FUEL_RATIO`]) of this fuel.
    FuelAfr(Fuel),
    /// The value has no unit: a count, a state, a raw value, or nothing.
    NoUnit,
}

impl LegacyUnit {
    /// The registry unit: the absolute one for an
    /// [`UnstatedPressure`](Self::UnstatedPressure), [`AIR_FUEL_RATIO`] for a
    /// [`FuelAfr`](Self::FuelAfr), none for [`NoUnit`](Self::NoUnit).
    #[must_use]
    pub const fn unit(self) -> Option<&'static Unit> {
        match self {
            Self::Unit(u) | Self::UnstatedPressure(u) => Some(u),
            Self::FuelAfr(_) => Some(&AIR_FUEL_RATIO),
            Self::NoUnit => None,
        }
    }

    /// For an [`UnstatedPressure`](Self::UnstatedPressure), the gauge unit
    /// with the same symbol and factor, if the registry has one (`psi`,
    /// `kPa`, `bar`, `inHg`, `mmHg`; not `Pa`, `hPa`, `MPa`).
    #[must_use]
    pub fn gauge(self) -> Option<&'static Unit> {
        let Self::UnstatedPressure(absolute) = self else {
            return None;
        };
        Dimension::GaugePressure
            .units()
            .find(|g| g.symbol == absolute.symbol && g.to_base == absolute.to_base)
    }
}

/// `text` as a [`LegacyUnit`] (module docs, "Matching").
///
/// # Errors
///
/// [`UnknownUnitString`] when `text` names nothing known.
pub fn legacy_unit(text: &str) -> Result<LegacyUnit, UnknownUnitString> {
    let trimmed = text.trim();
    if let Some(found) = alias(trimmed) {
        return Ok(found);
    }
    bracketed(trimmed)
        .and_then(alias)
        .ok_or_else(|| UnknownUnitString {
            text: text.to_owned(),
        })
}

/// The alias `text` exactly, if listed.
fn alias(text: &str) -> Option<LegacyUnit> {
    LEGACY_ALIASES
        .iter()
        .find(|(alias, _)| *alias == text)
        .map(|&(_, unit)| unit)
}

/// The last bracketed part of `text`, trimmed, when `text` ends with it and
/// it is not empty: `"RPM"` of `"Engine Speed (RPM)"`.
fn bracketed(text: &str) -> Option<&str> {
    let body = text.strip_suffix(')')?;
    let open = body.rfind('(')?;
    let inner = body.get(open + 1..)?.trim();
    (!inner.is_empty()).then_some(inner)
}

/// A unit string that [`legacy_unit`] does not know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownUnitString {
    /// The string, as given.
    pub text: String,
}

impl UnknownUnitString {
    /// The registry code this is reported with.
    #[must_use]
    pub const fn code(&self) -> Code {
        codes::UNKNOWN_UNIT_STRING
    }

    /// This as a Warning diagnostic at `provenance`, with
    /// [`code`](Self::code) and the `Display` message.
    #[must_use]
    pub fn to_diagnostic(&self, provenance: Provenance) -> Diagnostic {
        Diagnostic::warning(self.code(), self.to_string(), provenance)
    }
}

impl fmt::Display for UnknownUnitString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unit string {:?} names no known unit: the value is shown with it as a label \
             and cannot be converted",
            self.text
        )
    }
}

impl core::error::Error for UnknownUnitString {}

const fn u(unit: &'static Unit) -> LegacyUnit {
    LegacyUnit::Unit(unit)
}

const fn p(absolute: &'static Unit) -> LegacyUnit {
    LegacyUnit::UnstatedPressure(absolute)
}

const NO: LegacyUnit = LegacyUnit::NoUnit;

/// Every legacy unit string [`legacy_unit`] knows, and what it names; each
/// string once. Strings from the pinned corpus unless marked as legacy's.
pub const LEGACY_ALIASES: &[(&str, LegacyUnit)] = &[
    // Temperature.
    ("C", u(&CELSIUS)),
    ("°C", u(&CELSIUS)),
    ("Degrees C", u(&CELSIUS)),
    ("F", u(&FAHRENHEIT)),
    ("°F", u(&FAHRENHEIT)),
    ("Degrees F", u(&FAHRENHEIT)),
    ("DegreesF", u(&FAHRENHEIT)),
    // Pressure, absolute or gauge not said.
    ("Pa", p(&PASCAL)),
    ("hPa", p(&HECTOPASCAL)),
    // 1 mbar = 1 hPa exactly (SP 811, "bar").
    ("mbar", p(&HECTOPASCAL)),
    ("kPa", p(&KILOPASCAL)),
    ("MPa", p(&MEGAPASCAL)),
    ("bar", p(&BAR)),
    ("psi", p(&PSI)),
    ("inHg", p(&INCH_OF_MERCURY)),
    ("mmHg", p(&MILLIMETRE_OF_MERCURY)),
    // Absolute pressure.
    ("kPa absolute", u(&KILOPASCAL)),
    ("bar absolute", u(&BAR)),
    ("bara", u(&BAR)),
    ("psi absolute", u(&PSI)),
    ("psia", u(&PSI)),
    ("inHg absolute", u(&INCH_OF_MERCURY)),
    ("mmHg absolute", u(&MILLIMETRE_OF_MERCURY)),
    // Gauge pressure.
    ("kPa relative", u(&KILOPASCAL_GAUGE)),
    ("kPa relative sea level", u(&KILOPASCAL_GAUGE)),
    ("bar relative", u(&BAR_GAUGE)),
    ("bar relative sea level", u(&BAR_GAUGE)),
    ("psi relative", u(&PSI_GAUGE)),
    ("psi relative sea level", u(&PSI_GAUGE)),
    ("psirelativesealevel", u(&PSI_GAUGE)),
    ("inHg relative", u(&INCH_OF_MERCURY_GAUGE)),
    ("mmHg relative", u(&MILLIMETRE_OF_MERCURY_GAUGE)),
    ("mmHg relative sea level", u(&MILLIMETRE_OF_MERCURY_GAUGE)),
    // Speed and distance.
    ("km/h", u(&KILOMETRE_PER_HOUR)),
    ("KMH", u(&KILOMETRE_PER_HOUR)),
    ("mph", u(&MILE_PER_HOUR)),
    ("MPH", u(&MILE_PER_HOUR)),
    ("km", u(&KILOMETRE)),
    ("miles", u(&MILE)),
    // Mass, mass flow, volume, volume flow.
    ("g", u(&GRAM)),
    ("grams", u(&GRAM)),
    ("g/s", u(&GRAM_PER_SECOND)),
    ("g/sec", u(&GRAM_PER_SECOND)),
    ("kg/h", u(&KILOGRAM_PER_HOUR)),
    ("litres", u(&LITRE)),
    ("ml", u(&MILLILITRE)),
    ("gallon (US)", u(&US_GALLON)),
    ("gallon (UK)", u(&IMPERIAL_GALLON)),
    ("l/h", u(&LITRE_PER_HOUR)),
    ("L/min", u(&LITRE_PER_MINUTE)),
    ("cc/min", u(&CC_PER_MINUTE)),
    ("gph (US)", u(&US_GALLON_PER_HOUR)),
    ("gph (UK)", u(&IMPERIAL_GALLON_PER_HOUR)),
    // Time.
    ("sec", u(&SECOND)),
    ("seconds", u(&SECOND)),
    ("ms", u(&MILLISECOND)),
    ("milliseconds", u(&MILLISECOND)),
    ("µs", u(&MICROSECOND)),
    ("minutes", u(&MINUTE)),
    // Angle: ignition timing, crank and cam angles, corrections.
    ("°", u(&DEGREE)),
    ("degrees", u(&DEGREE)),
    ("Degrees", u(&DEGREE)),
    ("degrees (retard)", u(&DEGREE)),
    ("degrees BTDC", u(&DEGREE)),
    ("degrees of correction", u(&DEGREE)),
    ("Degrees Delta", u(&DEGREE)),
    ("°BTDC", u(&DEGREE)),
    ("°CA", u(&DEGREE)),
    ("° CRK", u(&DEGREE)),
    ("° Cor", u(&DEGREE)),
    ("°KW", u(&DEGREE)),
    ("KW °", u(&DEGREE)),
    ("degKW", u(&DEGREE)),
    // Rotational speed.
    ("rpm", u(&REV_PER_MINUTE)),
    ("RPM", u(&REV_PER_MINUTE)),
    // Electrical.
    ("V", u(&VOLT)),
    ("VDC", u(&VOLT)),
    ("volts", u(&VOLT)),
    ("Volts", u(&VOLT)),
    ("A", u(&AMPERE)),
    ("Amp", u(&AMPERE)),
    ("mA", u(&MILLIAMPERE)),
    ("ohm", u(&OHM)),
    ("Ohms", u(&OHM)),
    // Ratios and percentages.
    ("ratio", u(&RATIO)),
    ("Ratio", u(&RATIO)),
    ("Duty Ratio", u(&RATIO)),
    ("multiplier", u(&RATIO)),
    ("Multiplier", u(&RATIO)),
    ("multiplier value", u(&RATIO)),
    ("coefficient", u(&RATIO)),
    ("Koeffizient", u(&RATIO)),
    ("fuel-air equivalence ratio", u(&RATIO)),
    ("%", u(&PERCENT)),
    ("absolute %", u(&PERCENT)),
    ("% absolute", u(&PERCENT)),
    ("% relative", u(&PERCENT)),
    // Torque and acceleration.
    ("Nm", u(&NEWTON_METRE)),
    ("ft-lbs", u(&POUND_FOOT)),
    ("m/s²", u(&METRE_PER_SECOND_SQUARED)),
    // Air/fuel ratio and lambda.
    ("AFR", u(&AIR_FUEL_RATIO)),
    ("estimated AFR", u(&AIR_FUEL_RATIO)),
    ("Prosport AFR", u(&AIR_FUEL_RATIO)),
    ("Air/Fuel Ratio", u(&AIR_FUEL_RATIO)),
    ("Estimated Air/Fuel Ratio", u(&AIR_FUEL_RATIO)),
    ("estimated air/fuel ratio", u(&AIR_FUEL_RATIO)),
    (
        "Estimated Air/Fuel Ratio Points (Additive)",
        u(&AIR_FUEL_RATIO),
    ),
    ("AFR Gas", LegacyUnit::FuelAfr(Fuel::Gasoline)),
    // Legacy's external wideband strings (SensorConversionsAFR.java:28-34),
    // misspellings included.
    ("AFR Gasoline", LegacyUnit::FuelAfr(Fuel::Gasoline)),
    ("AFR Ethonal", LegacyUnit::FuelAfr(Fuel::Ethanol)),
    ("AFR Diesel", LegacyUnit::FuelAfr(Fuel::Diesel)),
    ("AFR Methonal", LegacyUnit::FuelAfr(Fuel::Methanol)),
    ("AFR LPG", LegacyUnit::FuelAfr(Fuel::Lpg)),
    ("AFR CNG", LegacyUnit::FuelAfr(Fuel::Cng)),
    ("AFR Hydrogen", LegacyUnit::FuelAfr(Fuel::Hydrogen)),
    ("lambda", u(&LAMBDA)),
    ("Lambda", u(&LAMBDA)),
    ("Lambda Additive", u(&LAMBDA)),
    ("lambda enrichment", u(&LAMBDA)),
    ("lambda enrichment (additive)", u(&LAMBDA)),
    ("Lambda Enrichment (Additive)", u(&LAMBDA)),
    // Engine load.
    ("g/rev", u(&GRAM_PER_REV)),
    ("mg/cyl", u(&MILLIGRAM_PER_CYLINDER)),
    // Legacy's (ConverterMaxMinDefaults.java:57).
    ("g/cyl", u(&GRAM_PER_CYLINDER)),
    // Milligram per stroke: one cylinder filling per intake stroke.
    ("mg/stroke", u(&MILLIGRAM_PER_CYLINDER)),
    ("mg/st", u(&MILLIGRAM_PER_CYLINDER)),
    // No unit: nothing, placeholders.
    ("", NO),
    ("-", NO),
    ("#", NO),
    ("?", NO),
    ("Unknown", NO),
    // No unit: raw ECU values.
    ("raw", NO),
    ("rawdata", NO),
    ("raw value", NO),
    ("raw ecu value", NO),
    ("raw enrichment value", NO),
    // No unit: counts.
    ("count", NO),
    ("Anzahl", NO),
    ("counter", NO),
    ("Counter", NO),
    ("Counter Cycles", NO),
    ("counter threshold", NO),
    ("Mode Counter", NO),
    ("misfire count", NO),
    ("Misfire Count", NO),
    ("Fehlzündungszähler", NO),
    ("Times", NO),
    ("steps", NO),
    ("Stufe", NO),
    // No unit: states, positions, indices.
    ("gear", NO),
    ("Gang", NO),
    ("position", NO),
    ("index", NO),
    ("index position", NO),
    ("offset", NO),
    ("status", NO),
    ("mode", NO),
    ("fueling mode", NO),
    ("flag", NO),
    ("bit flags", NO),
    ("On/Off", NO),
    ("Lean/Rich", NO),
    ("Open/Closed", NO),
];

#[cfg(test)]
mod tests;
