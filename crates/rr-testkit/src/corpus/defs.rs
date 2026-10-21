//! The pinned ECU / logger definition files of the corpus (D-028), read
//! from the manifest and checked against it (US-RR-27-4).
//!
//! `fixtures/corpus-manifest.toml` pins the definition tree under `[defs]`
//! (`[[defs.files]]`: path, size, SHA-256; fetched by `cargo xtask
//! fetch-corpus` into `RR_CORPUS_DIR`'s [`DEFS_ROOT`]) and the forum logger
//! package under `[defs.logger_v370]` (a zip that no tool downloads).
//! Tests that must cover *every* pinned definition file drive their list
//! from [`DefsPin`], never from a directory listing, so a file missing from
//! the fetched tree is a failure rather than one fewer file read.
//!
//! * [`read_def`] returns a pinned file's bytes after checking its size and
//!   SHA-256 against the manifest.
//! * [`read_logger_v370`] returns the members of the logger package, after
//!   the same check, or `None` when no copy is present (it is never
//!   downloaded; the copy read is `RR_LOGGER_ZIP`, else
//!   `<corpus>/defs/logger_v370.zip`, as fetch-corpus checks it).
//! * [`zip_members`] is the small zip reader that needs: stored and deflated
//!   members, each inflated to exactly its declared size and checked against
//!   its CRC-32. No zip64, encryption or multi-disk archives (the pinned
//!   package has none; one is refused, never misread).
//!
//! The ROM loader ([`super::Corpus`]) ignores `[defs]`; the formula harvest
//! ([`super::formulas`]) reads the same table for its own purposes.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::formulas::{DEFS_ROOT, sha256_hex};
use super::{CorpusError, plain_relative};

/// Environment variable naming a copy of the logger package (in a private
/// mirror); xtask's `fetch_corpus::LOGGER_ZIP_ENV`.
pub const LOGGER_ZIP_ENV: &str = "RR_LOGGER_ZIP";

/// Errors reading the pinned definitions.
#[derive(Debug, thiserror::Error)]
pub enum DefsError {
    /// The corpus could not be opened.
    #[error(transparent)]
    Corpus(#[from] CorpusError),
    /// A file could not be read.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The manifest has no usable `[defs]` table.
    #[error("{}: [defs]: {reason}", path.display())]
    Manifest {
        /// The manifest.
        path: PathBuf,
        /// What is wrong.
        reason: String,
    },
    /// A pinned file is not the pinned bytes.
    #[error(
        "{path}: {what} differs from the manifest (want {want}, got {got}); run `cargo xtask fetch-corpus`"
    )]
    NotPinned {
        /// The file, corpus-relative (or the logger package's path).
        path: String,
        /// `size` or `sha256`.
        what: &'static str,
        /// The manifest's value.
        want: String,
        /// The file's.
        got: String,
    },
    /// The logger package is not a zip this reader accepts.
    #[error("{}: {reason}", path.display())]
    Zip {
        /// The package.
        path: PathBuf,
        /// What is wrong.
        reason: String,
    },
}

/// One `[[defs.files]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DefFile {
    /// Path below the definition tree ([`DEFS_ROOT`]), `/`-separated.
    pub path: String,
    /// Size in bytes.
    pub size: u64,
    /// Lowercase hex SHA-256.
    pub sha256: String,
}

impl DefFile {
    /// The corpus-relative path, `defs/romraider/<path>`.
    #[must_use]
    pub fn corpus_path(&self) -> String {
        format!("{DEFS_ROOT}/{}", self.path)
    }

    /// Whether the file is XML by its extension (`.xml`, any case); the
    /// others pinned are DTDs.
    #[must_use]
    pub fn is_xml(&self) -> bool {
        Path::new(&self.path)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("xml"))
    }
}

/// The `[defs.logger_v370]` pin.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LoggerPin {
    /// The package's file name.
    pub file: String,
    /// Size in bytes.
    pub size: u64,
    /// Lowercase hex SHA-256.
    pub sha256: String,
}

