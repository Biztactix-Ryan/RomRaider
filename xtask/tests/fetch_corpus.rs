//! `cargo xtask fetch-corpus` (US-RR-11-5, D-025), end to end and offline.
//!
//! Every test builds its own small source repository of synthetic
//! pseudo-random bytes (never real ROM images) in the system temp directory,
//! outside the repository, with a matching manifest, and points the command
//! at it with `--source`. Nothing here touches the network; the run against
//! real upstream is a manual step (`cargo xtask fetch-corpus`).
//!
//! The source has `* text=auto eol=crlf` in its `.gitattributes` and
//! text-looking files, so a checkout would rewrite their line endings: the
//! command must hand back the raw blobs, byte for byte.
//!
//! US-RR-11-3 adds two read-only scans of this repository's working tree:
//! after a fetch run from inside it (no image bytes, no fetched git objects),
//! and against the committed manifest (no stock-ROM image anywhere in it).
//!
//! US-RR-16-5 (D-028) adds the ECU definitions: a second synthetic source
//! whose pinned subtree has file names with spaces (as upstream's do), files
//! outside the subtree, and CRLF text behind `* text=auto eol=crlf`, pinned
//! in a `[defs]` table; and the logger package, a synthetic stand-in pinned
//! by size and SHA-256 that is never downloaded, only verified when present.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest as _, Sha256};
use xtask::corpus_manifest::{
    DefFile, DefsCorpus, LoggerPackage, Manifest, RomCorpus, RomFile, SCHEMA,
};
use xtask::fetch_corpus::{
    DEFS_SOURCE_ENV, DEFS_STAMP_FILE, DIR_ENV, LOGGER_ZIP_ENV, SOURCE_ENV, STAMP_FILE,
};

/// A fresh, empty directory under the system temp dir for test `name`.
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rr-fetch-corpus-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "core.autocrlf=false",
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// Deterministic pseudo-random bytes (xorshift64*).
fn noise(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed | 1;
    (0..len)
        .map(|_| {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            (x.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 56) as u8
        })
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

const A: &str = "ADM/Forester/A2WC400H-2005-ADM-Subaru-Forester-XT-AT.hex";
const B: &str = "JDM/Legacy/AE5I410A-2011-JDM-Subaru-Legacy-GT-AT.hex";
const LF_TEXT: &str = "USDM/Impreza/lf-text.hex";
const CRLF_TEXT: &str = "USDM/Impreza/crlf-text.hex";

/// The synthetic corpus: path and bytes, sorted by path.
fn corpus() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        (A, noise(1, 4096)),
        (B, noise(2, 3001)),
        (CRLF_TEXT, b"one\r\ntwo\r\nthree\r\n".repeat(20)),
        (LF_TEXT, b"one\ntwo\nthree\n".repeat(20)),
    ]
}

struct Source {
    repo: PathBuf,
    commit: String,
    manifest: Manifest,
}

/// A source repository whose pinned commit holds [`corpus`] plus files that
/// are not in the manifest, followed by a later commit that changes `B` (so
/// fetching anything but the pin is caught).
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn make_source(base: &Path) -> Source {
    let repo = base.join("source");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--quiet"]);
    for (path, bytes) in corpus() {
        let p = repo.join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }
    fs::write(repo.join("README.md"), "synthetic test corpus\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "--quiet", "-m", "corpus"]);
    // Attributes that would rewrite every text-looking file on checkout.
    fs::write(repo.join(".gitattributes"), "* text=auto eol=crlf\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "--quiet", "-m", "attributes"]);
    let commit = git(&repo, &["rev-parse", "HEAD"]);
    fs::write(repo.join(B), noise(99, 3001)).unwrap();
    git(&repo, &["commit", "--quiet", "-am", "after the pin"]);

    let files = corpus()
        .into_iter()
        .map(|(path, bytes)| RomFile {
            path: path.to_owned(),
            size: bytes.len() as u64,
            sha256: sha256_hex(&bytes),
            filename_rom_id: None,
            year: None,
            year_end: None,
            region: None,
            model: None,
            trim: None,
            suspect: None,
        })
        .collect();
    let manifest = Manifest {
        schema: SCHEMA,
        roms: RomCorpus {
            upstream: "https://example.invalid/not-used.git".to_owned(),
            commit: commit.clone(),
            path_prefix: String::new(),
            files,
        },
        defs: None,
    };
    Source {
        repo,
        commit,
        manifest,
    }
}

#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn write_manifest(path: &Path, m: &Manifest) {
    fs::write(path, m.render().unwrap()).unwrap();
}

/// Runs `cargo xtask fetch-corpus <args>` with `env` set and the corpus
/// variables otherwise removed; returns the exit code and all output.
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn xtask(args: &[&dyn AsRef<std::ffi::OsStr>], env: &[(&str, &Path)]) -> (i32, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xtask"));
    cmd.arg("fetch-corpus")
        .args(args.iter().map(|a| a.as_ref()))
        .env_remove(DIR_ENV)
        .env_remove(SOURCE_ENV)
        .env_remove(DEFS_SOURCE_ENV)
        .env_remove(LOGGER_ZIP_ENV);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// Everything under `dir`, relative and `/`-separated, sorted.
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn listing(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, rel: &str, out: &mut Vec<String>) {
        for e in fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let name = e.file_name().to_string_lossy().into_owned();
            let r = if rel.is_empty() {
                name
            } else {
                format!("{rel}/{name}")
            };
            if e.file_type().unwrap().is_dir() {
                walk(&e.path(), &r, out);
            } else {
                out.push(r);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, "", &mut out);
    out.sort();
    out
}

#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn assert_is_corpus(cache: &Path) {
    let mut want: Vec<String> = corpus().iter().map(|(p, _)| (*p).to_owned()).collect();
    want.push(STAMP_FILE.to_owned());
    want.sort();
    assert_eq!(listing(cache), want);
    for (path, bytes) in corpus() {
        assert_eq!(fs::read(cache.join(path)).unwrap(), bytes, "{path}");
    }
}

/// Nothing but `keep` is left in `parent` (no work directories).
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn assert_no_leftovers(parent: &Path, keep: &[&str]) {
    let mut names: Vec<String> = fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, keep, "in {}", parent.display());
}

/// A manifest and a fetched, verified cache, for tests that start from one.
struct Fetched {
    base: PathBuf,
    source: Source,
    manifest: PathBuf,
    cache: PathBuf,
}

fn fetched(name: &str) -> Fetched {
    let base = scratch(name);
    let source = make_source(&base);
    let manifest = base.join("manifest.toml");
    write_manifest(&manifest, &source.manifest);
    let cache = base.join("cache").join("corpus");
    let (code, out) = xtask(
        &[
            &"--source",
            &source.repo,
            &"--dir",
            &cache,
            &"--manifest",
            &manifest,
        ],
        &[],
    );
    assert_eq!(code, 0, "{out}");
    Fetched {
        base,
        source,
        manifest,
        cache,
    }
}

