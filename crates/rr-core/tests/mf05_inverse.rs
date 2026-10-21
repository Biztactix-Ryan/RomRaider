//! US-RR-24-3, the third acceptance criterion of US-RR-24 (MF-05): *a
//! non-monotonic conversion with no `to_raw` is rejected for editing with a
//! diagnostic; 4-byte storage inverse never overflows.*
//!
//! Legacy's failure, `maps/Scale.java:79-140` (`approximateToByteFunction`,
//! called by `maps/DataCell.java:441` when a scaling has no `to_byte`): a
//! binary search that
//!
//! * assumes the formula is monotonic, so a formula that rises and falls
//!   was silently given *a* raw value, whichever half the search wandered
//!   into; and
//! * works in Java `int`s: `(int) Math.pow(2, 32)` saturates to
//!   `2^31 - 1`, the first step `(int) (2^32 / 2)` saturates too, and
//!   `currentStep + stepSize` then wraps negative. Over `u32` it never
//!   reaches a raw value above about `2^30`; over `i32` none beyond about
//!   `±2^30`. `DataCell.java:447-449` then casts the (long) rounded result
//!   to an `int`, so a `to_byte` answer above `2^31 - 1` wrapped as well.
//!
//! Half 1 (refused, with a diagnostic) is checked over generated
//! non-monotonic shapes on 1- to 4-byte storage (exhaustive and sampled
//! scans), and, where the scan is sampled, by the documented guarantee of
//! the check after the bisection: a turn within two raw values of the
//! answer is always caught. Half 2 (never overflows) is checked at the
//! ends of 4- and 8-byte storage for exact and solved inverses, huge and
//! tiny factors, offsets that put the raw estimate past the storage, and
//! decreasing scalings, plus the legacy inputs themselves as named
//! regressions. The tests run under the test profile, which keeps
//! `overflow-checks` on, so an overflowing integer step would panic here.

// In a `cfg(test)` module so the workspace's unwrap/expect/panic-in-tests
// allowance (clippy.toml) covers the helpers too.
#[cfg(test)]
mod mf05 {
    use proptest::prelude::*;
    use rr_core::conversion::{
        Conversion, ConversionSpec, Coverage, Form, FormSpec, Inverse, Linear, Monotonicity,
        NonFinite, NotInvertible, Reject,
    };
    use rr_core::diag::{Provenance, Severity, Span, codes};
    use rr_core::encoding::{Raw, RawRange, Storage};

    fn conv(to_real: &str, to_raw: Option<&str>) -> Conversion {
        Conversion::from_spec(&ConversionSpec {
            form: FormSpec::Expr { to_real, to_raw },
            unit: None,
            format: None,
        })
        .unwrap()
    }

    fn lin(m: f64, b: f64) -> Conversion {
        Conversion::new(Form::Linear(Linear::new(m, b).unwrap()), None, None)
    }

    fn range(s: Storage) -> RawRange {
        s.raw_range()
    }

    fn ends(s: Storage) -> (i128, i128) {
        s.int_range().unwrap()
    }

    fn int(c: &Conversion, target: f64, raw: RawRange) -> Result<i128, Reject> {
        c.to_raw_checked(target, raw).map(|r| match r {
            Raw::Int(v) => v,
            other => panic!("expected an integer, got {other:?}"),
        })
    }

    fn real(c: &Conversion, r: i128) -> f64 {
        c.to_real_raw(Raw::Int(r)).unwrap()
    }

    fn here() -> Provenance {
        Provenance::new("defs.xml", Span::line_col(12, 5))
    }

    fn raw_int(r: Raw) -> i128 {
        match r {
            Raw::Int(v) => v,
            other => panic!("expected an integer, got {other:?}"),
        }
    }

