//! The text form of byte patterns (US-RR-23-5): `@0x2000 80 F0 ?? 01`.
//!
//! The syntax is documented in the [module docs](super#text-form); this
//! file holds the parser, the printer ([`Display`](fmt::Display), which
//! [`BytePattern::parse`] reads back to an equal pattern) and the errors.

use core::fmt;
use core::ops::Range;
use core::str::FromStr;

use super::{BytePattern, PatternError};
use crate::diag::{Code, Diagnostic, Provenance, Span, codes};

/// The longest pattern text [`BytePattern::parse`] reads, in bytes. Longer
/// text is refused before it is read ([`ParseErrorKind::TooLong`]); real
/// patterns (a ROM ID, a frame header) are under 100 bytes.
pub const MAX_TEXT_LEN: usize = 4096;

/// The most bytes a pattern parsed from text may have
/// ([`ParseErrorKind::TooManyBytes`] beyond). Real patterns (a ROM ID, a
/// frame header) are under 32 bytes. The limit keeps the canonical text of
/// any parsed pattern (at most 6 characters per byte, plus the offset)
/// within [`MAX_TEXT_LEN`], so whatever parses prints as text that parses
/// back.
pub const MAX_BYTES: usize = 512;

impl BytePattern {
    /// Parses the text form of a byte pattern (see the
    /// [module docs](super#text-form)): an optional `@offset`, then bytes as
    /// pairs of hex digits, with `?` wildcards and `/mask` suffixes.
    ///
    /// ```
    /// use rr_core::pattern::BytePattern;
    ///
    /// let p = BytePattern::parse("@0x2 80 F0 ?? 01").unwrap();
    /// assert_eq!(p.offset(), 2);
    /// assert_eq!(p.mask(), &[0xFF, 0xFF, 0x00, 0xFF]);
    /// assert_eq!(p.to_string(), "@0x2 80 F0 ?? 01");
    /// assert_eq!(BytePattern::parse("@2 80F0??01").unwrap(), p);
    ///
    /// let e = BytePattern::parse("80 F0 ?? 0").unwrap_err();
    /// assert_eq!(e.code().as_str(), "PAT0001");
    /// assert_eq!(e.column(), 10);
    /// ```
    ///
    /// # Errors
    ///
    /// A [`ParseError`] for the first problem in the text, with its
    /// [`code`](ParseError::code) and position; see [`ParseErrorKind`].
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        Parser::new(text).parse()
    }
}

impl FromStr for BytePattern {
    type Err = ParseError;

    /// As [`BytePattern::parse`].
    fn from_str(text: &str) -> Result<Self, ParseError> {
        Self::parse(text)
    }
}

/// The canonical text form: `@0x<OFFSET>` (upper-case hex, left out when
/// the offset is `0`), then each byte as two upper-case hex digits, `?` for
/// an unchecked nibble, or `VV/MM` for any other mask, separated by single
/// spaces. [`BytePattern::parse`] reads it back to an equal pattern when
/// the pattern has at most [`MAX_BYTES`] bytes (as every pattern parsed from
/// text has).
impl fmt::Display for BytePattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.offset() != 0 {
            write!(f, "@{:#X} ", self.offset())?;
        }
        for (i, (&v, &m)) in self.value().iter().zip(self.mask()).enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            match m {
                0xFF => write!(f, "{v:02X}")?,
                0x00 => f.write_str("??")?,
                0xF0 => write!(f, "{:X}?", v >> 4)?,
                0x0F => write!(f, "?{:X}", v & 0x0F)?,
                _ => write!(f, "{v:02X}/{m:02X}")?,
            }
        }
        Ok(())
    }
}

/// Why pattern text did not parse, and where.
///
/// [`code`](Self::code) is the registry code it is reported with and
/// [`to_diagnostic`](Self::to_diagnostic) /
/// [`to_diagnostic_at`](Self::to_diagnostic_at) make the
/// [`Error`](crate::diag::Severity::Error) diagnostic. `Display` is the
/// message alone; the location is [`span`](Self::span) (a byte range of the
/// text) and [`column`](Self::column) (where that range starts, counted in
/// characters as an editor shows it).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParseError {
    kind: ParseErrorKind,
    start: usize,
    end: usize,
    column: u32,
}

impl ParseError {
    /// What went wrong.
    #[must_use]
    pub const fn kind(&self) -> &ParseErrorKind {
        &self.kind
    }

