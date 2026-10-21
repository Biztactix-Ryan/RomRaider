//! `cargo xtask leak-guard [PATH...]` (story US-RR-13, task US-RR-13-5):
//! the ROM-leak guard. Real ECU ROMs are manufacturer-copyrighted and car
//! traces carry VINs, so neither may be committed to the public repo
//! (SECURITY.md "Proprietary firmware" and "PII Handling", D-017). The
//! pre-commit hook (`hooks/pre-commit`) and CI (`ci.yml` job `leak-guard`)
//! run this check (US-RR-13-6).
//!
//! # What is checked
//!
//! Every file under [`PUBLIC_DIR`] (`fixtures/public`), relative to the
//! root, fails when
//!
//! 1. it is larger than [`MAX_PUBLIC_BYTES`] (64 KiB; the smallest real ROM
//!    is 160 KiB, [`crate::corpus_manifest::STANDARD_SIZES`]);
//! 2. its SHA-256 is one a corpus manifest lists: every stock-ROM image,
//!    definition file and the logger package of
//!    `fixtures/corpus-manifest.toml` (or each `--manifest`), plus every
//!    hash of a `sha256sum`-format `--sums` file (a private corpus's hash
//!    list);
//! 3. its bytes contain a known ROM ID anywhere: every `filename_rom_id`
//!    of the manifests and every `filename_rom_id`, `image_rom_id` and
//!    matching definition `xmlid` of `fixtures/corpus-coverage.toml`
//!    (exact bytes, trailing spaces trimmed; IDs shorter than
//!    [`MIN_ROM_ID_LEN`] are ignored);
//! 4. it is binary (holds a NUL byte or is not UTF-8), does not start
//!    with the synthetic-ROM marker ([`SYNTHETIC_MARKER`], fixtures/README.md
//!    "Synthetic ROMs"), and has a ROM-ID-shaped string (8 ASCII
//!    alphanumerics with a letter and a digit, as the corpus-coverage probe
//!    reads them) at an address where a corpus image keeps its ROM ID (the
//!    `image_rom_id_address`es of the coverage report, else
//!    [`DEFAULT_ID_ADDRESSES`]): a real ROM, or its first part, whose ID is
//!    not yet known;
//! 5. its bytes hold an unscrubbed VIN ([`find_vins`]): a run of exactly 17
//!    ASCII alphanumerics, all from the VIN alphabet (no I, O or Q, either
//!    case), with a letter and with the last four characters numeric (ISO
//!    3779's serial). Traces and logs are text or binary, so every file is
//!    scanned. A masked VIN (`JF1GD29X**********`, `JF1XXXXXXXXXXXXXX`) has
//!    a non-numeric tail and passes. The report names the VIN's offset and
//!    its first three characters only, never the VIN itself, so CI logs do
//!    not leak it either;
//! 6. it is a symlink (a public fixture is a regular file).
//!
//! Every other file given on the command line is checked against rule 2
//! only, for the stock-ROM images, the logger package and `--sums` hashes:
//! those are never acceptable anywhere in the repo. Definition-file hashes
//! are not checked outside `fixtures/public`, because `legacy/` keeps the
//! GPL RomRaider DTDs the definition corpus also carries
//! (`legacy/definitions/logger.dtd`, `cars_def.dtd`). The size, ID and VIN
//! rules are scoped to `fixtures/public`, where they hold.
//!
//! # Invocation
//!
//! With no paths, every file under `<root>/fixtures/public` is checked (CI).
//! Paths may be files or directories (walked, symlinks not followed); a
//! relative path is taken from the current directory, and where it lies
//! relative to `--root` (default: this workspace) decides whether it is a
//! public fixture. A pre-commit hook can therefore pass the staged paths
//! (`git diff --cached --name-only --diff-filter=ACMR`), or export the index
//! to a scratch tree and pass `--root <tree>`; the known hashes and IDs
//! always default to this workspace's committed manifest and coverage
//! report, not the `--root` tree's. Exit status: 0 when nothing
//! is found, 1 on any finding (each printed as `leak-guard: <path>:
//! <reason>`), 2 when an input cannot be read.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context as _, Result, bail};

