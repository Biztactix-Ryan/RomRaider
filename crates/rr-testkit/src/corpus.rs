//! Read-only loader for the external stock-ROM corpus (US-RR-11-9, D-025).
//!
//! `cargo xtask fetch-corpus` downloads the corpus into `RR_CORPUS_DIR` and
//! verifies it against the committed manifest `fixtures/corpus-manifest.toml`.
//! Tests read it through [`Corpus`]:
//!
//! * the directory is `RR_CORPUS_DIR`, else fetch-corpus's own default
//!   (`~/.cache/romraider-rs/corpus`; [`location`] has the exact rules,
//!   shared with fetch-corpus);
//! * a directory inside this repository's working tree, or containing it, is
//!   refused before anything is read ([`CorpusError::InsideRepo`];
//!   [`location`] covers symlinks, `..`, relative paths, not-yet-existing
//!   directories, case-insensitive file systems and git worktrees);
//! * a directory that does not exist or is empty is
//!   [`CorpusError::NotFetched`], distinct from every other error, so a
//!   caller can tell "fetch the corpus" apart from "the corpus is broken"
//!   ([`CorpusError::is_not_fetched`]);
//! * the manifest is read with this module's own serde model of schema 1
//!   (xtask's `corpus_manifest` is the format reference; rr-testkit cannot
//!   link xtask), including each entry's `[roms.files.suspect]` table;
//! * a suspect image (not a plain stock ROM of a standard size, US-RR-11-7)
//!   cannot be reached without saying what to do with it: [`Manifest::entries`]
//!   yields [`Entry::Clean`] or [`Entry::Suspect`], and the only other ways in
//!   are [`Manifest::clean`] and [`Manifest::suspect`]. [`RomFile`] has no
//!   public suspect flag to forget to check, and no public constructor, so a
//!   suspect entry cannot be re-made as a clean one;
//! * the bytes come back the same way (US-RR-11-2): [`Corpus::read`] returns
//!   an [`Image`], [`Image::Raw`] only for a clean entry and
//!   [`Image::Suspect`] (whole file, with why) for a suspect one, decided by
//!   the manifest entry itself, not by the caller. There is no `Vec<u8>`
//!   without that match; a container's or trailing-data image's embedded
//!   standard image is [`Suspect::embedded`]. (`tests/suspect_compile_fail.rs`
//!   proves the shape.)
//!
//! [`formulas`] reads the definition tree of the same corpus
//! (`defs/romraider/`, the manifest's `[defs]`): every formula, for the
//! expression parser's corpus tests (US-RR-20-10). [`defs`] lists every
//! pinned definition file (`[defs]`, `[defs.logger_v370]`) and reads each
//! one checked against its pin (US-RR-27-4).
//!
//! The loader never writes: it reads the manifest, lists the directory's top
//! level, and reads images on request ([`Corpus::read`], which refuses a
//! symlink and checks the size against the manifest; hashes are
//! fetch-corpus's job).
//!
//! # Usage
//!
//! Inside [`corpus_test!`](crate::corpus_test) (so the test is skipped, not
//! passed, without `RR_CORPUS_DIR`):
//!
//! ```ignore
//! use rr_testkit::corpus::{Corpus, Image};
//!
//! rr_testkit::corpus_test! {
//!     fn every_clean_image_has_a_standard_size() -> Result<(), rr_testkit::corpus::CorpusError> {
//!         let corpus = Corpus::open()?;
//!         for entry in corpus.manifest().entries() {
//!             match corpus.read(entry.file())? {
//!                 Image::Raw(bytes) => { /* a raw stock image */ }
//!                 Image::Suspect { bytes, suspect } => match suspect.embedded(&bytes) {
//!                     Some(image) => { /* the standard image inside it */ }
//!                     None => { /* blank, truncated, ...: never decode as raw */ }
//!                 },
//!             }
//!         }
//!         Ok(())
//!     }
//! }
//! ```
//!
//! Inside `corpus_test!` `RR_CORPUS_DIR` is always set, so
//! [`CorpusError::NotFetched`] there means the variable names a missing or
//! empty directory: propagate it and the test fails, naming the fix. Code
//! outside `corpus_test!` that wants to skip when the corpus is absent must
//! report the skip itself (a test that returns early passes; see
//! [`crate::fixtures`]).