#[test]
fn fetches_raw_blobs_at_the_pin_and_a_second_run_does_no_network_io() {
    let f = fetched("first");
    assert_is_corpus(&f.cache);
    assert_no_leftovers(&f.base.join("cache"), &["corpus"]);
    assert!(
        fs::read_to_string(f.cache.join(STAMP_FILE))
            .unwrap()
            .contains(&f.source.commit)
    );

    // The source no longer exists: success proves nothing was fetched.
    let gone = f.base.join("no-such-source");
    let (code, out) = xtask(
        &[
            &"--source",
            &gone,
            &"--dir",
            &f.cache,
            &"--manifest",
            &f.manifest,
        ],
        &[],
    );
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("nothing fetched"), "{out}");
    assert!(out.contains("4/4 image(s)"), "{out}");

    let (code, out) = xtask(
        &[
            &"--verify-only",
            &"--dir",
            &f.cache,
            &"--manifest",
            &f.manifest,
        ],
        &[],
    );
    assert_eq!(code, 0, "{out}");
    assert_is_corpus(&f.cache);
    fs::remove_dir_all(&f.base).unwrap();
}

#[test]
fn verify_only_lists_every_bad_path_and_changes_nothing() {
    let f = fetched("verify-only");
    fs::remove_file(f.cache.join(A)).unwrap();
    let b = fs::read(f.cache.join(B)).unwrap();
    fs::write(f.cache.join(B), &b[..b.len() - 1]).unwrap();
    let mut t = fs::read(f.cache.join(LF_TEXT)).unwrap();
    t[0] ^= 0x20;
    fs::write(f.cache.join(LF_TEXT), &t).unwrap();
    fs::write(f.cache.join("USDM/stray.bin"), b"x").unwrap();
    let before = listing(&f.cache);

    let (code, out) = xtask(
        &[
            &"--verify-only",
            &"--dir",
            &f.cache,
            &"--manifest",
            &f.manifest,
        ],
        &[],
    );
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("4 problem(s)"), "{out}");
    assert!(out.contains(&format!("{A}: missing")), "{out}");
    assert!(
        out.contains(&format!("{B}: size 3000 bytes, manifest says 3001")),
        "{out}"
    );
    assert!(out.contains(&format!("{LF_TEXT}: sha256 ")), "{out}");
    assert!(
        out.contains("USDM/stray.bin: not in the manifest (extra file)"),
        "{out}"
    );
    assert!(!out.contains(CRLF_TEXT), "a good file is not listed: {out}");
    assert_eq!(listing(&f.cache), before, "verify-only wrote something");
    assert_eq!(fs::read(f.cache.join(LF_TEXT)).unwrap(), t);
    fs::remove_dir_all(&f.base).unwrap();
}

#[test]
fn a_bad_cache_is_reported_and_replaced_by_a_verified_fetch() {
    let f = fetched("repair");
    fs::write(f.cache.join(B), b"truncated").unwrap();
    let mut a = fs::read(f.cache.join(A)).unwrap();
    a[100] ^= 1;
    fs::write(f.cache.join(A), &a).unwrap();
    fs::write(f.cache.join("stray.hex"), b"x").unwrap();
    fs::remove_file(f.cache.join(CRLF_TEXT)).unwrap();

    let (code, out) = xtask(
        &[
            &"--source",
            &f.source.repo,
            &"--dir",
            &f.cache,
            &"--manifest",
            &f.manifest,
        ],
        &[],
    );
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("did not verify (4 problem(s)) and was replaced"),
        "{out}"
    );
    for p in [A, B, CRLF_TEXT, "stray.hex"] {
        assert!(out.contains(p), "{p} not reported: {out}");
    }
    assert_is_corpus(&f.cache);
    assert_no_leftovers(&f.base.join("cache"), &["corpus"]);
    fs::remove_dir_all(&f.base).unwrap();
}

#[test]
fn hash_mismatch_from_the_source_fails_loudly_and_keeps_the_cache() {
    let f = fetched("hash-mismatch");
    let mut bad = f.source.manifest.clone();
    for file in &mut bad.roms.files {
        if file.path == A || file.path == LF_TEXT {
            file.sha256 = "0".repeat(64);
        }
    }
    let bad_path = f.base.join("bad.toml");
    write_manifest(&bad_path, &bad);

    let (code, out) = xtask(
        &[
            &"--source",
            &f.source.repo,
            &"--dir",
            &f.cache,
            &"--manifest",
            &bad_path,
        ],
        &[],
    );
    assert_ne!(code, 0, "{out}");
    assert!(
        out.contains("does not match the manifest (2 problem(s))"),
        "{out}"
    );
    assert!(out.contains(&format!("{A}: sha256 ")), "{out}");
    assert!(out.contains(&format!("{LF_TEXT}: sha256 ")), "{out}");
    assert!(out.contains("existing cache was not changed"), "{out}");
    // The old cache is intact and still verifies against the real manifest.
    assert_is_corpus(&f.cache);
    assert_no_leftovers(&f.base.join("cache"), &["corpus"]);

    // Into a new directory: nothing is created at all.
    let fresh = f.base.join("cache").join("fresh");
    let (code, out) = xtask(
        &[
            &"--source",
            &f.source.repo,
            &"--dir",
            &fresh,
            &"--manifest",
            &bad_path,
        ],
        &[],
    );
    assert_ne!(code, 0, "{out}");
    assert!(!fresh.exists());
    assert_no_leftovers(&f.base.join("cache"), &["corpus"]);
    fs::remove_dir_all(&f.base).unwrap();
}

#[test]
fn size_mismatch_or_missing_file_in_the_source_fails_and_creates_nothing() {
    let base = scratch("size-missing");
    let source = make_source(&base);
    let mut bad = source.manifest.clone();
    for file in &mut bad.roms.files {
        if file.path == B {
            file.size += 1;
        }
    }
    bad.roms.files.push(RomFile {
        path: "USDM/WRX/not-upstream.hex".to_owned(),
        ..bad.roms.files[0].clone()
    });
    bad.roms.files.sort_by(|x, y| x.path.cmp(&y.path));
    let manifest = base.join("bad.toml");
    write_manifest(&manifest, &bad);
    let cache = base.join("cache").join("corpus");
    fs::create_dir_all(base.join("cache")).unwrap();

    let (code, out) = xtask(
        &[
            &"--source",
            &source.repo,
            &"--dir",
            &cache,
            &"--manifest",
            &manifest,
        ],
        &[],
    );
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("(2 problem(s))"), "{out}");
    assert!(
        out.contains(&format!("{B}: size 3001 bytes, manifest says 3002")),
        "{out}"
    );
    assert!(out.contains("USDM/WRX/not-upstream.hex: missing"), "{out}");
    assert!(!cache.exists());
    assert_no_leftovers(&base.join("cache"), &[]);
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn the_source_defaults_to_the_manifest_then_the_environment() {
    let base = scratch("default-source");
    let source = make_source(&base);
    let mut m = source.manifest.clone();
    m.roms.upstream = source.repo.to_string_lossy().into_owned();
    let manifest = base.join("manifest.toml");
    write_manifest(&manifest, &m);

    // No --source, no RR_CORPUS_SOURCE: the manifest's upstream. No --dir:
    // RR_CORPUS_DIR.
    let cache = base.join("from-manifest");
    let (code, out) = xtask(&[&"--manifest", &manifest], &[(DIR_ENV, &cache)]);
    assert_eq!(code, 0, "{out}");
    assert_is_corpus(&cache);

    // RR_CORPUS_SOURCE wins over the manifest (here, an unusable upstream).
    write_manifest(&manifest, &source.manifest);
    let cache = base.join("from-env");
    let (code, out) = xtask(
        &[&"--dir", &cache, &"--manifest", &manifest],
        &[(SOURCE_ENV, &source.repo)],
    );
    assert_eq!(code, 0, "{out}");
    assert_is_corpus(&cache);
    fs::remove_dir_all(&base).unwrap();
}

