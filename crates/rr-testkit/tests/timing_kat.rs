//! rr-core's standard timing constants (`rr_core::timing::iso`, US-RR-14)
//! against the timing known-answer fixtures (`[[timing]]` cases in
//! `fixtures/kat/timing-*.toml`).
//!
//! US-RR-14-5: each ISO default timing parameter is a named constant backed
//! by a fixture case that cites the standard it is printed in. Checked here:
//!
//! * every `[[timing]]` case of every fixture names a constant of
//!   [`CONSTANTS`], and that constant equals the case's value
//!   ([`every_case_matches_its_constant`]);
//! * every constant of [`CONSTANTS`] has exactly one case
//!   ([`every_constant_has_exactly_one_case`]), so a constant cannot be
//!   added, or a case dropped, without the other;
//! * every case's source is a `standard` whose `doc` names the document
//!   and its edition year (`ISO 14230-2:1999`) and whose `section` is not
//!   empty ([`every_case_cites_document_edition_and_section`]);
//! * ISO 14230-2's P3min is 55 ms, the wait legacy wrote as a bare
//!   `sleep(55L)` (PROJECT.md MF-28; [`p3_min_is_55_ms`]);
//! * ISO 14230-2's whole default set, P1min to P4max, is a constant each
//!   with a case in `timing-iso14230-2.toml`, and that file holds nothing
//!   else ([`iso14230_2_default_set_is_complete`]);
//! * [`CONSTANTS`] lists every `pub const` of `timing/iso.rs`, so a new
//!   constant cannot escape these checks
//!   ([`constants_lists_every_iso_constant`]);
//! * PROJECT.md's MF-28 row names `iso14230_2::P3_MIN` and the fixture
//!   that backs it, and both exist ([`mf28_names_p3_min_and_its_fixture`]).
//!
//! Changing a constant or its case alone fails the first check. Expected
//! values come from the standards (the fixtures), never from rr-core
//! (D-024). This file is a test boundary: the fixture types do not name
//! rr-core.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use rr_core::timing::iso::{iso9141_2, iso14230_2, iso15765_2, iso15765_3};
use rr_testkit::fixtures::repo_root;
use rr_testkit::kat::{self, KatKind, Source, TimingCase};

/// Every constant of `rr_core::timing::iso`, by the name a fixture case
/// gives it (its path under `timing::iso`). A new constant goes here, and
/// needs a case.
const CONSTANTS: &[(&str, Duration)] = &[
    ("iso9141_2::W0_MIN", iso9141_2::W0_MIN),
    ("iso9141_2::W1_MIN", iso9141_2::W1_MIN),
    ("iso9141_2::W1_MAX", iso9141_2::W1_MAX),
    ("iso9141_2::W2_MIN", iso9141_2::W2_MIN),
    ("iso9141_2::W2_MAX", iso9141_2::W2_MAX),
    ("iso9141_2::W3_MIN", iso9141_2::W3_MIN),
    ("iso9141_2::W3_MAX", iso9141_2::W3_MAX),
    ("iso9141_2::W4_MIN", iso9141_2::W4_MIN),
    ("iso9141_2::W4_MAX", iso9141_2::W4_MAX),
    ("iso14230_2::P1_MIN", iso14230_2::P1_MIN),
    ("iso14230_2::P1_MAX", iso14230_2::P1_MAX),
    ("iso14230_2::P2_MIN", iso14230_2::P2_MIN),
    ("iso14230_2::P2_MAX", iso14230_2::P2_MAX),
    ("iso14230_2::P3_MIN", iso14230_2::P3_MIN),
    ("iso14230_2::P3_MAX", iso14230_2::P3_MAX),
    ("iso14230_2::P4_MIN", iso14230_2::P4_MIN),
    ("iso14230_2::P4_MAX", iso14230_2::P4_MAX),
    ("iso15765_2::N_AS_TIMEOUT", iso15765_2::N_AS_TIMEOUT),
    ("iso15765_2::N_AR_TIMEOUT", iso15765_2::N_AR_TIMEOUT),
    ("iso15765_2::N_BS_TIMEOUT", iso15765_2::N_BS_TIMEOUT),
    ("iso15765_2::N_CR_TIMEOUT", iso15765_2::N_CR_TIMEOUT),
    ("iso15765_2::STMIN_MAX", iso15765_2::STMIN_MAX),
    ("iso15765_2::STMIN_SUB_MS_MIN", iso15765_2::STMIN_SUB_MS_MIN),
    ("iso15765_2::STMIN_SUB_MS_MAX", iso15765_2::STMIN_SUB_MS_MAX),
    (
        "iso15765_3::P2_CAN_SERVER_MAX",
        iso15765_3::P2_CAN_SERVER_MAX,
    ),
    (
        "iso15765_3::P2_STAR_CAN_SERVER_MAX",
        iso15765_3::P2_STAR_CAN_SERVER_MAX,
    ),
];

