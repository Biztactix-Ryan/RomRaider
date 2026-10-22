//! Units (concept C5, US-RR-21): one registry of the units RomRaider shows
//! values in, with exact conversions between units of the same dimension
//! and a default display range and precision for each.
//!
//! A [`Unit`] has an `id` (lower-case ASCII, how code and profiles name
//! it), a `symbol` (what a gauge, table header or CSV column shows), a
//! long `name`, a [`Dimension`], a conversion to its dimension's base unit
//! ([`ToBase`]), a default [`DisplayRange`] and a default precision. The
//! registry is [`UNITS`]; [`by_id`] finds one, [`Dimension::base`] gives a
//! dimension's base unit and [`convert`] converts a value between two units
//! of one dimension.
//!
//! # What it replaces
//!
//! * `ConverterMaxMinDefaults`
//!   (`logger/ecu/definition/xml/ConverterMaxMinDefaults.java:33-62`):
//!   gauge minimum, maximum and step keyed by the lower-cased unit string,
//!   with `0..100` for any string it did not know. Here each registry unit
//!   carries its own [`DisplayRange`] (legacy's values where it had one,
//!   cited on the unit), and a string that names no unit is a lookup
//!   failure for the caller to report, never a silent default: the unit
//!   strings of legacy definitions are mapped onto these units by
//!   [`legacy_unit`] (below, "Legacy unit strings").
//! * The dyno's Imperial/Metric branches
//!   (`logger/ecu/ui/tab/dyno/DynoControlPanel.java:249-260`, `:297-302`,
//!   `:322-343`, `:893-942`) and `TorqueCalculator`
//!   (`logger/car/util/TorqueCalculator.java:31-36`): every unit switch
//!   was a string comparison and a hand-written, rounded factor
//!   (`0.4536` kg per lb, `745.7` W per hp, `6894.75728` Pa per psi,
//!   `14.503773801` psi per bar). Here a conversion is
//!   `convert(value, from, to)` with the exact defined factors below.
//! * `KPH_2_MPH = 1.609344` (`logger/car/util/Constants.java:27`,
//!   `DynoControlPanel.java:131`) is kilometres per mile, so its name is
//!   inverted (PROJECT.md MF-42). Here nobody names a factor:
//!   [`MILE_PER_HOUR`] is defined as 1.609344 [`KILOMETRE_PER_HOUR`], and
//!   the direction is the argument order of [`convert`].
//!
//! # Conversions
//!
//! A unit's value `v` is `(v + offset) * mul / div` in the base unit
//! ([`ToBase`]), and back, `base * div / mul - offset`. Every `offset`,
//! `mul` and `div` is written as the exact decimal its definition gives
//! (`0.45359237` kg per lb, `1609.344` m per mile), or as a ratio of two
//! such decimals where the factor itself has no finite decimal form (5/9
//! for the degree Fahrenheit; `4.4482216152605 / 0.64516` kPa per psi,
//! the pound-force over the square inch). Nothing is pre-multiplied into a
//! rounded constant: each conversion applies its definition, in `f64`.
//!
//! Temperature is affine, not just a factor: 32 °F is 0 °C. Its base unit
//! is the degree Celsius and each unit carries an `offset` applied before
//! the factor (`(t/°F − 32) × 5/9`, `T/K − 273.15`, NIST SP 811 (2008)
//! §4.2.1.1 and Appendix B.8), so freezing and boiling water, and every
//! whole degree, convert without rounding.
//!
//! [`convert`] goes through the base unit: from `from` to base, base to
//! `to`. Each step is at most an addition, a multiplication and a
//! division, so a result is within a few ulps of the exact value (the
//! known answers in `fixtures/kat/units-defined-factors.toml` are checked
//! by rr-testkit's `units_kat` test). A conversion to the same unit
//! returns the value unchanged.
//!
//! Base units are the metric units RomRaider displays (°C, kPa, km/h, g/s,
//! kW, ...) rather than strict SI ones (K, Pa, m/s, kg/s, W): a metric
//! value then converts to its base with no arithmetic at all, and the
//! common conversions (°F ↔ °C, mph ↔ km/h, psi ↔ kPa) take one step.
//!
//! # Fail closed
//!
//! [`convert`] refuses, as a [`ConvertError`], rather than guess:
//!
//! * units of different dimensions ([`ConvertError::DimensionMismatch`]):
//!   a temperature never becomes a pressure, and an absolute pressure never
//!   becomes a gauge pressure (below);
//! * a value, or a result, that is NaN or infinite
//!   ([`ConvertError::NonFinite`]); a result overflows only for values far
//!   beyond any sensor's range.
//!
//! [`convert_by_id`] also refuses an id that names no unit
//! ([`ConvertError::UnknownUnit`]).
//!
//! # Pressure: absolute and gauge
//!
//! An absolute pressure ([`Dimension::Pressure`]: manifold absolute,
//! atmospheric) and a gauge pressure, relative to the atmosphere
//! ([`Dimension::GaugePressure`]: boost) are different dimensions. Within
//! each, units differ only by a factor; between them the difference is the
//! atmospheric pressure at the time, which is a reading, not a unit
//! conversion, so [`convert`] refuses it. The two share unit symbols
//! (`psi`, `kPa`): a unit symbol is not modified to say gauge or absolute
//! (NIST SP 811 (2008) §7.4), the quantity's name does that. Legacy's `psi`
//! and `bar` gauge ranges (`-20..40`, `-1.5..3`) are boost ranges, so they
//! are the gauge units' defaults here.
//!
//! # Not here
//!
//! * Units whose conversion is not affine, such as fuel economy (`l/100km`
//!   is the reciprocal of `km/l`).
//!
//! # Air/fuel ratio and lambda
//!
//! Lambda is the air/fuel ratio over the fuel's stoichiometric ratio, so
//! the two convert only through a fuel: [`Dimension::AirFuelRatio`] and
//! [`Dimension::Lambda`] are separate dimensions, which [`convert`] refuses
//! to cross, and [`afr_to_lambda`] and [`lambda_to_afr`] convert with a
//! [`Fuel`]'s ratio from [`FUELS`] ([`fuel`] module docs: which value each
//! fuel uses and why).
//!
//! # Legacy unit strings
//!
//! [`legacy_unit`] reads a definition's unit string (`"Degrees F"`,
//! `"psi relative sea level"`, `"Engine Speed (RPM)"`, `"AFR Gas"`) as a
//! [`LegacyUnit`]: a registry unit, a pressure that does not say absolute or
//! gauge, a fuel's air/fuel ratio, or no unit. [`LEGACY_ALIASES`] lists the
//! strings, exactly as the pinned definition corpus spells them; one it
//! does not know is an [`UnknownUnitString`], reported as the Warning
//! `UNT0001` ([`UnknownUnitString::to_diagnostic`]). Matching is exact and
//! case-sensitive after trimming, with one rule for labels ending in a
//! bracketed unit; the private `legacy` module's docs give the rules.
//!
//! # No shared state
//!
//! The registry is `const` data: the module holds no `static`, cache, lock,
//! cell or atomic (`cargo xtask check-globals`, `STATELESS_PATHS` in
//! `xtask/src/globals.rs`).
//!
//! # Example
//!
//! ```
//! use rr_core::units::{self, ConvertError, Dimension};
//!
//! // 1 mph is 1.609344 km/h, exactly (MF-42).
//! assert_eq!(units::convert(1.0, &units::MILE_PER_HOUR, &units::KILOMETRE_PER_HOUR), Ok(1.609344));
//!
//! // Temperature is affine: 212 °F is 100 °C is 373.15 K.
//! let f = units::by_id("fahrenheit").unwrap();
//! assert_eq!(units::convert(212.0, f, &units::CELSIUS), Ok(100.0));
//! assert_eq!(units::convert_by_id(100.0, "celsius", "kelvin"), Ok(373.15));
//!
//! // Only within a dimension.
//! assert!(matches!(
//!     units::convert(1.0, &units::PSI, &units::CELSIUS),
//!     Err(ConvertError::DimensionMismatch { .. })
//! ));
//! assert_eq!(Dimension::Pressure.base().symbol, "kPa");
//! ```

