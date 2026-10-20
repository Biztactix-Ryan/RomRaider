//! `fixtures/corpus-manifest.toml` and `cargo xtask gen-corpus-manifest`
//! (US-RR-11-6, D-025; the `[defs]` table: US-RR-16-5, D-028). File-name
//! parsing, rendering and the mirror checks are unit-tested in
//! `xtask/src/corpus_manifest.rs`.
//!
//! The committed manifest is checked here without the mirrors: it parses,
//! it is the canonical rendering of itself (so its hash, the CI cache key,
//! is stable), it names the pinned upstream commits (ROMs and definitions)
//! and the logger package pin, and git keeps its `\n` endings. That it
//! matches a fresh generation from the mirrors needs them, so that test is
//! `#[ignore]`d (reported as skipped, never passed) and runs with:
//!
//! ```text
//! RR_CORPUS_MIRROR=/home/ryan/Repo/romraider-stock-roms \
//! RR_DEFS_MIRROR=/home/ryan/Repo/romraider-ecu-defs \
//!   cargo nextest run -p xtask --run-ignored only -E 'test(regenerating_from_the_mirror)'
//! ```
//!
//! That the local definitions mirror exists and is consistent (US-RR-16-2:
//! a bare mirror of upstream holding the pinned commit on its branch, its
//! `SHA256SUMS` agreeing with the committed `[defs]` entry for entry) needs
//! only that mirror, so it is a test of its own, also `#[ignore]`d:
//!
//! ```text
//! RR_DEFS_MIRROR=/home/ryan/Repo/romraider-ecu-defs \
//!   cargo nextest run -p xtask --run-ignored only -E 'test(the_definitions_mirror)'
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use xtask::corpus_manifest::{
    COMMITTED_PATH, DEFS_BRANCH, DEFS_DIR, DEFS_MIRROR_ENV, DEFS_MIRROR_GIT, DEFS_PATH_PREFIX,
    DEFS_PIN_COMMIT, DEFS_UPSTREAM_URL, LOGGER_V370_FILE, LOGGER_V370_SHA256, LOGGER_V370_SIZE,
    MIRROR_ENV, MIRROR_SUMS_FILE, Manifest, PATH_PREFIX, PIN_COMMIT, REGIONS, STANDARD_SIZES,
    SuspectKind, UPSTREAM_URL, generate_defs, parse_sums, standard_size,
};

/// Files in the mirror's SHA256SUMS (D-025).
const CORPUS_FILES: usize = 501;

/// Files of the pinned `RomRaider/` definitions subtree (D-028: 267 files,
/// about 229 MiB).
const DEFS_FILES: usize = 267;
const DEFS_BYTES: u64 = 239_467_067;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn committed_text() -> anyhow::Result<String> {
    Ok(fs::read_to_string(workspace_root().join(COMMITTED_PATH))?)
}

fn xtask(
    args: &[&str],
    mirror_env: Option<&Path>,
    defs_mirror_env: Option<&Path>,
) -> anyhow::Result<(i32, String)> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xtask"));
    cmd.arg("gen-corpus-manifest").args(args);
    match mirror_env {
        Some(dir) => cmd.env(MIRROR_ENV, dir),
        None => cmd.env_remove(MIRROR_ENV),
    };
    match defs_mirror_env {
        Some(dir) => cmd.env(DEFS_MIRROR_ENV, dir),
        None => cmd.env_remove(DEFS_MIRROR_ENV),
    };
    let out = cmd.output()?;
    Ok((
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    ))
}

#[test]
fn committed_manifest_is_valid_canonical_and_pinned() -> anyhow::Result<()> {
    let text = committed_text()?;
    let m = Manifest::from_toml(&text)?;
    assert_eq!(
        m.render()?,
        text,
        "not the canonical rendering: regenerate it"
    );
    assert_eq!(m.roms.upstream, UPSTREAM_URL);
    assert_eq!(m.roms.commit, PIN_COMMIT);
    assert!(PIN_COMMIT.starts_with("639f3c73"), "D-025 pins 639f3c73");
    assert_eq!(m.roms.path_prefix, PATH_PREFIX);
    assert_eq!(m.roms.files.len(), CORPUS_FILES);
    for f in &m.roms.files {
        assert!(
            REGIONS.iter().any(|r| f.path.starts_with(&format!("{r}/"))),
            "{}: not under a region directory",
            f.path
        );
        assert!(f.size > 0, "{}", f.path);
    }
    // The two non-standard sizes D-025 names are present, and marked
    // suspect (US-RR-11-7; US-RR-11-2 tests the classification itself).
    for size in [1_048_731, 1_048_784] {
        let odd: Vec<_> = m.roms.files.iter().filter(|f| f.size == size).collect();
        assert_eq!(odd.len(), 1, "one {size}-byte image");
        assert!(
            odd.iter().all(|f| f.is_suspect()),
            "{size}-byte image not suspect"
        );
    }
    // Most names parse in full; the ones that do not are listed without guesses.
    let parsed = m.roms.files.iter().filter(|f| f.model.is_some()).count();
    assert!(parsed >= CORPUS_FILES - 5, "{parsed} names parsed");
    Ok(())
}

