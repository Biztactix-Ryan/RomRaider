//! Sidecar journal (`<rom>.rrjournal`): a [`Journal`]'s history and the
//! checksum overrides of its saves, as text kept next to the ROM.
//!
//! Legacy saved nothing but the ROM: closing the editor lost every original
//! value, and nothing recorded that a ROM had been saved with a failing
//! checksum. Here the save pipeline writes the journal beside the ROM
//! (INFRASTRUCTURE.md "ROM saves") so a later session can still undo, list
//! and redo every change, and so that "saved with a failing checksum" is on
//! record (SECURITY.md, `RomWrite`).
//!
//! This module is the format only: [`encode`] turns a journal and its
//! overrides into text, [`decode`] turns text back into them. Reading and
//! writing the file belong to the save pipeline, which holds the
//! `RomWrite` capability; nothing here touches the file system.
//!
//! # What is recorded
//!
//! * **The base image**, as the SHA-256 of the original bytes
//!   ([`Image::original_hash`]). The journal is only ever replayed onto that
//!   image: [`decode`] refuses any other ([`SidecarError::BaseMismatch`])
//!   before applying anything.
//! * **The head image**, the SHA-256 of the bytes with the applied groups on
//!   top ([`Image::current_hash`]). After replaying, [`decode`] checks it, so
//!   a journal that replays cleanly but to different bytes is still refused
//!   ([`SidecarError::HeadMismatch`]).
//! * **Every group**: the applied ones ([`Journal::applied`]) and then the
//!   undone ones ([`Journal::undone`]), so undo and redo carry on where they
//!   stopped. Each patch keeps its offset, old bytes, new bytes and
//!   [`Origin`].
//! * **Checksum overrides** ([`ChecksumOverride`]): one per save the user
//!   confirmed despite failing checksums, naming the bytes saved, when, and
//!   each failing checksum with its stored and computed values. The
//!   checksum engine and the save pipeline fill these in; this module only
//!   validates and carries them.
//!
//! # Format
//!
//! TOML, like every other file format of our own (ARCHITECTURE.md "Profile
//! files", D-014): a reviewer can read a sidecar in any editor and diff two
//! of them, and the parser is the one the workspace already uses. Offsets
//! are written as hex integers and bytes as lower-case hex strings, the way
//! ROM addresses and bytes are read everywhere else:
//!
//! ```toml
//! # RomRaider sidecar journal: the edit history of the ROM beside it.
//! format = "rrjournal"
//! schema = 1
//! base_sha256 = "<64 hex digits>"
//! head_sha256 = "<64 hex digits>"
//!
//! [[applied]]
//! [[applied.patch]]
//! at = 0x0001f2a0
//! origin = "edit"
//! old = "0102"
//! new = "0a0b"
//!
//! [[undone]]
//! [[undone.patch]]
//! at = 0x00000010
//! origin = "paste"
//! old = "ff"
//! new = "00"
//!
//! [[checksum_override]]
//! image_sha256 = "<64 hex digits>"
//! confirmed_at_unix = 1792000000
//! [[checksum_override.failure]]
//! checksum = "subaru-sum32"
//! stored = "12345678"
//! computed = "9abcdef0"
//! ```
//!
//! `origin` is one of `edit`, `paste`, `checksum_fix` and `apply_diff`.
//! Hex digits are read in either case and written in lower case.
//!
//! # Versioning
//!
//! `format = "rrjournal"` says what the file is and `schema = 1` which
//! version of it, both checked before anything else. The rules:
//!
//! * a reader refuses a schema it does not know, newer or older
//!   ([`SidecarError::UnsupportedSchema`]), rather than guess;
//! * a reader refuses any key it does not know ([`SidecarError::Shape`]): a
//!   new key may change what the file means, so it comes with a new schema
//!   number, never silently;
//! * when the schema changes, the reader of the new one keeps reading the
//!   old ones through a migration function with its own tests
//!   (ARCHITECTURE.md "Profile files").
//!
//! # Untrusted input
//!
//! A sidecar is a file anyone could have edited or sent along with a ROM
//! (SECURITY.md "Untrusted input"), so [`decode`] trusts nothing in it:
//!
//! * the text is at most [`MAX_SIDECAR_LEN`] bytes, checked before parsing;
//!   the TOML parser's nesting depth is bounded (`toml` without its
//!   `unbounded` feature);
//! * every record is checked: hex strings and hashes well-formed, old and
//!   new bytes the same length, no empty group, every override valid
//!   ([`ChecksumOverride::new`]); a bad record names itself
//!   ([`SidecarError::Invalid`]);
//! * every patch is replayed through the journal, so each offset must lie
//!   in the image and each patch's old bytes must be exactly what the image
//!   holds at that point in the history ([`SidecarError::Replay`]);
//! * any error returns nothing: there is no partly replayed journal.
//!
//! No input makes it panic. The module holds no `static` of any kind.
//!
//! # Example
//!
//! ```
//! use rr_rom::image::Image;
//! use rr_rom::journal::sidecar::{self, SidecarError};
//! use rr_rom::journal::{Journal, Origin, Patch};
//!
//! let rom = vec![0x00, 0x11, 0x22, 0x33];
//! let mut journal = Journal::new(Image::new(rom.clone()));
//! journal.apply(Patch::replacing(journal.image(), 1, vec![0xAA], Origin::Edit)?)?;
//! journal.apply(Patch::replacing(journal.image(), 2, vec![0xBB], Origin::Edit)?)?;
//! journal.undo()?;
//!
//! let text = sidecar::encode(&journal, &[])?;
//! assert!(text.contains("schema = 1"));
//!
//! // Read back onto the same ROM: the history and the undo position return.
//! let restored = sidecar::decode(&text, Image::new(rom))?;
//! assert_eq!(restored.journal, journal);
//! assert!(restored.overrides.is_empty());
//!
//! // Onto any other image it is refused and nothing is applied.
//! let refused = sidecar::decode(&text, Image::new(vec![0xFF; 4]));
//! assert!(matches!(refused, Err(SidecarError::BaseMismatch { .. })));
//! # Ok::<(), Box<dyn core::error::Error>>(())
//! ```

