//! A minimal read-only SquashFS 4.0 reader: just enough to list every entry
//! of an AppImage's payload with its type, symlink target and file bytes, for
//! `cargo xtask check-release-artifacts` (US-RR-38-8, [`crate::release_artifacts`]).
//!
//! Pure Rust (no `unsquashfs`, no FUSE, nothing executed), so the check and
//! its tests run on every CI target. Little-endian SquashFS 4.0 only, as
//! written by mksquashfs / appimagetool; gzip (zlib), xz and zstd
//! compression. Anything else (lzo, lz4, lzma, another version, a malformed
//! table) is an error, never a partial listing: the content check fails
//! closed.
//!
//! Layout reference: the SquashFS 4.0 on-disk format as documented in the
//! Linux kernel (`fs/squashfs/squashfs_fs.h`) and squashfs-tools.

use crate::release_artifacts::{Entry, Node, decompress_bounded};

/// `hsqs`, little-endian.
pub const MAGIC: u32 = 0x7371_7368;
/// Superblock size in bytes.
pub const SUPERBLOCK_LEN: usize = 96;
/// A file inode's `fragment` index when its tail is not in a fragment.
pub const NO_FRAGMENT: u32 = 0xFFFF_FFFF;
/// Uncompressed size of a full metadata block.
pub const METADATA_BLOCK: usize = 8192;
/// Metadata block header bit: the block is stored uncompressed.
pub const META_UNCOMPRESSED: u16 = 0x8000;
/// Data / fragment block size bit: the block is stored uncompressed.
pub const DATA_UNCOMPRESSED: u32 = 1 << 24;
/// Fragment entries per metadata block (16 bytes each).
const FRAGMENTS_PER_BLOCK: u32 = 512;
/// Deepest directory nesting walked.
const MAX_DEPTH: usize = 64;
/// Most directory entries walked in one image.
const MAX_ENTRIES: usize = 100_000;
/// Longest symlink target accepted.
const MAX_SYMLINK: u32 = 4096;

/// Compressors this reader can decode (SquashFS compression ids 1, 4, 6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compression {
    /// Id 1: zlib streams ("gzip" in mksquashfs).
    Gzip,
    /// Id 4.
    Xz,
    /// Id 6.
    Zstd,
}

impl Compression {
    /// The SquashFS compression id.
    pub fn id(self) -> u16 {
        match self {
            Compression::Gzip => 1,
            Compression::Xz => 4,
            Compression::Zstd => 6,
        }
    }

    fn from_id(id: u16) -> Result<Self, String> {
        match id {
            1 => Ok(Compression::Gzip),
            4 => Ok(Compression::Xz),
            6 => Ok(Compression::Zstd),
            2 => Err("squashfs: lzma compression is not supported".to_owned()),
            3 => Err("squashfs: lzo compression is not supported".to_owned()),
            5 => Err("squashfs: lz4 compression is not supported".to_owned()),
            other => Err(format!("squashfs: unknown compression id {other}")),
        }
    }
}

/// The superblock fields this reader uses.
#[derive(Clone, Copy, Debug)]
pub struct Superblock {
    pub inode_count: u32,
    pub block_size: u32,
    pub fragment_count: u32,
    pub compression: Compression,
    pub root_inode: u64,
    /// Length of the filesystem image in bytes (before any padding).
    pub bytes_used: u64,
    pub inode_table: u64,
    pub directory_table: u64,
    pub fragment_table: u64,
}

fn slice(data: &[u8], at: u64, len: u64) -> Result<&[u8], String> {
    let start = usize::try_from(at).map_err(|_| format!("squashfs: offset {at} out of range"))?;
    let len = usize::try_from(len).map_err(|_| format!("squashfs: length {len} out of range"))?;
    start
        .checked_add(len)
        .and_then(|end| data.get(start..end))
        .ok_or_else(|| {
            format!(
                "squashfs: {len} bytes at {at} run past the end of the image ({} bytes)",
                data.len()
            )
        })
}

fn le_u16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn le_u32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn le_u64(b: &[u8], at: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(a)
}