#[cfg(unix)]
#[test]
fn a_file_url_source_works() {
    let base = scratch("file-url");
    let source = make_source(&base);
    let manifest = base.join("manifest.toml");
    write_manifest(&manifest, &source.manifest);
    let url = format!("file://{}", source.repo.display());
    let cache = base.join("corpus");
    let (code, out) = xtask(
        &[
            &"--source",
            &url,
            &"--dir",
            &cache,
            &"--manifest",
            &manifest,
        ],
        &[],
    );
    assert_eq!(code, 0, "{out}");
    assert_is_corpus(&cache);
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn a_directory_inside_the_repository_is_refused() {
    let base = scratch("in-repo");
    let source = make_source(&base);
    let manifest = base.join("manifest.toml");
    write_manifest(&manifest, &source.manifest);
    let inside = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("target")
        .join(format!("rr-fetch-corpus-refused-{}", std::process::id()));
    for verify_only in [false, true] {
        let mut args: Vec<&dyn AsRef<std::ffi::OsStr>> = vec![
            &"--source",
            &source.repo,
            &"--dir",
            &inside,
            &"--manifest",
            &manifest,
        ];
        if verify_only {
            args.push(&"--verify-only");
        }
        let (code, out) = xtask(&args, &[]);
        assert_ne!(code, 0, "{out}");
        assert!(out.contains("inside the repository"), "{out}");
        assert!(!inside.exists());
    }
    // Via RR_CORPUS_DIR too.
    let (code, out) = xtask(
        &[&"--source", &source.repo, &"--manifest", &manifest],
        &[(DIR_ENV, &inside)],
    );
    assert_ne!(code, 0, "{out}");
    assert!(!inside.exists());
    fs::remove_dir_all(&base).unwrap();
}

/// US-RR-11-9: other spellings of a place inside the repository are refused
/// too: a symlink to it, a relative path (resolved against the current
/// directory), `..` back into it, and the directory containing it. Nothing
/// is created; the repo is never written.
#[test]
fn disguised_directories_inside_the_repository_are_refused() {
    let base = scratch("in-repo-disguised");
    let source = make_source(&base);
    let manifest = base.join("manifest.toml");
    write_manifest(&manifest, &source.manifest);
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let name = format!("rr-fetch-corpus-disguised-{}", std::process::id());
    let target_dir = repo.join("target");
    let mut dirs = vec![
        // `..` through a directory that does not exist.
        repo.join("target/no-such-dir/../..")
            .join("target")
            .join(&name),
        repo.join("xtask").join("..").join("target").join(&name),
    ];
    #[cfg(unix)]
    {
        let link = base.join("to-target");
        std::os::unix::fs::symlink(&target_dir, &link).unwrap();
        dirs.push(link.join(&name));
    }
    for dir in &dirs {
        for verify_only in [false, true] {
            let mut args: Vec<&dyn AsRef<std::ffi::OsStr>> = vec![
                &"--source",
                &source.repo,
                &"--dir",
                dir,
                &"--manifest",
                &manifest,
            ];
            if verify_only {
                args.push(&"--verify-only");
            }
            let (code, out) = xtask(&args, &[]);
            assert_ne!(code, 0, "{}: {out}", dir.display());
            assert!(out.contains("inside the repository"), "{out}");
        }
    }
    // Relative to the current directory: run from inside the repo.
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(&repo)
        .args(["fetch-corpus", "--verify-only", "--dir"])
        .arg(Path::new("target").join(&name))
        .arg("--manifest")
        .arg(&manifest)
        .env_remove(DIR_ENV)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("inside the repository"),
        "{out:?}"
    );
    // The repository's parent contains it.
    let (code, out) = xtask(
        &[
            &"--verify-only",
            &"--dir",
            &repo.join(".."),
            &"--manifest",
            &manifest,
        ],
        &[],
    );
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("contains the repository"), "{out}");
    assert!(!target_dir.join(&name).exists());
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn a_directory_it_does_not_manage_is_never_replaced() {
    let base = scratch("not-ours");
    let source = make_source(&base);
    let manifest = base.join("manifest.toml");
    write_manifest(&manifest, &source.manifest);
    let dir = base.join("mine");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("notes.txt"), b"keep me").unwrap();

    let (code, out) = xtask(
        &[
            &"--source",
            &source.repo,
            &"--dir",
            &dir,
            &"--manifest",
            &manifest,
        ],
        &[],
    );
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("refusing to replace it"), "{out}");
    assert_eq!(listing(&dir), ["notes.txt"]);

    // An existing empty directory is fine.
    let empty = base.join("empty");
    fs::create_dir_all(&empty).unwrap();
    let (code, out) = xtask(
        &[
            &"--source",
            &source.repo,
            &"--dir",
            &empty,
            &"--manifest",
            &manifest,
        ],
        &[],
    );
    assert_eq!(code, 0, "{out}");
    assert_is_corpus(&empty);
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn verify_only_without_a_cache_fails() {
    let base = scratch("no-cache");
    let source = make_source(&base);
    let manifest = base.join("manifest.toml");
    write_manifest(&manifest, &source.manifest);
    let (code, out) = xtask(
        &[
            &"--verify-only",
            &"--dir",
            &base.join("absent"),
            &"--manifest",
            &manifest,
        ],
        &[],
    );
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("not a directory"), "{out}");
    assert!(!base.join("absent").exists());
    fs::remove_dir_all(&base).unwrap();
}

/// The verify-only report for `cache` against `manifest`: exit code and
/// output.
fn verify_only(cache: &Path, manifest: &Path) -> (i32, String) {
    xtask(
        &[&"--verify-only", &"--dir", &cache, &"--manifest", &manifest],
        &[],
    )
}

/// Asserts `out` reports exactly `problems` problems and lists every one of
/// `lines`, each on a line of its own.
fn assert_lists_all(out: &str, problems: usize, lines: &[String]) {
    assert!(
        out.contains(&format!("({problems} problem(s))")),
        "not {problems} problem(s): {out}"
    );
    for l in lines {
        assert!(
            out.lines().any(|o| o.trim().starts_with(l.as_str())),
            "{l:?} not listed: {out}"
        );
    }
}

