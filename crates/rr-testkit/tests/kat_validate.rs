//! Tests for the KAT fixture validator (`rr_testkit::kat::validate`,
//! `validate::cli` command line, US-RR-9-6).
//!
//! Proves the US-RR-9 acceptance criteria the validator owns: an expected
//! value without a source is rejected naming the file and the case; a
//! `hand-calc` source must show its working (formula, intermediate values,
//! result); a changed expected value needs a new changelog entry in base mode
//! (injected base text and a real temporary git repository); the clean sample
//! fixtures pass; and the command line exits non-zero on any violation.
//! For US-RR-9-1 it also proves the default run (what CI's `cargo xtask
//! validate-fixtures` does) discovers every KAT fixture in the repository,
//! and that stripping any one `source` from any real fixture is rejected.
//! For US-RR-24-2 it proves the `[range]` rules: a source with working that
//! reaches all four values, a conversion, a sane tolerance, a range that
//! agrees with its own `monotonicity` (`range`), and a changelog entry for a
//! changed or removed range in base mode.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use rr_testkit::kat::validate::{
    self, BaseSource, GitBase, Rule, ValidateError, Violation, validate_text,
};

fn samples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join("kat")
}

/// A fresh, empty scratch directory for one test.
fn scratch(name: &str) -> io::Result<PathBuf> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("kat-validate")
        .join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn check(text: &str) -> Vec<Violation> {
    validate_text(Path::new("fixtures/kat/bad.toml"), text, None)
}

/// Asserts exactly one violation, of `rule`, and returns its rendered text.
#[track_caller]
fn one(violations: &[Violation], rule: Rule) -> String {
    assert_eq!(violations.len(), 1, "{violations:#?}");
    assert_eq!(violations[0].rule, rule, "{violations:#?}");
    violations[0].to_string()
}

const DECODE_HEAD: &str = r#"schema = 1
title = "u16 big-endian, 0.1 scaling"

[definition.encoding]
storage = "u16"
endian = "big"

[definition.conversion]
to_real = "x * 0.1"

[input]
hex = "01 F4 FF 38"
"#;

const GOOD_CASE: &str = r#"
[[decode]]
offset = 0
raw = 500
real = 50.0
source = { kind = "hand-calc", working = "0x01F4 = 1*256 + 244 = 500; 500 * 0.1 = 50.0" }
"#;

fn decode(working: &str) -> String {
    format!(
        "{DECODE_HEAD}{GOOD_CASE}\n[[decode]]\noffset = 2\nraw = 65336\nreal = 6533.6\n\
         source = {{ kind = \"hand-calc\", working = {working:?} }}\n"
    )
}

const CHECKSUM: &str = r#"schema = 1
title = "sum16"

[input]
hex = "01 02 03 04 00 00"

[[checksum]]
algorithm = "sum16-be"
offset = 4
before = { hex = "00 00", valid = false, source = { kind = "hand-calc", working = "stored 00 00 = 0; computed 1+2+3+4 = 10" } }
after = { hex = "00 0A", valid = true, source = { kind = "hand-calc", working = "1+2+3+4 = 10 = 0x000A" } }
"#;

// ---------------------------------------------------------------------------
// Pass path
// ---------------------------------------------------------------------------

#[test]
fn clean_sample_fixtures_pass() {
    let files = validate::discover(&[samples()]).unwrap();
    assert!(files.len() >= 3, "{files:?}");
    let report = validate::validate_files(&files, None).unwrap();
    assert_eq!(report.files, files.len());
    assert!(report.violations.is_empty(), "{:#?}", report.violations);
}

#[test]
fn default_paths_include_the_samples() {
    let paths = validate::default_paths();
    assert!(paths.contains(&samples()), "{paths:?}");
    assert!(paths.iter().all(|p| p.is_dir()), "{paths:?}");
}

#[test]
fn good_inline_fixtures_pass() {
    let text = decode("0xFF38 = 255*256 + 56 = 65280 + 56 = 65336; 65336 * 0.1 = 6533.6");
    assert_eq!(check(&text), vec![]);
    assert_eq!(check(CHECKSUM), vec![]);
}

// ---------------------------------------------------------------------------
// Missing sources (US-RR-9-1)
// ---------------------------------------------------------------------------

#[test]
fn decode_case_without_source_names_file_and_case() {
    let text = format!("{DECODE_HEAD}{GOOD_CASE}\n[[decode]]\noffset = 2\nraw = 65336\n");
    let message = one(&check(&text), Rule::MissingSource);
    assert!(message.contains("fixtures/kat/bad.toml"), "{message}");
    assert!(message.contains("[[decode]] #2 (offset 2)"), "{message}");
    assert!(message.contains("has no `source`"), "{message}");
    assert!(message.contains("[missing-source]"), "{message}");
}

#[test]
fn checksum_state_without_source_names_file_case_and_state() {
    let text = CHECKSUM.replace(
        r#"after = { hex = "00 0A", valid = true, source = { kind = "hand-calc", working = "1+2+3+4 = 10 = 0x000A" } }"#,
        r#"after = { hex = "00 0A", valid = true }"#,
    );
    let message = one(&check(&text), Rule::MissingSource);
    assert!(
        message.contains("bad.toml: [[checksum]] #1 `after`"),
        "{message}"
    );
}

#[test]
fn every_missing_source_is_reported() {
    let text = format!(
        "{DECODE_HEAD}\n[[decode]]\nraw = 500\n\n[[decode]]\noffset = \"0x2\"\nraw = 65336\n"
    );
    let violations = check(&text);
    assert_eq!(violations.len(), 2, "{violations:#?}");
    assert!(violations.iter().all(|v| v.rule == Rule::MissingSource));
    assert_eq!(
        violations[0].location.as_deref(),
        Some("[[decode]] #1 (offset 0)")
    );
    assert_eq!(
        violations[1].location.as_deref(),
        Some("[[decode]] #2 (offset 2)")
    );
}

/// The repository root, canonicalized.
fn repo_root() -> io::Result<PathBuf> {
    rr_testkit::fixtures::repo_root().canonicalize()
}

/// Every fixture the validator finds with no paths (what CI runs), canonicalized.
fn default_fixtures() -> io::Result<Vec<PathBuf>> {
    validate::discover(&validate::default_paths())
        .map_err(io::Error::other)?
        .into_iter()
        .map(|p| p.canonicalize())
        .collect()
}

/// Every `*.toml` below `dir` that holds expected values (a `[[decode]]`,
/// `[[checksum]]`, `[[expr]]`, `[[unit]]`, `[[timing]]` or `[[codec]]` array), skipping
/// build output,
/// git data and `legacy/`.
fn kat_files_below(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            let skip = path.file_name().is_some_and(|name| {
                [".git", "target", "legacy"]
                    .map(std::ffi::OsStr::new)
                    .contains(&name)
            });
            if !skip {
                kat_files_below(&path, out)?;
            }
        } else if path.extension().is_some_and(|ext| ext == "toml")
            && let Ok(table) = fs::read_to_string(&path)?.parse::<toml::Table>()
            && ["decode", "checksum", "expr", "unit", "timing", "codec"]
                .iter()
                .any(|key| table.get(*key).is_some_and(toml::Value::is_array))
        {
            out.push(path.canonicalize()?);
        }
    }
    Ok(())
}

#[test]
fn default_discovery_covers_every_kat_fixture_in_the_repository() {
    // CI runs `cargo xtask validate-fixtures` with no paths, i.e. these.
    let found = default_fixtures().unwrap();
    let seed = repo_root().unwrap().join("fixtures").join("kat");
    let defaults: Vec<PathBuf> = validate::default_paths()
        .iter()
        .map(|p| p.canonicalize().unwrap())
        .collect();
    assert!(defaults.contains(&seed), "{defaults:?}");

    // Every *.toml in the seed directory is discovered, not a subset.
    let seed_files = fs::read_dir(&seed)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
        .count();
    assert!(seed_files >= 10, "{seed_files} seed fixtures");
    assert_eq!(
        found.iter().filter(|p| p.starts_with(&seed)).count(),
        seed_files
    );

    // No file anywhere in the repository holds expected values without
    // being one the default run validates.
    let mut kat = Vec::new();
    kat_files_below(&repo_root().unwrap(), &mut kat).unwrap();
    let missed: Vec<&PathBuf> = kat.iter().filter(|p| !found.contains(p)).collect();
    assert!(
        missed.is_empty(),
        "KAT fixtures outside the validator's default paths: {missed:?}"
    );
    assert!(kat.len() >= seed_files, "{kat:?}");
}

#[test]
fn directories_are_searched_recursively() {
    let dir = scratch("recursive").unwrap();
    fs::create_dir_all(dir.join("a").join("b")).unwrap();
    fs::write(dir.join("top.toml"), CHECKSUM).unwrap();
    fs::write(
        dir.join("a").join("b").join("deep.toml"),
        format!("{DECODE_HEAD}\n[[decode]]\nraw = 500\n"),
    )
    .unwrap();
    fs::write(dir.join("a").join("ignored.txt"), "not a fixture").unwrap();
    let files = validate::discover(std::slice::from_ref(&dir)).unwrap();
    assert_eq!(files.len(), 2, "{files:?}");
    let (code, _, stderr) = bin(&dir, &["."]);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("deep.toml: [[decode]] #1"), "{stderr}");
    assert!(stderr.contains("[missing-source]"), "{stderr}");
}

