//! Builders for release-artifact-shaped files, for the
//! `check-release-artifacts` tests (`release_artifacts.rs`, US-RR-38-8):
//! tar (+ gzip / xz / zstd), `ar` (.deb), a minimal ELF image, and a
//! SquashFS 4.0 writer for AppImage payloads.
//!
//! The writers are written from the formats' documentation, independently of
//! the readers in xtask/src/ (squashfs.rs, release_artifacts.rs), so a reader
//! bug is not mirrored here. The reader is also checked against real
//! appimagetool / mksquashfs output (see the task log of US-RR-38-8); this
//! writer's images unpack with `unsquashfs`.

#![allow(dead_code, reason = "each test uses a different subset")]
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only builders outside #[test] fns; panicking is how they fail the calling test"
)]

use std::collections::BTreeMap;
use std::io::Write as _;

/// One entry to put in an archive. Paths are relative, `/`-separated.
#[derive(Clone, Debug)]
pub enum Item {
    Dir(String),
    File(String, Vec<u8>),
    Symlink(String, String),
}

pub fn dir(p: &str) -> Item {
    Item::Dir(p.to_owned())
}

pub fn file(p: &str, bytes: impl Into<Vec<u8>>) -> Item {
    Item::File(p.to_owned(), bytes.into())
}

pub fn symlink(p: &str, target: &str) -> Item {
    Item::Symlink(p.to_owned(), target.to_owned())
}

/// Deterministic bytes that look like firmware: no structure, plenty of
/// NULs and high bytes, not an ELF image.
pub fn rom_bytes(len: usize) -> Vec<u8> {
    let mut x: u32 = 0x1234_5678;
    (0..len)
        .map(|i| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            if i % 7 == 0 { 0 } else { (x >> 24) as u8 }
        })
        .collect()
}

/// A minimal, well-formed little-endian ELF64 executable whose only section
/// holds `payload`; the file ends at the end of its section header table.
pub fn fake_elf(payload: &[u8]) -> Vec<u8> {
    let shoff = 64 + payload.len() as u64;
    let mut h = vec![0u8; 64];
    h[..4].copy_from_slice(b"\x7fELF");
    h[4] = 2; // ELFCLASS64
    h[5] = 1; // little-endian
    h[6] = 1; // EV_CURRENT
    h[16..18].copy_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    h[18..20].copy_from_slice(&0x3Eu16.to_le_bytes()); // x86-64
    h[20..24].copy_from_slice(&1u32.to_le_bytes());
    h[40..48].copy_from_slice(&shoff.to_le_bytes());
    h[52..54].copy_from_slice(&64u16.to_le_bytes()); // e_ehsize
    h[54..56].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize
    h[58..60].copy_from_slice(&64u16.to_le_bytes()); // e_shentsize
    h[60..62].copy_from_slice(&2u16.to_le_bytes()); // e_shnum
    let mut out = h;
    out.extend_from_slice(payload);
    out.extend_from_slice(&[0u8; 64]); // SHN_UNDEF
    let mut sh = [0u8; 64];
    sh[4..8].copy_from_slice(&1u32.to_le_bytes()); // SHT_PROGBITS
    sh[24..32].copy_from_slice(&64u64.to_le_bytes());
    sh[32..40].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(&sh);
    out
}

/// A [`fake_elf`] of exactly `len` bytes (`len` >= 192).
pub fn fake_elf_of_len(len: usize) -> Vec<u8> {
    let out = fake_elf(&vec![0x90; len - 192]);
    assert_eq!(out.len(), len);
    out
}

/// An uncompressed tar archive of `items`, in order.
pub fn tar(items: &[Item]) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    for item in items {
        let mut h = tar::Header::new_gnu();
        h.set_mtime(0);
        h.set_uid(0);
        h.set_gid(0);
        match item {
            Item::Dir(p) => {
                h.set_entry_type(tar::EntryType::Directory);
                h.set_mode(0o755);
                h.set_size(0);
                b.append_data(&mut h, format!("{}/", p.trim_end_matches('/')), &[][..])
                    .unwrap();
            }
            Item::File(p, bytes) => {
                h.set_entry_type(tar::EntryType::Regular);
                h.set_mode(0o644);
                h.set_size(bytes.len() as u64);
                b.append_data(&mut h, p, &bytes[..]).unwrap();
            }
            Item::Symlink(p, target) => {
                h.set_entry_type(tar::EntryType::Symlink);
                h.set_mode(0o777);
                h.set_size(0);
                b.append_link(&mut h, p, target).unwrap();
            }
        }
    }
    b.into_inner().unwrap()
}

