//! `cargo xtask check-release-artifacts <dir>`: the release artifact content
//! check (story US-RR-38, task US-RR-38-8; SECURITY.md "Proprietary
//! firmware", D-017, D-025). Release artifacts never contain ROM images or
//! fixture data.
//!
//! release.yml job `publish` runs it on the downloaded `assets/` directory
//! after writing SHA256SUMS and before anything is uploaded, so a finding
//! fails the job and nothing is published.
//!
//! Every asset is opened and every entry inside it is inspected, in pure
//! Rust (nothing from the artifacts is executed or extracted to disk):
//!
//! * `romraider-cli-<target>.tar.gz`: the cargo-dist tarball;
//! * `romraider-cli_<version>_<arch>.deb`: the `ar` members, then every
//!   entry of `control.tar[.gz|.xz|.zst]` and `data.tar[.gz|.xz|.zst]`;
//! * `romraider-cli-<arch>.AppImage`: the SquashFS payload after the ELF
//!   runtime ([`crate::squashfs`]), found the way the AppImage runtime finds
//!   it (the end of the runtime's ELF image);
//! * `SHA256SUMS`: must be small text.
//!
//! Any other file in the directory fails the check: it would be published
//! without having been looked inside. So does an archive that cannot be
//! read completely, or a compression the check cannot decode.
//!
//! Each entry is flagged when:
//!
//! * its path has a fixture or corpus directory component ([`FIXTURE_DIRS`])
//!   or a fixture manifest name, or a symlink points at one;
//! * its name has a ROM-image extension ([`ROM_EXTENSIONS`]: the corpus's
//!   raw `.hex` images, `.bin`, EcuFlash `.srf`, S-records, ...) or is a
//!   nested archive ([`ARCHIVE_EXTENSIONS`]), whose contents would go
//!   unchecked;
//! * it is ROM-sized ([`rom_size_reason`]): a multiple of 8 KiB from 32 KiB
//!   to 16 MiB (the stock corpus has 160 KiB, 192 KiB, 512 KiB, 1 MiB,
//!   1.5 MiB and 2 MiB images; EC-10 has 4 MiB ones), or such an image with
//!   up to 4 KiB appended (the corpus's two `suspect` images are 1,048,731
//!   and 1,048,784 bytes, D-025);
//! * it is text that reads as an Intel HEX or Motorola S-record image;
//! * it is not on the artifact's allow-list ([`Layout`]): the binary, the
//!   desktop entry, the icon, the licence / copyright, README, the AppImage's
//!   AppRun and `.DirIcon`, the .deb's control files; or it is on the list
//!   but is not what the list says it is.
//!
//! The only size exemption is for an allow-listed executable path, and only
//! when the file is a well-formed ELF image with nothing appended after its
//! last section ([`elf_image_end`]): a ROM renamed to `romraider-cli`, or
//! one appended to the binary, still fails. Allow-listed text files
//! (licence, desktop entry, icon, README) must be NUL-free UTF-8 and get
//! every other check, sizes included.
//!
//! Adding a file to a release artifact means adding it to its [`Layout`]
//! here; the check fails until then.

use std::fmt;
use std::io::{self, Read as _, Write};
use std::path::Path;

use anyhow::{Context as _, Result};

use crate::squashfs;

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;

/// Most bytes decoded out of one asset (all its files together).
pub const MAX_UNPACKED: u64 = 1024 * MIB;
/// Largest text file accepted at an allow-listed text path.
const MAX_TEXT: u64 = MIB;
/// Largest SHA256SUMS accepted.
const MAX_SUMS: u64 = 64 * KIB;

/// Path components that mark fixture or corpus data (compared
/// case-insensitively): `fixtures/` (public KAT and synthetic tiers),
/// `kat/` and `synthetic/` (their subdirectories, and
/// crates/rr-testkit/tests/data/kat), `corpus/` (`RR_CORPUS_DIR`'s default
/// `~/.cache/romraider-rs/corpus`, fuzz/corpus), `roms/` (the stock-ROM
/// mirror's checkout, D-025), `seeds/` and `regressions/` (fuzz inputs),
/// and test-data directories.
pub const FIXTURE_DIRS: &[&str] = &[
    "fixtures",
    "fixture",
    "kat",
    "synthetic",
    "corpus",
    "roms",
    "seeds",
    "regressions",
    "testdata",
    "test-data",
    "test_data",
];

