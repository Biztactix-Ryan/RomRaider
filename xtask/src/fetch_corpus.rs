//! `cargo xtask fetch-corpus [--source <URL|PATH>] [--dir <DIR>] [--manifest
//! <FILE>] [--verify-only]` (US-RR-11-5, D-025): download the external
//! stock-ROM corpus into `RR_CORPUS_DIR` and verify every image against
//! `fixtures/corpus-manifest.toml` ([`crate::corpus_manifest`]).
//!
//! # Where the corpus goes
//!
//! `--dir`, else `RR_CORPUS_DIR`, else the per-user cache directory
//! ([`default_dir`]): `%LOCALAPPDATA%\romraider-rs\corpus` on Windows,
//! otherwise `$XDG_CACHE_HOME/romraider-rs/corpus`, falling back to
//! `~/.cache/romraider-rs/corpus`. (The workspace has no `directories`
//! crate; this is the same layout, minus its macOS `~/Library/Caches`.)
//! Images sit at their manifest paths directly under that directory, next to
//! the [`STAMP_FILE`] that marks it as a cache this command manages.
//!
//! The directory must be outside the repository working tree, and must not
//! contain it: ROM bytes are never written inside the repo (D-025). The
//! command refuses such a directory before touching anything, and checks the
//! work directory it writes to as well. The check ([`check_outside_repo`],
//! US-RR-11-9) is [`crate::corpus_location`], the same source file
//! rr-testkit's corpus loader uses: it resolves symlinks and `..`, judges a
//! not-yet-existing directory by its nearest existing ancestor, ignores case
//! on Windows and macOS, and protects the git working tree around the
//! workspace and every other worktree of the same repository (but not
//! unrelated git repositories).
//!
//! # Where it comes from
//!
//! `--source`, else `RR_CORPUS_SOURCE`, else the manifest's
//! `[roms] upstream` (public `samuelw9999/RomRaider`, so CI needs no
//! secrets), always at the manifest's pinned `commit`. A source is anything
//! `git fetch` accepts: an https URL, a `file://` URL or a local repository
//! path (a bare mirror such as `/home/ryan/Repo/romraider-stock-roms/mirror.git`).
//! The source is only ever read: one shallow fetch, no credentials, no
//! prompts (`GIT_TERMINAL_PROMPT=0`).
//!
//! # How
//!
//! 1. If the directory exists, verify it ([`verify`]). A complete, verified
//!    cache is used as is: no network I/O at all.
//! 2. Otherwise, in a work directory next to the target (inside its parent,
//!    so the final rename stays on one filesystem; never inside the repo):
//!    `git init --bare`, `git fetch --depth 1 <source> <commit>`, then list
//!    the commit's tree (`git ls-tree -r -l`) and stream each manifest
//!    entry's blob out with `git cat-file --batch`. There is no checkout, so
//!    no `.gitattributes` (upstream has `* text=auto`), `core.autocrlf` or
//!    filter can convert a byte: files are the raw blobs.
//! 3. Every manifest entry that is a blob in that tree is written into the
//!    work directory, whatever its size, and the new tree is verified in
//!    full (size and SHA-256), then swapped into place (old cache moved
//!    aside, new one renamed in, old one deleted). Any mismatch (missing, not
//!    a regular file, size, hash, in any combination) fails with every bad
//!    path listed in one report (US-RR-11-1), removes the work directory and
//!    leaves the existing cache untouched: nothing a later `--verify-only`
//!    could accept is left behind.
//!
//! A cache that fails verification (missing, truncated, corrupted or extra
//! files) is never used: `--verify-only` reports it and fails, a plain run
//! reports it and replaces it with a fresh, verified fetch. An existing,
//! non-empty directory without [`STAMP_FILE`] is not ours, so it is never
//! replaced or deleted. Runs against the same directory must not overlap.
//!
//! # ECU definitions (D-028, US-RR-16-5)
//!
//! When the manifest has a `[defs]` table (the committed one does; the
//! synthetic manifests tests write need not), the same run then makes
//! [`defs_dir`], `<corpus>/defs/romraider/`, a verified copy of the pinned
//! definition subtree, exactly as above: from `--defs-source`, else
//! [`DEFS_SOURCE_ENV`], else `[defs] upstream` (public GitHub, so CI needs
//! no secrets), at `[defs] commit`, taking only the blobs under
//! `[defs] path_prefix` (paths with spaces included) and checking every
//! file's size and SHA-256, with its own [`DEFS_STAMP_FILE`]. The local
//! mirror `/home/ryan/Repo/romraider-ecu-defs/mirror.git` is a valid
//! source. The top-level `defs/` directory is not part of the ROM tree: ROM
//! verification ignores it, a directory holding only it counts as empty,
//! and replacing a bad ROM cache carries it over unchanged.
//!
//! Then the logger package `[defs.logger_v370]` ([`check_logger`]). It is
//! never downloaded (D-028). The copy checked is `--logger-zip`, else
//! [`LOGGER_ZIP_ENV`] (a path into Ryan's private mirror), else
//! [`logger_path`], `<corpus>/defs/logger_v370.zip`; it is only read, never
//! copied. No copy there is not an error: the run says so and carries on
//! (the public tier uses the v290 logger files in the pinned tree, and
//! v370-only checks skip). A copy of the wrong size or SHA-256 is an error,
//! as is a `--logger-zip` / [`LOGGER_ZIP_ENV`] path that does not exist or
//! lies inside the repository.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context as _, Result, bail};