pub mod defs;
pub mod formulas;
pub mod location;

use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

pub use location::{DIR_ENV, RepoGuardError};

/// Where the manifest is committed, relative to the repository root.
pub const MANIFEST_PATH: &str = "fixtures/corpus-manifest.toml";

/// The manifest schema version this loader reads.
pub const MANIFEST_SCHEMA: u32 = 1;

/// The image sizes a clean entry may have, in bytes: the sizes xtask's
/// `corpus_manifest::STANDARD_SIZES` names (with the legacy citations for
/// each). rr-testkit cannot link xtask, so the list is repeated here; a size
/// added there and not here fails the corpus-tier tests that check it.
pub const STANDARD_SIZES: [u64; 7] = [
    0x2_8000, 0x3_0000, 0x8_0000, 0x10_0000, 0x14_0000, 0x18_0000, 0x20_0000,
];

/// Errors from [`Corpus`] and [`Manifest`].
#[derive(Debug, thiserror::Error)]
pub enum CorpusError {
    /// `RR_CORPUS_DIR` is unset and there is no default to fall back on.
    #[error(
        "no corpus directory: set {DIR_ENV} (no {} to default from)",
        location::DEFAULT_DIR_VARS
    )]
    NoDir,
    /// The directory is inside the repository working tree, or contains it.
    #[error(transparent)]
    InsideRepo(#[from] RepoGuardError),
    /// The directory does not exist or is empty: the corpus has not been
    /// fetched there.
    #[error(
        "external corpus not fetched: {} {reason}; run `cargo xtask fetch-corpus` (it fetches into {DIR_ENV}, default ~/.cache/romraider-rs/corpus)",
        dir.display()
    )]
    NotFetched {
        /// The corpus directory.
        dir: PathBuf,
        /// `"does not exist"` or `"is empty"`.
        reason: &'static str,
    },
    /// The corpus path exists but is not a directory.
    #[error("corpus directory {} is not a directory", dir.display())]
    NotADirectory {
        /// The corpus path.
        dir: PathBuf,
    },
    /// A file or directory could not be read.
    #[error("cannot read {}: {source}", path.display())]
    Io {
        /// What was being read.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The manifest is not valid TOML, or not the schema-1 shape.
    #[error("corpus manifest {} is malformed: {source}", path.display())]
    ManifestToml {
        /// The manifest.
        path: PathBuf,
        /// The underlying error.
        source: toml::de::Error,
    },
    /// The manifest's `schema` is missing or not [`MANIFEST_SCHEMA`].
    #[error(
        "corpus manifest {} has schema {found} (this loader reads schema {MANIFEST_SCHEMA})",
        path.display()
    )]
    ManifestSchema {
        /// The manifest.
        path: PathBuf,
        /// The `schema` value found, as written (`missing` if absent).
        found: String,
    },
    /// A manifest entry is unusable (its path is not a plain relative
    /// `/`-separated path).
    #[error("corpus manifest {}: {reason}", path.display())]
    ManifestInvalid {
        /// The manifest.
        path: PathBuf,
        /// What is wrong.
        reason: String,
    },
    /// A manifest entry is not a regular file in the corpus (a symlink, a
    /// directory, or missing).
    #[error("corpus image {} is not a regular file; run `cargo xtask fetch-corpus` to repair the cache", path.display())]
    ImageNotAFile {
        /// The image path.
        path: PathBuf,
    },
    /// A corpus image's size disagrees with the manifest.
    #[error(
        "corpus image {} is {got} bytes, the manifest says {want}; run `cargo xtask fetch-corpus` to repair the cache",
        path.display()
    )]
    ImageSize {
        /// The image path.
        path: PathBuf,
        /// Size the manifest gives.
        want: u64,
        /// Size found.
        got: u64,
    },
}

impl CorpusError {
    /// Whether this is [`CorpusError::NotFetched`]: the corpus is simply not
    /// there yet, as opposed to misconfigured or broken.
    #[must_use]
    pub const fn is_not_fetched(&self) -> bool {
        matches!(self, Self::NotFetched { .. })
    }
}