/// The manifest's `[defs]` table, as far as these readers need it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DefsPin {
    /// The pinned upstream commit.
    pub commit: String,
    /// Every pinned file of the tree, in manifest order.
    pub files: Vec<DefFile>,
    /// The logger package pin.
    pub logger_v370: LoggerPin,
}

#[derive(Deserialize)]
struct ManifestDefs {
    defs: Option<DefsPin>,
}

impl DefsPin {
    /// Reads `[defs]` from the manifest at `manifest`.
    ///
    /// # Errors
    ///
    /// [`DefsError::Io`]; [`DefsError::Manifest`] when the TOML does not
    /// parse, there is no `[defs]`, or a path is not a plain relative
    /// `/`-separated one.
    pub fn load(manifest: &Path) -> Result<Self, DefsError> {
        let bad = |reason: String| DefsError::Manifest {
            path: manifest.to_path_buf(),
            reason,
        };
        let text = std::fs::read_to_string(manifest).map_err(|source| DefsError::Io {
            path: manifest.to_path_buf(),
            source,
        })?;
        let parsed: ManifestDefs = toml::from_str(&text).map_err(|e| bad(e.to_string()))?;
        let defs = parsed
            .defs
            .ok_or_else(|| bad("missing: the manifest pins no definition corpus".to_owned()))?;
        if let Some(f) = defs.files.iter().find(|f| !plain_relative(&f.path)) {
            return Err(bad(format!(
                "{:?} is not a plain relative `/`-separated path",
                f.path
            )));
        }
        Ok(defs)
    }
}

/// Checks `bytes` against a pinned size and SHA-256.
fn check_pin(path: &str, bytes: &[u8], size: u64, sha256: &str) -> Result<(), DefsError> {
    if bytes.len() as u64 != size {
        return Err(DefsError::NotPinned {
            path: path.to_owned(),
            what: "size",
            want: size.to_string(),
            got: bytes.len().to_string(),
        });
    }
    let got = sha256_hex(bytes);
    if !got.eq_ignore_ascii_case(sha256) {
        return Err(DefsError::NotPinned {
            path: path.to_owned(),
            what: "sha256",
            want: sha256.to_owned(),
            got,
        });
    }
    Ok(())
}

/// The bytes of pinned file `file` in corpus directory `corpus_dir`, after
/// checking them against the manifest's size and SHA-256.
///
/// # Errors
///
/// [`DefsError::Io`] (a missing file included) or [`DefsError::NotPinned`].
pub fn read_def(corpus_dir: &Path, file: &DefFile) -> Result<Vec<u8>, DefsError> {
    let full = corpus_dir.join(file.corpus_path());
    let bytes = std::fs::read(&full).map_err(|source| DefsError::Io { path: full, source })?;
    check_pin(&file.corpus_path(), &bytes, file.size, &file.sha256)?;
    Ok(bytes)
}

/// Where the logger package copy for corpus directory `corpus_dir` is
/// looked for: [`LOGGER_ZIP_ENV`] if set and non-empty, else
/// `<corpus>/defs/<file>` (fetch-corpus's rule).
#[must_use]
pub fn logger_v370_path(corpus_dir: &Path, pin: &LoggerPin) -> PathBuf {
    std::env::var_os(LOGGER_ZIP_ENV)
        .filter(|v| !v.is_empty())
        .map_or_else(|| corpus_dir.join("defs").join(&pin.file), PathBuf::from)
}

/// One member of a zip archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipMember {
    /// Its name in the archive.
    pub name: String,
    /// Its uncompressed bytes.
    pub bytes: Vec<u8>,
}

/// The logger package's path and members, or `None` when there is no copy
/// at [`logger_v370_path`]. The package is checked against the pin first.
///
/// # Errors
///
/// [`DefsError::Io`], [`DefsError::NotPinned`] or [`DefsError::Zip`].
pub fn read_logger_v370(
    corpus_dir: &Path,
    pin: &LoggerPin,
) -> Result<Option<(PathBuf, Vec<ZipMember>)>, DefsError> {
    let path = logger_v370_path(corpus_dir, pin);
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path).map_err(|source| DefsError::Io {
        path: path.clone(),
        source,
    })?;
    check_pin(&path.display().to_string(), &bytes, pin.size, &pin.sha256)?;
    let members = zip_members(&bytes).map_err(|reason| DefsError::Zip {
        path: path.clone(),
        reason,
    })?;
    Ok(Some((path, members)))
}