use crate::corpus_location;
use crate::corpus_manifest::{DEFS_DIR, DefsCorpus, LoggerPackage, Manifest, hash_file};

/// Environment variable naming the corpus directory when `--dir` is not given.
pub use crate::corpus_location::DIR_ENV;

/// The default cache directory below the per-user cache root.
pub use crate::corpus_location::CACHE_SUBDIR;

/// Environment variable naming the source when `--source` is not given.
pub const SOURCE_ENV: &str = "RR_CORPUS_SOURCE";

/// Environment variable naming the definitions source when `--defs-source`
/// is not given.
pub const DEFS_SOURCE_ENV: &str = "RR_DEFS_SOURCE";

/// Environment variable naming a copy of the logger package (in a private
/// mirror) when `--logger-zip` is not given.
pub const LOGGER_ZIP_ENV: &str = "RR_LOGGER_ZIP";

/// The marker file at the top of a cache this command manages. It is not a
/// corpus image and is not reported as an extra file.
pub const STAMP_FILE: &str = ".rr-corpus-cache";

/// The marker file at the top of the definitions tree ([`defs_dir`]).
pub const DEFS_STAMP_FILE: &str = ".rr-defs-cache";

/// The definitions tree's directory inside the cache's `defs/`.
pub const DEFS_TREE: &str = "romraider";

/// The default corpus directory (module docs), from the environment `env`
/// ([`crate::corpus_location::default_dir`], shared with rr-testkit's loader).
///
/// # Errors
///
/// None of the variables it needs is set.
pub fn default_dir(env: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf> {
    corpus_location::default_dir(env).with_context(|| {
        format!(
            "no corpus directory: pass --dir <DIR> or set {DIR_ENV} (no {} to default from)",
            corpus_location::DEFAULT_DIR_VARS
        )
    })
}

/// The corpus directory: `--dir`, else [`DIR_ENV`], else [`default_dir`].
///
/// # Errors
///
/// No default can be worked out.
pub fn corpus_dir(arg: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(dir) = arg {
        return Ok(dir);
    }
    match std::env::var_os(DIR_ENV) {
        Some(v) if !v.is_empty() => Ok(PathBuf::from(v)),
        _ => default_dir(|k| std::env::var_os(k)),
    }
}

/// The source: `--source`, else [`SOURCE_ENV`], else the manifest's upstream.
#[must_use]
pub fn source(arg: Option<String>, manifest: &Manifest) -> String {
    arg.filter(|s| !s.is_empty())
        .or_else(|| std::env::var(SOURCE_ENV).ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| manifest.roms.upstream.clone())
}

/// The definitions source: `--defs-source`, else [`DEFS_SOURCE_ENV`], else
/// `[defs] upstream`.
#[must_use]
pub fn defs_source(arg: Option<String>, defs: &DefsCorpus) -> String {
    arg.filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var(DEFS_SOURCE_ENV)
                .ok()
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| defs.upstream.clone())
}

/// The supplied copy of the logger package: `--logger-zip`, else
/// [`LOGGER_ZIP_ENV`]; `None` when neither is set.
#[must_use]
pub fn logger_source(arg: Option<PathBuf>) -> Option<PathBuf> {
    arg.filter(|p| !p.as_os_str().is_empty()).or_else(|| {
        std::env::var_os(LOGGER_ZIP_ENV)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    })
}

/// The definitions tree inside the corpus directory `corpus`:
/// `<corpus>/defs/romraider`.
#[must_use]
pub fn defs_dir(corpus: &Path) -> PathBuf {
    corpus.join(DEFS_DIR).join(DEFS_TREE)
}

/// Where a local copy of the logger package is looked for when none is
/// supplied: `<corpus>/defs/<file>`.
#[must_use]
pub fn logger_path(corpus: &Path, logger: &LoggerPackage) -> PathBuf {
    corpus.join(DEFS_DIR).join(&logger.file)
}

/// `path` made absolute and resolved through every existing ancestor
/// ([`corpus_location::resolve`]).
fn resolve(path: &Path) -> Result<PathBuf> {
    Ok(corpus_location::resolve(path)?)
}

/// Fails unless `dir` is outside every working tree of the repository at
/// `repo` and contains none of them (D-025: no ROM bytes inside the working
/// tree). `repo` is the workspace root; the git working tree around it and
/// the repository's other worktrees are protected too
/// ([`corpus_location`] docs: symlinks, `..`, relative and not-yet-existing
/// paths, case-insensitive file systems).
///
/// # Errors
///
/// `dir` is inside such a tree, is one, or contains one.
pub fn check_outside_repo(dir: &Path, repo: &Path) -> Result<()> {
    Ok(corpus_location::check_outside_repo(dir, repo)?)
}

/// One way a corpus tree disagrees with the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// A manifest entry is not there.
    Missing {
        /// Manifest path.
        path: String,
    },
    /// A manifest entry has the wrong size.
    Size {
        /// Manifest path.
        path: String,
        /// Size the manifest gives.
        want: u64,
        /// Size found.
        got: u64,
    },
    /// A manifest entry has the right size but the wrong SHA-256.
    Hash {
        /// Manifest path.
        path: String,
        /// Hash the manifest gives.
        want: String,
        /// Hash found.
        got: String,
    },
    /// Something at a manifest path that is not a regular file (a
    /// directory, a symlink, a submodule).
    NotAFile {
        /// Manifest path.
        path: String,
    },
    /// A file that is not in the manifest.
    Extra {
        /// Path relative to the corpus directory.
        path: String,
    },
}