/// US-RR-11-1: in a corrupted cache, two hash mismatches and two size
/// mismatches at once are all reported by `--verify-only` (non-zero exit,
/// every bad path listed, not just the first), and then all reported again
/// by a plain run while it replaces the cache.
#[test]
fn several_hash_and_size_mismatches_in_the_cache_are_all_listed() {
    let f = fetched("many-cache");
    for path in [A, LF_TEXT] {
        let mut b = fs::read(f.cache.join(path)).unwrap();
        b[7] ^= 0x01;
        fs::write(f.cache.join(path), &b).unwrap();
    }
    let b = fs::read(f.cache.join(B)).unwrap();
    fs::write(f.cache.join(B), &b[..1000]).unwrap();
    let mut c = fs::read(f.cache.join(CRLF_TEXT)).unwrap();
    c.extend_from_slice(b"\r\n");
    let c_len = c.len();
    fs::write(f.cache.join(CRLF_TEXT), &c).unwrap();
    let want = [
        format!("{A}: sha256 "),
        format!("{B}: size 1000 bytes, manifest says 3001"),
        format!(
            "{CRLF_TEXT}: size {c_len} bytes, manifest says {}",
            c_len - 2
        ),
        format!("{LF_TEXT}: sha256 "),
    ];

    let (code, out) = verify_only(&f.cache, &f.manifest);
    assert_eq!(code, 1, "{out}");
    assert_lists_all(&out, 4, &want);
    assert!(!out.contains(": ok:"), "{out}");

    let (code, out) = xtask(
        &[
            &"--source",
            &f.source.repo,
            &"--dir",
            &f.cache,
            &"--manifest",
            &f.manifest,
        ],
        &[],
    );
    assert_eq!(code, 0, "{out}");
    assert_lists_all(&out, 4, &want);
    assert_is_corpus(&f.cache);
    let (code, out) = verify_only(&f.cache, &f.manifest);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("ok: 4/4 image(s)"), "{out}");
    fs::remove_dir_all(&f.base).unwrap();
}

/// US-RR-11-1: when the source's bytes disagree with the manifest in several
/// ways at once (two hashes, two sizes, one path it does not have), a fetch
/// fails (non-zero exit) listing all five in one report, in both a fresh
/// directory and over an existing cache, and leaves nothing behind that
/// `--verify-only` would accept: no directory, an empty one, or the old
/// cache unchanged. `--verify-only` against the same manifest lists the same
/// five.
#[test]
fn several_mismatches_from_the_source_are_all_listed_and_nothing_partial_is_kept() {
    let f = fetched("many-source");
    let mut bad = f.source.manifest.clone();
    for file in &mut bad.roms.files {
        match file.path.as_str() {
            p if p == A || p == LF_TEXT => file.sha256 = "0".repeat(64),
            p if p == B || p == CRLF_TEXT => file.size += 5,
            _ => {}
        }
    }
    bad.roms.files.push(RomFile {
        path: "USDM/WRX/not-upstream.hex".to_owned(),
        ..bad.roms.files[0].clone()
    });
    bad.roms.files.sort_by(|x, y| x.path.cmp(&y.path));
    let bad_path = f.base.join("bad.toml");
    write_manifest(&bad_path, &bad);
    let crlf_len = corpus()
        .iter()
        .find(|(p, _)| *p == CRLF_TEXT)
        .map(|(_, b)| b.len())
        .unwrap();
    let want = [
        format!("{A}: sha256 "),
        format!("{B}: size 3001 bytes, manifest says 3006"),
        format!(
            "{CRLF_TEXT}: size {crlf_len} bytes, manifest says {}",
            crlf_len + 5
        ),
        format!("{LF_TEXT}: sha256 "),
        "USDM/WRX/not-upstream.hex: missing".to_owned(),
    ];
    let fetch = |dir: &Path| {
        xtask(
            &[
                &"--source",
                &f.source.repo,
                &"--dir",
                &dir,
                &"--manifest",
                &bad_path,
            ],
            &[],
        )
    };

    // A fresh directory: nothing is created.
    let fresh = f.base.join("cache").join("fresh");
    let (code, out) = fetch(&fresh);
    assert_eq!(code, 1, "{out}");
    assert_lists_all(&out, 5, &want);
    assert!(out.contains("existing cache was not changed"), "{out}");
    assert!(!fresh.exists());
    assert_ne!(verify_only(&fresh, &bad_path).0, 0);
    assert_ne!(verify_only(&fresh, &f.manifest).0, 0);

    // An existing empty directory stays empty: no stamp, no partial images.
    let empty = f.base.join("cache").join("empty");
    fs::create_dir_all(&empty).unwrap();
    let (code, out) = fetch(&empty);
    assert_eq!(code, 1, "{out}");
    assert_lists_all(&out, 5, &want);
    assert!(listing(&empty).is_empty(), "{:?}", listing(&empty));
    let (code, out) = verify_only(&empty, &f.manifest);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("(4 problem(s))"), "{out}");

    // Over a good cache: the cache is untouched and still verifies.
    let (code, out) = fetch(&f.cache);
    assert_eq!(code, 1, "{out}");
    assert_lists_all(&out, 5, &want);
    assert_is_corpus(&f.cache);
    assert_eq!(verify_only(&f.cache, &f.manifest).0, 0);
    assert_no_leftovers(&f.base.join("cache"), &["corpus", "empty"]);

    // --verify-only of the good bytes against the bad manifest: the same five.
    let (code, out) = verify_only(&f.cache, &bad_path);
    assert_eq!(code, 1, "{out}");
    assert_lists_all(&out, 5, &want);
    fs::remove_dir_all(&f.base).unwrap();
}

/// `git <args>` in the working tree at `dir`, with nothing from the
/// environment redirecting it; `None` unless it succeeds.
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn git_in(dir: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("git");
    for var in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        cmd.env_remove(var);
    }
    let out = cmd.arg("-C").arg(dir).args(args).output().unwrap();
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// The git working tree this workspace is in, canonicalised.
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn working_tree() -> PathBuf {
    let ws = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let top = git_in(&ws, &["rev-parse", "--show-toplevel"]).unwrap();
    Path::new(&top).canonicalize().unwrap()
}

/// Every regular file under `root` (symlinks not followed), skipping `.git`
/// (git's object store is checked with `git cat-file` instead: its objects
/// are compressed, so a byte scan cannot see them). Entries that vanish
/// while walking (other tests and builds use `target/` concurrently) are
/// skipped.
fn files_under(root: &Path) -> Vec<(PathBuf, fs::Metadata)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            if e.file_name() == ".git" {
                continue;
            }
            let Ok(ty) = e.file_type() else { continue };
            if ty.is_dir() {
                stack.push(e.path());
            } else if ty.is_file()
                && let Ok(meta) = e.metadata()
            {
                out.push((e.path(), meta));
            }
        }
    }
    out
}

