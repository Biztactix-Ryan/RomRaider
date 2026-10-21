//! US-RR-24-8 over the pinned definition corpus: every formula that is
//! syntactically linear is recognised as linear, and every linear `to_real`
//! formula inverts exactly (`to_raw(to_real(r)) == r`) over the raw ranges
//! scalings are stored in.
//!
//! The formulas are harvested at test time from the fetched corpus
//! (`RR_CORPUS_DIR`; [`rr_testkit::corpus::formulas`]), as
//! `tests/expr_corpus.rs` does; their text is never committed or printed
//! (D-028): output names a formula by its SHA-256 and first location.
//!
//! * **Detection.** The oracle is independent of rr-core's detector: a
//!   formula is *syntactically linear* when its text uses only digits,
//!   `.`, `e`/`E`, `x`, `+ - * /`, parentheses and spaces, and it is
//!   *numerically linear* when its value at ten raw values lies on the
//!   line through its values at 0 and 1 (to `1e-9` relative). Every formula
//!   that is both must be detected; every detected formula must be
//!   numerically linear, with the detected `m` and `b` matching that line.
//! * **Round trip.** Each detected `to_real` formula (ECU `expression`,
//!   logger `expr`) with a non-zero factor is inverted at every raw value of
//!   `u8` and `i8` storage, and at an even grid of 257 raw values (ends
//!   included) plus the 64 values at each end of `u16`, `i16`, `u32` and
//!   `i32` storage (a debug build; the property tests in rr-core cover the
//!   rest of the ranges). Where a neighbouring raw value has the same real
//!   value (`f64` cannot tell them apart), the raw value chosen must display
//!   the same instead.
//! * **Given `to_byte`.** Every distinct (`expression`, `to_byte`) pair of
//!   an ECU scaling is checked with `ExprForm::to_raw_mismatch` over `u8`
//!   and `u32`. At the pin, no pair whose `to_real` is linear disagrees
//!   (so the exact inverse writes what legacy's `to_byte` wrote, wherever
//!   legacy's was right); pairs with a non-linear `to_real` are counted
//!   only.

use std::collections::{BTreeMap, BTreeSet};

use rr_core::conversion::{Conversion, ConversionSpec, Form, FormSpec};
use rr_core::encoding::{Raw, Storage};
use rr_core::expr;
use rr_testkit::corpus::formulas::{self, Formula, FormulaError, FormulaKind};

/// Raw values the numeric oracle evaluates at, besides 0 and 1.
const PROBES: [f64; 10] = [
    2.0, 3.0, 5.0, 10.0, 77.0, 100.0, 255.0, 1000.0, -7.0, 65535.0,
];

/// The line `(m, b)` through the formula's values at 0 and 1, if every
/// probe lies on it.
fn numerically_linear(e: &expr::Expr) -> Option<(f64, f64)> {
    let f = |x: f64| e.eval(x, &[]).ok();
    let b = f(0.0)?;
    let m = f(1.0)? - b;
    for x in PROBES {
        let want = m * x + b;
        let got = f(x)?;
        let scale = want.abs().max(m.abs() * x.abs()).max(b.abs()).max(1.0);
        if (got - want).abs() > 1e-9 * scale {
            return None;
        }
    }
    Some((m, b))
}

fn syntactically_simple(text: &str) -> bool {
    text.bytes()
        .all(|c| c.is_ascii_digit() || b".eExX+-*/() ".contains(&c))
}

fn label(f: &Formula) -> String {
    let first = f.first();
    format!("sha256 {} (first {} {})", f.sha256, first.path, first.xpath)
}

/// The raw values a round trip is checked at.
fn raw_values(storage: Storage) -> Vec<i128> {
    let Some((lo, hi)) = storage.int_range() else {
        return Vec::new();
    };
    if hi - lo < 1 << 8 {
        return (lo..=hi).collect();
    }
    // The 64 values at each end, and an even grid of 257 (ends included).
    let span = hi - lo;
    let mut v: Vec<i128> = (0..=256).map(|k| lo + span * k / 256).collect();
    v.extend((0..64).map(|k| lo + k));
    v.extend((0..64).map(|k| hi - k));
    v.sort_unstable();
    v.dedup();
    v
}

