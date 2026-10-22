//! Patch journal (concept C10, second half): every change to a byte image.
//!
//! Legacy had no history at all: each table cell kept an "original" copy
//! (`maps/DataCell.java`) and "undo" could only revert a cell to it, so a
//! second edit lost the first and there was no redo. Here every change to an
//! [`Image`] (a user edit, a paste, a checksum fix-up, an applied diff) is a
//! [`Patch`]: the exact bytes it replaced and the bytes it wrote. A
//! [`Journal`] owns the image and the list of patches applied to it, so it
//! can step back and forward through every change, byte-exact.
//!
//! # Patches
//!
//! A [`Patch`] replaces the bytes in its [`range`](Patch::range), which must
//! hold [`old_bytes`](Patch::old_bytes), with
//! [`new_bytes`](Patch::new_bytes), and records its [`Origin`]. The old and
//! new bytes are the same length (a patch never resizes the image) and the
//! range end fits in `usize`; [`Patch::new`] refuses anything else with a
//! [`PatchError`], so every `Patch` value is well-formed.
//! [`Patch::replacing`] builds one with its old bytes read from an image.
//!
//! # Groups: one user action, one undo step
//!
//! Patches are applied in [`Group`]s. A group is what one user action
//! changed (a cell edit, a paste over a selection, one checksum fix-up), and
//! undo and redo move a whole group at a time. [`Journal::apply_group`]
//! applies a group's patches in order, each against the image as the ones
//! before it left it, and is atomic: if any patch is refused, the ones
//! already applied are reverted and the journal is exactly as it was.
//! [`Journal::apply`] is the one-patch group.
//!
//! # Undo and redo
//!
//! The journal is a list of groups and a cursor into it. Groups before the
//! cursor are applied ([`Journal::applied`]); groups after it were undone
//! and can be redone ([`Journal::undone`]).
//!
//! * [`Journal::undo`] reverts the last applied group, its patches in
//!   reverse order, and moves the cursor back one.
//! * [`Journal::redo`] re-applies the first undone group and moves the
//!   cursor forward one.
//! * Applying a new group discards the undone groups, as in every editor:
//!   history is a line, not a tree. A group that is refused, or that has no
//!   patches, discards nothing.
//!
//! Undo and redo go through the same old-byte check as every other patch
//! (`Image::apply_patch`), so each step is exact or refused, never
//! approximate. Because the journal owns its image and is the only code that
//! changes it, the image always holds exactly the original bytes with the
//! applied groups on top, and an undo or redo is never refused in practice.
//!
//! # Stale patches
//!
//! A patch records the bytes it expects to replace, and the journal refuses
//! it unless the image holds exactly those bytes when it is applied. A
//! patch built against bytes that have changed since (with
//! [`Patch::replacing`] before another edit, from a diff of an older state,
//! or read back from a sidecar journal made against a different history) is
//! **stale**: applying it would silently overwrite whatever changed those
//! bytes. It is refused with a [`JournalError::Refused`] whose error is
//! [`ApplyError::Stale`], which names the patch, the first differing
//! offset, and the byte expected and found there; [`JournalError::is_stale`] tells it apart from a range
//! past the end of the image. The check is against the image as it is now,
//! not its original bytes, and nothing changes when it fails:
//!
//! * a stale patch anywhere in a group refuses the whole group, and the
//!   patches before it are rolled back;
//! * a refused group keeps the undone groups, so redo is still available;
//! * undo and redo go through the same check (see above). The journal owns
//!   its image and lends it out only as `&Image`, so nothing else can change
//!   the bytes under its history: a stale undo or redo is a defence, not a
//!   case that arises.
//!
//! ```
//! use rr_rom::image::{ApplyError, Image};
//! use rr_rom::journal::{Journal, JournalError, Origin, Patch};
//!
//! let mut journal = Journal::new(Image::new(vec![0x00, 0x11]));
//! // Built against the image as it is now...
//! let late = Patch::replacing(journal.image(), 0, vec![0xEE], Origin::Edit)?;
//! // ...but another edit changes its byte first.
//! journal.apply(Patch::replacing(journal.image(), 0, vec![0xAA], Origin::Edit)?)?;
//!
//! let refused = journal.apply(late);
//! assert_eq!(
//!     refused,
//!     Err(JournalError::Refused {
//!         index: 0,
//!         error: ApplyError::Stale { offset: 0, expected: 0x00, found: 0xAA },
//!     })
//! );
//! assert!(refused.is_err_and(|e| e.is_stale()));
//! assert_eq!(journal.image().bytes(), &[0xAA, 0x11]);
//! assert_eq!(journal.applied().len(), 1);
//! # Ok::<(), Box<dyn core::error::Error>>(())
//! ```
//!
//! # Modified bytes
//!
//! What the applied groups changed is read from the image, compared with
//! its original bytes ([`Image::modified_ranges`],
//! [`Image::is_range_modified`] and friends; see the
//! [`image`](crate::image) module). Undoing a group clears its markers, and
//! a byte written back to its original value is not modified:
//!
//! ```
//! use rr_rom::image::Image;
//! use rr_rom::journal::{Journal, Origin, Patch};
//!
//! let mut journal = Journal::new(Image::new(vec![0; 8]));
//! journal.apply_group([
//!     Patch::new(1, vec![0, 0], vec![1, 2], Origin::Paste)?,
//!     Patch::new(3, vec![0], vec![3], Origin::Paste)?,
//!     Patch::new(6, vec![0], vec![0], Origin::Paste)?,
//! ])?;
//! assert_eq!(journal.image().modified_ranges(), vec![1..4]);
//! assert!(journal.image().is_byte_modified(2));
//! assert!(!journal.image().is_range_modified(4..8));
//! assert_eq!(journal.image().modified_ranges_in(2..8), vec![2..4]);
//!
//! journal.undo()?;
//! assert!(journal.image().modified_ranges().is_empty());
//! # Ok::<(), Box<dyn core::error::Error>>(())
//! ```
//!
//! # Never panics
//!
//! Patches can come from untrusted sidecar journals. Every range is checked
//! when the patch is built and again (against the image) when it is
//! applied: a bad patch is an error, never a panic. The module holds no
//! `static` of any kind.
//!
//! # Sidecar journal
//!
//! The history (applied and undone groups) and any checksum overrides are
//! saved next to the ROM as a `.rrjournal` file, bound to the image it was
//! made against by SHA-256; see the [`sidecar`] module.
//!
//! # Example
//!
//! ```
//! use rr_rom::image::Image;
//! use rr_rom::journal::{Journal, Origin, Patch};
//!
//! let mut journal = Journal::new(Image::new(vec![0x00, 0x11, 0x22, 0x33]));
//!
//! // One user action that writes two places is one undo step.
//! let a = Patch::new(0, vec![0x00], vec![0xAA], Origin::Edit)?;
//! let b = Patch::new(2, vec![0x22, 0x33], vec![0xBB, 0xCC], Origin::Edit)?;
//! journal.apply_group([a, b])?;
//! assert_eq!(journal.image().bytes(), &[0xAA, 0x11, 0xBB, 0xCC]);
//!
//! journal.undo()?;
//! assert_eq!(journal.image().bytes(), &[0x00, 0x11, 0x22, 0x33]);
//! journal.redo()?;
//! assert_eq!(journal.image().bytes(), &[0xAA, 0x11, 0xBB, 0xCC]);
//! # Ok::<(), Box<dyn core::error::Error>>(())
//! ```