/// File extensions of ROM images and fixture captures (case-insensitive):
/// raw images (`.bin`, `.rom`, `.img`, `.ori`; the stock corpus's `.hex`),
/// EcuFlash `.srf`, Intel HEX (`.ihex`, `.ihx`), Motorola S-records
/// (`.s19`, `.s28`, `.s37`, `.srec`, `.mot`) and comms traces (`.rrtrace`,
/// D-021).
pub const ROM_EXTENSIONS: &[&str] = &[
    "bin", "hex", "rom", "img", "ori", "srf", "ihex", "ihx", "s19", "s28", "s37", "srec", "mot",
    "rrtrace",
];

/// Archive extensions (case-insensitive) that must not be nested inside an
/// artifact: the check does not open archives within archives.
pub const ARCHIVE_EXTENSIONS: &[&str] = &[
    "tar", "gz", "tgz", "xz", "txz", "zst", "bz2", "zip", "7z", "rar", "deb", "rpm", "appimage",
    "squashfs", "sqfs", "cpio", "ar",
];

/// Why a file of `len` bytes looks like an ECU ROM image, if it does.
pub fn rom_size_reason(len: u64) -> Option<String> {
    let (min, max) = (32 * KIB, 16 * MIB);
    if (min..=max).contains(&len) && len.is_multiple_of(8 * KIB) {
        return Some(format!(
            "ROM-sized: {len} bytes ({} KiB) is a multiple of 8 KiB from 32 KiB to 16 MiB, the ECU ROM image range",
            len / KIB
        ));
    }
    let base = len - len % (64 * KIB);
    let extra = len - base;
    if (128 * KIB..=max).contains(&base) && extra < 4 * KIB {
        return Some(format!(
            "ROM-sized: {len} bytes is a {} KiB image plus {extra} trailing bytes (like the corpus's 1,048,731 / 1,048,784-byte images)",
            base / KIB
        ));
    }
    None
}

/// Where the ELF image in `data` ends: the furthest of its headers, its
/// program headers' file contents, its section header table and its
/// sections' file contents. This is also how the AppImage runtime finds its
/// SquashFS payload. Errors when `data` is not a well-formed ELF file.
pub fn elf_image_end(data: &[u8]) -> Result<u64, String> {
    if data.get(..4) != Some(b"\x7fELF") {
        return Err("no ELF magic".into());
    }
    let class64 = match data.get(4) {
        Some(1) => false,
        Some(2) => true,
        other => return Err(format!("bad ELF class {other:?}")),
    };
    let big = match data.get(5) {
        Some(1) => false,
        Some(2) => true,
        other => return Err(format!("bad ELF data encoding {other:?}")),
    };
    let rd = |at: u64, n: usize| -> Result<u64, String> {
        let at = usize::try_from(at).map_err(|_| "ELF offset out of range".to_owned())?;
        let b = at
            .checked_add(n)
            .and_then(|end| data.get(at..end))
            .ok_or_else(|| format!("ELF field at {at} is past the end of the file"))?;
        let mut v = 0u64;
        for i in 0..n {
            let byte = if big { b[i] } else { b[n - 1 - i] };
            v = (v << 8) | u64::from(byte);
        }
        Ok(v)
    };
    let word = if class64 { 8 } else { 4 };
    let (phoff, shoff) = if class64 {
        (rd(0x20, 8)?, rd(0x28, 8)?)
    } else {
        (rd(0x1C, 4)?, rd(0x20, 4)?)
    };
    let base = if class64 { 0x34 } else { 0x28 };
    let ehsize = rd(base, 2)?;
    let phentsize = rd(base + 2, 2)?;
    let phnum = rd(base + 4, 2)?;
    let shentsize = rd(base + 6, 2)?;
    let shnum = rd(base + 8, 2)?;
    let len = data.len() as u64;
    let span = |off: u64, size: u64| -> Result<u64, String> {
        off.checked_add(size)
            .filter(|&end| end <= len)
            .ok_or_else(|| format!("ELF region {off}+{size} is past the end of the file ({len})"))
    };
    let mut end = ehsize;
    if phnum > 0 {
        end = end.max(span(phoff, phentsize * phnum)?);
        for i in 0..phnum {
            let p = phoff + i * phentsize;
            let (offset, filesz) = if class64 {
                (rd(p + 8, word)?, rd(p + 0x20, word)?)
            } else {
                (rd(p + 4, word)?, rd(p + 0x10, word)?)
            };
            end = end.max(span(offset, filesz)?);
        }
    }
    if shnum > 0 {
        end = end.max(span(shoff, shentsize * shnum)?);
        for i in 0..shnum {
            let s = shoff + i * shentsize;
            const SHT_NOBITS: u64 = 8;
            if rd(s + 4, 4)? == SHT_NOBITS {
                continue;
            }
            let (offset, size) = if class64 {
                (rd(s + 0x18, word)?, rd(s + 0x20, word)?)
            } else {
                (rd(s + 0x10, word)?, rd(s + 0x14, word)?)
            };
            end = end.max(span(offset, size)?);
        }
    }
    Ok(end)
}