impl Problem {
    /// The path the problem is about.
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::Missing { path }
            | Self::Size { path, .. }
            | Self::Hash { path, .. }
            | Self::NotAFile { path }
            | Self::Extra { path } => path,
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { path } => write!(f, "{path}: missing"),
            Self::Size { path, want, got } => {
                write!(f, "{path}: size {got} bytes, manifest says {want}")
            }
            Self::Hash { path, want, got } => {
                write!(f, "{path}: sha256 {got}, manifest says {want}")
            }
            Self::NotAFile { path } => write!(f, "{path}: not a regular file"),
            Self::Extra { path } => write!(f, "{path}: not in the manifest (extra file)"),
        }
    }
}

/// The result of [`verify`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Verified {
    /// Manifest entries present with the right size and hash.
    pub ok: usize,
    /// Their total size in bytes.
    pub bytes: u64,
    /// Every disagreement, manifest entries first (in manifest order), then
    /// extra files (sorted).
    pub problems: Vec<Problem>,
}

/// One file a pinned tree must hold.
#[derive(Debug, Clone, Copy)]
struct Want<'a> {
    path: &'a str,
    size: u64,
    sha256: &'a str,
}

/// A pinned upstream tree this command keeps a verified copy of: the
/// stock-ROM corpus or the definition corpus.
#[derive(Debug, Clone)]
struct Pinned<'a> {
    commit: &'a str,
    path_prefix: &'a str,
    files: Vec<Want<'a>>,
    /// Its marker file, at the top of the copy.
    stamp: &'static str,
    /// The comment lines written into the marker file before `commit = ...`.
    stamp_comment: &'static str,
    /// Top-level names inside the copy that are not part of it: ignored by
    /// [`verify_pinned`], allowed in a directory [`is_ours`] adopts, and
    /// carried over when the copy is replaced.
    reserved: &'static [&'static str],
}

impl<'a> Pinned<'a> {
    fn roms(manifest: &'a Manifest) -> Self {
        let roms = &manifest.roms;
        Self {
            commit: &roms.commit,
            path_prefix: &roms.path_prefix,
            files: roms
                .files
                .iter()
                .map(|f| Want {
                    path: &f.path,
                    size: f.size,
                    sha256: &f.sha256,
                })
                .collect(),
            stamp: STAMP_FILE,
            stamp_comment: "# Stock-ROM corpus cache managed by `cargo xtask fetch-corpus` (D-025).\n\
                            # It may replace this whole directory (carrying `defs/` over); keep nothing else here.\n",
            reserved: &[DEFS_DIR],
        }
    }

    fn defs(defs: &'a DefsCorpus) -> Self {
        Self {
            commit: &defs.commit,
            path_prefix: &defs.path_prefix,
            files: defs
                .files
                .iter()
                .map(|f| Want {
                    path: &f.path,
                    size: f.size,
                    sha256: &f.sha256,
                })
                .collect(),
            stamp: DEFS_STAMP_FILE,
            stamp_comment: "# ECU-definition corpus cache managed by `cargo xtask fetch-corpus` (D-028).\n\
                            # It may replace this whole directory; keep nothing else here.\n",
            reserved: &[],
        }
    }

    fn upstream_path(&self, w: &Want<'_>) -> String {
        format!("{}{}", self.path_prefix, w.path)
    }
}

/// Checks the corpus tree at `dir` against `manifest`: every entry present
/// as a regular file with exactly the manifest size and SHA-256, and nothing
/// else there except [`STAMP_FILE`] and the definitions' `defs/` at the top
/// (and empty directories). Reads only; hashes on several threads.
///
/// # Errors
///
/// `dir` is not a readable directory, or a file cannot be read.
pub fn verify(dir: &Path, manifest: &Manifest) -> Result<Verified> {
    verify_pinned(dir, &Pinned::roms(manifest))
}

/// Checks the definitions tree of the corpus directory `corpus`
/// ([`defs_dir`]) against `defs` the way [`verify`] checks the ROMs, with
/// [`DEFS_STAMP_FILE`] as its marker. Reads only.
///
/// # Errors
///
/// The definitions tree is not a readable directory, or a file cannot be
/// read.
pub fn verify_defs(corpus: &Path, defs: &DefsCorpus) -> Result<Verified> {
    verify_pinned(&defs_dir(corpus), &Pinned::defs(defs))
}

