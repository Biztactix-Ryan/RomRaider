//! `cargo xtask gen-corpus-manifest [--check] [--mirror <DIR>] [--manifest
//! <FILE>]` (US-RR-11-6, D-025): the committed manifest of the external
//! stock-ROM corpus, `fixtures/corpus-manifest.toml`.
//!
//! The corpus is upstream `samuelw9999/RomRaider` at [`PIN_COMMIT`] (D-025).
//! The manifest holds metadata only: for every image its path, size and
//! SHA-256, plus the fields its file name encodes. The images are never
//! committed; `cargo xtask fetch-corpus` (US-RR-11-5) downloads them into
//! `RR_CORPUS_DIR` and verifies them against this file.
//!
//! # Where the manifest comes from
//!
//! It is generated from a local mirror (`--mirror <DIR>`, or the
//! `RR_CORPUS_MIRROR` environment variable) laid out as
//! `/home/ryan/Repo/romraider-stock-roms` is:
//!
//! * `<DIR>/roms/`: a git checkout of upstream at [`PIN_COMMIT`]. Every
//!   regular file in it except `.git/` and the [`NOT_CORPUS`] files at its
//!   top is a corpus image;
//! * `<DIR>/SHA256SUMS`: `sha256sum` output for those images, taken when the
//!   mirror was made (`<hex>  ./<path>`). The generated manifest must agree
//!   with it exactly, file for file and hash for hash, or generation fails.
//!
//! The command also checks that the checkout's `HEAD` is [`PIN_COMMIT`]
//! (with `git rev-parse`), so the commit recorded in the manifest header is
//! the one the hashes came from.
//!
//! Without `--check` the manifest is written; with `--check` nothing is
//! written and the command fails unless the committed file is byte-identical
//! to a fresh generation. Output is deterministic (entries sorted by path,
//! fixed field order, `\n` endings), so the file's hash is a stable CI cache
//! key (US-RR-11-8).
//!
//! # File-name fields
//!
//! Upstream names files `<ROM ID>-<year>[-<end year>]-<region>[-Subaru]-
//! <model>[-<trim>].hex`, for example
//! `A2WC400H-2005-ADM-Subaru-Forester-XT-AT.hex`. [`parse_file_name`] reads
//! that strictly, left to right: when a part does not match, it and every
//! later part are left out (absent in the manifest, never guessed) and the
//! generator reports the name. `filename_rom_id` is what the *file name*
//! says; the ROM ID read from the image by our own code is a different,
//! later field.
//!
//! # Suspect images
//!
//! An image is *suspect* when it is not a plain stock ROM of a standard size:
//! its entry carries a [`Suspect`] (`[roms.files.suspect]`) with a
//! [`SuspectKind`], a reason giving the evidence, and, where the analysis
//! found one, where a standard-size image sits inside the file. Corpus tests
//! must branch on it ([`RomFile::is_suspect`], [`RomCorpus::clean_files`],
//! [`RomCorpus::suspect_files`]) rather than feed a suspect file to code that
//! expects a raw image. [`classify`] decides, at generation time, from the
//! bytes:
//!
//! * a size in [`STANDARD_SIZES`] is clean, unless every byte is the same
//!   ([`SuspectKind::Blank`]: an erased or empty dump, not firmware);
//! * any other size is suspect, classified as the first of:
//!   [`SuspectKind::CrlfExpansion`] (every LF has a CR before it and dropping
//!   those CRs gives a standard size: a text-mode transfer),
//!   [`SuspectKind::Container`] (the file is a run of tagged chunks whose
//!   last payload is a standard-size image), [`SuspectKind::TrailingData`]
//!   (a standard-size prefix plus extra bytes, the extra described as padding,
//!   text or binary), [`SuspectKind::Truncated`] (short of the nearest
//!   standard size), or [`SuspectKind::Unknown`] (an empty file).
//!
//! The standard sizes come from legacy RomRaider and the corpus itself, not
//! from "is a power of two": 160 KiB and 192 KiB are the two 16-bit Subaru
//! ECU image sizes legacy converts between, and 1.25/1.5 MiB are flash sizes
//! legacy's J2534 reflash accepts (citations on [`STANDARD_SIZES`]). Every
//! corpus image of a listed size is a raw image (an SH or 16-bit reset
//! vector table at offset 0), so none of those sizes is suspect on its own.
//!
//! At [`PIN_COMMIT`] the two suspect images are the ones D-025 names,
//! `JDM/Legacy/AE5I410A…` (1,048,731 bytes) and `USDM/Legacy/DE5M301H…`
//! (1,048,784 bytes). Neither is CRLF-mangled (each has thousands of bare
//! LFs): both are `INFO`/`DRMI`/`MEML`/`MEMD` chunk containers (tag, then a
//! big-endian `u32` length) whose `MEMD` payload is a 1 MiB image ending the
//! file, after a 155- and a 208-byte header.
//!
//! Identical bytes under two paths (the same stock ROM filed under two
//! regions, e.g. `EZ1G108N` in ADM and EDM) are not suspect: the image is
//! fine. The generator lists them ([`duplicate_hashes`]) for information.
//!
//! # Definition corpus (D-028, US-RR-16-5)
//!
//! The optional `[defs]` table pins the ECU-definition corpus: the
//! [`DEFS_PATH_PREFIX`] subtree of [`DEFS_UPSTREAM_URL`] at
//! [`DEFS_PIN_COMMIT`], with the source's branch and licence position
//! ([`DEFS_LICENCE`]), and every file's path (relative to the prefix), size
//! and SHA-256 in `[[defs.files]]`. Some of those paths contain spaces
//! (`ecu/RR D2UJ001T.xml`); they are kept exactly as upstream spells them.
//! `[defs.logger_v370]` pins the forum logger-definition package
//! ([`LOGGER_V370_FILE`], [`LOGGER_V370_SIZE`] bytes, SHA-256
//! [`LOGGER_V370_SHA256`]), which no tool ever downloads: `fetch-corpus`
//! only verifies a copy Ryan supplies ([`crate::fetch_corpus`]). Like the
//! ROMs, nothing but this metadata is committed.
//!
//! The table is generated from a second local mirror (`--defs-mirror <DIR>`,
//! or [`DEFS_MIRROR_ENV`]) laid out as `/home/ryan/Repo/romraider-ecu-defs`
//! is:
//!
//! * `<DIR>/mirror.git`: a bare `git clone --mirror` of upstream that has
//!   [`DEFS_PIN_COMMIT`]. The files are read from it as raw blobs (no
//!   checkout, so nothing can convert a line ending);
//! * `<DIR>/SHA256SUMS`: `sha256sum`-style lines for the same files, paths
//!   relative to the prefix (`<hex>  ./ecu/...`). Generation fails unless it
//!   agrees exactly.
//!
//! A manifest without `[defs]` (the synthetic ones tests write) describes
//! ROMs only, and `fetch-corpus` then fetches no definitions. A ROM path may
//! not start with [`DEFS_DIR`]`/`: that top-level directory of the corpus
//! cache holds the definitions.
//!
//! # Who reads it
//!
//! xtask's own corpus commands read it with [`Manifest::from_toml`]. Tests
//! that consume the corpus run under rr-testkit, which xtask cannot link and
//! which should not link xtask (`cargo xtask check-deps`, rule 3), so they
//! read the same schema-1 TOML with their own serde model; this module's
//! docs and types are the format reference.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// Where the manifest is committed, relative to the workspace root.
pub const COMMITTED_PATH: &str = "fixtures/corpus-manifest.toml";

/// Environment variable naming the local mirror when `--mirror` is not given.
pub const MIRROR_ENV: &str = "RR_CORPUS_MIRROR";

/// The checkout of upstream inside the mirror directory.
pub const MIRROR_ROMS_DIR: &str = "roms";

/// The `sha256sum` file inside the mirror directory.
pub const MIRROR_SUMS_FILE: &str = "SHA256SUMS";

/// The manifest format version.
pub const SCHEMA: u32 = 1;

/// The upstream repository the corpus is fetched from (D-025).
pub const UPSTREAM_URL: &str = "https://github.com/samuelw9999/RomRaider";

/// The pinned upstream commit (D-025: `639f3c73`), in full.
pub const PIN_COMMIT: &str = "639f3c73c1bd8efee48347bd71bb064211b95242";

/// Where the corpus sits inside the upstream repository: the images are at
/// its root (`ADM/Forester/...`), so an entry's upstream path is
/// `PATH_PREFIX + path`.
pub const PATH_PREFIX: &str = "";

/// Files at the top of the upstream tree that are not corpus images.
pub const NOT_CORPUS: &[&str] = &[".gitattributes", "README.md"];

/// Region codes a file name may carry. Anything else is reported, not kept.
pub const REGIONS: &[&str] = &["ADM", "EDM", "JDM", "SADM", "USDM"];

/// The top-level directory of a corpus cache that holds the definition
/// corpora (D-028) rather than ROM images; no ROM path may start with it.
pub const DEFS_DIR: &str = "defs";

/// Environment variable naming the local definitions mirror when
/// `--defs-mirror` is not given.
pub const DEFS_MIRROR_ENV: &str = "RR_DEFS_MIRROR";

/// The bare mirror of the definitions upstream inside its mirror directory.
pub const DEFS_MIRROR_GIT: &str = "mirror.git";

/// The ECU-definition upstream (D-028).
pub const DEFS_UPSTREAM_URL: &str = "https://github.com/TD-D/SubaruDefs";

/// The upstream branch the pin was taken from (D-028).
pub const DEFS_BRANCH: &str = "Alpha";

/// The pinned definitions commit (D-028: `0b0c0c42`, 2024-09-20), in full.
pub const DEFS_PIN_COMMIT: &str = "0b0c0c42acea0d89ffd8b50378ae13ae32b5b43d";

/// The subtree of the definitions upstream that is the corpus: RomRaider
/// format only (`ECUFlash/` is not part of it, D-028).
pub const DEFS_PATH_PREFIX: &str = "RomRaider/";

/// The licence position of both definition sources, as D-028 records it.
pub const DEFS_LICENCE: &str = "none stated (no LICENSE file; a use-at-your-own-risk disclaimer only): all rights stay with the authors; fetched and used for testing only, never vendored, published or redistributed (D-028)";

/// File name of the pinned forum logger-definition package (D-028).
pub const LOGGER_V370_FILE: &str = "logger_v370.zip";

/// Its size in bytes (D-028).
pub const LOGGER_V370_SIZE: u64 = 986_211;

/// Its SHA-256 (D-028).
pub const LOGGER_V370_SHA256: &str =
    "280d386542f6fd49e3467363a62c12c6c642c127f3651a37265bdbac127ea0fa";

/// Where it was published. Recorded for people only: no tool downloads it.
pub const LOGGER_V370_URL: &str = "https://www.romraider.com/forum/download/file.php?id=38909";

/// How it is obtained and kept (D-028).
pub const LOGGER_V370_NOTE: &str = "downloaded once by hand in a browser and kept in a private mirror only (the forum asks that it not be re-hosted); never downloaded by tooling; fetch-corpus verifies a supplied copy and reports when there is none";