/// The schema-1 manifest file, as serde reads it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    #[expect(
        dead_code,
        reason = "checked before deserializing; kept so deny_unknown_fields accepts it"
    )]
    schema: u32,
    roms: Roms,
    /// The ECU-definition corpus pin (`[defs]`, D-028, US-RR-16-5; xtask's
    /// `corpus_manifest::DefsCorpus` is the format reference). This loader
    /// reads ROMs only, so the table is accepted and not modelled yet.
    #[serde(default)]
    #[expect(
        dead_code,
        reason = "accepted so deny_unknown_fields allows `[defs]`; the ROM loader does not read it"
    )]
    defs: Option<serde::de::IgnoredAny>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Roms {
    upstream: String,
    commit: String,
    path_prefix: String,
    files: Vec<RomFile>,
}

/// One corpus image as the manifest describes it. Whether it is suspect is
/// only available through [`Entry`] (module docs).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RomFile {
    /// Path relative to the corpus root, `/`-separated.
    pub path: String,
    /// Size in bytes.
    pub size: u64,
    /// Lowercase hex SHA-256 of the bytes.
    pub sha256: String,
    /// The ROM ID the file name gives (not one read from the image).
    #[serde(default)]
    pub filename_rom_id: Option<String>,
    /// Model year from the file name.
    #[serde(default)]
    pub year: Option<u16>,
    /// Last model year, when the file name gives a range.
    #[serde(default)]
    pub year_end: Option<u16>,
    /// Market region from the file name.
    #[serde(default)]
    pub region: Option<String>,
    /// Model from the file name.
    #[serde(default)]
    pub model: Option<String>,
    /// The rest of the file name after the model.
    #[serde(default)]
    pub trim: Option<String>,
    #[serde(default)]
    suspect: Option<Suspect>,
}

/// Why an image is suspect (`[roms.files.suspect]`, US-RR-11-7).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Suspect {
    /// What is wrong with it.
    pub kind: SuspectKind,
    /// Human-readable reason with the evidence.
    pub reason: String,
    /// Offset of a standard-size image found inside the file, when one was.
    #[serde(default)]
    pub image_offset: Option<u64>,
    /// Size of that image; present exactly when `image_offset` is.
    #[serde(default)]
    pub image_size: Option<u64>,
}

impl Suspect {
    /// The standard-size image embedded in a suspect file's `bytes` (the
    /// whole file, as [`Image::Suspect`] holds it): the `image_size` bytes at
    /// `image_offset`, when the manifest gives them (a container's payload,
    /// or the prefix before trailing data). `None` when it gives none (blank,
    /// CRLF expansion, truncated, unknown) or `bytes` is too short.
    #[must_use]
    pub fn embedded<'b>(&self, bytes: &'b [u8]) -> Option<&'b [u8]> {
        let start = usize::try_from(self.image_offset?).ok()?;
        let len = usize::try_from(self.image_size?).ok()?;
        bytes.get(start..start.checked_add(len)?)
    }
}

/// The classification of a suspect image (xtask's `corpus_manifest::classify`
/// documents each).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SuspectKind {
    /// A standard size whose bytes are all one value.
    Blank,
    /// A standard-size image with a CR inserted before every LF.
    CrlfExpansion,
    /// Tagged chunks whose last payload is a standard-size image.
    Container,
    /// A standard-size image followed by extra bytes.
    TrailingData,
    /// Shorter than the nearest standard size.
    Truncated,
    /// None of the above.
    Unknown,
}

/// A manifest entry, with its suspect status made explicit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry<'a> {
    /// A raw stock image of a standard size.
    Clean(&'a RomFile),
    /// An image that must not be treated as a raw stock image.
    Suspect {
        /// The image.
        file: &'a RomFile,
        /// Why it is suspect.
        suspect: &'a Suspect,
    },
}

/// The bytes of a corpus image ([`Corpus::read`]), with its suspect status
/// made explicit: there is no way to the bytes without matching this.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "a corpus image is only useful once matched as raw or suspect"]
pub enum Image<'f> {
    /// A clean entry: a raw stock image of a standard size.
    Raw(Vec<u8>),
    /// A suspect entry: the whole file, which must not be treated as a raw
    /// stock image ([`Suspect::embedded`] finds the one inside, if any).
    Suspect {
        /// The file's bytes, as they are.
        bytes: Vec<u8>,
        /// Why it is suspect.
        suspect: &'f Suspect,
    },
}