/// [`tar`] of `items` plus a file stored under the raw name `path`, which
/// `tar::Builder` would refuse (e.g. a `..` path).
pub fn tar_with_raw_path(items: &[Item], path: &str, bytes: &[u8]) -> Vec<u8> {
    let mut out = tar(items);
    // Drop the two zero end-of-archive blocks, append, and end again.
    while out.len() >= 512 && out[out.len() - 512..].iter().all(|&b| b == 0) {
        out.truncate(out.len() - 512);
    }
    let mut h = tar::Header::new_gnu();
    h.as_gnu_mut().unwrap().name[..path.len()].copy_from_slice(path.as_bytes());
    h.set_entry_type(tar::EntryType::Regular);
    h.set_mode(0o644);
    h.set_size(bytes.len() as u64);
    h.set_mtime(0);
    h.set_cksum();
    out.extend_from_slice(h.as_bytes());
    out.extend_from_slice(bytes);
    out.resize(out.len().div_ceil(512) * 512, 0);
    out.extend_from_slice(&[0u8; 1024]);
    out
}

pub fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(bytes).unwrap();
    e.finish().unwrap()
}

pub fn zlib(bytes: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(bytes).unwrap();
    e.finish().unwrap()
}

pub fn xz(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    lzma_rs::xz_compress(&mut std::io::BufReader::new(bytes), &mut out).unwrap();
    out
}

pub fn zstd(bytes: &[u8]) -> Vec<u8> {
    ruzstd::encoding::compress_to_vec(bytes, ruzstd::encoding::CompressionLevel::Fastest)
}

/// `bytes` compressed as a .deb member suffix: `` (none), `gz`, `xz`, `zst`.
pub fn compress(kind: &str, bytes: &[u8]) -> Vec<u8> {
    match kind {
        "" => bytes.to_vec(),
        "gz" => gzip(bytes),
        "xz" => xz(bytes),
        "zst" => zstd(bytes),
        other => panic!("no compressor {other}"),
    }
}

/// A System V / GNU `ar` archive of `members`, as dpkg-deb writes it.
pub fn ar(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"!<arch>\n".to_vec();
    for (name, body) in members {
        let header = format!(
            "{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
            name,
            0,
            0,
            0,
            "100644",
            body.len()
        );
        assert_eq!(header.len(), 60);
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(body);
        if body.len() % 2 == 1 {
            out.push(b'\n');
        }
    }
    out
}

/// `./`-prefixed copies of `items`, as dpkg-deb lays out tar members.
pub fn dot_slash(items: &[Item]) -> Vec<Item> {
    let mut out = vec![dir(".")];
    out.extend(items.iter().map(|i| match i {
        Item::Dir(p) => dir(&format!("./{p}")),
        Item::File(p, b) => file(&format!("./{p}"), b.clone()),
        Item::Symlink(p, t) => symlink(&format!("./{p}"), t),
    }));
    out
}

/// A .deb: debian-binary, control.tar.gz with `control`, data.tar.`data_kind`
/// with `data` (paths without `./`; it is added).
pub fn deb(control: &[Item], data: &[Item], data_kind: &str) -> Vec<u8> {
    let data_name = if data_kind.is_empty() {
        "data.tar".to_owned()
    } else {
        format!("data.tar.{data_kind}")
    };
    ar(&[
        ("debian-binary", b"2.0\n".to_vec()),
        ("control.tar.gz", gzip(&tar(&dot_slash(control)))),
        (&data_name, compress(data_kind, &tar(&dot_slash(data)))),
    ])
}

// --- SquashFS 4.0 writer ----------------------------------------------------