/// The comment block at the top of the generated file.
const HEADER: &str = "\
# External stock-ROM and ECU-definition corpus manifest (D-025, D-028).
# GENERATED: do not edit.
#   regenerate: cargo xtask gen-corpus-manifest --mirror <dir> --defs-mirror <dir>
#   check:      cargo xtask gen-corpus-manifest --check --mirror <dir> --defs-mirror <dir>
# Metadata only: the images and definitions are never committed.
# `cargo xtask fetch-corpus` downloads them into RR_CORPUS_DIR and verifies
# them against this file.
# An entry's upstream file is `[roms] upstream` at `commit`, path
# `path_prefix + path`. filename_rom_id, year, year_end, region, model and
# trim are parsed from the file name and absent when it does not parse;
# filename_rom_id is not the ROM ID read from the image.
# A [roms.files.suspect] table marks an image that is not a plain ROM of a
# standard size (kind, reason, and where a standard-size image sits inside
# it when one was found); corpus tests must handle those entries explicitly.
# [defs] pins the ECU definitions the same way (upstream, commit,
# path_prefix, files); [defs.logger_v370] pins the forum logger package,
# which no tool downloads: fetch-corpus only verifies a copy supplied to it.
";

/// The whole manifest file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Format version, [`SCHEMA`].
    pub schema: u32,
    /// The stock-ROM corpus.
    pub roms: RomCorpus,
    /// The ECU-definition corpus (D-028); absent in a ROM-only manifest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defs: Option<DefsCorpus>,
}

/// The ECU-definition corpus (D-028): where it comes from, every file in
/// it, and the logger package that is supplied by hand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefsCorpus {
    /// Upstream repository URL ([`DEFS_UPSTREAM_URL`]).
    pub upstream: String,
    /// The branch the pin was taken from ([`DEFS_BRANCH`]); for people only,
    /// the commit is what is fetched.
    pub branch: String,
    /// Full upstream commit SHA ([`DEFS_PIN_COMMIT`]).
    pub commit: String,
    /// Directory of the corpus inside the upstream repository, empty or
    /// ending in `/` ([`DEFS_PATH_PREFIX`]).
    pub path_prefix: String,
    /// The licence position ([`DEFS_LICENCE`]).
    pub licence: String,
    /// The forum logger-definition package (D-028).
    pub logger_v370: LoggerPackage,
    /// Every file of the subtree, sorted by path.
    pub files: Vec<DefFile>,
}

/// One file of the definition corpus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefFile {
    /// Path relative to [`DefsCorpus::path_prefix`], `/`-separated and
    /// spelled exactly as upstream does (spaces included):
    /// `ecu/standard/ecu_defs.xml`, `logger/logger_STD_EN_v290.xml`.
    pub path: String,
    /// Size in bytes.
    pub size: u64,
    /// Lowercase hex SHA-256 of the bytes.
    pub sha256: String,
}

/// A pinned file that is never downloaded by tooling, only verified when
/// supplied (D-028: the forum's `logger_v370.zip`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoggerPackage {
    /// File name ([`LOGGER_V370_FILE`]); one path component.
    pub file: String,
    /// Size in bytes ([`LOGGER_V370_SIZE`]).
    pub size: u64,
    /// Lowercase hex SHA-256 ([`LOGGER_V370_SHA256`]).
    pub sha256: String,
    /// Where it was published ([`LOGGER_V370_URL`]); never fetched.
    pub url: String,
    /// How it is obtained and kept ([`LOGGER_V370_NOTE`]).
    pub note: String,
}

impl DefsCorpus {
    /// Checks the invariants [`Manifest::validate`] checks for `[defs]`.
    ///
    /// # Errors
    ///
    /// The first violated invariant.
    pub fn validate(&self) -> Result<()> {
        validate_defs(self)
    }

    /// The path of `file` inside the upstream repository at
    /// [`DefsCorpus::commit`].
    #[must_use]
    pub fn upstream_path(&self, file: &DefFile) -> String {
        format!("{}{}", self.path_prefix, file.path)
    }
}

/// The stock-ROM corpus: where it comes from, and every image in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RomCorpus {
    /// Upstream repository URL ([`UPSTREAM_URL`]).
    pub upstream: String,
    /// Full upstream commit SHA ([`PIN_COMMIT`]).
    pub commit: String,
    /// Directory of the corpus inside the upstream repository, empty or
    /// ending in `/` ([`PATH_PREFIX`]).
    pub path_prefix: String,
    /// Every image, sorted by path.
    pub files: Vec<RomFile>,
}

/// One corpus image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RomFile {
    /// Path relative to the corpus root, `/`-separated
    /// (`ADM/Forester/A2WC400H-2005-ADM-Subaru-Forester-XT-AT.hex`).
    pub path: String,
    /// Size in bytes.
    pub size: u64,
    /// Lowercase hex SHA-256 of the bytes.
    pub sha256: String,
    /// The ROM ID the file name gives (not one read from the image).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename_rom_id: Option<String>,
    /// Model year from the file name (the first year of a range).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    /// Last model year, when the file name gives a range (`2001-02`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year_end: Option<u16>,
    /// Market region from the file name, one of [`REGIONS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// Model from the file name, as written (`Liberty`, `Legacy(Outback)`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The rest of the file name after the model (`XT-AT`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim: Option<String>,
    /// Why the image is suspect, when it is (US-RR-11-7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suspect: Option<Suspect>,
}

/// A suspect image (US-RR-11-7): corpus tests must handle it explicitly.
/// See the module docs and [`classify`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Suspect {
    /// What is wrong with it.
    pub kind: SuspectKind,
    /// Human-readable reason with the evidence (sizes, byte counts, chunks).
    pub reason: String,
    /// Offset of a standard-size image found inside the file, when one was
    /// (the payload of a [`SuspectKind::Container`], the prefix before
    /// [`SuspectKind::TrailingData`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_offset: Option<u64>,
    /// Size of that image, one of [`STANDARD_SIZES`]; present exactly when
    /// `image_offset` is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_size: Option<u64>,
}

/// The classification of a suspect image (see [`classify`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SuspectKind {
    /// A standard size whose bytes are all one value: an erased or empty
    /// dump, not firmware.
    Blank,
    /// A standard-size image with a CR inserted before every LF (a
    /// text-mode transfer); dropping the CR before each LF restores it.
    CrlfExpansion,
    /// A run of tagged chunks whose last payload is a standard-size image
    /// (`image_offset`/`image_size`): a container, not a raw image.
    Container,
    /// A standard-size image (offset 0) followed by extra bytes.
    TrailingData,
    /// Shorter than the nearest standard size.
    Truncated,
    /// None of the above (an empty file).
    Unknown,
}

/// The sizes a stock image may have, with a label for reasons. A size not
/// listed here is suspect. Evidence (legacy = `legacy/` at the fork's base):
///
/// * 160 KiB (`0x28000`) and 192 KiB (`0x30000`): the 16-bit Subaru ECU
///   image sizes, `Settings.SIXTEENBIT_SMALL_SIZE` / `SIXTEENBIT_LARGE_SIZE`
///   (legacy `src/main/java/com/romraider/Settings.java:134-135`), which the
///   editor converts between (`swing/ECUEditorMenuBar.java:321-329`,
///   `:629-660`). The corpus has 60 and 29 of them.
/// * 512 KiB: SH7055, `<filesize>512kb</filesize>`
///   (legacy `src/test/definitions/scalingbase_test.xml:18-19`). 72 in the
///   corpus.
/// * 1 MiB: SH7058, `<filesize>1024kb</filesize>`
///   (`scalingbase_test.xml:70-72`). 336 in the corpus.
/// * 1.25, 1.5 and 2 MiB: with 512 KiB and 1 MiB, the flash-image sizes
///   legacy's J2534 reflash path accepts
///   (`src/main/java/com/romraider/io/j2534/api/TestJ2534NCS.java:539-540`).
///   The corpus has one 1.5 MiB (`AF5G200A`, 2013 FA20DIT Forester) and one
///   2 MiB (`LV9N100B`, 2015 WRX S4) image, both raw (reset vectors at 0).
pub const STANDARD_SIZES: &[(u64, &str)] = &[
    (0x2_8000, "160 KiB, 16-bit ECU"),
    (0x3_0000, "192 KiB, 16-bit ECU"),
    (0x8_0000, "512 KiB, SH7055"),
    (0x10_0000, "1 MiB, SH7058"),
    (0x14_0000, "1.25 MiB"),
    (0x18_0000, "1.5 MiB"),
    (0x20_0000, "2 MiB"),
];

/// The label of `size` in [`STANDARD_SIZES`], or `None` if it is not a
/// standard size.
#[must_use]
pub fn standard_size(size: u64) -> Option<&'static str> {
    STANDARD_SIZES
        .iter()
        .find(|(s, _)| *s == size)
        .map(|(_, label)| *label)
}

/// The largest file the generator reads; corpus images are at most 2 MiB.
pub const MAX_IMAGE_BYTES: u64 = 16 << 20;

impl RomFile {
    /// Whether the image is suspect (has a [`Suspect`]). A corpus test must
    /// branch on this, or use [`RomCorpus::clean_files`].
    #[must_use]
    pub const fn is_suspect(&self) -> bool {
        self.suspect.is_some()
    }
}

impl RomCorpus {
    /// The path of `file` inside the upstream repository at
    /// [`RomCorpus::commit`].
    #[must_use]
    pub fn upstream_path(&self, file: &RomFile) -> String {
        format!("{}{}", self.path_prefix, file.path)
    }

    /// The images that are not suspect: raw images of a standard size.
    pub fn clean_files(&self) -> impl Iterator<Item = &RomFile> {
        self.files.iter().filter(|f| !f.is_suspect())
    }

    /// The suspect images, each with its [`Suspect`].
    pub fn suspect_files(&self) -> impl Iterator<Item = (&RomFile, &Suspect)> {
        self.files
            .iter()
            .filter_map(|f| f.suspect.as_ref().map(|s| (f, s)))
    }

    /// Images of a non-standard size that are *not* marked suspect; empty
    /// for a generated manifest. ([`Manifest::validate`] does not require
    /// it, so synthetic test manifests may use any size.)
    #[must_use]
    pub fn unmarked_nonstandard(&self) -> Vec<&RomFile> {
        self.files
            .iter()
            .filter(|f| !f.is_suspect() && standard_size(f.size).is_none())
            .collect()
    }
}