use crate::corpus_coverage::{self, Coverage};
use crate::corpus_manifest::{self, Manifest, hash_file, sha256_hex};

/// The public fixture tree, relative to the root.
pub const PUBLIC_DIR: &str = "fixtures/public";

/// Largest file allowed under [`PUBLIC_DIR`] (64 KiB).
pub const MAX_PUBLIC_BYTES: u64 = 64 * 1024;

/// How much of a public file the content rules read; a larger file already
/// fails the size rule, and its hash is still taken over the whole file.
pub const MAX_SCAN_BYTES: u64 = corpus_manifest::MAX_IMAGE_BYTES;

/// Known ROM IDs shorter than this (after trimming) are not searched for:
/// too short to tell from ordinary text.
pub const MIN_ROM_ID_LEN: usize = 6;

/// Length of a VIN (ISO 3779).
pub const VIN_LEN: usize = 17;

/// The start of every synthetic ROM (rr-testkit's reference encoder, padded
/// with NUL bytes to 32 bytes; fixtures/README.md "Synthetic ROMs").
pub const SYNTHETIC_MARKER: &[u8] = b"RomRaider-RS SYNTHETIC ROM v1";

/// Where corpus images keep their ROM ID, used when the coverage report
/// gives none (it gives these four: legacy definitions' `internalidaddress`
/// values for the corpus).
pub const DEFAULT_ID_ADDRESSES: &[usize] = &[0x200, 0x2000, 0x2004, 0x400C];

/// What the guard knows about real firmware: hashes, ROM IDs and the
/// addresses ROM IDs sit at.
#[derive(Debug, Clone, Default)]
pub struct Known {
    /// Lowercase hex SHA-256 to a label naming where it is listed, and
    /// whether it is checked outside [`PUBLIC_DIR`] too.
    hashes: BTreeMap<String, (String, bool)>,
    /// Known ROM IDs, as bytes.
    rom_ids: BTreeSet<Vec<u8>>,
    /// Addresses ROM IDs are read from.
    id_addresses: BTreeSet<usize>,
}

impl Known {
    /// Nothing known yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The committed manifest and coverage report under `root`
    /// ([`corpus_manifest::COMMITTED_PATH`], [`corpus_coverage::COMMITTED_PATH`]).
    ///
    /// # Errors
    ///
    /// Either file cannot be read or parsed.
    pub fn committed(root: &Path) -> Result<Self> {
        let mut k = Self::new();
        k.load_manifest(&root.join(corpus_manifest::COMMITTED_PATH))?;
        k.load_coverage(&root.join(corpus_coverage::COMMITTED_PATH))?;
        Ok(k)
    }

    /// Adds every hash and file-name ROM ID of the manifest at `path`.
    ///
    /// # Errors
    ///
    /// The file cannot be read or is not a valid manifest.
    pub fn load_manifest(&mut self, path: &Path) -> Result<()> {
        let text =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let m = Manifest::from_toml(&text).with_context(|| format!("in {}", path.display()))?;
        self.add_manifest(&m, &path.display().to_string());
        Ok(())
    }

    /// Adds every ROM ID and ID address of the coverage report at `path`.
    ///
    /// # Errors
    ///
    /// The file cannot be read or is not a valid report.
    pub fn load_coverage(&mut self, path: &Path) -> Result<()> {
        let text =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let c = Coverage::from_toml(&text).with_context(|| format!("in {}", path.display()))?;
        self.add_coverage(&c);
        Ok(())
    }

    /// Adds every hash of the `sha256sum`-format file at `path`.
    ///
    /// # Errors
    ///
    /// The file cannot be read or has a malformed line.
    pub fn load_sums(&mut self, path: &Path) -> Result<()> {
        let text =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let sums =
            corpus_manifest::parse_sums(&text).with_context(|| format!("in {}", path.display()))?;
        for (p, hash) in sums {
            self.add_hash(&hash, &format!("{p} in {}", path.display()), true);
        }
        Ok(())
    }

