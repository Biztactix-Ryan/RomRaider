//! Pattern (concept C2): one compiled matcher.
//!
//! ROM identification, ECU-ID profile selection, quirk `when` clauses, frame
//! sync headers, response validation and CSV log-format detection all ask
//! the same question: does this input look like that? Legacy answered it in
//! a different place each time: `RomID.checkMatch` compared the bytes at
//! `internalidaddress` with `internalidstring` (exact bytes for a `0x...`
//! string, ASCII case-insensitive text otherwise,
//! `maps/RomID.java:58-93`), the logger looked ECU IDs up in a map
//! (`logger/ecu/EcuLogger.java:402-422`), and the dyno tab guessed a CSV
//! format from substrings of its headers
//! (`logger/ecu/ui/tab/dyno/DynoControlPanel.java:1025-1031`). Here there is
//! one matcher, compiled once and matched cheaply, in four kinds:
//!
//! | Kind | Compiled type | Matches | Used for |
//! |------|---------------|---------|----------|
//! | bytes | [`BytePattern`] | bytes under a mask at an offset (`80 F0 ?? 01`) | ROM ID, frame sync, response validation |
//! | named op | [`OpPattern`] | an operation name, exactly | quirk `when` clauses |
//! | ID prefix | [`IdPrefix`] | the start of an ECU / ROM ID string | ECU-ID → profile selection |
//! | header set | [`HeaderSet`] | every required name among a CSV header row | log-format detection |
//!
//! [`Pattern`] is the one type a caller stores; each kind is also usable on
//! its own through its typed methods.
//!
//! # Compile once, match cheaply
//!
//! Every constructor validates its input and refuses a pattern with no
//! defined meaning (see [`PatternError`]): an empty byte string, a mask of
//! the wrong length, an offset whose end overflows `usize`, an empty name, a
//! duplicate header. A value of a pattern type is therefore always one that
//! can match something. A byte pattern stores its value pre-masked, so
//! matching is one bounds check and one `AND` and compare per byte. Nothing
//! is parsed, allocated or looked up at match time.
//!
//! # Text form
//!
//! A byte pattern also has a text form, read by [`BytePattern::parse`] (and
//! `FromStr`) and written by its `Display`:
//!
//! ```text
//! [@OFFSET] BYTE...
//! ```
//!
//! * **Words** are separated by spaces or tabs (any number, leading and
//!   trailing allowed). No other whitespace or separator is accepted.
//! * **Bytes** are pairs of hex digits, either case: `80 F0 01`. Pairs may
//!   run together in one word (`80F001`, as ECU IDs are written), and a word
//!   may start with `0x` / `0X` (`0x80F001`, as legacy's `internalidstring`
//!   writes hex).
//! * **Wildcards**: `??` matches any byte; `8?` checks only the high nibble
//!   (mask `F0`) and `?1` only the low one (mask `0F`).
//! * **Masks**: `VV/MM` checks only the bits set in `MM` (two hex digits),
//!   as `41/DF` matches `A` and `a`. The byte may not have bits outside its
//!   mask (`41/0F` is refused) or a `?` wildcard.
//! * **Offset**: an optional first word `@` + decimal digits or `@0x` + hex
//!   digits places the pattern's first byte at that offset (`@0x2000 80 F0
//!   ?? 01`, `@8192 80F0??01`). With none, the offset is `0`.
//!
//! The text is at most [`MAX_TEXT_LEN`] bytes and the pattern at most
//! [`MAX_BYTES`] bytes. Anything else is a
//! [`ParseError`] whose [`code`](ParseError::code) is `PAT0001` (syntax),
//! `PAT0002` (too long) or `PAT0003` (offset), with its byte span and
//! column; [`ParseError::to_diagnostic_at`] turns it into a
//! [`Diagnostic`](crate::diag::Diagnostic). Parsing never panics.
//!
//! `Display` writes the canonical form (upper-case, one space between
//! bytes, `@0x` offset only when it is not `0`), which parses back to an
//! equal pattern: every [`BytePattern`] of at most [`MAX_BYTES`] bytes (so
//! every one parsed from text) has a text form.
//!
//! # Why a match failed
//!
//! Each kind has a cheap `matches` returning `bool` and a `check` returning
//! `Result<(), Mismatch>`. A [`Mismatch`] says why there was no match (the
//! input is too short, which byte differs, which header is missing) and
//! holds only `Copy` data, so building one never allocates.
//! [`Pattern::explain`] renders it with the pattern's own names
//! (`no header named "Time/s"`).
//!
//! # Never panics
//!
//! Patterns are compiled from untrusted definitions and matched against
//! untrusted ROM images, frames and CSV files. Every index is
//! bounds-checked and every offset sum is a checked addition: a pattern
//! that reaches past the end of its input, at any offset up to
//! `usize::MAX`, is a [`Mismatch::OutOfBounds`], never a panic.
//!
//! The module holds no `static` of any kind: a pattern is plain data.
//!
//! # Example
//!
//! ```
//! use rr_core::pattern::{BytePattern, Mismatch, Pattern, Subject};
//!
//! // `80 F0 ?? 01` at offset 2.
//! let p = BytePattern::from_cells(2, &[Some(0x80), Some(0xF0), None, Some(0x01)]).unwrap();
//! assert!(p.matches(&[0, 0, 0x80, 0xF0, 0x55, 0x01]));
//! assert_eq!(
//!     p.check(&[0, 0, 0x80, 0xF1, 0x55, 0x01]),
//!     Err(Mismatch::ByteDiffers { position: 3, expected: 0xF0, mask: 0xFF, found: 0xF1 }),
//! );
//! assert!(matches!(p.check(&[0, 0, 0x80]), Err(Mismatch::OutOfBounds { .. })));
//!
//! let p = Pattern::from(p);
//! let m = p.check(Subject::Bytes(&[0; 4])).unwrap_err();
//! assert_eq!(
//!     p.explain(&m).to_string(),
//!     "input is 4 bytes; the pattern needs bytes 0x2..0x6",
//! );
//! ```