    /// The text at fault, as a byte range of the parsed text. It lies on
    /// character boundaries; it is empty only for empty text.
    #[must_use]
    pub const fn span(&self) -> Range<usize> {
        self.start..self.end
    }

    /// The 1-based column, in characters, at which [`span`](Self::span)
    /// starts.
    #[must_use]
    pub const fn column(&self) -> u32 {
        self.column
    }

    /// The registry code this error is reported with.
    #[must_use]
    pub const fn code(&self) -> Code {
        self.kind.code()
    }

    /// This error as an [`Error`](crate::diag::Severity::Error) diagnostic at
    /// `provenance`, with [`code`](Self::code) and the `Display` message.
    #[must_use]
    pub fn to_diagnostic(&self, provenance: Provenance) -> Diagnostic {
        Diagnostic::error(self.code(), self.to_string(), provenance)
    }

    /// As [`to_diagnostic`](Self::to_diagnostic), for pattern text that
    /// starts at `line:col` (1-based) of `source`: the diagnostic points at
    /// `line:(col + column - 1)`, the start of the text at fault.
    #[must_use]
    pub fn to_diagnostic_at(&self, source: impl Into<String>, line: u32, col: u32) -> Diagnostic {
        let col = col.saturating_add(self.column.saturating_sub(1));
        self.to_diagnostic(Provenance::new(source, Span::line_col(line, col)))
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl core::error::Error for ParseError {}

/// The kinds of [`ParseError`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParseErrorKind {
    /// No bytes: the text is empty, only whitespace, or only an offset. A
    /// pattern with no bytes would match every input. The span is the whole
    /// text.
    Empty,
    /// A character that cannot be where it is: not a hex digit, `?`, `/`,
    /// `@`, space or tab, or one of those out of place (a `/` with no byte
    /// before it, a `?` in a mask). The span is the character.
    UnexpectedChar(char),
    /// A byte with one hex digit (or `?`): `80 F` or `80F`. The span is the
    /// lone digit.
    IncompleteByte,
    /// A `0x` prefix with no hex digits after it. The span is the `0x`.
    EmptyHexPrefix,
    /// A `/` mask with fewer than two hex digits. The span is from the `/`
    /// to the end of its word.
    IncompleteMask,
    /// A `/` mask after a byte that already has a `?` wildcard (`8?/F0`):
    /// the two would disagree on which bits are checked. The span is the
    /// byte and the `/`.
    MaskOnWildcard,
    /// A byte with bits set outside its mask (`41/0F`): those bits could
    /// never be checked, so the byte is almost certainly not what was meant.
    /// The span is the byte and its mask.
    ValueOutsideMask {
        /// The byte as written.
        value: u8,
        /// Its mask.
        mask: u8,
    },
    /// An `@` not followed by a number: decimal digits, or `0x` and hex
    /// digits. The span is the whole `@` word.
    BadOffset,
    /// An `@` offset after the bytes, or a second one. The span is the `@`
    /// word.
    MisplacedOffset,
    /// An offset larger than the largest address (`usize::MAX`). The span is
    /// the `@` word.
    OffsetTooLarge,
    /// The pattern would end past the largest address: `offset + len`
    /// overflows `usize`. The span is the `@` word.
    OffsetOverflow {
        /// The offset.
        offset: usize,
        /// The pattern's length in bytes.
        len: usize,
    },
    /// More than [`MAX_BYTES`] bytes. The span is the first byte too many
    /// (with its mask, if any).
    TooManyBytes {
        /// The limit.
        limit: usize,
    },
    /// The text is longer than [`MAX_TEXT_LEN`] bytes. Checked before
    /// anything else; the span is the whole text.
    TooLong {
        /// The text's length, in bytes.
        len: usize,
        /// The limit.
        limit: usize,
    },
}

impl ParseErrorKind {
    /// The registry code this kind is reported with.
    #[must_use]
    pub const fn code(&self) -> Code {
        match self {
            Self::TooLong { .. } | Self::TooManyBytes { .. } => codes::PATTERN_TOO_LONG,
            Self::BadOffset
            | Self::MisplacedOffset
            | Self::OffsetTooLarge
            | Self::OffsetOverflow { .. } => codes::PATTERN_BAD_OFFSET,
            Self::Empty
            | Self::UnexpectedChar(_)
            | Self::IncompleteByte
            | Self::EmptyHexPrefix
            | Self::IncompleteMask
            | Self::MaskOnWildcard
            | Self::ValueOutsideMask { .. } => codes::PATTERN_SYNTAX,
        }
    }
}

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the byte pattern has no bytes"),
            Self::UnexpectedChar(c) => write!(
                f,
                "unexpected character {c:?}; a byte is two hex digits or `?` wildcards"
            ),
            Self::IncompleteByte => {
                f.write_str("incomplete byte; a byte is two hex digits or `?` wildcards")
            }
            Self::EmptyHexPrefix => f.write_str("`0x` is not followed by any hex digits"),
            Self::IncompleteMask => f.write_str("incomplete mask; a `/` mask is two hex digits"),
            Self::MaskOnWildcard => {
                f.write_str("a byte with a `?` wildcard cannot also have a `/` mask")
            }
            Self::ValueOutsideMask { value, mask } => write!(
                f,
                "byte {value:02X} has bits set outside its mask {mask:02X}"
            ),
            Self::BadOffset => {
                f.write_str("an offset is `@` then decimal digits, or `@0x` then hex digits")
            }
            Self::MisplacedOffset => f.write_str("the `@` offset must come once, before the bytes"),
            Self::OffsetTooLarge => write!(
                f,
                "the offset is larger than the largest address, {:#X}",
                usize::MAX
            ),
            Self::OffsetOverflow { offset, len } => write!(
                f,
                "{len} bytes at offset {offset:#X} end past the largest address"
            ),
            Self::TooManyBytes { limit } => {
                write!(
                    f,
                    "the pattern has more than {limit} bytes; the limit is {limit}"
                )
            }
            Self::TooLong { len, limit } => write!(
                f,
                "the pattern text is {len} bytes long; the limit is {limit} bytes"
            ),
        }
    }
}

