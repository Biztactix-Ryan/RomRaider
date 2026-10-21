//! Byte offsets to 1-based `line:col` [`Span`]s.

use rr_core::diag::Span;

/// Turns byte offsets into the decoded text into `line:col` spans.
///
/// Lines end at `\n`, `\r\n` or a lone `\r` (XML 1.0 §2.11), so a CRLF file
/// numbers its lines as its LF twin does. Columns count characters, as
/// [`Span::LineCol`] requires, not bytes.
///
/// The reader asks for offsets in (nearly) increasing order, so the locator
/// keeps a cursor and walks forward from it: locating every element and
/// attribute of a file costs one pass over the file. An offset behind the
/// cursor restarts the walk from the beginning, which is correct, just
/// slower; only a diagnostic does that.
#[derive(Debug, Clone)]
pub(crate) struct Locator<'a> {
    text: &'a str,
    /// Byte offset the line and column below describe.
    offset: usize,
    line: u32,
    col: u32,
    /// The byte before `offset` is a `\r`, so a `\n` at `offset` ends no
    /// further line.
    after_cr: bool,
}

impl<'a> Locator<'a> {
    pub(crate) const fn new(text: &'a str) -> Self {
        Self {
            text,
            offset: 0,
            line: 1,
            col: 1,
            after_cr: false,
        }
    }

    /// The position of byte `offset`. An offset past the end is the end; one
    /// inside a multi-byte character is that character.
    pub(crate) fn span(&mut self, offset: usize) -> Span {
        let mut target = offset.min(self.text.len());
        while !self.text.is_char_boundary(target) {
            target -= 1;
        }
        if target < self.offset {
            *self = Self::new(self.text);
        }
        let bytes = self.text.as_bytes();
        for &b in bytes.get(self.offset..target).unwrap_or_default() {
            match b {
                b'\r' => {
                    self.line = self.line.saturating_add(1);
                    self.col = 1;
                    self.after_cr = true;
                }
                b'\n' => {
                    if !self.after_cr {
                        self.line = self.line.saturating_add(1);
                    }
                    self.col = 1;
                    self.after_cr = false;
                }
                _ => {
                    // Count a character at its first byte, not at its
                    // continuation bytes (10xxxxxx).
                    if b & 0xC0 != 0x80 {
                        self.col = self.col.saturating_add(1);
                    }
                    self.after_cr = false;
                }
            }
        }
        // A character's first byte bumped `col` past it; continuation bytes
        // of the character at `target` were never walked, so `col` is right.
        self.offset = target;
        Span::line_col(self.line, self.col)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, offset: usize) -> (u32, u32) {
        match Locator::new(text).span(offset) {
            Span::LineCol { line, col } => (line, col),
            Span::ByteOffset(_) => unreachable!(),
        }
    }

    #[test]
    fn lf_crlf_and_cr_lines_number_alike() {
        for text in ["ab\ncd\nef", "ab\r\ncd\r\nef", "ab\rcd\ref"] {
            let e = text.find('e').unwrap_or_default();
            assert_eq!(at(text, 0), (1, 1), "{text:?}");
            assert_eq!(at(text, 1), (1, 2), "{text:?}");
            assert_eq!(at(text, e), (3, 1), "{text:?}");
            assert_eq!(at(text, e + 1), (3, 2), "{text:?}");
        }
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        let text = "<a b=\"°C\" c=\"x\"/>";
        let c = text.find('c').unwrap_or_default();
        // `<a b="°C" ` is 10 characters but 11 bytes.
        assert_eq!(at(text, c), (1, 11));
        // Inside the two-byte `°`: that character's column.
        let deg = text.find('°').unwrap_or_default();
        assert_eq!(at(text, deg + 1), (1, 7));
    }

    #[test]
    fn walking_back_restarts_and_past_the_end_clamps() {
        let text = "a\nb\nc";
        let mut loc = Locator::new(text);
        assert_eq!(loc.span(4), Span::line_col(3, 1));
        assert_eq!(loc.span(2), Span::line_col(2, 1));
        assert_eq!(loc.span(99), Span::line_col(3, 2));
        // A CR at the cursor, then the LF after it: still one line break.
        let mut loc = Locator::new("a\r\nb");
        assert_eq!(loc.span(2), Span::line_col(2, 1));
        assert_eq!(loc.span(3), Span::line_col(2, 1));
        assert_eq!(loc.span(4), Span::line_col(2, 2));
    }
}