    /// Adds the hashes and file-name ROM IDs of `m`; `source` names it in
    /// findings.
    pub fn add_manifest(&mut self, m: &Manifest, source: &str) {
        for f in &m.roms.files {
            self.add_hash(
                &f.sha256,
                &format!("stock ROM {} in {source}", f.path),
                true,
            );
            if let Some(id) = &f.filename_rom_id {
                self.add_rom_id(id);
            }
        }
        if let Some(defs) = &m.defs {
            for f in &defs.files {
                self.add_hash(
                    &f.sha256,
                    &format!("definition {} in {source}", f.path),
                    false,
                );
            }
            self.add_hash(
                &defs.logger_v370.sha256,
                &format!("logger package {} in {source}", defs.logger_v370.file),
                true,
            );
        }
    }

    /// Adds the ROM IDs and ID addresses of `c`.
    pub fn add_coverage(&mut self, c: &Coverage) {
        for r in &c.roms {
            for id in [&r.filename_rom_id, &r.image_rom_id].into_iter().flatten() {
                self.add_rom_id(id);
            }
            for d in &r.definitions {
                if let Some(id) = &d.xmlid {
                    self.add_rom_id(id);
                }
            }
            if let Some(a) = r.image_rom_id_address.as_deref().and_then(parse_hex) {
                self.id_addresses.insert(a);
            }
        }
    }

    /// Adds one hash (any case); `label` names where it is listed, and
    /// `everywhere` says whether it is rejected outside [`PUBLIC_DIR`] too.
    /// A hash added twice keeps its first label and is rejected everywhere
    /// if either addition says so.
    pub fn add_hash(&mut self, sha256: &str, label: &str, everywhere: bool) {
        let e = self
            .hashes
            .entry(sha256.to_ascii_lowercase())
            .or_insert_with(|| (label.to_owned(), everywhere));
        e.1 |= everywhere;
    }

    /// Adds one ROM ID (trailing spaces trimmed); one shorter than
    /// [`MIN_ROM_ID_LEN`] is ignored.
    pub fn add_rom_id(&mut self, id: &str) {
        let id = id.trim();
        if id.len() >= MIN_ROM_ID_LEN {
            self.rom_ids.insert(id.as_bytes().to_vec());
        }
    }

    /// Adds an address a ROM ID may sit at.
    pub fn add_id_address(&mut self, address: usize) {
        self.id_addresses.insert(address);
    }

    /// Number of hashes known.
    #[must_use]
    pub fn hash_count(&self) -> usize {
        self.hashes.len()
    }

    /// Number of ROM IDs known.
    #[must_use]
    pub fn rom_id_count(&self) -> usize {
        self.rom_ids.len()
    }

    /// The label of `sha256` (lowercase hex) when it is rejected in a
    /// public fixture (`public`) or elsewhere.
    #[must_use]
    pub fn hash_label(&self, sha256: &str, public: bool) -> Option<&str> {
        self.hashes
            .get(sha256)
            .filter(|(_, everywhere)| public || *everywhere)
            .map(|(label, _)| label.as_str())
    }

    fn addresses(&self) -> Vec<usize> {
        if self.id_addresses.is_empty() {
            DEFAULT_ID_ADDRESSES.to_vec()
        } else {
            self.id_addresses.iter().copied().collect()
        }
    }
}

fn parse_hex(s: &str) -> Option<usize> {
    let s = s.trim();
    let digits = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))?;
    usize::from_str_radix(digits, 16).ok()
}