/// US-RR-11-2 on the committed manifest: no image of a size outside
/// [`STANDARD_SIZES`] is left unmarked; every suspect gives a reason naming
/// its size; every clean image is a standard size; and the two suspects are
/// the D-025 containers, each with a standard-size embedded image ending the
/// file. (That each classification is what the bytes are is
/// `regenerating_from_the_mirror_matches_the_committed_manifest`, which
/// re-runs `classify` on every image, and the corpus-tier
/// `suspect_entries_match_the_fetched_bytes` in rr-testkit.)
#[test]
fn committed_manifest_marks_every_nonstandard_size_suspect() -> anyhow::Result<()> {
    let m = Manifest::from_toml(&committed_text()?)?;
    let unmarked: Vec<_> = m.roms.unmarked_nonstandard();
    assert!(
        unmarked.is_empty(),
        "non-standard sizes not marked suspect: {unmarked:?}"
    );
    for f in m.roms.clean_files() {
        assert!(standard_size(f.size).is_some(), "{}: {}", f.path, f.size);
    }
    let suspects: Vec<_> = m.roms.suspect_files().collect();
    assert_eq!(suspects.len(), 2, "{suspects:?}");
    for (f, s) in suspects {
        assert!(standard_size(f.size).is_none(), "{}", f.path);
        assert!(!s.reason.trim().is_empty(), "{}", f.path);
        assert!(
            s.reason.starts_with(&format!("size {} = ", f.size)),
            "{}: {}",
            f.path,
            s.reason
        );
        assert_eq!(s.kind, SuspectKind::Container, "{}", f.path);
        let (offset, size) = (s.image_offset.unwrap(), s.image_size.unwrap());
        assert!(
            STANDARD_SIZES.iter().any(|(std, _)| *std == size),
            "{}",
            f.path
        );
        assert_eq!(offset + size, f.size, "{}", f.path);
        assert!(
            s.reason.contains("MEMD(1048576)"),
            "{}: {}",
            f.path,
            s.reason
        );
    }
    Ok(())
}

#[test]
fn git_keeps_the_manifest_bytes_unconverted() -> anyhow::Result<()> {
    let out = Command::new("git")
        .args(["check-attr", "text", "eol", "--", COMMITTED_PATH])
        .current_dir(workspace_root())
        .output()?;
    assert!(out.status.success(), "git check-attr failed");
    let attrs = String::from_utf8(out.stdout)?;
    assert!(
        attrs.contains(&format!("{COMMITTED_PATH}: text: set")),
        "{attrs}"
    );
    assert!(
        attrs.contains(&format!("{COMMITTED_PATH}: eol: lf")),
        "{attrs}"
    );
    Ok(())
}

#[test]
fn no_mirror_is_a_clear_error() -> anyhow::Result<()> {
    let (code, out) = xtask(&["--check"], None, None)?;
    assert_ne!(code, 0, "{out}");
    assert!(
        out.contains("--mirror") && out.contains(MIRROR_ENV),
        "{out}"
    );

    let empty = Path::new(env!("CARGO_TARGET_TMPDIR")).join("corpus-manifest-empty-mirror");
    let _ = fs::remove_dir_all(&empty);
    fs::create_dir_all(&empty)?;
    let (code, out) = xtask(
        &["--check", "--mirror", &empty.to_string_lossy()],
        None,
        None,
    )?;
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("no `roms/` checkout"), "{out}");

    // A ROM mirror but no definitions mirror (US-RR-16-5): named, and
    // reported before anything is hashed.
    fs::create_dir_all(empty.join("roms"))?;
    let (code, out) = xtask(
        &["--check", "--mirror", &empty.to_string_lossy()],
        None,
        None,
    )?;
    assert_ne!(code, 0, "{out}");
    assert!(
        out.contains("no definitions mirror")
            && out.contains("--defs-mirror")
            && out.contains(DEFS_MIRROR_ENV),
        "{out}"
    );
    let empty_defs = empty.join("defs-mirror");
    fs::create_dir_all(&empty_defs)?;
    let (code, out) = xtask(
        &["--check", "--mirror", &empty.to_string_lossy()],
        None,
        Some(&empty_defs),
    )?;
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("no `mirror.git/`"), "{out}");
    Ok(())
}