use core::fmt;
use core::ops::Range;

use crate::image::{ApplyError, Image};

pub mod sidecar;

/// What made a [`Patch`]: the user action or tool it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Origin {
    /// A value typed or adjusted in a table.
    Edit,
    /// Values pasted from the clipboard.
    Paste,
    /// A checksum recomputed and written back.
    ChecksumFix,
    /// Changes taken from another ROM (a diff applied to this one).
    ApplyDiff,
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Edit => "edit",
            Self::Paste => "paste",
            Self::ChecksumFix => "checksum fix",
            Self::ApplyDiff => "apply diff",
        })
    }
}

/// One exact change to an image: the bytes in a range, before and after.
///
/// Well-formed by construction: the old and new bytes are the same length
/// and the range end fits in `usize` (see [`Patch::new`]). Whether the image
/// holds the old bytes is checked when the patch is applied.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Patch {
    /// First byte of the range.
    at: usize,
    /// The bytes the range must hold before the patch.
    old: Box<[u8]>,
    /// The bytes the range holds after the patch; as long as `old`.
    new: Box<[u8]>,
    /// What made the patch.
    origin: Origin,
}

impl Patch {
    /// A patch replacing `old` with `new` at `at`.
    ///
    /// # Errors
    ///
    /// [`PatchError::LengthMismatch`] if `old` and `new` differ in length,
    /// [`PatchError::RangeOverflow`] if `at + old.len()` overflows `usize`.
    pub fn new(
        at: usize,
        old: impl Into<Box<[u8]>>,
        new: impl Into<Box<[u8]>>,
        origin: Origin,
    ) -> Result<Self, PatchError> {
        let (old, new) = (old.into(), new.into());
        if old.len() != new.len() {
            return Err(PatchError::LengthMismatch {
                old: old.len(),
                new: new.len(),
            });
        }
        if at.checked_add(old.len()).is_none() {
            return Err(PatchError::RangeOverflow { at, len: old.len() });
        }
        Ok(Self {
            at,
            old,
            new,
            origin,
        })
    }