/// One entry unpacked from an artifact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Path inside the archive, as stored (normalized by [`inspect`]).
    pub path: String,
    pub node: Node,
}

/// What an entry is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Dir,
    File(Vec<u8>),
    Symlink(String),
    /// Anything else (hard link, device, fifo, ...), described.
    Other(String),
}

/// What an allow-listed path must be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expect {
    /// A complete ELF image, nothing appended.
    Elf,
    /// NUL-free UTF-8 text.
    Text,
    /// An ELF image if it starts with the ELF magic, else text (AppRun).
    ElfOrText,
    /// A symlink with exactly this target.
    Symlink(&'static str),
}

/// A tree inside an artifact and the allow-list it is checked against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// The cargo-dist tarball: one `rr-cli-<target>/` directory holding the
    /// binary, license.txt and README.md (dist-workspace.toml `include`).
    Tarball,
    /// The .deb's control.tar: cargo-deb's `control` and `md5sums`, plus
    /// the maintainer scripts the udev rules will need (INFRASTRUCTURE.md
    /// "Artifacts"); all text.
    DebControl,
    /// The .deb's data.tar: rr-cli's `[package.metadata.deb]` assets and
    /// the copyright file cargo-deb writes from `license-file`.
    DebData,
    /// The AppImage's AppDir (packaging/appimage/build.sh).
    AppImage,
}

impl Layout {
    /// Allow-listed files, relative to the tree's root (for
    /// [`Layout::Tarball`], to its `rr-cli-<target>/` directory).
    fn files(self) -> &'static [(&'static str, Expect)] {
        match self {
            Layout::Tarball => &[
                ("romraider-cli", Expect::Elf),
                ("license.txt", Expect::Text),
                ("README.md", Expect::Text),
            ],
            Layout::DebControl => &[
                ("control", Expect::Text),
                ("md5sums", Expect::Text),
                ("conffiles", Expect::Text),
                ("triggers", Expect::Text),
                ("preinst", Expect::Text),
                ("postinst", Expect::Text),
                ("prerm", Expect::Text),
                ("postrm", Expect::Text),
            ],
            Layout::DebData => &[
                ("usr/bin/romraider-cli", Expect::Elf),
                ("usr/share/applications/romraider-cli.desktop", Expect::Text),
                (
                    "usr/share/icons/hicolor/scalable/apps/romraider-cli.svg",
                    Expect::Text,
                ),
                ("usr/share/doc/romraider-cli/copyright", Expect::Text),
            ],
            Layout::AppImage => &[
                ("AppRun", Expect::ElfOrText),
                (".DirIcon", Expect::Symlink("romraider-cli.svg")),
                ("romraider-cli.desktop", Expect::Text),
                ("romraider-cli.svg", Expect::Text),
                ("usr/bin/romraider-cli", Expect::Elf),
                ("usr/share/applications/romraider-cli.desktop", Expect::Text),
                (
                    "usr/share/icons/hicolor/scalable/apps/romraider-cli.svg",
                    Expect::Text,
                ),
                ("usr/share/licenses/romraider-cli/license.txt", Expect::Text),
            ],
        }
    }

    /// The allow-listed file paths, relative to the tree's root (for
    /// [`Layout::Tarball`], to its `rr-cli-<target>/` directory).
    pub fn allowed_files(self) -> Vec<&'static str> {
        self.files().iter().map(|(p, _)| *p).collect()
    }

    /// `path` relative to the allow-list's root, or `None` when it is
    /// outside it (only [`Layout::Tarball`] has a root other than the
    /// tree's).
    fn relative(self, path: &str) -> Option<&str> {
        match self {
            Layout::Tarball => {
                let (top, rest) = path.split_once('/').unwrap_or((path, ""));
                top.starts_with("rr-cli-").then_some(rest)
            }
            _ => Some(path),
        }
    }

    fn expect(self, path: &str) -> Option<Expect> {
        let rel = self.relative(path)?;
        self.files()
            .iter()
            .find(|(p, _)| *p == rel)
            .map(|(_, e)| *e)
    }

    /// Whether `path` is the root or a directory leading to an allow-listed
    /// file.
    fn allows_dir(self, path: &str) -> bool {
        if path.is_empty() {
            return true;
        }
        let Some(rel) = self.relative(path) else {
            return false;
        };
        rel.is_empty()
            || self
                .files()
                .iter()
                .any(|(p, _)| p.strip_prefix(rel).is_some_and(|r| r.starts_with('/')))
    }
}

