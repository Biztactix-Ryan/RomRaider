//! `cargo xtask gen-synthetic [--check] [--dir <DIR>]` (US-RR-10-7, PROJECT.md
//! T2(b)).
//!
//! The synthetic ROMs and their manifests are built by rr-testkit's generator
//! (`rr_testkit::reference::synthetic`, from known values with the reference
//! codec). rr-testkit is dev-dependency only (`cargo xtask check-deps`, rule
//! 3), so xtask cannot link it; instead this subcommand runs rr-testkit's
//! `gen-synthetic` example with `cargo run` into a fresh scratch directory,
//! hashes what it wrote, and then:
//!
//! * without `--check`, makes `<DIR>` (default `fixtures/public/synthetic`)
//!   hold exactly the generated files plus `SHA256SUMS`, removing stale
//!   generated files;
//! * with `--check`, writes nothing and fails unless `<DIR>` holds exactly
//!   those files, byte for byte: a missing, extra or different file (or a
//!   `SHA256SUMS` line that does not match) is a problem.
//!
//! The comparison is on raw bytes, with no line-ending normalisation, so a
//! checkout that turned the manifest's `\n` into `\r\n` fails (the
//! directory's `.gitattributes` keeps git from doing that). `SHA256SUMS` is
//! in `sha256sum` format (`<hex>  <name>`, sorted by name, `\n` endings), so
//! the committed bytes can also be checked with `sha256sum -c SHA256SUMS`
//! without this code, as CI does on Linux and Windows.
//!
//! Exit status: 0 when up to date (or written), 1 on any difference, and
//! non-zero with an error when the generator cannot run.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail};
use sha2::{Digest as _, Sha256};

/// The package holding the generator.
pub const PACKAGE: &str = "rr-testkit";

/// The generator's example target.
pub const EXAMPLE: &str = "gen-synthetic";

/// Where the generated files are committed, relative to the workspace root.
pub const COMMITTED_DIR: &str = "fixtures/public/synthetic";

/// The hashes file written next to the generated files.
pub const SUMS_FILE: &str = "SHA256SUMS";

/// Files in the committed directory that are not generated and are left
/// alone (and not compared).
pub const NOT_GENERATED: &[&str] = &[".gitattributes"];

/// File name to contents, sorted by name.
pub type Files = BTreeMap<String, Vec<u8>>;

/// The `cargo run` invocation of the generator for the workspace at
/// `workspace_root`, writing into `out_dir`.
#[must_use]
pub fn command(workspace_root: &Path, out_dir: &Path) -> Command {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let mut cmd = Command::new(cargo);
    cmd.arg("run")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(workspace_root.join("Cargo.toml"))
        .args(["--package", PACKAGE, "--example", EXAMPLE, "--"])
        .arg(out_dir);
    cmd
}

/// Lowercase hex SHA-256 of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// The `SHA256SUMS` text for `files`: `<hex>  <name>\n` per file, by name.
#[must_use]
pub fn sums(files: &Files) -> String {
    files
        .iter()
        .map(|(name, bytes)| format!("{}  {name}\n", sha256_hex(bytes)))
        .collect()
}

/// `generated` plus its `SHA256SUMS`: what the committed directory must hold.
#[must_use]
pub fn expected(generated: &Files) -> Files {
    let mut out = generated.clone();
    out.insert(SUMS_FILE.to_owned(), sums(generated).into_bytes());
    out
}

/// The regular files directly in `dir`, by name, except [`NOT_GENERATED`].
/// A missing `dir` reads as empty.
///
/// # Errors
///
/// An unreadable directory or file, a subdirectory, or a non-UTF-8 name.
pub fn read_files(dir: &Path) -> Result<Files> {
    let mut out = Files::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            bail!("{}: file name is not UTF-8", path.display());
        };
        if NOT_GENERATED.contains(&name.as_str()) {
            continue;
        }
        if !entry
            .file_type()
            .with_context(|| format!("reading {}", path.display()))?
            .is_file()
        {
            bail!(
                "{}: not a regular file (only generated files belong in {COMMITTED_DIR})",
                path.display()
            );
        }
        let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        out.insert(name, bytes);
    }
    Ok(out)
}

/// Why `actual` differs from `expected`, one entry per file; empty when they
/// are identical.
#[must_use]
pub fn compare(expected: &Files, actual: &Files) -> Vec<String> {
    let mut problems = Vec::new();
    for (name, want) in expected {
        match actual.get(name) {
            None => problems.push(format!("{name}: missing (run `cargo xtask gen-synthetic`)")),
            Some(got) if got != want => problems.push(difference(name, want, got)),
            Some(_) => {}
        }
    }
    for name in actual.keys().filter(|n| !expected.contains_key(*n)) {
        problems.push(format!(
            "{name}: not generated by any published plan (stale or hand-added; `cargo xtask gen-synthetic` removes it)"
        ));
    }
    problems
}

fn difference(name: &str, want: &[u8], got: &[u8]) -> String {
    let at = want
        .iter()
        .zip(got)
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| want.len().min(got.len()));
    let mut out = format!(
        "{name}: differs from the generated bytes at offset {at:#x} (committed {} bytes, sha256 {}; generated {} bytes, sha256 {})",
        got.len(),
        sha256_hex(got),
        want.len(),
        sha256_hex(want),
    );
    if got.contains(&b'\r') && !want.contains(&b'\r') && !name.ends_with(".bin") {
        out.push_str(
            "; the committed file has \\r\\n line endings: check the directory's .gitattributes and core.autocrlf",
        );
    }
    out
}