/// The pinned commit of `source` and the blob of every [`corpus`] image in it.
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn source_oids(source: &Source) -> Vec<String> {
    let mut oids = vec![source.commit.clone()];
    for (path, _) in corpus() {
        let rev = format!("{}:{path}", source.commit);
        oids.push(git_in(&source.repo, &["rev-parse", &rev]).unwrap());
    }
    oids
}

/// The objects of `oids` present (loose, packed or via alternates) in the
/// repository whose working tree is `top`.
fn objects_in(top: &Path, oids: &[String]) -> Vec<String> {
    oids.iter()
        .filter(|oid| git_in(top, &["cat-file", "-e", oid]).is_some())
        .cloned()
        .collect()
}

/// Every file under `top` ([`files_under`]) modified since `since` that
/// begins with the first bytes of one of the pseudo-random [`corpus`] images
/// (an image, or a partial download of one), described.
fn image_bytes_under(top: &Path, since: std::time::SystemTime) -> Vec<String> {
    // The text images are too plain to look for.
    const PREFIX: usize = 64;
    let images: Vec<(&str, Vec<u8>)> = corpus()
        .into_iter()
        .filter(|(p, _)| [A, B].contains(p))
        .map(|(p, bytes)| (p, bytes[..PREFIX].to_vec()))
        .collect();
    let mut hits = Vec::new();
    for (path, meta) in files_under(top) {
        let len = meta.len();
        if len < PREFIX as u64 || meta.modified().is_ok_and(|t| t < since) {
            continue;
        }
        let mut head = [0u8; PREFIX];
        let Ok(mut f) = fs::File::open(&path) else {
            continue;
        };
        if std::io::Read::read_exact(&mut f, &mut head).is_err() {
            continue;
        }
        if let Some((image, _)) = images.iter().find(|(_, p)| p[..] == head[..]) {
            hits.push(format!("{} ({len} bytes) holds {image}", path.display()));
        }
    }
    hits
}

/// US-RR-11-3 (D-025): a fetch with its cache outside the repository writes
/// no ROM bytes inside the working tree: not the images, not a partial
/// download, not its work directory, and not the git objects it fetches.
///
/// The command runs from the top of the working tree with `GIT_DIR`,
/// `GIT_WORK_TREE` and `GIT_OBJECT_DIRECTORY` pointing at this repository (as
/// inside a git hook), so a git call that honoured them would write the
/// fetched pack into this repository. Afterwards no file anywhere in the
/// working tree (`target/` and ignored paths included) written since the
/// fetch started begins with the bytes of a corpus image, and none of the
/// fetched commit and blob objects is in this repository's object store.
#[test]
fn a_fetch_writes_no_rom_bytes_inside_the_repository() {
    let base = scratch("no-bytes-in-repo");
    let source = make_source(&base);
    let manifest = base.join("manifest.toml");
    write_manifest(&manifest, &source.manifest);
    let cache = base.join("cache").join("corpus");
    let top = working_tree();
    let git_dir = git_in(&top, &["rev-parse", "--absolute-git-dir"]).unwrap();
    let objects = git_in(
        &top,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "objects",
        ],
    )
    .unwrap();

    let oids = source_oids(&source);
    assert!(
        objects_in(&top, &oids).is_empty(),
        "synthetic corpus objects are already in {} before the fetch",
        top.display()
    );

    // The start, as the file system dates files (not the wall clock, D-010);
    // with coarse timestamps, anything from two seconds before counts.
    let marker = base.join("start-marker");
    fs::write(&marker, b"").unwrap();
    let start =
        fs::metadata(&marker).unwrap().modified().unwrap() - std::time::Duration::from_secs(2);
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(&top)
        .arg("fetch-corpus")
        .arg("--source")
        .arg(&source.repo)
        .arg("--dir")
        .arg(&cache)
        .arg("--manifest")
        .arg(&manifest)
        .env_remove(DIR_ENV)
        .env_remove(SOURCE_ENV)
        .env("GIT_DIR", &git_dir)
        .env("GIT_WORK_TREE", &top)
        .env("GIT_OBJECT_DIRECTORY", &objects)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_is_corpus(&cache);
    assert_no_leftovers(&base.join("cache"), &["corpus"]);

    let leaked = objects_in(&top, &oids);
    assert!(
        leaked.is_empty(),
        "fetched objects {leaked:?} were written into {}",
        top.display()
    );
    let hits = image_bytes_under(&top, start);
    assert!(
        hits.is_empty(),
        "corpus bytes inside the working tree {}:\n{}",
        top.display(),
        hits.join("\n")
    );
    fs::remove_dir_all(&base).unwrap();
}

/// US-RR-11-3 (D-025): no stock-ROM image of the committed manifest is
/// anywhere in the working tree (tracked, untracked or ignored, `target/`
/// included; `.git` aside): no file has both the size and the SHA-256 of a
/// manifest entry. (The commit-time ROM-leak guard for `fixtures/public`,
/// with ROM-ID patterns and a size cap, is US-RR-13.)
#[test]
fn no_corpus_image_is_inside_the_working_tree() {
    let top = working_tree();
    let text = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/corpus-manifest.toml"),
    )
    .unwrap();
    let manifest = Manifest::from_toml(&text).unwrap();
    let by_size: std::collections::BTreeMap<u64, Vec<&RomFile>> =
        manifest
            .roms
            .files
            .iter()
            .fold(Default::default(), |mut m, f| {
                m.entry(f.size).or_default().push(f);
                m
            });
    let mut hits = Vec::new();
    for (path, meta) in files_under(&top) {
        let Some(files) = by_size.get(&meta.len()) else {
            continue;
        };
        let Ok((_, sha)) = xtask::corpus_manifest::hash_file(&path) else {
            continue;
        };
        if let Some(f) = files.iter().find(|f| f.sha256 == sha) {
            hits.push(format!("{} is the corpus image {}", path.display(), f.path));
        }
    }
    assert!(
        hits.is_empty(),
        "stock-ROM images inside the working tree {} (D-025; keep the corpus in RR_CORPUS_DIR):\n{}",
        top.display(),
        hits.join("\n")
    );
}

// ---------------------------------------------------------------------------
// ECU definitions and the logger package (US-RR-16-5, D-028).

const DEFS_PREFIX: &str = "RomRaider/";
const DEFS_TOP: &str = "RR_D2UH001L.xml";
/// A doubled space before the extension, as upstream's Baja file has.
const DEFS_SPACED: &str = "ecu/2004USDMBajaXTAT-A2WC50  .xml";
/// Spaces inside the name, CRLF text.
const DEFS_CRLF: &str = "ecu/RR D2UJ001T.xml";
const DEFS_LOGGER: &str = "logger/logger_STD_EN_v290.xml";
const LOGGER_FILE: &str = "logger_v370.zip";
/// A file under [`DEFS_PREFIX`] at the pin that the manifest does not list
/// (US-RR-16-2): skipped, never extracted into the cache.
const DEFS_UNLISTED: &str = "ecu/unlisted extra.xml";

