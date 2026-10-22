//! Byte image (concept C10, first half): the ROM bytes being edited.
//!
//! The byte image is the only source of truth for a ROM's contents (D-006).
//! Legacy kept a value object per table cell, each with its own "original"
//! and "compare" copies (`maps/DataCell.java`), synced overlapping tables
//! through a byte-to-cell map (`byteCellMapping`, MF-03) and could only undo
//! by reverting a cell to its original. Here there is one owned byte buffer
//! and one snapshot of it as loaded; every table, checksum fix-up and
//! applied diff reads and changes that one buffer, so two tables that cover
//! the same bytes can never disagree.
//!
//! An [`Image`] holds:
//!
//! * the **current bytes**, read with [`Image::bytes`];
//! * the **original snapshot**, the bytes as loaded, read with
//!   [`Image::original`] and never changed afterwards;
//! * the **original hash**, the SHA-256 of that snapshot
//!   ([`Image::original_hash`]), which a sidecar journal records so it is
//!   only ever replayed onto the image it was made against.
//!
//! # Modified bytes
//!
//! A byte is **modified** when it differs from the original snapshot. That
//! is the editor's "changed" marker, computed from the bytes themselves
//! rather than tracked per cell as legacy did (`DataCell` compared each
//! cell's value with its `originalValue`), so it is right however the bytes
//! got there: a value edited and then edited back is not modified, and two
//! tables over the same bytes always agree.
//!
//! * [`Image::is_modified`]: any byte at all;
//! * [`Image::is_byte_modified`]: one offset;
//! * [`Image::is_range_modified`]: any byte in a range (one cell, one table);
//! * [`Image::modified_ranges`] and [`Image::modified_ranges_in`]: the
//!   modified bytes as sorted, coalesced ranges, for markers in a view.
//!
//! All are read-only (`&self`).
//!
//! # Mutation only through patches
//!
//! The image has no public method that changes its bytes. The one mutator,
//! `apply_patch`, is `pub(crate)`: only the patch journal (`rr-rom`'s
//! [`journal`](crate::journal) module) calls it, and the journal records
//! every call as a [`Patch`](crate::journal::Patch). So every change to a
//! ROM can be undone, redone, listed and saved, and nothing outside this
//! crate can change a byte without leaving that record.
//!
//! ```compile_fail,E0624
//! let mut image = rr_rom::image::Image::new(vec![0x00]);
//! // Not reachable outside rr-rom (E0624, private method): bytes change
//! // only through the journal.
//! let _ = image.apply_patch(0, &[0x00], &[0xFF]);
//! ```
//!
//! US-RR-31-4: `tests/no_unpatched_mutation.rs` checks this, and every other
//! route to the bytes (`DerefMut`, `IndexMut`, `AsMut`, a journal's image),
//! with pinned compile-fail diagnostics and a positive control.
//!
//! `apply_patch(at, old, new)` replaces the bytes at `at..at + old.len()`
//! with `new`, and refuses (changing nothing) when:
//!
//! * `old` and `new` differ in length: a patch never resizes the image
//!   ([`ApplyError::LengthMismatch`]);
//! * the range reaches past the end of the image, or past `usize::MAX`
//!   ([`ApplyError::OutOfBounds`]);
//! * the bytes in the range are not `old` ([`ApplyError::Stale`]): the patch
//!   was made against a different state of the image, and applying it would
//!   silently lose whatever changed those bytes since.
//!
//! Undoing a patch is applying it backwards, `apply_patch(at, new, old)`,
//! under the same checks, so an undo can never clobber a later change
//! either. Every check runs before the first byte is written: a refused
//! patch leaves the image exactly as it was.
//!
//! # Never panics
//!
//! Image bytes come from untrusted files and patches from untrusted
//! sidecar journals. Every range end is a checked addition and every index
//! a bounds-checked slice: a bad patch is an [`ApplyError`], never a panic.
//! The module holds no `static` of any kind.
//!
//! # Example
//!
//! ```
//! use rr_rom::image::{Image, ImageHash};
//!
//! let image = Image::new(vec![0x12, 0x34, 0x56]);
//! assert_eq!(image.bytes(), &[0x12, 0x34, 0x56]);
//! assert_eq!(image.original(), image.bytes());
//! assert!(!image.is_modified());
//! assert_eq!(image.original_hash(), ImageHash::of(&[0x12, 0x34, 0x56]));
//! assert_eq!(image.original_hash(), image.current_hash());
//! ```