/// One reason a file fails the guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// Larger than [`MAX_PUBLIC_BYTES`].
    TooLarge {
        /// The file's size.
        size: u64,
    },
    /// Its SHA-256 is listed in a corpus manifest or hash list.
    ManifestHash {
        /// Where the hash is listed.
        label: String,
    },
    /// Holds a known ROM ID.
    KnownRomId {
        /// The ID.
        id: String,
        /// Byte offset of its first occurrence.
        offset: usize,
    },
    /// A binary with a ROM-ID-shaped string at a ROM-ID address.
    RomIdAtAddress {
        /// The string.
        id: String,
        /// The address.
        address: usize,
    },
    /// Holds an unscrubbed VIN.
    Vin {
        /// Byte offset of the VIN.
        offset: usize,
        /// Its first three characters (the manufacturer code).
        wmi: String,
    },
    /// A symlink under [`PUBLIC_DIR`].
    Symlink,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { size } => write!(
                f,
                "{size} bytes, over the {MAX_PUBLIC_BYTES}-byte limit for {PUBLIC_DIR} (a binary this large may be a ROM)"
            ),
            Self::ManifestHash { label } => {
                write!(f, "bytes are identical to {label}")
            }
            Self::KnownRomId { id, offset } => {
                write!(f, "holds known ROM ID {id:?} at offset 0x{offset:X}")
            }
            Self::RomIdAtAddress { id, address } => write!(
                f,
                "binary with ROM-ID-shaped {id:?} at ROM ID address 0x{address:X} (a real ROM, or part of one)"
            ),
            Self::Vin { offset, wmi } => write!(
                f,
                "unscrubbed VIN ({wmi}...) at offset 0x{offset:X}; scrub it (`rr-cli trace scrub` / `rr-cli log scrub`)"
            ),
            Self::Symlink => write!(f, "symlink; a public fixture must be a regular file"),
        }
    }
}

/// The content findings for `data` (a file's first [`MAX_SCAN_BYTES`]
/// bytes) under [`PUBLIC_DIR`]: rules 3, 4 and 5 of the module docs.
#[must_use]
pub fn check_public_bytes(known: &Known, data: &[u8]) -> Vec<Finding> {
    let mut out = Vec::new();
    out.extend(
        find_known_ids(known, data)
            .into_iter()
            .map(|(offset, id)| Finding::KnownRomId { id, offset }),
    );
    if is_binary(data) && !data.starts_with(SYNTHETIC_MARKER) {
        for address in known.addresses() {
            if let Some(id) = rom_id_at(data, address) {
                out.push(Finding::RomIdAtAddress { id, address });
            }
        }
    }
    out.extend(
        find_vins(data)
            .into_iter()
            .map(|(offset, vin)| Finding::Vin {
                offset,
                wmi: vin.chars().take(3).collect(),
            }),
    );
    out
}

/// Every known ROM ID in `data`, each once, with the offset of its first
/// occurrence, in offset order.
#[must_use]
pub fn find_known_ids(known: &Known, data: &[u8]) -> Vec<(usize, String)> {
    let lengths: BTreeSet<usize> = known.rom_ids.iter().map(Vec::len).collect();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (i, b) in data.iter().enumerate() {
        if !b.is_ascii_alphanumeric() {
            continue;
        }
        for &len in &lengths {
            let Some(w) = data.get(i..i + len) else {
                continue;
            };
            if known.rom_ids.contains(w) && seen.insert(w) {
                out.push((i, String::from_utf8_lossy(w).into_owned()));
            }
        }
    }
    out
}

/// Whether `data` is binary: it holds a NUL byte or is not UTF-8.
#[must_use]
pub fn is_binary(data: &[u8]) -> bool {
    data.contains(&0) || std::str::from_utf8(data).is_err()
}

/// The ROM-ID-shaped string at `address` (8 ASCII alphanumerics with a
/// letter and a digit, as [`corpus_coverage::probe_id`] reads them).
#[must_use]
pub fn rom_id_at(data: &[u8], address: usize) -> Option<String> {
    let w = data.get(address..address + corpus_coverage::ID_PROBE_LEN)?;
    let plausible = w.iter().all(u8::is_ascii_alphanumeric)
        && w.iter().any(u8::is_ascii_alphabetic)
        && w.iter().any(u8::is_ascii_digit);
    plausible.then(|| String::from_utf8_lossy(w).into_owned())
}

/// Whether `b` may appear in a VIN (ISO 3779: digits and letters except I,
/// O and Q; either case accepted).
#[must_use]
pub fn is_vin_char(b: u8) -> bool {
    b.is_ascii_digit()
        || (b.is_ascii_alphabetic() && !matches!(b.to_ascii_uppercase(), b'I' | b'O' | b'Q'))
}