const EOCD_SIG: u32 = 0x0605_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const LOCAL_SIG: u32 = 0x0403_4b50;
const EOCD_LEN: usize = 22;

fn u16_at(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or_else(|| format!("truncated at offset {at}"))
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| format!("truncated at offset {at}"))
}

fn usize_at(b: &[u8], at: usize) -> Result<usize, String> {
    let v = u32_at(b, at)?;
    if v == u32::MAX {
        return Err(format!("zip64 field at offset {at} (not supported)"));
    }
    usize::try_from(v).map_err(|e| e.to_string())
}

/// Every file member of the zip archive `zip`, in central-directory order
/// (directory entries, names ending in `/`, are left out). Each is inflated
/// to exactly its declared size and checked against its CRC-32.
///
/// # Errors
///
/// A description of the first problem: no end-of-central-directory record,
/// a multi-disk, zip64 or encrypted archive, a compression method other than
/// stored (0) or deflate (8), a truncated record, a size or CRC-32 mismatch.
pub fn zip_members(zip: &[u8]) -> Result<Vec<ZipMember>, String> {
    // The end-of-central-directory record is the last 22 bytes plus a
    // comment of at most 64 KiB.
    let lowest = zip.len().saturating_sub(EOCD_LEN + usize::from(u16::MAX));
    let eocd = (lowest..=zip.len().saturating_sub(EOCD_LEN))
        .rev()
        .find(|&at| u32_at(zip, at) == Ok(EOCD_SIG))
        .ok_or("no end-of-central-directory record: not a zip archive")?;
    if u16_at(zip, eocd + 4)? != 0 || u16_at(zip, eocd + 6)? != 0 {
        return Err("multi-disk archive (not supported)".to_owned());
    }
    let count = usize::from(u16_at(zip, eocd + 10)?);
    let mut at = usize_at(zip, eocd + 16)?;
    let mut members = Vec::with_capacity(count);
    for index in 0..count {
        if u32_at(zip, at)? != CENTRAL_SIG {
            return Err(format!("central directory entry {index}: bad signature"));
        }
        let flags = u16_at(zip, at + 8)?;
        let method = u16_at(zip, at + 10)?;
        let crc = u32_at(zip, at + 16)?;
        let packed = usize_at(zip, at + 20)?;
        let size = usize_at(zip, at + 24)?;
        let name_len = usize::from(u16_at(zip, at + 28)?);
        let extra_len = usize::from(u16_at(zip, at + 30)?);
        let comment_len = usize::from(u16_at(zip, at + 32)?);
        let local = usize_at(zip, at + 42)?;
        let name_bytes = zip
            .get(at + 46..at + 46 + name_len)
            .ok_or_else(|| format!("central directory entry {index}: truncated name"))?;
        let name = String::from_utf8_lossy(name_bytes).into_owned();
        at += 46 + name_len + extra_len + comment_len;
        if name.ends_with('/') {
            continue;
        }
        if flags & 1 != 0 {
            return Err(format!("{name}: encrypted (not supported)"));
        }
        if u32_at(zip, local)? != LOCAL_SIG {
            return Err(format!("{name}: bad local header signature"));
        }
        let data = local
            + 30
            + usize::from(u16_at(zip, local + 26)?)
            + usize::from(u16_at(zip, local + 28)?);
        let raw = zip
            .get(data..data + packed)
            .ok_or_else(|| format!("{name}: truncated data"))?;
        let bytes = match method {
            0 => raw.to_vec(),
            8 => {
                let mut out = Vec::with_capacity(size);
                // One byte past the declared size, so a member that inflates
                // to more is caught without inflating all of it.
                let limit = u64::try_from(size).map_err(|e| e.to_string())? + 1;
                flate2::read::DeflateDecoder::new(raw)
                    .take(limit)
                    .read_to_end(&mut out)
                    .map_err(|e| format!("{name}: {e}"))?;
                out
            }
            other => {
                return Err(format!(
                    "{name}: compression method {other} (not supported)"
                ));
            }
        };
        if bytes.len() != size {
            return Err(format!(
                "{name}: {} bytes uncompressed, the archive says {size}",
                bytes.len()
            ));
        }
        let mut sum = flate2::Crc::new();
        sum.update(&bytes);
        if sum.sum() != crc {
            return Err(format!(
                "{name}: CRC-32 {:08x}, the archive says {crc:08x}",
                sum.sum()
            ));
        }
        members.push(ZipMember { name, bytes });
    }
    Ok(members)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    /// A zip of `(name, data, deflate?)` members, built by hand.
    fn zip(members: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data, deflate) in members {
            let packed = if *deflate {
                let mut enc =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                enc.write_all(data).unwrap();
                enc.finish().unwrap()
            } else {
                data.to_vec()
            };
            let mut crc = flate2::Crc::new();
            crc.update(data);
            let method: u16 = if *deflate { 8 } else { 0 };
            let offset = u32::try_from(out.len()).unwrap();
            let common = |v: &mut Vec<u8>| {
                v.extend_from_slice(&20u16.to_le_bytes()); // version needed
                v.extend_from_slice(&0u16.to_le_bytes()); // flags
                v.extend_from_slice(&method.to_le_bytes());
                v.extend_from_slice(&[0; 4]); // time, date
                v.extend_from_slice(&crc.sum().to_le_bytes());
                v.extend_from_slice(&u32::try_from(packed.len()).unwrap().to_le_bytes());
                v.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
                v.extend_from_slice(&u16::try_from(name.len()).unwrap().to_le_bytes());
                v.extend_from_slice(&0u16.to_le_bytes()); // extra
            };
            out.extend_from_slice(&LOCAL_SIG.to_le_bytes());
            common(&mut out);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&packed);
            central.extend_from_slice(&CENTRAL_SIG.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes()); // version made by
            common(&mut central);
            central.extend_from_slice(&[0; 6]); // comment, disk, internal attrs
            central.extend_from_slice(&[0; 4]); // external attrs
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_offset = u32::try_from(out.len()).unwrap();
        let n = u16::try_from(members.len()).unwrap();
        out.extend_from_slice(&central);
        out.extend_from_slice(&EOCD_SIG.to_le_bytes());
        out.extend_from_slice(&[0; 4]); // disk numbers
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&u32::try_from(central.len()).unwrap().to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // comment
        out
    }

    #[test]
    fn reads_stored_and_deflated_members() {
        let big = b"<logger>".repeat(500);
        let z = zip(&[("a.xml", b"<a/>", false), ("b/c.xml", &big, true)]);
        let got = zip_members(&z).unwrap();
        assert_eq!(
            got,
            vec![
                ZipMember {
                    name: "a.xml".into(),
                    bytes: b"<a/>".to_vec()
                },
                ZipMember {
                    name: "b/c.xml".into(),
                    bytes: big
                },
            ]
        );
    }

    #[test]
    fn refuses_a_bad_crc_and_a_non_zip() {
        let mut z = zip(&[("a.xml", b"<a/>", false)]);
        // The stored data sits right after the 30-byte local header and the
        // 5-byte name.
        z[35] = b'!';
        let err = zip_members(&z).unwrap_err();
        assert!(err.contains("a.xml: CRC-32"), "{err}");
        assert!(zip_members(b"<roms/>").unwrap_err().contains("not a zip"));
    }

    #[test]
    fn def_file_kinds_and_paths() {
        let f = |path: &str| DefFile {
            path: path.into(),
            size: 0,
            sha256: String::new(),
        };
        assert!(f("ecu/RR_AZ1J200N.XML").is_xml());
        assert!(f("ecu/x.xml").is_xml());
        assert!(!f("logger/logger.dtd").is_xml());
        assert_eq!(f("ecu/x.xml").corpus_path(), "defs/romraider/ecu/x.xml");
    }
}