    /// `err` is the not-monotonic refusal, reported as an editor sees it:
    /// conversion-wide (editing is disabled, not just this value), an
    /// `Error` diagnostic `CNV0006` whose message is the reason and names
    /// the raw values where the conversion rises and falls, both inside
    /// `raw` and with a real value. Returns `(rise, fall)`.
    fn assert_not_monotonic(c: &Conversion, err: &Reject, raw: RawRange) -> (Raw, Raw) {
        let Reject::NotInvertible(NotInvertible::NotMonotonic { rise, fall }) = *err else {
            panic!("expected NotMonotonic, got {err:?}")
        };
        assert!(raw.contains(rise) && raw.contains(fall), "{err:?}");
        assert_ne!(rise, fall);
        assert!(c.to_real_raw(rise).is_ok() && c.to_real_raw(fall).is_ok());
        assert!(err.is_conversion_wide());
        assert_eq!(err.code(), codes::NOT_INVERTIBLE);
        let d = err.to_diagnostic(here());
        assert_eq!(d.severity(), Severity::Error);
        assert!(d.is_error());
        assert_eq!(d.code().as_str(), "CNV0006");
        assert_eq!(d.provenance(), &here());
        assert_eq!(d.message(), err.to_string());
        let show = |r: Raw| match r {
            Raw::Int(v) => v.to_string(),
            Raw::F32(v) => v.to_string(),
            Raw::F64(v) => v.to_string(),
        };
        let m = d.message();
        assert!(m.starts_with("the conversion is not monotonic"), "{m}");
        assert!(
            m.contains(&format!("rises at raw value {}", show(rise))),
            "{m}"
        );
        assert!(
            m.contains(&format!("falls at raw value {}", show(fall))),
            "{m}"
        );
        assert!(m.contains("more than one raw value"), "{m}");
        (rise, fall)
    }

    // -----------------------------------------------------------------------
    // Half 1: a non-monotonic conversion is refused for editing
    // -----------------------------------------------------------------------

    /// A non-monotonic `to_real` over integers `lo ..= hi`, of a shape
    /// definitions could hold: a parabola, a `V` built with `abs`, a cubic
    /// with two interior turns, a sine over whole periods, a bump. `turn` is
    /// in `(0, 1)`: where the (first) turn lies across the range. Returns the
    /// formula and the raw values `a < b < c` it turns across (`b` above or
    /// below both), so the generator itself is checked to be non-monotonic.
    fn shape(kind: u8, lo: i128, hi: i128, turn: f64, k: f64) -> (String, [i128; 3]) {
        let span = hi - lo;
        let at = |f: f64| lo + (span as f64 * f) as i128;
        let c = at(turn);
        match kind {
            0 => (format!("(x-({c}))^2*{k:?}"), [lo, c, hi]),
            1 => (format!("{k:?}-abs(x-({c}))"), [lo, c, hi]),
            2 => {
                let (a, m, b) = (at(0.2), at(0.5), at(0.8));
                (
                    format!("(x-({a}))*(x-({m}))*(x-({b}))/{k:?}"),
                    [a, at(0.35), m],
                )
            }
            3 => {
                // Two whole periods: p = span / (4 pi), turning at a
                // quarter and three quarters of each.
                let p = span as f64 / (4.0 * core::f64::consts::PI);
                (
                    format!("sin((x-({lo}))/{p:?})*{k:?}"),
                    [lo, at(0.125), at(0.25)],
                )
            }
            _ => {
                let w = (span as f64 / 8.0).max(1.0);
                (format!("{k:?}/(1+((x-({c}))/{w:?})^2)"), [lo, c, hi])
            }
        }
    }

    const STORAGES: [Storage; 8] = [
        Storage::U8,
        Storage::I8,
        Storage::U16,
        Storage::I16,
        Storage::U24,
        Storage::I24,
        Storage::U32,
        Storage::I32,
    ];