/// Every unscrubbed VIN in `data` with its offset: each maximal run of
/// exactly [`VIN_LEN`] ASCII alphanumerics that is all VIN characters, has
/// a letter, and ends in four digits.
#[must_use]
pub fn find_vins(data: &[u8]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let len = data
            .get(i..)
            .unwrap_or_default()
            .iter()
            .take_while(|b| b.is_ascii_alphanumeric())
            .count();
        if len == 0 {
            i += 1;
            continue;
        }
        if let Some(run) = data.get(i..i + len)
            && is_vin(run)
        {
            out.push((i, String::from_utf8_lossy(run).into_owned()));
        }
        i += len;
    }
    out
}

/// Whether `run` is an unscrubbed VIN ([`find_vins`]).
#[must_use]
pub fn is_vin(run: &[u8]) -> bool {
    run.len() == VIN_LEN
        && run.iter().copied().all(is_vin_char)
        && run.iter().any(u8::is_ascii_alphabetic)
        && run
            .get(VIN_LEN - 4..)
            .is_some_and(|tail| tail.iter().all(u8::is_ascii_digit))
}

/// One finding in one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The file, relative to the root with `/` separators when it is inside
    /// it, else as resolved.
    pub path: String,
    /// What is wrong.
    pub finding: Finding,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.finding)
    }
}

/// The outcome of a run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Files checked.
    pub files: usize,
    /// Of those, under [`PUBLIC_DIR`].
    pub public_files: usize,
    /// Every finding, in path order.
    pub violations: Vec<Violation>,
}

/// Checks `paths` (files or directories; none means `<root>/fixtures/public`)
/// against `known` (module docs).
///
/// # Errors
///
/// A path does not exist or a file or directory cannot be read.
pub fn check_paths(root: &Path, paths: &[PathBuf], known: &Known) -> Result<Report> {
    let root = fs::canonicalize(root).with_context(|| format!("resolving {}", root.display()))?;
    let starts: Vec<PathBuf> = if paths.is_empty() {
        let public = root.join(PUBLIC_DIR);
        if !public.is_dir() {
            bail!("{} is not a directory", public.display());
        }
        vec![public]
    } else {
        paths.iter().map(|p| resolve(p)).collect::<Result<_>>()?
    };
    let mut files = BTreeSet::new();
    for s in &starts {
        walk(s, &mut files)?;
    }
    let mut report = Report::default();
    for file in &files {
        let rel = file.strip_prefix(&root).ok();
        let public = rel.is_some_and(is_public);
        let shown = rel.map_or_else(|| file.display().to_string(), slash_path);
        report.files += 1;
        if public {
            report.public_files += 1;
        }
        for finding in check_file(file, public, known)? {
            report.violations.push(Violation {
                path: shown.clone(),
                finding,
            });
        }
    }
    Ok(report)
}

/// The findings for one file; `public` applies every rule, else only the
/// hash rule.
///
/// # Errors
///
/// The file cannot be read.
pub fn check_file(path: &Path, public: bool, known: &Known) -> Result<Vec<Finding>> {
    let meta = fs::symlink_metadata(path).with_context(|| format!("reading {}", path.display()))?;
    if meta.file_type().is_symlink() {
        return Ok(if public {
            vec![Finding::Symlink]
        } else {
            vec![]
        });
    }
    let mut out = Vec::new();
    let size = meta.len();
    if public && size > MAX_PUBLIC_BYTES {
        out.push(Finding::TooLarge { size });
    }
    let data = if public {
        let mut buf = Vec::new();
        fs::File::open(path)
            .and_then(|f| f.take(MAX_SCAN_BYTES).read_to_end(&mut buf))
            .with_context(|| format!("reading {}", path.display()))?;
        Some(buf)
    } else {
        None
    };
    let sha = match &data {
        Some(d) if (d.len() as u64) == size => sha256_hex(d),
        _ => hash_file(path)?.1,
    };
    if let Some(label) = known.hash_label(&sha, public) {
        out.push(Finding::ManifestHash {
            label: label.to_owned(),
        });
    }
    if let Some(d) = &data {
        out.extend(check_public_bytes(known, d));
    }
    Ok(out)
}

/// Whether `rel` (relative to the root) is under [`PUBLIC_DIR`].
#[must_use]
pub fn is_public(rel: &Path) -> bool {
    let mut parts = rel.components().filter(|c| !matches!(c, Component::CurDir));
    PUBLIC_DIR
        .split('/')
        .all(|want| parts.next().is_some_and(|c| c.as_os_str() == want))
        && parts.next().is_some()
}