use core::fmt;

use serde::Deserialize;

use super::{Group, Journal, JournalError, Origin, Patch, PatchError};
use crate::image::{Image, ImageHash};

/// The value of the `format` key.
pub const FORMAT: &str = "rrjournal";

/// The schema version [`encode`] writes and [`decode`] reads.
pub const SCHEMA: u32 = 1;

/// The longest sidecar text [`decode`] reads and [`encode`] produces, in
/// bytes (32 MiB): room for a whole-image diff of a 4 MiB ROM, old and new
/// bytes in hex, with history to spare.
pub const MAX_SIDECAR_LEN: usize = 32 << 20;

/// The longest checksum name a [`ChecksumFailure`] carries, in bytes.
pub const MAX_CHECKSUM_NAME_LEN: usize = 64;

/// The longest stored or computed checksum value a [`ChecksumFailure`]
/// carries, in bytes.
pub const MAX_CHECKSUM_VALUE_LEN: usize = 32;

/// The latest confirmation time a [`ChecksumOverride`] carries, in seconds
/// since the Unix epoch: the largest TOML integer.
pub const MAX_CONFIRMED_AT_UNIX: u64 = i64::MAX.unsigned_abs();

/// One save the user confirmed although checksums failed (SECURITY.md,
/// `RomWrite`): what was saved, when, and which checksums failed.
///
/// Valid by construction (see [`ChecksumOverride::new`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChecksumOverride {
    /// SHA-256 of the bytes saved.
    image: ImageHash,
    /// When the user confirmed, in seconds since the Unix epoch.
    confirmed_at_unix: u64,
    /// The checksums that failed; never empty.
    failures: Box<[ChecksumFailure]>,
}

