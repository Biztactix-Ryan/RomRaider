//! US-RR-24-9 over the pinned definition corpus: every `to_real` formula
//! that is not linear is solved numerically, the verdict on whether it
//! can be (monotonic, no hole inside the range) matches a brute-force
//! check, and every solvable one round-trips over the raw ranges scalings
//! are stored in.
//!
//! The formulas are harvested at test time from the fetched corpus
//! (`RR_CORPUS_DIR`; [`rr_testkit::corpus::formulas`]), as
//! `tests/conversion_linear_corpus.rs` does; their text is never committed
//! or printed (D-028): output names a formula by its SHA-256 and first
//! location.
//!
//! * **Verdict.** Over `u8`, `i8`, `u16` and `i16` storage (exhaustive),
//!   an independent brute force over every raw value says whether the
//!   formula rises and falls, has a raw value with no real value between
//!   two that have one, or is constant; `Conversion::inverse` must refuse
//!   exactly those (`NotMonotonic`, `Gap`, `Constant`) and accept the rest.
//!   Over `u32` and `i32` (sampled) a refusal must name raw values that
//!   witness it.
//! * **Round trip.** For every accepted (formula, storage), at every raw
//!   value of `u8` and `i8` and at an even grid of 257 raw values plus the
//!   64 at each end of the others: `to_raw(to_real(r))` displays as `r`
//!   does, and is `r` itself unless a neighbour shares its real value.
//! * **Given `to_byte`.** No ECU scaling whose `expression` is not linear
//!   but has a `to_byte` is refused over unsigned 8-, 16- or 32-bit
//!   storage (the conversion module docs rely on this: refusing to fall
//!   back on `to_byte` costs no scaling legacy could edit there).

use std::collections::{BTreeMap, BTreeSet};

use rr_core::conversion::{Conversion, ConversionSpec, FormSpec, NotInvertible, Reject};
use rr_core::encoding::{Raw, RawRange, Storage};
use rr_testkit::corpus::formulas::{self, Formula, FormulaError, FormulaKind};

fn label(f: &Formula) -> String {
    let first = f.first();
    format!("sha256 {} (first {} {})", f.sha256, first.path, first.xpath)
}

fn build(text: &str) -> Option<Conversion> {
    Conversion::from_spec(&ConversionSpec {
        form: FormSpec::Expr {
            to_real: text,
            to_raw: None,
        },
        unit: None,
        format: None,
    })
    .ok()
}

/// What brute force says about a conversion over every raw value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Brute {
    Solvable,
    NotMonotonic,
    Gap,
    Constant,
    NoValue,
}

/// Every raw value of `storage`, in order: rises, falls, and holes with
/// values on both sides.
fn brute(c: &Conversion, storage: Storage) -> Brute {
    let (lo, hi) = storage.int_range().unwrap_or((0, -1));
    let values: Vec<Option<f64>> = (lo..=hi).map(|r| c.to_real_raw(Raw::Int(r)).ok()).collect();
    let first = values.iter().position(Option::is_some);
    let last = values.iter().rposition(Option::is_some);
    let (Some(first), Some(last)) = (first, last) else {
        return Brute::NoValue;
    };
    let run = &values[first..=last];
    if run.iter().any(Option::is_none) {
        return Brute::Gap;
    }
    let run: Vec<f64> = run.iter().flatten().copied().collect();
    let rises = run.windows(2).any(|w| w[1] > w[0]);
    let falls = run.windows(2).any(|w| w[1] < w[0]);
    match (rises, falls) {
        (true, true) => Brute::NotMonotonic,
        (false, false) => Brute::Constant,
        _ => Brute::Solvable,
    }
}

fn verdict(r: &Result<rr_core::conversion::Inverse<'_>, Reject>) -> Brute {
    match r {
        Ok(_) => Brute::Solvable,
        Err(Reject::NotInvertible(NotInvertible::NotMonotonic { .. })) => Brute::NotMonotonic,
        Err(Reject::NotInvertible(NotInvertible::Gap { .. })) => Brute::Gap,
        Err(Reject::Singular { .. }) => Brute::Constant,
        Err(_) => Brute::NoValue,
    }
}