impl fmt::Display for Layout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Layout::Tarball => "tarball",
            Layout::DebControl => ".deb control.tar",
            Layout::DebData => ".deb data.tar",
            Layout::AppImage => "AppImage",
        })
    }
}

/// `path` without `./` / `/` decoration; an error for absolute paths or
/// `..` components.
fn normalize(path: &str) -> Result<String, String> {
    let mut parts = Vec::new();
    for c in path.split('/') {
        match c {
            "" | "." => {}
            ".." => return Err(format!("path {path:?} has a `..` component")),
            c => parts.push(c),
        }
    }
    if path.starts_with('/') {
        return Err(format!("path {path:?} is absolute"));
    }
    Ok(parts.join("/"))
}

/// The extension of the last component of `path`, lowercased (none for a
/// dot-file such as `.DirIcon`).
fn extension(path: &str) -> Option<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let (stem, ext) = name.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty()).then(|| ext.to_ascii_lowercase())
}

/// Findings from a path's components and name alone.
fn path_findings(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    for c in path.split('/') {
        let lower = c.to_ascii_lowercase();
        if FIXTURE_DIRS.contains(&lower.as_str()) {
            out.push(format!(
                "fixture path: has a `{c}` component (fixture / corpus data)"
            ));
        }
    }
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    if name == "corpus-manifest.toml" || name.ends_with(".manifest.toml") {
        out.push(format!("fixture manifest name `{name}`"));
    }
    if let Some(ext) = extension(path) {
        if ROM_EXTENSIONS.contains(&ext.as_str()) {
            out.push(format!("ROM image extension `.{ext}`"));
        }
        if ARCHIVE_EXTENSIONS.contains(&ext.as_str()) {
            out.push(format!(
                "nested archive `.{ext}`: its contents would not be inspected"
            ));
        }
    }
    out
}

/// Why `data` is not acceptable text, if it is not.
fn text_problem(data: &[u8]) -> Option<String> {
    if data.len() as u64 > MAX_TEXT {
        return Some(format!(
            "{} bytes is too large for an allow-listed text file",
            data.len()
        ));
    }
    if data.contains(&0) {
        return Some("contains NUL bytes: not a text file".into());
    }
    std::str::from_utf8(data)
        .err()
        .map(|_| "is not UTF-8 text".into())
}

/// Whether text reads as an Intel HEX or Motorola S-record image: at least
/// 16 non-empty lines, 90% of them records.
fn hex_image_reason(data: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(data).ok()?;
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let hex = |s: &str| s.len() >= 6 && s.bytes().all(|b| b.is_ascii_hexdigit());
    let records = lines
        .iter()
        .filter(|l| {
            l.strip_prefix(':').is_some_and(|r| r.len() >= 10 && hex(r))
                || (l.len() >= 10
                    && l.starts_with('S')
                    && l.as_bytes()[1].is_ascii_digit()
                    && hex(&l[2..]))
        })
        .count();
    (lines.len() >= 16 && records * 10 >= lines.len() * 9).then(|| {
        format!(
            "looks like an Intel HEX / Motorola S-record image ({records} of {} lines are records)",
            lines.len()
        )
    })
}