    /// The criterion, by hand, on every storage up to 4 bytes (exhaustive
    /// up to 2 bytes, sampled above): each shape, with no `to_raw`, is
    /// refused for any value (in range, out of range, not a number) with the
    /// same `NotMonotonic` diagnostic, before a value is looked at.
    #[test]
    fn non_monotonic_conversion_without_to_raw_is_refused_with_a_diagnostic_on_every_storage() {
        for s in STORAGES {
            let raw = range(s);
            let (lo, hi) = ends(s);
            for kind in 0..5 {
                let (text, [a, b, c]) = shape(kind, lo, hi, 0.5, 3.0);
                let conversion = conv(&text, None);
                let (fa, fb, fc) = (
                    real(&conversion, a),
                    real(&conversion, b),
                    real(&conversion, c),
                );
                assert!(
                    (fb > fa && fb > fc) || (fb < fa && fb < fc),
                    "{text} over {s:?} is not non-monotonic: {fa} {fb} {fc}"
                );
                let err = conversion.inverse(raw).unwrap_err();
                assert_not_monotonic(&conversion, &err, raw);
                for target in [fa, fb, fc, (fa + fb) / 2.0, 1e300, -1e300, f64::NAN] {
                    assert_eq!(
                        conversion.to_raw_checked(target, raw),
                        Err(err),
                        "{text} over {s:?} at {target}"
                    );
                }
            }
        }
    }

    /// The same over floats: `x*x` falls to 0 and rises again; `-abs(x-1)`
    /// rises to 1 and falls again.
    #[test]
    fn non_monotonic_conversion_over_float_storage_is_refused_with_a_diagnostic() {
        for s in [Storage::F32, Storage::F64] {
            let raw = range(s);
            for text in ["x*x", "-abs(x-1)"] {
                let c = conv(text, None);
                let err = c.to_raw_checked(4.0, raw).unwrap_err();
                assert_not_monotonic(&c, &err, raw);
                assert_eq!(c.inverse(raw).unwrap_err(), err, "{text} over {s:?}");
            }
        }
    }

    /// A `to_raw` the definition gives does not stand in: which raw value
    /// it picks among those with the same real value cannot be checked to
    /// be the one, so the refusal is the same with or without it.
    #[test]
    fn a_given_to_raw_does_not_make_a_non_monotonic_conversion_editable() {
        for s in [Storage::U8, Storage::U32] {
            let raw = range(s);
            let (lo, hi) = ends(s);
            let c = (lo + hi) / 2;
            let without = conv(&format!("(x-({c}))^2"), None);
            let with = conv(&format!("(x-({c}))^2"), Some(&format!("sqrt(x)+({c})")));
            let err = without.to_raw_checked(100.0, raw).unwrap_err();
            assert_not_monotonic(&without, &err, raw);
            assert_eq!(with.to_raw_checked(100.0, raw), Err(err), "{s:?}");
        }
    }

    proptest! {
        // Each case scans a raw range of up to 2^32 values (sampled).
        #![proptest_config(ProptestConfig { cases: 64, max_shrink_iters: 16, ..ProptestConfig::default() })]

        /// For generated non-monotonic shapes, turning anywhere inside the
        /// range, over 1- to 4-byte storage, with or without a `to_raw`:
        /// every target (a real value the cell holds, one it cannot, a
        /// non-number) is refused with the one conversion-wide
        /// `NotMonotonic` diagnostic.
        #[test]
        fn non_monotonic_conversions_are_refused_for_editing_with_a_diagnostic(
            s in prop::sample::select(STORAGES.to_vec()),
            kind in 0_u8..5,
            turn in 0.1_f64..0.9,
            k in prop_oneof![0.001_f64..10.0, 1.0_f64..1e6],
            to_raw in prop::option::of(prop::sample::select(vec!["x", "sqrt(x)", "x*2+1"])),
            pick in any::<u64>(),
        ) {
            let raw = range(s);
            let (lo, hi) = ends(s);
            let (text, [a, b, c]) = shape(kind, lo, hi, turn, k);
            let conversion = conv(&text, to_raw);
            let (fa, fb, fc) = (real(&conversion, a), real(&conversion, b), real(&conversion, c));
            prop_assume!((fb > fa && fb > fc) || (fb < fa && fb < fc));
            let err = conversion.inverse(raw).unwrap_err();
            assert_not_monotonic(&conversion, &err, raw);
            let span = u128::try_from(hi - lo).unwrap() + 1;
            let r = lo + i128::try_from(u128::from(pick) % span).unwrap();
            let held = real(&conversion, r);
            for target in [held, fb, fa.max(fc) * 2.0 + 1e9, f64::NAN] {
                prop_assert_eq!(conversion.to_raw_checked(target, raw), Err(err), "{} at {}", text, target);
            }
        }
    }