/// One nibble of a byte as written: a hex digit or a `?`.
#[derive(Clone, Copy)]
enum Nibble {
    Digit(u8),
    Wild,
}

impl Nibble {
    const fn of(b: u8) -> Option<Self> {
        match b {
            b'?' => Some(Self::Wild),
            _ => match hex(b) {
                Some(d) => Some(Self::Digit(d)),
                None => None,
            },
        }
    }

    /// The nibble's value and mask, in the low four bits.
    const fn value_mask(self) -> (u8, u8) {
        match self {
            Self::Digit(d) => (d, 0x0F),
            Self::Wild => (0, 0),
        }
    }
}

const fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

const fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t')
}

/// Whether `word` starts with a `0x` / `0X` prefix.
fn has_hex_prefix(word: &[u8]) -> bool {
    matches!(word, [b'0', b'x' | b'X', ..])
}

struct Parser<'a> {
    text: &'a str,
    offset: usize,
    /// Byte range of the `@` word, once one is read.
    offset_span: Option<Range<usize>>,
    cells: Vec<(u8, u8)>,
}

impl<'a> Parser<'a> {
    const fn new(text: &'a str) -> Self {
        Self {
            text,
            offset: 0,
            offset_span: None,
            cells: Vec::new(),
        }
    }

    fn error(&self, kind: ParseErrorKind, span: Range<usize>) -> ParseError {
        // Every span the parser makes starts on a character boundary (it
        // only ever steps over ASCII), so `get` finds the prefix.
        let before = self.text.get(..span.start).unwrap_or("");
        let column = u32::try_from(before.chars().count())
            .unwrap_or(u32::MAX)
            .saturating_add(1);
        ParseError {
            kind,
            start: span.start,
            end: span.end,
            column,
        }
    }

    /// The character at byte `at` (a character boundary), as an
    /// [`ParseErrorKind::UnexpectedChar`] spanning it.
    fn unexpected(&self, at: usize) -> ParseError {
        let c = self
            .text
            .get(at..)
            .and_then(|s| s.chars().next())
            .unwrap_or(char::REPLACEMENT_CHARACTER);
        self.error(ParseErrorKind::UnexpectedChar(c), at..at + c.len_utf8())
    }