/// Compression for [`squashfs`]; `None` stores every block uncompressed
/// (the superblock still says gzip, as `mksquashfs -noI -noD -noF -noX`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comp {
    None,
    Gzip,
    Xz,
    Zstd,
}

impl Comp {
    fn id(self) -> u16 {
        match self {
            Comp::None | Comp::Gzip => 1,
            Comp::Xz => 4,
            Comp::Zstd => 6,
        }
    }

    fn compress(self, raw: &[u8]) -> Option<Vec<u8>> {
        let c = match self {
            Comp::None => return None,
            Comp::Gzip => zlib(raw),
            Comp::Xz => xz(raw),
            Comp::Zstd => zstd(raw),
        };
        (c.len() < raw.len()).then_some(c)
    }
}

/// A metadata stream (inode or directory table) of 8 KiB blocks, each with
/// a 2-byte header (bit 15: stored uncompressed).
struct Meta {
    comp: Comp,
    raw: Vec<u8>,
    disk: Vec<u8>,
}

impl Meta {
    fn new(comp: Comp) -> Self {
        Meta {
            comp,
            raw: Vec::new(),
            disk: Vec::new(),
        }
    }

    /// (on-disk start of the current block relative to the table, offset
    /// in it).
    fn pos(&self) -> (u64, u16) {
        (self.disk.len() as u64, self.raw.len() as u16)
    }

    fn write(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            let k = (8192 - self.raw.len()).min(bytes.len());
            self.raw.extend_from_slice(&bytes[..k]);
            bytes = &bytes[k..];
            if self.raw.len() == 8192 {
                self.flush();
            }
        }
    }

    fn flush(&mut self) {
        if self.raw.is_empty() {
            return;
        }
        match self.comp.compress(&self.raw) {
            Some(c) => {
                self.disk.extend_from_slice(&(c.len() as u16).to_le_bytes());
                self.disk.extend_from_slice(&c);
            }
            None => {
                self.disk
                    .extend_from_slice(&(self.raw.len() as u16 | 0x8000).to_le_bytes());
                self.disk.extend_from_slice(&self.raw);
            }
        }
        self.raw.clear();
    }

    fn finish(mut self) -> Vec<u8> {
        self.flush();
        self.disk
    }
}

enum Tree {
    Dir(BTreeMap<String, Tree>),
    File(Vec<u8>),
    Symlink(String),
}

fn insert(root: &mut BTreeMap<String, Tree>, path: &str, leaf: Tree) {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let mut cur = root;
    for (i, part) in parts.iter().enumerate() {
        if i + 1 == parts.len() {
            if let Tree::Dir(_) = leaf {
                cur.entry((*part).to_owned())
                    .or_insert_with(|| Tree::Dir(BTreeMap::new()));
            } else {
                cur.insert((*part).to_owned(), leaf);
            }
            return;
        }
        let next = cur
            .entry((*part).to_owned())
            .or_insert_with(|| Tree::Dir(BTreeMap::new()));
        let Tree::Dir(children) = next else {
            panic!("{path}: a parent is not a directory")
        };
        cur = children;
    }
}

struct Writer {
    comp: Comp,
    block_size: u32,
    /// Everything after the superblock up to the inode table.
    data: Vec<u8>,
    frag_buf: Vec<u8>,
    /// (absolute start, size word) of each fragment block.
    frags: Vec<(u64, u32)>,
    inodes: Meta,
    dirs: Meta,
    next_inode: u32,
}

impl Writer {
    fn abs(&self) -> u64 {
        96 + self.data.len() as u64
    }

    /// Appends a data block; returns its size word.
    fn block(&mut self, raw: &[u8]) -> u32 {
        match self.comp.compress(raw) {
            Some(c) => {
                self.data.extend_from_slice(&c);
                c.len() as u32
            }
            None => {
                self.data.extend_from_slice(raw);
                raw.len() as u32 | (1 << 24)
            }
        }
    }

    fn flush_fragment(&mut self) {
        if self.frag_buf.is_empty() {
            return;
        }
        let start = self.abs();
        let raw = std::mem::take(&mut self.frag_buf);
        let size = self.block(&raw);
        self.frags.push((start, size));
    }