fn verify_pinned(dir: &Path, spec: &Pinned<'_>) -> Result<Verified> {
    if !dir.is_dir() {
        bail!("{}: not a directory", dir.display());
    }
    let files = &spec.files;
    let mut problems: Vec<Option<Problem>> = vec![None; files.len()];
    let mut to_hash = Vec::new();
    for (i, f) in files.iter().enumerate() {
        let path = dir.join(f.path);
        let found = match fs::symlink_metadata(&path) {
            Ok(meta) if !meta.is_file() => Some(Problem::NotAFile {
                path: f.path.to_owned(),
            }),
            Ok(meta) if meta.len() != f.size => Some(Problem::Size {
                path: f.path.to_owned(),
                want: f.size,
                got: meta.len(),
            }),
            Ok(_) => {
                to_hash.push(i);
                None
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Some(Problem::Missing {
                path: f.path.to_owned(),
            }),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        if let Some(slot) = problems.get_mut(i) {
            *slot = found;
        }
    }

    for (i, (size, sha256)) in hash_all(dir, files, &to_hash)? {
        if let (Some(f), Some(slot)) = (files.get(i), problems.get_mut(i))
            && (size != f.size || sha256 != f.sha256)
        {
            *slot = Some(if size == f.size {
                Problem::Hash {
                    path: f.path.to_owned(),
                    want: f.sha256.to_owned(),
                    got: sha256,
                }
            } else {
                Problem::Size {
                    path: f.path.to_owned(),
                    want: f.size,
                    got: size,
                }
            });
        }
    }

    let mut out = Verified::default();
    for (f, p) in files.iter().zip(&problems) {
        if p.is_none() {
            out.ok += 1;
            out.bytes += f.size;
        }
    }
    out.problems = problems.into_iter().flatten().collect();
    let known: BTreeSet<&str> = files.iter().map(|f| f.path).collect();
    let mut extra = Vec::new();
    find_extra(dir, "", &known, spec, &mut extra)?;
    extra.sort();
    out.problems
        .extend(extra.into_iter().map(|path| Problem::Extra { path }));
    Ok(out)
}

/// Size and SHA-256 of `files[i]` under `dir` for each `i` in `which`,
/// hashed on up to eight threads.
fn hash_all(
    dir: &Path,
    files: &[Want<'_>],
    which: &[usize],
) -> Result<Vec<(usize, (u64, String))>> {
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .clamp(1, 8)
        .min(which.len().max(1));
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::with_capacity(which.len()));
    let failure: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let Some(&i) = which.get(next.fetch_add(1, Ordering::Relaxed)) else {
                        return;
                    };
                    let Some(f) = files.get(i) else { return };
                    match hash_file(&dir.join(f.path)) {
                        Ok(r) => {
                            if let Ok(mut v) = results.lock() {
                                v.push((i, r));
                            }
                        }
                        Err(e) => {
                            if let Ok(mut slot) = failure.lock() {
                                slot.get_or_insert(e);
                            }
                            return;
                        }
                    }
                }
            });
        }
    });
    if let Some(e) = failure.into_inner().ok().flatten() {
        return Err(e);
    }
    let results = results
        .into_inner()
        .map_err(|_| anyhow::anyhow!("a hashing thread panicked"))?;
    if results.len() != which.len() {
        bail!("hashing stopped early");
    }
    Ok(results)
}

fn find_extra(
    dir: &Path,
    rel: &str,
    known: &BTreeSet<&str>,
    spec: &Pinned<'_>,
    out: &mut Vec<String>,
) -> Result<()> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        entries.push(entry.with_context(|| format!("reading {}", dir.display()))?);
    }
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if rel.is_empty() && (name == spec.stamp || spec.reserved.contains(&name.as_str())) {
            continue;
        }
        let rel_path = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        let kind = entry
            .file_type()
            .with_context(|| format!("reading {}", entry.path().display()))?;
        if kind.is_dir() {
            find_extra(&entry.path(), &rel_path, known, spec, out)?;
        } else if !known.contains(rel_path.as_str()) {
            out.push(rel_path);
        }
    }
    Ok(())
}

/// What [`fetch`] did.
#[derive(Debug)]
pub enum Outcome {
    /// The cache was already complete and verified; nothing was fetched.
    AlreadyVerified(Verified),
    /// The corpus was fetched, verified and moved into place.
    Fetched {
        /// The new cache's verification (no problems).
        verified: Verified,
        /// What was wrong with the cache it replaced; empty when there was
        /// none.
        replaced: Vec<Problem>,
        /// Files in the source tree that are not in the manifest and were not
        /// extracted (`README.md`, `.gitattributes`).
        skipped: usize,
    },
}

/// Makes `dir` a complete, verified copy of the corpus `manifest` describes,
/// fetching from `source` only when it is not one already (module docs).
/// The definitions are [`fetch_defs`]'s job; a `defs/` already in `dir` is
/// kept as it is.
///
/// # Errors
///
/// `dir` is inside (or contains) `repo`, or is a non-empty directory this
/// command does not manage; git fails; or the source at the pinned commit
/// does not match the manifest (every bad path listed). On error an
/// existing cache is left as it was.
pub fn fetch(dir: &Path, manifest: &Manifest, source: &str, repo: &Path) -> Result<Outcome> {
    check_outside_repo(dir, repo)?;
    manifest.validate()?;
    fetch_pinned(dir, &Pinned::roms(manifest), source, repo)
}

/// Makes [`defs_dir`] of the corpus directory `corpus` a complete, verified
/// copy of the definition corpus `defs` describes, fetching from `source`
/// only when it is not one already, exactly as [`fetch`] does for the ROMs
/// (module docs). The logger package is [`check_logger`]'s job.
///
/// # Errors
///
/// As [`fetch`]: the directory is inside (or contains) `repo`, or is a
/// non-empty directory this command does not manage; `defs` is invalid; git
/// fails; or the source at the pinned commit does not match `defs` (every
/// bad path listed). On error an existing copy is left as it was.
pub fn fetch_defs(corpus: &Path, defs: &DefsCorpus, source: &str, repo: &Path) -> Result<Outcome> {
    let dir = defs_dir(corpus);
    check_outside_repo(corpus, repo)?;
    check_outside_repo(&dir, repo)?;
    defs.validate()?;
    fetch_pinned(&dir, &Pinned::defs(defs), source, repo)
}