/// Problems found inverting `c` over `storage`, and how many raw values
/// shared their real value with a neighbour.
fn round_trip(c: &Conversion, storage: Storage, problems: &mut Vec<String>, at: &str) -> usize {
    let raw = storage.raw_range();
    let mut shared = 0;
    for r in raw_values(storage) {
        let Ok(real) = c.to_real_raw(Raw::Int(r)) else {
            continue;
        };
        let neighbour_same = [r - 1, r + 1]
            .into_iter()
            .filter(|&n| raw.contains(Raw::Int(n)))
            .any(|n| c.to_real_raw(Raw::Int(n)) == Ok(real));
        let got = c.to_raw_checked(real, raw);
        let ok = if neighbour_same {
            shared += 1;
            got.is_ok_and(|g| c.to_real_raw(g) == Ok(real))
        } else {
            got == Ok(Raw::Int(r))
        };
        if !ok {
            problems.push(format!(
                "{at}: {storage:?} raw {r} (real {real}) inverts to {got:?}"
            ));
            return shared;
        }
    }
    shared
}

/// A scaling's `expression` and `to_byte` texts, as far as found.
type Pair<'a> = (Option<&'a str>, Option<&'a str>);

/// What the corpus check found.
#[derive(Debug, Default)]
struct Counts {
    distinct: usize,
    parsed: usize,
    simple_linear: usize,
    detected: usize,
    detected_constant: usize,
    detected_not_simple: usize,
    numerically_linear_not_detected: usize,
    round_tripped: usize,
    shared: usize,
    pairs: usize,
    pairs_linear: usize,
    mismatch_u8: usize,
    mismatch_u32: usize,
    mismatch_nonlinear: usize,
}