fn slash_path(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// `p` made absolute with its directory resolved (symlinks and `..`), but
/// its last component kept, so a symlink is checked as one.
fn resolve(p: &Path) -> Result<PathBuf> {
    let abs = std::path::absolute(p).with_context(|| format!("resolving {}", p.display()))?;
    fs::symlink_metadata(&abs).with_context(|| format!("{} does not exist", p.display()))?;
    match (abs.parent(), abs.file_name()) {
        (Some(dir), Some(name)) => {
            let dir =
                fs::canonicalize(dir).with_context(|| format!("resolving {}", p.display()))?;
            Ok(dir.join(name))
        }
        _ => fs::canonicalize(&abs).with_context(|| format!("resolving {}", p.display())),
    }
}

fn walk(p: &Path, out: &mut BTreeSet<PathBuf>) -> Result<()> {
    let meta = fs::symlink_metadata(p).with_context(|| format!("reading {}", p.display()))?;
    if !meta.is_dir() {
        out.insert(p.to_path_buf());
        return Ok(());
    }
    for entry in fs::read_dir(p).with_context(|| format!("reading {}", p.display()))? {
        let entry = entry.with_context(|| format!("reading {}", p.display()))?;
        walk(&entry.path(), out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known() -> Known {
        let mut k = Known::new();
        k.add_rom_id("A2WC400H");
        k.add_rom_id("A2WC50  ");
        k.add_rom_id("ABC");
        k
    }

    /// A fake binary ROM image of `size` bytes, 0xFF-filled, with `id` at
    /// `address`.
    fn image(size: usize, id: &[u8], address: usize) -> Vec<u8> {
        let mut v = vec![0xFF; size];
        v[0] = 0;
        v[address..address + id.len()].copy_from_slice(id);
        v
    }

    #[test]
    fn known_ids_are_trimmed_and_short_ones_dropped() {
        let k = known();
        assert_eq!(k.rom_id_count(), 2);
        let data = b"xx A2WC50 yy A2WC400H A2WC400H";
        assert_eq!(
            find_known_ids(&k, data),
            [(3, "A2WC50".to_owned()), (13, "A2WC400H".to_owned())]
        );
        assert!(find_known_ids(&k, b"ABC a2wc400h A2WC4OOH").is_empty());
    }

    #[test]
    fn known_id_inside_a_binary_is_found() {
        let d = image(0x3000, b"A2WC400H", 0x1234);
        assert_eq!(
            check_public_bytes(&known(), &d),
            [Finding::KnownRomId {
                id: "A2WC400H".into(),
                offset: 0x1234
            }]
        );
    }

    #[test]
    fn unknown_id_at_an_id_address_of_a_binary_is_found() {
        let k = known();
        for &a in DEFAULT_ID_ADDRESSES {
            let d = image(0x5000, b"ZZ9X123Q", a);
            assert_eq!(
                check_public_bytes(&k, &d),
                [Finding::RomIdAtAddress {
                    id: "ZZ9X123Q".into(),
                    address: a
                }],
                "address 0x{a:X}"
            );
        }
        // Not at an ID address, all letters, or all digits: not ID-shaped.
        for (id, a) in [
            (&b"ZZ9X123Q"[..], 0x2001),
            (b"ZZZZZZZZ", 0x2000),
            (b"12345678", 0x2000),
        ] {
            assert!(check_public_bytes(&k, &image(0x5000, id, a)).is_empty());
        }
    }

    #[test]
    fn coverage_addresses_replace_the_defaults() {
        let mut k = known();
        k.add_id_address(0x100);
        let d = image(0x3000, b"ZZ9X123Q", 0x100);
        assert_eq!(
            check_public_bytes(&k, &d),
            [Finding::RomIdAtAddress {
                id: "ZZ9X123Q".into(),
                address: 0x100
            }]
        );
        assert!(check_public_bytes(&k, &image(0x3000, b"ZZ9X123Q", 0x2000)).is_empty());
    }

    #[test]
    fn synthetic_marker_and_text_skip_the_address_rule_only() {
        let k = known();
        let mut d = image(0x3000, b"ZZ9X123Q", 0x2000);
        d[..SYNTHETIC_MARKER.len()].copy_from_slice(SYNTHETIC_MARKER);
        assert!(check_public_bytes(&k, &d).is_empty());
        // A known ID still fails a synthetic-marked file.
        d[0x2000..0x2008].copy_from_slice(b"A2WC400H");
        assert_eq!(check_public_bytes(&k, &d).len(), 1);
        // Text with an ID-shaped word at 0x200 is not a binary.
        let mut t = vec![b' '; 0x300];
        t[0x200..0x208].copy_from_slice(b"ZZ9X123Q");
        assert!(!is_binary(&t));
        assert!(check_public_bytes(&k, &t).is_empty());
        assert!(is_binary(&[0xC3, 0x28]));
    }

    #[test]
    fn vins_are_found_and_reported_without_the_vin() {
        let text = b"vin=JF1GD29662G512345;\nvin: 1hgcm82633a004352\n";
        let found = find_vins(text);
        assert_eq!(
            found,
            [
                (4, "JF1GD29662G512345".to_owned()),
                (28, "1hgcm82633a004352".to_owned())
            ]
        );
        let f = check_public_bytes(&Known::new(), text);
        let shown: Vec<String> = f.iter().map(ToString::to_string).collect();
        assert!(shown[0].starts_with("unscrubbed VIN (JF1...) at offset 0x4"));
        assert!(shown.iter().all(|s| !s.contains("512345")));
        // In a binary trace, between non-alphanumeric bytes.
        let mut bin = vec![0x80, 0x10, 0xF0];
        bin.extend_from_slice(b"JF1GD29662G512345");
        bin.push(0x00);
        assert_eq!(find_vins(&bin), [(3, "JF1GD29662G512345".to_owned())]);
    }

    #[test]
    fn scrubbed_or_vin_like_runs_pass() {
        for s in [
            &b"JF1GD29662G******"[..],
            b"JF1XXXXXXXXXXXXXX",
            b"XXXXXXXXXXXXXXXXX",
            b"JF1GD29662G51234",                 // 16
            b"JF1GD29662G5123456",               // 18
            b"JF1GD29662G5I2345",                // I is not a VIN character
            b"JF1GO29662G512345",                // O
            b"JF1GQ29662G512345",                // Q
            b"12345678901234567",                // no letter
            b"JF1GD29662G51234A",                // tail not numeric
            b"6dad26e8a605d53570c3523b5d95082e", // a hash
        ] {
            assert!(
                find_vins(s).is_empty(),
                "{:?} should pass",
                String::from_utf8_lossy(s)
            );
        }
    }

    #[test]
    fn hashes_match_case_insensitively() {
        let mut k = Known::new();
        let h = sha256_hex(b"rom");
        k.add_hash(&h.to_ascii_uppercase(), "planted", true);
        assert_eq!(k.hash_label(&h, true), Some("planted"));
        assert_eq!(k.hash_label(&h, false), Some("planted"));
        let d = sha256_hex(b"definition");
        k.add_hash(&d, "a definition", false);
        assert_eq!(k.hash_label(&d, true), Some("a definition"));
        assert_eq!(k.hash_label(&d, false), None);
        k.add_hash(&d, "also a ROM", true);
        assert_eq!(k.hash_label(&d, false), Some("a definition"));
        assert_eq!(k.hash_count(), 2);
    }

    #[test]
    fn public_scope_is_component_wise() {
        assert!(is_public(Path::new("fixtures/public/a.bin")));
        assert!(is_public(Path::new("./fixtures/public/x/y.trace")));
        assert!(!is_public(Path::new("fixtures/public")));
        assert!(!is_public(Path::new("fixtures/publicity/a.bin")));
        assert!(!is_public(Path::new("fixtures/kat/a.toml")));
        assert!(!is_public(Path::new("other/fixtures/public/a.bin")));
    }

    #[test]
    fn hex_addresses_parse() {
        assert_eq!(parse_hex("0x2000"), Some(0x2000));
        assert_eq!(parse_hex("0X400C"), Some(0x400C));
        assert_eq!(parse_hex("2000"), None);
    }
}
