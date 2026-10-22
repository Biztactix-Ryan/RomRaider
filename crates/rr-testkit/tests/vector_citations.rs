//! US-RR-14-1: every standard test-vector fixture cites its standard
//! document, edition and section (or, for the transcribed NCS vectors, the
//! legacy test file and lines).
//!
//! The rule itself is the validator's (`citation`,
//! `rr_testkit::kat::validate`), so CI's `cargo xtask validate-fixtures`
//! applies it to every fixture, present and future; its rules are tested
//! case by case in `kat_validate.rs`. This test pins it to the real
//! fixtures:
//!
//! * which fixtures are vector fixtures: the validator's
//!   [`validate::VECTOR_PREFIXES`] are pinned here, every `[[timing]]` and
//!   `[[codec]]` fixture is one whatever its name, the per-family counts are
//!   pinned, and a fixture with a `standard` case source that is not a
//!   vector fixture must be one of [`NOT_VECTORS`], so a new standards
//!   fixture cannot dodge the rule by its name
//!   ([`vector_fixtures_are_identified_by_name_and_shape`]);
//! * every vector fixture passes the validator with its citations, has a
//!   structured citation with document, edition and section (or a legacy
//!   file and lines), and fails rule `citation` with its citations removed
//!   ([`every_vector_fixture_cites_its_documents`]);
//! * every cited legacy file exists under `legacy/` and has every cited
//!   line ([`cited_legacy_files_and_lines_exist`]);
//! * the flagged exceptions are exactly where they are allowed, and the
//!   rest of the vector fixtures meet the criterion without one
//!   ([`flagged_exceptions_are_exactly_the_pinned_fixtures`]): SAE J1979's
//!   edition and section were not read first-hand (paywalled; `verified =
//!   false`, with the secondary `via` sources), and no published standard
//!   defines the checksum sums and XORs (`legacy-algorithm`).

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use rr_testkit::fixtures::repo_root;
use rr_testkit::kat::validate::{self, Rule};
use rr_testkit::kat::{self, Citation, Kat, KatKind, Source};

/// The vector-fixture file-name prefixes, pinned: the validator's list may
/// not lose one.
const PREFIXES: &[&str] = &["timing-", "j1979-", "checksum-", "ncs-codec-"];

/// How many vector fixtures each prefix has today. A new or removed vector
/// fixture changes this on purpose.
const COUNTS: &[(&str, usize)] = &[
    ("timing-", 4),
    ("j1979-", 32),
    ("checksum-", 4),
    ("ncs-codec-", 1),
];

/// Fixtures with a `standard` case source that are not US-RR-14 vector
/// fixtures: IEEE 754 encodings, the JEP precedence rules and NIST SP 811
/// unit factors (US-RR-9, US-RR-19, US-RR-21). A new fixture citing a
/// standard must be named as a vector fixture, or listed here in review.
const NOT_VECTORS: &[&str] = &[
    "expr-jep-precedence.toml",
    "f32-be.toml",
    "f64-be.toml",
    "units-defined-factors.toml",
];

/// The vector fixtures that meet the criterion with no flagged exception:
/// verified `standard` citations (the ISO timing values, the CRC catalogue)
/// or the legacy test file they transcribe (NCS).
const FULLY_CITED: &[&str] = &[
    "checksum-crc.toml",
    "ncs-codec-legacy.toml",
    "timing-iso14230-2.toml",
    "timing-iso15765-2.toml",
    "timing-iso15765-3.toml",
    "timing-iso9141-2.toml",
];