impl<'a> Entry<'a> {
    /// The image, whichever kind it is.
    #[must_use]
    pub const fn file(&self) -> &'a RomFile {
        match self {
            Self::Clean(file) | Self::Suspect { file, .. } => file,
        }
    }
}

/// The corpus manifest (schema 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    path: PathBuf,
    upstream: String,
    commit: String,
    path_prefix: String,
    files: Vec<RomFile>,
}

impl Manifest {
    /// Reads the manifest at `path`.
    ///
    /// # Errors
    ///
    /// [`CorpusError::Io`], or as [`Manifest::parse`].
    pub fn load(path: impl AsRef<Path>) -> Result<Self, CorpusError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|source| CorpusError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(path, &text)
    }

    /// Parses manifest text; `path` only names it in errors.
    ///
    /// # Errors
    ///
    /// [`CorpusError::ManifestToml`], [`CorpusError::ManifestSchema`] or
    /// [`CorpusError::ManifestInvalid`].
    pub fn parse(path: impl Into<PathBuf>, text: &str) -> Result<Self, CorpusError> {
        let path = path.into();
        let table = match toml::from_str::<toml::Table>(text) {
            Ok(t) => t,
            Err(source) => return Err(CorpusError::ManifestToml { path, source }),
        };
        match table.get("schema") {
            Some(toml::Value::Integer(n)) if *n == i64::from(MANIFEST_SCHEMA) => {}
            other => {
                return Err(CorpusError::ManifestSchema {
                    path,
                    found: other.map_or_else(|| "missing".to_owned(), ToString::to_string),
                });
            }
        }
        let file: ManifestFile = match toml::Value::Table(table).try_into() {
            Ok(f) => f,
            Err(source) => return Err(CorpusError::ManifestToml { path, source }),
        };
        for f in &file.roms.files {
            if !plain_relative(&f.path) {
                return Err(CorpusError::ManifestInvalid {
                    reason: format!("{:?} is not a plain relative `/`-separated path", f.path),
                    path,
                });
            }
        }
        Ok(Self {
            path,
            upstream: file.roms.upstream,
            commit: file.roms.commit,
            path_prefix: file.roms.path_prefix,
            files: file.roms.files,
        })
    }

    /// The file it was read from.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Upstream repository URL.
    #[must_use]
    pub fn upstream(&self) -> &str {
        &self.upstream
    }

    /// Pinned upstream commit.
    #[must_use]
    pub fn commit(&self) -> &str {
        &self.commit
    }

    /// Directory of the corpus inside the upstream repository.
    #[must_use]
    pub fn path_prefix(&self) -> &str {
        &self.path_prefix
    }

    /// Number of images.
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether it lists no images.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Every image, in manifest order, as [`Entry::Clean`] or
    /// [`Entry::Suspect`].
    pub fn entries(&self) -> impl Iterator<Item = Entry<'_>> {
        self.files.iter().map(|file| match &file.suspect {
            None => Entry::Clean(file),
            Some(suspect) => Entry::Suspect { file, suspect },
        })
    }

    /// The images that are not suspect.
    pub fn clean(&self) -> impl Iterator<Item = &RomFile> {
        self.files.iter().filter(|f| f.suspect.is_none())
    }

    /// The suspect images, each with why.
    pub fn suspect(&self) -> impl Iterator<Item = (&RomFile, &Suspect)> {
        self.files
            .iter()
            .filter_map(|f| f.suspect.as_ref().map(|s| (f, s)))
    }
}

/// `path` is non-empty, `/`-separated, and every component is a plain name
/// (no `.`, `..`, root, drive, or backslash), so joining it to the corpus
/// directory stays inside it.
fn plain_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && path.split('/').all(|part| {
            let mut c = Path::new(part).components();
            matches!((c.next(), c.next()), (Some(Component::Normal(n)), None) if n == part)
        })
}

/// The external stock-ROM corpus: a directory outside the repository and the
/// manifest describing it.
#[derive(Debug, Clone)]
pub struct Corpus {
    dir: PathBuf,
    manifest: Manifest,
}

impl Corpus {
    /// Opens the corpus at `RR_CORPUS_DIR` (else the default directory) with
    /// the committed manifest.
    ///
    /// # Errors
    ///
    /// [`CorpusError::NoDir`], or as [`Corpus::open_at`].
    pub fn open() -> Result<Self, CorpusError> {
        let dir = location::dir_from_env(|k| std::env::var_os(k)).ok_or(CorpusError::NoDir)?;
        Self::open_at(dir, crate::fixtures::repo_root().join(MANIFEST_PATH))
    }