/// US-RR-16-5 (D-028): the committed manifest pins the ECU definitions (the
/// `RomRaider/` subtree of TD-D/SubaruDefs at `0b0c0c42`, every file with
/// its size and SHA-256, the four names with spaces among them) and the
/// logger package by size and SHA-256, with the licence position recorded.
#[test]
fn committed_manifest_pins_the_definitions_and_the_logger_package() -> anyhow::Result<()> {
    let m = Manifest::from_toml(&committed_text()?)?;
    let defs = m.defs.as_ref().expect("committed manifest has no [defs]");
    assert_eq!(defs.upstream, DEFS_UPSTREAM_URL);
    assert_eq!(DEFS_UPSTREAM_URL, "https://github.com/TD-D/SubaruDefs");
    assert_eq!(defs.branch, DEFS_BRANCH);
    assert_eq!(defs.commit, DEFS_PIN_COMMIT);
    assert_eq!(DEFS_PIN_COMMIT, "0b0c0c42acea0d89ffd8b50378ae13ae32b5b43d");
    assert_eq!(defs.path_prefix, DEFS_PATH_PREFIX);
    assert_eq!(DEFS_PATH_PREFIX, "RomRaider/");
    assert!(defs.licence.contains("D-028"), "{}", defs.licence);
    assert!(defs.licence.contains("never vendored"), "{}", defs.licence);

    assert_eq!(defs.files.len(), DEFS_FILES);
    let total: u64 = defs.files.iter().map(|f| f.size).sum();
    assert_eq!(total, DEFS_BYTES);
    for f in &defs.files {
        assert!(f.size > 0, "{}", f.path);
        assert!(
            !f.path.starts_with("ECUFlash/") && !f.path.starts_with("RomRaider/"),
            "{}: paths are relative to the prefix",
            f.path
        );
    }
    let spaced: Vec<&str> = defs
        .files
        .iter()
        .map(|f| f.path.as_str())
        .filter(|p| p.contains(' '))
        .collect();
    assert_eq!(
        spaced,
        [
            "ecu/2004USDMBajaXTAT-A2WC50  .xml",
            "ecu/2007 Outback 3R AT D2UH001T.xml",
            "ecu/RR D2UJ001T.xml",
            "ecu/RR_EE5I910H .xml",
        ]
    );
    // The definitions D-028 names, and the public tier's v290 logger files.
    for path in [
        "ecu/standard/ecu_defs.xml",
        "ecu/metric/ecu_defs.xml",
        "ecu/standard/ecu_defs_merpmod.xml",
        "logger/logger_STD_EN_v290.xml",
        "logger/logger_METRIC_EN_v290.xml",
        "logger/logger_IMP_EN_v290.xml",
    ] {
        assert!(
            defs.files.iter().any(|f| f.path == path),
            "{path} not in [defs]"
        );
    }

    let logger = &defs.logger_v370;
    assert_eq!(logger.file, LOGGER_V370_FILE);
    assert_eq!(logger.size, LOGGER_V370_SIZE);
    assert_eq!(LOGGER_V370_SIZE, 986_211);
    assert_eq!(logger.sha256, LOGGER_V370_SHA256);
    assert_eq!(
        LOGGER_V370_SHA256,
        "280d386542f6fd49e3467363a62c12c6c642c127f3651a37265bdbac127ea0fa"
    );
    // No ROM entry can collide with the cache's definitions directory.
    assert!(
        m.roms
            .files
            .iter()
            .all(|f| !f.path.starts_with(&format!("{DEFS_DIR}/")))
    );
    Ok(())
}

#[test]
#[ignore = "needs the local corpus mirrors: set RR_CORPUS_MIRROR and RR_DEFS_MIRROR and run with --run-ignored only"]
fn regenerating_from_the_mirror_matches_the_committed_manifest() -> anyhow::Result<()> {
    let Some(mirror) = std::env::var_os(MIRROR_ENV).filter(|v| !v.is_empty()) else {
        panic!(
            "set {MIRROR_ENV} to the local corpus mirror (a directory holding `roms/` and `SHA256SUMS`, \
             e.g. /home/ryan/Repo/romraider-stock-roms) to run this test"
        );
    };
    let Some(defs_mirror) = std::env::var_os(DEFS_MIRROR_ENV).filter(|v| !v.is_empty()) else {
        panic!(
            "set {DEFS_MIRROR_ENV} to the local definitions mirror (a directory holding `mirror.git/` and `SHA256SUMS`, \
             e.g. /home/ryan/Repo/romraider-ecu-defs) to run this test"
        );
    };
    let (code, out) = xtask(
        &["--check"],
        Some(Path::new(&mirror)),
        Some(Path::new(&defs_mirror)),
    )?;
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("byte-identical to a fresh generation"),
        "{out}"
    );
    assert!(out.contains(&format!("{CORPUS_FILES} image(s)")), "{out}");
    assert!(
        out.contains(&format!(
            "{DEFS_FILES} definition file(s), {DEFS_BYTES} bytes"
        )),
        "{out}"
    );
    Ok(())
}

