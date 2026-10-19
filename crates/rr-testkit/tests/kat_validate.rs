//! Tests for the KAT fixture validator (`rr_testkit::kat::validate`,
//! `validate::cli` command line, US-RR-9-6).
//!
//! Proves the US-RR-9 acceptance criteria the validator owns: an expected
//! value without a source is rejected naming the file and the case; a
//! `hand-calc` source must show its working (formula, intermediate values,
//! result); a changed expected value needs a new changelog entry in base mode
//! (injected base text and a real temporary git repository); the clean sample
//! fixtures pass; and the command line exits non-zero on any violation.

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
    let reworded = head.replace("500 was misread", "500 was misread earlier");
    // Editing the old entry's text makes it a new entry; that counts.
    assert_eq!(against(&base, &reworded), vec![]);
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
    ] {
        let (code, _, stderr) = bin(&dir, args);
        assert_eq!(code, 2, "{args:?}: {stderr}");
        assert!(stderr.contains("validate-fixtures: error:"), "{stderr}");
    }
}