/// `(fixture file name, case number from 1, case)` for every `[[timing]]`
/// case.
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn timing_cases() -> Vec<(String, usize, TimingCase)> {
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
        let KatKind::Timing { cases } = fixture.data.kind else {
            continue; // other fixtures have no timing parameters
        };
        for (index, case) in cases.into_iter().enumerate() {
            out.push((name.clone(), index + 1, case));
        }
    }
    out
}

/// Why `case` disagrees with [`CONSTANTS`], or `None` when it agrees.
fn mismatch(case: &TimingCase) -> Option<String> {
    let Some(&(_, constant)) = CONSTANTS.iter().find(|(name, _)| *name == case.name) else {
        return Some(format!(
            "{:?} names no constant of rr_core::timing::iso",
            case.name
        ));
    };
    (constant != case.duration()).then(|| {
        format!(
            "{} is {constant:?}, the fixture says {} {} ({:?})",
            case.name,
            case.amount,
            case.unit.key(),
            case.duration()
        )
    })
}

/// Why `source` does not cite a document, its edition year and a section,
/// or `None` when it does.
fn citation_problem(source: &Source) -> Option<String> {
    let Source::Standard { doc, section } = source else {
        return Some(format!(
            "source is not a `standard` (a timing default is printed in one): {source:?}"
        ));
    };
    if section.trim().is_empty() {
        return Some("source `section` is empty".to_owned());
    }
    // `ISO 14230-2:1999`: the document number, a colon, a four-digit year.
    let has_edition = doc
        .split(':')
        .skip(1)
        .any(|after| after.len() >= 4 && after.bytes().take(4).all(|b| b.is_ascii_digit()));
    (!has_edition).then(|| {
        format!(
            "source `doc` {doc:?} does not name its edition as `<document>:<year>` \
             (`ISO 14230-2:1999`)"
        )
    })
}

