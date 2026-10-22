//! Fuels and their stoichiometric air/fuel ratios: the data that turns an
//! air/fuel ratio into lambda and back (US-RR-21-5).
//!
//! Lambda is the air/fuel ratio over the fuel's stoichiometric ratio,
//! `λ = AFR / AFRs`, so the same mixture is one lambda and a different AFR
//! for each fuel: λ 1.0 is 14.7 AFR on gasoline and 6.47 on methanol. An
//! [`AirFuelRatio`](super::Dimension::AirFuelRatio) value and a
//! [`Lambda`](super::Dimension::Lambda) one therefore convert only through a
//! [`Fuel`] ([`afr_to_lambda`], [`lambda_to_afr`]), never through
//! [`convert`](super::convert).
//!
//! # The table
//!
//! [`FUELS`] lists each fuel's stoichiometric ratio by mass and where the
//! value comes from ([`StoichBasis`]):
//!
//! * A pure compound (methanol, ethanol, hydrogen) has one stoichiometric
//!   ratio, fixed by its formula: [`StoichBasis::Reference`], the value of
//!   Heywood, *Internal Combustion Engine Fundamentals* (McGraw-Hill, 1988),
//!   Appendix D, Table D.4. Each agrees with the working on the entry: the
//!   moles of O₂ that burn one mole of the fuel, times 4.773 moles of air per
//!   mole of O₂ (1 O₂ : 3.773 atmospheric N₂) and 28.962 g per mole of air
//!   (Heywood §3.3), over the fuel's molar mass.
//! * A blend (gasoline, diesel, LPG, CNG) has a ratio that depends on its
//!   composition, so any one value is a convention:
//!   [`StoichBasis::Legacy`], legacy RomRaider's value
//!   (`logger/external/core/SensorConversionsLambda.java:28-34`, the same
//!   factors inverted in `SensorConversionsAFR.java:27-34`), which is what a
//!   RomRaider user's logs and gauges have shown. Each entry gives the
//!   reference value beside it.
//!
//! Legacy's pure-compound values were rounded: methanol 6.4 (chemistry
//! 6.47) and hydrogen 34 (34.3). The reference values replace them, so a
//! methanol AFR here is about 1 % above legacy's for the same lambda.
//! Ethanol is 9.0 in both.
//!
//! Not listed: E85 and other ethanol blends. Their ethanol content varies by
//! season and market (ASTM D5798 allows 51 to 83 % by volume), so their
//! ratio is not a constant of the fuel; a flex-fuel log needs the measured
//! ethanol content, not a table row.
//!
//! # Refusals
//!
//! An air/fuel ratio and a lambda are positive and finite. [`afr_to_lambda`]
//! and [`lambda_to_afr`] refuse anything else as a [`ConvertError`]: a NaN
//! or infinity is [`ConvertError::NonFinite`], zero or a negative value
//! [`ConvertError::NotPositive`] (a sensor's "no reading" is not a mixture).
//!
//! The table is `const` data: no shared state (`STATELESS_PATHS`).

use core::fmt;

use super::{AIR_FUEL_RATIO, ConvertError, LAMBDA};

/// A fuel with a stoichiometric air/fuel ratio in [`FUELS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Fuel {
    /// Gasoline (petrol).
    Gasoline,
    /// Ethanol, C₂H₅OH.
    Ethanol,
    /// Methanol, CH₃OH.
    Methanol,
    /// Diesel.
    Diesel,
    /// Liquefied petroleum gas (propane and butane).
    Lpg,
    /// Compressed natural gas (mostly methane).
    Cng,
    /// Hydrogen, H₂.
    Hydrogen,
}

/// Where a fuel's stoichiometric ratio comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoichBasis {
    /// Legacy RomRaider's value, kept for a blend whose ratio depends on its
    /// composition (module docs).
    Legacy,
    /// A published reference value for a pure compound (module docs).
    Reference,
}

/// One row of [`FUELS`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct FuelInfo {
    /// The fuel.
    pub fuel: Fuel,
    /// How code and profiles name it: lower-case ASCII, unique.
    pub id: &'static str,
    /// Its name.
    pub name: &'static str,
    /// The stoichiometric air/fuel ratio by mass: lambda 1.
    pub stoich_afr: f64,
    /// Where `stoich_afr` comes from.
    pub basis: StoichBasis,
    /// Legacy RomRaider's value, `SensorConversionsLambda.java:28-34`.
    pub legacy_afr: f64,
}

/// Legacy 14.7 (`SensorConversionsLambda.java:28`), as the pinned
/// definitions assume: the ECU scaling "Estimated Air/Fuel Ratio" is
/// `14.7/(1+x*.0078125)` and the SSM logger's "estimated AFR" is
/// lambda × 14.7. Heywood Table D.4 gives 14.6; the pinned OBD logger
/// definitions' "AFR Gas" use 14.64.
pub const GASOLINE: FuelInfo = FuelInfo {
    fuel: Fuel::Gasoline,
    id: "gasoline",
    name: "gasoline",
    stoich_afr: 14.7,
    basis: StoichBasis::Legacy,
    legacy_afr: 14.7,
};

/// C₂H₅OH + 3 O₂: 3 × 4.773 × 28.962 / 46.069 = 9.0019 (Heywood Table
/// D.4: 9.00). Legacy 9.0 (`SensorConversionsLambda.java:29`).
pub const ETHANOL: FuelInfo = FuelInfo {
    fuel: Fuel::Ethanol,
    id: "ethanol",
    name: "ethanol",
    stoich_afr: 9.0,
    basis: StoichBasis::Reference,
    legacy_afr: 9.0,
};