    fn inode_header(&mut self, kind: u16, mode: u16) -> (u64, u32) {
        let (blk, off) = self.inodes.pos();
        let number = self.next_inode;
        self.next_inode += 1;
        let mut h = Vec::new();
        h.extend_from_slice(&kind.to_le_bytes());
        h.extend_from_slice(&mode.to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes()); // uid index
        h.extend_from_slice(&0u16.to_le_bytes()); // gid index
        h.extend_from_slice(&0u32.to_le_bytes()); // mtime
        h.extend_from_slice(&number.to_le_bytes());
        self.inodes.write(&h);
        ((blk << 16) | u64::from(off), number)
    }

    fn file(&mut self, bytes: &[u8]) -> (u64, u32) {
        let bs = self.block_size as usize;
        let start = self.abs();
        let full = bytes.len() / bs;
        let sizes: Vec<u32> = (0..full)
            .map(|i| self.block(&bytes[i * bs..(i + 1) * bs]))
            .collect();
        let tail = &bytes[full * bs..];
        let (frag, frag_off) = if tail.is_empty() {
            (0xFFFF_FFFF, 0)
        } else {
            if self.frag_buf.len() + tail.len() > bs {
                self.flush_fragment();
            }
            let off = self.frag_buf.len() as u32;
            self.frag_buf.extend_from_slice(tail);
            (self.frags.len() as u32, off)
        };
        let (r, n) = self.inode_header(2, 0o644);
        let mut b = Vec::new();
        b.extend_from_slice(&(start as u32).to_le_bytes());
        b.extend_from_slice(&frag.to_le_bytes());
        b.extend_from_slice(&frag_off.to_le_bytes());
        b.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        for s in sizes {
            b.extend_from_slice(&s.to_le_bytes());
        }
        self.inodes.write(&b);
        (r, n)
    }

    fn symlink(&mut self, target: &str) -> (u64, u32) {
        let (r, n) = self.inode_header(3, 0o777);
        let mut b = Vec::new();
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&(target.len() as u32).to_le_bytes());
        b.extend_from_slice(target.as_bytes());
        self.inodes.write(&b);
        (r, n)
    }

    fn dir(&mut self, children: &BTreeMap<String, Tree>) -> (u64, u32) {
        let mut listing = Vec::new();
        let mut subdirs = 0u32;
        for (name, child) in children {
            let (r, n, kind) = match child {
                Tree::Dir(c) => {
                    subdirs += 1;
                    let (r, n) = self.dir(c);
                    (r, n, 1u16)
                }
                Tree::File(b) => {
                    let (r, n) = self.file(b);
                    (r, n, 2)
                }
                Tree::Symlink(t) => {
                    let (r, n) = self.symlink(t);
                    (r, n, 3)
                }
            };
            // One header per entry: count 0 (= 1 entry).
            listing.extend_from_slice(&0u32.to_le_bytes());
            listing.extend_from_slice(&((r >> 16) as u32).to_le_bytes());
            listing.extend_from_slice(&n.to_le_bytes());
            listing.extend_from_slice(&((r & 0xFFFF) as u16).to_le_bytes());
            listing.extend_from_slice(&0i16.to_le_bytes());
            listing.extend_from_slice(&kind.to_le_bytes());
            listing.extend_from_slice(&((name.len() - 1) as u16).to_le_bytes());
            listing.extend_from_slice(name.as_bytes());
        }
        let (blk, off) = self.dirs.pos();
        self.dirs.write(&listing);
        let (r, n) = self.inode_header(1, 0o755);
        let mut b = Vec::new();
        b.extend_from_slice(&(blk as u32).to_le_bytes());
        b.extend_from_slice(&(2 + subdirs).to_le_bytes());
        b.extend_from_slice(&((listing.len() + 3) as u16).to_le_bytes());
        b.extend_from_slice(&off.to_le_bytes());
        b.extend_from_slice(&(n + 1).to_le_bytes()); // parent (unused here)
        self.inodes.write(&b);
        (r, n)
    }
}