/// Every finding for the entries of one tree of an artifact; `label` names
/// the artifact (and member) in each message. Empty when it is clean.
pub fn inspect(label: &str, layout: Layout, entries: &[Entry]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for e in entries {
        let path = match normalize(&e.path) {
            Ok(p) => p,
            Err(why) => {
                problems.push(format!("{label}: {why}"));
                continue;
            }
        };
        let mut flag = |why: String| problems.push(format!("{label}: {path}: {why}"));
        if seen.contains(&path) {
            flag("appears more than once (a later copy would replace the first)".into());
        }
        seen.push(path.clone());
        for f in path_findings(&path) {
            flag(f);
        }
        let expect = layout.expect(&path);
        match &e.node {
            Node::Dir => {
                if !layout.allows_dir(&path) {
                    flag(format!(
                        "unexpected directory (not in the {layout} allow-list)"
                    ));
                }
            }
            Node::Symlink(target) => {
                match expect {
                    Some(Expect::Symlink(want)) if want == target => {}
                    Some(Expect::Symlink(want)) => {
                        flag(format!("symlink to {target:?}, expected {want:?}"));
                    }
                    _ => flag(format!(
                        "unexpected symlink to {target:?} (not in the {layout} allow-list)"
                    )),
                }
                for f in path_findings(target) {
                    flag(format!("symlink target {target:?}: {f}"));
                }
            }
            Node::Other(kind) => flag(format!("unexpected {kind}")),
            Node::File(data) => {
                let len = data.len() as u64;
                let is_elf = data.starts_with(b"\x7fELF");
                let as_elf = match expect {
                    Some(Expect::Elf) => true,
                    Some(Expect::ElfOrText) => is_elf,
                    _ => false,
                };
                // The only size exemption: a well-formed ELF image at an
                // allow-listed executable path, with nothing appended.
                let mut size_exempt = false;
                if as_elf {
                    match elf_image_end(data) {
                        Ok(end) if end == len => size_exempt = true,
                        Ok(end) => flag(format!(
                            "{} bytes appended after the end of the ELF image (at {end} of {len})",
                            len - end
                        )),
                        Err(why) => {
                            flag(format!("should be an ELF executable, but is not ({why})"))
                        }
                    }
                } else {
                    match expect {
                        Some(Expect::Text | Expect::ElfOrText) => {
                            if let Some(why) = text_problem(data) {
                                flag(why);
                            }
                        }
                        Some(Expect::Symlink(want)) => {
                            flag(format!("is a file, expected a symlink to {want:?}"));
                        }
                        Some(Expect::Elf) => {}
                        None => flag(format!(
                            "unexpected file of {len} bytes (not in the {layout} allow-list)"
                        )),
                    }
                }
                if !size_exempt && let Some(why) = rom_size_reason(len) {
                    flag(why);
                }
                if !is_elf && let Some(why) = hex_image_reason(data) {
                    flag(why);
                }
            }
        }
    }
    problems
}

/// A `Write` that fails once more than `left` bytes are written.
struct Bounded {
    out: Vec<u8>,
    left: u64,
}

impl Write for Bounded {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.len() as u64 > self.left {
            return Err(io::Error::other("decompressed data exceeds the size limit"));
        }
        self.left -= buf.len() as u64;
        self.out.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// `raw` decompressed as `kind` (`gz`, `zlib`, `xz`, `zst`, or `` for none),
/// at most `max` bytes; an error past that or on corrupt input.
pub fn decompress_bounded(kind: &str, raw: &[u8], max: u64) -> Result<Vec<u8>, String> {
    let mut w = Bounded {
        out: Vec::new(),
        left: max,
    };
    let res = match kind {
        "" => w.write_all(raw),
        "gz" => io::copy(&mut flate2::read::MultiGzDecoder::new(raw), &mut w).map(drop),
        "zlib" => io::copy(&mut flate2::read::ZlibDecoder::new(raw), &mut w).map(drop),
        "xz" => lzma_rs::xz_decompress(&mut io::BufReader::new(raw), &mut w)
            .map_err(|e| io::Error::other(e.to_string())),
        "zst" => {
            // One or more frames back to back.
            let mut input = raw;
            let mut res = Ok(());
            while !input.is_empty() {
                match ruzstd::decoding::StreamingDecoder::new(&mut input) {
                    Ok(mut d) => {
                        if let Err(e) = io::copy(&mut d, &mut w) {
                            res = Err(e);
                            break;
                        }
                    }
                    Err(e) => {
                        res = Err(io::Error::other(e.to_string()));
                        break;
                    }
                }
            }
            res
        }
        other => return Err(format!("unsupported compression `{other}`")),
    };
    res.map_err(|e| {
        format!(
            "{} decompression failed: {e}",
            if kind.is_empty() { "copy" } else { kind }
        )
    })?;
    Ok(w.out)
}

/// Every entry of the (uncompressed) tar archive `data`, file bytes
/// included, at most `budget` bytes of them in total.
pub fn read_tar(data: &[u8], budget: &mut u64) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    let mut archive = tar::Archive::new(data);
    let entries = archive.entries().map_err(|e| format!("tar: {e}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("tar: {e}"))?;
        let path = entry.path_bytes().into_owned();
        let path = String::from_utf8_lossy(&path).into_owned();
        let kind = entry.header().entry_type();
        let node = if kind.is_file() {
            let size = entry.size();
            if size > *budget {
                return Err(format!(
                    "tar: {path}: {size} bytes exceeds the unpack budget"
                ));
            }
            *budget -= size;
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .map_err(|e| format!("tar: {path}: {e}"))?;
            Node::File(bytes)
        } else if kind.is_dir() {
            Node::Dir
        } else if kind.is_symlink() {
            let target = entry
                .link_name_bytes()
                .map(|t| String::from_utf8_lossy(&t).into_owned())
                .unwrap_or_default();
            Node::Symlink(target)
        } else if kind.is_hard_link() {
            let target = entry
                .link_name_bytes()
                .map(|t| String::from_utf8_lossy(&t).into_owned())
                .unwrap_or_default();
            Node::Other(format!("hard link to {target:?}"))
        } else if kind.is_pax_global_extensions() {
            continue;
        } else {
            Node::Other(format!("tar entry of type {:?}", kind.as_byte() as char))
        };
        out.push(Entry { path, node });
    }
    Ok(out)
}

/// The members of the `ar` archive `data` (a .deb), in order.
pub fn read_ar(data: &[u8]) -> Result<Vec<(String, &[u8])>, String> {
    let mut rest = data
        .strip_prefix(b"!<arch>\n")
        .ok_or("not an ar archive (no `!<arch>` magic)")?;
    let mut out = Vec::new();
    while !rest.is_empty() {
        if rest == b"\n" {
            break;
        }
        let header = rest.get(..60).ok_or("ar: truncated member header")?;
        if &header[58..60] != b"`\n" {
            return Err("ar: bad member header".into());
        }
        let name = String::from_utf8_lossy(&header[..16]).trim_end().to_owned();
        let name = name.strip_suffix('/').unwrap_or(&name).to_owned();
        let size: usize = std::str::from_utf8(&header[48..58])
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .ok_or_else(|| format!("ar: member {name:?} has a bad size"))?;
        let body = rest
            .get(60..)
            .and_then(|b| b.get(..size))
            .ok_or_else(|| format!("ar: member {name:?} is truncated"))?;
        out.push((name, body));
        let next = 60 + size + size % 2;
        rest = rest.get(next..).unwrap_or_default();
    }
    Ok(out)
}

/// What a release asset is, by its file name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    Tarball,
    Deb,
    AppImage,
    Sums,
}