pub mod fuel;
mod legacy;
mod registry;

pub use fuel::{FUELS, Fuel, FuelInfo, StoichBasis, afr_to_lambda, lambda_to_afr};
pub use legacy::{LEGACY_ALIASES, LegacyUnit, UnknownUnitString, legacy_unit};
pub use registry::*;

use core::fmt;

/// What a unit measures. Units convert only within one dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Dimension {
    /// Temperature. Base: degree Celsius.
    Temperature,
    /// Absolute pressure (manifold absolute, atmospheric). Base: kilopascal.
    Pressure,
    /// Gauge pressure, relative to the atmosphere (boost). Base: kilopascal
    /// (gauge).
    GaugePressure,
    /// Speed. Base: kilometre per hour.
    Speed,
    /// Length and distance. Base: metre.
    Length,
    /// Mass. Base: kilogram.
    Mass,
    /// Mass flow (airflow). Base: gram per second.
    MassFlow,
    /// Volume. Base: litre.
    Volume,
    /// Volume flow (fuel flow). Base: litre per hour.
    VolumeFlow,
    /// Time. Base: second.
    Time,
    /// Plane angle (ignition timing, throttle angle). Base: degree.
    Angle,
    /// Rotational speed. Base: revolution per minute.
    RotationalSpeed,
    /// Electric potential. Base: volt.
    Voltage,
    /// Electric current. Base: ampere.
    Current,
    /// Electric resistance. Base: ohm.
    Resistance,
    /// A dimensionless ratio (a multiplier, a duty cycle). Base: ratio, 1.
    Ratio,
    /// Torque. Base: newton metre.
    Torque,
    /// Power. Base: kilowatt.
    Power,
    /// Acceleration. Base: metre per second squared.
    Acceleration,
    /// Air/fuel ratio by mass. Converting it to lambda depends on the fuel
    /// ([`afr_to_lambda`]), so it is its own dimension.
    AirFuelRatio,
    /// Lambda, the air/fuel ratio over the fuel's stoichiometric ratio.
    Lambda,
    /// Air mass per engine revolution (engine load). Base: gram per
    /// revolution.
    MassPerRevolution,
    /// Air mass per cylinder filling. Base: gram per cylinder.
    MassPerCylinder,
}