fn fetch_pinned(dir: &Path, spec: &Pinned<'_>, source: &str, repo: &Path) -> Result<Outcome> {
    let mut replaced = Vec::new();
    let exists = match fs::symlink_metadata(dir) {
        Ok(m) if m.is_dir() => true,
        Ok(_) => bail!("{}: exists and is not a directory", dir.display()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    if exists {
        let v = verify_pinned(dir, spec)?;
        if v.problems.is_empty() {
            return Ok(Outcome::AlreadyVerified(v));
        }
        if !is_ours(dir, spec)? {
            let mut msg = format!(
                "{} is not a corpus cache made by fetch-corpus (no {}) and does not verify ({} problem(s)); refusing to replace it. Point {DIR_ENV} or --dir at a new or empty directory",
                dir.display(),
                spec.stamp,
                v.problems.len()
            );
            for p in v.problems.iter().take(20) {
                msg.push_str(&format!("\n  {p}"));
            }
            bail!(msg);
        }
        replaced = v.problems;
    }

    let name = dir
        .file_name()
        .with_context(|| format!("{}: no directory name", dir.display()))?
        .to_string_lossy()
        .into_owned();
    let parent = match dir.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    // The work directory (and the old cache moved into it) must be outside
    // the repository too; it is a sibling of `dir`, so this holds whenever
    // `dir`'s does, but it is checked rather than assumed.
    check_outside_repo(&Work::path_for(&parent, &name), repo)?;
    fs::create_dir_all(&parent).with_context(|| format!("creating {}", parent.display()))?;
    let work = Work::create(&parent, &name)?;
    let tree = work.path.join("corpus");
    // A cache that did not verify and cannot be refetched is still named,
    // with every bad path, rather than lost behind the git error.
    let (skipped, absent) =
        extract(&work.path.join("git"), &tree, spec, source).with_context(|| {
            if replaced.is_empty() {
                format!("fetching {} from {source}", dir.display())
            } else {
                listing(
                    &format!(
                        "{} did not verify ({} problem(s)) and could not be refetched from {source}; it was left as it was",
                        dir.display(),
                        replaced.len()
                    ),
                    &replaced,
                )
            }
        })?;

    let mut verified = verify_pinned(&tree, spec)?;
    // An entry the source does not have as a blob was not extracted, so
    // `verify` sees it as missing; say what the source actually had there.
    for p in &mut verified.problems {
        if let Problem::Missing { path } = p
            && let Some(why) = absent.get(path.as_str())
        {
            *p = why.clone();
        }
    }
    if !verified.problems.is_empty() {
        bail!(listing(
            &format!(
                "{source} at {} does not match the manifest ({} problem(s)); the existing cache was not changed",
                spec.commit,
                verified.problems.len()
            ),
            &verified.problems
        ));
    }

    let old = work.path.join("old");
    if exists {
        fs::rename(dir, &old)
            .with_context(|| format!("moving the old cache {} aside", dir.display()))?;
        // The reserved entries (the ROM cache's `defs/`) are not part of
        // this tree: they move into the new one unchanged.
        if let Err(e) = carry(&old, &tree, spec.reserved) {
            let _ = carry(&tree, &old, spec.reserved);
            let _ = fs::rename(&old, dir);
            return Err(e).with_context(|| {
                format!(
                    "carrying {} over to the new cache",
                    spec.reserved.join(", ")
                )
            });
        }
    }
    if let Err(e) = fs::rename(&tree, dir) {
        if exists {
            let _ = carry(&tree, &old, spec.reserved);
            let _ = fs::rename(&old, dir);
        }
        return Err(e).with_context(|| format!("moving the new cache into {}", dir.display()));
    }
    drop(work);
    Ok(Outcome::Fetched {
        verified,
        replaced,
        skipped,
    })
}

/// Moves each of `names` that exists in `from` into `to`.
fn carry(from: &Path, to: &Path, names: &[&str]) -> Result<()> {
    for name in names {
        let src = from.join(name);
        if fs::symlink_metadata(&src).is_ok() {
            fs::rename(&src, to.join(name))
                .with_context(|| format!("moving {} to {}", src.display(), to.display()))?;
        }
    }
    Ok(())
}

/// What [`check_logger`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoggerStatus {
    /// No copy was supplied and there is none at `looked`. Not an error
    /// (D-028): checks that need v370 skip.
    Absent {
        /// Where a copy was looked for ([`logger_path`]).
        looked: PathBuf,
    },
    /// The copy at `path` has the pinned size and SHA-256.
    Verified {
        /// The copy checked.
        path: PathBuf,
    },
}

/// Checks the logger package `logger` (module docs): the `supplied` copy if
/// there is one, else [`logger_path`] in the corpus directory `corpus`.
/// Reads only; never downloads or copies anything.
///
/// # Errors
///
/// The copy has the wrong size or SHA-256, is not a regular file, or cannot
/// be read; or `supplied` does not exist or is inside `repo`.
pub fn check_logger(
    corpus: &Path,
    logger: &LoggerPackage,
    supplied: Option<&Path>,
    repo: &Path,
) -> Result<LoggerStatus> {
    let path = match supplied {
        Some(p) => {
            check_outside_repo(p, repo).with_context(|| {
                format!(
                    "{}: the logger package must be kept outside the repository (D-028)",
                    p.display()
                )
            })?;
            if let Err(e) = fs::metadata(p) {
                if e.kind() == io::ErrorKind::NotFound {
                    bail!(
                        "{}: no such file (given by --logger-zip or {LOGGER_ZIP_ENV})",
                        p.display()
                    );
                }
                return Err(e).with_context(|| format!("reading {}", p.display()));
            }
            p.to_path_buf()
        }
        None => {
            let p = logger_path(corpus, logger);
            match fs::symlink_metadata(&p) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    return Ok(LoggerStatus::Absent { looked: p });
                }
                Err(e) => return Err(e).with_context(|| format!("reading {}", p.display())),
                Ok(_) => p,
            }
        }
    };
    let meta = fs::metadata(&path).with_context(|| format!("reading {}", path.display()))?;
    if !meta.is_file() {
        bail!(
            "{}: not a regular file (expected {})",
            path.display(),
            logger.file
        );
    }
    let (size, sha256) = hash_file(&path)?;
    if size != logger.size {
        bail!(
            "{}: size {size} bytes, manifest pins {} bytes for {}: not the pinned package (D-028)",
            path.display(),
            logger.size,
            logger.file
        );
    }
    if sha256 != logger.sha256 {
        bail!(
            "{}: sha256 {sha256}, manifest pins {} for {}: not the pinned package (D-028)",
            path.display(),
            logger.sha256,
            logger.file
        );
    }
    Ok(LoggerStatus::Verified { path })
}

