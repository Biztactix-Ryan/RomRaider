//! rr-core's expression parser and evaluator (`rr_core::expr`, US-RR-20)
//! against the expression known-answer fixtures (`[[expr]]` cases in
//! `fixtures/kat/*.toml`).
//!
//! US-RR-20-2: JEP operator precedence and `^` semantics match documented JEP
//! behaviour. `fixtures/kat/expr-jep-precedence.toml` holds the cases, each
//! citing the JEP 2.x documentation (or, for `^` values, `%` and rr-core's
//! native bit operators, a hand calculation from the rule its notes cite).
//!
//! US-RR-20-10: formulas of the pinned definition corpus evaluate to
//! hand-checked values. `fixtures/kat/expr-corpus.toml` names each formula
//! by the SHA-256 of its text (`formula_ref`; D-028: the definitions have no
//! licence, so the text stays in the fetched corpus), and
//! [`corpus_formula_cases_match_rr_core`] looks each one up in a fresh
//! harvest of `RR_CORPUS_DIR` ([`rr_testkit::corpus::formulas`]); without the
//! corpus it is skipped and only the fixture's shape is checked here.
//!
//! Every `[[expr]]` case of every fixture must parse with
//! [`rr_core::expr::parse`] and, evaluated at its `x` and `params`, give its
//! `value`:
//!
//! * a case without `tolerance` is exact, compared bit for bit (so `-0.0` is
//!   not `+0.0`);
//! * a case with `tolerance` is within that absolute tolerance;
//! * a `nan`, `inf` or `-inf` value is JEP's result, which rr-core reports as
//!   [`EvalError::NonFinite`] carrying it: NaN for NaN, the same infinity for
//!   an infinity;
//! * the formula's parameter references are exactly the case's `params`
//!   keys, bound by id.
//!
//! This file is a test boundary: the fixture types do not name rr-core.

use std::fs;
use std::path::PathBuf;

use rr_core::expr::{self, EvalError};
use rr_testkit::corpus::formulas::{self, FormulaKind};
use rr_testkit::fixtures::repo_root;
use rr_testkit::kat::{self, ExprCase, Formula, FormulaTier, KatKind};

/// The JEP fixture, and the fewest cases it may hold (so a truncated file
/// cannot pass quietly).
const JEP_FIXTURE: &str = "expr-jep-precedence.toml";
const JEP_MIN_CASES: usize = 100;

/// The corpus-formula fixture and the fewest hand-checked cases it may hold
/// (US-RR-20 asks for at least 50).
const CORPUS_FIXTURE: &str = "expr-corpus.toml";
const CORPUS_MIN_CASES: usize = 50;