/// CH₃OH + 1.5 O₂: 1.5 × 4.773 × 28.962 / 32.042 = 6.4713 (Heywood
/// Table D.4: 6.47). Legacy 6.4 (`SensorConversionsLambda.java:31`).
pub const METHANOL: FuelInfo = FuelInfo {
    fuel: Fuel::Methanol,
    id: "methanol",
    name: "methanol",
    stoich_afr: 6.47,
    basis: StoichBasis::Reference,
    legacy_afr: 6.4,
};

/// Legacy 14.6 (`SensorConversionsLambda.java:30`). Heywood Table D.4:
/// 14.5 for light diesel.
pub const DIESEL: FuelInfo = FuelInfo {
    fuel: Fuel::Diesel,
    id: "diesel",
    name: "diesel",
    stoich_afr: 14.6,
    basis: StoichBasis::Legacy,
    legacy_afr: 14.6,
};

/// Legacy 15.5 (`SensorConversionsLambda.java:32`). Pure propane is 15.67
/// (Heywood Table D.4); butane, the rest of LPG, is lower.
pub const LPG: FuelInfo = FuelInfo {
    fuel: Fuel::Lpg,
    id: "lpg",
    name: "liquefied petroleum gas",
    stoich_afr: 15.5,
    basis: StoichBasis::Legacy,
    legacy_afr: 15.5,
};

/// Legacy 17.2 (`SensorConversionsLambda.java:33`). Pure methane is 17.23
/// (Heywood Table D.4); natural gas's other gases move it.
pub const CNG: FuelInfo = FuelInfo {
    fuel: Fuel::Cng,
    id: "cng",
    name: "compressed natural gas",
    stoich_afr: 17.2,
    basis: StoichBasis::Legacy,
    legacy_afr: 17.2,
};

/// H₂ + 0.5 O₂: 0.5 × 4.773 × 28.962 / 2.016 = 34.285 (Heywood Table
/// D.4: 34.3). Legacy 34 (`SensorConversionsLambda.java:34`).
pub const HYDROGEN: FuelInfo = FuelInfo {
    fuel: Fuel::Hydrogen,
    id: "hydrogen",
    name: "hydrogen",
    stoich_afr: 34.3,
    basis: StoichBasis::Reference,
    legacy_afr: 34.0,
};

/// Every fuel, in [`Fuel`] declaration order.
pub const FUELS: &[FuelInfo] = &[GASOLINE, ETHANOL, METHANOL, DIESEL, LPG, CNG, HYDROGEN];

impl Fuel {
    /// Every fuel, in declaration order.
    pub const ALL: [Self; 7] = [
        Self::Gasoline,
        Self::Ethanol,
        Self::Methanol,
        Self::Diesel,
        Self::Lpg,
        Self::Cng,
        Self::Hydrogen,
    ];

    /// The fuel's row of [`FUELS`].
    #[must_use]
    pub const fn info(self) -> &'static FuelInfo {
        match self {
            Self::Gasoline => &GASOLINE,
            Self::Ethanol => &ETHANOL,
            Self::Methanol => &METHANOL,
            Self::Diesel => &DIESEL,
            Self::Lpg => &LPG,
            Self::Cng => &CNG,
            Self::Hydrogen => &HYDROGEN,
        }
    }

    /// The fuel's [`id`](FuelInfo::id).
    #[must_use]
    pub const fn id(self) -> &'static str {
        self.info().id
    }

    /// The stoichiometric air/fuel ratio by mass: the AFR of lambda 1.
    #[must_use]
    pub const fn stoich_afr(self) -> f64 {
        self.info().stoich_afr
    }

    /// The fuel whose [`id`](FuelInfo::id) is `id` (exact), if any.
    #[must_use]
    pub fn by_id(id: &str) -> Option<Self> {
        FUELS.iter().find(|f| f.id == id).map(|f| f.fuel)
    }
}

/// The fuel's [`id`](FuelInfo::id).
impl fmt::Display for Fuel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// `value`, checked to be a positive finite ratio, divided or multiplied by
/// `fuel`'s stoichiometric ratio.
fn ratio(
    value: f64,
    from: &'static str,
    to: &'static str,
    stoich: f64,
    divide: bool,
) -> Result<f64, ConvertError> {
    if !value.is_finite() {
        return Err(ConvertError::NonFinite { value, from, to });
    }
    if value <= 0.0 {
        return Err(ConvertError::NotPositive { value, from, to });
    }
    let result = if divide {
        value / stoich
    } else {
        value * stoich
    };
    if result.is_finite() {
        Ok(result)
    } else {
        Err(ConvertError::NonFinite { value, from, to })
    }
}

/// The lambda of air/fuel ratio `afr` on `fuel`: `afr / stoich`.
///
/// # Errors
///
/// [`ConvertError::NonFinite`] for a NaN or infinite `afr`, and
/// [`ConvertError::NotPositive`] for zero or a negative one.
pub fn afr_to_lambda(afr: f64, fuel: Fuel) -> Result<f64, ConvertError> {
    ratio(afr, AIR_FUEL_RATIO.id, LAMBDA.id, fuel.stoich_afr(), true)
}

/// The air/fuel ratio of `lambda` on `fuel`: `lambda × stoich`.
///
/// # Errors
///
/// [`ConvertError::NonFinite`] for a NaN or infinite `lambda` (or one so
/// large its AFR is), and [`ConvertError::NotPositive`] for zero or a
/// negative one.
pub fn lambda_to_afr(lambda: f64, fuel: Fuel) -> Result<f64, ConvertError> {
    ratio(
        lambda,
        LAMBDA.id,
        AIR_FUEL_RATIO.id,
        fuel.stoich_afr(),
        false,
    )
}

#[cfg(test)]
mod tests;
