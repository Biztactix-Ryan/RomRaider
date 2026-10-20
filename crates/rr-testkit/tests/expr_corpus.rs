//! US-RR-20-10 (PROJECT.md T2, story US-RR-20): every distinct formula of
//! the pinned definition corpus parses with `rr_core::expr`.
//!
//! The formulas are harvested at test time from the fetched corpus
//! (`RR_CORPUS_DIR`; [`rr_testkit::corpus::formulas`]): every ECU
//! `<scaling expression|to_byte>` and every logger `<conversion expr>`, the
//! v290 logger files of the pinned tree included. Their text is never
//! committed (D-028); `fixtures/corpus-formulas.toml` records, per distinct
//! formula, its SHA-256, occurrence count, kinds and first location, and
//! [`corpus_formula_metadata_is_current`] keeps that file in step with the
//! corpus. Output names formulas by hash and location only, so CI logs do
//! not publish them either.
//!
//! The corpus-tier tests are skipped (reported as `...::requires_RR_CORPUS_DIR`)
//! without the corpus; the public tests check the committed metadata's shape,
//! that the parse check itself refuses an unparseable formula and a stale
//! exclusion (on a synthetic harvest), and that a missing v370 logger package
//! is reported as skipped.

use std::collections::BTreeSet;

use rr_core::expr;
use rr_testkit::corpus::formulas::{
    self, FormulaError, FormulaKind, Harvest, METADATA_PATH, harvest_xml, sha256_hex,
};
use rr_testkit::fixtures::repo_root;

/// Corpus formulas that are not in the supported dialect, by SHA-256 of
/// their text, each with why. Every other distinct formula must parse, and
/// every formula listed here must exist in the corpus and fail to parse, so
/// the list cannot go stale.
///
/// At the pin only one formula fails, and it is a definition bug, not a gap
/// in the dialect: so the parser is right to refuse it.
const EXCLUDED: &[(&str, &str)] = &[(
    "bb02ec9a9755b63b578e170b9e06add03514b72d2b4a390119f2e34f253d0384",
    "an ECU `to_byte` (5 occurrences, first defs/romraider/ecu/RR_AE5W100V.xml \
     /roms[1]/rom[2]/table[143]/scaling[1]/@to_byte) with unbalanced parentheses: \
     26 bytes, two `(` and three `)`, the expression complete before the last \
     `)` (byte 25). rr-core refuses it with EXP0001 at that `)`. JEP's grammar \
     has no production for a trailing `)` either (Start ::= Expression \
     (<EOF> | <SEMI>)), so legacy never evaluated it: its parseExpression \
     records a ParseException and leaves nothing to evaluate. A definition \
     fix belongs upstream (TD-D/SubaruDefs), not in the parser.",
)];

/// What [`check`] found.
struct Checked {
    /// Distinct formulas that parse (and are not excluded).
    parsed: usize,
    /// Excluded formulas, which do not parse.
    refused: usize,
    /// Everything wrong, one line each, naming formulas by hash and location.
    problems: Vec<String>,
}

/// Parses every distinct formula of `harvest`: each must parse unless
/// `excluded` lists its SHA-256, and then it must not; every `excluded` entry
/// must be in the harvest; an ECU scaling must reference nothing but `x`.
fn check(harvest: &Harvest, excluded: &[(&str, &str)]) -> Checked {
    let excluded: BTreeSet<&str> = excluded.iter().map(|(sha, _)| *sha).collect();
    let mut problems = Vec::new();
    let (mut parsed, mut refused) = (0_usize, 0_usize);
    for f in &harvest.formulas {
        let first = f.first();
        let at = format!(
            "sha256 {} ({} occurrence(s), first {} {})",
            f.sha256,
            f.occurrences.len(),
            first.path,
            first.xpath
        );
        match (expr::parse(&f.text), excluded.contains(f.sha256.as_str())) {
            (Ok(e), false) => {
                parsed += 1;
                let ecu = f.kinds().iter().any(|k| *k != FormulaKind::LoggerExpr);
                if ecu && !e.params().is_empty() {
                    problems.push(format!(
                        "{at}: an ECU scaling with {} parameter reference(s); a scaling has only `x`",
                        e.params().len()
                    ));
                }
            }
            (Ok(_), true) => problems.push(format!(
                "{at}: listed in EXCLUDED but parses; remove it from the list"
            )),
            (Err(_), true) => refused += 1,
            (Err(error), false) => problems.push(format!(
                "{at}: does not parse: {} at bytes {:?}",
                error.code().as_str(),
                error.span().range()
            )),
        }
    }
    for sha in excluded {
        if harvest.by_sha256(sha).is_none() {
            problems.push(format!(
                "EXCLUDED lists sha256 {sha}, which is not in the pinned corpus"
            ));
        }
    }
    Checked {
        parsed,
        refused,
        problems,
    }
}