    /// `x`, except that over `N+1 ..= N+w` it runs back down from `N-1` to
    /// `N-w` (negated when `!up`): a turn `w` raw values wide, far narrower
    /// than the sample's spacing over `u32` (about 2^18).
    fn wiggle(n: i128, w: i128, up: bool) -> Conversion {
        let body = format!("if((x>{n})&&(x<={}),2*(x-{n}),0)", n + w);
        let text = if up {
            format!("x-{body}")
        } else {
            format!("-x+{body}")
        };
        conv(&text, None)
    }

    /// What the check after a sampled bisection guarantees (see "What a
    /// sampled scan guarantees" in the `conversion` module docs): a written
    /// raw value `r` has a real value, the raw values `r-2 ..= r+2` (inside
    /// the range) move the conversion's way, and `r` is the nearest of them
    /// to `target`; a refusal names a raw value where the conversion really
    /// does turn. Returns whether `target` was refused.
    fn check_around_the_answer(
        c: &Conversion,
        inv: &Inverse<'_>,
        raw: RawRange,
        target: f64,
        up: bool,
        wiggle: (i128, i128),
    ) -> bool {
        let f = |i: i128| real(c, i);
        let (lo, hi) = (wiggle.0 + 1, wiggle.0 + wiggle.1);
        let got = inv.to_raw_checked(target);
        match got {
            Ok(r) => {
                let r = raw_int(r);
                let window: Vec<i128> = (r - 2..=r + 2)
                    .filter(|&i| raw.contains(Raw::Int(i)))
                    .collect();
                for pair in window.windows(2) {
                    let (p, q) = (f(pair[0]), f(pair[1]));
                    assert!(
                        if up { q >= p } else { q <= p },
                        "{target}: wrote {r}, but the conversion turns between {} and {}",
                        pair[0],
                        pair[1]
                    );
                }
                let dist = |i: i128| (f(i) - target).abs();
                for &i in &window {
                    assert!(dist(r) <= dist(i), "{target}: wrote {r}, {i} is nearer");
                }
                false
            }
            Err(Reject::NotInvertible(NotInvertible::NotMonotonic { rise, fall })) => {
                // The turn reported from around the answer is inside the
                // wiggle and real: against the trend, from the raw value
                // before it.
                let turn = raw_int(if up { fall } else { rise });
                assert!((lo..=hi).contains(&turn), "{target}: turn at {turn}");
                let (p, q) = (f(turn - 1), f(turn));
                assert!(
                    if up { q < p } else { q > p },
                    "{target}: no turn at {turn}"
                );
                true
            }
            Err(e) => panic!("{target}: unexpected {e:?}"),
        }
    }

    /// A turn the `u32` sample cannot see, one raw value wide, at
    /// `N = 3_000_000_001` (on no grid point or landmark): the scan reports
    /// the conversion monotonic, and every value whose answer lands within
    /// two raw values of the turn is refused with `NotMonotonic` naming it.
    #[test]
    fn a_turn_narrower_than_the_sample_is_caught_around_the_answer() {
        let u32r = range(Storage::U32);
        let n = 3_000_000_001_i128;
        for up in [true, false] {
            let c = wiggle(n, 1, up);
            let scan = c.real_range(u32r).unwrap();
            assert_eq!(scan.coverage(), Coverage::Sampled);
            let trend = if up {
                Monotonicity::Increasing
            } else {
                Monotonicity::Decreasing
            };
            assert_eq!(scan.monotonicity(), trend, "the sample saw the turn");
            let inv = c.inverse(u32r).unwrap();
            let s = if up { 1.0 } else { -1.0 };
            // N-1 is the real value of both N-1 and N+1; N that of N and
            // N+2 is N+2: every value from N-1.5 to N+0.5 is answered by
            // a raw value within two of the turn at N+1.
            let n = n as f64;
            for t in [n - 1.5, n - 1.0, n - 0.75, n - 0.5, n - 0.25, n, n + 0.25] {
                let target = s * t;
                assert!(
                    check_around_the_answer(&c, &inv, u32r, target, up, (3_000_000_001, 1)),
                    "{target} (up: {up}) was written"
                );
                let err = c.to_raw_checked(target, u32r).unwrap_err();
                assert_not_monotonic(&c, &err, u32r);
            }
            // Away from the turn, answers are exact.
            for r in [0_i128, 1, 2_999_999_990, 3_000_000_010, 4_294_967_295] {
                assert_eq!(int(&c, real(&c, r), u32r), Ok(r), "{r} (up: {up})");
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 64, max_shrink_iters: 16, ..ProptestConfig::default() })]