impl ChecksumOverride {
    /// An override of the save of the image whose SHA-256 is `image`,
    /// confirmed at `confirmed_at_unix` (seconds since the Unix epoch, from
    /// the injected clock's `wall_now`, D-010), despite `failures`.
    ///
    /// # Errors
    ///
    /// [`OverrideError::NoFailures`] if `failures` is empty (there is
    /// nothing to override), [`OverrideError::TimeOutOfRange`] if
    /// `confirmed_at_unix` is past [`MAX_CONFIRMED_AT_UNIX`].
    pub fn new(
        image: ImageHash,
        confirmed_at_unix: u64,
        failures: impl IntoIterator<Item = ChecksumFailure>,
    ) -> Result<Self, OverrideError> {
        let failures: Box<[ChecksumFailure]> = failures.into_iter().collect();
        if failures.is_empty() {
            return Err(OverrideError::NoFailures);
        }
        if confirmed_at_unix > MAX_CONFIRMED_AT_UNIX {
            return Err(OverrideError::TimeOutOfRange {
                secs: confirmed_at_unix,
            });
        }
        Ok(Self {
            image,
            confirmed_at_unix,
            failures,
        })
    }

    /// SHA-256 of the bytes that were saved.
    #[must_use]
    pub const fn image(&self) -> ImageHash {
        self.image
    }

    /// When the user confirmed the save, in seconds since the Unix epoch.
    #[must_use]
    pub const fn confirmed_at_unix(&self) -> u64 {
        self.confirmed_at_unix
    }

    /// The checksums that failed; never empty.
    #[must_use]
    pub fn failures(&self) -> &[ChecksumFailure] {
        &self.failures
    }
}

/// One checksum that did not verify when a [`ChecksumOverride`] was
/// confirmed.
///
/// Valid by construction (see [`ChecksumFailure::new`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChecksumFailure {
    /// The checksum's name, as the checksum engine identifies it.
    checksum: String,
    /// The checksum value stored in the image.
    stored: Box<[u8]>,
    /// The value the checksum algorithm computes over the image.
    computed: Box<[u8]>,
}

impl ChecksumFailure {
    /// The checksum named `checksum` holds `stored` in the image, but its
    /// algorithm computes `computed`. The values are raw bytes as stored in
    /// the image.
    ///
    /// # Errors
    ///
    /// [`OverrideError::ChecksumName`] unless `checksum` is 1 to
    /// [`MAX_CHECKSUM_NAME_LEN`] bytes of printable ASCII (space allowed)
    /// other than `"` and `\`; [`OverrideError::ValueLength`] unless
    /// `stored` and `computed` are the same length, 1 to
    /// [`MAX_CHECKSUM_VALUE_LEN`] bytes.
    pub fn new(
        checksum: impl Into<String>,
        stored: impl Into<Box<[u8]>>,
        computed: impl Into<Box<[u8]>>,
    ) -> Result<Self, OverrideError> {
        let (checksum, stored, computed) = (checksum.into(), stored.into(), computed.into());
        if !is_checksum_name(&checksum) {
            return Err(OverrideError::ChecksumName { name: checksum });
        }
        if stored.len() != computed.len() || !(1..=MAX_CHECKSUM_VALUE_LEN).contains(&stored.len()) {
            return Err(OverrideError::ValueLength {
                stored: stored.len(),
                computed: computed.len(),
            });
        }
        Ok(Self {
            checksum,
            stored,
            computed,
        })
    }

    /// The checksum's name.
    #[must_use]
    pub fn checksum(&self) -> &str {
        &self.checksum
    }

    /// The value stored in the image.
    #[must_use]
    pub fn stored(&self) -> &[u8] {
        &self.stored
    }

    /// The value the algorithm computes.
    #[must_use]
    pub fn computed(&self) -> &[u8] {
        &self.computed
    }
}

/// Whether `name` is a valid checksum name: 1 to [`MAX_CHECKSUM_NAME_LEN`]
/// bytes of printable ASCII or space, with no `"` or `\`, so it is written
/// into a TOML string as is.
fn is_checksum_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_CHECKSUM_NAME_LEN
        && name
            .bytes()
            .all(|b| b == b' ' || (b.is_ascii_graphic() && b != b'"' && b != b'\\'))
}