/// The raw values a round trip is checked at.
fn raw_values(storage: Storage) -> Vec<i128> {
    let Some((lo, hi)) = storage.int_range() else {
        return Vec::new();
    };
    if hi - lo < 1 << 8 {
        return (lo..=hi).collect();
    }
    let span = hi - lo;
    let mut v: Vec<i128> = (0..=256).map(|k| lo + span * k / 256).collect();
    v.extend((0..64).map(|k| lo + k));
    v.extend((0..64).map(|k| hi - k));
    v.sort_unstable();
    v.dedup();
    v
}

/// Round-trip problems of `c` over `storage`, and how many raw values
/// shared their real value with a neighbour.
fn round_trip(c: &Conversion, storage: Storage, problems: &mut Vec<String>, at: &str) -> usize {
    let raw = storage.raw_range();
    let Ok(inv) = c.inverse(raw) else {
        return 0;
    };
    let mut shared = 0;
    for r in raw_values(storage) {
        let Ok(real) = c.to_real_raw(Raw::Int(r)) else {
            continue;
        };
        let neighbour_same = [r - 1, r + 1]
            .into_iter()
            .filter(|&n| raw.contains(Raw::Int(n)))
            .any(|n| c.to_real_raw(Raw::Int(n)) == Ok(real));
        let got = inv.to_raw_checked(real);
        let ok = if neighbour_same {
            shared += 1;
            got.is_ok_and(|g| c.to_real_raw(g) == Ok(real))
        } else {
            got == Ok(Raw::Int(r))
        };
        if !ok {
            problems.push(format!(
                "{at}: {storage:?} raw {r} (real {real}) solves to {got:?}"
            ));
            return shared;
        }
    }
    shared
}

/// A sampled refusal must name raw values that witness it.
fn witnessed(c: &Conversion, raw: RawRange, err: &Reject) -> bool {
    let value = |r: Raw| c.to_real_raw(r).ok();
    match *err {
        Reject::NotInvertible(NotInvertible::NotMonotonic { rise, fall }) => {
            value(rise).is_some() && value(fall).is_some()
        }
        Reject::NotInvertible(NotInvertible::Gap { at }) => {
            raw.contains(at.raw()) && value(at.raw()).is_none()
        }
        _ => true,
    }
}

/// A scaling's `expression` and `to_byte` texts, as far as found.
type Pair<'a> = (Option<&'a str>, Option<&'a str>);

/// What the corpus check found.
#[derive(Debug, Default)]
struct Counts {
    distinct: usize,
    nonlinear: usize,
    verdicts: BTreeMap<String, usize>,
    round_tripped: usize,
    shared: usize,
    pairs_nonlinear: usize,
    pairs_refused_unsigned: usize,
    pairs_refused_signed: usize,
}

const EXACT: [Storage; 4] = [Storage::U8, Storage::I8, Storage::U16, Storage::I16];
const SAMPLED: [Storage; 2] = [Storage::U32, Storage::I32];