/// Parses the superblock at the start of `data`.
pub fn superblock(data: &[u8]) -> Result<Superblock, String> {
    let b = data
        .get(..SUPERBLOCK_LEN)
        .ok_or("squashfs: shorter than a superblock")?;
    if le_u32(b, 0) != MAGIC {
        return Err("squashfs: no `hsqs` superblock magic (not a little-endian SquashFS)".into());
    }
    let (major, minor) = (le_u16(b, 28), le_u16(b, 30));
    if (major, minor) != (4, 0) {
        return Err(format!(
            "squashfs: version {major}.{minor} is not supported (4.0 only)"
        ));
    }
    let block_size = le_u32(b, 12);
    if !block_size.is_power_of_two() || !(4096..=1 << 20).contains(&block_size) {
        return Err(format!("squashfs: bad block size {block_size}"));
    }
    let sb = Superblock {
        inode_count: le_u32(b, 4),
        block_size,
        fragment_count: le_u32(b, 16),
        compression: Compression::from_id(le_u16(b, 20))?,
        root_inode: le_u64(b, 32),
        bytes_used: le_u64(b, 40),
        inode_table: le_u64(b, 64),
        directory_table: le_u64(b, 72),
        fragment_table: le_u64(b, 80),
    };
    if sb.bytes_used > data.len() as u64 {
        return Err(format!(
            "squashfs: bytes_used {} is past the end of the image ({} bytes)",
            sb.bytes_used,
            data.len()
        ));
    }
    Ok(sb)
}

/// A parsed image: the bytes and its superblock.
struct Fs<'a> {
    data: &'a [u8],
    sb: Superblock,
}

/// Reads metadata bytes sequentially across 8 KiB metadata blocks.
struct Cursor<'f, 'a> {
    fs: &'f Fs<'a>,
    /// Absolute offset of the next block to load.
    next: u64,
    buf: Vec<u8>,
    pos: usize,
}

impl<'a> Fs<'a> {
    /// Decodes the metadata block at absolute offset `at`; returns its bytes
    /// and the offset of the block after it.
    fn metadata_block(&self, at: u64) -> Result<(Vec<u8>, u64), String> {
        let header = slice(self.data, at, 2)?;
        let header = u16::from_le_bytes([header[0], header[1]]);
        let len = u64::from(header & !META_UNCOMPRESSED);
        let raw = slice(self.data, at + 2, len)?;
        let block = if header & META_UNCOMPRESSED != 0 {
            raw.to_vec()
        } else {
            self.decompress(raw, METADATA_BLOCK)?
        };
        if block.len() > METADATA_BLOCK {
            return Err(format!(
                "squashfs: metadata block at {at} is {} bytes",
                block.len()
            ));
        }
        Ok((block, at + 2 + len))
    }

    fn decompress(&self, raw: &[u8], max: usize) -> Result<Vec<u8>, String> {
        let ext = match self.sb.compression {
            Compression::Gzip => "zlib",
            Compression::Xz => "xz",
            Compression::Zstd => "zst",
        };
        decompress_bounded(ext, raw, max as u64).map_err(|e| format!("squashfs: {e}"))
    }

    /// A cursor at `offset` within the metadata block `block` bytes into the
    /// table starting at `table`.
    fn cursor(&self, table: u64, block: u64, offset: u16) -> Result<Cursor<'_, 'a>, String> {
        let start = table
            .checked_add(block)
            .ok_or("squashfs: metadata reference overflows")?;
        let (buf, next) = self.metadata_block(start)?;
        let pos = usize::from(offset);
        if pos > buf.len() {
            return Err(format!(
                "squashfs: offset {pos} is past the end of the metadata block at {start}"
            ));
        }
        Ok(Cursor {
            fs: self,
            next,
            buf,
            pos,
        })
    }

    fn inode(&self, reference: u64) -> Result<Inode, String> {
        let mut c = self.cursor(
            self.sb.inode_table,
            reference >> 16,
            (reference & 0xFFFF) as u16,
        )?;
        let header = c.take(16)?;
        let kind = le_u16(&header, 0);
        Ok(match kind {
            1 => {
                let b = c.take(16)?;
                Inode::Dir {
                    block: u64::from(le_u32(&b, 0)),
                    size: u32::from(le_u16(&b, 8)),
                    offset: le_u16(&b, 10),
                }
            }
            8 => {
                let b = c.take(24)?;
                Inode::Dir {
                    size: le_u32(&b, 4),
                    block: u64::from(le_u32(&b, 8)),
                    offset: le_u16(&b, 18),
                }
            }
            2 => {
                let b = c.take(16)?;
                let (start, fragment) = (u64::from(le_u32(&b, 0)), le_u32(&b, 4));
                let (frag_offset, size) = (le_u32(&b, 8), u64::from(le_u32(&b, 12)));
                let blocks = self.block_sizes(&mut c, size, fragment)?;
                Inode::File {
                    start,
                    size,
                    fragment,
                    frag_offset,
                    blocks,
                }
            }
            9 => {
                let b = c.take(40)?;
                let (start, size) = (le_u64(&b, 0), le_u64(&b, 8));
                let (fragment, frag_offset) = (le_u32(&b, 28), le_u32(&b, 32));
                let blocks = self.block_sizes(&mut c, size, fragment)?;
                Inode::File {
                    start,
                    size,
                    fragment,
                    frag_offset,
                    blocks,
                }
            }
            3 | 10 => {
                let b = c.take(8)?;
                let len = le_u32(&b, 4);
                if len > MAX_SYMLINK {
                    return Err(format!("squashfs: symlink target of {len} bytes"));
                }
                let target = c.take(len as usize)?;
                Inode::Symlink(String::from_utf8_lossy(&target).into_owned())
            }
            4 | 11 => Inode::Other("block device"),
            5 | 12 => Inode::Other("character device"),
            6 | 13 => Inode::Other("named pipe"),
            7 | 14 => Inode::Other("socket"),
            other => return Err(format!("squashfs: unknown inode type {other}")),
        })
    }