/// Why a [`ChecksumOverride`] or [`ChecksumFailure`] could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverrideError {
    /// An override with no failing checksum.
    NoFailures,
    /// A confirmation time past [`MAX_CONFIRMED_AT_UNIX`].
    TimeOutOfRange {
        /// The time given, in seconds since the Unix epoch.
        secs: u64,
    },
    /// A checksum name that is empty, longer than
    /// [`MAX_CHECKSUM_NAME_LEN`], or not printable ASCII without `"` and `\`.
    ChecksumName {
        /// The name given.
        name: String,
    },
    /// Stored and computed values that differ in length, are empty, or are
    /// longer than [`MAX_CHECKSUM_VALUE_LEN`].
    ValueLength {
        /// Length of the stored value.
        stored: usize,
        /// Length of the computed value.
        computed: usize,
    },
}

impl fmt::Display for OverrideError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFailures => f.write_str("a checksum override names no failing checksum"),
            Self::TimeOutOfRange { secs } => write!(
                f,
                "confirmation time {secs} is past the latest supported ({MAX_CONFIRMED_AT_UNIX})"
            ),
            Self::ChecksumName { name } => write!(
                f,
                "checksum name {name:?} is not 1 to {MAX_CHECKSUM_NAME_LEN} printable ASCII \
                 characters without '\"' or '\\'"
            ),
            Self::ValueLength { stored, computed } => write!(
                f,
                "checksum values of {stored} (stored) and {computed} (computed) bytes; both must \
                 be the same length, 1 to {MAX_CHECKSUM_VALUE_LEN} bytes"
            ),
        }
    }
}

impl core::error::Error for OverrideError {}

/// A journal and its checksum overrides, read back by [`decode`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restored {
    /// The journal over the base image, with every applied group applied
    /// and every undone group ready to redo.
    pub journal: Journal,
    /// The checksum overrides, in the order they were recorded.
    pub overrides: Vec<ChecksumOverride>,
}

/// The sidecar text of `journal` and `overrides`.
///
/// The output is deterministic, and [`decode`] reads it back onto the
/// journal's base image as an equal journal and overrides.
///
/// # Errors
///
/// [`EncodeError::TooLarge`] if the text would be longer than
/// [`MAX_SIDECAR_LEN`], so that [`decode`] would refuse it.
pub fn encode(journal: &Journal, overrides: &[ChecksumOverride]) -> Result<String, EncodeError> {
    encode_within(journal, overrides, MAX_SIDECAR_LEN)
}

/// [`encode`] with the length limit `max`.
fn encode_within(
    journal: &Journal,
    overrides: &[ChecksumOverride],
    max: usize,
) -> Result<String, EncodeError> {
    let text = Document { journal, overrides }.to_string();
    if text.len() > max {
        return Err(EncodeError::TooLarge {
            len: text.len(),
            max,
        });
    }
    Ok(text)
}

/// Why [`encode`] produced no text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    /// The text would be longer than [`MAX_SIDECAR_LEN`].
    TooLarge {
        /// Its length in bytes.
        len: usize,
        /// The limit.
        max: usize,
    },
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::TooLarge { len, max } => write!(
                f,
                "sidecar journal would be {len} bytes, over the {max}-byte limit it could be read \
                 back within"
            ),
        }
    }
}

impl core::error::Error for EncodeError {}

/// Reads sidecar `text` back onto `base`, the image as loaded: a journal over
/// `base` with the recorded history, and the recorded checksum overrides.
///
/// All or nothing: on any error, nothing is returned and `base` is dropped.
///
/// # Errors
///
/// [`SidecarError::BaseMismatch`] if `base` is not the image the journal
/// was made against, [`SidecarError::ImageModified`] if `base` carries
/// changes of its own; anything else means the text is not a valid sidecar
/// (see [`SidecarError`]).
pub fn decode(text: &str, base: Image) -> Result<Restored, SidecarError> {
    decode_within(text, base, MAX_SIDECAR_LEN)
}

