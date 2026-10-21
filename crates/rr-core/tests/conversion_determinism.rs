//! US-RR-24-5, the fifth acceptance criterion of US-RR-24: *the same input
//! always yields the same output regardless of call order (no
//! cache-dependent results).*
//!
//! Legacy kept one global, synchronised cache of parsed JEP formulas
//! (`util/JEPUtil.java`, MF-25), so what a formula gave could depend on
//! what had been parsed before it. A conversion here is a plain value, and
//! the per-module tests already ask one conversion one kind of question in
//! a shuffled order. This test asks the whole surface at once:
//!
//! * a pool of conversions, linear, linear and non-linear formulas (exact
//!   and solved inverses, a definition `to_raw`, a division by zero, a
//!   non-monotonic shape), lookup tables, and specs that are refused when
//!   built, plus generated linear factors;
//! * a generated list of operations over that pool, interleaving
//!   different conversions and different operations (`from_spec`,
//!   `to_real`, `to_real_raw`, `real_range`, `to_raw_checked`, `inverse`
//!   then its `to_raw_checked`, `to_raw_mismatch`) across every storage;
//! * every answer compared with the same call made in isolation, on a
//!   conversion built fresh for that one call on a thread of its own, after running the list
//!   in its generated order with each call made twice (first call against
//!   later call, which is what a memo would change), in a shuffled order on
//!   the same, already used, conversions, in reverse on clones, and from
//!   several threads at once, each in its own order over the shared pool.
//!
//! Answers are compared as text: `f64` `Debug` is the shortest text that
//! reads back to the same value, so two answers print alike exactly when
//! they are the same bits (up to a NaN's payload), and an error carrying a
//! NaN compares equal to itself, which `PartialEq` would not.

// In a `cfg(test)` module so the workspace's unwrap/expect/panic-in-tests
// allowance (clippy.toml) covers the helpers too.
#[cfg(test)]
mod determinism {
    use std::thread;

    use proptest::prelude::*;
    use rr_core::conversion::{Conversion, ConversionError, ConversionSpec, FormSpec};
    use rr_core::encoding::{Raw, RawRange, Storage};

    /// Every storage: exhaustive (up to 16 bits) and sampled integer
    /// ranges, and both floats.
    const STORAGES: [Storage; 13] = Storage::ALL;

    /// A falling table, a rising one, and one that is refused (raw values
    /// not strictly increasing).
    const LUT_UP: &[(f64, f64)] = &[(0.0, -40.0), (64.0, 10.0), (128.0, 60.0), (255.0, 120.0)];
    const LUT_DOWN: &[(f64, f64)] = &[(-100.0, 500.0), (0.0, 250.0), (100.0, 0.0)];
    const LUT_BAD: &[(f64, f64)] = &[(0.0, 1.0), (0.0, 2.0)];