/// The kind of release asset named `name`, if it is one the check opens.
pub fn asset_kind(name: &str) -> Option<AssetKind> {
    if name == "SHA256SUMS" {
        Some(AssetKind::Sums)
    } else if name.starts_with("romraider-cli-") && name.ends_with(".tar.gz") {
        Some(AssetKind::Tarball)
    } else if name.starts_with("romraider-cli_") && name.ends_with(".deb") {
        Some(AssetKind::Deb)
    } else if name.starts_with("romraider-cli-") && name.ends_with(".AppImage") {
        Some(AssetKind::AppImage)
    } else {
        None
    }
}

/// The result of checking one asset.
#[derive(Debug, Default)]
pub struct AssetReport {
    /// Entries inspected inside it.
    pub entries: usize,
    pub problems: Vec<String>,
}

/// The compression suffix of a .deb member `control.tar.xz` etc.
fn deb_member(name: &str) -> Option<(&'static str, &'static str)> {
    for (tree, label) in [("control.tar", "control"), ("data.tar", "data")] {
        let Some(rest) = name.strip_prefix(tree) else {
            continue;
        };
        let kind = match rest {
            "" => "",
            ".gz" => "gz",
            ".xz" => "xz",
            ".zst" => "zst",
            _ => return None,
        };
        return Some((label, kind));
    }
    None
}

/// Checks the release asset `name` with contents `data`.
pub fn check_asset(name: &str, data: &[u8]) -> AssetReport {
    let mut r = AssetReport::default();
    let mut budget = MAX_UNPACKED;
    match asset_kind(name) {
        None => r.problems.push(format!(
            "{name}: not a known release asset (romraider-cli-<target>.tar.gz, romraider-cli_<version>_<arch>.deb, romraider-cli-<arch>.AppImage, SHA256SUMS); its contents cannot be vouched for"
        )),
        Some(AssetKind::Sums) => {
            r.entries = 1;
            if data.len() as u64 > MAX_SUMS {
                r.problems
                    .push(format!("{name}: {} bytes is too large", data.len()));
            } else if let Some(why) = text_problem(data) {
                r.problems.push(format!("{name}: {why}"));
            }
        }
        Some(AssetKind::Tarball) => {
            match decompress_bounded("gz", data, budget).and_then(|t| read_tar(&t, &mut budget)) {
                Ok(entries) => {
                    r.entries = entries.len();
                    r.problems = inspect(name, Layout::Tarball, &entries);
                }
                Err(e) => r.problems.push(format!("{name}: cannot be read completely: {e}")),
            }
        }
        Some(AssetKind::Deb) => check_deb(name, data, &mut budget, &mut r),
        Some(AssetKind::AppImage) => check_appimage(name, data, budget, &mut r),
    }
    if r.entries == 0 && r.problems.is_empty() {
        r.problems.push(format!("{name}: holds no entries"));
    }
    r
}

