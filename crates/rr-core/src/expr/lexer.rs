//! Tokens (US-RR-20-6).
//!
//! The lexer reads one token at a time, on demand, from the formula text; it
//! keeps only a byte position. Token rules, against the JEP 2.x grammar's
//! token names (`INTEGER_LITERAL`, `FLOATING_POINT_LITERAL`, `EXPONENT`,
//! `INDENTIFIER1`, in `org/nfunk/jep/ParserConstants.class` of
//! `legacy/lib/common/jep.jar`):
//!
//! * whitespace is `' '`, `'\t'`, `'\n'` and `'\r'`, as in JEP;
//! * a number is `digits [. digits] [exponent]` or `. digits [exponent]`,
//!   with `exponent = (e|E) [+|-] digits`: `12`, `1.`, `.5`, `1.5e-3`, `2E4`.
//!   There is no sign (that is a prefix operator) and no hexadecimal: legacy
//!   documents that `0x18` fails to parse (`util/BitWise.java:53-54`);
//! * an identifier is an ASCII letter or `_` followed by ASCII letters,
//!   digits and `_`. JEP also allows `.` and some non-ASCII letters; no
//!   formula in the definition corpus uses them;
//! * `[id:unit]` is one token, a parameter reference
//!   (`logger/ecu/definition/EcuDerivedParameterConvertorImpl.java:109-114`:
//!   the unit runs to the first `]`). JEP's `[...]` list syntax is not supported.
//!
//! A number, name or reference longer than the literal limit
//! ([`Limits::max_literal_len`](super::Limits::max_literal_len)) is an error
//! once it has been scanned, before its text is converted or copied.

use super::ast::ByteSpan;
use super::error::{ParseError, ParseErrorKind, RefProblem};

/// How a number, a name and a reference are named in error messages.
const NUMBER: &str = "a number";
const NAME: &str = "a name";
const REF: &str = "a parameter reference";

/// A token kind. Identifiers and references keep only spans; the text is
/// read back from the formula.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Tok {
    /// A number literal, already converted.
    Num(f64),
    /// An identifier.
    Ident,
    /// `[id:unit]`, with the spans of `id` and `unit`.
    Ref {
        id: ByteSpan,
        unit: ByteSpan,
    },
    LParen,
    RParen,
    Comma,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Bang,
    AndAnd,
    OrOr,
    Amp,
    Pipe,
    EqEq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    Shl,
    Shr,
    /// End of the formula.
    Eof,
}

impl Tok {
    /// How the token is named in an error message.
    pub(crate) const fn describe(self) -> &'static str {
        match self {
            Self::Num(_) => NUMBER,
            Self::Ident => NAME,
            Self::Ref { .. } => REF,
            Self::LParen => "`(`",
            Self::RParen => "`)`",
            Self::Comma => "`,`",
            Self::Plus => "`+`",
            Self::Minus => "`-`",
            Self::Star => "`*`",
            Self::Slash => "`/`",
            Self::Percent => "`%`",
            Self::Caret => "`^`",
            Self::Bang => "`!`",
            Self::AndAnd => "`&&`",
            Self::OrOr => "`||`",
            Self::Amp => "`&`",
            Self::Pipe => "`|`",
            Self::EqEq => "`==`",
            Self::Ne => "`!=`",
            Self::Lt => "`<`",
            Self::Gt => "`>`",
            Self::Le => "`<=`",
            Self::Ge => "`>=`",
            Self::Shl => "`<<`",
            Self::Shr => "`>>`",
            Self::Eof => "the end of the formula",
        }
    }

    /// `true` for a token that can begin an operand. After a complete
    /// operand, such a token means an operator is missing.
    pub(crate) const fn starts_operand(self) -> bool {
        matches!(
            self,
            Self::Num(_)
                | Self::Ident
                | Self::Ref { .. }
                | Self::LParen
                | Self::Plus
                | Self::Minus
                | Self::Bang
        )
    }
}

/// A token and where it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Token {
    pub(crate) tok: Tok,
    pub(crate) span: ByteSpan,
}

