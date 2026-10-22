//! US-RR-21-2, the second acceptance criterion of US-RR-21 (units registry,
//! C5): *converting A -> B -> A round-trips within floating tolerance for
//! every unit pair in the same dimension.*
//!
//! # Which pairs
//!
//! Every ordered pair `(A, B)` of distinct registry units that share a
//! [`Dimension`], enumerated from [`UNITS`] by [`same_dimension_pairs`], so a
//! unit added to the registry is covered without touching this file. With
//! the registry as written (67 units in 23 dimensions) that is 206 ordered
//! pairs; [`pair_count_matches_the_registry`] checks the enumeration against
//! the per-dimension unit counts and that it is not empty. Each generated
//! case checks *every* pair, not one sampled pair, so 1024 cases are 1024
//! values per pair and kind of value.
//!
//! AFR <-> lambda is not a [`convert`] pair (the two are separate
//! dimensions, joined only through a fuel); its round trip through
//! [`afr_to_lambda`] and [`lambda_to_afr`] is checked for every [`Fuel`].
//!
//! # Which values
//!
//! For every pair, three values per case:
//!
//! * a point of `A`'s default display range widened by twice its span on
//!   each side (°C `-20..200` becomes `-460..640`, below absolute zero; a
//!   boost gauge's vacuum; deceleration);
//! * `±10^e`, `e` uniform in `[-100, 100]`: far beyond any sensor in both
//!   directions, where neither overflow nor gradual underflow can occur
//!   (every registry factor, and every product of the factors one
//!   conversion applies, is within `1e±12`);
//! * an arbitrary finite `f64`, any sign, including zero, subnormals and
//!   `±f64::MAX` ([`extreme_values_round_trip_or_are_refused`]).
//!
//! Negative values are checked in every dimension, not only where the
//! quantity can be negative: [`convert`] is affine and accepts any finite
//! value, so its round trip must hold for any.
//!
//! # Tolerance, and how it is derived
//!
//! A conversion is `to.from_base(from.to_base(v))`, `to_base(v) = (v +
//! offset) * mul / div` and `from_base(b) = b * div / mul - offset`
//! (`units.rs`, [`rr_core::units::ToBase`]). The round trip is four such
//! steps: at most 12 rounded `f64` operations (an addition, a
//! multiplication and a division per step). In exact arithmetic it is the
//! identity, whatever the constants' own representation error, because
//! each step's constants are undone by the same constants.
//!
//! Each rounded operation is `x op y` times `(1 + δ)`, `|δ| <= u = ε/2`
//! (`ε` = [`f64::EPSILON`]). Every remaining operation after it is an exact
//! affine map with slope ±1 when measured in `A`'s units, so the error it
//! introduces reaches the result unscaled, in `A`'s units. Every
//! intermediate result, measured in `A`'s units, is at most
//!
//! ```text
//! S = |v| + |offset_A| + |offset_B| * k,   k = (mul_B / div_B) / (mul_A / div_A)
//! ```
//!
//! (`k` is the size of one `B` in `A`: `|offset_B|` is in `B`'s units).
//! Hence `|round_trip(v) - v| <= 12 u S (1 + O(u)) = 6 ε S (1 + O(u))`;
//! [`tolerance`] allows `8 ε S`, the same `8 ε` the deterministic
//! `round_trips_within_a_dimension` unit test uses. That is a relative
//! bound of `8 ε ≈ 1.8e-15` for every factor-only pair (`S = |v|`, so 0
//! round-trips to exactly 0), far inside the `1e-12` one would otherwise
//! state. For a temperature `S` keeps the offsets: near the offset (`v`
//! close to 32 °F or −273.15 °C) the subtraction cancels and only the
//! absolute error `8 ε (|offset_A| + k |offset_B|)` (`≈ 4.4e-13` °F for
//! °F <-> K) is meaningful, so that is what is allowed there.
//!
//! One absolute term, [`UNDERFLOW_ABS`] = `1e-300`, covers gradual
//! underflow for arbitrary values: an operation whose result is subnormal
//! errs by up to `2^-1075` (≈ `2.5e-324`) in its own unit, at most `1e12`
//! times that in `A`'s units, for at most 12 operations: under `3e-311`.
//! It is irrelevant to the first two kinds of value, whose intermediates
//! are never subnormal.
//!
//! # Overflow
//!
//! A value so large that a conversion's result (or one of its
//! intermediate products) is infinite is refused by [`convert`] as
//! [`ConvertError::NonFinite`] carrying the value and both ids, never a
//! panic and never an infinite `Ok`. The arbitrary-value test asserts
//! exactly that, and that it happens only for `|v| > OVERFLOW_FLOOR`
//! (`1e290`): with every factor within `1e±12`, nothing smaller can
//! overflow. Every other round trip that both legs accept is held to the
//! tolerance above.

