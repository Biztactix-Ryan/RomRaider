//! An opened ROM (US-RR-34-8): an image paired with the [`Definition`] it
//! is shown and edited through, and whether editing is allowed at all.
//!
//! Opening is [`OpenRom::open`]: the caller has already found the image's
//! definition (the ROM-ID index, `rr_defs::resolve::RomIdIndex`, which
//! refuses an image more than one definition matches) and resolved it, and
//! hands over the definition with the image (or a journal restored from a
//! sidecar over it). Opening checks that the two belong together; today
//! that is the image size.
//!
//! # Size check
//!
//! A definition's `<filesize>` ([`RomId::file_size`], already in bytes:
//! `512kb` is 524 288, parsed once when the definition was read) is the
//! size of image its table addresses are laid out for. An image of any
//! other size is probably not that ROM, or not a whole one (a partial read,
//! a dump with a header or trailing bytes), and an edit made through the
//! definition could change the wrong bytes. So a size mismatch, longer or
//! shorter, is a [`ROM_SIZE_MISMATCH`](codes::ROM_SIZE_MISMATCH) Error
//! naming the definition and both sizes, and the ROM opens with editing
//! disabled ([`EditingDisabled::SizeMismatch`]), for viewing only (PROJECT.md
//! EC-02).
//!
//! The image size compared is its original size: a patch never resizes an
//! image, so it is the size now too.
//!
//! A definition with no `<filesize>` (in its own `<romid>` or any base's)
//! declares no size and is not checked: there is nothing to compare with,
//! and legacy, which read no size either, opened it.
//!
//! # Legacy
//!
//! Legacy RomRaider never compared the sizes. Opening a ROM set its RAM
//! offset to the definition's file size minus the image length
//! (`xml/DOMRomUnmarshaller.java:72-75`), and every table address was
//! shifted by it (`maps/DataCell.java:173-185`, `289-307`), so an image of
//! the wrong size opened silently, every table read and written at an
//! address moved by the difference; a table then pushed past the end of
//! the image was only reported when it failed to load
//! (`maps/Rom.java:254-262`).
//!
//! # Editing disabled
//!
//! An [`OpenRom`] owns its journal and lends it only as `&Journal`, so the
//! one way to change its bytes is [`OpenRom::edit`], which returns an
//! [`Editor`] only while editing is allowed and the [`EditingDisabled`]
//! reason otherwise. Every change, undo and redo included, goes through the
//! editor; a read-only ROM has no route to a `&mut Journal`, and the
//! editor gives none out either, so its journal cannot be swapped for
//! another over a different image. The state is fixed when the ROM is
//! opened: nothing re-enables editing.
//!
//! The [`ROM_SIZE_MISMATCH`](codes::ROM_SIZE_MISMATCH) Error also withholds
//! the [`OpenRom`] from [`Loaded::value`] and [`Loaded::into_result`]: a
//! caller that opens the ROM anyway, to show it, says so with
//! [`Loaded::accept_errors`], and gets it read-only.
//!
//! # Example
//!
//! ```
//! use rr_defs::resolve::DefinitionResolver;
//! use rr_defs::romraider::DefinitionSet;
//! use rr_rom::image::Image;
//! use rr_rom::journal::{Origin, Patch};
//! use rr_rom::open::{EditingDisabled, OpenRom};
//!
//! let xml = br#"<roms><rom>
//!   <romid><xmlid>ROM A</xmlid><internalidaddress>0</internalidaddress>
//!     <internalidstring>ROMA</internalidstring><filesize>1kb</filesize></romid>
//! </rom></roms>"#;
//! let (set, _) = DefinitionSet::read_all([("defs.xml", &xml[..])]).into_result().unwrap();
//! let mut resolver = DefinitionResolver::new(&set);
//! let definition = resolver.resolve(set.find("rom a").unwrap()).into_result().unwrap().0.unwrap();
//!
//! // The declared size: editable.
//! let (mut rom, _) = OpenRom::open(definition.clone(), Image::new(vec![0; 1024]), "a.bin")
//!     .into_result()
//!     .unwrap();
//! let patch = Patch::replacing(rom.image(), 8, vec![0xFF], Origin::Edit)?;
//! rom.edit()?.apply(patch)?;
//! assert_eq!(rom.image().bytes()[8], 0xFF);
//!
//! // One byte short: an Error, and opened anyway only read-only.
//! let loaded = OpenRom::open(definition, Image::new(vec![0; 1023]), "short.bin");
//! assert!(loaded.has_errors());
//! let (mut rom, diagnostics) = loaded.accept_errors();
//! assert!(diagnostics[0].message().contains("1023 bytes"));
//! assert_eq!(
//!     rom.edit().err(),
//!     Some(EditingDisabled::SizeMismatch { declared: 1024, actual: 1023 })
//! );
//! # Ok::<(), Box<dyn core::error::Error>>(())
//! ```