    /// Opens the corpus at `dir` with the manifest at `manifest`, refusing a
    /// `dir` inside (or containing) this repository's working trees.
    ///
    /// # Errors
    ///
    /// As [`Corpus::open_in`], with this repository as `repo`.
    pub fn open_at(dir: impl AsRef<Path>, manifest: impl AsRef<Path>) -> Result<Self, CorpusError> {
        Self::open_in(dir, manifest, crate::fixtures::repo_root())
    }

    /// Opens the corpus at `dir` with the manifest at `manifest`, refusing a
    /// `dir` inside or containing any working tree of the repository at
    /// `repo` ([`location::protected_roots`]). In order: the repository
    /// check, the manifest, then the directory.
    ///
    /// # Errors
    ///
    /// [`CorpusError::InsideRepo`]; the [`Manifest::load`] errors;
    /// [`CorpusError::NotFetched`] when `dir` does not exist or is empty;
    /// [`CorpusError::NotADirectory`]; [`CorpusError::Io`].
    pub fn open_in(
        dir: impl AsRef<Path>,
        manifest: impl AsRef<Path>,
        repo: impl AsRef<Path>,
    ) -> Result<Self, CorpusError> {
        let dir = dir.as_ref().to_path_buf();
        location::check_outside_repo(&dir, repo.as_ref())?;
        let manifest = Manifest::load(manifest)?;
        let io = |source| CorpusError::Io {
            path: dir.clone(),
            source,
        };
        match std::fs::metadata(&dir) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => return Err(CorpusError::NotADirectory { dir }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(CorpusError::NotFetched {
                    dir,
                    reason: "does not exist",
                });
            }
            Err(e) => return Err(io(e)),
        }
        if std::fs::read_dir(&dir).map_err(io)?.next().is_none() {
            return Err(CorpusError::NotFetched {
                dir,
                reason: "is empty",
            });
        }
        Ok(Self { dir, manifest })
    }

    /// The corpus directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The manifest.
    #[must_use]
    pub const fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Where `file` is in the corpus directory.
    #[must_use]
    pub fn path(&self, file: &RomFile) -> PathBuf {
        self.dir.join(&file.path)
    }

    /// The bytes of `file`, after checking it is a regular file (not a
    /// symlink) of the manifest size: [`Image::Raw`] if the manifest entry is
    /// clean, [`Image::Suspect`] if it is suspect.
    ///
    /// # Errors
    ///
    /// [`CorpusError::ImageNotAFile`], [`CorpusError::ImageSize`] or
    /// [`CorpusError::Io`].
    pub fn read<'f>(&self, file: &'f RomFile) -> Result<Image<'f>, CorpusError> {
        let path = self.path(file);
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.is_file() => {}
            Ok(_) => return Err(CorpusError::ImageNotAFile { path }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(CorpusError::ImageNotAFile { path });
            }
            Err(source) => return Err(CorpusError::Io { path, source }),
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(source) => return Err(CorpusError::Io { path, source }),
        };
        let got = bytes.len() as u64;
        if got != file.size {
            return Err(CorpusError::ImageSize {
                path,
                want: file.size,
                got,
            });
        }
        Ok(match &file.suspect {
            None => Image::Raw(bytes),
            Some(suspect) => Image::Suspect { bytes, suspect },
        })
    }
}

impl fmt::Display for Corpus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} image(s) at {} ({} @ {})",
            self.manifest.len(),
            self.dir.display(),
            self.manifest.upstream,
            self.manifest.commit
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_relative_paths_only() {
        for ok in ["a.hex", "ADM/Forester/x-y.hex", "a b/c.hex"] {
            assert!(plain_relative(ok), "{ok}");
        }
        for bad in [
            "",
            "/abs.hex",
            "../up.hex",
            "a/../b",
            "./a",
            "a//b",
            "a/",
            "a\\b",
            "a/./b",
        ] {
            assert!(!plain_relative(bad), "{bad}");
        }
        #[cfg(windows)]
        assert!(!plain_relative("C:x"));
    }
}
