//! Lookup-table conversions: `(raw, real)` points, linearly interpolated.

use super::{LutError, ToRealError, finite};

/// A lookup table: at least two `(raw, real)` points, every value finite,
/// raw values strictly increasing.
///
/// The real value of a raw value is that of the point with that raw value,
/// or the straight line between the two points around it; outside the
/// first and last point there is none (see the
/// [module docs](super#raw-to-real)). Real values may go in any direction:
/// whether the table can be inverted for an edit is decided where edits are
/// checked, not here.
#[derive(Debug, Clone, PartialEq)]
pub struct Lut {
    points: Vec<(f64, f64)>,
}

impl Lut {
    /// A table of `points`, in order.
    ///
    /// # Errors
    ///
    /// [`LutError`] (which converts into
    /// [`ConversionError::Lut`](super::ConversionError::Lut)): fewer than two points, a NaN or infinite value, or a raw value not
    /// greater than the one before it.
    pub fn new(points: &[(f64, f64)]) -> Result<Self, LutError> {
        if points.len() < 2 {
            return Err(LutError::TooFewPoints {
                found: points.len(),
            });
        }
        if let Some(index) = points
            .iter()
            .position(|&(raw, real)| !(raw.is_finite() && real.is_finite()))
        {
            return Err(LutError::NonFinite { index });
        }
        if let Some(i) = points
            .iter()
            .zip(points.iter().skip(1))
            .position(|(before, after)| after.0 <= before.0)
        {
            return Err(LutError::NotIncreasing { index: i + 1 });
        }
        Ok(Self {
            points: points.to_vec(),
        })
    }

    /// The points, raw values increasing.
    #[must_use]
    pub fn points(&self) -> &[(f64, f64)] {
        &self.points
    }

    /// The real value at raw value `x`.
    ///
    /// # Errors
    ///
    /// [`ToRealError::OutsideTable`] when `x` is below the first raw value,
    /// above the last or NaN, and [`ToRealError::NonFinite`] when the
    /// interpolation overflows.
    pub fn to_real(&self, x: f64) -> Result<f64, ToRealError> {
        // The number of points at or below `x`; the point before that count
        // is the one at or just below `x`.
        let at_or_below = self.points.partition_point(|&(raw, _)| raw <= x);
        let Some(&(r0, y0)) = at_or_below.checked_sub(1).and_then(|i| self.points.get(i)) else {
            return Err(self.outside(x));
        };
        if r0 == x {
            return Ok(y0);
        }
        let Some(&(r1, y1)) = self.points.get(at_or_below) else {
            return Err(self.outside(x));
        };
        finite(y0 + (y1 - y0) * ((x - r0) / (r1 - r0)))
    }

    fn outside(&self, x: f64) -> ToRealError {
        let first = self.points.first().map_or(f64::NAN, |p| p.0);
        let last = self.points.last().map_or(f64::NAN, |p| p.0);
        ToRealError::OutsideTable {
            raw: x,
            min: first,
            max: last,
        }
    }
}