/// The synthetic definition files: path below [`DEFS_PREFIX`] and bytes,
/// sorted by path.
fn defs_files() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        (DEFS_TOP, b"<roms/>\n".to_vec()),
        (DEFS_SPACED, b"<rom>\n  <table/>\n</rom>\n".repeat(30)),
        (DEFS_CRLF, b"<rom>\r\n  <table/>\r\n</rom>\r\n".repeat(30)),
        (DEFS_LOGGER, noise(7, 2048)),
    ]
}

/// A synthetic stand-in for the logger package (never the real one).
fn logger_bytes() -> Vec<u8> {
    noise(70, 5000)
}

struct DefsSource {
    repo: PathBuf,
    defs: DefsCorpus,
}

/// A definitions source whose pinned commit holds [`defs_files`] under
/// [`DEFS_PREFIX`], plus [`DEFS_UNLISTED`] and files outside the prefix
/// that the manifest does not list, followed by a later commit that
/// changes one (so fetching anything but the pin is caught).
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn make_defs_source(base: &Path) -> DefsSource {
    let repo = base.join("defs-source");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--quiet"]);
    for (path, bytes) in defs_files() {
        let p = repo.join(DEFS_PREFIX).join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }
    fs::write(repo.join(DEFS_PREFIX).join(DEFS_UNLISTED), "<rom/>\n").unwrap();
    fs::create_dir_all(repo.join("ECUFlash")).unwrap();
    fs::write(repo.join("ECUFlash/not corpus.xml"), "<rom/>\n").unwrap();
    fs::write(repo.join("README.md"), "synthetic test definitions\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "--quiet", "-m", "defs"]);
    fs::write(repo.join(".gitattributes"), "* text=auto eol=crlf\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "--quiet", "-m", "attributes"]);
    let commit = git(&repo, &["rev-parse", "HEAD"]);
    fs::write(
        repo.join(DEFS_PREFIX).join(DEFS_TOP),
        "<roms>changed</roms>\n",
    )
    .unwrap();
    git(&repo, &["commit", "--quiet", "-am", "after the pin"]);

    let logger = logger_bytes();
    let defs = DefsCorpus {
        upstream: "https://example.invalid/defs-not-used.git".to_owned(),
        branch: "Alpha".to_owned(),
        commit,
        path_prefix: DEFS_PREFIX.to_owned(),
        licence: "none stated (synthetic test data)".to_owned(),
        logger_v370: LoggerPackage {
            file: LOGGER_FILE.to_owned(),
            size: logger.len() as u64,
            sha256: sha256_hex(&logger),
            url: "https://example.invalid/never-downloaded".to_owned(),
            note: "synthetic".to_owned(),
        },
        files: defs_files()
            .into_iter()
            .map(|(path, bytes)| DefFile {
                path: path.to_owned(),
                size: bytes.len() as u64,
                sha256: sha256_hex(&bytes),
            })
            .collect(),
    };
    DefsSource { repo, defs }
}

/// A ROM source and a definitions source, a manifest naming both, and a
/// cache fetched from them.
struct FetchedDefs {
    base: PathBuf,
    roms: Source,
    defs: DefsSource,
    manifest: PathBuf,
    cache: PathBuf,
    /// The first run's output.
    out: String,
}

impl FetchedDefs {
    fn defs_tree(&self) -> PathBuf {
        self.cache.join("defs").join("romraider")
    }

    fn logger(&self) -> PathBuf {
        self.cache.join("defs").join(LOGGER_FILE)
    }

    /// `fetch-corpus <args> --dir <cache> --manifest <manifest>`.
    fn run(&self, args: &[&dyn AsRef<std::ffi::OsStr>], env: &[(&str, &Path)]) -> (i32, String) {
        let mut all: Vec<&dyn AsRef<std::ffi::OsStr>> = args.to_vec();
        all.extend([
            &"--dir" as &dyn AsRef<std::ffi::OsStr>,
            &self.cache,
            &"--manifest",
            &self.manifest,
        ]);
        xtask(&all, env)
    }

    /// A run whose sources do not exist: success proves nothing was fetched.
    fn run_offline(&self, extra: &[&dyn AsRef<std::ffi::OsStr>]) -> (i32, String) {
        let gone = self.base.join("no-such-source");
        let mut args: Vec<&dyn AsRef<std::ffi::OsStr>> =
            vec![&"--source", &gone, &"--defs-source", &gone];
        args.extend_from_slice(extra);
        self.run(&args, &[])
    }

    fn verify_only(&self) -> (i32, String) {
        self.run(&[&"--verify-only"], &[])
    }
}

#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn manifest_with_defs(base: &Path, roms: &Source, defs: &DefsCorpus) -> PathBuf {
    let mut m = roms.manifest.clone();
    m.defs = Some(defs.clone());
    let path = base.join("manifest.toml");
    fs::write(&path, m.render().unwrap()).unwrap();
    path
}

fn fetched_with_defs(name: &str) -> FetchedDefs {
    let base = scratch(name);
    let roms = make_source(&base);
    let defs = make_defs_source(&base);
    let manifest = manifest_with_defs(&base, &roms, &defs.defs);
    let mut f = FetchedDefs {
        cache: base.join("cache").join("corpus"),
        base,
        roms,
        defs,
        manifest,
        out: String::new(),
    };
    let (code, out) = f.run(
        &[&"--source", &f.roms.repo, &"--defs-source", &f.defs.repo],
        &[],
    );
    assert_eq!(code, 0, "{out}");
    f.out = out;
    f
}

/// The definitions tree is exactly [`defs_files`] (raw bytes) and its stamp.
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn assert_is_defs(tree: &Path) {
    let mut want: Vec<String> = defs_files().iter().map(|(p, _)| (*p).to_owned()).collect();
    want.push(DEFS_STAMP_FILE.to_owned());
    want.sort();
    assert_eq!(listing(tree), want);
    for (path, bytes) in defs_files() {
        assert_eq!(fs::read(tree.join(path)).unwrap(), bytes, "{path}");
    }
}

/// The ROM part of the cache is exactly [`corpus`] and its stamp (the
/// `defs/` directory aside).
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn assert_roms_intact(cache: &Path) {
    let mut want: Vec<String> = corpus().iter().map(|(p, _)| (*p).to_owned()).collect();
    want.push(STAMP_FILE.to_owned());
    want.sort();
    let roms: Vec<String> = listing(cache)
        .into_iter()
        .filter(|p| !p.starts_with("defs/"))
        .collect();
    assert_eq!(roms, want);
    for (path, bytes) in corpus() {
        assert_eq!(fs::read(cache.join(path)).unwrap(), bytes, "{path}");
    }
}

#[test]
fn fetches_and_verifies_the_definitions_alongside_the_roms_with_spaces_in_paths() {
    let f = fetched_with_defs("defs-first");
    assert!(f.out.contains("fetch-corpus: ok: fetched "), "{}", f.out);
    assert!(
        f.out.contains(&format!(
            "fetch-corpus: defs: ok: fetched `{DEFS_PREFIX}` of {} at {}; 4/4 definition file(s), ",
            f.defs.repo.display(),
            f.defs.defs.commit
        )),
        "{}",
        f.out
    );
    // README.md, .gitattributes, ECUFlash/ and the unlisted file under the
    // prefix were not extracted (assert_is_defs below checks the listing).
    assert!(
        f.out.contains("(4 file(s) not in the manifest skipped)"),
        "{}",
        f.out
    );
    assert!(!f.defs_tree().join(DEFS_UNLISTED).exists());
    // No copy of the logger package: reported clearly, not an error.
    assert!(
        f.out
            .contains("fetch-corpus: logger v370: not present: no ")
            && f.out.contains(&f.logger().display().to_string())
            && f.out.contains("never downloaded")
            && f.out.contains("v290"),
        "{}",
        f.out
    );
    assert_is_defs(&f.defs_tree());
    assert_roms_intact(&f.cache);
    assert!(
        fs::read_to_string(f.defs_tree().join(DEFS_STAMP_FILE))
            .unwrap()
            .contains(&f.defs.defs.commit)
    );
    assert_no_leftovers(&f.base.join("cache"), &["corpus"]);
    assert_no_leftovers(&f.cache.join("defs"), &["romraider"]);

    // A complete cache is used as is: nothing fetched, for either part.
    let (code, out) = f.run_offline(&[]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("fetch-corpus: ok: 4/4 image(s), ") && out.contains("; nothing fetched"),
        "{out}"
    );
    assert!(
        out.contains("fetch-corpus: defs: ok: 4/4 definition file(s), "),
        "{out}"
    );
    assert_eq!(
        out.matches("; nothing fetched").count(),
        2,
        "both parts already verified: {out}"
    );

    let (code, out) = f.verify_only();
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("fetch-corpus: ok: 4/4 image(s), "), "{out}");
    assert!(
        out.contains("fetch-corpus: defs: ok: 4/4 definition file(s), "),
        "{out}"
    );
    assert!(
        out.contains("fetch-corpus: logger v370: not present"),
        "{out}"
    );
    assert_is_defs(&f.defs_tree());
    fs::remove_dir_all(&f.base).unwrap();
}