#[test]
fn every_case_matches_its_constant() {
    let cases = timing_cases();
    assert!(!cases.is_empty(), "no [[timing]] cases in fixtures/kat");
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|(file, number, case)| {
            mismatch(case).map(|why| format!("{file} [[timing]] #{number}: {why}"))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_constant_has_exactly_one_case() {
    let mut count: BTreeMap<&str, usize> = CONSTANTS.iter().map(|(name, _)| (*name, 0)).collect();
    let cases = timing_cases();
    for (_, _, case) in &cases {
        if let Some(n) = count.get_mut(case.name.as_str()) {
            *n += 1;
        }
    }
    let wrong: Vec<String> = count
        .iter()
        .filter(|&(_, &n)| n != 1)
        .map(|(name, n)| format!("{name}: {n} cases"))
        .collect();
    assert!(
        wrong.is_empty(),
        "each constant needs exactly one [[timing]] case:\n{}",
        wrong.join("\n")
    );
    assert_eq!(
        CONSTANTS.len(),
        count.len(),
        "CONSTANTS names a constant twice"
    );
}

#[test]
fn every_case_cites_document_edition_and_section() {
    let failures: Vec<String> = timing_cases()
        .iter()
        .filter_map(|(file, number, case)| {
            citation_problem(&case.source)
                .map(|why| format!("{file} [[timing]] #{number} ({}): {why}", case.name))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn p3_min_is_55_ms() {
    assert_eq!(
        iso14230_2::P3_MIN,
        Duration::from_millis(55),
        "ISO 14230-2:1999 Table 3: P3min defaults to 55 ms"
    );
    let cases = timing_cases();
    let found: Vec<(&str, &TimingCase)> = cases
        .iter()
        .filter(|(_, _, case)| case.name == "iso14230_2::P3_MIN")
        .map(|(file, _, case)| (file.as_str(), case))
        .collect();
    let [(file, case)] = found.as_slice() else {
        panic!("expected one iso14230_2::P3_MIN case, found {found:?}");
    };
    assert_eq!(*file, ISO14230_2_FIXTURE);
    assert_eq!(case.duration(), Duration::from_millis(55));
}

/// The fixture holding ISO 14230-2's default timings, by file name.
const ISO14230_2_FIXTURE: &str = "timing-iso14230-2.toml";

/// ISO 14230-2:1999 Table 3's default timing parameters, by constant name:
/// the minimum and maximum of each of P1 to P4.
const ISO14230_2_DEFAULTS: [&str; 8] = [
    "iso14230_2::P1_MIN",
    "iso14230_2::P1_MAX",
    "iso14230_2::P2_MIN",
    "iso14230_2::P2_MAX",
    "iso14230_2::P3_MIN",
    "iso14230_2::P3_MAX",
    "iso14230_2::P4_MIN",
    "iso14230_2::P4_MAX",
];

#[test]
fn iso14230_2_default_set_is_complete() {
    let cases = timing_cases();
    let mut problems = Vec::new();
    for name in ISO14230_2_DEFAULTS {
        if !CONSTANTS.iter().any(|(c, _)| *c == name) {
            problems.push(format!("{name}: no constant in CONSTANTS"));
        }
        let in_file = cases
            .iter()
            .filter(|(file, _, case)| file == ISO14230_2_FIXTURE && case.name == name)
            .count();
        if in_file != 1 {
            problems.push(format!("{name}: {in_file} cases in {ISO14230_2_FIXTURE}"));
        }
    }
    for (file, number, case) in &cases {
        let ours = case.name.starts_with("iso14230_2::");
        if (file == ISO14230_2_FIXTURE) != ours {
            problems.push(format!(
                "{file} [[timing]] #{number} ({}): ISO 14230-2 cases belong in \
                 {ISO14230_2_FIXTURE}, and only they",
                case.name
            ));
        }
        if ours && !ISO14230_2_DEFAULTS.contains(&case.name.as_str()) {
            problems.push(format!(
                "{file} [[timing]] #{number}: {} is not a Table 3 default",
                case.name
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// `module::NAME` for every `pub const` inside a `pub mod module {` of
/// `source` (rr-core's `timing/iso.rs`).
fn declared_constants(source: &str) -> Vec<String> {
    let mut module: Option<&str> = None;
    let mut out = Vec::new();
    for line in source.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("pub mod ") {
            module = rest.strip_suffix('{').map(str::trim);
        } else if t == "}" && !line.starts_with(' ') {
            module = None;
        } else if let (Some(m), Some(rest)) = (module, t.strip_prefix("pub const "))
            && let Some((name, _)) = rest.split_once(':')
        {
            out.push(format!("{m}::{}", name.trim()));
        }
    }
    out
}

#[test]
fn constants_lists_every_iso_constant() {
    let path = repo_root().join("crates/rr-core/src/timing/iso.rs");
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        .replace("\r\n", "\n");
    let mut declared = declared_constants(&source);
    declared.sort();
    let mut listed: Vec<String> = CONSTANTS.iter().map(|(n, _)| (*n).to_owned()).collect();
    listed.sort();
    assert_eq!(
        declared, listed,
        "CONSTANTS must list exactly the `pub const`s of timing/iso.rs"
    );
    for name in ISO14230_2_DEFAULTS {
        assert!(declared.iter().any(|d| d == name), "{name} not declared");
    }
}

#[test]
fn declared_constants_reads_modules() {
    let source = "pub mod a_1 {\n    use x;\n    pub const P: Duration = D;\n}\n\
                  pub mod b {\n    pub const Q_MAX: Duration = D;\n}\npub const OUT: u8 = 0;\n";
    assert_eq!(declared_constants(source), ["a_1::P", "b::Q_MAX"]);
}

/// The cells of PROJECT.md's Must-Fix ledger row `id` (`| MF-28 | P1 |
/// ... |`), trimmed, without the empty edges.
fn ledger_row<'a>(project: &'a str, id: &str) -> Option<Vec<&'a str>> {
    project.lines().find_map(|line| {
        let cells: Vec<&str> = line.trim().split('|').map(str::trim).collect();
        (line.trim_start().starts_with('|') && cells.get(1) == Some(&id))
            .then(|| cells[1..cells.len().saturating_sub(1)].to_vec())
    })
}

/// Why PROJECT.md's MF-28 row does not reference ISO 14230-2's P3min
/// constant and the fixture backing it, or nothing when it does.
/// `fixture_cases` gives the `[[timing]]` case names of a fixture path
/// (relative to the repository), or `None` when it does not exist.
fn mf28_problems(
    project: &str,
    fixture_cases: &dyn Fn(&str) -> Option<Vec<String>>,
) -> Vec<String> {
    const CONSTANT: &str = "iso14230_2::P3_MIN";
    const FIXTURE: &str = "fixtures/kat/timing-iso14230-2.toml";
    let Some(cells) = ledger_row(project, "MF-28") else {
        return vec!["PROJECT.md has no `| MF-28 |` ledger row".to_owned()];
    };
    let mut problems = Vec::new();
    // | ID | Pri | Area | Java behaviour | Required behaviour | Proving test |
    if cells.len() != 6 {
        problems.push(format!(
            "MF-28 row has {} cells, the ledger has 6",
            cells.len()
        ));
    }
    let row = cells.join(" | ");
    let spans: Vec<&str> = row.split('`').skip(1).step_by(2).collect();
    let mut named_constant = false;
    let mut named_fixture = false;
    for span in &spans {
        let short = span.strip_prefix("rr_core::timing::iso::").unwrap_or(span);
        if short.starts_with("iso") && short.contains("::") {
            if !CONSTANTS.iter().any(|(n, _)| *n == short) {
                problems.push(format!("MF-28 names `{span}`, not a timing::iso constant"));
            }
            named_constant |= short == CONSTANT;
        }
        if span.starts_with("fixtures/") {
            match fixture_cases(span) {
                None => problems.push(format!("MF-28 names `{span}`, which does not exist")),
                Some(names) if *span == FIXTURE && !names.iter().any(|n| n == CONSTANT) => {
                    problems.push(format!("`{span}` has no {CONSTANT} case"));
                }
                Some(_) => {}
            }
            named_fixture |= *span == FIXTURE;
        }
    }
    if !named_constant {
        problems.push(format!("MF-28 does not name `{CONSTANT}`"));
    }
    if !named_fixture {
        problems.push(format!("MF-28 does not name `{FIXTURE}`"));
    }
    problems
}

#[test]
fn mf28_names_p3_min_and_its_fixture() {
    let path = repo_root().join(".project").join("PROJECT.md");
    let project = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        .replace("\r\n", "\n");
    let cases = timing_cases();
    let fixture_cases = |rel: &str| -> Option<Vec<String>> {
        let file = rel.strip_prefix("fixtures/kat/")?;
        repo_root().join(rel).is_file().then(|| {
            cases
                .iter()
                .filter(|(f, _, _)| f == file)
                .map(|(_, _, c)| c.name.clone())
                .collect()
        })
    };
    let problems = mf28_problems(&project, &fixture_cases);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn mf28_check_catches_violations() {
    const GOOD: &str = "| ID | Pri | Area | Java | Required | Test |\n\
        | MF-28 | P1 | Comms | `sleep(55L)` | `rr_core::timing::iso::iso14230_2::P3_MIN`, \
        backed by `fixtures/kat/timing-iso14230-2.toml`. | T4 |\n";
    let real = |rel: &str| -> Option<Vec<String>> {
        (rel == "fixtures/kat/timing-iso14230-2.toml")
            .then(|| vec!["iso14230_2::P3_MIN".to_owned()])
    };
    assert_eq!(mf28_problems(GOOD, &real), Vec::<String>::new());

    let has = |text: &str, needle: &str| {
        let p = mf28_problems(text, &real);
        assert!(
            p.iter().any(|m| m.contains(needle)),
            "{needle:?} not in {p:?}"
        );
    };
    has(&GOOD.replace("MF-28", "MF-29"), "no `| MF-28 |`");
    has(
        &GOOD.replace("iso14230_2::P3_MIN`", "iso14230_2::P3_MAX`"),
        "does not name `iso14230_2::P3_MIN`",
    );
    has(
        &GOOD.replace("P3_MIN`", "P5_MIN`"),
        "not a timing::iso constant",
    );
    has(
        &GOOD.replace("timing-iso14230-2.toml", "timing.toml"),
        "does not exist",
    );
    has(
        &GOOD.replace("`fixtures/kat/timing-iso14230-2.toml`", "the fixture"),
        "does not name `fixtures/",
    );
    has(&GOOD.replace("| T4 |", "|"), "5 cells");
    let without_case = |_: &str| Some(Vec::new());
    assert!(
        mf28_problems(GOOD, &without_case)
            .iter()
            .any(|m| m.contains("has no iso14230_2::P3_MIN case"))
    );
}

#[test]
fn a_changed_value_or_unknown_name_is_reported() {
    let mut case = TimingCase {
        name: "iso14230_2::P3_MIN".to_owned(),
        amount: 55,
        unit: kat::TimeUnit::Millis,
        source: Source::Standard {
            doc: "ISO 14230-2:1999".to_owned(),
            section: "4.5.1, Table 3".to_owned(),
        },
        note: None,
    };
    assert_eq!(mismatch(&case), None);
    case.amount = 55_000;
    case.unit = kat::TimeUnit::Micros;
    assert_eq!(mismatch(&case), None, "55 000 us is 55 ms");
    case.amount = 56_000;
    assert!(mismatch(&case).is_some_and(|m| m.contains("P3_MIN")));
    case.name = "iso14230_2::P5_MIN".to_owned();
    assert!(mismatch(&case).is_some_and(|m| m.contains("names no constant")));
}

#[test]
fn a_citation_without_edition_or_section_is_reported() {
    let standard = |doc: &str, section: &str| Source::Standard {
        doc: doc.to_owned(),
        section: section.to_owned(),
    };
    assert_eq!(
        citation_problem(&standard("ISO 14230-2:1999 (first edition)", "4.5.1")),
        None
    );
    assert!(citation_problem(&standard("ISO 14230-2", "4.5.1")).is_some());
    assert!(citation_problem(&standard("ISO 14230-2:19", "4.5.1")).is_some());
    assert!(citation_problem(&standard("ISO 14230-2:1999", " ")).is_some());
    assert!(
        citation_problem(&Source::HandCalc {
            working: "55 = 50 + 5".to_owned()
        })
        .is_some()
    );
}