/// `heading` and every problem, one per line.
#[must_use]
pub fn listing(heading: &str, problems: &[Problem]) -> String {
    let mut msg = heading.to_owned();
    for p in problems {
        msg.push_str(&format!("\n  {p}"));
    }
    msg
}

/// An empty directory, one holding the spec's marker file, or one holding
/// nothing but its reserved entries.
fn is_ours(dir: &Path, spec: &Pinned<'_>) -> Result<bool> {
    if dir.join(spec.stamp).is_file() {
        return Ok(true);
    }
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        if !spec
            .reserved
            .contains(&entry.file_name().to_string_lossy().as_ref())
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The work directory `<parent>/.<name>.fetch-<pid>`, removed on drop.
struct Work {
    path: PathBuf,
}

impl Work {
    fn prefix(name: &str) -> String {
        format!(".{name}.fetch-")
    }

    fn path_for(parent: &Path, name: &str) -> PathBuf {
        parent.join(format!("{}{}", Self::prefix(name), std::process::id()))
    }

    fn create(parent: &Path, name: &str) -> Result<Self> {
        let prefix = Self::prefix(name);
        // Leftovers of interrupted runs (runs must not overlap, module docs).
        if let Ok(entries) = fs::read_dir(parent) {
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().starts_with(&prefix) {
                    let _ = fs::remove_dir_all(e.path());
                }
            }
        }
        let path = Self::path_for(parent, name);
        fs::create_dir(&path).with_context(|| format!("creating {}", path.display()))?;
        Ok(Self { path })
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// `git` with no prompts and nothing from the environment that could point
/// it at another repository (such as this one, when run from a git hook:
/// D-025, the fetched objects must stay in the work directory).
fn bare_git() -> Command {
    let mut cmd = Command::new("git");
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES");
    cmd
}

/// [`bare_git`] on the repository at `git_dir`.
fn git(git_dir: &Path) -> Command {
    let mut cmd = bare_git();
    cmd.arg("--git-dir").arg(git_dir);
    cmd
}

fn run(mut cmd: Command, what: &str) -> Result<Vec<u8>> {
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("cannot run git ({what})"))?;
    if !out.status.success() {
        bail!(
            "{what} failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out.stdout)
}

/// A [`resolve`]d local path as git takes it for a repository. On Windows
/// the path is canonical, so verbatim (`\\?\C:\...`), which git reads as a
/// UNC path to host `?`; the plain form names the same directory.
fn git_path(path: PathBuf) -> OsString {
    #[cfg(windows)]
    if let Some(s) = path.to_str() {
        if let Some(unc) = s.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{unc}").into();
        }
        if let Some(disk) = s.strip_prefix(r"\\?\") {
            return disk.into();
        }
    }
    path.into_os_string()
}

/// One blob in a git tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TreeBlob {
    pub(crate) oid: String,
    pub(crate) size: u64,
}

/// Every path in `commit`'s tree in the repository at `git_dir`
/// (`git ls-tree -r -l -z --full-tree`), mapped to its blob, or to `None`
/// for anything that is not a regular-file blob (a symlink or a
/// submodule). Paths are git's, byte for byte (spaces included).
///
/// # Errors
///
/// git fails (the commit is not there), or the listing does not parse.
pub(crate) fn list_tree(
    git_dir: &Path,
    commit: &str,
) -> Result<BTreeMap<String, Option<TreeBlob>>> {
    let mut ls = git(git_dir);
    ls.args(["ls-tree", "-r", "-l", "-z", "--full-tree", commit]);
    let listing_bytes = run(ls, &format!("git ls-tree {commit}"))?;
    Ok(parse_ls_tree(&listing_bytes)?
        .into_iter()
        .map(|(path, blob)| (path.to_owned(), blob))
        .collect())
}

/// Fetches `spec`'s commit from `source` into a new bare repository at
/// `git_dir` and writes the raw blob of every entry the commit has as a
/// blob under `tree`, whatever its size ([`verify`] reports a wrong size or
/// hash). Returns how many tree files were not in the manifest, and the
/// problem for each manifest path that is not a blob there (missing, or a
/// symlink or submodule), keyed by manifest path.
fn extract(
    git_dir: &Path,
    tree: &Path,
    spec: &Pinned<'_>,
    source: &str,
) -> Result<(usize, BTreeMap<String, Problem>)> {
    let commit = spec.commit;
    // A local path is resolved here: git runs with another directory as its
    // repository, and the source may be relative to ours.
    let source_arg: OsString = if Path::new(source).exists() {
        git_path(resolve(Path::new(source))?)
    } else {
        source.into()
    };

    let mut init = bare_git();
    init.args(["init", "--quiet", "--bare"]).arg(git_dir);
    run(init, "git init")?;

    let mut fetch = git(git_dir);
    fetch
        .args(["fetch", "--quiet", "--depth", "1", "--no-tags", "--"])
        .arg(&source_arg)
        .arg(commit);
    run(fetch, &format!("git fetch {source} {commit}"))?;

    let entries = list_tree(git_dir, commit)?;

    let mut absent = BTreeMap::new();
    let mut wanted = Vec::new();
    for f in &spec.files {
        let upstream = spec.upstream_path(f);
        let path = f.path.to_owned();
        match entries.get(upstream.as_str()) {
            None => {
                absent.insert(path.clone(), Problem::Missing { path });
            }
            Some(None) => {
                absent.insert(path.clone(), Problem::NotAFile { path });
            }
            Some(Some(b)) => wanted.push((f.path, b)),
        }
    }
    let skipped = entries.len() - wanted.len();

    fs::create_dir(tree).with_context(|| format!("creating {}", tree.display()))?;
    stream_blobs(git_dir, &wanted, |i, blob| {
        let Some((rel, _)) = wanted.get(i) else {
            bail!("git cat-file: unexpected blob {i}");
        };
        let path = tree.join(rel);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let mut out =
            fs::File::create(&path).with_context(|| format!("creating {}", path.display()))?;
        io::copy(blob, &mut out).with_context(|| format!("writing {}", path.display()))?;
        out.sync_all()
            .with_context(|| format!("writing {}", path.display()))
    })?;

    fs::write(
        tree.join(spec.stamp),
        format!("{}commit = \"{commit}\"\n", spec.stamp_comment),
    )
    .with_context(|| format!("writing {}", tree.join(spec.stamp).display()))?;
    Ok((skipped, absent))
}

/// Streams the raw bytes of each of `blobs` (a label for messages, and the
/// blob) out of the repository at `git_dir` with one `git cat-file
/// --batch`, in order, calling `each(i, reader)` with a reader over exactly
/// blob `i`'s bytes. Whatever `each` leaves unread is read and discarded;
/// a short blob or an unexpected header is an error.
///
/// # Errors
///
/// git fails, its output is not what was asked for, or `each` fails.
pub(crate) fn stream_blobs(
    git_dir: &Path,
    blobs: &[(&str, &TreeBlob)],
    mut each: impl FnMut(usize, &mut dyn Read) -> Result<()>,
) -> Result<()> {
    let mut cat = git(git_dir);
    cat.args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cat.spawn().context("cannot run git cat-file")?;
    let mut stdin = child.stdin.take().context("git cat-file: no stdin")?;
    let stdout = child.stdout.take().context("git cat-file: no stdout")?;
    let request: String = blobs.iter().map(|(_, b)| format!("{}\n", b.oid)).collect();
    let writer = std::thread::spawn(move || stdin.write_all(request.as_bytes()));
    let mut reader = BufReader::new(stdout);
    let copied = read_batch(&mut reader, blobs, &mut each);
    drop(reader);
    let wrote = writer.join();
    let status = child.wait().context("waiting for git cat-file")?;
    copied?;
    match wrote {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(e).context("writing to git cat-file"),
        Err(_) => bail!("git cat-file: writer thread panicked"),
    }
    if !status.success() {
        bail!("git cat-file --batch failed ({status})");
    }
    Ok(())
}

/// Reads `git cat-file --batch` output for `blobs`, in order (see
/// [`stream_blobs`]).
fn read_batch(
    reader: &mut impl BufRead,
    blobs: &[(&str, &TreeBlob)],
    each: &mut impl FnMut(usize, &mut dyn Read) -> Result<()>,
) -> Result<()> {
    let mut header = Vec::new();
    for (i, (label, b)) in blobs.iter().enumerate() {
        header.clear();
        reader
            .read_until(b'\n', &mut header)
            .context("reading git cat-file output")?;
        let text = String::from_utf8_lossy(&header);
        let mut parts = text.split_whitespace();
        let (oid, kind, size) = (parts.next(), parts.next(), parts.next());
        if oid != Some(b.oid.as_str())
            || kind != Some("blob")
            || size.and_then(|s| s.parse::<u64>().ok()) != Some(b.size)
        {
            bail!(
                "git cat-file: unexpected header {:?} for {label} ({})",
                text.trim_end(),
                b.oid
            );
        }
        let mut body = reader.by_ref().take(b.size);
        each(i, &mut body)?;
        // Whatever `each` did not read is still this blob's.
        io::copy(&mut body, &mut io::sink()).context("reading git cat-file output")?;
        if body.limit() != 0 {
            bail!(
                "git cat-file: {label} ended after {} of {} bytes",
                b.size - body.limit(),
                b.size
            );
        }
        let mut lf = [0u8; 1];
        reader
            .read_exact(&mut lf)
            .context("reading git cat-file output")?;
        if lf != *b"\n" {
            bail!("git cat-file: no newline after {label}");
        }
    }
    Ok(())
}

/// `git ls-tree -r -l -z` output as path to blob; `None` for anything that is
/// not a regular-file blob (a symlink or a submodule).
fn parse_ls_tree(out: &[u8]) -> Result<BTreeMap<&str, Option<TreeBlob>>> {
    let mut map = BTreeMap::new();
    for rec in out.split(|&b| b == 0).filter(|r| !r.is_empty()) {
        let rec = std::str::from_utf8(rec).context("git ls-tree: path is not UTF-8")?;
        let Some((meta, path)) = rec.split_once('\t') else {
            bail!("git ls-tree: malformed line {rec:?}");
        };
        let mut parts = meta.split_whitespace();
        let (mode, kind, oid, size) = (parts.next(), parts.next(), parts.next(), parts.next());
        let blob = match (mode, kind, oid, size) {
            (Some("100644" | "100755"), Some("blob"), Some(oid), Some(size)) => Some(TreeBlob {
                oid: oid.to_owned(),
                size: size
                    .parse()
                    .with_context(|| format!("git ls-tree: bad size in {rec:?}"))?,
            }),
            (Some(_), Some(_), Some(_), Some(_)) => None,
            _ => bail!("git ls-tree: malformed line {rec:?}"),
        };
        map.insert(path, blob);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    #[cfg(not(windows))]
    fn default_dir_follows_xdg_then_home() -> Result<()> {
        let tail = Path::new("romraider-rs").join("corpus");
        assert_eq!(
            default_dir(env(&[("XDG_CACHE_HOME", "/x/cache"), ("HOME", "/h")]))?,
            Path::new("/x/cache").join(&tail)
        );
        // A relative XDG_CACHE_HOME is ignored, as the spec says.
        assert_eq!(
            default_dir(env(&[("XDG_CACHE_HOME", "rel"), ("HOME", "/h")]))?,
            Path::new("/h/.cache").join(&tail)
        );
        assert_eq!(
            default_dir(env(&[("XDG_CACHE_HOME", ""), ("HOME", "/h")]))?,
            Path::new("/h/.cache").join(&tail)
        );
        assert!(default_dir(env(&[])).is_err());
        Ok(())
    }

    #[test]
    #[cfg(windows)]
    fn default_dir_is_under_local_app_data() -> Result<()> {
        assert_eq!(
            default_dir(env(&[("LOCALAPPDATA", r"C:\Users\u\AppData\Local")]))?,
            Path::new(r"C:\Users\u\AppData\Local\romraider-rs\corpus")
        );
        assert!(default_dir(env(&[("HOME", "/h")])).is_err());
        Ok(())
    }

    #[test]
    fn inside_or_containing_the_repo_is_refused() -> Result<()> {
        let base = std::env::temp_dir().join(format!("rr-fetch-unit-{}", std::process::id()));
        let repo = base.join("repo");
        fs::create_dir_all(repo.join("target"))?;
        let inside = [
            repo.clone(),
            repo.join("target/corpus"),
            repo.join("not/yet/there"),
            base.join("elsewhere/../repo/x"),
        ];
        for d in &inside {
            assert!(check_outside_repo(d, &repo).is_err(), "{}", d.display());
        }
        assert!(
            check_outside_repo(&base, &repo).is_err(),
            "contains the repo"
        );
        assert!(check_outside_repo(&base.join("corpus"), &repo).is_ok());
        assert!(check_outside_repo(&base.join("repo-corpus"), &repo).is_ok());
        fs::remove_dir_all(&base)?;
        Ok(())
    }

    #[test]
    fn ls_tree_parsing_keeps_blobs_and_marks_the_rest() -> Result<()> {
        let out = b"100644 blob aaaa     3\tA/one.hex\0\
                    100755 blob bbbb     5\tA/two words.hex\0\
                    120000 blob cccc     4\tlink.hex\0\
                    160000 commit dddd       -\tsub\0";
        let map = parse_ls_tree(out)?;
        assert_eq!(map.len(), 4);
        assert!(matches!(map.get("A/one.hex"), Some(Some(b)) if b.oid == "aaaa" && b.size == 3));
        assert!(matches!(map.get("A/two words.hex"), Some(Some(b)) if b.size == 5));
        assert!(matches!(map.get("link.hex"), Some(None)));
        assert!(matches!(map.get("sub"), Some(None)));
        assert!(parse_ls_tree(b"garbage\0").is_err());
        assert!(parse_ls_tree(b"100644 blob aaaa x\tA\0").is_err());
        Ok(())
    }

    #[test]
    fn problems_name_their_path() {
        let p = Problem::Size {
            path: "A/b.hex".to_owned(),
            want: 2,
            got: 1,
        };
        assert_eq!(p.path(), "A/b.hex");
        assert_eq!(p.to_string(), "A/b.hex: size 1 bytes, manifest says 2");
        assert_eq!(
            listing("head", &[p.clone(), p]),
            "head\n  A/b.hex: size 1 bytes, manifest says 2\n  A/b.hex: size 1 bytes, manifest says 2"
        );
    }
}