    /// The block size list that follows a file inode.
    fn block_sizes(
        &self,
        c: &mut Cursor<'_, '_>,
        size: u64,
        fragment: u32,
    ) -> Result<Vec<u32>, String> {
        let bs = u64::from(self.sb.block_size);
        let count = if fragment == NO_FRAGMENT {
            size.div_ceil(bs)
        } else {
            size / bs
        };
        if count > self.data.len() as u64 {
            return Err(format!(
                "squashfs: a {size}-byte file has more blocks than the image has bytes"
            ));
        }
        let raw = c.take(count as usize * 4)?;
        Ok(raw.chunks_exact(4).map(|w| le_u32(w, 0)).collect())
    }

    /// The decoded fragment block `index`.
    fn fragment(&self, index: u32) -> Result<Vec<u8>, String> {
        if index >= self.sb.fragment_count {
            return Err(format!(
                "squashfs: fragment {index} of {}",
                self.sb.fragment_count
            ));
        }
        let lookup = slice(
            self.data,
            self.sb.fragment_table + u64::from(index / FRAGMENTS_PER_BLOCK) * 8,
            8,
        )?;
        let (block, _) = self.metadata_block(le_u64(lookup, 0))?;
        let at = (index % FRAGMENTS_PER_BLOCK) as usize * 16;
        let entry = block
            .get(at..at + 16)
            .ok_or_else(|| format!("squashfs: fragment entry {index} is missing"))?;
        self.data_block(le_u64(entry, 0), le_u32(entry, 8))
    }

    /// The decoded data or fragment block of on-disk `size` word at `at`.
    fn data_block(&self, at: u64, size: u32) -> Result<Vec<u8>, String> {
        let len = u64::from(size & (DATA_UNCOMPRESSED - 1));
        let raw = slice(self.data, at, len)?;
        if size & DATA_UNCOMPRESSED != 0 {
            Ok(raw.to_vec())
        } else {
            self.decompress(raw, self.sb.block_size as usize)
        }
    }

    fn file_bytes(&self, file: &Inode, budget: &mut u64) -> Result<Vec<u8>, String> {
        let Inode::File {
            start,
            size,
            fragment,
            frag_offset,
            blocks,
        } = file
        else {
            return Err("squashfs: not a file".into());
        };
        if *size > *budget {
            return Err(format!(
                "squashfs: a {size}-byte file exceeds the unpack budget"
            ));
        }
        *budget -= size;
        let bs = u64::from(self.sb.block_size);
        let mut out = Vec::with_capacity(*size as usize);
        let mut at = *start;
        for &b in blocks {
            let want = (*size - out.len() as u64).min(bs) as usize;
            if b == 0 {
                // A sparse block: all zeros.
                out.resize(out.len() + want, 0);
                continue;
            }
            let block = self.data_block(at, b)?;
            at += u64::from(b & (DATA_UNCOMPRESSED - 1));
            if block.len() < want {
                return Err(format!(
                    "squashfs: data block at {at} decodes to {} bytes, not {want}",
                    block.len()
                ));
            }
            out.extend_from_slice(&block[..want]);
        }
        if *fragment != NO_FRAGMENT {
            let tail = (*size - out.len() as u64) as usize;
            let block = self.fragment(*fragment)?;
            let from = *frag_offset as usize;
            let bytes = from
                .checked_add(tail)
                .and_then(|end| block.get(from..end))
                .ok_or_else(|| {
                    format!("squashfs: fragment {fragment} is shorter than the file's tail")
                })?;
            out.extend_from_slice(bytes);
        }
        if out.len() as u64 != *size {
            return Err(format!(
                "squashfs: file decodes to {} bytes, its inode says {size}",
                out.len()
            ));
        }
        Ok(out)
    }