impl Dimension {
    /// Every dimension, in declaration order.
    pub const ALL: [Self; 23] = [
        Self::Temperature,
        Self::Pressure,
        Self::GaugePressure,
        Self::Speed,
        Self::Length,
        Self::Mass,
        Self::MassFlow,
        Self::Volume,
        Self::VolumeFlow,
        Self::Time,
        Self::Angle,
        Self::RotationalSpeed,
        Self::Voltage,
        Self::Current,
        Self::Resistance,
        Self::Ratio,
        Self::Torque,
        Self::Power,
        Self::Acceleration,
        Self::AirFuelRatio,
        Self::Lambda,
        Self::MassPerRevolution,
        Self::MassPerCylinder,
    ];

    /// The dimension's name, lower case (`temperature`, `gauge pressure`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Temperature => "temperature",
            Self::Pressure => "absolute pressure",
            Self::GaugePressure => "gauge pressure",
            Self::Speed => "speed",
            Self::Length => "length",
            Self::Mass => "mass",
            Self::MassFlow => "mass flow",
            Self::Volume => "volume",
            Self::VolumeFlow => "volume flow",
            Self::Time => "time",
            Self::Angle => "angle",
            Self::RotationalSpeed => "rotational speed",
            Self::Voltage => "voltage",
            Self::Current => "current",
            Self::Resistance => "resistance",
            Self::Ratio => "ratio",
            Self::Torque => "torque",
            Self::Power => "power",
            Self::Acceleration => "acceleration",
            Self::AirFuelRatio => "air/fuel ratio",
            Self::Lambda => "lambda",
            Self::MassPerRevolution => "mass per revolution",
            Self::MassPerCylinder => "mass per cylinder",
        }
    }

    /// The dimension's base unit: the one whose [`ToBase`] is
    /// [`ToBase::IDENTITY`].
    #[must_use]
    pub const fn base(self) -> &'static Unit {
        match self {
            Self::Temperature => &CELSIUS,
            Self::Pressure => &KILOPASCAL,
            Self::GaugePressure => &KILOPASCAL_GAUGE,
            Self::Speed => &KILOMETRE_PER_HOUR,
            Self::Length => &METRE,
            Self::Mass => &KILOGRAM,
            Self::MassFlow => &GRAM_PER_SECOND,
            Self::Volume => &LITRE,
            Self::VolumeFlow => &LITRE_PER_HOUR,
            Self::Time => &SECOND,
            Self::Angle => &DEGREE,
            Self::RotationalSpeed => &REV_PER_MINUTE,
            Self::Voltage => &VOLT,
            Self::Current => &AMPERE,
            Self::Resistance => &OHM,
            Self::Ratio => &RATIO,
            Self::Torque => &NEWTON_METRE,
            Self::Power => &KILOWATT,
            Self::Acceleration => &METRE_PER_SECOND_SQUARED,
            Self::AirFuelRatio => &AIR_FUEL_RATIO,
            Self::Lambda => &LAMBDA,
            Self::MassPerRevolution => &GRAM_PER_REV,
            Self::MassPerCylinder => &GRAM_PER_CYLINDER,
        }
    }

    /// The registry's units of this dimension, in registry order.
    pub fn units(self) -> impl Iterator<Item = &'static Unit> {
        UNITS.iter().filter(move |u| u.dimension == self)
    }
}