/// Makes `dir` hold exactly `expected` (plus any [`NOT_GENERATED`] file):
/// writes new or changed files and removes files no plan generates. Returns
/// the names written or removed.
///
/// # Errors
///
/// Any read or write error.
pub fn install(dir: &Path, expected: &Files) -> Result<Vec<String>> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let actual = read_files(dir)?;
    let mut changed = Vec::new();
    for name in actual.keys().filter(|n| !expected.contains_key(*n)) {
        let path = dir.join(name);
        fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        changed.push(format!("removed {name}"));
    }
    for (name, bytes) in expected {
        if actual.get(name) != Some(bytes) {
            let path = dir.join(name);
            fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
            changed.push(format!("wrote {name}"));
        }
    }
    Ok(changed)
}

/// Runs the generator for the workspace at `root` into a fresh scratch
/// directory and returns what it wrote (without `SHA256SUMS`). The scratch
/// directory is removed afterwards.
///
/// # Errors
///
/// The generator cannot be started, exits non-zero, or writes nothing.
pub fn generate(root: &Path) -> Result<Files> {
    let scratch = scratch_dir()?;
    let result = run_generator(root, &scratch);
    let _ = fs::remove_dir_all(&scratch);
    result
}

fn run_generator(root: &Path, scratch: &Path) -> Result<Files> {
    // The example lists the files it wrote on stdout; the caller prints its
    // own summary, so only the generator's errors (stderr) are passed on.
    let status = command(root, scratch)
        .stdout(std::process::Stdio::null())
        .status()
        .context("gen-synthetic: cannot run cargo")?;
    if !status.success() {
        bail!("gen-synthetic: the {PACKAGE} `{EXAMPLE}` example failed ({status})");
    }
    let files = read_files(scratch)?;
    if files.is_empty() {
        bail!("gen-synthetic: the generator wrote no files");
    }
    if files.contains_key(SUMS_FILE) {
        bail!("gen-synthetic: the generator wrote {SUMS_FILE}, which xtask owns");
    }
    Ok(files)
}

/// A new, empty directory under the system temp directory, unique to this
/// process.
fn scratch_dir() -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("rr-gen-synthetic-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(entries: &[(&str, &[u8])]) -> Files {
        entries
            .iter()
            .map(|(n, b)| ((*n).to_owned(), b.to_vec()))
            .collect()
    }

    #[test]
    fn sha256_known_answers() {
        // FIPS 180-2 appendix B.1 and the empty message.
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sums_are_sorted_sha256sum_lines() {
        let f = files(&[("b.bin", b"abc"), ("a.toml", b"")]);
        assert_eq!(
            sums(&f),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  a.toml\n\
             ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  b.bin\n"
        );
        let e = expected(&f);
        assert_eq!(e.len(), 3);
        assert_eq!(e[SUMS_FILE], sums(&f).into_bytes());
    }

    #[test]
    fn compare_flags_every_difference() {
        let want = expected(&files(&[
            ("x.bin", b"\x00\x01\x02"),
            ("x.manifest.toml", b"a\nb\n"),
        ]));
        assert!(compare(&want, &want).is_empty());

        let mut flipped = want.clone();
        flipped.insert("x.bin".into(), b"\x00\x01\x03".to_vec());
        let p = compare(&want, &flipped);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(
            p[0].starts_with("x.bin: differs") && p[0].contains("offset 0x2"),
            "{p:?}"
        );

        let mut short = want.clone();
        short.insert("x.bin".into(), b"\x00\x01".to_vec());
        assert!(compare(&want, &short)[0].contains("offset 0x2"));

        let mut crlf = want.clone();
        crlf.insert("x.manifest.toml".into(), b"a\r\nb\r\n".to_vec());
        let p = compare(&want, &crlf);
        assert!(p[0].contains("\\r\\n line endings"), "{p:?}");

        let mut missing = want.clone();
        missing.remove("x.bin");
        assert!(compare(&want, &missing)[0].starts_with("x.bin: missing"));

        let mut extra = want.clone();
        extra.insert("old.bin".into(), Vec::new());
        assert!(compare(&want, &extra)[0].starts_with("old.bin: not generated"));

        let mut sums_edited = want.clone();
        sums_edited.insert(SUMS_FILE.into(), b"0  x.bin\n".to_vec());
        assert!(compare(&want, &sums_edited)[0].starts_with("SHA256SUMS: differs"));
    }

    #[test]
    fn install_writes_and_prunes_then_compares_clean() -> Result<()> {
        let dir =
            std::env::temp_dir().join(format!("rr-gen-synthetic-unit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("stale.bin"), b"old")?;
        fs::write(dir.join(".gitattributes"), b"*.bin binary\n")?;
        let want = expected(&files(&[("x.bin", b"\xff")]));
        let changed = install(&dir, &want)?;
        assert_eq!(
            changed,
            ["removed stale.bin", "wrote SHA256SUMS", "wrote x.bin"]
        );
        assert!(compare(&want, &read_files(&dir)?).is_empty());
        assert!(dir.join(".gitattributes").is_file());
        assert!(install(&dir, &want)?.is_empty());
        fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