/// A SquashFS 4.0 image of `items` (parent directories implied), padded to
/// 4 KiB like mksquashfs.
pub fn squashfs(items: &[Item], comp: Comp, block_size: u32) -> Vec<u8> {
    let mut root = BTreeMap::new();
    for item in items {
        match item {
            Item::Dir(p) => insert(&mut root, p, Tree::Dir(BTreeMap::new())),
            Item::File(p, b) => insert(&mut root, p, Tree::File(b.clone())),
            Item::Symlink(p, t) => insert(&mut root, p, Tree::Symlink(t.clone())),
        }
    }
    let mut w = Writer {
        comp,
        block_size,
        data: Vec::new(),
        frag_buf: Vec::new(),
        frags: Vec::new(),
        inodes: Meta::new(comp),
        dirs: Meta::new(comp),
        next_inode: 1,
    };
    let (root_ref, _) = w.dir(&root);
    w.flush_fragment();
    let inode_count = w.next_inode - 1;

    let mut img = vec![0u8; 96];
    img.extend_from_slice(&w.data);
    let inode_table = img.len() as u64;
    img.extend_from_slice(&std::mem::replace(&mut w.inodes, Meta::new(comp)).finish());
    let dir_table = img.len() as u64;
    img.extend_from_slice(&std::mem::replace(&mut w.dirs, Meta::new(comp)).finish());
    // Fragment entries (512 per metadata block), then their lookup table.
    let mut frag_blocks = Vec::new();
    for chunk in w.frags.chunks(512) {
        frag_blocks.push(img.len() as u64);
        let mut m = Meta::new(comp);
        for (start, size) in chunk {
            m.write(&start.to_le_bytes());
            m.write(&size.to_le_bytes());
            m.write(&0u32.to_le_bytes());
        }
        img.extend_from_slice(&m.finish());
    }
    let frag_table = img.len() as u64;
    for b in &frag_blocks {
        img.extend_from_slice(&b.to_le_bytes());
    }
    // One id (0), then its lookup table.
    let id_block = img.len() as u64;
    let mut m = Meta::new(comp);
    m.write(&0u32.to_le_bytes());
    img.extend_from_slice(&m.finish());
    let id_table = img.len() as u64;
    img.extend_from_slice(&id_block.to_le_bytes());
    let bytes_used = img.len() as u64;

    let flags: u16 = if comp == Comp::None {
        0x0001 | 0x0002 | 0x0008 | 0x0200 | 0x0800
    } else {
        0x0200
    };
    let mut sb = Vec::new();
    sb.extend_from_slice(&0x7371_7368u32.to_le_bytes());
    sb.extend_from_slice(&inode_count.to_le_bytes());
    sb.extend_from_slice(&0u32.to_le_bytes()); // mtime
    sb.extend_from_slice(&block_size.to_le_bytes());
    sb.extend_from_slice(&(w.frags.len() as u32).to_le_bytes());
    sb.extend_from_slice(&comp.id().to_le_bytes());
    sb.extend_from_slice(&(block_size.trailing_zeros() as u16).to_le_bytes());
    sb.extend_from_slice(&flags.to_le_bytes());
    sb.extend_from_slice(&1u16.to_le_bytes()); // id count
    sb.extend_from_slice(&4u16.to_le_bytes());
    sb.extend_from_slice(&0u16.to_le_bytes());
    sb.extend_from_slice(&root_ref.to_le_bytes());
    sb.extend_from_slice(&bytes_used.to_le_bytes());
    sb.extend_from_slice(&id_table.to_le_bytes());
    sb.extend_from_slice(&u64::MAX.to_le_bytes()); // xattr id table
    sb.extend_from_slice(&inode_table.to_le_bytes());
    sb.extend_from_slice(&dir_table.to_le_bytes());
    sb.extend_from_slice(&frag_table.to_le_bytes());
    sb.extend_from_slice(&u64::MAX.to_le_bytes()); // export table
    assert_eq!(sb.len(), 96);
    img[..96].copy_from_slice(&sb);
    img.resize(img.len().div_ceil(4096) * 4096, 0);
    img
}