/// The dimension's [`name`](Dimension::name).
impl fmt::Display for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// How a unit's value becomes its dimension's base unit:
/// `base = (value + offset) * mul / div`, and back,
/// `value = base * div / mul - offset`.
///
/// `offset`, `mul` and `div` are the exact decimals of the unit's
/// definition (module docs, "Conversions"); `mul` and `div` are positive
/// and every field is finite (the registry tests check both).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToBase {
    /// Added to the value before scaling, in the unit itself (`-32` for
    /// the degree Fahrenheit). Zero for every unit but temperatures.
    pub offset: f64,
    /// Multiplies the offset value.
    pub mul: f64,
    /// Divides the scaled value.
    pub div: f64,
}

impl ToBase {
    /// The base unit's own conversion: no offset, factor 1.
    pub const IDENTITY: Self = Self::ratio(1.0, 1.0);

    /// A pure factor `mul / div`, no offset.
    #[must_use]
    pub const fn ratio(mul: f64, div: f64) -> Self {
        Self {
            offset: 0.0,
            mul,
            div,
        }
    }

    /// `value` in the base unit: `(value + offset) * mul / div`.
    #[must_use]
    pub fn to_base(self, value: f64) -> f64 {
        (value + self.offset) * self.mul / self.div
    }

    /// The base-unit value `base` in this unit: `base * div / mul - offset`.
    #[must_use]
    pub fn from_base(self, base: f64) -> f64 {
        base * self.div / self.mul - self.offset
    }
}

/// A unit's default gauge range: where a gauge or graph axis starts and
/// ends, and its major tick interval. A default only: a definition or
/// profile may set its own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayRange {
    /// The low end.
    pub min: f64,
    /// The high end, above `min`.
    pub max: f64,
    /// The major tick interval, positive.
    pub step: f64,
}

/// One registry unit. Units are defined only here, in [`UNITS`]; outside
/// rr-core they are read, never built.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Unit {
    /// How code and profiles name the unit: lower-case ASCII letters,
    /// digits and `_`, unique in the registry (`fahrenheit`, `psi_gauge`).
    pub id: &'static str,
    /// What a display shows: `°F`, `psi`, `km/h`. Empty only for the
    /// dimensionless [`RATIO`].
    pub symbol: &'static str,
    /// The unit's full name: `degree Fahrenheit`.
    pub name: &'static str,
    /// What it measures.
    pub dimension: Dimension,
    /// Its conversion to the dimension's base unit.
    pub to_base: ToBase,
    /// The default gauge range.
    pub display: DisplayRange,
    /// The default number of decimal places shown.
    pub precision: u8,
}