#[test]
fn a_manifest_without_defs_fetches_no_definitions() {
    let f = fetched("rom-only");
    let (code, out) = verify_only(&f.cache, &f.manifest);
    assert_eq!(code, 0, "{out}");
    assert!(
        !out.contains("defs:") && !out.contains("logger v370"),
        "ROM-only manifest: {out}"
    );
    assert!(!f.cache.join("defs").exists());
    fs::remove_dir_all(&f.base).unwrap();
}

#[test]
fn definition_mismatches_from_the_source_are_all_listed_and_nothing_is_kept() {
    let base = scratch("defs-bad-source");
    let roms = make_source(&base);
    let src = make_defs_source(&base);
    let mut defs = src.defs.clone();
    // Same size, wrong hash; wrong size; and a file the source lacks.
    defs.files[1].sha256 = "0".repeat(64);
    defs.files[2].size += 1;
    defs.files.insert(
        3,
        DefFile {
            path: "ecu/missing file.xml".to_owned(),
            size: 1,
            sha256: "1".repeat(64),
        },
    );
    let manifest = manifest_with_defs(&base, &roms, &defs);
    let cache = base.join("cache").join("corpus");
    let (code, out) = xtask(
        &[
            &"--source",
            &roms.repo,
            &"--defs-source",
            &src.repo,
            &"--dir",
            &cache,
            &"--manifest",
            &manifest,
        ],
        &[],
    );
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("3 problem(s)"), "{out}");
    assert!(
        out.contains(&format!(
            "{DEFS_SPACED}: sha256 {}, manifest says {}",
            src.defs.files[1].sha256,
            "0".repeat(64)
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "{DEFS_CRLF}: size {} bytes, manifest says {}",
            src.defs.files[2].size,
            src.defs.files[2].size + 1
        )),
        "{out}"
    );
    assert!(out.contains("ecu/missing file.xml: missing"), "{out}");
    assert!(
        !out.contains(&format!("{DEFS_TOP}:")),
        "a good file is listed: {out}"
    );
    // The ROMs were fetched; no definitions tree and no work directory.
    assert_roms_intact(&cache);
    assert_no_leftovers(&cache.join("defs"), &[]);
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn a_bad_definitions_cache_is_listed_by_verify_only_and_repaired_by_a_fetch() {
    let f = fetched_with_defs("defs-repair");
    let tree = f.defs_tree();
    let mut crlf = fs::read(tree.join(DEFS_CRLF)).unwrap();
    crlf[3] ^= 0x20;
    fs::write(tree.join(DEFS_CRLF), &crlf).unwrap();
    let logger_def = fs::read(tree.join(DEFS_LOGGER)).unwrap();
    fs::write(tree.join(DEFS_LOGGER), &logger_def[..100]).unwrap();
    fs::remove_file(tree.join(DEFS_SPACED)).unwrap();
    fs::write(tree.join("ecu/stray file.xml"), b"x").unwrap();
    let before = listing(&tree);

    let (code, out) = f.verify_only();
    assert_ne!(code, 0, "{out}");
    // The ROMs still verify; every bad definition is listed.
    assert!(out.contains("fetch-corpus: ok: 4/4 image(s), "), "{out}");
    assert!(out.contains("fetch-corpus: defs: "), "{out}");
    assert!(out.contains("4 problem(s)"), "{out}");
    assert!(out.contains(&format!("{DEFS_CRLF}: sha256 ")), "{out}");
    assert!(
        out.contains(&format!(
            "{DEFS_LOGGER}: size 100 bytes, manifest says 2048"
        )),
        "{out}"
    );
    assert!(out.contains(&format!("{DEFS_SPACED}: missing")), "{out}");
    assert!(
        out.contains("ecu/stray file.xml: not in the manifest (extra file)"),
        "{out}"
    );
    assert_eq!(listing(&tree), before, "verify-only wrote something");

    let (code, out) = f.run(
        &[
            &"--source",
            &f.base.join("gone"),
            &"--defs-source",
            &f.defs.repo,
        ],
        &[],
    );
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("did not verify (4 problem(s)) and were replaced"),
        "{out}"
    );
    assert!(out.contains("fetch-corpus: defs: ok: fetched "), "{out}");
    assert_is_defs(&tree);
    assert_roms_intact(&f.cache);
    assert_no_leftovers(&f.cache.join("defs"), &["romraider"]);
    fs::remove_dir_all(&f.base).unwrap();
}