rr_testkit::corpus_test! {
    /// Every distinct formula parses, except the [`EXCLUDED`] ones, which
    /// must not; an ECU scaling references nothing but `x`.
    fn every_corpus_formula_parses() -> Result<(), FormulaError> {
        let harvest = formulas::harvest()?;
        let corpus = rr_testkit::corpus::Corpus::open()?;
        eprintln!("{}", formulas::logger_v370_status(corpus.dir()));
        let Checked { parsed, refused, problems } = check(&harvest, EXCLUDED);
        eprintln!(
            "corpus formulas: {} distinct ({} ecu-expression, {} ecu-to_byte, {} logger-expr) \
             in {} occurrence(s) across {} file(s): {parsed} parse, {refused} excluded",
            harvest.formulas.len(),
            harvest.distinct_of(FormulaKind::EcuExpression),
            harvest.distinct_of(FormulaKind::EcuToByte),
            harvest.distinct_of(FormulaKind::LoggerExpr),
            harvest.occurrences(),
            harvest.files()
        );
        assert!(
            problems.is_empty(),
            "{} problem(s) with the corpus formulas:\n{}",
            problems.len(),
            problems.join("\n")
        );
        assert_eq!(parsed + refused, harvest.formulas.len());
        assert!(harvest.formulas.len() >= 700, "a truncated harvest");
        Ok(())
    }

    /// `fixtures/corpus-formulas.toml` is what a fresh harvest renders.
    fn corpus_formula_metadata_is_current() -> Result<(), FormulaError> {
        let fresh = formulas::harvest()?.render_metadata();
        let committed = formulas::committed_metadata()?;
        if fresh != committed {
            let line = fresh
                .lines()
                .zip(committed.lines())
                .position(|(a, b)| a != b)
                .map_or_else(
                    || fresh.lines().count().min(committed.lines().count()) + 1,
                    |i| i + 1,
                );
            panic!(
                "{METADATA_PATH} is out of date (first difference at line {line}); regenerate it: \
                 RR_CORPUS_DIR=<dir> cargo run -p rr-testkit --example gen-corpus-formulas"
            );
        }
        Ok(())
    }
}

/// The committed metadata holds metadata only (D-028): every `[[formulas]]`
/// entry has exactly `sha256`, `occurrences`, `kinds` and `first`
/// (`path`, `xpath`), so formula text cannot be added to it unnoticed; the
/// hashes are sorted and distinct; the summary agrees with the entries; and
/// every [`EXCLUDED`] formula is in it.
#[test]
fn committed_metadata_is_metadata_only_and_consistent() {
    let text = std::fs::read_to_string(repo_root().join(METADATA_PATH)).unwrap();
    let table: toml::Table = toml::from_str(&text).unwrap();
    assert_eq!(table["schema"].as_integer(), Some(1));
    let summary = table["summary"].as_table().unwrap();
    let entries = table["formulas"].as_array().unwrap();
    let mut hashes = Vec::new();
    let mut occurrences = 0;
    let mut by_kind = std::collections::BTreeMap::<String, i64>::new();
    for entry in entries {
        let entry = entry.as_table().unwrap();
        let keys: Vec<&str> = entry.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            ["first", "kinds", "occurrences", "sha256"],
            "{entry:?}"
        );
        let sha = entry["sha256"].as_str().unwrap();
        assert!(
            sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
            "{sha}"
        );
        hashes.push(sha.to_owned());
        let n = entry["occurrences"].as_integer().unwrap();
        assert!(n >= 1);
        occurrences += n;
        for kind in entry["kinds"].as_array().unwrap() {
            *by_kind
                .entry(kind.as_str().unwrap().to_owned())
                .or_default() += 1;
        }
        let first = entry["first"].as_table().unwrap();
        let keys: Vec<&str> = first.keys().map(String::as_str).collect();
        assert_eq!(keys, ["path", "xpath"]);
        assert!(
            first["path"]
                .as_str()
                .unwrap()
                .starts_with("defs/romraider/")
        );
        let xpath = first["xpath"].as_str().unwrap();
        assert!(
            xpath.starts_with('/') && xpath.contains("/@") && !xpath.contains(['"', '\'', '=']),
            "{xpath}: a positional XPath, no predicates on content"
        );
    }
    assert!(hashes.windows(2).all(|w| w[0] < w[1]), "sorted, distinct");
    assert_eq!(summary["distinct"].as_integer(), Some(entries.len() as i64));
    assert_eq!(summary["occurrences"].as_integer(), Some(occurrences));
    for (kind, n) in by_kind {
        let key = format!("distinct_{}", kind.replace('-', "_"));
        assert_eq!(summary[key.as_str()].as_integer(), Some(n), "{key}");
    }
    for (sha, reason) in EXCLUDED {
        assert!(
            hashes.iter().any(|h| h == sha),
            "EXCLUDED {sha} not in the metadata"
        );
        assert!(!reason.trim().is_empty());
    }
}