/// [`decode`] with the length limit `max`.
fn decode_within(text: &str, base: Image, max: usize) -> Result<Restored, SidecarError> {
    if text.len() > max {
        return Err(SidecarError::TooLarge {
            len: text.len(),
            max,
        });
    }
    let doc = parse(text)?;
    let expected = hash(&doc.base_sha256, "base_sha256")?;
    if base.original_hash() != expected {
        return Err(SidecarError::BaseMismatch {
            expected,
            found: base.original_hash(),
        });
    }
    if base.is_modified() {
        return Err(SidecarError::ImageModified);
    }
    let head = hash(&doc.head_sha256, "head_sha256")?;
    let applied = groups(doc.applied, "applied")?;
    let undone = groups(doc.undone, "undone")?;
    let overrides = doc
        .overrides
        .into_iter()
        .enumerate()
        .map(|(i, o)| checksum_override(o, &format!("checksum_override[{i}]")))
        .collect::<Result<Vec<_>, _>>()?;
    let journal = replay(base, applied, undone)?;
    let found = journal.image().current_hash();
    if found != head {
        return Err(SidecarError::HeadMismatch {
            expected: head,
            found,
        });
    }
    Ok(Restored { journal, overrides })
}

/// Why [`decode`] refused a sidecar. Nothing was applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidecarError {
    /// The text is longer than [`MAX_SIDECAR_LEN`].
    TooLarge {
        /// Its length in bytes.
        len: usize,
        /// The limit.
        max: usize,
    },
    /// The text is not valid TOML.
    Syntax {
        /// The parser's message.
        message: String,
    },
    /// The text has no `format = "rrjournal"` key: it is some other file.
    NotAJournal,
    /// The text has no top-level `schema` key.
    MissingSchema {
        /// [`SCHEMA`].
        supported: u32,
    },
    /// The `schema` is not one this reader knows, or not an integer.
    UnsupportedSchema {
        /// The value found, as written in TOML.
        found: String,
        /// [`SCHEMA`].
        supported: u32,
    },
    /// A key is missing, unknown, or of the wrong type.
    Shape {
        /// The deserializer's message, naming the key.
        message: String,
    },
    /// A record is malformed.
    Invalid {
        /// Where it is, e.g. `applied[2].patch[0].old`.
        record: String,
        /// What is wrong with it.
        error: RecordError,
    },
    /// The image is not the one the journal was made against.
    BaseMismatch {
        /// SHA-256 of the image the journal was made against.
        expected: ImageHash,
        /// SHA-256 of the image given.
        found: ImageHash,
    },
    /// The image given has changes of its own; a journal replays only onto
    /// an image as loaded.
    ImageModified,
    /// A group does not apply where the history puts it: a patch reaches
    /// past the end of the image, or its old bytes are not what the image
    /// holds there.
    Replay {
        /// Which group, e.g. `undone[0]`.
        record: String,
        /// Why the journal refused it.
        error: JournalError,
    },
    /// The history replays, but not to the bytes the journal recorded.
    HeadMismatch {
        /// SHA-256 recorded for the bytes after the applied groups.
        expected: ImageHash,
        /// SHA-256 of the bytes after replaying them.
        found: ImageHash,
    },
}