/// US-RR-16-2, "a local mirror exists": the definitions mirror D-028 names
/// is a bare mirror of upstream, outside the repository, whose branch holds
/// the pinned commit; its `SHA256SUMS` lists exactly the committed `[defs]`
/// files with their hashes; and the blobs it holds at the pin re-hash to
/// the committed `[defs]` table ([`generate_defs`] reads every one).
#[test]
#[ignore = "needs the local definitions mirror: set RR_DEFS_MIRROR and run with --run-ignored only"]
fn the_definitions_mirror_holds_the_pin_and_agrees_with_the_manifest() -> anyhow::Result<()> {
    let Some(mirror) = std::env::var_os(DEFS_MIRROR_ENV).filter(|v| !v.is_empty()) else {
        panic!(
            "set {DEFS_MIRROR_ENV} to the local definitions mirror (a directory holding `{DEFS_MIRROR_GIT}/` and `{MIRROR_SUMS_FILE}`, \
             e.g. /home/ryan/Repo/romraider-ecu-defs) to run this test"
        );
    };
    let mirror = PathBuf::from(mirror);
    let git_dir = mirror.join(DEFS_MIRROR_GIT);
    let git = |args: &[&str]| -> anyhow::Result<(bool, String)> {
        let out = Command::new("git")
            .arg("--git-dir")
            .arg(&git_dir)
            .args(args)
            .output()?;
        Ok((
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).trim().to_owned(),
        ))
    };

    // Outside the repository: nothing from upstream can be committed.
    let root = fs::canonicalize(workspace_root())?;
    let at = fs::canonicalize(&mirror)?;
    assert!(
        !at.starts_with(&root),
        "{}: the mirror is inside the repository {}",
        at.display(),
        root.display()
    );

    // A bare mirror of upstream, holding the pinned commit on its branch.
    assert_eq!(
        git(&["rev-parse", "--is-bare-repository"])?,
        (true, "true".to_owned()),
        "{}: not a bare repository",
        git_dir.display()
    );
    let (ok, url) = git(&["config", "--get", "remote.origin.url"])?;
    assert!(ok, "{}: no remote.origin.url", git_dir.display());
    assert_eq!(url.trim_end_matches(".git"), DEFS_UPSTREAM_URL);
    assert_eq!(
        git(&["cat-file", "-t", DEFS_PIN_COMMIT])?,
        (true, "commit".to_owned()),
        "{}: lacks the pinned commit {DEFS_PIN_COMMIT}",
        git_dir.display()
    );
    let branch = format!("refs/heads/{DEFS_BRANCH}");
    assert!(
        git(&["merge-base", "--is-ancestor", DEFS_PIN_COMMIT, &branch])?.0,
        "{}: {DEFS_PIN_COMMIT} is not on {branch}",
        git_dir.display()
    );

    // SHA256SUMS agrees with the committed [defs], entry for entry.
    let m = Manifest::from_toml(&committed_text()?)?;
    let defs = m.defs.as_ref().expect("committed manifest has no [defs]");
    let sums = parse_sums(&fs::read_to_string(mirror.join(MIRROR_SUMS_FILE))?)?;
    let want: BTreeMap<&str, &str> = defs
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.sha256.as_str()))
        .collect();
    let mut problems = Vec::new();
    for (path, sha) in &sums {
        match want.get(path.as_str()) {
            None => problems.push(format!("{path}: in {MIRROR_SUMS_FILE}, not in [defs]")),
            Some(w) if *w != sha => {
                problems.push(format!("{path}: {MIRROR_SUMS_FILE} {sha}, [defs] {w}"));
            }
            Some(_) => {}
        }
    }
    for path in want.keys().filter(|p| !sums.contains_key(**p)) {
        problems.push(format!("{path}: in [defs], not in {MIRROR_SUMS_FILE}"));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    assert_eq!(sums.len(), DEFS_FILES);

    // The mirror's blobs at the pin are the committed bytes.
    let generated = generate_defs(&mirror)?;
    assert_eq!(&generated, defs);
    Ok(())
}