use core::fmt;
use core::ops::Range;

use sha2::{Digest as _, Sha256};

/// A ROM byte image: the current bytes plus a snapshot of them as loaded.
///
/// See the [module documentation](self). `Debug` prints the length and the
/// original hash, not the bytes (an image is typically 0.5 to 2 MiB).
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    /// The bytes as they are now. Changed only by [`Image::apply_patch`].
    bytes: Vec<u8>,
    /// The bytes as loaded. Never changed.
    original: Box<[u8]>,
    /// SHA-256 of `original`, computed once at load.
    original_hash: ImageHash,
}

impl Image {
    /// An image of `bytes`, which are also its original snapshot.
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        let original: Box<[u8]> = bytes.as_slice().into();
        let original_hash = ImageHash::of(&original);
        Self {
            bytes,
            original,
            original_hash,
        }
    }

    /// The current bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The bytes as loaded, before any patch.
    #[must_use]
    pub fn original(&self) -> &[u8] {
        &self.original
    }

    /// SHA-256 of [`original`](Self::original), the identity a sidecar
    /// journal is bound to.
    #[must_use]
    pub const fn original_hash(&self) -> ImageHash {
        self.original_hash
    }

    /// SHA-256 of the current bytes, computed on each call.
    #[must_use]
    pub fn current_hash(&self) -> ImageHash {
        ImageHash::of(&self.bytes)
    }

    /// The image length in bytes. Patches never change it.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether the image has no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Whether any byte differs from the original snapshot. A byte changed
    /// and then changed back is not modified.
    #[must_use]
    pub fn is_modified(&self) -> bool {
        *self.bytes != *self.original
    }

    /// Whether the byte at `offset` differs from the original snapshot.
    /// `false` past the end of the image.
    #[must_use]
    pub fn is_byte_modified(&self, offset: usize) -> bool {
        self.bytes.get(offset) != self.original.get(offset)
    }

    /// Whether any byte in `range` differs from the original snapshot: the
    /// "modified" marker of a table cell or a whole table over those bytes.
    /// The part of `range` past the end of the image is ignored, and an
    /// empty or reversed range is never modified.
    #[must_use]
    pub fn is_range_modified(&self, range: Range<usize>) -> bool {
        let (_, current, original) = self.window(range);
        current != original
    }

    /// Every run of bytes that differs from the original snapshot, as
    /// sorted, non-overlapping ranges, with neighbouring differing bytes
    /// merged into one range (so two ranges always have an unmodified byte
    /// between them). Empty when the image is not
    /// [modified](Self::is_modified).
    ///
    /// A byte changed and then changed back is not modified. Read-only: the
    /// ranges describe the image, they cannot change it.
    #[must_use]
    pub fn modified_ranges(&self) -> Vec<Range<usize>> {
        self.modified_ranges_in(0..self.bytes.len())
    }

    /// [`modified_ranges`](Self::modified_ranges) within `range` only, each
    /// clipped to it: what a view of those bytes (one table, one visible
    /// page of a hex view) needs to mark. The part of `range` past the end
    /// of the image is ignored; an empty or reversed range has none.
    #[must_use]
    pub fn modified_ranges_in(&self, range: Range<usize>) -> Vec<Range<usize>> {
        let (start, current, original) = self.window(range);
        let mut ranges: Vec<Range<usize>> = Vec::new();
        for (i, (now, was)) in current.iter().zip(original).enumerate() {
            if now == was {
                continue;
            }
            // Cannot overflow: `offset` is an index into `self.bytes`, and a
            // `Vec` holds at most `isize::MAX` bytes.
            let offset = start + i;
            match ranges.last_mut() {
                Some(last) if last.end == offset => last.end = offset + 1,
                _ => ranges.push(offset..offset + 1),
            }
        }
        ranges
    }

    /// The current and original bytes in `range` clipped to the image, and
    /// the offset they start at. A reversed range is empty.
    fn window(&self, range: Range<usize>) -> (usize, &[u8], &[u8]) {
        let end = range.end.min(self.bytes.len());
        let start = range.start.min(end);
        // Both always succeed: `start <= end <= len`, and the snapshot is as
        // long as the current bytes (a patch never resizes the image).
        let current = self.bytes.get(start..end).unwrap_or_default();
        let original = self.original.get(start..end).unwrap_or_default();
        (start, current, original)
    }

    /// Replaces the bytes at `at..at + old.len()`, which must currently be
    /// `old`, with `new`. Changes nothing on error. Undo is
    /// `apply_patch(at, new, old)`.
    ///
    /// `pub(crate)` so that only the patch journal can change image bytes
    /// (see the [module documentation](self)).
    ///
    /// # Errors
    ///
    /// [`ApplyError::LengthMismatch`] if `old` and `new` differ in length,
    /// [`ApplyError::OutOfBounds`] if the range does not lie within the
    /// image, [`ApplyError::Stale`] if the range does not hold `old`.
    pub(crate) fn apply_patch(
        &mut self,
        at: usize,
        old: &[u8],
        new: &[u8],
    ) -> Result<(), ApplyError> {
        if old.len() != new.len() {
            return Err(ApplyError::LengthMismatch {
                old: old.len(),
                new: new.len(),
            });
        }
        let image_len = self.bytes.len();
        let out_of_bounds = ApplyError::OutOfBounds {
            at,
            len: old.len(),
            image_len,
        };
        let Some(end) = at.checked_add(old.len()) else {
            return Err(out_of_bounds);
        };
        let Some(target) = self.bytes.get_mut(at..end) else {
            return Err(out_of_bounds);
        };
        if let Some((i, (&found, &expected))) = target
            .iter()
            .zip(old)
            .enumerate()
            .find(|(_, (found, expected))| found != expected)
        {
            return Err(ApplyError::Stale {
                offset: at + i,
                expected,
                found,
            });
        }
        target.copy_from_slice(new);
        Ok(())
    }
}