#[cfg(test)]
mod tests;

use core::fmt;
use core::ops::Deref;

use rr_core::diag::{Diagnostic, Loaded, Provenance, Span, codes};
use rr_defs::resolve::{Definition, RomId};

use crate::image::Image;
use crate::journal::{Group, Journal, JournalError, Patch};

/// A ROM opened through its [`Definition`]: the definition, the image's
/// journal, and whether it may be edited. See the [module docs](self).
#[derive(Debug, Clone)]
pub struct OpenRom {
    definition: Definition,
    journal: Journal,
    source: String,
    /// `Some` when editing is disabled, and why. Set once, by [`OpenRom::open`].
    disabled: Option<EditingDisabled>,
}

impl OpenRom {
    /// Opens `journal` (an [`Image`], or a journal restored over one)
    /// through `definition`; `source` names the image in diagnostics.
    ///
    /// Checks the image size against the definition's declared size: a
    /// mismatch is a [`ROM_SIZE_MISMATCH`](codes::ROM_SIZE_MISMATCH) Error
    /// and an [`OpenRom`] with editing disabled, withheld unless the caller
    /// accepts the error. With no declared size, or a match, the ROM opens
    /// editable with no diagnostic.
    pub fn open(
        definition: Definition,
        journal: impl Into<Journal>,
        source: impl Into<String>,
    ) -> Loaded<Self> {
        let journal = journal.into();
        let source = source.into();
        let mismatch = size_mismatch(definition.romid(), journal.image());
        let diagnostics = mismatch
            .iter()
            .map(|m| m.diagnostic(definition.xmlid(), &source))
            .collect::<Vec<_>>();
        let rom = Self {
            definition,
            journal,
            source,
            disabled: mismatch,
        };
        Loaded::new(rom, diagnostics)
    }

    /// The definition the ROM is shown and edited through.
    #[must_use]
    pub const fn definition(&self) -> &Definition {
        &self.definition
    }

    /// The image's history: what was changed, undone and redone.
    #[must_use]
    pub const fn journal(&self) -> &Journal {
        &self.journal
    }

    /// The image, with every applied change.
    #[must_use]
    pub const fn image(&self) -> &Image {
        self.journal.image()
    }

    /// The name of the image, as diagnostics give it.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Whether the ROM may be edited ([`edit`](Self::edit) succeeds).
    #[must_use]
    pub const fn is_editable(&self) -> bool {
        self.disabled.is_none()
    }

    /// Why editing is disabled; `None` when it is not.
    #[must_use]
    pub const fn editing_disabled(&self) -> Option<EditingDisabled> {
        self.disabled
    }

    /// The editor: the one way to change the ROM's bytes (apply, undo,
    /// redo).
    ///
    /// # Errors
    ///
    /// [`EditingDisabled`] when the ROM was opened read-only (an image
    /// whose size differs from the definition's); nothing can change it.
    pub fn edit(&mut self) -> Result<Editor<'_>, EditingDisabled> {
        match self.disabled {
            Some(why) => Err(why),
            None => Ok(Editor {
                journal: &mut self.journal,
            }),
        }
    }
}

/// Why an [`OpenRom`] cannot be edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EditingDisabled {
    /// The image is not the size the definition declares
    /// ([`ROM_SIZE_MISMATCH`](codes::ROM_SIZE_MISMATCH)).
    SizeMismatch {
        /// The definition's `<filesize>`, in bytes.
        declared: u32,
        /// The image's length, in bytes.
        actual: usize,
    },
}