fn check_deb(name: &str, data: &[u8], budget: &mut u64, r: &mut AssetReport) {
    let members = match read_ar(data) {
        Ok(m) => m,
        Err(e) => {
            r.problems
                .push(format!("{name}: cannot be read completely: {e}"));
            return;
        }
    };
    let mut trees = 0;
    for (i, (member, body)) in members.iter().enumerate() {
        let label = format!("{name}!{member}");
        if member == "debian-binary" {
            if i != 0 || *body != b"2.0\n" {
                r.problems
                    .push(format!("{label}: is not the leading `2.0` format marker"));
            }
            r.entries += 1;
            continue;
        }
        let Some((tree, kind)) = deb_member(member) else {
            r.problems.push(format!(
                "{label}: unexpected .deb member (only debian-binary, control.tar[.gz|.xz|.zst], data.tar[.gz|.xz|.zst])"
            ));
            for f in path_findings(member) {
                r.problems.push(format!("{label}: {f}"));
            }
            if let Some(why) = rom_size_reason(body.len() as u64) {
                r.problems.push(format!("{label}: {why}"));
            }
            continue;
        };
        let layout = if tree == "data" {
            Layout::DebData
        } else {
            Layout::DebControl
        };
        trees += 1;
        match decompress_bounded(kind, body, *budget).and_then(|t| read_tar(&t, budget)) {
            Ok(entries) => {
                r.entries += entries.len();
                r.problems.extend(inspect(&label, layout, &entries));
            }
            Err(e) => r
                .problems
                .push(format!("{label}: cannot be read completely: {e}")),
        }
    }
    if trees == 0 {
        r.problems
            .push(format!("{name}: has no control.tar or data.tar member"));
    }
}

fn check_appimage(name: &str, data: &[u8], budget: u64, r: &mut AssetReport) {
    let offset = match elf_image_end(data) {
        Ok(o) => o as usize,
        Err(e) => {
            r.problems.push(format!(
                "{name}: the AppImage runtime is not an ELF image ({e})"
            ));
            return;
        }
    };
    let image = &data[offset..];
    let label = format!("{name}!squashfs");
    let sb = match squashfs::superblock(image) {
        Ok(sb) => sb,
        Err(e) => {
            r.problems.push(format!(
                "{label}: no readable SquashFS payload at offset {offset}: {e}"
            ));
            return;
        }
    };
    // mksquashfs pads the image to a multiple of 4 KiB; anything past that
    // is data the runtime never mounts.
    let padded = sb.bytes_used.div_ceil(4096) * 4096;
    if image.len() as u64 > padded {
        let extra = image.len() as u64 - padded;
        r.problems.push(format!(
            "{name}: {extra} bytes appended after the SquashFS payload and its padding (which end at {})",
            offset as u64 + padded
        ));
        if let Some(why) = rom_size_reason(extra) {
            r.problems
                .push(format!("{name}: the appended data is {why}"));
        }
    }
    match squashfs::read(&image[..sb.bytes_used as usize], budget) {
        Ok(entries) => {
            r.entries = entries.len();
            r.problems
                .extend(inspect(&label, Layout::AppImage, &entries));
        }
        Err(e) => r
            .problems
            .push(format!("{label}: cannot be read completely: {e}")),
    }
}

/// The result of checking a directory of release assets.
#[derive(Debug, Default)]
pub struct Report {
    /// `(asset name, entries inspected)`, in name order.
    pub assets: Vec<(String, usize)>,
    pub problems: Vec<String>,
}