/// The unit whose [`id`](Unit::id) is `id` (exact, case-sensitive), if the
/// registry has one.
#[must_use]
pub fn by_id(id: &str) -> Option<&'static Unit> {
    UNITS.iter().find(|u| u.id == id)
}

/// `value` in `from`, converted to `to`.
///
/// The same unit returns `value` unchanged; otherwise the value goes
/// through the dimension's base unit (module docs, "Conversions").
///
/// # Errors
///
/// [`ConvertError::DimensionMismatch`] when the units measure different
/// things, and [`ConvertError::NonFinite`] when `value`, or the result, is
/// NaN or infinite.
pub fn convert(value: f64, from: &Unit, to: &Unit) -> Result<f64, ConvertError> {
    if from.dimension != to.dimension {
        return Err(ConvertError::DimensionMismatch {
            from: from.id,
            from_dimension: from.dimension,
            to: to.id,
            to_dimension: to.dimension,
        });
    }
    let non_finite = || ConvertError::NonFinite {
        value,
        from: from.id,
        to: to.id,
    };
    if !value.is_finite() {
        return Err(non_finite());
    }
    if from.id == to.id {
        return Ok(value);
    }
    let result = to.to_base.from_base(from.to_base.to_base(value));
    if result.is_finite() {
        Ok(result)
    } else {
        Err(non_finite())
    }
}

/// [`convert`] with both units named by [`id`](Unit::id).
///
/// # Errors
///
/// [`ConvertError::UnknownUnit`] for an id the registry does not have (the
/// `from` id first), else as [`convert`].
pub fn convert_by_id(value: f64, from: &str, to: &str) -> Result<f64, ConvertError> {
    let unit = |id: &str| by_id(id).ok_or_else(|| ConvertError::UnknownUnit(id.to_owned()));
    convert(value, unit(from)?, unit(to)?)
}

/// Why [`convert`] (or [`convert_by_id`]) gave no value.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ConvertError {
    /// The units measure different things.
    DimensionMismatch {
        /// The `from` unit's id.
        from: &'static str,
        /// What it measures.
        from_dimension: Dimension,
        /// The `to` unit's id.
        to: &'static str,
        /// What it measures.
        to_dimension: Dimension,
    },
    /// The value, or its converted value, is NaN or infinite.
    NonFinite {
        /// The value given.
        value: f64,
        /// The `from` unit's id.
        from: &'static str,
        /// The `to` unit's id.
        to: &'static str,
    },
    /// No registry unit has this id.
    UnknownUnit(String),
    /// The value is zero or negative where only a positive one has a
    /// meaning: an air/fuel ratio or a lambda ([`afr_to_lambda`],
    /// [`lambda_to_afr`]).
    NotPositive {
        /// The value given.
        value: f64,
        /// The `from` unit's id.
        from: &'static str,
        /// The `to` unit's id.
        to: &'static str,
    },
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DimensionMismatch {
                from,
                from_dimension,
                to,
                to_dimension,
            } => write!(
                f,
                "cannot convert {from} ({from_dimension}) to {to} ({to_dimension}): \
                 units convert only within one dimension"
            ),
            Self::NonFinite { value, from, to } => write!(
                f,
                "{value} {from} has no finite value in {to}: a converted value must be a \
                 finite number"
            ),
            Self::UnknownUnit(id) => write!(f, "no unit has the id {id:?}"),
            Self::NotPositive { value, from, to } => write!(
                f,
                "{value} {from} has no value in {to}: an air/fuel ratio or lambda is positive"
            ),
        }
    }
}

impl core::error::Error for ConvertError {}

#[cfg(test)]
mod tests;