rr_testkit::corpus_test! {
    /// Every syntactically linear corpus formula is detected, every
    /// detection is right, and every linear `to_real` inverts exactly.
    fn linear_corpus_formulas_are_detected_and_invert_exactly() -> Result<(), FormulaError> {
        let harvest = formulas::harvest()?;
        let mut n = Counts { distinct: harvest.formulas.len(), ..Counts::default() };
        let mut problems = Vec::new();
        for f in &harvest.formulas {
            let Ok(e) = expr::parse(&f.text) else { continue };
            n.parsed += 1;
            let at = label(f);
            let numeric = if e.params().is_empty() { numerically_linear(&e) } else { None };
            let simple = syntactically_simple(&f.text);
            // Detection, through the conversion API (parameters refused).
            let built = Conversion::from_spec(&ConversionSpec {
                form: FormSpec::Expr { to_real: &f.text, to_raw: None },
                unit: None,
                format: None,
            });
            let detected = built.as_ref().ok().and_then(|c| c.form().linear());
            if simple && numeric.is_some() {
                n.simple_linear += 1;
                if detected.is_none() {
                    problems.push(format!("{at}: syntactically linear but not detected"));
                }
            }
            match (detected, numeric) {
                (Some(l), Some((m, b))) => {
                    n.detected += 1;
                    n.detected_not_simple += usize::from(!simple);
                    let close = |a: f64, z: f64| (a - z).abs() <= 1e-9 * a.abs().max(z.abs()).max(1.0);
                    if !close(l.m(), m) || !close(l.b(), b) {
                        problems.push(format!(
                            "{at}: detected m = {}, b = {}; its values give m = {m}, b = {b}",
                            l.m(),
                            l.b()
                        ));
                    }
                }
                (Some(l), None) => problems.push(format!(
                    "{at}: detected as linear (m = {}, b = {}) but its values are not",
                    l.m(),
                    l.b()
                )),
                (None, Some(_)) => {
                    n.numerically_linear_not_detected += 1;
                    eprintln!("numerically linear, not detected (not syntactically linear): {at}");
                }
                (None, None) => {}
            }
            // Round trip, for the raw → real formulas.
            let to_real = f.kinds().iter().any(|k| *k != FormulaKind::EcuToByte);
            let (Some(l), Ok(c)) = (detected, &built) else { continue };
            if l.m() == 0.0 {
                n.detected_constant += 1;
                continue;
            }
            if !to_real {
                continue;
            }
            n.round_tripped += 1;
            for storage in [Storage::U8, Storage::I8, Storage::U16, Storage::I16, Storage::U32, Storage::I32] {
                n.shared += round_trip(c, storage, &mut problems, &at);
            }
        }
        // ECU scalings with both formulas: (path, scaling element) -> texts.
        let mut scalings: BTreeMap<(&str, &str), Pair<'_>> = BTreeMap::new();
        for f in &harvest.formulas {
            for o in &f.occurrences {
                let (slot, element) = match o.kind {
                    FormulaKind::EcuExpression => (0, o.xpath.strip_suffix("/@expression")),
                    FormulaKind::EcuToByte => (1, o.xpath.strip_suffix("/@to_byte")),
                    FormulaKind::LoggerExpr => continue,
                };
                let Some(element) = element else { continue };
                let entry = scalings.entry((o.path.as_str(), element)).or_default();
                if slot == 0 { entry.0 = Some(&f.text) } else { entry.1 = Some(&f.text) }
            }
        }
        let pairs: BTreeSet<(&str, &str)> = scalings
            .values()
            .filter_map(|&(a, b)| Some((a?, b?)))
            .collect();
        n.pairs = pairs.len();
        for (to_real, to_raw) in pairs {
            let Ok(c) = Conversion::from_spec(&ConversionSpec {
                form: FormSpec::Expr { to_real, to_raw: Some(to_raw) },
                unit: None,
                format: None,
            }) else { continue };
            let Form::Expr(e) = c.form() else { continue };
            let mismatch = |raw: Storage| usize::from(e.to_raw_mismatch(raw.raw_range()).is_some());
            if e.linear().is_some_and(|l| l.m() != 0.0) {
                n.pairs_linear += 1;
                n.mismatch_u8 += mismatch(Storage::U8);
                n.mismatch_u32 += mismatch(Storage::U32);
            } else {
                n.mismatch_nonlinear += mismatch(Storage::U8).max(mismatch(Storage::U32));
            }
        }
        eprintln!("corpus linear conversions: {n:?}");
        assert!(
            problems.is_empty(),
            "{} problem(s):\n{}",
            problems.len(),
            problems.join("\n")
        );
        assert!(n.parsed <= n.distinct);
        assert_eq!((n.mismatch_u8, n.mismatch_u32), (0, 0), "{n:?}");
        assert!(n.detected >= 100, "suspiciously few linear formulas: {n:?}");
        assert!(n.round_tripped >= 100, "suspiciously few round trips: {n:?}");
        Ok(())
    }
}

/// The oracle itself, on formulas whose answer is known (public tier).
#[test]
fn the_oracle_tells_linear_from_not() {
    let lin = |t: &str| numerically_linear(&expr::parse(t).unwrap());
    assert_eq!(lin("x*0.25-50"), Some((0.25, -50.0)));
    assert_eq!(lin("100-x/2"), Some((-0.5, 100.0)));
    assert_eq!(lin("x*x"), None);
    assert_eq!(lin("1/x"), None);
    assert_eq!(lin("abs(x)"), None);
    assert!(syntactically_simple("(x-40)*1e-3/2"));
    assert!(!syntactically_simple("exp(x)"));
    assert!(!syntactically_simple("x^2"));
    // The round-trip sample: every u8 value; ends and a grid for u32.
    assert_eq!(raw_values(Storage::U8).len(), 256);
    let u32s = raw_values(Storage::U32);
    assert_eq!(u32s.first(), Some(&0));
    assert_eq!(u32s.last(), Some(&4_294_967_295));
    let mut problems = Vec::new();
    let c = Conversion::from_spec(&ConversionSpec {
        form: FormSpec::Expr {
            to_real: "x*0.5-10",
            to_raw: None,
        },
        unit: None,
        format: None,
    })
    .unwrap();
    assert_eq!(round_trip(&c, Storage::U16, &mut problems, "test"), 0);
    assert!(problems.is_empty(), "{problems:?}");
}