mod bytes;
mod name;
mod parse;

pub use bytes::BytePattern;
pub use name::{Case, HeaderSet, IdPrefix, OpPattern};
pub use parse::{MAX_BYTES, MAX_TEXT_LEN, ParseError, ParseErrorKind};

use core::fmt;

/// A compiled pattern of any kind.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Pattern {
    /// Bytes under a mask at an offset.
    Bytes(BytePattern),
    /// A named operation.
    Op(OpPattern),
    /// The start of an ID string.
    IdPrefix(IdPrefix),
    /// A set of required header names.
    HeaderSet(HeaderSet),
}

impl Pattern {
    /// The kind of pattern, which is also the kind of [`Subject`] it matches.
    #[must_use]
    pub const fn kind(&self) -> PatternKind {
        match self {
            Self::Bytes(_) => PatternKind::Bytes,
            Self::Op(_) => PatternKind::Op,
            Self::IdPrefix(_) => PatternKind::IdPrefix,
            Self::HeaderSet(_) => PatternKind::HeaderSet,
        }
    }

    /// Whether `subject` matches. As `check(subject).is_ok()`, but never
    /// builds a [`Mismatch`].
    #[must_use]
    pub fn matches(&self, subject: Subject<'_>) -> bool {
        match (self, subject) {
            (Self::Bytes(p), Subject::Bytes(input)) => p.matches(input),
            (Self::Op(p), Subject::Op(op)) => p.matches(op),
            (Self::IdPrefix(p), Subject::Id(id)) => p.matches(id),
            (Self::HeaderSet(p), Subject::Headers(headers)) => p.matches(headers),
            _ => false,
        }
    }

    /// Matches `subject`.
    ///
    /// # Errors
    ///
    /// A [`Mismatch`] saying why `subject` does not match:
    /// [`Mismatch::WrongSubject`] when the subject is of another kind than
    /// the pattern, otherwise as the kind's own `check`.
    pub fn check(&self, subject: Subject<'_>) -> Result<(), Mismatch> {
        match (self, subject) {
            (Self::Bytes(p), Subject::Bytes(input)) => p.check(input),
            (Self::Op(p), Subject::Op(op)) => p.check(op),
            (Self::IdPrefix(p), Subject::Id(id)) => p.check(id),
            (Self::HeaderSet(p), Subject::Headers(headers)) => p.check(headers),
            (p, s) => Err(Mismatch::WrongSubject {
                pattern: p.kind(),
                subject: s.kind(),
            }),
        }
    }