/// Reads tokens from a formula no longer than `u32::MAX` bytes.
pub(crate) struct Lexer<'s> {
    src: &'s str,
    pos: usize,
    /// The literal length limit.
    max_literal: u32,
}

const fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

const fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

impl<'s> Lexer<'s> {
    /// A lexer over `src`, which the caller has checked fits in `u32`,
    /// refusing literals longer than `max_literal` bytes.
    pub(crate) const fn new(src: &'s str, max_literal: u32) -> Self {
        Self {
            src,
            pos: 0,
            max_literal,
        }
    }

    /// Refuses the literal `start..end` (a `token` such as "a number") if it
    /// is over the literal length limit.
    fn check_literal(
        &self,
        token: &'static str,
        start: usize,
        end: usize,
    ) -> Result<(), ParseError> {
        let len = end - start;
        if len > self.max_literal as usize {
            let kind = ParseErrorKind::LiteralTooLong {
                token,
                len,
                limit: self.max_literal,
            };
            return Err(Self::error(kind, start, end));
        }
        Ok(())
    }

    fn byte(&self, at: usize) -> Option<u8> {
        self.src.as_bytes().get(at).copied()
    }

    /// `start..end` as a span. Offsets never exceed the formula length,
    /// which the parser checked fits in `u32`.
    fn span(start: usize, end: usize) -> ByteSpan {
        let s = u32::try_from(start).unwrap_or(u32::MAX);
        let e = u32::try_from(end).unwrap_or(u32::MAX);
        ByteSpan::new(s, e)
    }

    fn token(&mut self, tok: Tok, start: usize, len: usize) -> Token {
        self.pos = start + len;
        Token {
            tok,
            span: Self::span(start, self.pos),
        }
    }

    fn error(kind: ParseErrorKind, start: usize, end: usize) -> ParseError {
        ParseError::new(kind, Self::span(start, end))
    }

    /// Advances past every byte from `at` for which `pred` holds; returns the
    /// first position where it does not. (Counted with an iterator rather
    /// than a loop, so no mutation of it can spin forever.)
    fn skip_while(&self, at: usize, pred: impl Fn(u8) -> bool) -> usize {
        let rest = self.src.as_bytes().get(at..).unwrap_or(&[]);
        at + rest.iter().take_while(|&&b| pred(b)).count()
    }