/// [`check`], which [`every_corpus_formula_parses`] runs over the corpus,
/// fails on a synthetic harvest: an unparseable formula that is not
/// excluded, an excluded formula that parses, an exclusion naming no formula,
/// and an ECU scaling with a parameter reference are each one problem; the
/// same formulas, rightly excluded, are none. Checked without the corpus.
#[test]
fn the_parse_check_refuses_what_it_should() {
    let ecu = harvest_xml(
        "defs/romraider/ecu/synthetic.xml",
        br#"<roms><rom><table><scaling expression="(x+1))" to_byte="x-1"/></table>
<table><scaling expression="[P8:rpm]*2" to_byte="x/2"/></table></rom></roms>"#,
    )
    .unwrap();
    let logger = harvest_xml(
        "defs/romraider/logger/synthetic.xml",
        br#"<logger><conversion expr="[P8:rpm]*2"/><conversion expr="x/4"/></logger>"#,
    )
    .unwrap();
    let harvest = Harvest::from_files("c0ffee", vec![ecu, logger]);
    assert_eq!(harvest.formulas.len(), 5);
    let bad = sha256_hex(b"(x+1))");
    let parses = sha256_hex(b"x/4");
    let absent = sha256_hex(b"x*3");

    let none = check(&harvest, &[]);
    assert_eq!((none.parsed, none.refused), (4, 0));
    assert_eq!(none.problems.len(), 2, "{:#?}", none.problems);
    assert!(
        none.problems
            .iter()
            .any(|p| p.starts_with(&format!("sha256 {bad} ")) && p.contains("does not parse: EXP")),
        "{:#?}",
        none.problems
    );
    assert!(
        none.problems
            .iter()
            .any(|p| p.contains("an ECU scaling with 1 parameter reference(s)")),
        "{:#?}",
        none.problems
    );
    assert!(
        none.problems.iter().all(|p| !p.contains("(x+1))")),
        "problems name formulas by hash, never by text"
    );

    let wrong = check(
        &harvest,
        &[
            (bad.as_str(), "r"),
            (parses.as_str(), "r"),
            (absent.as_str(), "r"),
        ],
    );
    assert_eq!((wrong.parsed, wrong.refused), (3, 1));
    assert_eq!(wrong.problems.len(), 3, "{:#?}", wrong.problems);
    assert!(
        wrong
            .problems
            .iter()
            .any(|p| p.starts_with(&format!("sha256 {parses} "))
                && p.contains("listed in EXCLUDED but parses"))
    );
    assert!(
        wrong
            .problems
            .iter()
            .any(|p| p.contains(&format!("EXCLUDED lists sha256 {absent}")))
    );

    let right = check(&harvest, &[(bad.as_str(), "r")]);
    assert_eq!((right.parsed, right.refused), (4, 1));
    assert_eq!(right.problems.len(), 1, "only the ECU parameter reference");
}

/// Without the v370 logger package (D-028) the corpus-tier formula test
/// says, in its output, that v370 is skipped and why; it is never silent.
#[test]
fn a_missing_logger_v370_is_reported_as_skipped() {
    if std::env::var_os("RR_LOGGER_ZIP").is_some_and(|v| !v.is_empty()) {
        eprintln!("RR_LOGGER_ZIP is set: the absent-package message is not exercised");
        return;
    }
    let dir = repo_root().join("target").join("no-such-corpus-dir");
    let status = formulas::logger_v370_status(&dir);
    assert!(
        status.starts_with("logger v370: not present")
            && status.contains("skipped")
            && status.contains("D-028")
            && status.contains("logger_v370.zip"),
        "{status}"
    );
}