rr_testkit::corpus_test! {
    /// Every non-linear corpus formula's solvability matches brute force,
    /// and every solvable one round-trips.
    fn nonlinear_corpus_formulas_solve_or_are_refused_honestly() -> Result<(), FormulaError> {
        let harvest = formulas::harvest()?;
        let mut n = Counts { distinct: harvest.formulas.len(), ..Counts::default() };
        let mut problems = Vec::new();
        for f in &harvest.formulas {
            if !f.kinds().iter().any(|k| *k != FormulaKind::EcuToByte) {
                continue;
            }
            let Some(c) = build(&f.text) else { continue };
            if c.form().linear().is_some() {
                continue;
            }
            n.nonlinear += 1;
            let at = label(f);
            for storage in EXACT {
                let inv = c.inverse(storage.raw_range());
                let (got, want) = (verdict(&inv), brute(&c, storage));
                *n.verdicts.entry(format!("{storage:?} {got:?}")).or_default() += 1;
                if got != want {
                    problems.push(format!("{at}: {storage:?} verdict {got:?}, brute force {want:?}"));
                }
            }
            for storage in SAMPLED {
                let raw = storage.raw_range();
                let inv = c.inverse(raw);
                *n.verdicts.entry(format!("{storage:?} {:?}", verdict(&inv))).or_default() += 1;
                if let Err(e) = &inv
                    && !witnessed(&c, raw, e)
                {
                    problems.push(format!("{at}: {storage:?} refused without a witness: {e:?}"));
                }
            }
            let mut any = false;
            for storage in EXACT.into_iter().chain(SAMPLED) {
                if c.inverse(storage.raw_range()).is_ok() {
                    any = true;
                    n.shared += round_trip(&c, storage, &mut problems, &at);
                }
            }
            n.round_tripped += usize::from(any);
        }
        // ECU scalings with both formulas whose expression is not linear.
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
        let with_to_byte: BTreeSet<&str> = scalings
            .values()
            .filter_map(|&(a, b)| b.and(a))
            .collect();
        for text in with_to_byte {
            let Some(c) = build(text) else { continue };
            if c.form().linear().is_some() {
                continue;
            }
            n.pairs_nonlinear += 1;
            let refused = |s: &[Storage]| s.iter().any(|s| c.inverse(s.raw_range()).is_err());
            if refused(&[Storage::U8, Storage::U16, Storage::U32]) {
                n.pairs_refused_unsigned += 1;
            }
            n.pairs_refused_signed += usize::from(refused(&[Storage::I8, Storage::I16, Storage::I32]));
        }
        eprintln!("corpus solved conversions: {n:?}");
        assert!(
            problems.is_empty(),
            "{} problem(s):\n{}",
            problems.len(),
            problems.join("\n")
        );
        assert!(n.nonlinear <= n.distinct, "{n:?}");
        assert_eq!(n.pairs_refused_unsigned, 0, "{n:?}");
        assert!(n.nonlinear >= 10, "suspiciously few non-linear formulas: {n:?}");
        assert!(n.round_tripped >= 10, "suspiciously few round trips: {n:?}");
        Ok(())
    }
}

/// The oracle itself, on formulas whose answer is known (public tier).
#[test]
fn the_brute_force_oracle_tells_the_cases_apart() {
    let c = |t: &str| build(t).unwrap();
    assert_eq!(brute(&c("x*x"), Storage::U8), Brute::Solvable);
    assert_eq!(brute(&c("x*x"), Storage::I8), Brute::NotMonotonic);
    assert_eq!(brute(&c("100/x"), Storage::I8), Brute::Gap);
    assert_eq!(brute(&c("100/x"), Storage::U8), Brute::Solvable);
    assert_eq!(brute(&c("x-x+1"), Storage::U8), Brute::Constant);
    assert_eq!(brute(&c("sqrt(x-1000)"), Storage::U8), Brute::NoValue);
    for (t, s) in [
        ("x*x", Storage::U8),
        ("x*x", Storage::I8),
        ("100/x", Storage::I8),
        ("x-x+1", Storage::U8),
        ("sqrt(x-1000)", Storage::U8),
    ] {
        let conv = c(t);
        assert_eq!(
            verdict(&conv.inverse(s.raw_range())),
            brute(&conv, s),
            "{t}"
        );
    }
    let mut problems = Vec::new();
    assert_eq!(
        round_trip(&c("x-x%4"), Storage::U8, &mut problems, "test"),
        256
    );
    assert_eq!(
        round_trip(&c("sqrt(x)"), Storage::U32, &mut problems, "test"),
        0
    );
    assert!(problems.is_empty(), "{problems:?}");
    let sq = c("x*x");
    let i8r = Storage::I8.raw_range();
    let err = sq.inverse(i8r).unwrap_err();
    assert!(witnessed(&sq, i8r, &err));
    assert!(!witnessed(
        &c("sqrt(x-1000)+x"),
        i8r,
        &Reject::NotInvertible(NotInvertible::NotMonotonic {
            rise: Raw::Int(1),
            fall: Raw::Int(2)
        })
    ));
}
