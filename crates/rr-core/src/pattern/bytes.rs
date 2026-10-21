//! Byte patterns: bytes under a mask at an offset.

use super::{Case, Mismatch, PatternError};

/// Bytes under a mask at an offset: `80 F0 ?? 01` at `0x2000`.
///
/// Byte `i` of the pattern matches input byte `offset + i` when
/// `input & mask[i] == value[i]`. A mask byte of `0xFF` checks the whole
/// byte, `0x00` is a wildcard (`??`), anything else checks only its set
/// bits. Bits of the value outside the mask are don't-care and are cleared
/// at compile time, so [`value`](Self::value) is always pre-masked.
///
/// Built only through the constructors, which refuse an empty pattern, a
/// mask of the wrong length and an offset whose end overflows `usize`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BytePattern {
    offset: usize,
    /// `bytes & mask`, so a match is `input & mask == value`.
    value: Box<[u8]>,
    mask: Box<[u8]>,
}

impl BytePattern {
    /// Compiles `bytes` under `mask` at `offset`.
    ///
    /// # Errors
    ///
    /// * [`PatternError::EmptyBytes`] for an empty `bytes`;
    /// * [`PatternError::MaskLength`] when `mask` is not as long as `bytes`;
    /// * [`PatternError::OffsetOverflow`] when `offset + bytes.len()`
    ///   overflows `usize`.
    pub fn new(offset: usize, bytes: &[u8], mask: &[u8]) -> Result<Self, PatternError> {
        if bytes.len() != mask.len() {
            return Err(PatternError::MaskLength {
                bytes: bytes.len(),
                mask: mask.len(),
            });
        }
        Self::build(offset, bytes.iter().zip(mask).map(|(&b, &m)| (b, m)))
    }

    /// Compiles `bytes` at `offset` with every bit checked.
    ///
    /// # Errors
    ///
    /// As [`new`](Self::new).
    pub fn exact(offset: usize, bytes: &[u8]) -> Result<Self, PatternError> {
        Self::build(offset, bytes.iter().map(|&b| (b, 0xFF)))
    }

    /// Compiles one cell per byte at `offset`: `Some(b)` checks the whole
    /// byte, `None` is a wildcard.
    ///
    /// # Errors
    ///
    /// As [`new`](Self::new).
    pub fn from_cells(offset: usize, cells: &[Option<u8>]) -> Result<Self, PatternError> {
        Self::build(
            offset,
            cells.iter().map(|cell| match *cell {
                Some(b) => (b, 0xFF),
                None => (0, 0),
            }),
        )
    }

    /// Compiles the bytes of `text` at `offset`, as legacy matched a textual
    /// `internalidstring` (`maps/RomID.java:82-86`).
    ///
    /// With [`Case::AsciiInsensitive`] (legacy's `equalsIgnoreCase`) each
    /// ASCII letter is checked under the mask `0xDF`, which accepts exactly
    /// its upper- and lower-case forms; every other byte is checked whole.
    ///
    /// # Errors
    ///
    /// As [`new`](Self::new).
    pub fn text(offset: usize, text: &str, case: Case) -> Result<Self, PatternError> {
        Self::build(
            offset,
            text.bytes().map(|b| match case {
                Case::AsciiInsensitive if b.is_ascii_alphabetic() => (b, 0xDF),
                _ => (b, 0xFF),
            }),
        )
    }

    pub(super) fn build(
        offset: usize,
        cells: impl ExactSizeIterator<Item = (u8, u8)>,
    ) -> Result<Self, PatternError> {
        let len = cells.len();
        if len == 0 {
            return Err(PatternError::EmptyBytes);
        }
        if offset.checked_add(len).is_none() {
            return Err(PatternError::OffsetOverflow { offset, len });
        }
        let (value, mask): (Vec<u8>, Vec<u8>) = cells.map(|(b, m)| (b & m, m)).unzip();
        Ok(Self {
            offset,
            value: value.into_boxed_slice(),
            mask: mask.into_boxed_slice(),
        })
    }

    /// Offset of the first byte, from the start of the input (or from the
    /// `base` of [`check_at`](Self::check_at)).
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// Length in bytes; never zero.
    #[must_use]
    #[expect(clippy::len_without_is_empty, reason = "a byte pattern is never empty")]
    pub const fn len(&self) -> usize {
        self.value.len()
    }

    /// The expected bytes, already masked.
    #[must_use]
    pub const fn value(&self) -> &[u8] {
        &self.value
    }

    /// The mask: the bits checked in each byte.
    #[must_use]
    pub const fn mask(&self) -> &[u8] {
        &self.mask
    }

    /// Whether every bit is checked (no wildcard and no partial mask).
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.mask.iter().all(|&m| m == 0xFF)
    }

    /// Whether `input` matches at the pattern's offset.
    #[must_use]
    pub fn matches(&self, input: &[u8]) -> bool {
        self.matches_at(input, 0)
    }

    /// Whether `input` matches with the pattern placed at `base` (its bytes
    /// start at `base + offset`). Never panics, whatever `base`.
    #[must_use]
    pub fn matches_at(&self, input: &[u8], base: usize) -> bool {
        let Some(window) = self.window(input, base) else {
            return false;
        };
        window
            .iter()
            .zip(&*self.value)
            .zip(&*self.mask)
            .all(|((&b, &v), &m)| b & m == v)
    }

    /// Matches `input` at the pattern's offset.
    ///
    /// # Errors
    ///
    /// As [`check_at`](Self::check_at) with `base` `0`.
    pub fn check(&self, input: &[u8]) -> Result<(), Mismatch> {
        self.check_at(input, 0)
    }

    /// Matches `input` with the pattern placed at `base`.
    ///
    /// # Errors
    ///
    /// * [`Mismatch::OutOfBounds`] when the pattern's bytes do not all lie
    ///   inside `input` (including when `base + offset + len` overflows);
    /// * [`Mismatch::ByteDiffers`] for the first byte that differs in a
    ///   checked bit.
    pub fn check_at(&self, input: &[u8], base: usize) -> Result<(), Mismatch> {
        let window = self.window(input, base).ok_or(Mismatch::OutOfBounds {
            base,
            offset: self.offset,
            len: self.len(),
            input_len: input.len(),
        })?;
        let first_diff = window
            .iter()
            .zip(&*self.value)
            .zip(&*self.mask)
            .enumerate()
            .find(|&(_, ((&b, &v), &m))| b & m != v);
        match first_diff {
            None => Ok(()),
            Some((i, ((&found, &expected), &mask))) => Err(Mismatch::ByteDiffers {
                // `window` is `input[base + offset ..][.. len]` and `i < len`,
                // so this sum is at most `input.len()`: it cannot overflow.
                position: base + self.offset + i,
                expected,
                mask,
                found,
            }),
        }
    }

    /// The first `base` at which `input` matches
    /// ([`matches_at`](Self::matches_at)), scanning from `0`: where a frame
    /// sync header starts, less the pattern's offset.
    #[must_use]
    pub fn find(&self, input: &[u8]) -> Option<usize> {
        // Construction guarantees `offset + len` does not overflow.
        let span = self.offset.checked_add(self.len())?;
        let last = input.len().checked_sub(span)?;
        (0..=last).find(|&base| self.matches_at(input, base))
    }

    /// The input bytes the pattern covers at `base`, or `None` when they do
    /// not all lie inside `input`.
    fn window<'a>(&self, input: &'a [u8], base: usize) -> Option<&'a [u8]> {
        let start = base.checked_add(self.offset)?;
        let end = start.checked_add(self.len())?;
        input.get(start..end)
    }
}