impl Manifest {
    /// Parses and validates a manifest.
    ///
    /// # Errors
    ///
    /// Invalid TOML, an unknown field, a schema other than [`SCHEMA`], a bad
    /// commit, path prefix, path or hash, or paths not strictly sorted.
    pub fn from_toml(text: &str) -> Result<Self> {
        let manifest: Self = toml::from_str(text).context("corpus manifest: invalid TOML")?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// The canonical text of the manifest: the generated-file header, then
    /// the TOML. The same manifest always renders to the same bytes.
    ///
    /// # Errors
    ///
    /// Only if TOML serialisation fails.
    pub fn render(&self) -> Result<String> {
        let body = toml::to_string(self).context("corpus manifest: serialising")?;
        Ok(format!("{HEADER}\n{body}"))
    }

    /// Checks the invariants [`Manifest::from_toml`] promises.
    ///
    /// # Errors
    ///
    /// The first violated invariant.
    pub fn validate(&self) -> Result<()> {
        if self.schema != SCHEMA {
            bail!(
                "corpus manifest: schema {} (expected {SCHEMA})",
                self.schema
            );
        }
        let roms = &self.roms;
        if !is_lower_hex(&roms.commit, 40) {
            bail!(
                "corpus manifest: commit {:?} is not a full 40-hex-digit SHA",
                roms.commit
            );
        }
        if !(roms.path_prefix.is_empty()
            || (roms.path_prefix.ends_with('/')
                && valid_rel_path(&roms.path_prefix[..roms.path_prefix.len() - 1])))
        {
            bail!(
                "corpus manifest: path_prefix {:?} must be empty or a relative directory ending in `/`",
                roms.path_prefix
            );
        }
        for (i, f) in roms.files.iter().enumerate() {
            if !valid_rel_path(&f.path) {
                bail!(
                    "corpus manifest: {:?} is not a relative `/`-separated path",
                    f.path
                );
            }
            if f.path.split('/').next() == Some(DEFS_DIR) {
                bail!(
                    "corpus manifest: {}: ROM paths may not start with `{DEFS_DIR}/` (the cache keeps the definitions there)",
                    f.path
                );
            }
            if !is_lower_hex(&f.sha256, 64) {
                bail!(
                    "corpus manifest: {}: sha256 {:?} is not 64 lowercase hex digits",
                    f.path,
                    f.sha256
                );
            }
            if let Some(s) = &f.suspect {
                validate_suspect(f, s)?;
            }
            if let Some(prev) = i.checked_sub(1).and_then(|p| roms.files.get(p))
                && prev.path >= f.path
            {
                bail!(
                    "corpus manifest: {} is not after {} (paths must be unique and sorted)",
                    f.path,
                    prev.path
                );
            }
        }
        if let Some(defs) = &self.defs {
            validate_defs(defs)?;
        }
        Ok(())
    }
}

fn valid_prefix(prefix: &str) -> bool {
    prefix.is_empty() || prefix.strip_suffix('/').is_some_and(valid_rel_path)
}

fn validate_defs(defs: &DefsCorpus) -> Result<()> {
    if !is_lower_hex(&defs.commit, 40) {
        bail!(
            "corpus manifest: [defs] commit {:?} is not a full 40-hex-digit SHA",
            defs.commit
        );
    }
    if !valid_prefix(&defs.path_prefix) {
        bail!(
            "corpus manifest: [defs] path_prefix {:?} must be empty or a relative directory ending in `/`",
            defs.path_prefix
        );
    }
    for (field, value) in [
        ("upstream", &defs.upstream),
        ("branch", &defs.branch),
        ("licence", &defs.licence),
    ] {
        if value.trim().is_empty() {
            bail!("corpus manifest: [defs] {field} is empty");
        }
    }
    let logger = &defs.logger_v370;
    if logger.file.contains('/') || !valid_rel_path(&logger.file) {
        bail!(
            "corpus manifest: [defs.logger_v370] file {:?} must be a single file name",
            logger.file
        );
    }
    if !is_lower_hex(&logger.sha256, 64) {
        bail!(
            "corpus manifest: [defs.logger_v370] sha256 {:?} is not 64 lowercase hex digits",
            logger.sha256
        );
    }
    if logger.size == 0 {
        bail!("corpus manifest: [defs.logger_v370] size is 0");
    }
    for (i, f) in defs.files.iter().enumerate() {
        if !valid_rel_path(&f.path) {
            bail!(
                "corpus manifest: [defs] {:?} is not a relative `/`-separated path",
                f.path
            );
        }
        if !is_lower_hex(&f.sha256, 64) {
            bail!(
                "corpus manifest: [defs] {}: sha256 {:?} is not 64 lowercase hex digits",
                f.path,
                f.sha256
            );
        }
        if let Some(prev) = i.checked_sub(1).and_then(|p| defs.files.get(p))
            && prev.path >= f.path
        {
            bail!(
                "corpus manifest: [defs] {} is not after {} (paths must be unique and sorted)",
                f.path,
                prev.path
            );
        }
    }
    Ok(())
}

fn validate_suspect(f: &RomFile, s: &Suspect) -> Result<()> {
    if s.reason.trim().is_empty() {
        bail!("corpus manifest: {}: suspect with an empty reason", f.path);
    }
    match (s.image_offset, s.image_size) {
        (None, None) => {}
        (Some(offset), Some(size)) => {
            if standard_size(size).is_none() {
                bail!(
                    "corpus manifest: {}: suspect image_size {size} is not a standard size",
                    f.path
                );
            }
            if offset.checked_add(size).is_none_or(|end| end > f.size) {
                bail!(
                    "corpus manifest: {}: suspect image {offset}+{size} runs past the file's {} bytes",
                    f.path,
                    f.size
                );
            }
        }
        _ => bail!(
            "corpus manifest: {}: suspect image_offset and image_size must be given together",
            f.path
        ),
    }
    Ok(())
}

fn is_lower_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A non-empty relative path of `/`-separated, non-empty components, none
/// of them `.` or `..`, with no `\` (so it means the same on every OS).
fn valid_rel_path(p: &str) -> bool {
    !p.is_empty()
        && !p.contains('\\')
        && p.split('/')
            .all(|c| !c.is_empty() && c != "." && c != ".." && !c.contains(':'))
}

/// The fields a file name encodes; absent where it does not parse.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameFields {
    /// See [`RomFile::filename_rom_id`].
    pub rom_id: Option<String>,
    /// See [`RomFile::year`].
    pub year: Option<u16>,
    /// See [`RomFile::year_end`].
    pub year_end: Option<u16>,
    /// See [`RomFile::region`].
    pub region: Option<String>,
    /// See [`RomFile::model`].
    pub model: Option<String>,
    /// See [`RomFile::trim`].
    pub trim: Option<String>,
}

/// Parses an upstream file name (see the module docs). Returns the fields
/// that parsed and, when parsing stopped early, why.
#[must_use]
pub fn parse_file_name(name: &str) -> (NameFields, Option<String>) {
    let mut out = NameFields::default();
    let Some(stem) = strip_hex_extension(name) else {
        return (out, Some("no `.hex` extension".to_owned()));
    };
    let mut parts = stem.split('-').peekable();

    match parts.next() {
        Some(id) if id.len() == 8 && id.bytes().all(|b| b.is_ascii_alphanumeric()) => {
            out.rom_id = Some(id.to_owned());
        }
        other => {
            return (
                out,
                Some(format!(
                    "ROM ID {:?} is not 8 ASCII letters and digits",
                    other.unwrap_or_default()
                )),
            );
        }
    }

    let Some(year) = parts.next().and_then(parse_year) else {
        return (
            out,
            Some("no 4-digit model year after the ROM ID".to_owned()),
        );
    };
    out.year = Some(year);
    // An optional end year: `2001-02` or `2007-2008`.
    if let Some(end) = parts
        .peek()
        .copied()
        .filter(|p| p.bytes().all(|b| b.is_ascii_digit()))
    {
        let end_year = match end.len() {
            2 => end.parse::<u16>().ok().map(|yy| year - year % 100 + yy),
            4 => parse_year(end),
            _ => None,
        };
        match end_year {
            Some(e) if e > year => out.year_end = Some(e),
            _ => {
                return (
                    out,
                    Some(format!(
                        "year range {year}-{end} does not end after it starts"
                    )),
                );
            }
        }
        parts.next();
    }

    match parts.next() {
        Some(r) if REGIONS.contains(&r) => out.region = Some(r.to_owned()),
        other => {
            return (
                out,
                Some(format!(
                    "region {:?} is not one of {}",
                    other.unwrap_or_default(),
                    REGIONS.join(", ")
                )),
            );
        }
    }

    if parts.peek() == Some(&"Subaru") {
        parts.next();
    }
    match parts.next() {
        Some(m) if !m.is_empty() => out.model = Some(m.to_owned()),
        _ => return (out, Some("no model after the region".to_owned())),
    }

    let trim: Vec<&str> = parts.collect();
    if !trim.is_empty() {
        if trim.iter().any(|t| t.is_empty()) {
            return (out, Some("empty part in the trim".to_owned()));
        }
        out.trim = Some(trim.join("-"));
    }
    (out, None)
}

fn strip_hex_extension(name: &str) -> Option<&str> {
    let dot = name.len().checked_sub(4)?;
    let (stem, ext) = (name.get(..dot)?, name.get(dot..)?);
    (ext.eq_ignore_ascii_case(".hex") && !stem.is_empty()).then_some(stem)
}

fn parse_year(s: &str) -> Option<u16> {
    let y = (s.len() == 4 && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse::<u16>().ok())
        .flatten()?;
    (1900..=2099).contains(&y).then_some(y)
}

/// Classifies an image from its bytes (see the module docs): `None` for a
/// clean image, else why it is suspect.
#[must_use]
pub fn classify(bytes: &[u8]) -> Option<Suspect> {
    let size = bytes.len() as u64;
    let suspect = |kind, reason| Suspect {
        kind,
        reason,
        image_offset: None,
        image_size: None,
    };
    if let Some(label) = standard_size(size) {
        return bytes
            .first()
            .filter(|&&b| bytes.iter().all(|&x| x == b))
            .map(|b| {
                suspect(
                    SuspectKind::Blank,
                    format!(
                        "size {size} is standard ({label}) but every byte is 0x{b:02X}: an erased or empty dump, not firmware"
                    ),
                )
            });
    }
    // `size {size} = <a> ± <b>, not a standard size: <classification>`.
    let head =
        |a: u64, op: char, b: u64| format!("size {size} = {a} {op} {b}, not a standard size");
    if bytes.is_empty() {
        return Some(suspect(
            SuspectKind::Unknown,
            "size 0, not a standard size: empty file".to_owned(),
        ));
    }

    let crlf = LineEnds::count(bytes);
    if crlf.bare_lf == 0
        && crlf.crlf > 0
        && let Some(label) = standard_size(size - crlf.crlf)
    {
        return Some(suspect(
            SuspectKind::CrlfExpansion,
            format!(
                "{}: CRLF expansion: all {} LF have a CR before them and none is bare; stripping the CR before each LF yields {}, a standard size ({label})",
                head(size - crlf.crlf, '+', crlf.crlf),
                crlf.crlf,
                size - crlf.crlf
            ),
        ));
    }
    let not_crlf = if crlf.bare_lf > 0 {
        format!(
            "not CRLF expansion ({} bare LF, {} CR-before-LF)",
            crlf.bare_lf, crlf.crlf
        )
    } else {
        format!(
            "not CRLF expansion (stripping its {} CR-before-LF yields {}, not a standard size)",
            crlf.crlf,
            size - crlf.crlf
        )
    };

    if let Some(chunks) = walk_chunks(bytes)
        && let Some(last) = chunks.last()
        && let Some(label) = standard_size(last.len)
    {
        let list: Vec<String> = chunks
            .iter()
            .map(|c| format!("{}({})", c.tag, c.len))
            .collect();
        return Some(Suspect {
            kind: SuspectKind::Container,
            reason: format!(
                "{}: tagged-chunk container, not a raw image: chunks {}; a {}-byte header, then the {} payload, a standard {}-byte image ({label}) ending the file; {not_crlf}",
                head(last.offset, '+', last.len),
                list.join(" "),
                last.offset,
                last.tag,
                last.len
            ),
            image_offset: Some(last.offset),
            image_size: Some(last.len),
        });
    }

    let (nearest, label) = STANDARD_SIZES
        .iter()
        .copied()
        .min_by_key(|(s, _)| s.abs_diff(size))
        .unwrap_or((0, ""));
    if nearest < size {
        let extra = size - nearest;
        let tail = bytes
            .get(usize::try_from(nearest).unwrap_or(usize::MAX)..)
            .unwrap_or_default();
        return Some(Suspect {
            kind: SuspectKind::TrailingData,
            reason: format!(
                "{}: trailing data: a standard {nearest}-byte prefix ({label}) then {extra} byte(s) of {}; {not_crlf}",
                head(nearest, '+', extra),
                describe_fill(tail)
            ),
            image_offset: Some(0),
            image_size: Some(nearest),
        });
    }
    Some(suspect(
        SuspectKind::Truncated,
        format!(
            "{}: truncated: {} byte(s) short of {nearest} ({label}); {} CR-before-LF, {} bare LF",
            head(nearest, '-', nearest - size),
            nearest - size,
            crlf.crlf,
            crlf.bare_lf
        ),
    ))
}