    fn parse(mut self) -> Result<BytePattern, ParseError> {
        let text = self.text.as_bytes();
        if text.len() > MAX_TEXT_LEN {
            return Err(self.error(
                ParseErrorKind::TooLong {
                    len: text.len(),
                    limit: MAX_TEXT_LEN,
                },
                0..text.len(),
            ));
        }
        // Words run between spaces and tabs (both ASCII, so every word
        // starts and ends on a character boundary). Runs of blanks give
        // empty words, which read as nothing.
        let mut start = 0;
        for word in text.split(|&b| is_space(b)) {
            let end = start + word.len();
            if word.first() == Some(&b'@') {
                self.read_offset(word, start..end)?;
            } else {
                self.read_bytes(word, start)?;
            }
            start = end + 1;
        }
        match BytePattern::build(self.offset, self.cells.iter().copied()) {
            Ok(p) => Ok(p),
            Err(PatternError::OffsetOverflow { offset, len }) => Err(self.error(
                ParseErrorKind::OffsetOverflow { offset, len },
                self.offset_span.clone().unwrap_or(0..text.len()),
            )),
            // `build` refuses only an empty pattern and an overflowing one.
            Err(_) => Err(self.error(ParseErrorKind::Empty, 0..text.len())),
        }
    }

    /// Reads the `@offset` word at `span`.
    fn read_offset(&mut self, word: &[u8], span: Range<usize>) -> Result<(), ParseError> {
        if self.offset_span.is_some() || !self.cells.is_empty() {
            return Err(self.error(ParseErrorKind::MisplacedOffset, span));
        }
        let number = word.get(1..).unwrap_or(&[]);
        let (digits, radix) = if has_hex_prefix(number) {
            (number.get(2..).unwrap_or(&[]), 16)
        } else {
            (number, 10)
        };
        let is_digit = |b: &u8| char::from(*b).is_digit(radix);
        if digits.is_empty() || !digits.iter().all(is_digit) {
            return Err(self.error(ParseErrorKind::BadOffset, span));
        }
        // Only ASCII digits of `radix` remain (no sign, which
        // `from_str_radix` would accept), so the only possible failure is
        // a value too large for `usize`.
        let parsed = core::str::from_utf8(digits)
            .ok()
            .and_then(|s| usize::from_str_radix(s, radix).ok());
        let Some(offset) = parsed else {
            return Err(self.error(ParseErrorKind::OffsetTooLarge, span));
        };
        self.offset = offset;
        self.offset_span = Some(span);
        Ok(())
    }

    /// Reads a word of bytes starting at byte `start` of the text.
    fn read_bytes(&mut self, word: &[u8], start: usize) -> Result<(), ParseError> {
        let mut i = 0;
        if has_hex_prefix(word) {
            if word.len() == 2 {
                return Err(self.error(ParseErrorKind::EmptyHexPrefix, start..start + 2));
            }
            i = 2;
        }
        while i < word.len() {
            let cell_start = start + i;
            let nibble = |j: usize| word.get(j).copied().and_then(Nibble::of);
            let Some(hi) = nibble(i) else {
                return Err(self.unexpected(cell_start));
            };
            if i + 1 >= word.len() {
                return Err(self.error(ParseErrorKind::IncompleteByte, cell_start..cell_start + 1));
            }
            let Some(lo) = nibble(i + 1) else {
                return Err(self.unexpected(cell_start + 1));
            };
            let (hv, hm) = hi.value_mask();
            let (lv, lm) = lo.value_mask();
            let value = (hv << 4) + lv;
            let mut mask = (hm << 4) + lm;
            i += 2;
            if word.get(i) == Some(&b'/') {
                if mask != 0xFF {
                    return Err(
                        self.error(ParseErrorKind::MaskOnWildcard, cell_start..start + i + 1)
                    );
                }
                let mut m = 0u8;
                for j in [i + 1, i + 2] {
                    if j >= word.len() {
                        return Err(self.error(
                            ParseErrorKind::IncompleteMask,
                            start + i..start + word.len(),
                        ));
                    }
                    let Some(d) = word.get(j).copied().and_then(hex) else {
                        return Err(self.unexpected(start + j));
                    };
                    m = (m << 4) + d;
                }
                mask = m;
                if value & !mask != 0 {
                    return Err(self.error(
                        ParseErrorKind::ValueOutsideMask { value, mask },
                        cell_start..start + i + 3,
                    ));
                }
                i += 3;
            }
            if self.cells.len() >= MAX_BYTES {
                return Err(self.error(
                    ParseErrorKind::TooManyBytes { limit: MAX_BYTES },
                    cell_start..start + i,
                ));
            }
            self.cells.push((value, mask));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