    /// The fixed part of the pool.
    fn catalogue() -> Vec<ConversionSpec<'static>> {
        let expr = |to_real, to_raw| FormSpec::Expr { to_real, to_raw };
        [
            FormSpec::Linear { m: 0.1, b: -40.0 },
            FormSpec::Linear { m: -0.25, b: 100.0 },
            FormSpec::Linear { m: 0.0, b: 3.0 },
            FormSpec::Linear {
                m: f64::NAN,
                b: 0.0,
            },
            expr("x*0.1", None),
            expr("(x-40)/2", None),
            expr("100-x/2", None),
            expr("x/256*100", Some("x*256/100")),
            expr("x*2", Some("x/3")),
            expr("x*x/256", None),
            expr("sqrt(x)*4", None),
            expr("exp(x/50)", None),
            expr("1000/(x-10)", None),
            expr("x*x*x/1000", None),
            expr("sin(x/20)*100", None),
            expr("x-x%7", None),
            expr("x+y", None),
            expr("x*", None),
            FormSpec::Lut(LUT_UP),
            FormSpec::Lut(LUT_DOWN),
            FormSpec::Lut(LUT_BAD),
        ]
        .into_iter()
        .map(|form| ConversionSpec {
            form,
            unit: None,
            format: None,
        })
        .collect()
    }

    /// One call on one conversion of the pool (index into the pool) and,
    /// where it takes one, one storage (index into [`STORAGES`]).
    #[derive(Debug, Clone, Copy)]
    enum Op {
        Build(usize),
        ToReal(usize, f64),
        ToRealRaw(usize, usize, f64),
        Range(usize, usize),
        ToRaw(usize, usize, f64),
        ViaInverse(usize, usize, f64),
        Mismatch(usize, usize),
    }

    impl Op {
        const fn spec(self) -> usize {
            match self {
                Self::Build(i)
                | Self::ToReal(i, _)
                | Self::ToRealRaw(i, _, _)
                | Self::Range(i, _)
                | Self::ToRaw(i, _, _)
                | Self::ViaInverse(i, _, _)
                | Self::Mismatch(i, _) => i,
            }
        }
    }

    type Built = Result<Conversion, ConversionError>;

    /// The answer to `op`, asked of `built` (the conversion its spec gives),
    /// as text.
    fn answer(op: Op, spec: &ConversionSpec<'_>, built: &Built) -> String {
        if let Op::Build(_) = op {
            let fresh = Conversion::from_spec(spec);
            // Building again gives the conversion the pool holds.
            assert_eq!(format!("{fresh:?}"), format!("{built:?}"));
            return format!("{fresh:?}");
        }
        let Ok(c) = built else {
            return format!("unbuilt: {built:?}");
        };
        let raw = |s: usize| STORAGES[s].raw_range();
        match op {
            Op::Build(_) => unreachable!(),
            Op::ToReal(_, x) => format!("{:?}", c.to_real(x).map(f64::to_bits)),
            Op::ToRealRaw(_, s, pick) => {
                let r = raw_in(raw(s), pick);
                format!("{r:?} {:?}", c.to_real_raw(r).map(f64::to_bits))
            }
            Op::Range(_, s) => format!("{:?}", c.real_range(raw(s))),
            Op::ToRaw(_, s, t) => format!("{:?}", c.to_raw_checked(t, raw(s))),
            // The inverse's answer is the conversion's answer.
            Op::ViaInverse(_, s, t) => {
                format!("{:?}", c.inverse(raw(s)).and_then(|i| i.to_raw_checked(t)))
            }
            Op::Mismatch(_, s) => match c.form() {
                rr_core::conversion::Form::Expr(e) => format!("{:?}", e.to_raw_mismatch(raw(s))),
                _ => "not a formula".to_owned(),
            },
        }
    }

    /// The raw value at fraction `pick` (0..1) of `range`.
    fn raw_in(range: RawRange, pick: f64) -> Raw {
        match range {
            RawRange::Int(r) => {
                let (lo, hi) = (r.min(), r.max());
                #[expect(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
                let off = ((hi - lo) as f64 * pick) as i128;
                Raw::Int((lo + off).clamp(lo, hi))
            }
            #[expect(clippy::cast_possible_truncation)]
            RawRange::F32 { min, max } => {
                Raw::F32(f64::from(min).mul_add(1.0 - pick, f64::from(max) * pick) as f32)
            }
            RawRange::F64 { min, max } => Raw::F64(min.mul_add(1.0 - pick, max * pick)),
        }
    }

    /// The same call asked once, alone, of a conversion built for it, on a
    /// thread of its own, so that not even thread-local state is shared
    /// with any other call.
    fn isolated(op: Op, specs: &[ConversionSpec<'_>]) -> String {
        let spec = &specs[op.spec()];
        thread::scope(|scope| {
            scope
                .spawn(|| answer(op, spec, &Conversion::from_spec(spec)))
                .join()
                .unwrap()
        })
    }

    /// A deterministic shuffle of `0..n` from `seed` (xorshift64).
    fn shuffled(n: usize, seed: u64) -> Vec<usize> {
        let mut order: Vec<usize> = (0..n).collect();
        let mut s = seed | 1;
        for k in (1..n).rev() {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            order.swap(k, usize::try_from(s % (k as u64 + 1)).unwrap());
        }
        order
    }

    /// Generated linear specs added to the catalogue.
    fn linears() -> impl Strategy<Value = Vec<ConversionSpec<'static>>> {
        let m = prop_oneof![-1e3_f64..1e3, -1.0_f64..1.0, Just(1.0), Just(-1.0)];
        prop::collection::vec((m, -1e4_f64..1e4), 0..4).prop_map(|v| {
            v.into_iter()
                .map(|(m, b)| ConversionSpec {
                    form: FormSpec::Linear { m, b },
                    unit: None,
                    format: None,
                })
                .collect()
        })
    }

    /// Real values: everyday ones, wide ones, and the awkward ones.
    fn value() -> impl Strategy<Value = f64> {
        prop_oneof![
            4 => -300.0_f64..300.0,
            2 => -1e7_f64..1e7,
            1 => prop::sample::select(vec![
                0.0,
                -0.0,
                f64::NAN,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::MAX,
                f64::MIN_POSITIVE,
                1e300,
            ]),
        ]
    }

    /// A pool and a list of operations over it. The operations draw their
    /// values from a short list, so one value is asked of many conversions
    /// and storages: a cache keyed on only part of a call's inputs (the
    /// formula text, the value, the storage) then answers one call with
    /// another's result.
    fn scenario() -> impl Strategy<Value = (Vec<ConversionSpec<'static>>, Vec<Op>)> {
        (linears(), prop::collection::vec(value(), 1..5)).prop_flat_map(|(extra, values)| {
            let mut specs = catalogue();
            specs.extend(extra);
            let n = specs.len();
            let s = STORAGES.len();
            let v = || prop::sample::select(values.clone());
            let op = prop_oneof![
                1 => (0..n).prop_map(Op::Build),
                1 => (0..n, v()).prop_map(|(i, x)| Op::ToReal(i, x)),
                1 => (0..n, 0..s, 0.0_f64..=1.0).prop_map(|(i, s, p)| Op::ToRealRaw(i, s, p)),
                1 => (0..n, 0..s).prop_map(|(i, s)| Op::Range(i, s)),
                3 => (0..n, 0..s, v()).prop_map(|(i, s, t)| Op::ToRaw(i, s, t)),
                1 => (0..n, 0..s, v()).prop_map(|(i, s, t)| Op::ViaInverse(i, s, t)),
                1 => (0..n, 0..s).prop_map(|(i, s)| Op::Mismatch(i, s)),
            ];
            (Just(specs), prop::collection::vec(op, 1..24))
        })
    }

    const THREADS: usize = 4;

    proptest! {
        #![proptest_config(ProptestConfig { cases: 32, max_shrink_iters: 32, ..ProptestConfig::default() })]

        /// Every call, in any order, on used, cloned or shared conversions,
        /// from any thread, answers as it does alone on a fresh one.
        #[test]
        fn answers_do_not_depend_on_call_order(
            (specs, ops) in scenario(),
            seed in any::<u64>(),
        ) {
            let expected: Vec<String> = ops.iter().map(|&op| isolated(op, &specs)).collect();
            let pool: Vec<Built> = specs.iter().map(Conversion::from_spec).collect();
            let ask = |k: usize, pool: &[Built]| {
                let op = ops[k];
                answer(op, &specs[op.spec()], &pool[op.spec()])
            };

            // In order, each call twice: a first call against a later one.
            for k in 0..ops.len() {
                prop_assert_eq!(&ask(k, &pool), &expected[k], "first call {:?}", ops[k]);
                prop_assert_eq!(&ask(k, &pool), &expected[k], "second call {:?}", ops[k]);
            }
            // Shuffled, on the same conversions, now used.
            for k in shuffled(ops.len(), seed) {
                prop_assert_eq!(&ask(k, &pool), &expected[k], "shuffled {:?}", ops[k]);
            }
            // Reversed, on clones of the used conversions.
            let clones = pool.clone();
            for k in (0..ops.len()).rev() {
                prop_assert_eq!(&ask(k, &clones), &expected[k], "clone {:?}", ops[k]);
            }
            // Several threads at once over the shared pool, each in its own
            // order.
            let answers: Vec<Vec<(usize, String)>> = thread::scope(|scope| {
                let handles: Vec<_> = (0..THREADS)
                    .map(|t| {
                        let (pool, ask) = (&pool, &ask);
                        let order = shuffled(ops.len(), seed.rotate_left(u32::try_from(t).unwrap() * 8 + 1));
                        scope.spawn(move || order.into_iter().map(|k| (k, ask(k, pool))).collect())
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap()).collect()
            });
            for (k, got) in answers.into_iter().flatten() {
                prop_assert_eq!(&got, &expected[k], "threaded {:?}", ops[k]);
            }
        }
    }

    /// The pool covers what it claims: refused specs, an exact and a
    /// solved inverse, a non-monotonic refusal, a division by zero, and a
    /// definition `to_raw` that disagrees.
    #[test]
    fn the_catalogue_covers_each_kind() {
        let specs = catalogue();
        let built: Vec<Built> = specs.iter().map(Conversion::from_spec).collect();
        assert!(built.iter().filter(|b| b.is_err()).count() >= 4);
        let u16r = Storage::U16.raw_range();
        let ok = |i: usize| built[i].as_ref().unwrap();
        // `(x-40)/2` is linear: inverted from its coefficients.
        assert!(ok(5).form().linear().is_some());
        // `x*x/256` is solved.
        assert!(ok(9).form().linear().is_none());
        assert!(ok(9).to_raw_checked(4.0, u16r).is_ok());
        // `sin(x/20)*100` does not invert.
        assert!(ok(14).inverse(u16r).is_err());
        // `1000/(x-10)` has no real value at 10.
        assert!(ok(12).to_real(10.0).is_err());
        // `x*2` with `to_raw` `x/3` is caught.
        let rr_core::conversion::Form::Expr(e) = ok(8).form() else {
            panic!()
        };
        assert!(e.to_raw_mismatch(u16r).is_some());
    }
}