    /// A patch writing `new` at `at` in `image`, with the old bytes read
    /// from the image as it is now.
    ///
    /// # Errors
    ///
    /// [`PatchError::RangeOverflow`] if `at + new.len()` overflows `usize`,
    /// [`PatchError::OutOfBounds`] if the range does not lie within the
    /// image.
    pub fn replacing(
        image: &Image,
        at: usize,
        new: impl Into<Box<[u8]>>,
        origin: Origin,
    ) -> Result<Self, PatchError> {
        let new = new.into();
        let len = new.len();
        let end = at
            .checked_add(len)
            .ok_or(PatchError::RangeOverflow { at, len })?;
        let old = image.bytes().get(at..end).ok_or(PatchError::OutOfBounds {
            at,
            len,
            image_len: image.len(),
        })?;
        Ok(Self {
            at,
            old: old.into(),
            new,
            origin,
        })
    }

    /// The image range the patch replaces.
    #[must_use]
    pub const fn range(&self) -> Range<usize> {
        // Cannot overflow: checked when the patch was built.
        self.at..self.at + self.old.len()
    }

    /// The bytes the range holds before the patch.
    #[must_use]
    pub fn old_bytes(&self) -> &[u8] {
        &self.old
    }

    /// The bytes the range holds after the patch.
    #[must_use]
    pub fn new_bytes(&self) -> &[u8] {
        &self.new
    }

    /// What made the patch.
    #[must_use]
    pub const fn origin(&self) -> Origin {
        self.origin
    }
}

/// Why a [`Patch`] could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchError {
    /// The old and new byte strings differ in length; a patch never resizes
    /// the image.
    LengthMismatch {
        /// Length of the old bytes.
        old: usize,
        /// Length of the new bytes.
        new: usize,
    },
    /// The range end `at + len` overflows `usize`.
    RangeOverflow {
        /// First byte of the range.
        at: usize,
        /// Length of the range.
        len: usize,
    },
    /// The range `at..at + len` does not lie within the image the old bytes
    /// were to be read from ([`Patch::replacing`]).
    OutOfBounds {
        /// First byte of the range.
        at: usize,
        /// Length of the range.
        len: usize,
        /// Length of the image.
        image_len: usize,
    },
}

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::LengthMismatch { old, new } => write!(
                f,
                "patch replaces {old} bytes with {new}; a patch may not resize the image"
            ),
            Self::RangeOverflow { at, len } => write!(
                f,
                "patch of {len} bytes at {at:#x} ends past the largest address"
            ),
            Self::OutOfBounds { at, len, image_len } => write!(
                f,
                "patch of {len} bytes at {at:#x} reaches past the end of the {image_len}-byte image"
            ),
        }
    }
}