impl EditingDisabled {
    /// The [`ROM_SIZE_MISMATCH`](codes::ROM_SIZE_MISMATCH) Error for this
    /// mismatch of the image `source` with the definition `xmlid`, at the
    /// first byte the two sizes disagree about (the end of the shorter).
    fn diagnostic(self, xmlid: &str, source: &str) -> Diagnostic {
        let Self::SizeMismatch { declared, actual } = self;
        let declared_len = u64::from(declared);
        let actual_len = u64::try_from(actual).unwrap_or(u64::MAX);
        let (diff, how) = if actual_len < declared_len {
            (declared_len - actual_len, "short")
        } else {
            (actual_len - declared_len, "too long")
        };
        let message = format!(
            "the image is {} but definition `{xmlid}` declares a file size of {} \
             ({diff} bytes {how}), so it may not be this ROM or not all of it: \
             opened read-only, editing is disabled",
            Size(actual_len),
            Size(declared_len),
        );
        Diagnostic::error(
            codes::ROM_SIZE_MISMATCH,
            message,
            Provenance::new(source, Span::byte_offset(actual_len.min(declared_len))),
        )
    }
}

impl fmt::Display for EditingDisabled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::SizeMismatch { declared, actual } => write!(
                f,
                "editing is disabled: the image is {actual} bytes, the definition declares {declared}"
            ),
        }
    }
}

impl core::error::Error for EditingDisabled {}

/// A byte count for a message: `524288 bytes (512 KiB)`, the KiB form only
/// when exact, as definitions write `<filesize>` (`512kb`).
struct Size(u64);

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} bytes", self.0)?;
        if self.0 >= 1024 && self.0.is_multiple_of(1024) {
            write!(f, " ({} KiB)", self.0 / 1024)?;
        }
        Ok(())
    }
}

/// The mismatch of `image` with the size `romid` declares; `None` when it
/// declares none or the sizes agree.
fn size_mismatch(romid: &RomId, image: &Image) -> Option<EditingDisabled> {
    let declared = romid.file_size()?;
    let actual = image.original().len();
    let agrees = usize::try_from(declared).is_ok_and(|d| d == actual);
    (!agrees).then_some(EditingDisabled::SizeMismatch { declared, actual })
}

/// Changes an editable [`OpenRom`]'s bytes through its journal: what
/// [`OpenRom::edit`] lends. Reads go to the [`Journal`] it derefs to; every
/// change is one of the journal's own, under the same checks.
#[derive(Debug)]
pub struct Editor<'r> {
    journal: &'r mut Journal,
}

impl Editor<'_> {
    /// [`Journal::apply`].
    ///
    /// # Errors
    ///
    /// As [`Journal::apply`]; nothing changes.
    pub fn apply(&mut self, patch: Patch) -> Result<(), JournalError> {
        self.journal.apply(patch)
    }

    /// [`Journal::apply_group`]: `patches` as one undo step.
    ///
    /// # Errors
    ///
    /// As [`Journal::apply_group`]; nothing changes.
    pub fn apply_group(
        &mut self,
        patches: impl IntoIterator<Item = Patch>,
    ) -> Result<(), JournalError> {
        self.journal.apply_group(patches)
    }

    /// [`Journal::undo`].
    ///
    /// # Errors
    ///
    /// As [`Journal::undo`]; nothing changes.
    pub fn undo(&mut self) -> Result<Option<&Group>, JournalError> {
        self.journal.undo()
    }

    /// [`Journal::redo`].
    ///
    /// # Errors
    ///
    /// As [`Journal::redo`]; nothing changes.
    pub fn redo(&mut self) -> Result<Option<&Group>, JournalError> {
        self.journal.redo()
    }

    /// [`Journal::undo_all`].
    ///
    /// # Errors
    ///
    /// As [`Journal::undo_all`].
    pub fn undo_all(&mut self) -> Result<(), JournalError> {
        self.journal.undo_all()
    }

    /// [`Journal::redo_all`].
    ///
    /// # Errors
    ///
    /// As [`Journal::redo_all`].
    pub fn redo_all(&mut self) -> Result<(), JournalError> {
        self.journal.redo_all()
    }
}

impl Deref for Editor<'_> {
    type Target = Journal;

    fn deref(&self) -> &Journal {
        self.journal
    }
}