impl From<Vec<u8>> for Image {
    fn from(bytes: Vec<u8>) -> Self {
        Self::new(bytes)
    }
}

impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Image")
            .field("len", &self.bytes.len())
            .field("original_hash", &self.original_hash)
            .field("modified", &self.is_modified())
            .finish()
    }
}

/// A SHA-256 digest identifying a byte image.
///
/// `Display` (and `Debug`) write it as 64 lower-case hex digits, the form
/// `sha256sum` prints.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImageHash([u8; 32]);

impl ImageHash {
    /// The SHA-256 of `bytes`.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    /// A hash from its 32 digest bytes, as read back from a stored form.
    #[must_use]
    pub const fn from_bytes(digest: [u8; 32]) -> Self {
        Self(digest)
    }

    /// The 32 digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for ImageHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl fmt::Debug for ImageHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ImageHash({self})")
    }
}

/// Why a patch could not be applied to an [`Image`]. The image is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyError {
    /// The old and new byte strings differ in length; a patch never resizes
    /// the image.
    LengthMismatch {
        /// Length of the old bytes.
        old: usize,
        /// Length of the new bytes.
        new: usize,
    },
    /// The range `at..at + len` does not lie within the image (or its end
    /// overflows `usize`).
    OutOfBounds {
        /// First byte of the range.
        at: usize,
        /// Length of the range.
        len: usize,
        /// Length of the image.
        image_len: usize,
    },
    /// The image does not hold the patch's old bytes: the patch was made
    /// against a different state of the image. Reports the first byte that
    /// differs.
    Stale {
        /// Image offset of the first differing byte.
        offset: usize,
        /// The byte the patch expected there.
        expected: u8,
        /// The byte the image holds there.
        found: u8,
    },
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::LengthMismatch { old, new } => write!(
                f,
                "patch replaces {old} bytes with {new}; a patch may not resize the image"
            ),
            Self::OutOfBounds { at, len, image_len } => write!(
                f,
                "patch of {len} bytes at {at:#x} reaches past the end of the {image_len}-byte image"
            ),
            Self::Stale {
                offset,
                expected,
                found,
            } => write!(
                f,
                "stale patch: expected byte {expected:#04x} at {offset:#x}, the image holds {found:#04x}"
            ),
        }
    }
}

impl core::error::Error for ApplyError {}

#[cfg(test)]
mod tests;