#[test]
fn the_logger_package_is_verified_in_the_cache_and_a_mismatch_is_an_error() {
    let f = fetched_with_defs("logger-in-cache");
    let good = logger_bytes();
    fs::write(f.logger(), &good).unwrap();

    let ok_line = format!(
        "fetch-corpus: logger v370: ok: {} matches the pin ({} bytes, sha256 {})",
        f.logger().display(),
        good.len(),
        sha256_hex(&good)
    );
    let (code, out) = f.verify_only();
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&ok_line), "{out}");
    // The copy sits in `defs/`, so the ROM tree still verifies.
    assert!(out.contains("fetch-corpus: ok: 4/4 image(s), "), "{out}");
    let (code, out) = f.run_offline(&[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&ok_line), "{out}");

    // Same size, one byte changed: a hash mismatch fails both modes.
    let mut bad = good.clone();
    bad[10] ^= 1;
    fs::write(f.logger(), &bad).unwrap();
    for (code, out) in [f.verify_only(), f.run_offline(&[])] {
        assert_ne!(code, 0, "{out}");
        assert!(
            out.contains(&format!(
                "{}: sha256 {}, manifest pins {} for {LOGGER_FILE}: not the pinned package",
                f.logger().display(),
                sha256_hex(&bad),
                sha256_hex(&good)
            )),
            "{out}"
        );
    }
    // Truncated: a size mismatch.
    fs::write(f.logger(), &good[..4000]).unwrap();
    let (code, out) = f.verify_only();
    assert_ne!(code, 0, "{out}");
    assert!(
        out.contains(&format!(
            "{}: size 4000 bytes, manifest pins {} bytes for {LOGGER_FILE}",
            f.logger().display(),
            good.len()
        )),
        "{out}"
    );
    // The bad copy was only read: fetch-corpus never replaces or deletes it.
    assert_eq!(fs::read(f.logger()).unwrap(), &good[..4000]);
    fs::remove_dir_all(&f.base).unwrap();
}

#[test]
fn a_supplied_logger_package_is_verified_where_it_is_and_never_copied() {
    let f = fetched_with_defs("logger-supplied");
    let good = logger_bytes();
    let private = f.base.join("private mirror");
    fs::create_dir_all(&private).unwrap();
    let copy = private.join(LOGGER_FILE);
    fs::write(&copy, &good).unwrap();
    let ok_line = format!(
        "fetch-corpus: logger v370: ok: {} matches the pin",
        copy.display()
    );

    let (code, out) = f.run_offline(&[&"--logger-zip", &copy]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&ok_line), "{out}");
    let (code, out) = f.run(&[&"--verify-only"], &[(LOGGER_ZIP_ENV, &copy)]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&ok_line), "{out}");
    assert!(
        !f.logger().exists(),
        "the package was copied into the cache"
    );

    // A supplied copy that does not match is an error.
    fs::write(&copy, &good[1..]).unwrap();
    let (code, out) = f.run_offline(&[&"--logger-zip", &copy]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("manifest pins"), "{out}");

    // So is a supplied path that does not exist (it is never "absent").
    let missing = private.join("nope.zip");
    let (code, out) = f.run(&[&"--verify-only", &"--logger-zip", &missing], &[]);
    assert_ne!(code, 0, "{out}");
    assert!(
        out.contains(&format!("{}: no such file", missing.display())),
        "{out}"
    );

    // And one inside the repository: the package must never sit where it
    // could be committed.
    let inside = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/logger_v370.zip");
    let (code, out) = f.run(&[&"--verify-only"], &[(LOGGER_ZIP_ENV, &inside)]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("outside the repository"), "{out}");
    fs::remove_dir_all(&f.base).unwrap();
}

#[test]
fn replacing_a_bad_rom_cache_keeps_the_definitions_and_the_logger_copy() {
    let f = fetched_with_defs("defs-kept");
    fs::write(f.logger(), logger_bytes()).unwrap();
    let mut a = fs::read(f.cache.join(A)).unwrap();
    a[0] ^= 1;
    fs::write(f.cache.join(A), &a).unwrap();

    // The definitions source is gone: they must not need refetching.
    let (code, out) = f.run(
        &[
            &"--source",
            &f.roms.repo,
            &"--defs-source",
            &f.base.join("gone"),
        ],
        &[],
    );
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("did not verify (1 problem(s)) and was replaced"),
        "{out}"
    );
    assert!(out.contains("fetch-corpus: ok: fetched "), "{out}");
    assert!(
        out.contains("fetch-corpus: defs: ok: 4/4 definition file(s), ")
            && out.contains("; nothing fetched"),
        "{out}"
    );
    assert!(out.contains("fetch-corpus: logger v370: ok: "), "{out}");
    assert_roms_intact(&f.cache);
    assert_is_defs(&f.defs_tree());
    assert_eq!(fs::read(f.logger()).unwrap(), logger_bytes());
    assert_no_leftovers(&f.base.join("cache"), &["corpus"]);
    fs::remove_dir_all(&f.base).unwrap();
}

#[test]
fn a_corpus_directory_holding_only_the_logger_copy_is_adopted() {
    // Ryan drops the hand-downloaded package into RR_CORPUS_DIR before the
    // first fetch (D-028): that directory is not "someone else's".
    let base = scratch("defs-adopt");
    let roms = make_source(&base);
    let src = make_defs_source(&base);
    let manifest = manifest_with_defs(&base, &roms, &src.defs);
    let cache = base.join("corpus");
    fs::create_dir_all(cache.join("defs")).unwrap();
    fs::write(cache.join("defs").join(LOGGER_FILE), logger_bytes()).unwrap();
    let (code, out) = xtask(
        &[&"--manifest", &manifest],
        &[
            (DIR_ENV, &cache),
            (SOURCE_ENV, &roms.repo),
            (DEFS_SOURCE_ENV, &src.repo),
        ],
    );
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("fetch-corpus: ok: fetched "), "{out}");
    assert!(out.contains("fetch-corpus: defs: ok: fetched "), "{out}");
    assert!(out.contains("fetch-corpus: logger v370: ok: "), "{out}");
    assert_roms_intact(&cache);
    assert_is_defs(&cache.join("defs").join("romraider"));
    fs::remove_dir_all(&base).unwrap();
}

/// US-RR-16-2: a definitions cache with one byte flipped (same size) that
/// cannot be refetched (its source is gone) fails the fetch, naming the
/// file and leaving the cache as it was; the ROMs still verify.
#[test]
fn a_bad_definitions_cache_that_cannot_be_refetched_is_named_and_kept() {
    let f = fetched_with_defs("defs-bad-offline");
    let tree = f.defs_tree();
    let mut spaced = fs::read(tree.join(DEFS_SPACED)).unwrap();
    spaced[5] ^= 0x01;
    fs::write(tree.join(DEFS_SPACED), &spaced).unwrap();
    let before = listing(&tree);

    let (code, out) = f.run_offline(&[]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("fetch-corpus: ok: 4/4 image(s), "), "{out}");
    assert!(
        out.contains(&format!(
            "{} did not verify (1 problem(s)) and could not be refetched from ",
            tree.display()
        )),
        "{out}"
    );
    assert!(out.contains(&format!("  {DEFS_SPACED}: sha256 ")), "{out}");
    assert_eq!(listing(&tree), before);
    assert_eq!(fs::read(tree.join(DEFS_SPACED)).unwrap(), spaced);
    assert_no_leftovers(&f.cache.join("defs"), &["romraider"]);
    fs::remove_dir_all(&f.base).unwrap();
}