impl fmt::Display for SidecarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { len, max } => write!(
                f,
                "sidecar journal is {len} bytes, over the {max}-byte limit"
            ),
            Self::Syntax { message } => {
                write!(f, "sidecar journal is not valid TOML: {message}")
            }
            Self::NotAJournal => write!(
                f,
                "not a sidecar journal: no top-level `format = \"{FORMAT}\"` key"
            ),
            Self::MissingSchema { supported } => write!(
                f,
                "sidecar journal has no top-level `schema` key (supported schema: {supported})"
            ),
            Self::UnsupportedSchema { found, supported } => write!(
                f,
                "sidecar journal has unsupported schema {found} (supported schema: {supported})"
            ),
            Self::Shape { message } => write!(
                f,
                "sidecar journal does not have the expected keys: {message}"
            ),
            Self::Invalid { record, error } => {
                write!(f, "sidecar journal record `{record}` is invalid: {error}")
            }
            Self::BaseMismatch { expected, found } => write!(
                f,
                "sidecar journal belongs to the image with SHA-256 {expected}, but this image's \
                 SHA-256 is {found}; nothing was applied"
            ),
            Self::ImageModified => f.write_str(
                "a sidecar journal replays only onto an image as loaded, and this one has changes",
            ),
            Self::Replay { record, error } => write!(
                f,
                "sidecar journal group `{record}` does not apply to the image: {error}"
            ),
            Self::HeadMismatch { expected, found } => write!(
                f,
                "sidecar journal records SHA-256 {expected} after its applied groups, but \
                 replaying them gives {found}"
            ),
        }
    }
}

impl core::error::Error for SidecarError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Invalid { error, .. } => Some(error),
            Self::Replay { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// What is wrong with one sidecar record ([`SidecarError::Invalid`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordError {
    /// Bytes that are not an even number of hex digits.
    NotHex,
    /// A hash that is not 64 hex digits.
    NotSha256,
    /// A group with no patches.
    EmptyGroup,
    /// A patch that cannot be built.
    Patch(PatchError),
    /// A checksum override or failure that cannot be built.
    Override(OverrideError),
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotHex => f.write_str("not an even number of hex digits"),
            Self::NotSha256 => f.write_str("not a SHA-256 of 64 hex digits"),
            Self::EmptyGroup => f.write_str("a group with no patches"),
            Self::Patch(error) => error.fmt(f),
            Self::Override(error) => error.fmt(f),
        }
    }
}

impl core::error::Error for RecordError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Patch(error) => Some(error),
            Self::Override(error) => Some(error),
            Self::NotHex | Self::NotSha256 | Self::EmptyGroup => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Writing.

/// The sidecar text of a journal and its overrides, written by `Display`.
struct Document<'a> {
    journal: &'a Journal,
    overrides: &'a [ChecksumOverride],
}

impl fmt::Display for Document<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let image = self.journal.image();
        writeln!(
            f,
            "# RomRaider sidecar journal: the edit history of the ROM beside it."
        )?;
        writeln!(f, "format = \"{FORMAT}\"")?;
        writeln!(f, "schema = {SCHEMA}")?;
        writeln!(f, "base_sha256 = \"{}\"", image.original_hash())?;
        writeln!(f, "head_sha256 = \"{}\"", image.current_hash())?;
        fmt_groups(f, "applied", self.journal.applied())?;
        fmt_groups(f, "undone", self.journal.undone())?;
        for o in self.overrides {
            writeln!(f, "\n[[checksum_override]]")?;
            writeln!(f, "image_sha256 = \"{}\"", o.image)?;
            writeln!(f, "confirmed_at_unix = {}", o.confirmed_at_unix)?;
            for failure in &o.failures {
                writeln!(f, "[[checksum_override.failure]]")?;
                // A checksum name holds no `"`, `\` or control character
                // (`is_checksum_name`), so it needs no escaping.
                writeln!(f, "checksum = \"{}\"", failure.checksum)?;
                writeln!(f, "stored = \"{}\"", Hex(&failure.stored))?;
                writeln!(f, "computed = \"{}\"", Hex(&failure.computed))?;
            }
        }
        Ok(())
    }
}

/// Writes `groups` as the TOML array of tables `list`.
fn fmt_groups(f: &mut fmt::Formatter<'_>, list: &str, groups: &[Group]) -> fmt::Result {
    for group in groups {
        writeln!(f, "\n[[{list}]]")?;
        for patch in group.patches() {
            writeln!(f, "[[{list}.patch]]")?;
            // `at` is at most the image length (the journal applied the
            // patch), so it fits a TOML integer.
            writeln!(f, "at = {:#010x}", patch.at)?;
            writeln!(f, "origin = \"{}\"", origin_name(patch.origin))?;
            writeln!(f, "old = \"{}\"", Hex(&patch.old))?;
            writeln!(f, "new = \"{}\"", Hex(&patch.new))?;
        }
    }
    Ok(())
}