/// `(fixture file name, case number from 1, case)` for every `[[expr]]` case.
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn expr_cases() -> Vec<(String, usize, ExprCase)> {
    let dir = repo_root().join("fixtures").join("kat");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    let mut out = Vec::new();
    for path in paths {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let fixture = kat::load(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        let KatKind::Expr { cases } = fixture.data.kind else {
            continue; // decode and checksum fixtures have no formulas to run
        };
        for (index, case) in cases.into_iter().enumerate() {
            out.push((name.clone(), index + 1, case));
        }
    }
    out
}

/// Why `case`, whose formula text is `text`, fails against rr-core, or
/// `None` when it passes.
fn mismatch(text: &str, case: &ExprCase) -> Option<String> {
    let parsed = match expr::parse(text) {
        Ok(parsed) => parsed,
        Err(error) => return Some(format!("does not parse: {error}")),
    };
    let mut params = Vec::with_capacity(parsed.params().len());
    for reference in parsed.params() {
        match case.params.get(reference.id()) {
            Some(&value) => params.push(value),
            None => {
                return Some(format!(
                    "has parameter reference {reference} but the case's `params` has no {}",
                    reference.id()
                ));
            }
        }
    }
    if let Some(extra) = case
        .params
        .keys()
        .find(|id| !parsed.params().iter().any(|r| r.id() == id.as_str()))
    {
        return Some(format!(
            "`params` binds {extra}, which the formula does not reference"
        ));
    }
    let got = parsed.eval(case.x, &params);
    let expected = case.value;
    let ok = match got {
        Ok(v) if expected.is_finite() => match case.tolerance {
            Some(tolerance) => (v - expected).abs() <= tolerance,
            None => v.to_bits() == expected.to_bits(),
        },
        Err(EvalError::NonFinite(v)) if expected.is_nan() => v.is_nan(),
        Err(EvalError::NonFinite(v)) if expected.is_infinite() => v == expected,
        _ => false,
    };
    (!ok).then(|| {
        let want = if expected.is_finite() {
            match case.tolerance {
                Some(tolerance) => format!("Ok({expected:?}) within {tolerance:e}"),
                None => format!("Ok({expected:?}) bit for bit"),
            }
        } else {
            format!("Err(NonFinite({expected:?}))")
        };
        format!(
            "at x = {:?}, params = {:?} gave {got:?}, expected {want}",
            case.x, case.params
        )
    })
}

#[test]
fn every_expr_case_parses_and_evaluates_to_its_known_answer() {
    let cases = expr_cases();
    let jep = cases
        .iter()
        .filter(|(name, ..)| name == JEP_FIXTURE)
        .count();
    assert!(
        jep >= JEP_MIN_CASES,
        "fixtures/kat/{JEP_FIXTURE} has {jep} [[expr]] case(s); at least {JEP_MIN_CASES} expected"
    );
    let mut references = 0;
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|(name, number, case)| {
            let Some(text) = case.formula.text() else {
                references += 1; // evaluated by corpus_formula_cases_match_rr_core
                return None;
            };
            mismatch(text, case).map(|why| format!("{name} [[expr]] #{number} `{text}`: {why}"))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} inline [[expr]] case(s) disagree with rr-core:\n{}",
        failures.len(),
        cases.len() - references,
        failures.join("\n")
    );
}

/// The corpus fixture holds only references (its formulas have no licence,
/// D-028), at least [`CORPUS_MIN_CASES`] of them, each to the public
/// corpus tier, and no reference elsewhere: checked without the corpus.
#[test]
fn corpus_fixture_names_its_formulas_by_hash_only() {
    let cases = expr_cases();
    let mut corpus = 0;
    for (name, number, case) in &cases {
        match (&case.formula, name == CORPUS_FIXTURE) {
            (Formula::Ref(reference), true) => {
                corpus += 1;
                assert_eq!(
                    reference.tier,
                    FormulaTier::Corpus,
                    "{name} [[expr]] #{number}: the public fixture refers to the corpus tier only"
                );
                let path = reference.path.to_string_lossy();
                assert!(
                    path.starts_with("defs/romraider/"),
                    "{name} [[expr]] #{number}: {path} is not in the definition tree"
                );
            }
            (Formula::Text(text), true) => panic!(
                "{name} [[expr]] #{number}: inline formula `{text}`; a corpus formula is never committed (D-028), use `formula_ref`"
            ),
            (Formula::Ref(_), false) => panic!(
                "{name} [[expr]] #{number}: a `formula_ref` outside {CORPUS_FIXTURE}; keep corpus formulas there"
            ),
            (Formula::Text(_), false) => {}
        }
    }
    assert!(
        corpus >= CORPUS_MIN_CASES,
        "fixtures/kat/{CORPUS_FIXTURE} has {corpus} case(s); at least {CORPUS_MIN_CASES} expected"
    );
}