/// LF counts in an image: CR-LF pairs and LFs without a CR before them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LineEnds {
    crlf: u64,
    bare_lf: u64,
}

impl LineEnds {
    fn count(bytes: &[u8]) -> Self {
        let mut out = Self {
            crlf: 0,
            bare_lf: 0,
        };
        let mut prev = None;
        for &b in bytes {
            if b == b'\n' {
                if prev == Some(b'\r') {
                    out.crlf += 1;
                } else {
                    out.bare_lf += 1;
                }
            }
            prev = Some(b);
        }
        out
    }
}

/// One chunk of a tagged-chunk container.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Chunk {
    tag: String,
    /// Offset of the payload (after the 8-byte chunk header).
    offset: u64,
    len: u64,
}

/// Reads `bytes` as tagged chunks (four ASCII capitals or digits, then a
/// big-endian `u32` payload length, then the payload) covering the whole
/// file, as the two suspect corpus images are laid out. `None` unless there
/// are at least two chunks and they end exactly at the end of the file.
fn walk_chunks(bytes: &[u8]) -> Option<Vec<Chunk>> {
    const MAX_CHUNKS: usize = 64;
    let mut chunks = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        if chunks.len() == MAX_CHUNKS {
            return None;
        }
        let header = bytes.get(at..at.checked_add(8)?)?;
        let (tag, len) = header.split_at(4);
        if !tag
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        {
            return None;
        }
        let len = u32::from_be_bytes(len.try_into().ok()?);
        let offset = at + 8;
        let end = offset.checked_add(usize::try_from(len).ok()?)?;
        if end > bytes.len() {
            return None;
        }
        chunks.push(Chunk {
            tag: String::from_utf8_lossy(tag).into_owned(),
            offset: offset as u64,
            len: u64::from(len),
        });
        at = end;
    }
    (chunks.len() >= 2).then_some(chunks)
}

/// What a run of extra bytes looks like, for a reason.
fn describe_fill(tail: &[u8]) -> String {
    match tail.first() {
        Some(&b) if tail.iter().all(|&x| x == b) => format!("0x{b:02X} padding"),
        Some(_)
            if tail
                .iter()
                .all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace()) =>
        {
            "printable text".to_owned()
        }
        _ => "binary data".to_owned(),
    }
}

/// Groups of paths whose images have the same SHA-256 (sorted), for the
/// generator's report; these are not suspect.
#[must_use]
pub fn duplicate_hashes(manifest: &Manifest) -> Vec<Vec<&str>> {
    let mut by_hash: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for f in &manifest.roms.files {
        by_hash.entry(&f.sha256).or_default().push(&f.path);
    }
    by_hash
        .into_values()
        .filter(|paths| paths.len() > 1)
        .collect()
}

/// One image found in the mirror checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scanned {
    /// Path relative to the checkout, `/`-separated.
    pub path: String,
    /// Size in bytes.
    pub size: u64,
    /// Lowercase hex SHA-256.
    pub sha256: String,
    /// The image's classification ([`classify`]).
    pub suspect: Option<Suspect>,
}

/// Every corpus image in the checkout at `roms_dir` (see the module docs for
/// which files count), sorted by path, hashed and [`classify`]-ed. Each file
/// is read into memory, one at a time, and nothing is copied or written.
///
/// # Errors
///
/// An unreadable directory or file, a non-UTF-8 name, anything other than a
/// regular file or directory (a symlink, say), or a file over
/// [`MAX_IMAGE_BYTES`].
pub fn scan_checkout(roms_dir: &Path) -> Result<Vec<Scanned>> {
    let mut out = Vec::new();
    scan_dir(roms_dir, "", &mut out)?;
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn scan_dir(dir: &Path, rel: &str, out: &mut Vec<Scanned>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            bail!("{}: file name is not UTF-8", path.display());
        };
        if rel.is_empty() && (name == ".git" || NOT_CORPUS.contains(&name.as_str())) {
            continue;
        }
        let rel_path = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        let kind = entry
            .file_type()
            .with_context(|| format!("reading {}", path.display()))?;
        if kind.is_dir() {
            scan_dir(&path, &rel_path, out)?;
        } else if kind.is_file() {
            let len = entry
                .metadata()
                .with_context(|| format!("reading {}", path.display()))?
                .len();
            if len > MAX_IMAGE_BYTES {
                bail!(
                    "{}: {len} bytes, over the {MAX_IMAGE_BYTES}-byte limit for a ROM image",
                    path.display()
                );
            }
            let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            out.push(Scanned {
                path: rel_path,
                size: bytes.len() as u64,
                sha256: sha256_hex(&bytes),
                suspect: classify(&bytes),
            });
        } else {
            bail!(
                "{}: not a regular file or directory (symlinks are not followed)",
                path.display()
            );
        }
    }
    Ok(())
}

/// Size and lowercase hex SHA-256 of the file at `path`, read in chunks.
///
/// # Errors
///
/// The file cannot be read.
pub fn hash_file(path: &Path) -> Result<(u64, String)> {
    let mut file = fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("reading {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(buf.get(..n).unwrap_or_default());
        size += n as u64;
    }
    Ok((size, to_hex(&hasher.finalize())))
}

/// Lowercase hex SHA-256 of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    to_hex(&Sha256::digest(bytes))
}

fn to_hex(digest: &[u8]) -> String {
    digest.iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Parses `sha256sum` output (`<hex>  ./<path>` or `<hex>  <path>`, also the
/// binary-mode `<hex> *<path>`) into path to hash.
///
/// # Errors
///
/// A malformed line, a bad hash or path, or a path listed twice.
pub fn parse_sums(text: &str) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let Some((hash, rest)) = line.split_once(' ') else {
            bail!("SHA256SUMS line {}: expected `<sha256>  <path>`", n + 1);
        };
        let path = rest.strip_prefix([' ', '*']).unwrap_or(rest);
        let path = path.strip_prefix("./").unwrap_or(path);
        let hash = hash.to_ascii_lowercase();
        if !is_lower_hex(&hash, 64) || !valid_rel_path(path) {
            bail!("SHA256SUMS line {}: expected `<sha256>  <path>`", n + 1);
        }
        if out.insert(path.to_owned(), hash).is_some() {
            bail!("SHA256SUMS line {}: {path} listed twice", n + 1);
        }
    }
    Ok(out)
}

/// Every way `scanned` disagrees with the mirror's `SHA256SUMS`; empty when
/// they list the same files with the same hashes.
#[must_use]
pub fn compare_with_sums(scanned: &[Scanned], sums: &BTreeMap<String, String>) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen = BTreeMap::new();
    for s in scanned {
        seen.insert(s.path.as_str(), ());
        match sums.get(&s.path) {
            None => problems.push(format!("{}: in the checkout but not in SHA256SUMS", s.path)),
            Some(want) if *want != s.sha256 => problems.push(format!(
                "{}: sha256 {} in the checkout, {want} in SHA256SUMS",
                s.path, s.sha256
            )),
            Some(_) => {}
        }
    }
    for path in sums.keys().filter(|p| !seen.contains_key(p.as_str())) {
        problems.push(format!("{path}: in SHA256SUMS but not in the checkout"));
    }
    problems
}

/// A file whose name did not fully parse, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameIssue {
    /// The entry's path.
    pub path: String,
    /// Why parsing stopped.
    pub reason: String,
}

/// The manifest for `scanned` (already sorted by path), plus every file name
/// that did not fully parse.
#[must_use]
pub fn build(scanned: &[Scanned]) -> (Manifest, Vec<NameIssue>) {
    let mut issues = Vec::new();
    let files = scanned
        .iter()
        .map(|s| {
            let name = s.path.rsplit('/').next().unwrap_or(&s.path);
            let (fields, issue) = parse_file_name(name);
            if let Some(reason) = issue {
                issues.push(NameIssue {
                    path: s.path.clone(),
                    reason,
                });
            }
            RomFile {
                path: s.path.clone(),
                size: s.size,
                sha256: s.sha256.clone(),
                filename_rom_id: fields.rom_id,
                year: fields.year,
                year_end: fields.year_end,
                region: fields.region,
                model: fields.model,
                trim: fields.trim,
                suspect: s.suspect.clone(),
            }
        })
        .collect();
    let manifest = Manifest {
        schema: SCHEMA,
        roms: RomCorpus {
            upstream: UPSTREAM_URL.to_owned(),
            commit: PIN_COMMIT.to_owned(),
            path_prefix: PATH_PREFIX.to_owned(),
            files,
        },
        defs: None,
    };
    (manifest, issues)
}