    /// `mismatch`, rendered with this pattern's names and values
    /// (`no header named "Time/s"` rather than `required header 2 is
    /// missing`).
    ///
    /// A mismatch that did not come from this pattern renders as its own
    /// [`Display`](fmt::Display).
    #[must_use]
    pub const fn explain<'a>(&'a self, mismatch: &'a Mismatch) -> Explain<'a> {
        Explain {
            pattern: self,
            mismatch,
        }
    }
}

impl From<BytePattern> for Pattern {
    fn from(p: BytePattern) -> Self {
        Self::Bytes(p)
    }
}

impl From<OpPattern> for Pattern {
    fn from(p: OpPattern) -> Self {
        Self::Op(p)
    }
}

impl From<IdPrefix> for Pattern {
    fn from(p: IdPrefix) -> Self {
        Self::IdPrefix(p)
    }
}

impl From<HeaderSet> for Pattern {
    fn from(p: HeaderSet) -> Self {
        Self::HeaderSet(p)
    }
}

/// The four kinds of pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PatternKind {
    /// [`BytePattern`], matching [`Subject::Bytes`].
    Bytes,
    /// [`OpPattern`], matching [`Subject::Op`].
    Op,
    /// [`IdPrefix`], matching [`Subject::Id`].
    IdPrefix,
    /// [`HeaderSet`], matching [`Subject::Headers`].
    HeaderSet,
}

impl fmt::Display for PatternKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Bytes => "bytes",
            Self::Op => "op",
            Self::IdPrefix => "ID prefix",
            Self::HeaderSet => "header set",
        })
    }
}

/// What a [`Pattern`] is matched against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Subject<'a> {
    /// A byte string: a ROM image, a frame, a response.
    Bytes(&'a [u8]),
    /// An operation name.
    Op(&'a str),
    /// An ECU or ROM ID.
    Id(&'a str),
    /// A CSV header row, one name per column.
    Headers(&'a [&'a str]),
}

impl Subject<'_> {
    /// The kind of pattern that matches this subject.
    #[must_use]
    pub const fn kind(&self) -> PatternKind {
        match self {
            Self::Bytes(_) => PatternKind::Bytes,
            Self::Op(_) => PatternKind::Op,
            Self::Id(_) => PatternKind::IdPrefix,
            Self::Headers(_) => PatternKind::HeaderSet,
        }
    }
}

/// Why an input did not match a pattern.
///
/// Holds only `Copy` data, so a failed match never allocates;
/// [`Pattern::explain`] adds the pattern's names when rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Mismatch {
    /// The subject is of another kind than the pattern.
    WrongSubject {
        /// The pattern's kind.
        pattern: PatternKind,
        /// The subject's kind.
        subject: PatternKind,
    },
    /// The byte pattern reaches past the end of the input (or past
    /// `usize::MAX`): it needs bytes `base + offset .. base + offset + len`.
    OutOfBounds {
        /// Where in the input the pattern was placed (`0` for
        /// [`BytePattern::check`]).
        base: usize,
        /// The pattern's offset from `base`.
        offset: usize,
        /// The pattern's length in bytes.
        len: usize,
        /// The input's length in bytes.
        input_len: usize,
    },
    /// A byte differs from the pattern in a bit the mask checks. This is the
    /// first such byte.
    ByteDiffers {
        /// Absolute position of the byte in the input.
        position: usize,
        /// The expected value (already masked).
        expected: u8,
        /// The mask: the bits that are checked.
        mask: u8,
        /// The byte found.
        found: u8,
    },
    /// The op name is not the pattern's.
    OpDiffers,
    /// The ID is shorter than the prefix.
    IdTooShort {
        /// Length of the prefix in bytes.
        prefix_len: usize,
        /// Length of the ID in bytes.
        id_len: usize,
    },
    /// The ID differs from the prefix; this is the first differing byte.
    IdDiffers {
        /// Byte position in the ID.
        position: usize,
    },
    /// A required header is not in the header row. This is the first one
    /// missing.
    HeaderMissing {
        /// Index of the name in [`HeaderSet::names`].
        index: usize,
    },
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::WrongSubject { pattern, subject } => {
                write!(
                    f,
                    "pattern kind {pattern} cannot match subject kind {subject}"
                )
            }
            Self::OutOfBounds {
                base,
                offset,
                len,
                input_len,
            } => {
                write!(f, "input is {input_len} bytes; ")?;
                match base
                    .checked_add(offset)
                    .and_then(|start| Some((start, start.checked_add(len)?)))
                {
                    Some((start, end)) => {
                        write!(f, "the pattern needs bytes {start:#x}..{end:#x}")
                    }
                    None => write!(
                        f,
                        "the pattern ({len} bytes at {offset:#x} past {base:#x}) \
                         reaches past the end of the address space"
                    ),
                }
            }
            Self::ByteDiffers {
                position,
                expected,
                mask,
                found,
            } => {
                write!(
                    f,
                    "byte at {position:#x} is {found:#04x}; expected {expected:#04x}"
                )?;
                if mask != 0xFF {
                    write!(f, " under mask {mask:#04x}")?;
                }
                Ok(())
            }
            Self::OpDiffers => f.write_str("the op differs"),
            Self::IdTooShort { prefix_len, id_len } => write!(
                f,
                "ID is {id_len} bytes, shorter than the {prefix_len}-byte prefix"
            ),
            Self::IdDiffers { position } => {
                write!(f, "ID differs from the prefix at byte {position}")
            }
            Self::HeaderMissing { index } => write!(f, "required header {index} is missing"),
        }
    }
}