    /// `(name, inode reference)` for each entry of a directory.
    fn listing(&self, block: u64, offset: u16, size: u32) -> Result<Vec<(String, u64)>, String> {
        // A directory's size counts 3 bytes more than its listing.
        let Some(mut left) = size.checked_sub(3) else {
            return Err(format!("squashfs: directory size {size}"));
        };
        let mut out = Vec::new();
        if left == 0 {
            // An empty directory: its block / offset may point past the
            // directory table, so nothing is read.
            return Ok(out);
        }
        let mut c = self.cursor(self.sb.directory_table, block, offset)?;
        while left > 0 {
            let h = c.take(12)?;
            left = left
                .checked_sub(12)
                .ok_or("squashfs: directory header overruns its listing")?;
            let count = le_u32(&h, 0) + 1;
            let start = u64::from(le_u32(&h, 4));
            if count > 256 {
                return Err(format!("squashfs: directory header with {count} entries"));
            }
            for _ in 0..count {
                let e = c.take(8)?;
                let name_len = u32::from(le_u16(&e, 6)) + 1;
                let name = c.take(name_len as usize)?;
                left = left
                    .checked_sub(8 + name_len)
                    .ok_or("squashfs: directory entry overruns its listing")?;
                let name = String::from_utf8_lossy(&name).into_owned();
                if name.is_empty() || name == "." || name == ".." || name.contains('/') {
                    return Err(format!("squashfs: bad directory entry name {name:?}"));
                }
                out.push((name, (start << 16) | u64::from(le_u16(&e, 0))));
            }
        }
        Ok(out)
    }

    fn walk(
        &self,
        dir: &Inode,
        prefix: &str,
        depth: usize,
        budget: &mut u64,
        out: &mut Vec<Entry>,
    ) -> Result<(), String> {
        let Inode::Dir {
            block,
            offset,
            size,
        } = dir
        else {
            return Err("squashfs: root inode is not a directory".into());
        };
        if depth > MAX_DEPTH {
            return Err(format!(
                "squashfs: directories nest deeper than {MAX_DEPTH}"
            ));
        }
        for (name, reference) in self.listing(*block, *offset, *size)? {
            if out.len() >= MAX_ENTRIES {
                return Err(format!("squashfs: more than {MAX_ENTRIES} entries"));
            }
            let path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let inode = self.inode(reference)?;
            let node = match &inode {
                Inode::Dir { .. } => Node::Dir,
                Inode::File { .. } => Node::File(self.file_bytes(&inode, budget)?),
                Inode::Symlink(t) => Node::Symlink(t.clone()),
                Inode::Other(k) => Node::Other((*k).to_owned()),
            };
            let is_dir = matches!(node, Node::Dir);
            out.push(Entry {
                path: path.clone(),
                node,
            });
            if is_dir {
                self.walk(&inode, &path, depth + 1, budget, out)?;
            }
        }
        Ok(())
    }
}

impl Cursor<'_, '_> {
    /// The next `n` metadata bytes, loading further blocks as needed.
    fn take(&mut self, n: usize) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(n.min(METADATA_BLOCK));
        while out.len() < n {
            if self.pos == self.buf.len() {
                let (buf, next) = self.fs.metadata_block(self.next)?;
                if buf.is_empty() {
                    return Err("squashfs: empty metadata block".into());
                }
                self.buf = buf;
                self.next = next;
                self.pos = 0;
            }
            let k = (n - out.len()).min(self.buf.len() - self.pos);
            out.extend_from_slice(&self.buf[self.pos..self.pos + k]);
            self.pos += k;
        }
        Ok(out)
    }
}

/// An inode, as far as the listing needs it.
enum Inode {
    Dir {
        /// Listing start, relative to the directory table.
        block: u64,
        offset: u16,
        size: u32,
    },
    File {
        start: u64,
        size: u64,
        fragment: u32,
        frag_offset: u32,
        blocks: Vec<u32>,
    },
    Symlink(String),
    Other(&'static str),
}

/// Every entry of the SquashFS image `data` (which starts at its
/// superblock), depth-first, paths relative to the root without a leading
/// `/`, file bytes included. At most `budget` bytes of file content are
/// decoded in total.
pub fn read(data: &[u8], mut budget: u64) -> Result<Vec<Entry>, String> {
    let sb = superblock(data)?;
    let fs = Fs { data, sb };
    let root = fs.inode(sb.root_inode)?;
    let mut out = Vec::new();
    fs.walk(&root, "", 0, &mut budget, &mut out)?;
    // Every inode but the root's is reached through a directory entry
    // (hard links aside, which mksquashfs does not write for an AppDir).
    if out.len() as u64 + 1 < u64::from(sb.inode_count) {
        return Err(format!(
            "squashfs: superblock counts {} inodes, the directory tree reaches {}",
            sb.inode_count,
            out.len() + 1
        ));
    }
    Ok(out)
}