    /// The next token.
    pub(crate) fn next_token(&mut self) -> Result<Token, ParseError> {
        let start = self.skip_while(self.pos, |b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'));
        let Some(b) = self.byte(start) else {
            return Ok(self.token(Tok::Eof, start, 0));
        };
        let next = self.byte(start + 1);
        let digit_follows = next.is_some_and(|n| n.is_ascii_digit());
        if b.is_ascii_digit() || (b == b'.' && digit_follows) {
            return self.number(start);
        }
        if is_ident_start(b) {
            // An identifier's first byte is also an identifier byte.
            let end = self.skip_while(start, is_ident_char);
            self.check_literal(NAME, start, end)?;
            return Ok(self.token(Tok::Ident, start, end - start));
        }
        let (tok, len) = match (b, next) {
            (b'&', Some(b'&')) => (Tok::AndAnd, 2),
            (b'|', Some(b'|')) => (Tok::OrOr, 2),
            (b'=', Some(b'=')) => (Tok::EqEq, 2),
            (b'!', Some(b'=')) => (Tok::Ne, 2),
            (b'<', Some(b'=')) => (Tok::Le, 2),
            (b'>', Some(b'=')) => (Tok::Ge, 2),
            (b'<', Some(b'<')) => (Tok::Shl, 2),
            (b'>', Some(b'>')) => (Tok::Shr, 2),
            (b'(', _) => (Tok::LParen, 1),
            (b')', _) => (Tok::RParen, 1),
            (b',', _) => (Tok::Comma, 1),
            (b'+', _) => (Tok::Plus, 1),
            (b'-', _) => (Tok::Minus, 1),
            (b'*', _) => (Tok::Star, 1),
            (b'/', _) => (Tok::Slash, 1),
            (b'%', _) => (Tok::Percent, 1),
            (b'^', _) => (Tok::Caret, 1),
            (b'!', _) => (Tok::Bang, 1),
            (b'&', _) => (Tok::Amp, 1),
            (b'|', _) => (Tok::Pipe, 1),
            (b'<', _) => (Tok::Lt, 1),
            (b'>', _) => (Tok::Gt, 1),
            (b'[', _) => return self.reference(start),
            (b'=', _) => return Err(Self::error(ParseErrorKind::Assignment, start, start + 1)),
            _ => return Err(self.unexpected_char(start)),
        };
        Ok(self.token(tok, start, len))
    }

    /// The (possibly multi-byte) character at byte `at`, a char boundary.
    fn char_at(&self, at: usize) -> char {
        self.src
            .get(at..)
            .and_then(|rest| rest.chars().next())
            .unwrap_or(char::REPLACEMENT_CHARACTER)
    }

    /// An error for the character at `at`.
    fn unexpected_char(&self, at: usize) -> ParseError {
        let c = self.char_at(at);
        Self::error(ParseErrorKind::UnexpectedChar(c), at, at + c.len_utf8())
    }

    /// A number literal starting at `start` (a digit, or `.` then a digit).
    fn number(&mut self, start: usize) -> Result<Token, ParseError> {
        let is_digit = |b: u8| b.is_ascii_digit();
        let mut end = self.skip_while(start, is_digit);
        if self.byte(end) == Some(b'.') {
            end = self.skip_while(end + 1, is_digit);
        }
        if matches!(self.byte(end), Some(b'e' | b'E')) {
            // Only a complete exponent belongs to the number: in `2e` or
            // `2e+` the `e` starts a name, as in JEP's token rules.
            let sign = usize::from(matches!(self.byte(end + 1), Some(b'+' | b'-')));
            let digits = end + 1 + sign;
            if self.byte(digits).is_some_and(is_digit) {
                end = self.skip_while(digits, is_digit);
            }
        }
        self.check_literal(NUMBER, start, end)?;
        let text = self.src.get(start..end).unwrap_or("");
        match text.parse::<f64>() {
            Ok(v) if v.is_finite() => Ok(self.token(Tok::Num(v), start, end - start)),
            // Every literal the rules above accept is valid `f64` syntax, so
            // only an overflow to infinity can get here.
            _ => Err(Self::error(ParseErrorKind::NumberOutOfRange, start, end)),
        }
    }

    /// A `[id:unit]` reference starting at `start` (the `[`).
    fn reference(&mut self, start: usize) -> Result<Token, ParseError> {
        let bad = |problem, from, to| Self::error(ParseErrorKind::BadRef(problem), from, to);
        let len = self.src.len();
        let id_start = start + 1;
        let id_end = self.skip_while(id_start, is_ident_char);
        match self.byte(id_end) {
            None => return Err(bad(RefProblem::Unterminated, start, len)),
            Some(b':' | b']') if id_end == id_start => {
                return Err(bad(RefProblem::EmptyId, start, id_end + 1));
            }
            Some(b':') => {}
            Some(b']') => return Err(bad(RefProblem::MissingUnit, start, id_end + 1)),
            Some(_) => {
                let c = self.char_at(id_end);
                return Err(bad(RefProblem::BadIdChar(c), id_end, id_end + c.len_utf8()));
            }
        }
        let unit_start = id_end + 1;
        let rest = self.src.get(unit_start..).unwrap_or("");
        let mut unit_end = None;
        for (i, c) in rest.char_indices() {
            if c == ']' {
                unit_end = Some(unit_start + i);
                break;
            }
            if c == '[' || c.is_control() {
                let at = unit_start + i;
                return Err(bad(RefProblem::BadUnitChar(c), at, at + c.len_utf8()));
            }
        }
        let Some(unit_end) = unit_end else {
            return Err(bad(RefProblem::Unterminated, start, len));
        };
        if unit_end == unit_start {
            return Err(bad(RefProblem::EmptyUnit, start, unit_end + 1));
        }
        let end = unit_end + 1;
        self.check_literal(REF, start, end)?;
        let tok = Tok::Ref {
            id: Self::span(id_start, id_end),
            unit: Self::span(unit_start, unit_end),
        };
        Ok(self.token(tok, start, end - start))
    }
}