#[test]
fn stripping_any_source_from_any_real_fixture_is_rejected() {
    let mut mutants = 0;
    for path in default_fixtures().unwrap() {
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(
            validate_text(&path, &text, None),
            vec![],
            "{}",
            path.display()
        );
        let table: toml::Table = text.parse().unwrap();

        // (array key, case index, checksum state or None, expected location prefix)
        let mut targets = Vec::new();
        for (key, states) in [
            ("decode", &[][..]),
            ("unit", &[][..]),
            ("timing", &[][..]),
            ("codec", &[][..]),
            ("checksum", &["before", "after"][..]),
        ] {
            let Some(cases) = table.get(key).and_then(toml::Value::as_array) else {
                continue;
            };
            for index in 0..cases.len() {
                if states.is_empty() {
                    targets.push((key, index, None, format!("[[{key}]] #{} ", index + 1)));
                }
                for state in states {
                    targets.push((
                        key,
                        index,
                        Some(*state),
                        format!("[[checksum]] #{} `{state}`", index + 1),
                    ));
                }
            }
        }

        if table.contains_key("range") {
            targets.push(("range", 0, None, "[range]".to_owned()));
        }

        for (key, index, state, location) in targets {
            let mut stripped = table.clone();
            let case = if key == "range" {
                stripped[key].as_table_mut().unwrap()
            } else {
                stripped[key].as_array_mut().unwrap()[index]
                    .as_table_mut()
                    .unwrap()
            };
            let holder = match state {
                Some(state) => case[state].as_table_mut().unwrap(),
                None => case,
            };
            assert!(
                holder.remove("source").is_some(),
                "{}: {location}",
                path.display()
            );
            let violations = validate_text(&path, &toml::to_string(&stripped).unwrap(), None);
            let message = one(&violations, Rule::MissingSource);
            assert!(
                violations[0]
                    .location
                    .as_deref()
                    .is_some_and(|l| l.starts_with(&location)),
                "{}: expected {location}, got {message}",
                path.display()
            );
            mutants += 1;
        }
    }
    assert!(mutants >= 100, "only {mutants} expected values stripped");
}

// ---------------------------------------------------------------------------
// hand-calc working rule (US-RR-9-2)
// ---------------------------------------------------------------------------

#[test]
fn hand_calc_without_working_key_is_rejected() {
    let text = format!(
        "{DECODE_HEAD}{GOOD_CASE}\n[[decode]]\noffset = 2\nraw = 65336\nsource = {{ kind = \"hand-calc\" }}\n"
    );
    let message = one(&check(&text), Rule::HandCalcWorking);
    assert!(message.contains("[[decode]] #2 (offset 2)"), "{message}");
    assert!(message.contains("no `working`"), "{message}");
}

#[test]
fn hand_calc_trivial_working_is_rejected() {
    for (working, why) in [
        ("", "is empty"),
        ("   ", "is empty"),
        ("65336 and 6533.6", "has no `=`"),
        ("trust me", "has no `=`"),
        ("x = 65336", "fewer than two numbers"),
        ("= 6533.6", "fewer than two numbers"),
        (
            "bytes FF 38: 255*256 + 56 = 65335; 65335 * 0.1 = 6533.5",
            "does not arrive at the expected raw value 65336",
        ),
        (
            "0xFF38 = 255*256 + 56 = 65336",
            "does not arrive at the expected real value 6533.6",
        ),
        ("u16 = u8 + u8", "fewer than two numbers"),
        // Restating the answer is not working: no formula ...
        ("65336 = 65336; 6533.6 = 6533.6", "shows no formula"),
        ("raw = 65336, real = 6533.6", "shows no formula"),
        ("0xFF38 = 65336 = 6533.6", "shows no formula"),
        // ... and a formula over nothing but the result shows no
        // intermediate value either.
        (
            "65336 - 65336 + 65336 = 0xFF38; 6533.6 = 6533.6",
            "no intermediate value",
        ),
        ("6533.6 * 0xFF38 = 6533.6 * 65336", "no intermediate value"),
    ] {
        let violations = check(&decode(working));
        assert_eq!(violations.len(), 1, "{working:?}: {violations:#?}");
        let message = one(&violations, Rule::HandCalcWorking);
        assert!(
            message.contains("[[decode]] #2 (offset 2)"),
            "{working:?}: {message}"
        );
        assert!(message.contains(why), "{working:?}: {message}");
    }
}

#[test]
fn hand_calc_working_matches_within_tolerance_and_in_hex() {
    let text = format!(
        "{DECODE_HEAD}\n[[decode]]\noffset = 2\nraw = 65336\nreal = 6533.6\ntolerance = 0.05\n\
         source = {{ kind = \"hand-calc\", working = \"0xFF38 = 0xFF*0x100 + 0x38; 65336 * 0.1 = 6533.62\" }}\n"
    );
    assert_eq!(check(&text), vec![]);
    let hex_raw = format!(
        "{DECODE_HEAD}\n[[decode]]\nraw = 500\nreal = 50.0\n\
         source = {{ kind = \"hand-calc\", working = \"1*256 + 244 = 0x01F4; 0x1F4 * 0.1 = 50\" }}\n"
    );
    assert_eq!(check(&hex_raw), vec![]);
}

#[test]
fn hand_calc_checksum_working_must_mention_the_stored_checksum() {
    let text = CHECKSUM.replace("1+2+3+4 = 10 = 0x000A", "1+2+3+4 = 11");
    let message = one(&check(&text), Rule::HandCalcWorking);
    assert!(message.contains("[[checksum]] #1 `after`"), "{message}");
    assert!(message.contains("stored checksum 00 0A"), "{message}");
    // Little-endian value and literal bytes both count.
    let le = CHECKSUM.replace("1+2+3+4 = 10 = 0x000A", "LE bytes: 0x0A00 = 2560 = 10*256");
    assert_eq!(check(&le), vec![]);
}

// ---------------------------------------------------------------------------
// Other source and case rules
// ---------------------------------------------------------------------------

fn with_source(source: &str) -> String {
    format!("{DECODE_HEAD}\n[[decode]]\nraw = 500\nreal = 50.0\nsource = {source}\n")
}