/// The `origin` value of `origin`; [`OriginDoc`] reads it back.
const fn origin_name(origin: Origin) -> &'static str {
    match origin {
        Origin::Edit => "edit",
        Origin::Paste => "paste",
        Origin::ChecksumFix => "checksum_fix",
        Origin::ApplyDiff => "apply_diff",
    }
}

/// Bytes written as lower-case hex digits, two per byte.
struct Hex<'a>(&'a [u8]);

impl fmt::Display for Hex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

// ---------------------------------------------------------------------------
// Reading.

/// The keys of a sidecar besides `format` and `schema`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentDoc {
    base_sha256: String,
    head_sha256: String,
    #[serde(default)]
    applied: Vec<GroupDoc>,
    #[serde(default)]
    undone: Vec<GroupDoc>,
    #[serde(default, rename = "checksum_override")]
    overrides: Vec<OverrideDoc>,
}

/// One `[[applied]]` or `[[undone]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupDoc {
    /// Defaulted so an empty group is reported as [`RecordError::EmptyGroup`].
    #[serde(default)]
    patch: Vec<PatchDoc>,
}

/// One `[[*.patch]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchDoc {
    at: usize,
    origin: OriginDoc,
    old: String,
    new: String,
}

/// An `origin` value, as [`origin_name`] writes it.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum OriginDoc {
    Edit,
    Paste,
    ChecksumFix,
    ApplyDiff,
}

impl From<OriginDoc> for Origin {
    fn from(origin: OriginDoc) -> Self {
        match origin {
            OriginDoc::Edit => Self::Edit,
            OriginDoc::Paste => Self::Paste,
            OriginDoc::ChecksumFix => Self::ChecksumFix,
            OriginDoc::ApplyDiff => Self::ApplyDiff,
        }
    }
}

/// One `[[checksum_override]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OverrideDoc {
    image_sha256: String,
    confirmed_at_unix: u64,
    /// Defaulted so no failures is reported as [`OverrideError::NoFailures`].
    #[serde(default)]
    failure: Vec<FailureDoc>,
}

/// One `[[checksum_override.failure]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FailureDoc {
    checksum: String,
    stored: String,
    computed: String,
}

/// Parses `text` and checks its `format` and `schema` keys.
fn parse(text: &str) -> Result<DocumentDoc, SidecarError> {
    let mut table = toml::from_str::<toml::Table>(text).map_err(|e| SidecarError::Syntax {
        message: e.to_string(),
    })?;
    match table.remove("format") {
        Some(toml::Value::String(format)) if format == FORMAT => {}
        _ => return Err(SidecarError::NotAJournal),
    }
    match table.remove("schema") {
        None => {
            return Err(SidecarError::MissingSchema { supported: SCHEMA });
        }
        Some(toml::Value::Integer(n)) if n == i64::from(SCHEMA) => {}
        Some(other) => {
            return Err(SidecarError::UnsupportedSchema {
                found: other.to_string(),
                supported: SCHEMA,
            });
        }
    }
    toml::Value::Table(table)
        .try_into::<DocumentDoc>()
        .map_err(|e| SidecarError::Shape {
            message: e.to_string(),
        })
}

/// A [`SidecarError::Invalid`] at `record`.
fn invalid(record: impl Into<String>, error: RecordError) -> SidecarError {
    SidecarError::Invalid {
        record: record.into(),
        error,
    }
}

/// The hash written as `text` in the record `record`.
fn hash(text: &str, record: &str) -> Result<ImageHash, SidecarError> {
    from_hex(text)
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
        .map(ImageHash::from_bytes)
        .ok_or_else(|| invalid(record, RecordError::NotSha256))
}