fn kat_dir() -> PathBuf {
    repo_root().join("fixtures").join("kat")
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

type Fixtures = BTreeMap<String, (PathBuf, Kat)>;

/// Every fixture under `fixtures/kat`, by file name.
fn all_fixtures() -> Result<Fixtures, Box<dyn Error>> {
    let mut out = BTreeMap::new();
    for path in validate::discover(&[kat_dir()])? {
        let kat = kat::load(&path)?.data;
        out.insert(name_of(&path), (path, kat));
    }
    Ok(out)
}

/// The vector fixtures, as the validator identifies them.
fn vector_fixtures() -> Result<Fixtures, Box<dyn Error>> {
    Ok(all_fixtures()?
        .into_iter()
        .filter(|(_, (path, kat))| validate::is_vector_fixture(path, kat))
        .collect())
}

fn case_sources(kat: &Kat) -> Vec<&Source> {
    match &kat.kind {
        KatKind::Decode { cases, range, .. } => cases
            .iter()
            .map(|c| &c.source)
            .chain(range.iter().map(|r| &r.source))
            .collect(),
        KatKind::Checksum { cases, .. } => cases
            .iter()
            .flat_map(|c| [&c.before.source, &c.after.source])
            .collect(),
        KatKind::Expr { cases } => cases.iter().map(|c| &c.source).collect(),
        KatKind::Unit { cases } => cases.iter().map(|c| &c.source).collect(),
        KatKind::Timing { cases } => cases.iter().map(|c| &c.source).collect(),
        KatKind::Codec { cases } => cases.iter().map(|c| &c.source).collect(),
    }
}

#[test]
fn vector_fixtures_are_identified_by_name_and_shape() {
    assert_eq!(validate::VECTOR_PREFIXES, PREFIXES);
    assert!(all_fixtures().unwrap().len() > 40);
    let vectors = vector_fixtures().unwrap();
    for (prefix, count) in COUNTS {
        let found: Vec<_> = vectors.keys().filter(|n| n.starts_with(prefix)).collect();
        assert_eq!(found.len(), *count, "{prefix}*: {found:?}");
    }
    assert_eq!(
        vectors.len(),
        COUNTS.iter().map(|(_, n)| n).sum::<usize>(),
        "a vector fixture outside the pinned prefixes: {:?}",
        vectors.keys().collect::<Vec<_>>()
    );
    for (name, (path, kat)) in all_fixtures().unwrap() {
        // Every timing and codec fixture is a vector fixture, by shape.
        if matches!(kat.kind, KatKind::Timing { .. } | KatKind::Codec { .. }) {
            assert!(validate::is_vector_fixture(&path, &kat), "{name}");
        }
        // A fixture resting on a standard is a vector fixture, or a known
        // non-US-RR-14 one.
        let cites_standard = case_sources(&kat)
            .iter()
            .any(|s| matches!(s, Source::Standard { .. }));
        if cites_standard && !vectors.contains_key(&name) {
            assert!(
                NOT_VECTORS.contains(&name.as_str()),
                "{name} has a `standard` source but is not a vector fixture: name it \
                 {PREFIXES:?}* (and give it a [[citation]]) or list it in NOT_VECTORS"
            );
        }
    }
    // A renamed timing or codec fixture is still a vector fixture.
    let (_, (_, timing)) = vectors
        .iter()
        .find(|(n, _)| n.starts_with("timing-"))
        .unwrap();
    assert!(validate::is_vector_fixture(
        Path::new("renamed.toml"),
        timing
    ));
}

#[test]
fn every_vector_fixture_cites_its_documents() {
    let vectors = vector_fixtures().unwrap();
    let files: Vec<PathBuf> = vectors.values().map(|(path, _)| path.clone()).collect();
    let report = validate::validate_files(&files, None).unwrap();
    assert!(report.violations.is_empty(), "{:#?}", report.violations);

    for (name, (path, kat)) in &vectors {
        assert!(!kat.citations.is_empty(), "{name}: no [[citation]]");
        for citation in &kat.citations {
            match citation {
                Citation::Standard {
                    doc,
                    edition,
                    section,
                    ..
                } => {
                    for (field, value) in [("doc", doc), ("edition", edition), ("section", section)]
                    {
                        assert!(!value.trim().is_empty(), "{name}: empty `{field}`");
                    }
                    assert!(
                        edition.chars().filter(char::is_ascii_digit).count() >= 4
                            || edition.to_lowercase().contains("version"),
                        "{name}: edition {edition:?}"
                    );
                }
                Citation::LegacyTest { file, lines }
                | Citation::LegacyAlgorithm { file, lines, .. } => {
                    assert!(file.starts_with("legacy/src/"), "{name}: {file}");
                    kat::parse_line_spec(lines).unwrap();
                }
            }
        }
        // Without its citations it fails the validator, rule `citation`.
        let text = fs::read_to_string(path).unwrap();
        let mut table: toml::Table = toml::from_str(&text).unwrap();
        assert!(table.remove("citation").is_some(), "{name}");
        let stripped = toml::to_string(&table).unwrap();
        let violations = validate::validate_text(path, &stripped, None);
        assert!(
            violations
                .iter()
                .any(|v| v.rule == Rule::Citation && v.location.is_none()),
            "{name} without citations: {violations:#?}"
        );
        assert!(
            violations.iter().all(|v| v.rule == Rule::Citation),
            "{name} without citations: {violations:#?}"
        );
    }
}

#[test]
fn cited_legacy_files_and_lines_exist() {
    let root = repo_root();
    let mut checked = 0;
    for (name, (_, kat)) in all_fixtures().unwrap() {
        let citations = kat.citations.iter().filter_map(|c| match c {
            Citation::LegacyTest { file, lines }
            | Citation::LegacyAlgorithm { file, lines, .. } => Some((file, lines)),
            Citation::Standard { .. } => None,
        });
        let sources = case_sources(&kat).into_iter().filter_map(|s| match s {
            Source::LegacyTest { file, lines } => Some((file, lines)),
            _ => None,
        });
        for (file, lines) in citations.chain(sources) {
            assert!(file.starts_with("legacy/"), "{name}: {file}");
            let text = fs::read_to_string(root.join(file))
                .unwrap_or_else(|e| panic!("{name}: cited {file}: {e}"));
            let count = u32::try_from(text.lines().count()).unwrap();
            for range in kat::parse_line_spec(lines).unwrap() {
                assert!(
                    *range.end() <= count,
                    "{name}: {file}:{lines} is past the end of the file ({count} lines)"
                );
            }
            checked += 1;
        }
    }
    // The NCS citation and its six case sources, and the checksum families'
    // legacy algorithms.
    assert!(checked >= 7 + 15, "{checked}");
}

#[test]
fn flagged_exceptions_are_exactly_the_pinned_fixtures() {
    assert_eq!(validate::UNVERIFIED_CITATION_PREFIXES, ["j1979-01-"]);
    assert_eq!(
        validate::LEGACY_ALGORITHM_FIXTURES,
        [
            "checksum-sum-bytes.toml",
            "checksum-sum-words.toml",
            "checksum-xor.toml"
        ]
    );
    let mut unverified = Vec::new();
    let mut legacy_algorithm = Vec::new();
    for (name, (_, kat)) in all_fixtures().unwrap() {
        for citation in &kat.citations {
            match citation {
                Citation::Standard {
                    verified: false,
                    via,
                    ..
                } => {
                    // Two independent secondary sources, each pinned by a
                    // revision or digest.
                    assert!(via.len() >= 2, "{name}: {via:?}");
                    unverified.push(name.clone());
                }
                Citation::LegacyAlgorithm { .. } => legacy_algorithm.push(name.clone()),
                _ => {}
            }
        }
    }
    unverified.dedup();
    legacy_algorithm.dedup();
    let j1979: Vec<String> = vector_fixtures()
        .unwrap()
        .into_keys()
        .filter(|n| n.starts_with("j1979-"))
        .collect();
    assert_eq!(unverified, j1979, "unverified citations");
    assert_eq!(
        legacy_algorithm,
        validate::LEGACY_ALGORITHM_FIXTURES,
        "legacy-algorithm citations"
    );
    // Every other vector fixture meets the criterion outright.
    let others: Vec<String> = vector_fixtures()
        .unwrap()
        .into_keys()
        .filter(|n| !unverified.contains(n) && !legacy_algorithm.contains(n))
        .collect();
    assert_eq!(others, FULLY_CITED);
}