        /// Turns 1 to 3 raw values wide anywhere in `u32` or `i32`, rising
        /// or falling, that the sample missed: every value near the turn is
        /// either refused with `NotMonotonic` naming a raw value inside it,
        /// or written as a raw value whose two neighbours each side move the
        /// conversion's way and are no nearer (the guarantee: a turn within
        /// two raw values of the answer is never written across). Some value
        /// is always refused.
        #[test]
        fn a_narrow_turn_the_sample_missed_is_never_written_across(
            signed in any::<bool>(),
            at in 0.01_f64..0.99,
            w in 1_i128..=3,
            up in any::<bool>(),
        ) {
            let s = if signed { Storage::I32 } else { Storage::U32 };
            let raw = range(s);
            let (lo, hi) = ends(s);
            let n = lo + ((hi - lo) as f64 * at) as i128;
            let c = wiggle(n, w, up);
            let scan = c.real_range(raw).unwrap();
            prop_assume!(scan.monotonicity() != Monotonicity::NonMonotonic);
            prop_assert_eq!(scan.coverage(), Coverage::Sampled);
            let inv = c.inverse(raw).unwrap();
            let sign = if up { 1.0 } else { -1.0 };
            let mut refused = 0;
            for q in -(4 * (w + 3))..=(4 * 3) {
                let target = sign * (n as f64 + q as f64 / 4.0);
                if check_around_the_answer(&c, &inv, raw, target, up, (n, w)) {
                    refused += 1;
                }
            }
            prop_assert!(refused > 0, "nothing near the turn at {} was refused", n);
        }
    }

    // -----------------------------------------------------------------------
    // Half 2: 4-byte storage inverse never overflows (MF-05)
    // -----------------------------------------------------------------------

    /// MF-05 regression, legacy input 1: a non-linear 4-byte scaling with
    /// no `to_byte`, so `Scale.approximateToByteFunction` searched for it.
    /// Its `int` search started at `2^30 - 1` with a saturated step of
    /// `2^31 - 1`, so the first `currentStep + stepSize` wrapped to about
    /// `-2^30`; simulated with Java `int` arithmetic, the value of raw
    /// 3_000_000_000 (27000000000) came back as about 1_073_741_792, and
    /// every raw value above 2^31 (and over `i32`, beyond about `±2^30`)
    /// was out of its reach. Here each is found exactly, and a step past
    /// either end is refused, not wrapped.
    #[test]
    fn mf05_regression_legacy_four_byte_search_wrapped_in_int() {
        let cube = conv("x*x*x/1e18", None);
        let u32r = range(Storage::U32);
        assert_eq!(int(&cube, 27_000_000_000.0, u32r), Ok(3_000_000_000));
        for r in [
            0_i128,
            1,
            1_073_741_823,
            1_073_741_824,
            2_147_483_647,
            2_147_483_648,
            2_147_483_649,
            3_000_000_000,
            4_294_967_294,
            4_294_967_295,
        ] {
            assert_eq!(int(&cube, real(&cube, r), u32r), Ok(r), "u32 raw {r}");
        }
        let top = real(&cube, 4_294_967_295);
        let step = top - real(&cube, 4_294_967_294);
        assert!(matches!(
            cube.to_raw_checked(top + step, u32r),
            Err(Reject::OutOfRange { nearest: Raw::Int(4_294_967_295), max, .. }) if max == top
        ));
        let i32r = range(Storage::I32);
        for r in [
            -2_147_483_648_i128,
            -2_147_483_647,
            -1_073_741_825,
            -1,
            0,
            1_073_741_824,
            2_147_483_646,
            2_147_483_647,
        ] {
            assert_eq!(int(&cube, real(&cube, r), i32r), Ok(r), "i32 raw {r}");
        }
        let bottom = real(&cube, -2_147_483_648);
        let step = real(&cube, -2_147_483_647) - bottom;
        assert!(matches!(
            cube.to_raw_checked(bottom - step, i32r),
            Err(Reject::OutOfRange { nearest: Raw::Int(-2_147_483_648), min, .. }) if min == bottom
        ));
    }

    /// MF-05 regression, legacy input 2: a 4-byte scaling with a `to_byte`.
    /// `DataCell.setRealValue` cast `Math.round(to_byte(v))` to an `int`, so
    /// 3000000 with `x*0.001` / `x*1000` became `(int) 3000000000L`,
    /// -1294967296, and 4294967.296 (one step past the top) became 0. The
    /// `to_byte` is not used to write; the inverse of `to_real` is exact
    /// over the whole range, and the step past the top is refused.
    #[test]
    fn mf05_regression_legacy_to_byte_result_cast_to_int() {
        let c = conv("x*0.001", Some("x*1000"));
        let u32r = range(Storage::U32);
        assert_eq!(int(&c, 3_000_000.0, u32r), Ok(3_000_000_000));
        assert_eq!(int(&c, 4_294_967.295, u32r), Ok(4_294_967_295));
        assert!(matches!(
            c.to_raw_checked(4_294_967.296, u32r),
            Err(Reject::OutOfRange {
                nearest: Raw::Int(4_294_967_295),
                ..
            })
        ));
        let i32r = range(Storage::I32);
        assert_eq!(int(&c, -2_147_483.648, i32r), Ok(-2_147_483_648));
        assert!(matches!(
            c.to_raw_checked(2_147_483.648, i32r),
            Err(Reject::OutOfRange {
                nearest: Raw::Int(2_147_483_647),
                ..
            })
        ));
    }

    /// The ends of 4-byte storage under scalings that stress the raw
    /// arithmetic: identity and its negation, a huge factor, a tiny one, an
    /// offset that puts `(t - b) / m` billions of raw values past either
    /// end. Every raw value at and next to an end comes back exactly; one
    /// raw step past an end is refused at that end, the range the right way
    /// round, never wrapped to the other end or clamped.
    #[test]
    fn four_byte_ends_invert_exactly_and_a_step_past_is_refused() {
        let cases: [(f64, f64); 10] = [
            (1.0, 0.0),
            (-1.0, 0.0),
            (1e9, 0.0),
            (-1e9, 5.0),
            (1e-9, 0.0),
            (-1e-9, 1.0),
            (1.0, -3e9),
            (1.0, 3e9),
            (-1.0, 5e9),
            (0.5, -1e10),
        ];
        for s in [Storage::U32, Storage::I32] {
            let raw = range(s);
            let (lo, hi) = ends(s);
            for (m, b) in cases {
                for c in [lin(m, b), conv(&format!("x*{m:?}+{b:?}"), None)] {
                    for r in [lo, lo + 1, lo + 2, hi - 2, hi - 1, hi] {
                        assert_eq!(int(&c, real(&c, r), raw), Ok(r), "{s:?} {m}x+{b} raw {r}");
                    }
                    let (low, high) = (real(&c, lo), real(&c, hi));
                    let (min, max) = (low.min(high), low.max(high));
                    // One raw step past each end, in real terms.
                    let past_max = max + m.abs();
                    let past_min = min - m.abs();
                    let at_max = if m > 0.0 { hi } else { lo };
                    let at_min = if m > 0.0 { lo } else { hi };
                    assert_eq!(
                        c.to_raw_checked(past_max, raw),
                        Err(Reject::OutOfRange {
                            target: past_max,
                            min,
                            max,
                            nearest: Raw::Int(at_max)
                        }),
                        "{s:?} {m}x+{b} above"
                    );
                    assert_eq!(
                        c.to_raw_checked(past_min, raw),
                        Err(Reject::OutOfRange {
                            target: past_min,
                            min,
                            max,
                            nearest: Raw::Int(at_min)
                        }),
                        "{s:?} {m}x+{b} below"
                    );
                    // And far past: the raw estimate is beyond i64.
                    for far in [1e300, -1e300, f64::MAX, -f64::MAX] {
                        assert!(
                            matches!(c.to_raw_checked(far, raw), Err(Reject::OutOfRange { nearest, .. }) if nearest == Raw::Int(if far > 0.0 { at_max } else { at_min })),
                            "{s:?} {m}x+{b} at {far}"
                        );
                    }
                }
            }
        }
    }

    /// A factor so large the conversion overflows inside the 4-byte range:
    /// `x*2e300` is infinite from raw 89884657. Values up to the last finite
    /// one are found; `f64::MAX`, 0.74 of a raw step past it (and 0.6 past it), is refused as
    /// not finite (the next raw value has no real value), never written as
    /// a wrapped or clamped raw value. With `x*1e300` `f64::MAX` is only
    /// 0.49 of a step past the last finite value, so by the half-step rule
    /// it is that raw value.
    #[test]
    fn a_four_byte_factor_that_overflows_is_refused_past_the_last_finite_value() {
        let u32r = range(Storage::U32);
        for c in [lin(2e300, 0.0), conv("x*2e300", None)] {
            assert_eq!(int(&c, 1e308, u32r), Ok(50_000_000));
            let last = 89_884_656_i128;
            assert!(c.to_real(last as f64).is_ok());
            assert!(c.to_real((last + 1) as f64).is_err());
            assert_eq!(int(&c, real(&c, last), u32r), Ok(last));
            for t in [f64::MAX, real(&c, last) + 1.2e300] {
                assert!(
                    matches!(
                        c.to_raw_checked(t, u32r),
                        Err(Reject::NonFinite(NonFinite::Beyond { raw: Raw::Int(r), .. })) if r == last
                    ),
                    "{t}: {:?}",
                    c.to_raw_checked(t, u32r)
                );
            }
        }
        for c in [lin(1e300, 0.0), conv("x*1e300", None)] {
            assert_eq!(int(&c, f64::MAX, u32r), Ok(179_769_313));
            assert!(c.to_real(179_769_314.0).is_err());
        }
    }

    /// A decreasing solved (non-linear) scaling over 4-byte storage:
    /// `-sqrt(x)` over `u32` runs from 0 at raw 0 down to about -65536 at
    /// the top; the cube `-x*x*x/1e18` over `i32` from about 9.9e9 down to
    /// -9.9e9. Ends exact, a step past either refused at the right end.
    #[test]
    fn a_decreasing_solved_four_byte_scaling_inverts_at_its_ends() {
        for (text, s) in [("-sqrt(x)", Storage::U32), ("-x*x*x/1e18", Storage::I32)] {
            let c = conv(text, None);
            let raw = range(s);
            let (lo, hi) = ends(s);
            for r in [lo, lo + 1, lo + 2, -1, 0, 1, hi - 2, hi - 1, hi] {
                if r < lo {
                    continue;
                }
                assert_eq!(int(&c, real(&c, r), raw), Ok(r), "{text} raw {r}");
            }
            let (max, min) = (real(&c, lo), real(&c, hi));
            let up = real(&c, lo) - real(&c, lo + 1);
            let down = real(&c, hi - 1) - real(&c, hi);
            assert!(
                matches!(
                    c.to_raw_checked(max + up, raw),
                    Err(Reject::OutOfRange { nearest, max: m, .. }) if nearest == Raw::Int(lo) && m == max
                ),
                "{text} above"
            );
            assert!(
                matches!(
                    c.to_raw_checked(min - down, raw),
                    Err(Reject::OutOfRange { nearest, min: m, .. }) if nearest == Raw::Int(hi) && m == min
                ),
                "{text} below"
            );
        }
    }

    /// A linear factor `m` of either sign from `1e-12` to `1e12`.
    fn factor() -> impl Strategy<Value = f64> {
        (any::<bool>(), -12_i32..=12, 1.0_f64..10.0)
            .prop_map(|(neg, e, mant)| if neg { -1.0 } else { 1.0 } * mant * 10_f64.powi(e))
    }

    /// The raw-value shift an offset `b` corresponds to: none, a few, or
    /// enough to put the estimate past `i32`, `u32` or `i64`.
    fn shift() -> impl Strategy<Value = f64> {
        prop::sample::select(vec![
            0.0,
            17.0,
            -17.0,
            2_147_483_648.0,
            -2_147_483_649.0,
            4_294_967_296.0,
            -4_294_967_296.0,
            3e9,
            -3e9,
            9.3e18,
            -9.3e18,
        ])
    }

    fn pick_raw(lo: i128, hi: i128, zone: u8, n: u64) -> i128 {
        let span = u128::try_from(hi - lo).unwrap() + 1;
        let k = i128::from(n % 4);
        match zone % 3 {
            0 => lo + k,
            1 => hi - k,
            _ => lo + i128::try_from(u128::from(n) % span).unwrap(),
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        /// Linear scalings over 4- and 8-byte storage, huge and tiny
        /// factors, offsets that move the estimate past the storage, raw
        /// values at the ends: a raw value's real value comes back as that
        /// raw value (or, where f64 cannot tell neighbours apart, one that
        /// displays the same), and any finite target is either written as a
        /// raw value inside the range that no neighbour beats, or refused
        /// consistently. Nothing panics, so no integer step overflows
        /// (`overflow-checks` are on in tests).
        #[test]
        fn four_byte_linear_inverse_never_overflows_or_wraps(
            s in prop::sample::select(vec![Storage::U32, Storage::I32, Storage::U64, Storage::I64]),
            m in factor(),
            shift in shift(),
            zone in any::<u8>(),
            n in any::<u64>(),
            target in prop_oneof![
                any::<f64>().prop_filter("finite", |t| t.is_finite()),
                -1e22_f64..1e22,
                -1e10_f64..1e10,
            ],
            expr in any::<bool>(),
        ) {
            let raw = range(s);
            let (lo, hi) = ends(s);
            let b = -m * shift;
            let c = if expr { conv(&format!("x*{m:?}+{b:?}"), None) } else { lin(m, b) };
            let r = pick_raw(lo, hi, zone, n);
            let v = real(&c, r);
            let got = raw_int(c.to_raw_checked(v, raw).unwrap());
            prop_assert!((lo..=hi).contains(&got));
            prop_assert_eq!(real(&c, got), v, "{:?} {}x+{} raw {}", s, m, b, r);
            let shared = [r - 1, r + 1]
                .into_iter()
                .filter(|i| (lo..=hi).contains(i))
                .any(|i| real(&c, i) == v);
            if !shared {
                prop_assert_eq!(got, r, "{:?} {}x+{}", s, m, b);
            }
            match c.to_raw_checked(target, raw) {
                Ok(g) => {
                    let g = raw_int(g);
                    prop_assert!((lo..=hi).contains(&g));
                    let d = |i: i128| (real(&c, i) - target).abs();
                    for i in [g - 1, g + 1] {
                        if (lo..=hi).contains(&i) {
                            prop_assert!(d(g) <= d(i), "{} beats {} for {}", i, g, target);
                        }
                    }
                }
                Err(Reject::OutOfRange { target: t, min, max, nearest }) => {
                    prop_assert_eq!(t.to_bits(), target.to_bits());
                    prop_assert!(target < min || target > max);
                    let end = raw_int(nearest);
                    prop_assert!(end == lo || end == hi, "{} is not an end", end);
                    let (low, high) = (real(&c, lo), real(&c, hi));
                    prop_assert_eq!((min, max), (low.min(high), low.max(high)));
                    prop_assert_eq!(real(&c, end), if target > max { max } else { min });
                }
                Err(e) => prop_assert!(false, "{:?} {}x+{} at {}: {:?}", s, m, b, target, e),
            }
        }
    }
}