impl core::error::Error for PatchError {}

/// The patches of one user action: undone and redone as one step.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Group {
    /// Applied in order; undone in reverse. Never empty.
    patches: Box<[Patch]>,
}

impl Group {
    /// The group's patches, in the order they were applied.
    #[must_use]
    pub fn patches(&self) -> &[Patch] {
        &self.patches
    }
}

/// An image and the history of every change made to it.
///
/// See the [module documentation](self).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Journal {
    /// The image. Changed only through `groups`.
    image: Image,
    /// Every group still in history: `..cursor` applied, `cursor..` undone.
    groups: Vec<Group>,
    /// Number of applied groups, `<= groups.len()`.
    cursor: usize,
}

impl Journal {
    /// A journal with no history over `image`.
    #[must_use]
    pub const fn new(image: Image) -> Self {
        Self {
            image,
            groups: Vec::new(),
            cursor: 0,
        }
    }

    /// The image, with the applied groups on top of its original bytes.
    #[must_use]
    pub const fn image(&self) -> &Image {
        &self.image
    }

    /// The image, ending the history.
    #[must_use]
    pub fn into_image(self) -> Image {
        self.image
    }

    /// The applied groups, oldest first. The last one is what
    /// [`undo`](Self::undo) reverts.
    #[must_use]
    pub fn applied(&self) -> &[Group] {
        self.groups.get(..self.cursor).unwrap_or_default()
    }

    /// The undone groups, in the order [`redo`](Self::redo) re-applies them.
    #[must_use]
    pub fn undone(&self) -> &[Group] {
        self.groups.get(self.cursor..).unwrap_or_default()
    }

    /// Whether there is a group to undo.
    #[must_use]
    pub const fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    /// Whether there is a group to redo.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.cursor < self.groups.len()
    }

    /// Applies one patch as its own undo step. Same as
    /// [`apply_group`](Self::apply_group) with one patch.
    ///
    /// # Errors
    ///
    /// [`JournalError::Refused`] if the image refuses the patch; nothing
    /// changes.
    pub fn apply(&mut self, patch: Patch) -> Result<(), JournalError> {
        self.apply_group([patch])
    }

    /// Applies `patches` in order as one undo step and discards the undone
    /// groups. Each patch is checked against the image as the patches before
    /// it left it, so a later patch may rewrite bytes an earlier one wrote.
    ///
    /// Atomic: on error the patches already applied are reverted and the
    /// journal (image and history) is unchanged. No patches is a no-op that
    /// records nothing and keeps the undone groups.
    ///
    /// # Errors
    ///
    /// [`JournalError::Refused`] for the first patch the image refuses
    /// (stale old bytes or a range past the end of the image).
    pub fn apply_group(
        &mut self,
        patches: impl IntoIterator<Item = Patch>,
    ) -> Result<(), JournalError> {
        let patches: Box<[Patch]> = patches.into_iter().collect();
        if patches.is_empty() {
            return Ok(());
        }
        apply_steps(&mut self.image, patches.iter().map(Step::forward))?;
        self.groups.truncate(self.cursor);
        self.groups.push(Group { patches });
        self.cursor = self.groups.len();
        Ok(())
    }

    /// Reverts the last applied group, its patches in reverse order, and
    /// returns it; `None` if there is nothing to undo.
    ///
    /// # Errors
    ///
    /// [`JournalError::Refused`] if the image refuses a reverted patch,
    /// which the journal's own bookkeeping rules out; nothing changes.
    pub fn undo(&mut self) -> Result<Option<&Group>, JournalError> {
        let Some(index) = self.cursor.checked_sub(1) else {
            return Ok(None);
        };
        let Some(group) = self.groups.get(index) else {
            return Ok(None);
        };
        apply_steps(
            &mut self.image,
            group.patches.iter().rev().map(Step::backward),
        )?;
        self.cursor = index;
        Ok(Some(group))
    }

    /// Re-applies the first undone group and returns it; `None` if there is
    /// nothing to redo.
    ///
    /// # Errors
    ///
    /// [`JournalError::Refused`] if the image refuses a patch, which the
    /// journal's own bookkeeping rules out; nothing changes.
    pub fn redo(&mut self) -> Result<Option<&Group>, JournalError> {
        let Some(group) = self.groups.get(self.cursor) else {
            return Ok(None);
        };
        apply_steps(&mut self.image, group.patches.iter().map(Step::forward))?;
        self.cursor += 1;
        Ok(Some(group))
    }

    /// Undoes every applied group, leaving the image's original bytes.
    ///
    /// # Errors
    ///
    /// As [`undo`](Self::undo); the groups undone before the error stay
    /// undone.
    pub fn undo_all(&mut self) -> Result<(), JournalError> {
        while self.undo()?.is_some() {}
        Ok(())
    }

    /// Redoes every undone group.
    ///
    /// # Errors
    ///
    /// As [`redo`](Self::redo); the groups redone before the error stay
    /// redone.
    pub fn redo_all(&mut self) -> Result<(), JournalError> {
        while self.redo()?.is_some() {}
        Ok(())
    }
}