/// The commit checked out at `dir` (`git rev-parse HEAD`).
///
/// # Errors
///
/// git cannot run or `dir` is not a git checkout.
pub fn checkout_head(dir: &Path) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .context("cannot run git")?;
    if !out.status.success() {
        bail!(
            "{}: `git rev-parse HEAD` failed: {}",
            dir.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// The result of generating from a mirror.
#[derive(Debug)]
pub struct Generated {
    /// The manifest.
    pub manifest: Manifest,
    /// Its canonical bytes ([`Manifest::render`]).
    pub text: String,
    /// File names that did not fully parse.
    pub issues: Vec<NameIssue>,
}

/// Fails unless the ROM mirror at `mirror` has its `roms/` checkout (module
/// docs); [`generate`] checks it first, so a wrong ROM mirror is reported
/// before anything about the definitions mirror.
///
/// # Errors
///
/// There is no such checkout.
pub fn require_roms_checkout(mirror: &Path) -> Result<()> {
    if !mirror.join(MIRROR_ROMS_DIR).is_dir() {
        bail!(
            "{}: no `{MIRROR_ROMS_DIR}/` checkout (expected a mirror laid out as described in xtask/src/corpus_manifest.rs)",
            mirror.display()
        );
    }
    Ok(())
}

/// Fails unless the definitions mirror at `defs_mirror` has its
/// `mirror.git/` (module docs), so a wrong path is reported before any
/// hashing.
///
/// # Errors
///
/// There is no such directory.
pub fn require_defs_mirror(defs_mirror: &Path) -> Result<()> {
    if !defs_mirror.join(DEFS_MIRROR_GIT).is_dir() {
        bail!(
            "{}: no `{DEFS_MIRROR_GIT}/` (expected a definitions mirror laid out as described in xtask/src/corpus_manifest.rs)",
            defs_mirror.display()
        );
    }
    Ok(())
}

/// Generates the manifest from the ROM mirror at `mirror` and the
/// definitions mirror at `defs_mirror` (module docs): checks the ROM
/// checkout is at [`PIN_COMMIT`], hashes every image and every definition
/// file (raw blobs at [`DEFS_PIN_COMMIT`]), and requires exact agreement
/// with each mirror's `SHA256SUMS`.
///
/// # Errors
///
/// A checkout or mirror is missing or not at its pin, a read fails, or the
/// hashes disagree with a `SHA256SUMS` (every disagreement is listed).
pub fn generate(mirror: &Path, defs_mirror: &Path) -> Result<Generated> {
    require_roms_checkout(mirror)?;
    let roms = mirror.join(MIRROR_ROMS_DIR);
    let sums_path = mirror.join(MIRROR_SUMS_FILE);
    let head = checkout_head(&roms)?;
    if head != PIN_COMMIT {
        bail!(
            "{}: checkout is at {head}, not the pinned upstream commit {PIN_COMMIT}",
            roms.display()
        );
    }
    let sums_text = fs::read_to_string(&sums_path)
        .with_context(|| format!("reading {}", sums_path.display()))?;
    let sums = parse_sums(&sums_text)?;
    let scanned = scan_checkout(&roms)?;
    agree_with_sums(&roms, &sums_path, &scanned, &sums)?;
    let (mut manifest, issues) = build(&scanned);
    manifest.defs = Some(generate_defs(defs_mirror)?);
    manifest.validate()?;
    let text = manifest.render()?;
    Ok(Generated {
        manifest,
        text,
        issues,
    })
}

fn agree_with_sums(
    what: &Path,
    sums_path: &Path,
    scanned: &[Scanned],
    sums: &BTreeMap<String, String>,
) -> Result<()> {
    let problems = compare_with_sums(scanned, sums);
    if problems.is_empty() {
        return Ok(());
    }
    let mut msg = format!(
        "{} disagree(s) with {}:",
        what.display(),
        sums_path.display()
    );
    for p in &problems {
        let _ = write!(msg, "\n  {p}");
    }
    bail!(msg)
}

/// Every file under [`DEFS_PATH_PREFIX`] in the bare repository `git_dir`
/// at [`DEFS_PIN_COMMIT`] ([`scan_tree`]).
///
/// # Errors
///
/// As [`scan_tree`].
pub fn scan_defs_mirror(git_dir: &Path) -> Result<Vec<Scanned>> {
    scan_tree(git_dir, DEFS_PIN_COMMIT, DEFS_PATH_PREFIX).with_context(|| {
        format!(
            "{} must be a mirror of {DEFS_UPSTREAM_URL} that has {DEFS_PIN_COMMIT}",
            git_dir.display()
        )
    })
}

/// Every file under `prefix` (empty, or ending in `/`) in the bare
/// repository `git_dir` at `commit`, read as raw blobs (never a checkout, so
/// no attribute or `core.autocrlf` can change a byte), hashed, and sorted by
/// its path relative to `prefix`. Paths are kept exactly as git has them,
/// spaces included.
///
/// # Errors
///
/// git fails or the repository lacks the commit; nothing is under
/// `prefix`; or something there is a symlink, a submodule or a path the
/// manifest cannot hold.
pub fn scan_tree(git_dir: &Path, commit: &str, prefix: &str) -> Result<Vec<Scanned>> {
    let entries = crate::fetch_corpus::list_tree(git_dir, commit)?;
    let mut wanted = Vec::new();
    for (path, blob) in &entries {
        let Some(rel) = path.strip_prefix(prefix) else {
            continue;
        };
        let Some(blob) = blob else {
            bail!(
                "{}: {path:?} at {commit} is not a regular file (a symlink or submodule)",
                git_dir.display()
            );
        };
        if !valid_rel_path(rel) {
            bail!(
                "{}: {path:?} at {commit} is not a path the manifest can hold",
                git_dir.display()
            );
        }
        wanted.push((rel, blob));
    }
    if wanted.is_empty() {
        bail!(
            "{}: nothing under `{prefix}` at {commit}",
            git_dir.display()
        );
    }
    let mut out = Vec::with_capacity(wanted.len());
    crate::fetch_corpus::stream_blobs(git_dir, &wanted, |i, reader| {
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut size = 0u64;
        loop {
            let n = reader
                .read(&mut buf)
                .context("reading git cat-file output")?;
            if n == 0 {
                break;
            }
            hasher.update(buf.get(..n).unwrap_or_default());
            size += n as u64;
        }
        let Some((path, _)) = wanted.get(i) else {
            bail!("git cat-file: unexpected blob {i}");
        };
        out.push(Scanned {
            path: (*path).to_owned(),
            size,
            sha256: to_hex(&hasher.finalize()),
            suspect: None,
        });
        Ok(())
    })?;
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// The `[defs]` table from the definitions mirror at `defs_mirror` (module
/// docs), with the logger package pinned by the constants (it is never in a
/// mirror this tool reads).
///
/// # Errors
///
/// No `mirror.git/`, [`scan_defs_mirror`] fails, or the files disagree with
/// the mirror's `SHA256SUMS`.
pub fn generate_defs(defs_mirror: &Path) -> Result<DefsCorpus> {
    require_defs_mirror(defs_mirror)?;
    let git_dir = defs_mirror.join(DEFS_MIRROR_GIT);
    let sums_path = defs_mirror.join(MIRROR_SUMS_FILE);
    let sums_text = fs::read_to_string(&sums_path)
        .with_context(|| format!("reading {}", sums_path.display()))?;
    let sums = parse_sums(&sums_text)?;
    let scanned = scan_defs_mirror(&git_dir)?;
    agree_with_sums(&git_dir, &sums_path, &scanned, &sums)?;
    Ok(DefsCorpus {
        upstream: DEFS_UPSTREAM_URL.to_owned(),
        branch: DEFS_BRANCH.to_owned(),
        commit: DEFS_PIN_COMMIT.to_owned(),
        path_prefix: DEFS_PATH_PREFIX.to_owned(),
        licence: DEFS_LICENCE.to_owned(),
        logger_v370: LoggerPackage {
            file: LOGGER_V370_FILE.to_owned(),
            size: LOGGER_V370_SIZE,
            sha256: LOGGER_V370_SHA256.to_owned(),
            url: LOGGER_V370_URL.to_owned(),
            note: LOGGER_V370_NOTE.to_owned(),
        },
        files: scanned
            .into_iter()
            .map(|s| DefFile {
                path: s.path,
                size: s.size,
                sha256: s.sha256,
            })
            .collect(),
    })
}

/// Why the committed manifest text differs from the generated one; empty
/// when they are byte-identical. Names entries where possible.
#[must_use]
pub fn compare_committed(generated: &Generated, committed: &str) -> Vec<String> {
    if committed == generated.text {
        return Vec::new();
    }
    let old = match Manifest::from_toml(committed) {
        Ok(m) => m,
        Err(e) => return vec![format!("committed manifest does not parse: {e:#}")],
    };
    let new = &generated.manifest;
    let mut problems = Vec::new();
    if old.schema != new.schema {
        problems.push(format!("schema {} (generated {})", old.schema, new.schema));
    }
    for (field, a, b) in [
        ("upstream", &old.roms.upstream, &new.roms.upstream),
        ("commit", &old.roms.commit, &new.roms.commit),
        ("path_prefix", &old.roms.path_prefix, &new.roms.path_prefix),
    ] {
        if a != b {
            problems.push(format!("[roms] {field} {a:?} (generated {b:?})"));
        }
    }
    let old_files: BTreeMap<&str, &RomFile> = old
        .roms
        .files
        .iter()
        .map(|f| (f.path.as_str(), f))
        .collect();
    let new_files: BTreeMap<&str, &RomFile> = new
        .roms
        .files
        .iter()
        .map(|f| (f.path.as_str(), f))
        .collect();
    for (path, want) in &new_files {
        match old_files.get(path) {
            None => problems.push(format!("{path}: missing from the committed manifest")),
            Some(got) if got != want => {
                problems.push(format!(
                    "{path}: committed entry differs from the generated one"
                ));
            }
            Some(_) => {}
        }
    }
    for path in old_files.keys().filter(|p| !new_files.contains_key(*p)) {
        problems.push(format!(
            "{path}: in the committed manifest but not in the mirror"
        ));
    }
    compare_defs(old.defs.as_ref(), new.defs.as_ref(), &mut problems);
    if problems.is_empty() {
        let hint = if committed.contains('\r') {
            " (it has \\r\\n line endings: check fixtures/.gitattributes and core.autocrlf)"
        } else {
            ""
        };
        problems.push(format!(
            "same entries, but the committed bytes are not the canonical rendering{hint}"
        ));
    }
    problems
}

fn compare_defs(old: Option<&DefsCorpus>, new: Option<&DefsCorpus>, problems: &mut Vec<String>) {
    let (old, new) = match (old, new) {
        (None, None) => return,
        (None, Some(_)) => {
            problems.push("[defs]: missing from the committed manifest".to_owned());
            return;
        }
        (Some(_), None) => {
            problems.push("[defs]: in the committed manifest but not generated".to_owned());
            return;
        }
        (Some(old), Some(new)) => (old, new),
    };
    for (field, a, b) in [
        ("upstream", &old.upstream, &new.upstream),
        ("branch", &old.branch, &new.branch),
        ("commit", &old.commit, &new.commit),
        ("path_prefix", &old.path_prefix, &new.path_prefix),
        ("licence", &old.licence, &new.licence),
    ] {
        if a != b {
            problems.push(format!("[defs] {field} {a:?} (generated {b:?})"));
        }
    }
    if old.logger_v370 != new.logger_v370 {
        problems.push("[defs.logger_v370] differs from the generated one".to_owned());
    }
    let old_files: BTreeMap<&str, &DefFile> =
        old.files.iter().map(|f| (f.path.as_str(), f)).collect();
    let new_files: BTreeMap<&str, &DefFile> =
        new.files.iter().map(|f| (f.path.as_str(), f)).collect();
    for (path, want) in &new_files {
        match old_files.get(path) {
            None => problems.push(format!(
                "[defs] {path}: missing from the committed manifest"
            )),
            Some(got) if got != want => problems.push(format!(
                "[defs] {path}: committed entry differs from the generated one"
            )),
            Some(_) => {}
        }
    }
    for path in old_files.keys().filter(|p| !new_files.contains_key(*p)) {
        problems.push(format!(
            "[defs] {path}: in the committed manifest but not in the mirror"
        ));
    }
}

/// The definitions mirror directory: `--defs-mirror`, else
/// [`DEFS_MIRROR_ENV`].
///
/// # Errors
///
/// Neither is set.
pub fn defs_mirror_dir(arg: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(dir) = arg {
        return Ok(dir);
    }
    match std::env::var_os(DEFS_MIRROR_ENV) {
        Some(v) if !v.is_empty() => Ok(PathBuf::from(v)),
        _ => bail!(
            "no definitions mirror: pass --defs-mirror <DIR> or set {DEFS_MIRROR_ENV} (a directory holding `{DEFS_MIRROR_GIT}/`, a bare mirror of {DEFS_UPSTREAM_URL} with {DEFS_PIN_COMMIT}, and `{MIRROR_SUMS_FILE}`)"
        ),
    }
}

/// The mirror directory: `--mirror`, else [`MIRROR_ENV`].
///
/// # Errors
///
/// Neither is set.
pub fn mirror_dir(arg: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(dir) = arg {
        return Ok(dir);
    }
    match std::env::var_os(MIRROR_ENV) {
        Some(v) if !v.is_empty() => Ok(PathBuf::from(v)),
        _ => bail!(
            "no corpus mirror: pass --mirror <DIR> or set {MIRROR_ENV} (a directory holding `{MIRROR_ROMS_DIR}/`, a checkout of {UPSTREAM_URL} at {PIN_COMMIT}, and `{MIRROR_SUMS_FILE}`)"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(name: &str) -> NameFields {
        let (f, issue) = parse_file_name(name);
        assert_eq!(issue, None, "{name}");
        f
    }

    fn some(s: &str) -> Option<String> {
        Some(s.to_owned())
    }

    #[test]
    fn parses_the_common_shape() {
        assert_eq!(
            fields("A2WC400H-2005-ADM-Subaru-Forester-XT-AT.hex"),
            NameFields {
                rom_id: some("A2WC400H"),
                year: Some(2005),
                year_end: None,
                region: some("ADM"),
                model: some("Forester"),
                trim: some("XT-AT"),
            }
        );
    }

    #[test]
    fn make_is_optional_and_trim_may_be_absent() {
        let f = fields("A2WD002T-2005-ADM-Liberty-GT.hex");
        assert_eq!(
            (f.region, f.model, f.trim),
            (some("ADM"), some("Liberty"), some("GT"))
        );
        let f = fields("A2UJ000J-2007-USDM-Subaru-Impreza.hex");
        assert_eq!((f.model, f.trim), (some("Impreza"), None));
    }

    #[test]
    fn year_ranges_two_and_four_digit() {
        let f = fields("A4RG050N-2001-02-JDM-Subaru-Impreza-STi-MT-AT.hex");
        assert_eq!((f.year, f.year_end), (Some(2001), Some(2002)));
        assert_eq!(f.trim, some("STi-MT-AT"));
        let f = fields("AZ1G101M-2009-10-EDM-Subaru-Forester-XT-MT.hex");
        assert_eq!((f.year, f.year_end), (Some(2009), Some(2010)));
        let f = fields("A8DK100I-2007-2008-USDM-Forester-XT.hex");
        assert_eq!(
            (f.year, f.year_end, f.region, f.model),
            (Some(2007), Some(2008), some("USDM"), some("Forester"))
        );
    }

    #[test]
    fn extension_case_is_ignored_and_names_kept_as_written() {
        let f = fields("A4RL100J-2003-EDM-Forester-Turbo.HEX");
        assert_eq!(f.rom_id, some("A4RL100J"));
        let f = fields("AE5K500V-2012-USDM-Subaru-Impreza-STI.Hex");
        assert_eq!(f.trim, some("STI"));
        let f = fields("EP5G401z-2013-EDM-Subaru-XVCVT.hex");
        assert_eq!((f.rom_id, f.model), (some("EP5G401z"), some("XVCVT")));
        let f = fields("A2TB001C-2008-USDM-Subaru-Legacy(Outback)-GT(XT)-AT.hex");
        assert_eq!(
            (f.model, f.trim),
            (some("Legacy(Outback)"), some("GT(XT)-AT"))
        );
        let f = fields("A8DH100E-2006-SADM-Subaru-Forester-XT-AT.hex");
        assert_eq!(f.region, some("SADM"));
    }

    #[test]
    fn unparsed_parts_are_absent_and_reported() {
        // A doubled ROM ID: nothing after it can be anchored, so nothing is kept.
        let (f, issue) = parse_file_name("A2WC50-A2WC50ZU-2004-USDM-Subaru-Baja-XT-AT.hex");
        assert_eq!(f, NameFields::default());
        assert!(issue.is_some_and(|r| r.contains("\"A2WC50\"")));

        // A placeholder year: the ROM ID is kept, the rest is not guessed.
        let (f, issue) = parse_file_name("A4TJ1X00-20xx-JDM-Subaru-Impreza-STi-MT-AT.hex");
        assert_eq!(
            f,
            NameFields {
                rom_id: some("A4TJ1X00"),
                ..NameFields::default()
            }
        );
        assert!(issue.is_some_and(|r| r.contains("model year")));

        let (f, issue) = parse_file_name("A2WC400H-2005-XDM-Subaru-Forester.hex");
        assert_eq!((f.year, f.region), (Some(2005), None));
        assert!(issue.is_some_and(|r| r.contains("\"XDM\"")));

        let (f, issue) = parse_file_name("A2WC400H-2005-06-ADM-Subaru.hex");
        assert_eq!(
            (f.year_end, f.region, f.model),
            (Some(2006), some("ADM"), None)
        );
        assert!(issue.is_some_and(|r| r.contains("no model")));

        let (_, issue) = parse_file_name("A2WC400H-2005-04-ADM-Forester.hex");
        assert!(issue.is_some_and(|r| r.contains("does not end after")));

        let (f, issue) = parse_file_name("A2WC400H-2005-ADM-Forester.bin");
        assert_eq!(f, NameFields::default());
        assert!(issue.is_some_and(|r| r.contains(".hex")));

        assert!(parse_file_name(".hex").1.is_some());
        assert!(
            parse_file_name("A2WC400H-2005-ADM-Forester--AT.hex")
                .1
                .is_some()
        );
        assert!(
            parse_file_name("A2WC400H-1899-ADM-Forester.hex")
                .1
                .is_some()
        );
    }

    fn file(path: &str, sha_digit: char) -> RomFile {
        let name = path.rsplit('/').next().unwrap_or(path);
        let (f, _) = parse_file_name(name);
        RomFile {
            path: path.to_owned(),
            size: 524_288,
            sha256: std::iter::repeat_n(sha_digit, 64).collect(),
            filename_rom_id: f.rom_id,
            year: f.year,
            year_end: f.year_end,
            region: f.region,
            model: f.model,
            trim: f.trim,
            suspect: None,
        }
    }

    fn sample() -> Manifest {
        let mut odd = file("JDM/Legacy/AE5I410A-2011-JDM-Subaru-Legacy-GT.hex", 'b');
        odd.size = 1_048_731;
        odd.suspect = Some(Suspect {
            kind: SuspectKind::Container,
            reason: "size 1048731 = 155 + 1048576, not a standard size: container".to_owned(),
            image_offset: Some(155),
            image_size: Some(1_048_576),
        });
        Manifest {
            schema: SCHEMA,
            roms: RomCorpus {
                upstream: UPSTREAM_URL.to_owned(),
                commit: PIN_COMMIT.to_owned(),
                path_prefix: PATH_PREFIX.to_owned(),
                files: vec![
                    file(
                        "ADM/Forester/A2WC400H-2005-ADM-Subaru-Forester-XT-AT.hex",
                        'a',
                    ),
                    odd,
                    file(
                        "USDM/Baja/A2WC50-A2WC50ZU-2004-USDM-Subaru-Baja-XT-AT.hex",
                        'c',
                    ),
                ],
            },
            defs: None,
        }
    }

    #[test]
    fn manifest_round_trips_through_its_rendering() -> Result<()> {
        let m = sample();
        let text = m.render()?;
        assert!(text.starts_with("# External stock-ROM and ECU-definition corpus manifest"));
        assert!(!text.contains('\r'));
        let back = Manifest::from_toml(&text)?;
        assert_eq!(back, m);
        assert_eq!(back.render()?, text, "rendering is canonical");
        // Absent fields are omitted, not written as empty values.
        let baja = text
            .split("[[roms.files]]")
            .find(|s| s.contains("A2WC50ZU"))
            .unwrap_or_default();
        assert!(!baja.contains("filename_rom_id"), "{baja}");
        assert!(
            text.contains("[roms.files.suspect]\nkind = \"container\"\nreason = \"size 1048731"),
            "{text}"
        );
        assert!(
            text.contains("image_offset = 155\nimage_size = 1048576\n"),
            "{text}"
        );
        Ok(())
    }

    #[test]
    fn upstream_path_prefixes_the_entry_path() {
        let mut m = sample();
        let f = m.roms.files[0].clone();
        assert_eq!(m.roms.upstream_path(&f), f.path);
        m.roms.path_prefix = "roms/".to_owned();
        assert_eq!(m.roms.upstream_path(&f), format!("roms/{}", f.path));
    }

    #[test]
    fn validation_rejects_bad_manifests() -> Result<()> {
        let ok = sample().render()?;
        let cases: [(&str, String); 8] = [
            ("schema", ok.replace("schema = 1", "schema = 2")),
            (
                "unknown field",
                ok.replace("schema = 1", "schema = 1\nextra = 1"),
            ),
            ("short commit", ok.replace(PIN_COMMIT, "639f3c73")),
            ("bad hash", ok.replacen(&"a".repeat(64), &"A".repeat(64), 1)),
            (
                "abs path",
                ok.replace("\"ADM/Forester/", "\"/ADM/Forester/"),
            ),
            (
                "dotdot path",
                ok.replace("\"ADM/Forester/", "\"../Forester/"),
            ),
            (
                "prefix",
                ok.replace("path_prefix = \"\"", "path_prefix = \"roms\""),
            ),
            (
                "unsorted",
                ok.replace("ADM/Forester/A2WC", "ZZZ/Forester/A2WC"),
            ),
        ];
        for (what, text) in cases {
            assert!(Manifest::from_toml(&text).is_err(), "{what} accepted");
        }
        let dup = ok.replace(
            "USDM/Baja/A2WC50-A2WC50ZU-2004-USDM-Subaru-Baja-XT-AT.hex",
            "JDM/Legacy/AE5I410A-2011-JDM-Subaru-Legacy-GT.hex",
        );
        assert!(Manifest::from_toml(&dup).is_err(), "duplicate accepted");
        Ok(())
    }

    #[test]
    fn sums_parse_and_compare() -> Result<()> {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let sums = parse_sums(&format!(
            "{a}  ./X/one.hex\n{}  Y/two.hex\r\n\n",
            b.to_uppercase()
        ))?;
        assert_eq!(sums.get("X/one.hex"), Some(&a));
        assert_eq!(sums.get("Y/two.hex"), Some(&b));
        assert!(
            parse_sums(&format!("{a}  ./x\n{a}  x\n")).is_err(),
            "duplicate"
        );
        assert!(parse_sums("abc  x\n").is_err(), "short hash");
        assert!(
            parse_sums(&format!("{a}  ../x\n")).is_err(),
            "escaping path"
        );

        let scanned = |path: &str, sha: &str| Scanned {
            path: path.to_owned(),
            size: 1,
            sha256: sha.to_owned(),
            suspect: None,
        };
        assert!(
            compare_with_sums(&[scanned("X/one.hex", &a), scanned("Y/two.hex", &b)], &sums)
                .is_empty()
        );
        let p = compare_with_sums(&[scanned("X/one.hex", &b), scanned("Z/new.hex", &a)], &sums);
        assert_eq!(p.len(), 3, "{p:?}");
        assert!(p[0].starts_with("X/one.hex: sha256"));
        assert!(p[1].starts_with("Z/new.hex: in the checkout but not"));
        assert!(p[2].starts_with("Y/two.hex: in SHA256SUMS but not"));
        Ok(())
    }

    #[test]
    fn scan_skips_git_and_top_level_docs_and_hashes_the_rest() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("rr-corpus-scan-unit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".git"))?;
        fs::create_dir_all(dir.join("ADM/Forester"))?;
        fs::write(dir.join(".git/HEAD"), b"ref")?;
        fs::write(dir.join("README.md"), b"docs")?;
        fs::write(dir.join(".gitattributes"), b"* text=auto")?;
        fs::write(dir.join("ADM/Forester/B.hex"), b"abc")?;
        fs::write(dir.join("ADM/Forester/A.hex"), b"")?;
        // A README below the top is not exempt.
        fs::write(dir.join("ADM/README.md"), b"x")?;
        let got = scan_checkout(&dir)?;
        fs::remove_dir_all(&dir)?;
        let paths: Vec<&str> = got.iter().map(|s| s.path.as_str()).collect();
        assert_eq!(
            paths,
            ["ADM/Forester/A.hex", "ADM/Forester/B.hex", "ADM/README.md"]
        );
        assert_eq!(got[1].size, 3);
        assert_eq!(
            got[1].suspect.as_ref().map(|s| s.kind),
            Some(SuspectKind::Truncated)
        );
        assert_eq!(
            got[1].sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            got[0].sha256,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        Ok(())
    }

    #[test]
    fn build_records_name_issues_and_copies_the_classification() {
        let scanned: Vec<Scanned> = [
            "ADM/Forester/A2WC400H-2005-ADM-Subaru-Forester-XT-AT.hex",
            "USDM/Baja/A2WC50-A2WC50ZU-2004-USDM-Subaru-Baja-XT-AT.hex",
        ]
        .iter()
        .map(|p| Scanned {
            path: (*p).to_owned(),
            size: 1,
            sha256: "0".repeat(64),
            suspect: None,
        })
        .collect();
        let mut scanned = scanned;
        scanned[1].suspect = classify(&[]);
        let (m, issues) = build(&scanned);
        assert_eq!(m.roms.files.len(), 2);
        assert!(!m.roms.files[0].is_suspect());
        assert_eq!(m.roms.files[1].suspect, classify(&[]));
        assert_eq!(m.roms.files[0].model, some("Forester"));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].path, scanned[1].path);
        assert!(m.validate().is_ok());
    }
    /// Deterministic noise standing in for firmware: synthetic, never a ROM.
    fn noise(len: usize, seed: u64) -> Vec<u8> {
        let mut x = seed | 1;
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x.to_le_bytes()[0]
            })
            .collect()
    }

    fn crlf_expand(bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(bytes.len() + bytes.len() / 128);
        for &b in bytes {
            if b == b'\n' {
                out.push(b'\r');
            }
            out.push(b);
        }
        out
    }

    fn kind(bytes: &[u8]) -> Option<SuspectKind> {
        classify(bytes).map(|s| s.kind)
    }

    #[test]
    fn standard_sizes_are_clean_unless_blank() {
        for &(size, _) in STANDARD_SIZES {
            let len = usize::try_from(size).unwrap_or(0);
            assert_eq!(classify(&noise(len, size)), None, "{size}");
            for fill in [0x00, 0xFF] {
                let s = classify(&vec![fill; len]);
                assert_eq!(
                    s.as_ref().map(|s| s.kind),
                    Some(SuspectKind::Blank),
                    "{size}"
                );
                let reason = s.map(|s| s.reason).unwrap_or_default();
                assert!(
                    reason.contains(&format!("every byte is 0x{fill:02X}")),
                    "{reason}"
                );
            }
        }
        // The table is what the module docs cite: 160/192 KiB and 1.25/1.5 MiB
        // are standard although not powers of two.
        let sizes: Vec<u64> = STANDARD_SIZES.iter().map(|(s, _)| *s).collect();
        assert_eq!(
            sizes,
            [
                163_840, 196_608, 524_288, 1_048_576, 1_310_720, 1_572_864, 2_097_152
            ]
        );
        assert_eq!(standard_size(1_048_731), None);
    }

    #[test]
    fn crlf_expansion_is_detected_and_counted() {
        let mut image = noise(163_840, 7);
        // Make sure the original already holds a CRLF pair: it becomes CR CR LF.
        image[100] = b'\r';
        image[101] = b'\n';
        let lfs = image.iter().filter(|&&b| b == b'\n').count();
        assert!(lfs > 100, "{lfs}");
        let mangled = crlf_expand(&image);
        assert_eq!(mangled.len(), 163_840 + lfs);
        let s = classify(&mangled);
        assert_eq!(s.as_ref().map(|s| s.kind), Some(SuspectKind::CrlfExpansion));
        let reason = s.map(|s| s.reason).unwrap_or_default();
        assert!(
            reason.starts_with(&format!(
                "size {} = 163840 + {lfs}, not a standard size: CRLF expansion",
                mangled.len()
            )),
            "{reason}"
        );
        assert!(reason.contains("yields 163840"), "{reason}");

        // One bare LF left behind: not a clean expansion, so trailing data.
        let mut partly = mangled.clone();
        let lf = partly.iter().rposition(|&b| b == b'\n').unwrap_or(1);
        partly[lf - 1] = 0x00;
        assert_eq!(kind(&partly), Some(SuspectKind::TrailingData));
        let reason = classify(&partly).map(|s| s.reason).unwrap_or_default();
        assert!(reason.contains("not CRLF expansion (1 bare LF"), "{reason}");
    }

    fn chunk(tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = tag.to_vec();
        out.extend_from_slice(&u32::try_from(payload.len()).unwrap_or(0).to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn a_chunk_container_around_a_standard_image_is_classified() {
        // Laid out like the corpus's 1,048,731-byte image (synthetic bytes).
        let image = noise(1_048_576, 3);
        let file = [
            chunk(b"INFO", &noise(39, 4)),
            chunk(b"DRMI", &noise(48, 5)),
            chunk(b"MEML", &noise(36, 6)),
            chunk(b"MEMD", &image),
        ]
        .concat();
        assert_eq!(file.len(), 1_048_731);
        let s = classify(&file).unwrap_or_else(|| panic!("clean"));
        assert_eq!(s.kind, SuspectKind::Container);
        assert_eq!((s.image_offset, s.image_size), (Some(155), Some(1_048_576)));
        assert!(
            s.reason.starts_with(
                "size 1048731 = 155 + 1048576, not a standard size: tagged-chunk container"
            ),
            "{}",
            s.reason
        );
        assert!(
            s.reason
                .contains("chunks INFO(39) DRMI(48) MEML(36) MEMD(1048576)"),
            "{}",
            s.reason
        );
        assert!(s.reason.contains("not CRLF expansion ("), "{}", s.reason);
        assert_eq!(file.get(155..), Some(image.as_slice()));

        // Chunks that do not end at the end of the file are not a container.
        let mut longer = file.clone();
        longer.push(0);
        assert_eq!(kind(&longer), Some(SuspectKind::TrailingData));
        // Nor are chunks whose last payload is not a standard size.
        let odd = [
            chunk(b"INFO", &noise(39, 4)),
            chunk(b"MEMD", &noise(1_048_000, 3)),
        ]
        .concat();
        assert_ne!(kind(&odd), Some(SuspectKind::Container));
        // Nor a lower-case tag.
        let lower = [chunk(b"info", &noise(139, 4)), chunk(b"MEMD", &image)].concat();
        assert_eq!(lower.len(), 1_048_731);
        assert_eq!(kind(&lower), Some(SuspectKind::TrailingData));
    }

    #[test]
    fn trailing_data_is_measured_and_described() {
        let image = noise(524_288, 9);
        for (tail, what) in [
            (vec![0xFF; 100], "0xFF padding"),
            (vec![0x00; 7], "0x00 padding"),
            (b"saved by tool v1.2\r\n".to_vec(), "printable text"),
            (vec![1, 2, 3, 200], "binary data"),
        ] {
            let file = [image.clone(), tail.clone()].concat();
            let s = classify(&file).unwrap_or_else(|| panic!("clean"));
            assert_eq!(s.kind, SuspectKind::TrailingData);
            assert_eq!((s.image_offset, s.image_size), (Some(0), Some(524_288)));
            assert!(
                s.reason.starts_with(&format!(
                    "size {} = 524288 + {}, not a standard size: trailing data",
                    file.len(),
                    tail.len()
                )),
                "{}",
                s.reason
            );
            assert!(s.reason.contains(&format!("of {what};")), "{}", s.reason);
        }
    }

    #[test]
    fn short_and_empty_files_are_truncated_or_unknown() {
        let s = classify(&noise(1_048_576 - 10, 2)).unwrap_or_else(|| panic!("clean"));
        assert_eq!(s.kind, SuspectKind::Truncated);
        assert!(
            s.reason
                .starts_with("size 1048566 = 1048576 - 10, not a standard size: truncated"),
            "{}",
            s.reason
        );
        assert_eq!((s.image_offset, s.image_size), (None, None));
        assert_eq!(kind(&[0x5A]), Some(SuspectKind::Truncated));
        assert_eq!(kind(&[]), Some(SuspectKind::Unknown));
    }

    #[test]
    fn every_nonstandard_size_gets_a_specific_reason() {
        for len in [
            1, 1000, 163_841, 196_607, 300_000, 1_048_731, 1_048_784, 2_200_000,
        ] {
            let s = classify(&noise(len, len as u64)).unwrap_or_else(|| panic!("{len} clean"));
            assert!(
                s.reason.starts_with(&format!("size {len} = ")),
                "{}",
                s.reason
            );
            assert_ne!(s.kind, SuspectKind::Blank);
        }
    }

    #[test]
    fn accessors_split_clean_from_suspect() {
        let m = sample();
        let clean: Vec<&str> = m.roms.clean_files().map(|f| f.path.as_str()).collect();
        assert_eq!(clean.len(), 2);
        let suspect: Vec<(&str, SuspectKind)> = m
            .roms
            .suspect_files()
            .map(|(f, s)| (f.path.as_str(), s.kind))
            .collect();
        assert_eq!(
            suspect,
            [(
                "JDM/Legacy/AE5I410A-2011-JDM-Subaru-Legacy-GT.hex",
                SuspectKind::Container
            )]
        );
        assert!(m.roms.unmarked_nonstandard().is_empty());
        let mut unmarked = m;
        unmarked.roms.files[1].suspect = None;
        assert_eq!(unmarked.roms.unmarked_nonstandard().len(), 1);
    }

    #[test]
    fn validation_rejects_bad_suspects() -> Result<()> {
        let ok = sample().render()?;
        for (what, text) in [
            (
                "empty reason",
                ok.replace(
                    "reason = \"size 1048731 = 155 + 1048576, not a standard size: container\"",
                    "reason = \" \"",
                ),
            ),
            (
                "non-standard image",
                ok.replace("image_size = 1048576", "image_size = 1048000"),
            ),
            (
                "past the end",
                ok.replace("image_offset = 155", "image_offset = 156"),
            ),
            ("half a range", ok.replace("image_offset = 155\n", "")),
            (
                "unknown kind",
                ok.replace("kind = \"container\"", "kind = \"odd\""),
            ),
        ] {
            assert_ne!(text, ok, "{what}: replacement did not apply");
            assert!(Manifest::from_toml(&text).is_err(), "{what} accepted");
        }
        Ok(())
    }

    #[test]
    fn duplicate_hashes_are_grouped() {
        let mut m = sample();
        assert!(duplicate_hashes(&m).is_empty());
        m.roms.files[2].sha256 = m.roms.files[0].sha256.clone();
        assert_eq!(
            duplicate_hashes(&m),
            [vec![
                m.roms.files[0].path.as_str(),
                m.roms.files[2].path.as_str()
            ]]
        );
    }

    fn def(path: &str, sha_digit: char) -> DefFile {
        DefFile {
            path: path.to_owned(),
            size: 1234,
            sha256: std::iter::repeat_n(sha_digit, 64).collect(),
        }
    }

    /// [`sample`] plus a `[defs]` table whose paths have spaces as upstream's
    /// do (a doubled one before the extension, and one inside the name).
    fn sample_with_defs() -> Manifest {
        let mut m = sample();
        m.defs = Some(DefsCorpus {
            upstream: DEFS_UPSTREAM_URL.to_owned(),
            branch: DEFS_BRANCH.to_owned(),
            commit: DEFS_PIN_COMMIT.to_owned(),
            path_prefix: DEFS_PATH_PREFIX.to_owned(),
            licence: DEFS_LICENCE.to_owned(),
            logger_v370: LoggerPackage {
                file: LOGGER_V370_FILE.to_owned(),
                size: LOGGER_V370_SIZE,
                sha256: LOGGER_V370_SHA256.to_owned(),
                url: LOGGER_V370_URL.to_owned(),
                note: LOGGER_V370_NOTE.to_owned(),
            },
            files: vec![
                def("ecu/2004USDMBajaXTAT-A2WC50  .xml", 'd'),
                def("ecu/RR D2UJ001T.xml", 'e'),
                def("logger/logger_STD_EN_v290.xml", 'f'),
            ],
        });
        m
    }

    #[test]
    fn defs_round_trip_and_keep_paths_with_spaces() -> Result<()> {
        let m = sample_with_defs();
        let text = m.render()?;
        let back = Manifest::from_toml(&text)?;
        assert_eq!(back, m);
        assert_eq!(back.render()?, text, "rendering is canonical");
        assert!(
            text.contains(&format!(
                "[defs]\nupstream = \"{DEFS_UPSTREAM_URL}\"\nbranch = \"{DEFS_BRANCH}\"\ncommit = \"{DEFS_PIN_COMMIT}\"\npath_prefix = \"{DEFS_PATH_PREFIX}\"\nlicence = "
            )),
            "{text}"
        );
        assert!(
            text.contains(&format!(
                "[defs.logger_v370]\nfile = \"{LOGGER_V370_FILE}\"\nsize = {LOGGER_V370_SIZE}\nsha256 = \"{LOGGER_V370_SHA256}\"\n"
            )),
            "{text}"
        );
        assert!(
            text.contains("[[defs.files]]\npath = \"ecu/RR D2UJ001T.xml\"\nsize = 1234\n"),
            "{text}"
        );
        assert!(text.contains("path = \"ecu/2004USDMBajaXTAT-A2WC50  .xml\""));
        let defs = back
            .defs
            .as_ref()
            .map(|d| d.files.clone())
            .unwrap_or_default();
        assert_eq!(
            back.defs.as_ref().map(|d| d.upstream_path(&defs[1])),
            Some("RomRaider/ecu/RR D2UJ001T.xml".to_owned())
        );
        // A ROM-only manifest renders no [defs] at all.
        assert!(!sample().render()?.contains("\n[defs"));
        Ok(())
    }

    #[test]
    fn defs_validation_rejects_bad_tables() -> Result<()> {
        let ok = sample_with_defs().render()?;
        assert!(Manifest::from_toml(&ok).is_ok());
        let cases: [(&str, String); 11] = [
            ("short commit", ok.replace(DEFS_PIN_COMMIT, "0b0c0c42")),
            ("bad hash", ok.replace(&"d".repeat(64), &"D".repeat(64))),
            (
                "unsorted",
                ok.replace("\"logger/logger_STD", "\"a/logger_STD"),
            ),
            (
                "dotdot path",
                ok.replace("\"ecu/RR D2UJ001T.xml\"", "\"../RR D2UJ001T.xml\""),
            ),
            (
                "duplicate",
                ok.replace(
                    "\"ecu/RR D2UJ001T.xml\"",
                    "\"ecu/2004USDMBajaXTAT-A2WC50  .xml\"",
                ),
            ),
            (
                "prefix",
                ok.replace(
                    "path_prefix = \"RomRaider/\"",
                    "path_prefix = \"RomRaider\"",
                ),
            ),
            (
                "logger path",
                ok.replace("file = \"logger_v370.zip\"", "file = \"x/logger_v370.zip\""),
            ),
            ("logger hash", ok.replace(LOGGER_V370_SHA256, "280d3865")),
            ("logger size", ok.replace("size = 986211", "size = 0")),
            (
                "unknown field",
                ok.replace("branch = \"Alpha\"", "branch = \"Alpha\"\nextra = 1"),
            ),
            ("empty licence", ok.replace(DEFS_LICENCE, " ")),
        ];
        for (what, text) in cases {
            assert_ne!(text, ok, "{what}: replacement did not apply");
            assert!(Manifest::from_toml(&text).is_err(), "{what} accepted");
        }
        let no_logger = ok
            .split("[defs.logger_v370]")
            .next()
            .unwrap_or_default()
            .to_owned()
            + "[[defs.files]]"
            + ok.split_once("[[defs.files]]")
                .map(|(_, r)| r)
                .unwrap_or_default();
        assert!(Manifest::from_toml(&no_logger).is_err(), "no logger pin");

        let mut m = sample();
        m.roms.files = vec![file("defs/romraider/x.hex", 'a')];
        let err = m
            .validate()
            .map_err(|e| e.to_string())
            .err()
            .unwrap_or_default();
        assert!(err.contains("may not start with `defs/`"), "{err}");
        Ok(())
    }

    #[test]
    fn check_names_definition_differences() -> Result<()> {
        let generated = |m: Manifest| -> Result<Generated> {
            Ok(Generated {
                text: m.render()?,
                manifest: m,
                issues: Vec::new(),
            })
        };
        let g = generated(sample_with_defs())?;
        assert!(compare_committed(&g, &g.text).is_empty());
        let p = compare_committed(&g, &sample().render()?);
        assert_eq!(p, ["[defs]: missing from the committed manifest"]);

        let mut changed = sample_with_defs();
        if let Some(d) = changed.defs.as_mut() {
            d.commit = "1".repeat(40);
            d.files[1].sha256 = "9".repeat(64);
            d.files.pop();
            d.logger_v370.size += 1;
        }
        let p = compare_committed(&g, &changed.render()?);
        assert_eq!(
            p,
            [
                format!(
                    "[defs] commit {:?} (generated {DEFS_PIN_COMMIT:?})",
                    "1".repeat(40)
                ),
                "[defs.logger_v370] differs from the generated one".to_owned(),
                "[defs] ecu/RR D2UJ001T.xml: committed entry differs from the generated one"
                    .to_owned(),
                "[defs] logger/logger_STD_EN_v290.xml: missing from the committed manifest"
                    .to_owned(),
            ]
        );
        let p = compare_committed(&generated(sample())?, &g.text);
        assert_eq!(p, ["[defs]: in the committed manifest but not generated"]);
        Ok(())
    }

    fn git(dir: &Path, args: &[&str]) -> Result<String> {
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
            .output()?;
        if !out.status.success() {
            bail!("git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    #[test]
    fn scan_tree_reads_raw_blobs_under_the_prefix_with_spaces_kept() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("rr-defs-scan-unit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let files: [(&str, &[u8]); 5] = [
            ("Pre/ecu/RR D2UJ001T.xml", b"<roms>\r\n</roms>\r\n"),
            ("Pre/ecu/A2WC50  .xml", b"<roms>\n</roms>\n"),
            ("Pre/top.xml", b"top"),
            ("Other/x.xml", b"not under the prefix"),
            ("README.md", b"docs"),
        ];
        for (path, bytes) in files {
            let p = dir.join(path);
            fs::create_dir_all(p.parent().unwrap_or(&dir))?;
            fs::write(p, bytes)?;
        }
        git(&dir, &["init", "--quiet"])?;
        git(&dir, &["add", "-A"])?;
        git(&dir, &["commit", "--quiet", "-m", "defs"])?;
        // Attributes that would rewrite every text file on checkout: the
        // scan reads blobs, so they must not matter.
        fs::write(dir.join(".gitattributes"), "* text=auto eol=crlf\n")?;
        git(&dir, &["add", "-A"])?;
        git(&dir, &["commit", "--quiet", "-m", "attributes"])?;
        let commit = git(&dir, &["rev-parse", "HEAD"])?;
        let git_dir = dir.join(".git");

        let got = scan_tree(&git_dir, &commit, "Pre/")?;
        let want: Vec<(String, u64, String)> = [
            ("ecu/A2WC50  .xml", &b"<roms>\n</roms>\n"[..]),
            ("ecu/RR D2UJ001T.xml", &b"<roms>\r\n</roms>\r\n"[..]),
            ("top.xml", &b"top"[..]),
        ]
        .iter()
        .map(|(p, b)| ((*p).to_owned(), b.len() as u64, sha256_hex(b)))
        .collect();
        let got: Vec<(String, u64, String)> = got
            .into_iter()
            .map(|s| (s.path, s.size, s.sha256))
            .collect();
        assert_eq!(got, want);

        assert!(scan_tree(&git_dir, &commit, "Nothing/").is_err(), "empty");
        assert!(
            scan_tree(&git_dir, &"0".repeat(40), "Pre/").is_err(),
            "missing commit"
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("top.xml", dir.join("Pre/link.xml"))?;
            git(&dir, &["add", "-A"])?;
            git(&dir, &["commit", "--quiet", "-m", "link"])?;
            let head = git(&dir, &["rev-parse", "HEAD"])?;
            let err = scan_tree(&git_dir, &head, "Pre/")
                .map_err(|e| e.to_string())
                .err()
                .unwrap_or_default();
            assert!(err.contains("not a regular file"), "{err}");
        }
        fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