rr_testkit::corpus_test! {
    /// US-RR-20-10: every `formula_ref` case names a formula of the pinned
    /// corpus that occurs at its `path` / `xpath`, and rr-core evaluates it
    /// to the hand-checked value. The cases also have to spread over what
    /// the corpus holds: ECU raw-to-real and real-to-raw scalings, logger
    /// `x` conversions and derived parameters, division by `x`, `if(...)`
    /// and `%`.
    fn corpus_formula_cases_match_rr_core() -> Result<(), formulas::FormulaError> {
        let harvest = formulas::harvest()?;
        let mut failures = Vec::new();
        let mut checked = Vec::new();
        for (name, number, case) in expr_cases() {
            let Formula::Ref(reference) = &case.formula else {
                continue;
            };
            let at = format!("{name} [[expr]] #{number} (`{}`)", case.formula);
            let Some(formula) = harvest.by_sha256(reference.sha256.as_str()) else {
                failures.push(format!("{at}: no formula of the pinned corpus has this sha256"));
                continue;
            };
            let path = reference.path.to_string_lossy().replace('\\', "/");
            if !formula.occurs_at(&path, &reference.xpath) {
                failures.push(format!(
                    "{at}: the formula does not occur at {path} {}; it first occurs at {} {}",
                    reference.xpath,
                    formula.first().path,
                    formula.first().xpath
                ));
                continue;
            }
            if let Some(why) = mismatch(&formula.text, &case) {
                failures.push(format!("{at}: {why}"));
            }
            checked.push(formula);
        }
        assert!(
            failures.is_empty(),
            "{} corpus [[expr]] case(s) fail:\n{}",
            failures.len(),
            failures.join("\n")
        );
        assert!(checked.len() >= CORPUS_MIN_CASES, "{} corpus case(s)", checked.len());
        let distinct: std::collections::BTreeSet<&str> =
            checked.iter().map(|f| f.sha256.as_str()).collect();
        let any = |what: &str, p: &dyn Fn(&formulas::Formula) -> bool| {
            assert!(checked.iter().any(|f| p(f)), "no corpus case covers {what}");
        };
        for kind in [FormulaKind::EcuExpression, FormulaKind::EcuToByte, FormulaKind::LoggerExpr] {
            any(kind.as_str(), &|f| f.kinds().contains(&kind));
        }
        any("a derived parameter (parameter references)", &|f| f.text.contains('['));
        any("if(...)", &|f| f.text.contains("if("));
        any("%", &|f| f.text.contains('%'));
        any("division by x", &|f| f.text.contains("/x") || f.text.contains("/(1+x"));
        eprintln!(
            "corpus formula KATs: {} case(s) over {} distinct corpus formula(s) agree with rr-core",
            checked.len(),
            distinct.len()
        );
        Ok(())
    }
}

/// The JEP fixture covers what US-RR-20-2 asks for: the whole precedence
/// ladder, `^` on both sides of a prefix minus, `%` signs, non-finite
/// results, and the native operators, so a later edit that drops a group is
/// caught here rather than in review.
#[test]
fn jep_fixture_covers_every_operator() {
    let cases: Vec<ExprCase> = expr_cases()
        .into_iter()
        .filter(|(name, ..)| name == JEP_FIXTURE)
        .map(|(_, _, case)| case)
        .collect();
    let has = |needle: &str| {
        cases
            .iter()
            .any(|c| c.formula.text().is_some_and(|t| t.contains(needle)))
    };
    for op in [
        "||", "&&", "==", "!=", "<=", ">=", " < ", " > ", "+", " - ", "*", "/", "%", "^", "!", "&",
        " | ", "<<", ">>", "mod(",
    ] {
        assert!(has(op), "no case uses {op:?}");
    }
    for formula in ["-2^2", "2^-2", "-2^-2", "(-2)^2", "2^3^2"] {
        assert!(
            cases.iter().any(|c| c.formula.text() == Some(formula)),
            "no case for {formula}"
        );
    }
    assert!(cases.iter().any(|c| c.value.is_nan()), "no NaN case");
    assert!(
        cases.iter().any(|c| c.value.is_infinite()),
        "no infinite case"
    );
    assert!(
        cases
            .iter()
            .any(|c| c.value == 0.0 && c.value.is_sign_negative()),
        "no -0.0 case"
    );
    assert!(
        cases.iter().any(|c| c.tolerance.is_some()),
        "no inexact case"
    );
}