#[test]
fn other_source_kinds_need_their_fields() {
    for (source, rule) in [
        (
            r#"{ kind = "standard", doc = "SAE J1979", section = " " }"#,
            Rule::EmptySource,
        ),
        (r#"{ kind = "stock-rom", rom = "" }"#, Rule::EmptySource),
        (r#"{ kind = "car-trace", trace = "" }"#, Rule::EmptySource),
        (
            r#"{ kind = "cross-source", sources = ["RomRaider ECU XML"] }"#,
            Rule::CrossSource,
        ),
        (
            r#"{ kind = "cross-source", sources = ["XDF", "xdf "] }"#,
            Rule::CrossSource,
        ),
        (
            r#"{ kind = "cross-source", sources = ["XDF", ""] }"#,
            Rule::CrossSource,
        ),
        (
            r#"{ kind = "car-trace", trace = "t1", note = "matched RomRaider Java logger" }"#,
            Rule::ForbiddenSource,
        ),
    ] {
        let message = one(&check(&with_source(source)), rule);
        assert!(
            message.contains("[[decode]] #1 (offset 0)"),
            "{source}: {message}"
        );
    }
    for source in [
        r#"{ kind = "standard", doc = "SAE J1979", section = "Annex B" }"#,
        r#"{ kind = "stock-rom", rom = "A2WC510N" }"#,
        r#"{ kind = "car-trace", trace = "traces/idle.csv" }"#,
        r#"{ kind = "cross-source", sources = ["RomRaider ECU XML", "TunerPro XDF"] }"#,
    ] {
        assert_eq!(check(&with_source(source)), vec![], "{source}");
    }
}

#[test]
fn case_rules() {
    let no_conversion = DECODE_HEAD.replace("[definition.conversion]\nto_real = \"x * 0.1\"\n", "");
    let text = format!("{no_conversion}{GOOD_CASE}");
    one(&check(&text), Rule::RealWithoutConversion);

    let text = format!(
        "{DECODE_HEAD}\n[[decode]]\noffset = 3\nraw = 56\nsource = {{ kind = \"stock-rom\", rom = \"R\" }}\n"
    );
    let message = one(&check(&text), Rule::OffsetOutOfRange);
    assert!(message.contains("[[decode]] #1 (offset 3)"), "{message}");

    for (tolerance, real) in [("-1.0", "real = 50.0\n"), ("0.1", "")] {
        let text = format!(
            "{DECODE_HEAD}\n[[decode]]\nraw = 500\n{real}tolerance = {tolerance}\n\
             source = {{ kind = \"stock-rom\", rom = \"R\" }}\n"
        );
        one(&check(&text), Rule::Tolerance);
    }

    let text = CHECKSUM.replace("offset = 4", "offset = 5");
    let message = one(&check(&text), Rule::OffsetOutOfRange);
    assert!(message.contains("[[checksum]] #1"), "{message}");

    let text = CHECKSUM
        .replace(r#"after = { hex = "00 0A""#, r#"after = { hex = "0A""#)
        .replace("1+2+3+4 = 10 = 0x000A", "1+2+3+4 = 10 = 0x0A");
    one(&check(&text), Rule::ChecksumWidth);

    let text = format!("{CHECKSUM}\n[[changelog]]\ndate = 2026-10-19\nreason = \" \"\n");
    let message = one(&check(&text), Rule::ChangelogReason);
    assert!(message.contains("[[changelog]] #1"), "{message}");
}

#[test]
fn unloadable_fixture_is_a_load_violation() {
    let message = one(&check("schema = 2\ntitle = \"t\"\n"), Rule::Load);
    assert!(message.contains("bad.toml"), "{message}");
    assert!(message.contains("unsupported schema 2"), "{message}");
    one(&check("not toml ["), Rule::Load);
}

// ---------------------------------------------------------------------------
// Base mode: changed expected values need a changelog entry (US-RR-9-4)
// ---------------------------------------------------------------------------

const CHANGELOG: &str =
    "\n[[changelog]]\ndate = 2026-10-20\nreason = \"500 was misread; the cell is 0x01F5\"\n";

fn against(base: &str, head: &str) -> Vec<Violation> {
    validate_text(Path::new("fixtures/kat/x.toml"), head, Some(("base", base)))
}

#[test]
fn changed_expected_value_without_changelog_is_rejected() {
    let base = format!("{DECODE_HEAD}{GOOD_CASE}");
    let head = base
        .replace("01 F4", "01 F5")
        .replace("raw = 500", "raw = 501")
        .replace("real = 50.0", "real = 50.1")
        .replace(
            "0x01F4 = 1*256 + 244 = 500; 500 * 0.1 = 50.0",
            "0x01F5 = 1*256 + 245 = 501; 501 * 0.1 = 50.1",
        );
    let message = one(&against(&base, &head), Rule::ChangedWithoutChangelog);
    assert!(message.contains("fixtures/kat/x.toml"), "{message}");
    assert!(message.contains("since base"), "{message}");
    assert!(message.contains("the [input] changed"), "{message}");
    assert!(
        message.contains("[[decode]] #1 (offset 0) (raw = 500, real = 50) was changed or removed"),
        "{message}"
    );

    assert_eq!(against(&base, &format!("{head}{CHANGELOG}")), vec![]);
}

#[test]
fn each_kind_of_expected_value_change_is_detected() {
    let base = format!("{DECODE_HEAD}{GOOD_CASE}{CHANGELOG}");
    let tolerance = base.replace("real = 50.0\n", "real = 50.0\ntolerance = 1e-6\n");
    let definition = base.replace("x * 0.1", "x * 0.10");
    let removed = format!("{DECODE_HEAD}{CHANGELOG}\n[[checksum]]\n");
    for head in [tolerance, definition] {
        one(&against(&base, &head), Rule::ChangedWithoutChangelog);
    }
    // A fixture with no cases left does not load at all: that is reported.
    one(&against(&base, &removed), Rule::Load);

    let checksum_base = CHECKSUM.to_owned();
    let checksum_head = CHECKSUM.replace("valid = false", "valid = true");
    let message = one(
        &against(&checksum_base, &checksum_head),
        Rule::ChangedWithoutChangelog,
    );
    assert!(
        message.contains("[[checksum]] #1 (sum16-be at offset 4"),
        "{message}"
    );

    let switched = format!("{DECODE_HEAD}{GOOD_CASE}").replace(
        "[input]\nhex = \"01 F4 FF 38\"",
        "[input]\nhex = \"01 02 03 04 00 00\"",
    );
    one(
        &against(&checksum_base, &switched),
        Rule::ChangedWithoutChangelog,
    );
}

#[test]
fn removing_a_case_needs_a_changelog_entry_but_adding_or_reordering_does_not() {
    let second = "\n[[decode]]\noffset = 2\nraw = 65336\nreal = 6533.6\nsource = { kind = \"stock-rom\", rom = \"R\" }\n";
    let base = format!("{DECODE_HEAD}{GOOD_CASE}{second}");
    let removed = format!("{DECODE_HEAD}{GOOD_CASE}");
    let message = one(&against(&base, &removed), Rule::ChangedWithoutChangelog);
    assert!(message.contains("[[decode]] #2 (offset 2)"), "{message}");

    let reordered = format!("{DECODE_HEAD}{second}{GOOD_CASE}");
    assert_eq!(against(&base, &reordered), vec![]);
    let added = format!(
        "{base}\n[[decode]]\noffset = 1\nraw = 0xF4FF\nsource = {{ kind = \"stock-rom\", rom = \"R\" }}\n"
    );
    assert_eq!(against(&base, &added), vec![]);
}

#[test]
fn source_note_and_title_edits_need_no_changelog() {
    let base = format!("{DECODE_HEAD}{GOOD_CASE}");
    let head = base
        .replace("0.1 scaling", "0.1 degC scaling")
        .replace(
            "0x01F4 = 1*256 + 244 = 500; 500 * 0.1 = 50.0",
            "bytes 01 F4 big-endian: 0x01*256 + 0xF4 = 500; 500 * 0.1 = 50.0",
        )
        .replace("real = 50.0\n", "real = 50.0\nnote = \"first cell\"\n");
    assert_eq!(against(&base, &head), vec![]);
}

#[test]
fn an_existing_changelog_entry_does_not_cover_a_new_change() {
    let base = format!("{DECODE_HEAD}{GOOD_CASE}{CHANGELOG}");
    let head = base.replace("real = 50.0", "real = 50.0\ntolerance = 1e-9");
    one(&against(&base, &head), Rule::ChangedWithoutChangelog);

    // Rewording or re-dating the old entry replaces it; it adds nothing.
    let reworded = head.replace("500 was misread", "500 was misread earlier");
    let message = one(&against(&base, &reworded), Rule::ChangedWithoutChangelog);
    assert!(
        message.contains("1 entry of the base changelog was edited or removed"),
        "{message}"
    );
    let redated = head.replace("date = 2026-10-20", "date = 2026-10-21");
    one(&against(&base, &redated), Rule::ChangedWithoutChangelog);
    // Replacing the old entry with a new one is no net addition either.
    let replaced = head.replace("500 was misread; the cell is 0x01F5", NEW_REASON);
    one(&against(&base, &replaced), Rule::ChangedWithoutChangelog);
    // A copy of the old entry (same or new date) explains nothing new.
    let copied = format!("{head}{CHANGELOG}");
    one(&against(&base, &copied), Rule::ChangedWithoutChangelog);
    let copied = format!("{head}{}", CHANGELOG.replace("2026-10-20", "2026-10-22"));
    one(&against(&base, &copied), Rule::ChangedWithoutChangelog);

    // A genuinely new entry passes, also next to a reworded old one.
    let added = format!("{head}{}", new_entry(NEW_REASON));
    assert_eq!(against(&base, &added), vec![]);
    let both = format!("{reworded}{}", new_entry(NEW_REASON));
    assert_eq!(against(&base, &both), vec![]);
}

const NEW_REASON: &str = "a tolerance is needed: the conversion is computed in f32";

fn new_entry(reason: &str) -> String {
    format!("\n[[changelog]]\ndate = 2026-10-21\nreason = {reason:?}\nauthor = \"kat-test\"\n")
}

#[test]
fn a_blank_changelog_reason_does_not_explain_a_change() {
    let base = format!("{DECODE_HEAD}{GOOD_CASE}");
    let head = base.replace("raw = 500", "raw = 0x1F4\ntolerance = 0.0");
    for reason in ["", "   ", "\t\n"] {
        let violations = against(&base, &format!("{head}{}", new_entry(reason)));
        let rules: Vec<Rule> = violations.iter().map(|v| v.rule).collect();
        assert_eq!(
            rules,
            [Rule::ChangelogReason, Rule::ChangedWithoutChangelog],
            "{reason:?}: {violations:#?}"
        );
    }
    // Two blank entries are not one good one either.
    let two = format!("{head}{}{}", new_entry(""), new_entry(" "));
    assert!(
        against(&base, &two)
            .iter()
            .any(|v| v.rule == Rule::ChangedWithoutChangelog)
    );
}

#[test]
fn moving_offsets_or_input_under_unchanged_values_is_a_change() {
    let second = "\n[[decode]]\noffset = 2\nraw = 65336\nreal = 6533.6\nsource = { kind = \"stock-rom\", rom = \"R\" }\n";
    let first = GOOD_CASE.replace(
        "source = { kind = \"hand-calc\", working = \"0x01F4 = 1*256 + 244 = 500; 500 * 0.1 = 50.0\" }",
        "source = { kind = \"stock-rom\", rom = \"R\" }",
    );
    let base = format!("{DECODE_HEAD}{first}{second}");
    // The same two values, each now claimed for the other cell.
    let swapped = base
        .replace("offset = 0\n", "offset = X\n")
        .replace("offset = 2\n", "offset = 0\n")
        .replace("offset = X\n", "offset = 2\n");
    let message = one(&against(&base, &swapped), Rule::ChangedWithoutChangelog);
    assert!(message.contains("[[decode]] #1 (offset 0)"), "{message}");
    assert!(message.contains("[[decode]] #2 (offset 2)"), "{message}");
    // The same values for different input bytes.
    let input = base.replace("01 F4 FF 38", "01 F4 FF 39");
    let message = one(&against(&base, &input), Rule::ChangedWithoutChangelog);
    assert!(message.contains("the [input] changed"), "{message}");
    // The same values read with a different encoding.
    let endian = base.replace("endian = \"big\"", "endian = \"little\"");
    let message = one(&against(&base, &endian), Rule::ChangedWithoutChangelog);
    assert!(message.contains("the [definition] changed"), "{message}");
}

#[test]
fn new_fixture_and_unloadable_base_are_not_compared() {
    let head = format!("{DECODE_HEAD}{GOOD_CASE}");
    assert_eq!(against("schema = 1\n", &head), vec![]);

    struct NoBase;
    impl BaseSource for NoBase {
        fn label(&self) -> &str {
            "none"
        }
        fn read(&self, _: &Path) -> Result<Option<String>, ValidateError> {
            Ok(None)
        }
    }
    let dir = scratch("no-base").unwrap();
    fs::write(dir.join("a.toml"), &head).unwrap();
    let report = validate::validate_files(&[dir.join("a.toml")], Some(&NoBase)).unwrap();
    assert_eq!((report.files, report.compared), (1, 0));
    assert!(report.violations.is_empty());
}

// ---------------------------------------------------------------------------
// Git base (temporary repository) and the command line
// ---------------------------------------------------------------------------

fn git(dir: &Path, args: &[&str]) -> io::Result<()> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=kat-test",
            "-c",
            "user.email=kat-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()?;
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(())
}

/// A git repo with `fixtures/kat/u16.toml` committed (tagged `base`).
fn repo(name: &str) -> io::Result<(PathBuf, PathBuf, String)> {
    let dir = scratch(name)?;
    git(&dir, &["init", "--quiet"])?;
    let kat = dir.join("fixtures").join("kat");
    fs::create_dir_all(&kat)?;
    let fixture = kat.join("u16.toml");
    let text = format!("{DECODE_HEAD}{GOOD_CASE}");
    fs::write(&fixture, &text)?;
    git(&dir, &["add", "."])?;
    git(&dir, &["commit", "--quiet", "-m", "seed"])?;
    git(&dir, &["tag", "base"])?;
    Ok((dir, fixture, text))
}

const CHANGED: [(&str, &str); 2] = [
    ("raw = 500", "raw = 499"),
    (
        "0x01F4 = 1*256 + 244 = 500; 500 * 0.1 = 50.0",
        "0x01F4 = 1*256 + 244 = 500; less 1 = 499",
    ),
];

fn changed(text: &str) -> String {
    let text = CHANGED
        .iter()
        .fold(text.to_owned(), |t, (from, to)| t.replace(from, to));
    text.replace("real = 50.0\n", "")
}

#[test]
fn git_base_detects_a_changed_value_and_accepts_a_changelog_entry() {
    let (dir, fixture, text) = repo("git-base").unwrap();
    let base = GitBase::resolve(&dir, "base").unwrap();
    assert_eq!(base.commit().len(), 40, "{}", base.commit());
    assert_eq!(base.read(&fixture).unwrap().as_deref(), Some(text.as_str()));
    assert_eq!(base.read(&dir.join("fixtures/kat/new.toml")).unwrap(), None);

    // Committed change on top of the base: compared against the base commit.
    fs::write(&fixture, changed(&text)).unwrap();
    git(&dir, &["commit", "--quiet", "-am", "change"]).unwrap();
    let files = validate::discover(&[dir.join("fixtures")]).unwrap();
    let report = validate::validate_files(&files, Some(&base)).unwrap();
    assert_eq!((report.files, report.compared), (1, 1));
    let message = one(&report.violations, Rule::ChangedWithoutChangelog);
    assert!(message.contains("u16.toml"), "{message}");
    assert!(message.contains("since base ("), "{message}");

    // Uncommitted fix in the working tree: the changelog entry passes.
    fs::write(&fixture, format!("{}{CHANGELOG}", changed(&text))).unwrap();
    let report = validate::validate_files(&files, Some(&base)).unwrap();
    assert!(report.violations.is_empty(), "{:#?}", report.violations);
}

/// `git rev-parse <rev>` in `dir`.
fn rev(dir: &Path, rev: &str) -> io::Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--verify", rev])
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()?;
    assert!(out.status.success(), "rev-parse {rev}");
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Validates every fixture under `dir/fixtures` against `base`.
fn report(dir: &Path, base: &GitBase) -> Result<validate::Report, ValidateError> {
    let files = validate::discover(&[dir.join("fixtures")])?;
    validate::validate_files(&files, Some(base))
}

#[test]
fn renaming_or_moving_a_fixture_does_not_hide_a_changed_value() {
    let (dir, fixture, text) = repo("git-rename").unwrap();
    let moved_dir = dir.join("fixtures").join("kat").join("moved");
    fs::create_dir_all(&moved_dir).unwrap();
    git(
        &dir,
        &[
            "mv",
            "fixtures/kat/u16.toml",
            "fixtures/kat/moved/u16-be.toml",
        ],
    )
    .unwrap();
    let moved = moved_dir.join("u16-be.toml");
    assert!(!fixture.exists());

    // A pure rename (committed) is compared, and is not a change.
    git(&dir, &["commit", "--quiet", "-m", "rename"]).unwrap();
    let base = GitBase::resolve(&dir, "base").unwrap();
    assert_eq!(base.read(&moved).unwrap().as_deref(), Some(text.as_str()));
    let r = report(&dir, &base).unwrap();
    assert_eq!((r.files, r.compared), (1, 1));
    assert!(r.violations.is_empty(), "{:#?}", r.violations);

    // Rename plus a changed value, committed: rejected.
    fs::write(&moved, changed(&text)).unwrap();
    git(&dir, &["commit", "--quiet", "-am", "change"]).unwrap();
    let base = GitBase::resolve(&dir, "base").unwrap();
    let r = report(&dir, &base).unwrap();
    assert_eq!((r.files, r.compared), (1, 1));
    let message = one(&r.violations, Rule::ChangedWithoutChangelog);
    assert!(message.contains("u16-be.toml"), "{message}");

    // With a new changelog entry it passes.
    fs::write(&moved, format!("{}{CHANGELOG}", changed(&text))).unwrap();
    assert!(report(&dir, &base).unwrap().violations.is_empty());
}

/// A repository shaped like CI's checkout after a push: `origin/master`
/// (no fixtures), `origin/rust-rewrite` (adds the fixture, tagged `base`)
/// and the pushed branch `feature`: a commit that changes an expected value
/// without a changelog entry, then an unrelated commit. Returns the directory, the
/// fixture, the base text, and the master and pushed commits.
fn pushed(name: &str) -> io::Result<(PathBuf, PathBuf, String, String, String)> {
    let dir = scratch(name)?;
    git(&dir, &["init", "--quiet"])?;
    fs::write(dir.join("README"), "master\n")?;
    git(&dir, &["add", "."])?;
    git(&dir, &["commit", "--quiet", "-m", "master"])?;
    let master = rev(&dir, "HEAD")?;
    let kat = dir.join("fixtures").join("kat");
    fs::create_dir_all(&kat)?;
    let fixture = kat.join("u16.toml");
    let text = format!("{DECODE_HEAD}{GOOD_CASE}");
    fs::write(&fixture, &text)?;
    git(&dir, &["add", "."])?;
    git(&dir, &["commit", "--quiet", "-m", "seed"])?;
    git(&dir, &["tag", "base"])?;
    let seed = rev(&dir, "HEAD")?;
    fs::write(&fixture, changed(&text))?;
    git(&dir, &["commit", "--quiet", "-am", "change"])?;
    // A later commit that leaves the fixture alone: the base must reach back
    // past it to the fork point.
    fs::write(dir.join("README"), "feature\n")?;
    git(&dir, &["commit", "--quiet", "-am", "unrelated"])?;
    let head = rev(&dir, "HEAD")?;
    for (name, at) in [
        ("master", &master),
        ("HEAD", &master),
        ("rust-rewrite", &seed),
        ("feature", &head),
    ] {
        git(
            &dir,
            &["update-ref", &format!("refs/remotes/origin/{name}"), at],
        )?;
    }
    Ok((dir, fixture, text, master, head))
}

const ZEROS: &str = "0000000000000000000000000000000000000000";

#[test]
fn push_base_checks_a_new_branch_against_where_it_forked() {
    let (dir, _, _, master, _) = pushed("push-new-branch").unwrap();
    let seed = rev(&dir, "base").unwrap();
    // The default branch has no fixtures: comparing with it checks nothing,
    // which is why a new branch is not compared with it.
    let r = report(&dir, &GitBase::resolve(&dir, "origin/master").unwrap()).unwrap();
    assert_eq!((r.compared, r.violations.len()), (0, 0));
    assert_eq!(
        GitBase::resolve(&dir, "origin/master").unwrap().commit(),
        master
    );

    for before in ["", "  ", ZEROS] {
        let base = GitBase::for_push(&dir, before).unwrap();
        assert_eq!(base.commit(), seed, "{before:?}");
        let r = report(&dir, &base).unwrap();
        assert_eq!(r.compared, 1, "{before:?}");
        let message = one(&r.violations, Rule::ChangedWithoutChangelog);
        assert!(
            message.contains("where the pushed commits leave"),
            "{message}"
        );
    }
}

#[test]
fn push_base_is_the_previous_head_on_an_ordinary_push() {
    let (dir, fixture, text, _, head) = pushed("push-ordinary").unwrap();
    // The push that brought the change: before = the seed commit.
    let seed = rev(&dir, "base").unwrap();
    let base = GitBase::for_push(&dir, &seed).unwrap();
    assert_eq!(base.commit(), seed);
    assert!(
        base.label().starts_with("the commit before the push"),
        "{}",
        base.label()
    );
    one(
        &report(&dir, &base).unwrap().violations,
        Rule::ChangedWithoutChangelog,
    );

    // A later push that only adds the changelog entry: before = the change.
    fs::write(&fixture, format!("{}{CHANGELOG}", changed(&text))).unwrap();
    git(&dir, &["commit", "--quiet", "-am", "explain"]).unwrap();
    let base = GitBase::for_push(&dir, &head).unwrap();
    assert_eq!(base.commit(), head);
    assert!(report(&dir, &base).unwrap().violations.is_empty());
}

#[test]
fn push_base_after_a_force_push_still_checks_the_rewritten_commits() {
    let (dir, fixture, text, _, head) = pushed("push-force").unwrap();
    let seed = rev(&dir, "base").unwrap();
    // Rewrite the branch: the new head is another commit on the seed with
    // the same unexplained change.
    git(&dir, &["reset", "--quiet", "--hard", &seed]).unwrap();
    fs::write(&fixture, changed(&text).replace("Big", "big")).unwrap();
    fs::write(dir.join("other"), "rewritten\n").unwrap();
    git(&dir, &["add", "."]).unwrap();
    git(&dir, &["commit", "--quiet", "-m", "rewritten"]).unwrap();
    let new_head = rev(&dir, "HEAD").unwrap();
    git(
        &dir,
        &["update-ref", "refs/remotes/origin/feature", &new_head],
    )
    .unwrap();

    // Old head still present: compared with the fork point of old and new.
    let base = GitBase::for_push(&dir, &head).unwrap();
    assert_eq!(base.commit(), seed);
    one(
        &report(&dir, &base).unwrap().violations,
        Rule::ChangedWithoutChangelog,
    );
    // Old head gone (not fetched): compared with where HEAD leaves the
    // other branches.
    let gone = "1234567890abcdef1234567890abcdef12345678";
    let base = GitBase::for_push(&dir, gone).unwrap();
    assert_eq!(base.commit(), seed);
    one(
        &report(&dir, &base).unwrap().violations,
        Rule::ChangedWithoutChangelog,
    );
}

#[test]
fn push_base_without_other_branches_is_the_root_commit() {
    // No remote-tracking branches at all (a fresh repository): every commit
    // is new, so the base is the root and later changes are still checked.
    let (dir, fixture, text) = repo("push-root").unwrap();
    let root = rev(&dir, "HEAD").unwrap();
    fs::write(&fixture, changed(&text)).unwrap();
    git(&dir, &["commit", "--quiet", "-am", "change"]).unwrap();
    let base = GitBase::for_push(&dir, ZEROS).unwrap();
    assert_eq!(base.commit(), root);
    assert!(
        base.label().starts_with("the root of the pushed commits"),
        "{}",
        base.label()
    );
    one(
        &report(&dir, &base).unwrap().violations,
        Rule::ChangedWithoutChangelog,
    );
}

#[test]
fn git_base_rejects_unknown_revisions() {
    let (dir, _, _) = repo("git-bad-rev").unwrap();
    let error = GitBase::resolve(&dir, "no-such-rev").unwrap_err();
    assert!(
        error.to_string().contains("does not name a commit"),
        "{error}"
    );
}

/// Runs the `validate-fixtures` command line (`validate::cli`, what the
/// example and `cargo xtask validate-fixtures` run) in `dir`.
fn bin(dir: &Path, args: &[&str]) -> (u8, String, String) {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let code = validate::cli(
        args.iter().map(std::ffi::OsString::from),
        dir,
        &mut stdout,
        &mut stderr,
    );
    (
        code,
        String::from_utf8_lossy(&stdout).into_owned(),
        String::from_utf8_lossy(&stderr).into_owned(),
    )
}

#[test]
fn command_line_exit_status_follows_the_violations() {
    let (code, stdout, stderr) = bin(Path::new(env!("CARGO_MANIFEST_DIR")), &[]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("validate-fixtures: ok ("), "{stdout}");

    let (dir, fixture, text) = repo("bin").unwrap();
    let (code, stdout, stderr) = bin(&dir, &["--base", "base", "fixtures"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("1 compared with base"), "{stdout}");

    fs::write(&fixture, changed(&text)).unwrap();
    let (code, _, stderr) = bin(&dir, &["--base=base", "fixtures"]);
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr.contains("validate-fixtures: fixtures/kat/u16.toml: expected values changed"),
        "{stderr}"
    );
    assert!(stderr.contains("[changed-without-changelog]"), "{stderr}");

    // CI on a new branch's first push (before = all zeros, or empty).
    git(&dir, &["commit", "--quiet", "-am", "change"]).unwrap();
    for args in [
        &["--push-before", ZEROS, "fixtures"][..],
        &["--push-before=", "fixtures"],
    ] {
        let (code, _, stderr) = bin(&dir, args);
        assert_eq!(code, 1, "{args:?}: {stderr}");
        assert!(stderr.contains("[changed-without-changelog]"), "{stderr}");
        assert!(
            stderr.contains("compared with the root of the pushed commits"),
            "{stderr}"
        );
    }

    // Without --base only the static rules apply.
    let (code, _, stderr) = bin(&dir, &["fixtures"]);
    assert_eq!(code, 0, "{stderr}");

    let no_source = format!("{DECODE_HEAD}\n[[decode]]\nraw = 500\n");
    fs::write(&fixture, no_source).unwrap();
    let (code, _, stderr) = bin(&dir, &["fixtures/kat/u16.toml"]);
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr.contains(
            "fixtures/kat/u16.toml: [[decode]] #1 (offset 0): expected value has no `source`"
        ),
        "{stderr}"
    );

    for args in [
        &["--bogus"][..],
        &["--base"],
        &["missing.toml"],
        &["--base", "nope"],
        &["--push-before"],
        &["--base", "base", "--push-before", ZEROS],
    ] {
        let (code, _, stderr) = bin(&dir, args);
        assert_eq!(code, 2, "{args:?}: {stderr}");
        assert!(stderr.contains("validate-fixtures: error:"), "{stderr}");
    }
}

// ---------------------------------------------------------------------------
// Expression fixtures (`[[expr]]`, US-RR-20-2)
// ---------------------------------------------------------------------------

fn expr(formula: &str, value: &str, extra: &str, working: &str) -> String {
    format!(
        "schema = 1\ntitle = \"expr\"\n\n[[expr]]\nformula = {formula:?}\nvalue = {value}\n{extra}\
         source = {{ kind = \"hand-calc\", working = {working:?} }}\n"
    )
}

#[test]
fn expr_cases_follow_the_working_rule() {
    assert_eq!(
        check(&expr("2+3*4", "14.0", "", "2 + (3*4) = 2 + 12 = 14")),
        vec![]
    );
    let message = one(
        &check(&expr("2+3*4", "14.0", "", "(2 + 3)*4 = 5*4 = 20")),
        Rule::HandCalcWorking,
    );
    assert!(message.contains("[[expr]] #1 (`2+3*4`)"), "{message}");
    assert!(message.contains("expected value 14"), "{message}");
    one(
        &check(&expr("14", "14.0", "", "14 = 14")),
        Rule::HandCalcWorking,
    );

    // A value that is not finite is named by word, with the sign of an infinity.
    let nan = "-8 < 0 and 1/3 = 0.333 is not an integer, so (-8)^(1/3) = NaN";
    assert_eq!(check(&expr("(-8)^(1/3)", "nan", "", nan)), vec![]);
    let no_word = "-8 < 0 and 1/3 = 0.333 is not an integer";
    one(
        &check(&expr("(-8)^(1/3)", "nan", "", no_word)),
        Rule::HandCalcWorking,
    );
    let banana = "-8 < 0 and 1/3 = 0.333: nanana";
    one(
        &check(&expr("(-8)^(1/3)", "nan", "", banana)),
        Rule::HandCalcWorking,
    );
    assert_eq!(
        check(&expr("1/0", "inf", "", "1/0 = 1 / +0 = +inf")),
        vec![]
    );
    assert_eq!(
        check(&expr("-1/0", "-inf", "", "-1/0 = (-1) / +0 = -Infinity")),
        vec![]
    );
    one(
        &check(&expr("-1/0", "-inf", "", "-1/0 = (-1) / 0 = inf")),
        Rule::HandCalcWorking,
    );
    one(
        &check(&expr("1/0", "inf", "", "1/0 = 1 / 0 = -inf")),
        Rule::HandCalcWorking,
    );

    // Every expected value needs a source.
    let missing = "schema = 1\ntitle = \"e\"\n[[expr]]\nformula = \"1+1\"\nvalue = 2.0\n";
    let message = one(&check(missing), Rule::MissingSource);
    assert!(message.contains("[[expr]] #1 (`1+1`)"), "{message}");
}

#[test]
fn expr_tolerance_is_finite_and_only_for_a_finite_value() {
    let sqrt2 = "1.41421356237 * 1.41421356237 = 2.0000000000; so 2^0.5 = 1.4142135623730951";
    let tolerance = |t: &str| format!("tolerance = {t}\n");
    assert_eq!(
        check(&expr(
            "2^0.5",
            "1.4142135623730951",
            &tolerance("1e-15"),
            sqrt2
        )),
        vec![]
    );
    one(
        &check(&expr(
            "2^0.5",
            "1.4142135623730951",
            &tolerance("-1.0"),
            sqrt2,
        )),
        Rule::Tolerance,
    );
    // An infinite tolerance also swallows every number of the working.
    let infinite = check(&expr(
        "2^0.5",
        "1.4142135623730951",
        &tolerance("inf"),
        sqrt2,
    ));
    assert!(
        infinite.iter().any(|v| v.rule == Rule::Tolerance),
        "{infinite:#?}"
    );
    let message = one(
        &check(&expr(
            "1/0",
            "inf",
            &tolerance("0.5"),
            "1/0 = 1 / +0 = +inf",
        )),
        Rule::Tolerance,
    );
    assert!(message.contains("finite `value`"), "{message}");
}

#[test]
fn changed_expr_value_needs_a_changelog_and_nan_matches_itself() {
    let base = format!(
        "{}\n[[expr]]\nformula = \"(-8)^(1/3)\"\nvalue = nan\n\
         source = {{ kind = \"standard\", doc = \"d\", section = \"s\" }}\n",
        expr("2+3*4", "14.0", "", "2 + (3*4) = 2 + 12 = 14")
    );
    // Same cases, reordered and with a reworded source: not a change.
    let reordered = "schema = 1\ntitle = \"expr\"\n\n[[expr]]\nformula = \"(-8)^(1/3)\"\nvalue = nan\n\
         source = { kind = \"standard\", doc = \"d2\", section = \"s\" }\n\n[[expr]]\n\
         formula = \"2+3*4\"\nvalue = 14.0\n\
         source = { kind = \"hand-calc\", working = \"2 + 12 = 14\" }\n";
    assert_eq!(against(&base, reordered), vec![]);

    for head in [
        base.replace("value = 14.0", "value = 20.0")
            .replace("2 + (3*4) = 2 + 12 = 14", "(2 + 3)*4 = 5*4 = 20"),
        base.replace("formula = \"2+3*4\"", "formula = \"2+3*5\"")
            .replace("value = 14.0", "value = 17.0")
            .replace("2 + (3*4) = 2 + 12 = 14", "2 + (3*5) = 2 + 15 = 17"),
        base.replace("value = 14.0", "x = 1.0\nvalue = 14.0"),
        base.replace("value = nan", "value = inf"),
    ] {
        let message = one(&against(&base, &head), Rule::ChangedWithoutChangelog);
        assert!(message.contains("was changed or removed"), "{message}");
        assert_eq!(against(&base, &format!("{head}{CHANGELOG}")), vec![]);
    }

    let decode_head = format!("{DECODE_HEAD}{GOOD_CASE}");
    let message = one(&against(&decode_head, &base), Rule::ChangedWithoutChangelog);
    assert!(
        message.contains(
            "switched between [[decode]], [[checksum]], [[expr]], [[unit]], [[timing]] and [[codec]]"
        ),
        "{message}"
    );
}

// ---------------------------------------------------------------------------
// [range] (US-RR-24-2)
// ---------------------------------------------------------------------------

/// A falling `[range]` for `DECODE_HEAD` with a substituted conversion.
const RANGE: &str = r#"
[range]
real_min = -27.5
raw_at_min = 255
real_max = 100.0
raw_at_max = 0
monotonicity = "decreasing"
source = { kind = "hand-calc", working = "factor -0.5 < 0: max = 100 - 0*0.5 = 100.0 at raw 0; min = 100 - 255*0.5 = -27.5 at raw 255" }
"#;

fn with_range(range: &str) -> String {
    format!("{DECODE_HEAD}{GOOD_CASE}{range}")
}

#[test]
fn a_good_range_passes() {
    assert_eq!(check(&with_range(RANGE)), vec![]);
    let increasing = RANGE
        .replace("real_min = -27.5", "real_min = 0.0")
        .replace("raw_at_min = 255", "raw_at_min = 0")
        .replace("real_max = 100.0", "real_max = 6553.5")
        .replace("raw_at_max = 0", "raw_at_max = 65535")
        .replace("\"decreasing\"", "\"increasing\"")
        .replace(
            r#"working = "factor -0.5 < 0: max = 100 - 0*0.5 = 100.0 at raw 0; min = 100 - 255*0.5 = -27.5 at raw 255""#,
            r#"working = "min = 0*0.1 = 0.0 at raw 0; max = 65535*0.1 = 6553.5 at raw 65535""#,
        );
    assert_eq!(check(&with_range(&increasing)), vec![]);
}

#[test]
fn range_without_source_or_working_is_rejected_at_the_range() {
    let no_source: String = RANGE
        .lines()
        .filter(|l| !l.starts_with("source"))
        .map(|l| format!("{l}\n"))
        .collect();
    let violations = check(&with_range(&no_source));
    one(&violations, Rule::MissingSource);
    assert_eq!(violations[0].location.as_deref(), Some("[range]"));

    // The working must reach each of the four values.
    for (missing, working) in [
        (
            "raw_at_min",
            "max = 100 - 0*0.5 = 100.0 at raw 0; min = 100 - 127.5 = -27.5",
        ),
        (
            "raw_at_max",
            "max = 100 - 0.5*1 = 100.0; min = 100 - 255*0.5 = -27.5 at raw 255",
        ),
        (
            "real_min",
            "max = 100 - 0*0.5 = 100.0 at raw 0; min at raw 255 = 100 - 255*0.5",
        ),
        (
            "real_max",
            "max at raw 0; min = 50*2 - 255*0.5 = -27.5 at raw 255",
        ),
    ] {
        let text = with_range(&RANGE.replace(
            "factor -0.5 < 0: max = 100 - 0*0.5 = 100.0 at raw 0; min = 100 - 255*0.5 = -27.5 at raw 255",
            working,
        ));
        let message = one(&check(&text), Rule::HandCalcWorking);
        assert!(message.contains(missing), "{missing}: {message}");
        assert!(message.contains("[range]"), "{message}");
    }
}

#[test]
fn range_rules() {
    let no_conversion = DECODE_HEAD.replace("[definition.conversion]\nto_real = \"x * 0.1\"\n", "");
    let text = format!(
        "{no_conversion}\n[[decode]]\nraw = 500\nsource = {{ kind = \"stock-rom\", rom = \"R\" }}\n{RANGE}"
    );
    let message = one(&check(&text), Rule::RealWithoutConversion);
    assert!(message.contains("[range]"), "{message}");

    let text = with_range(&RANGE.replace("monotonicity", "tolerance = -1.0\nmonotonicity"));
    one(&check(&text), Rule::Tolerance);

    // MF-13 written into a fixture: min and max swapped.
    let swapped = RANGE
        .replace("real_min = -27.5", "real_min = 100.0")
        .replace("real_max = 100.0", "real_max = -27.5");
    let message = one(&check(&with_range(&swapped)), Rule::Range);
    assert!(message.contains("swapped"), "{message}");

    // Decreasing, but the minimum is put at the lowest raw value.
    let raws = RANGE
        .replace("raw_at_min = 255", "raw_at_min = 0")
        .replace("raw_at_max = 0", "raw_at_max = 255");
    let message = one(&check(&with_range(&raws)), Rule::Range);
    assert!(message.contains("decreasing"), "{message}");

    let increasing = RANGE.replace("\"decreasing\"", "\"increasing\"");
    let message = one(&check(&with_range(&increasing)), Rule::Range);
    assert!(message.contains("increasing"), "{message}");

    let constant = RANGE.replace("\"decreasing\"", "\"constant\"");
    let message = one(&check(&with_range(&constant)), Rule::Range);
    assert!(message.contains("constant"), "{message}");

    let flat = RANGE.replace("real_min = -27.5", "real_min = 100.0");
    let flat = flat.replace("= -27.5 at raw 255", "= 100.0 at raw 255");
    let message = one(&check(&with_range(&flat)), Rule::Range);
    assert!(message.contains("`constant`"), "{message}");

    let non_finite = RANGE.replace("real_min = -27.5", "real_min = -inf");
    let violations = check(&with_range(&non_finite));
    let rules: Vec<Rule> = violations.iter().map(|v| v.rule).collect();
    assert_eq!(
        rules,
        [Rule::HandCalcWorking, Rule::Range],
        "{violations:#?}"
    );
    assert!(violations[1].message.contains("finite"), "{violations:#?}");

    // A non-monotonic range may put its extremes anywhere.
    let turns = raws.replace("\"decreasing\"", "\"non-monotonic\"");
    assert_eq!(check(&with_range(&turns)), vec![]);
}

#[test]
fn changing_or_removing_a_range_needs_a_changelog_entry_but_adding_one_does_not() {
    let base = with_range(RANGE);
    assert_eq!(against(&format!("{DECODE_HEAD}{GOOD_CASE}"), &base), vec![]);

    let removed = format!("{DECODE_HEAD}{GOOD_CASE}");
    let message = one(&against(&base, &removed), Rule::ChangedWithoutChangelog);
    assert!(
        message.contains("the [range] (min -27.5 at raw 255, max 100 at raw 0, decreasing)"),
        "{message}"
    );
    assert_eq!(against(&base, &format!("{removed}{CHANGELOG}")), vec![]);

    for head in [
        RANGE.replace("real_min = -27.5", "real_min = -27.5\ntolerance = 1e-9"),
        RANGE.replace("\"decreasing\"", "\"non-monotonic\""),
        RANGE.replace("raw_at_max = 0", "raw_at_max = 0x00"),
    ] {
        let head = with_range(&head);
        if head == base {
            continue;
        }
        let violations = against(&base, &head);
        if head.contains("0x00") {
            // The same raw value spelled differently is not a change.
            assert_eq!(violations, vec![]);
        } else {
            one(&violations, Rule::ChangedWithoutChangelog);
        }
    }

    // Editing only the source or note is not a change.
    let reworded = with_range(&RANGE.replace("factor -0.5 < 0", "the factor -0.5 is negative"));
    assert_eq!(against(&base, &reworded), vec![]);
}

// ---------------------------------------------------------------------------
// [[unit]] fixtures (US-RR-21-4)
// ---------------------------------------------------------------------------

/// A one-case unit fixture: 1 `from` is `expected` `to`, with `extra` keys
/// and a hand-calc `working`.
fn unit(from: &str, to: &str, expected: &str, extra: &str, working: &str) -> String {
    format!(
        "schema = 1\ntitle = \"unit\"\n\n[[unit]]\nfrom = {from:?}\nto = {to:?}\nvalue = 1.0\n\
         expected = {expected}\n{extra}source = {{ kind = \"hand-calc\", working = {working:?} }}\n"
    )
}

const MILE: &str = "1 mi = 1609.344 m = 1609.344 / 1000 km = 1.609344 km";

#[test]
fn unit_cases_follow_the_source_and_tolerance_rules() {
    assert_eq!(check(&unit("mi", "km", "1.609344", "", MILE)), vec![]);
    // The working must arrive at `expected`.
    let message = one(
        &check(&unit(
            "mi",
            "km",
            "1.609344",
            "",
            "1 mi = 1609 m = 1609 / 1000 km = 1.609 km",
        )),
        Rule::HandCalcWorking,
    );
    assert!(message.contains("[[unit]] #1 (1 mi -> km)"), "{message}");
    assert!(message.contains("expected value 1.609344 km"), "{message}");
    // ... and show more than the result.
    one(
        &check(&unit(
            "mi",
            "km",
            "1.609344",
            "",
            "1.609344 = 1.609344 * 1.609344 / 1.609344",
        )),
        Rule::HandCalcWorking,
    );
    // A tolerance is finite and non-negative.
    assert_eq!(
        check(&unit("mi", "km", "1.609344", "tolerance = 1e-15\n", MILE)),
        vec![]
    );
    let message = one(
        &check(&unit("mi", "km", "1.609344", "tolerance = -1.0\n", MILE)),
        Rule::Tolerance,
    );
    assert!(message.contains("[[unit]] #1"), "{message}");
    // Sources are checked as for any case.
    let java = unit("mi", "km", "1.609344", "", MILE).replace(
        &format!("{{ kind = \"hand-calc\", working = {MILE:?} }}"),
        "{ kind = \"standard\", doc = \"the Java app\", section = \"dyno\" }",
    );
    one(&check(&java), Rule::ForbiddenSource);
    let missing = "schema = 1\ntitle = \"u\"\n[[unit]]\nfrom = \"mi\"\nto = \"km\"\nvalue = 1.0\nexpected = 1.609344\n";
    let message = one(&check(missing), Rule::MissingSource);
    assert!(message.contains("[[unit]] #1 (1 mi -> km)"), "{message}");
}

#[test]
fn changed_unit_value_needs_a_changelog() {
    let base = unit("mi", "km", "1.609344", "", MILE);
    // A reworded source is not a change.
    assert_eq!(
        against(&base, &base.replace("= 1.609344 km", "= 1.6093440 km")),
        vec![]
    );
    for head in [
        unit("mi", "km", "1.609", "tolerance = 0.001\n", MILE),
        unit(
            "mi",
            "m",
            "1609.344",
            "",
            "1 mi = 1609.344 * 1 m = 1609.344 m",
        ),
        unit("mph", "km", "1.609344", "", MILE),
        base.replace("value = 1.0", "value = 2.0"),
    ] {
        let message = one(&against(&base, &head), Rule::ChangedWithoutChangelog);
        assert!(
            message.contains("[[unit]] #1 (1 mi -> km) (expected = 1.609344, tolerance = None) was changed or removed"),
            "{message}"
        );
        assert_eq!(against(&base, &format!("{head}{CHANGELOG}")), vec![]);
    }
    let message = one(
        &against(&base, &format!("{DECODE_HEAD}{GOOD_CASE}")),
        Rule::ChangedWithoutChangelog,
    );
    assert!(message.contains("switched between"), "{message}");
}

// ---------------------------------------------------------------------------
// [[timing]] fixtures (US-RR-14-5)
// ---------------------------------------------------------------------------

/// A one-case timing fixture: `name` is `value` (`ms = 55` / `us = 100`),
/// with a `source`.
fn timing(name: &str, value: &str, source: &str) -> String {
    format!(
        "schema = 1\ntitle = \"timing\"\n{ISO_14230_2}\n[[timing]]\nname = {name:?}\n{value}\nsource = {source}\n"
    )
}

/// The `[[citation]]` a timing fixture needs (rule `citation`, US-RR-14-1).
const ISO_14230_2: &str = "\n[[citation]]\nkind = \"standard\"\ndoc = \"ISO 14230-2\"\n\
     edition = \"1999 (first edition, 1999-03-15)\"\nsection = \"4.5.1, Table 3\"\n";

const TABLE_3: &str =
    "{ kind = \"standard\", doc = \"ISO 14230-2:1999\", section = \"4.5.1, Table 3\" }";

#[test]
fn timing_cases_follow_the_source_rules() {
    assert_eq!(
        check(&timing("iso14230_2::P3_MIN", "ms = 55", TABLE_3)),
        vec![]
    );
    let message = one(
        &check(&timing(
            "iso14230_2::P3_MIN",
            "ms = 55",
            "{ kind = \"standard\", doc = \"ISO 14230-2:1999\", section = \" \" }",
        )),
        Rule::EmptySource,
    );
    assert!(
        message.contains("[[timing]] #1 (iso14230_2::P3_MIN)"),
        "{message}"
    );
    one(
        &check(&timing(
            "iso14230_2::P3_MIN",
            "ms = 55",
            "{ kind = \"standard\", doc = \"ISO 14230-2 per NCSLoggerConnection.java\", section = \"111\" }",
        )),
        Rule::ForbiddenSource,
    );
    // A hand calculation must arrive at the value, and show more than it.
    assert_eq!(
        check(&timing(
            "iso15765_2::STMIN_SUB_MS_MIN",
            "us = 100",
            "{ kind = \"hand-calc\", working = \"F1h = 1 * 100 us = 100 us\" }",
        )),
        vec![]
    );
    let message = one(
        &check(&timing(
            "iso15765_2::STMIN_SUB_MS_MIN",
            "us = 100",
            "{ kind = \"hand-calc\", working = \"F2h = 2 * 50 us * 2 = 200 us\" }",
        )),
        Rule::HandCalcWorking,
    );
    assert!(message.contains("expected value 100 us"), "{message}");
    let missing = "schema = 1\ntitle = \"t\"\n[[timing]]\nname = \"iso14230_2::P3_MIN\"\nms = 55\n";
    let message = one(&check(missing), Rule::MissingSource);
    assert!(
        message.contains("[[timing]] #1 (iso14230_2::P3_MIN)"),
        "{message}"
    );
}

#[test]
fn changed_timing_value_needs_a_changelog() {
    let base = timing("iso14230_2::P3_MIN", "ms = 55", TABLE_3);
    // A reworded source, or the same duration in microseconds, is not a
    // change.
    assert_eq!(
        against(&base, &base.replace("Table 3", "Table 3, P3")),
        vec![]
    );
    assert_eq!(
        against(&base, &base.replace("ms = 55", "us = 55000")),
        vec![]
    );
    for head in [
        base.replace("ms = 55", "ms = 50"),
        base.replace("ms = 55", "us = 55"),
        base.replace("P3_MIN", "P3_MAX"),
    ] {
        let message = one(&against(&base, &head), Rule::ChangedWithoutChangelog);
        assert!(
            message.contains("[[timing]] #1 (iso14230_2::P3_MIN) (ms = 55) was changed or removed"),
            "{message}"
        );
        assert_eq!(against(&base, &format!("{head}{CHANGELOG}")), vec![]);
    }
    let message = one(
        &against(&base, &format!("{DECODE_HEAD}{GOOD_CASE}")),
        Rule::ChangedWithoutChangelog,
    );
    assert!(message.contains("switched between"), "{message}");
}

// ---------------------------------------------------------------------------
// [[codec]] fixtures and the `legacy-test` source (US-RR-14-8)
// ---------------------------------------------------------------------------

/// The `[[citation]]` a codec fixture needs (rule `citation`, US-RR-14-1).
const NCS_TEST: &str = "\n[[citation]]\nkind = \"legacy-test\"\n\
     file = \"legacy/src/test/java/com/romraider/maps/checksum/NcsCoDecTest.java\"\n\
     lines = \"33,35,37,39-83\"\n";

const LEGACY: &str = "{ kind = \"legacy-test\", file = \"legacy/src/test/java/com/romraider/maps/checksum/NcsCoDecTest.java\", lines = \"37,45-49\" }";

/// A one-case codec fixture with `source`.
fn codec(source: &str) -> String {
    format!(
        "schema = 1\ntitle = \"codec\"\n{NCS_TEST}\n[[codec]]\nop = \"ncs-crc16\"\n\
         params = {{ complement = false, store = \"big\" }}\ninput = \"31 32\"\n\
         output = \"D1 0D\"\nsource = {source}\n"
    )
}

#[test]
fn codec_cases_follow_the_source_rules() {
    // A legacy test file path is not a Java citation in a [[codec]] case.
    assert_eq!(check(&codec(LEGACY)), vec![]);
    let message = one(
        &check(&codec(&LEGACY.replace("\"37,45-49\"", "\"45-49,37\""))),
        Rule::LegacyTest,
    );
    assert!(message.contains("[[codec]] #1 (ncs-crc16)"), "{message}");
    for file in [
        "legacy/src/main/java/com/romraider/maps/checksum/NcsCoDec.java",
        "legacy/src/test/",
        "legacy/src/test/../main/X.java",
        "legacy/src/test//X.java",
        "legacy\\\\src\\\\test\\\\X.java",
        "/legacy/src/test/X.java",
    ] {
        let source = LEGACY.replace(
            "legacy/src/test/java/com/romraider/maps/checksum/NcsCoDecTest.java",
            file,
        );
        // The source's file is also not the cited one (rule `citation`).
        let violations = check(&codec(&source));
        assert!(
            violations.iter().any(|v| v.rule == Rule::LegacyTest)
                && violations
                    .iter()
                    .all(|v| matches!(v.rule, Rule::LegacyTest | Rule::Citation)),
            "{violations:#?}"
        );
    }
    // Any other source still may not cite Java (and this one's document is
    // not cited either, rule `citation`).
    let violations = check(&codec(
        "{ kind = \"standard\", doc = \"NcsCoDecTest.java\", section = \"48\" }",
    ));
    assert!(
        violations.iter().any(|v| v.rule == Rule::ForbiddenSource),
        "{violations:#?}"
    );
    // A hand calculation must arrive at the output bytes.
    assert_eq!(
        check(&codec(
            "{ kind = \"hand-calc\", working = \"0xD100 + 0x0D = 0xD10D\" }"
        )),
        vec![]
    );
    one(
        &check(&codec(
            "{ kind = \"hand-calc\", working = \"0xD100 + 0x0E = 0xD10E\" }",
        )),
        Rule::HandCalcWorking,
    );
    let missing = codec(LEGACY).replace(&format!("source = {LEGACY}\n"), "");
    let message = one(&check(&missing), Rule::MissingSource);
    assert!(message.contains("[[codec]] #1 (ncs-crc16)"), "{message}");
}

#[test]
fn legacy_test_source_is_only_for_codec_cases() {
    let text = timing("iso14230_2::P3_MIN", "ms = 55", LEGACY);
    let message = one(&check(&text), Rule::LegacyTest);
    assert!(message.contains("only for a [[codec]] case"), "{message}");
}

#[test]
fn changed_codec_output_needs_a_changelog() {
    let base = codec(LEGACY);
    assert_eq!(
        against(&base, &base.replace("\"37,45-49\"", "\"37,45-48\"")),
        vec![]
    );
    for head in [
        base.replace("\"D1 0D\"", "\"D1 0E\""),
        base.replace("\"31 32\"", "\"31 33\""),
        base.replace("complement = false", "complement = true"),
        base.replace("op = \"ncs-crc16\"", "op = \"ncs-crc\""),
    ] {
        let message = one(&against(&base, &head), Rule::ChangedWithoutChangelog);
        assert!(
            message.contains("[[codec]] #1 (ncs-crc16) (output D1 0D) was changed or removed"),
            "{message}"
        );
        assert_eq!(against(&base, &format!("{head}{CHANGELOG}")), vec![]);
    }
    let message = one(
        &against(&base, &format!("{DECODE_HEAD}{GOOD_CASE}")),
        Rule::ChangedWithoutChangelog,
    );
    assert!(message.contains("switched between"), "{message}");
}

// ---------------------------------------------------------------------------
// [[citation]] and standard test-vector fixtures (US-RR-14-1)
// ---------------------------------------------------------------------------

/// Validates `text` as `fixtures/kat/<name>`: the rule `citation` depends on
/// the file name.
fn check_as(name: &str, text: &str) -> Vec<Violation> {
    validate_text(&Path::new("fixtures/kat").join(name), text, None)
}

/// A `standard` citation with the given fields (TOML values, quoted).
fn standard(doc: &str, edition: &str, section: &str, extra: &str) -> String {
    format!(
        "\n[[citation]]\nkind = \"standard\"\ndoc = {doc:?}\nedition = {edition:?}\n\
         section = {section:?}\n{extra}"
    )
}

/// `body` (a fixture without top-level tables before its cases) with
/// `citations` inserted after its `title`.
fn cited(body: &str, citations: &str) -> String {
    let mut parts = body.splitn(3, '\n');
    let (head, title) = (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    );
    let rest = parts.next().unwrap_or_default();
    format!("{head}\n{title}\n{citations}\n{rest}")
}

/// A one-case timing fixture without a citation.
const UNCITED_TIMING: &str = "schema = 1\ntitle = \"t\"\n\n[[timing]]\nname = \"iso14230_2::P3_MIN\"\n\
     ms = 55\nsource = { kind = \"standard\", doc = \"ISO 14230-2:1999\", section = \"Table 3\" }\n";

/// A one-case codec fixture without a citation.
fn uncited_codec() -> String {
    codec(LEGACY).replace(NCS_TEST, "")
}

const UNVERIFIED: &str = "verified = false\nvia = [\"Wikipedia, OBD-II PIDs, revision 1\"]\n";

#[test]
fn vector_fixture_without_a_citation_is_rejected() {
    let decode_text = format!("{DECODE_HEAD}{GOOD_CASE}");
    // By file name, whatever the shape.
    for (name, text) in [
        ("timing-iso14230-2.toml", UNCITED_TIMING.to_owned()),
        ("j1979-01-engine-speed-u16.toml", decode_text.clone()),
        ("checksum-sum-bytes.toml", CHECKSUM.to_owned()),
        ("ncs-codec-legacy.toml", uncited_codec()),
    ] {
        let violations = check_as(name, &text);
        let file_level: Vec<_> = violations.iter().filter(|v| v.location.is_none()).collect();
        assert_eq!(file_level.len(), 1, "{name}: {violations:#?}");
        assert_eq!(file_level[0].rule, Rule::Citation, "{name}");
        assert!(
            file_level[0].message.contains("needs a [[citation]]"),
            "{}",
            file_level[0]
        );
    }
    // By shape, whatever the name: a renamed timing or codec fixture
    // cannot dodge the rule.
    for text in [UNCITED_TIMING.to_owned(), uncited_codec()] {
        let violations = check_as("renamed.toml", &text);
        assert!(
            violations
                .iter()
                .any(|v| v.rule == Rule::Citation && v.location.is_none()),
            "{violations:#?}"
        );
    }
    // A fixture that is not a vector fixture needs none.
    assert_eq!(check_as("u16-be.toml", &decode_text), vec![]);
    assert_eq!(check_as("u16-be.toml", CHECKSUM), vec![]);
    // With one, each passes.
    let iso = standard("ISO 14230-2", "1999", "4.5.1, Table 3", "");
    assert_eq!(
        check_as("timing-iso14230-2.toml", &cited(UNCITED_TIMING, &iso)),
        vec![]
    );
    assert_eq!(
        check_as("renamed.toml", &cited(UNCITED_TIMING, &iso)),
        vec![]
    );
    assert_eq!(check_as("ncs-codec-legacy.toml", &codec(LEGACY)), vec![]);
    let crc = standard(
        "CRC RevEng catalogue",
        "last updated 2024-12-11",
        "CRC-16/ARC",
        "",
    );
    assert_eq!(
        check_as("checksum-crc.toml", &cited(CHECKSUM, &crc)),
        vec![]
    );
}

#[test]
fn standard_citation_needs_document_edition_and_section() {
    for (edition, ok) in [
        ("1999 (first edition, 1999-03-15)", true),
        ("2004-10-15", true),
        ("version 3, 1993-08-19", true),
        ("version 3", true),
        ("rev. B", true),
        ("Revision C2", true),
        ("first edition", false),
        ("ELM327DSJ", false),
        ("latest", false),
        ("199", false),
        ("3099", false),
        ("", false),
        ("  ", false),
    ] {
        let text = cited(
            UNCITED_TIMING,
            &standard("ISO 14230-2", edition, "Table 3", ""),
        );
        let violations = check_as("timing-iso14230-2.toml", &text);
        if ok {
            assert_eq!(violations, vec![], "{edition:?}");
        } else {
            let message = one(&violations, Rule::Citation);
            assert!(message.contains("[[citation]] #1 (standard)"), "{message}");
        }
    }
    for (doc, section) in [("", "Table 3"), ("ISO 14230-2", " ")] {
        let text = cited(UNCITED_TIMING, &standard(doc, "1999", section, ""));
        let violations = check_as("timing-iso14230-2.toml", &text);
        assert!(
            violations.iter().all(|v| v.rule == Rule::Citation) && !violations.is_empty(),
            "{violations:#?}"
        );
        assert!(
            violations.iter().any(|v| v
                .message
                .contains("non-empty `doc`, `edition` and `section`")),
            "{violations:#?}"
        );
    }
    // A missing field, or an unknown one, does not load.
    let text = cited(
        UNCITED_TIMING,
        "\n[[citation]]\nkind = \"standard\"\ndoc = \"ISO 14230-2\"\nsection = \"Table 3\"\n",
    );
    one(&check_as("timing-iso14230-2.toml", &text), Rule::Load);
    let text = cited(
        UNCITED_TIMING,
        &standard("ISO 14230-2", "1999", "Table 3", "year = 1999\n"),
    );
    one(&check_as("timing-iso14230-2.toml", &text), Rule::Load);
    // A standard citation is a published document, never Java.
    let text = cited(
        UNCITED_TIMING,
        &standard(
            "ISO 14230-2",
            "1999",
            "Table 3, as NCSLoggerConnection.java uses it",
            "",
        ),
    );
    one(
        &check_as("timing-iso14230-2.toml", &text),
        Rule::ForbiddenSource,
    );
}

#[test]
fn each_standard_source_names_a_cited_document() {
    // The case cites ISO 14230-2:1999; the fixture cites ISO 9141-2 only.
    let text = cited(
        UNCITED_TIMING,
        &standard("ISO 9141-2", "1994", "7.4, Figure 6", ""),
    );
    let message = one(&check_as("timing-iso14230-2.toml", &text), Rule::Citation);
    assert!(
        message.contains("[[timing]] #1 (iso14230_2::P3_MIN)")
            && message.contains("\"ISO 14230-2:1999\" is not cited"),
        "{message}"
    );
    // Citing both is fine.
    let both = format!(
        "{}{}",
        standard("ISO 9141-2", "1994", "7.4, Figure 6", ""),
        standard("ISO 14230-2", "1999", "Table 3", "")
    );
    assert_eq!(
        check_as("timing-iso14230-2.toml", &cited(UNCITED_TIMING, &both)),
        vec![]
    );
    // Outside a vector fixture a standard source needs no citation.
    let ieee = format!(
        "{DECODE_HEAD}\n[[decode]]\noffset = 0\nraw = 500\nreal = 50.0\n\
         source = {{ kind = \"standard\", doc = \"IEEE 754-2008\", section = \"3.4\" }}\n"
    );
    assert_eq!(check_as("u16-be.toml", &ieee), vec![]);
}

#[test]
fn unverified_citation_is_a_narrow_flagged_exception() {
    let decode_text = format!("{DECODE_HEAD}{GOOD_CASE}");
    let j1979 = |extra: &str| {
        cited(
            &decode_text,
            &standard(
                "SAE J1979",
                "2017-02 (as Wikipedia lists it; not consulted)",
                "service $01, PID 0x0C",
                extra,
            ),
        )
    };
    // Allowed in a j1979-01-* fixture, with `via`.
    assert_eq!(
        check_as("j1979-01-engine-speed-u16.toml", &j1979(UNVERIFIED)),
        vec![]
    );
    // Without `via`, or with an empty one, it is not.
    for extra in [
        "verified = false\n",
        "verified = false\nvia = []\n",
        "verified = false\nvia = [\" \"]\n",
    ] {
        let message = one(
            &check_as("j1979-01-engine-speed-u16.toml", &j1979(extra)),
            Rule::Citation,
        );
        assert!(message.contains("needs `via`"), "{message}");
    }
    // Anywhere else it is not allowed, vector fixture or not.
    for name in [
        "j1979-02-freeze-frame.toml",
        "timing-iso14230-2.toml",
        "u16-be.toml",
        "checksum-crc.toml",
    ] {
        let message = one(&check_as(name, &j1979(UNVERIFIED)), Rule::Citation);
        assert!(message.contains("flagged exception"), "{name}: {message}");
    }
    // `verified = true` (the default) needs no `via`.
    assert_eq!(
        check_as(
            "j1979-01-engine-speed-u16.toml",
            &j1979("verified = true\n")
        ),
        vec![]
    );
}

#[test]
fn legacy_citations_are_pinned_to_their_fixtures() {
    // legacy-test: only in a [[codec]] fixture, under legacy/src/test/, and
    // each legacy-test source's file must be cited.
    let message = one(
        &check_as(
            "timing-iso14230-2.toml",
            &cited(UNCITED_TIMING, &format!("{ISO_14230_2}{NCS_TEST}")),
        ),
        Rule::Citation,
    );
    assert!(
        message.contains("only for a [[codec]] fixture"),
        "{message}"
    );
    let other = NCS_TEST.replace("NcsCoDecTest.java", "OtherTest.java");
    let message = one(
        &check_as("ncs-codec-legacy.toml", &cited(&uncited_codec(), &other)),
        Rule::Citation,
    );
    assert!(
        message.contains("[[codec]] #1 (ncs-crc16)") && message.contains("is not cited"),
        "{message}"
    );
    let main = NCS_TEST.replace("legacy/src/test/", "legacy/src/main/");
    let violations = check_as("ncs-codec-legacy.toml", &cited(&uncited_codec(), &main));
    assert!(
        violations
            .iter()
            .any(|v| v.message.contains("must be a relative")),
        "{violations:#?}"
    );
    let bad_lines = NCS_TEST.replace("33,35,37,39-83", "83,33");
    one(
        &check_as(
            "ncs-codec-legacy.toml",
            &cited(&uncited_codec(), &bad_lines),
        ),
        Rule::Citation,
    );

    // legacy-algorithm: only in the pinned checksum fixtures, under
    // legacy/src/main/, with a note.
    let algorithm = |file: &str, lines: &str, note: &str| {
        format!(
            "\n[[citation]]\nkind = \"legacy-algorithm\"\nfile = {file:?}\nlines = {lines:?}\nnote = {note:?}\n"
        )
    };
    let good = algorithm(
        "legacy/src/main/java/com/romraider/io/protocol/ssm/iso9141/SSMChecksumCalculator.java",
        "31-37",
        "no published standard defines a byte sum",
    );
    for name in validate::LEGACY_ALGORITHM_FIXTURES {
        assert_eq!(check_as(name, &cited(CHECKSUM, &good)), vec![], "{name}");
    }
    for name in [
        "checksum-crc.toml",
        "checksum-sum-other.toml",
        "u16-be.toml",
    ] {
        let message = one(&check_as(name, &cited(CHECKSUM, &good)), Rule::Citation);
        assert!(message.contains("flagged exception"), "{name}: {message}");
    }
    for bad in [
        algorithm("legacy/src/test/java/X.java", "1", "no standard"),
        algorithm("legacy/src/main/../test/X.java", "1", "no standard"),
        algorithm("legacy/src/main/X.java", "2-1", "no standard"),
        algorithm("legacy/src/main/X.java", "1", " "),
    ] {
        one(
            &check_as("checksum-sum-bytes.toml", &cited(CHECKSUM, &bad)),
            Rule::Citation,
        );
    }
}

#[test]
fn citations_change_freely_in_base_mode() {
    let base = cited(
        UNCITED_TIMING,
        &standard("ISO 14230-2", "1999", "Table 3", ""),
    );
    let head = cited(
        UNCITED_TIMING,
        &standard("ISO 14230-2", "1999 (first edition)", "4.5.1, Table 3", ""),
    );
    assert_eq!(
        validate_text(
            Path::new("fixtures/kat/timing-iso14230-2.toml"),
            &head,
            Some(("base", &base))
        ),
        vec![]
    );
}