// In a `cfg(test)` module so the workspace's unwrap/expect/panic-in-tests
// allowance (clippy.toml) covers the helpers too.
#[cfg(test)]
mod round_trip {
    use proptest::prelude::*;
    use rr_core::units::{
        ConvertError, Dimension, Fuel, UNITS, Unit, afr_to_lambda, convert, lambda_to_afr,
    };

    /// Absolute allowance for gradual underflow (module docs, "Tolerance").
    const UNDERFLOW_ABS: f64 = 1e-300;

    /// No value at or below this magnitude overflows in any registry
    /// conversion (module docs, "Overflow").
    const OVERFLOW_FLOOR: f64 = 1e290;

    /// Every ordered pair of distinct units sharing a dimension, from the
    /// registry.
    fn same_dimension_pairs() -> Vec<(&'static Unit, &'static Unit)> {
        UNITS
            .iter()
            .flat_map(|a| {
                UNITS
                    .iter()
                    .filter(move |b| b.dimension == a.dimension && b.id != a.id)
                    .map(move |b| (a, b))
            })
            .collect()
    }

    /// The largest magnitude any intermediate of the round trip `v` in `a`
    /// -> `b` -> `a` reaches, measured in `a`'s units (module docs, `S`).
    fn scale(v: f64, a: &Unit, b: &Unit) -> f64 {
        let (ta, tb) = (a.to_base, b.to_base);
        let k = (tb.mul / tb.div) / (ta.mul / ta.div);
        v.abs() + ta.offset.abs() + tb.offset.abs() * k
    }

    /// The allowed `|round_trip(v) - v|` (module docs, "Tolerance").
    fn tolerance(v: f64, a: &Unit, b: &Unit) -> f64 {
        8.0 * f64::EPSILON * scale(v, a, b) + UNDERFLOW_ABS
    }

    /// `v` in `a` -> `b` -> `a`, both legs accepted and within tolerance.
    fn assert_round_trip(v: f64, a: &Unit, b: &Unit) -> Result<(), TestCaseError> {
        let there = convert(v, a, b);
        prop_assert!(
            matches!(there, Ok(x) if x.is_finite()),
            "{v:e} {} -> {} gave {there:?}",
            a.id,
            b.id
        );
        let there = there.unwrap();
        let back = convert(there, b, a);
        prop_assert!(
            matches!(back, Ok(x) if x.is_finite()),
            "{v:e} {} -> {there:e} {} -> {back:?}",
            a.id,
            b.id
        );
        let back = back.unwrap();
        let (err, tol) = ((back - v).abs(), tolerance(v, a, b));
        prop_assert!(
            err <= tol,
            "{v:e} {} -> {there:e} {} -> {back:e}: error {err:e} > tolerance {tol:e}",
            a.id,
            b.id
        );
        Ok(())
    }

    /// One leg of an arbitrary-value round trip: a finite `Ok`, or the
    /// overflow refusal for a value beyond [`OVERFLOW_FLOOR`] (asserted).
    /// `None` when refused.
    fn leg(v: f64, original: f64, from: &Unit, to: &Unit) -> Result<Option<f64>, TestCaseError> {
        match convert(v, from, to) {
            Ok(x) => {
                prop_assert!(x.is_finite(), "{v:e} {} -> {} gave {x}", from.id, to.id);
                Ok(Some(x))
            }
            Err(ConvertError::NonFinite {
                value,
                from: f,
                to: t,
            }) => {
                prop_assert_eq!(value.to_bits(), v.to_bits());
                prop_assert_eq!((f, t), (from.id, to.id));
                prop_assert!(
                    original.abs() > OVERFLOW_FLOOR,
                    "{v:e} {} -> {} refused as non-finite, though {original:e} is not huge",
                    from.id,
                    to.id
                );
                Ok(None)
            }
            Err(e) => Err(TestCaseError::fail(format!(
                "{v:e} {} -> {}: unexpected {e:?}",
                from.id, to.id
            ))),
        }
    }