/// The bytes written as `text` in the record `record`.
fn bytes(text: &str, record: &str) -> Result<Vec<u8>, SidecarError> {
    from_hex(text).ok_or_else(|| invalid(record, RecordError::NotHex))
}

/// The groups of the array of tables `list`, each checked to be non-empty
/// and made of well-formed patches.
fn groups(docs: Vec<GroupDoc>, list: &str) -> Result<Vec<Vec<Patch>>, SidecarError> {
    docs.into_iter()
        .enumerate()
        .map(|(g, doc)| {
            if doc.patch.is_empty() {
                return Err(invalid(format!("{list}[{g}]"), RecordError::EmptyGroup));
            }
            doc.patch
                .into_iter()
                .enumerate()
                .map(|(p, patch)| {
                    let record = format!("{list}[{g}].patch[{p}]");
                    let old = bytes(&patch.old, &format!("{record}.old"))?;
                    let new = bytes(&patch.new, &format!("{record}.new"))?;
                    Patch::new(patch.at, old, new, patch.origin.into())
                        .map_err(|e| invalid(record, RecordError::Patch(e)))
                })
                .collect()
        })
        .collect()
}

/// The checksum override `doc`, at `record`.
fn checksum_override(doc: OverrideDoc, record: &str) -> Result<ChecksumOverride, SidecarError> {
    let image = hash(&doc.image_sha256, &format!("{record}.image_sha256"))?;
    let failures = doc
        .failure
        .into_iter()
        .enumerate()
        .map(|(i, failure)| {
            let at = format!("{record}.failure[{i}]");
            let stored = bytes(&failure.stored, &format!("{at}.stored"))?;
            let computed = bytes(&failure.computed, &format!("{at}.computed"))?;
            ChecksumFailure::new(failure.checksum, stored, computed)
                .map_err(|e| invalid(at, RecordError::Override(e)))
        })
        .collect::<Result<Vec<_>, _>>()?;
    ChecksumOverride::new(image, doc.confirmed_at_unix, failures)
        .map_err(|e| invalid(record, RecordError::Override(e)))
}

/// A journal over `base` with `applied` applied and `undone` ready to redo.
///
/// Every group goes through [`Journal::apply_group`], so every patch is
/// checked against the image as the history before it left it. The undone
/// groups are applied after the applied ones (where redo would put them)
/// and then undone again, newest first.
fn replay(
    base: Image,
    applied: Vec<Vec<Patch>>,
    undone: Vec<Vec<Patch>>,
) -> Result<Journal, SidecarError> {
    let replay_error = |list: &str, i: usize, error: JournalError| SidecarError::Replay {
        record: format!("{list}[{i}]"),
        error,
    };
    let mut journal = Journal::new(base);
    for (i, group) in applied.into_iter().enumerate() {
        journal
            .apply_group(group)
            .map_err(|e| replay_error("applied", i, e))?;
    }
    let undone_len = undone.len();
    for (i, group) in undone.into_iter().enumerate() {
        journal
            .apply_group(group)
            .map_err(|e| replay_error("undone", i, e))?;
    }
    for i in (0..undone_len).rev() {
        // Cannot be refused: it reverts a group just applied to the image
        // this function holds exclusively.
        journal.undo().map_err(|e| replay_error("undone", i, e))?;
    }
    Ok(journal)
}

/// `text` read as hex digits, two per byte, in either case; `None` unless
/// it is an even number of hex digits.
fn from_hex(text: &str) -> Option<Vec<u8>> {
    let digits = text.as_bytes();
    if !digits.len().is_multiple_of(2) {
        return None;
    }
    digits
        .chunks_exact(2)
        .map(|pair| match *pair {
            [high, low] => Some(nibble(high)? * 16 + nibble(low)?),
            _ => None,
        })
        .collect()
}

/// The value of the hex digit `digit`.
fn nibble(digit: u8) -> Option<u8> {
    char::from(digit)
        .to_digit(16)
        .and_then(|value| u8::try_from(value).ok())
}

#[cfg(test)]
mod tests;