/// A [`Mismatch`] rendered with its pattern's names (see
/// [`Pattern::explain`]).
#[derive(Debug, Clone, Copy)]
pub struct Explain<'a> {
    pattern: &'a Pattern,
    mismatch: &'a Mismatch,
}

impl fmt::Display for Explain<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.pattern, *self.mismatch) {
            (Pattern::Op(p), Mismatch::OpDiffers) => write!(f, "op is not {:?}", p.name()),
            (Pattern::IdPrefix(p), Mismatch::IdTooShort { id_len, .. }) => write!(
                f,
                "ID is {id_len} bytes, shorter than the prefix {:?}",
                p.prefix()
            ),
            (Pattern::IdPrefix(p), Mismatch::IdDiffers { position }) => write!(
                f,
                "ID does not start with {:?} (differs at byte {position})",
                p.prefix()
            ),
            (Pattern::HeaderSet(p), Mismatch::HeaderMissing { index }) => {
                match p.names().get(index) {
                    Some(name) => write!(f, "no header named {name:?}"),
                    None => fmt::Display::fmt(self.mismatch, f),
                }
            }
            _ => fmt::Display::fmt(self.mismatch, f),
        }
    }
}

/// Why a pattern could not be compiled.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PatternError {
    /// A byte pattern with no bytes, which would match every input.
    EmptyBytes,
    /// A mask whose length differs from the byte string's.
    MaskLength {
        /// Length of the byte string.
        bytes: usize,
        /// Length of the mask.
        mask: usize,
    },
    /// A byte pattern whose end, `offset + len`, overflows `usize`, so it
    /// could never match.
    OffsetOverflow {
        /// The offset.
        offset: usize,
        /// The pattern's length in bytes.
        len: usize,
    },
    /// An empty op name, ID prefix or header name.
    EmptyName {
        /// The kind of pattern being compiled.
        kind: PatternKind,
    },
    /// An op name containing whitespace or a control character.
    InvalidOpName {
        /// Byte position of the first offending character.
        position: usize,
    },
    /// A header set with no names, which would match every header row.
    EmptyHeaderSet,
    /// The same header name twice in a header set (compared as the set's
    /// [`Case`] compares).
    DuplicateHeader {
        /// Index of the first occurrence.
        first: usize,
        /// Index of the repeat.
        second: usize,
    },
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyBytes => f.write_str("byte pattern is empty"),
            Self::MaskLength { bytes, mask } => write!(
                f,
                "mask is {mask} bytes but the byte string is {bytes} bytes"
            ),
            Self::OffsetOverflow { offset, len } => write!(
                f,
                "byte pattern of {len} bytes at offset {offset:#x} ends past the address space"
            ),
            Self::EmptyName { kind } => write!(f, "{kind} pattern has an empty name"),
            Self::InvalidOpName { position } => write!(
                f,
                "op name has whitespace or a control character at byte {position}"
            ),
            Self::EmptyHeaderSet => f.write_str("header set has no names"),
            Self::DuplicateHeader { first, second } => {
                write!(f, "header name {second} repeats header name {first}")
            }
        }
    }
}

impl core::error::Error for PatternError {}

#[cfg(test)]
mod tests;