impl From<Image> for Journal {
    fn from(image: Image) -> Self {
        Self::new(image)
    }
}

/// Why the journal refused a group, an undo or a redo. Nothing changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalError {
    /// The image refused a patch.
    Refused {
        /// Position of the patch in the order it was applied: within the
        /// group for an apply or redo, counted from the group's last patch
        /// for an undo.
        index: usize,
        /// Why the image refused it.
        error: ApplyError,
    },
}

impl JournalError {
    /// Whether a patch was refused as stale: the image did not hold the
    /// bytes it expected to replace (see [Stale patches](self#stale-patches)).
    #[must_use]
    pub const fn is_stale(&self) -> bool {
        matches!(
            self,
            Self::Refused {
                error: ApplyError::Stale { .. },
                ..
            }
        )
    }
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused { index, error } => write!(f, "patch {index} refused: {error}"),
        }
    }
}

impl core::error::Error for JournalError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Refused { error, .. } => Some(error),
        }
    }
}

/// One patch application in a direction: replace `from` with `to` at `at`.
#[derive(Clone, Copy)]
struct Step<'a> {
    at: usize,
    from: &'a [u8],
    to: &'a [u8],
}

impl<'a> Step<'a> {
    /// Applying `patch`.
    fn forward(patch: &'a Patch) -> Self {
        Self {
            at: patch.at,
            from: &patch.old,
            to: &patch.new,
        }
    }

    /// Reverting `patch`.
    fn backward(patch: &'a Patch) -> Self {
        Self {
            at: patch.at,
            from: &patch.new,
            to: &patch.old,
        }
    }
}

/// Applies `steps` to `image` in order, all or nothing: on the first
/// refusal, reverts the steps already applied (newest first) and returns
/// the refused step's position.
fn apply_steps<'a>(
    image: &mut Image,
    steps: impl Iterator<Item = Step<'a>>,
) -> Result<(), JournalError> {
    let mut done: Vec<Step<'a>> = Vec::new();
    for (index, step) in steps.enumerate() {
        if let Err(error) = image.apply_patch(step.at, step.from, step.to) {
            for undo in done.iter().rev() {
                // Cannot be refused: it reverses a step just applied to the
                // image this function holds exclusively.
                let reverted = image.apply_patch(undo.at, undo.to, undo.from);
                debug_assert_eq!(reverted, Ok(()), "rollback of a step just applied");
            }
            return Err(JournalError::Refused { index, error });
        }
        done.push(step);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