    #[test]
    fn pair_count_matches_the_registry() {
        let pairs = same_dimension_pairs();
        let want: usize = Dimension::ALL
            .iter()
            .map(|d| {
                let n = d.units().count();
                n * n.saturating_sub(1)
            })
            .sum();
        assert_eq!(pairs.len(), want);
        assert!(!pairs.is_empty());
        // The count the module docs state, for the registry as written. A
        // new unit changes it: update the docs' figure with it.
        assert_eq!(pairs.len(), 206);
        for (a, b) in &pairs {
            assert_eq!(a.dimension, b.dimension);
            assert_ne!(a.id, b.id);
        }
    }

    /// A finite magnitude `±10^e`, `e` in `[-100, 100]`.
    fn wide_magnitude() -> impl Strategy<Value = f64> {
        (-100.0_f64..=100.0, any::<bool>()).prop_map(|(e, neg)| {
            let m = 10.0_f64.powf(e);
            if neg { -m } else { m }
        })
    }

    /// Any finite `f64`: zero, subnormal or normal, either sign.
    fn any_finite() -> impl Strategy<Value = f64> {
        prop::num::f64::POSITIVE
            | prop::num::f64::NEGATIVE
            | prop::num::f64::NORMAL
            | prop::num::f64::SUBNORMAL
            | prop::num::f64::ZERO
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1024))]

        /// US-RR-21-2: every same-dimension pair (206 for the registry as
        /// written) round-trips within `8 ε S` for a point of the source
        /// unit's widened display range and for `±10^e`, `e` in
        /// `[-100, 100]`.
        #[test]
        fn every_same_dimension_pair_round_trips(
            t in 0.0_f64..=1.0,
            wide in wide_magnitude(),
        ) {
            let pairs = same_dimension_pairs();
            prop_assert!(!pairs.is_empty());
            for (a, b) in pairs {
                let d = a.display;
                let span = d.max - d.min;
                let in_range = (d.min - 2.0 * span) + t * (5.0 * span);
                assert_round_trip(in_range, a, b)?;
                assert_round_trip(wide, a, b)?;
            }
        }

        /// US-RR-21-2, arbitrary finite values: each leg is a finite `Ok`
        /// or, only beyond `1e290`, the `NonFinite` refusal (never a panic
        /// or an infinite value); a round trip both legs accept is within
        /// tolerance, subnormals included.
        #[test]
        fn extreme_values_round_trip_or_are_refused(v in any_finite()) {
            prop_assume!(v.is_finite());
            for (a, b) in same_dimension_pairs() {
                let Some(there) = leg(v, v, a, b)? else { continue };
                let Some(back) = leg(there, v, b, a)? else { continue };
                let (err, tol) = ((back - v).abs(), tolerance(v, a, b));
                prop_assert!(
                    err <= tol,
                    "{v:e} {} -> {there:e} {} -> {back:e}: error {err:e} > tolerance {tol:e}",
                    a.id,
                    b.id
                );
            }
        }

        /// US-RR-21-2, AFR <-> lambda for every fuel: `afr / stoich *
        /// stoich` (and `lambda * stoich / stoich`) is two roundings, so
        /// within `(2u + u²)|x| ≈ ε|x|` of `x`; `2 ε |x|` is allowed. Values
        /// are positive (the API refuses others) from a typical mixture
        /// range and from `10^e`, `e` in `[-300, 300]`, where neither leg
        /// can overflow or underflow (stoichiometric ratios are 6.47..34.3).
        #[test]
        fn afr_and_lambda_round_trip_per_fuel(
            typical in 0.05_f64..=40.0,
            e in -300.0_f64..=300.0,
        ) {
            let wide = 10.0_f64.powf(e);
            for fuel in Fuel::ALL {
                for x in [typical, wide] {
                    let lambda = afr_to_lambda(x, fuel);
                    prop_assert!(matches!(lambda, Ok(l) if l > 0.0 && l.is_finite()), "{fuel}: {x:e} -> {lambda:?}");
                    let back = lambda_to_afr(lambda.unwrap(), fuel);
                    prop_assert!(matches!(back, Ok(y) if (y - x).abs() <= 2.0 * f64::EPSILON * x),
                        "{fuel}: AFR {x:e} -> lambda -> {back:?}");

                    let afr = lambda_to_afr(x, fuel);
                    prop_assert!(matches!(afr, Ok(a) if a > 0.0 && a.is_finite()), "{fuel}: {x:e} -> {afr:?}");
                    let back = afr_to_lambda(afr.unwrap(), fuel);
                    prop_assert!(matches!(back, Ok(y) if (y - x).abs() <= 2.0 * f64::EPSILON * x),
                        "{fuel}: lambda {x:e} -> AFR -> {back:?}");
                }
            }
        }
    }
}