/// Checks every file in `dir` (release.yml `publish`'s `assets/`).
pub fn check_dir(dir: &Path) -> Result<Report> {
    let mut names = Vec::new();
    for e in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let e = e.with_context(|| format!("reading {}", dir.display()))?;
        names.push((e.file_name().to_string_lossy().into_owned(), e.file_type()?));
    }
    names.sort_by(|a, b| a.0.cmp(&b.0));
    let mut report = Report::default();
    for (name, ty) in &names {
        if !ty.is_file() {
            report.problems.push(format!(
                "{name}: is not a regular file (a directory or symlink would publish unchecked content)"
            ));
            continue;
        }
        let path = dir.join(name);
        let data = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let r = check_asset(name, &data);
        report.assets.push((name.clone(), r.entries));
        report.problems.extend(r.problems);
    }
    if !report
        .assets
        .iter()
        .any(|(n, _)| asset_kind(n).is_some_and(|k| k != AssetKind::Sums))
    {
        report.problems.push(format!(
            "{}: holds no release artifact to check",
            dir.display()
        ));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rom_sizes_from_the_corpus_and_ec10_are_flagged() {
        // Every image size in the D-025 stock corpus, plus EC-10's 4 MiB.
        for len in [
            163_840, 196_608, 524_288, 1_048_576, 1_048_731, 1_048_784, 1_572_864, 2_097_152,
            4_194_304, 32_768, 65_536, 131_072, 262_144, 8_388_608, 16_777_216,
        ] {
            assert!(rom_size_reason(len).is_some(), "{len} not flagged");
        }
    }

    #[test]
    fn non_rom_sizes_pass() {
        // The committed text assets, a typical binary size, the 2 KiB
        // synthetic fixture (caught by path/extension instead), and sizes
        // just outside the rules.
        for len in [
            0, 212, 435, 512, 2048, 18_036, 18_168, 32_767, 40_000, 981_912, 1_052_672, 16_781_312,
            20_000_000,
        ] {
            assert_eq!(rom_size_reason(len), None, "{len} flagged");
        }
    }

    #[test]
    fn path_findings_cover_fixture_dirs_and_extensions() {
        let f = |p: &str| path_findings(p).join("; ");
        assert!(f("fixtures/public/synthetic/standard.bin").contains("`fixtures`"));
        assert!(f("fixtures/public/synthetic/standard.bin").contains("`.bin`"));
        assert!(f("x/Corpus/USDM/a").contains("`Corpus`"));
        assert!(f("share/AE5I410A.HEX").contains("`.hex`"));
        assert!(f("share/a.srf").contains("`.srf`"));
        assert!(f("share/inner.tar.gz").contains("nested archive"));
        assert!(f("share/standard.manifest.toml").contains("fixture manifest"));
        assert!(f("usr/share/doc/romraider-cli/copyright").is_empty());
        assert!(f(".DirIcon").is_empty());
    }

    #[test]
    fn hex_images_are_recognised() {
        let ihex: String = (0..20)
            .map(|i| format!(":10{i:04X}0000112233445566778899AABBCCDDEEFF00\n"))
            .collect();
        assert!(hex_image_reason(ihex.as_bytes()).is_some());
        let srec: String = (0..20)
            .map(|i| format!("S113{i:04X}00112233445566778899AABBCCDD\n"))
            .collect();
        assert!(hex_image_reason(srec.as_bytes()).is_some());
        assert!(hex_image_reason(b"[Desktop Entry]\nName=x\n").is_none());
    }

    #[test]
    fn normalize_rejects_escapes() {
        assert_eq!(normalize("./usr/bin/").as_deref(), Ok("usr/bin"));
        assert!(normalize("/etc/passwd").is_err());
        assert!(normalize("usr/../../x").is_err());
    }

    #[test]
    fn layouts_allow_only_their_directories() {
        assert!(Layout::DebData.allows_dir("usr/share/icons/hicolor"));
        assert!(!Layout::DebData.allows_dir("usr/share/romraider-cli"));
        assert!(Layout::Tarball.allows_dir("rr-cli-x86_64-unknown-linux-gnu"));
        assert!(!Layout::Tarball.allows_dir("other"));
        assert!(!Layout::Tarball.allows_dir("rr-cli-x/fixtures"));
        assert_eq!(
            Layout::Tarball.expect("rr-cli-x86_64-unknown-linux-gnu/romraider-cli"),
            Some(Expect::Elf)
        );
        assert_eq!(Layout::Tarball.expect("romraider-cli"), None);
    }

    #[test]
    fn asset_kinds() {
        assert_eq!(
            asset_kind("romraider-cli-x86_64-unknown-linux-gnu.tar.gz"),
            Some(AssetKind::Tarball)
        );
        assert_eq!(
            asset_kind("romraider-cli_0.1.0-1_amd64.deb"),
            Some(AssetKind::Deb)
        );
        assert_eq!(
            asset_kind("romraider-cli-x86_64.AppImage"),
            Some(AssetKind::AppImage)
        );
        assert_eq!(asset_kind("SHA256SUMS"), Some(AssetKind::Sums));
        assert_eq!(asset_kind("standard.bin"), None);
        assert_eq!(asset_kind("romraider-cli-x86_64.zip"), None);
    }

    #[test]
    fn bounded_decompression_stops_at_the_limit() {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&[0u8; 100_000]).unwrap();
        let gz = gz.finish().unwrap();
        assert_eq!(
            decompress_bounded("gz", &gz, 100_000).unwrap().len(),
            100_000
        );
        assert!(decompress_bounded("gz", &gz, 99_999).is_err());
        assert!(decompress_bounded("bz2", &gz, 1 << 20).is_err());
        assert!(decompress_bounded("xz", b"not xz", 1 << 20).is_err());
        assert!(decompress_bounded("zst", b"not zstd", 1 << 20).is_err());
    }
}