/// An AppImage: a [`fake_elf`] "runtime" followed by a SquashFS of `items`.
pub fn appimage(items: &[Item], comp: Comp) -> Vec<u8> {
    let mut out = fake_elf(b"type2 runtime stand-in");
    out.extend_from_slice(&squashfs(items, comp, 4096));
    out
}

// --- Clean artifacts, as the committed recipes build them --------------------

/// The tarball's top directory (cargo-dist: `rr-cli-<target>`).
pub const TARBALL_ROOT: &str = "rr-cli-x86_64-unknown-linux-gnu";

pub const DESKTOP: &str = "[Desktop Entry]\nType=Application\nName=RomRaider CLI\nExec=romraider-cli\nIcon=romraider-cli\nTerminal=true\nCategories=Utility;\n";
pub const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 64 64\"><rect width=\"64\" height=\"64\"/></svg>\n";
pub const LICENSE: &str = "GNU GENERAL PUBLIC LICENSE\nVersion 2, June 1991\n";
pub const APPRUN: &str = "#!/bin/sh\nHERE=$(dirname \"$(readlink -f \"$0\")\")\nexec \"${APPDIR:-$HERE}/usr/bin/romraider-cli\" \"$@\"\n";

/// The binary: a complete ELF image of exactly 1 MiB, a ROM size, so a
/// clean pass also proves the ELF exemption.
pub fn binary() -> Vec<u8> {
    fake_elf_of_len(1 << 20)
}

pub fn tarball_items() -> Vec<Item> {
    let r = TARBALL_ROOT;
    vec![
        dir(r),
        file(&format!("{r}/README.md"), "# RomRaider-RS\n"),
        file(&format!("{r}/romraider-cli"), binary()),
        file(&format!("{r}/license.txt"), LICENSE),
    ]
}

pub fn deb_control_items() -> Vec<Item> {
    vec![
        file(
            "control",
            "Package: romraider-cli\nVersion: 0.1.0-1\nArchitecture: amd64\n",
        ),
        file(
            "md5sums",
            "0123456789abcdef0123456789abcdef  usr/bin/romraider-cli\n",
        ),
    ]
}

pub fn deb_data_items() -> Vec<Item> {
    vec![
        dir("usr"),
        dir("usr/share"),
        dir("usr/share/applications"),
        file("usr/share/applications/romraider-cli.desktop", DESKTOP),
        dir("usr/share/icons"),
        dir("usr/share/icons/hicolor"),
        dir("usr/share/icons/hicolor/scalable"),
        dir("usr/share/icons/hicolor/scalable/apps"),
        file(
            "usr/share/icons/hicolor/scalable/apps/romraider-cli.svg",
            SVG,
        ),
        dir("usr/share/doc"),
        dir("usr/share/doc/romraider-cli"),
        file("usr/share/doc/romraider-cli/copyright", LICENSE),
        dir("usr/bin"),
        file("usr/bin/romraider-cli", binary()),
    ]
}

pub fn appimage_items() -> Vec<Item> {
    vec![
        symlink(".DirIcon", "romraider-cli.svg"),
        file("AppRun", APPRUN),
        file("romraider-cli.desktop", DESKTOP),
        file("romraider-cli.svg", SVG),
        file("usr/bin/romraider-cli", binary()),
        file("usr/share/applications/romraider-cli.desktop", DESKTOP),
        file(
            "usr/share/icons/hicolor/scalable/apps/romraider-cli.svg",
            SVG,
        ),
        file("usr/share/licenses/romraider-cli/license.txt", LICENSE),
    ]
}

pub const TARBALL: &str = "romraider-cli-x86_64-unknown-linux-gnu.tar.gz";
pub const DEB: &str = "romraider-cli_0.1.0-1_amd64.deb";
pub const APPIMAGE: &str = "romraider-cli-x86_64.AppImage";

pub fn clean_tarball() -> Vec<u8> {
    gzip(&tar(&tarball_items()))
}

pub fn clean_deb() -> Vec<u8> {
    deb(&deb_control_items(), &deb_data_items(), "xz")
}

pub fn clean_appimage() -> Vec<u8> {
    appimage(&appimage_items(), Comp::Zstd)
}
