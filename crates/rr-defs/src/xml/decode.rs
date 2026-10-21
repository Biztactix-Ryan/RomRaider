//! Turning the input bytes into text the reader can parse (US-RR-27-6).
//!
//! The reader works on `&str`. Decoding is the one step that sees raw
//! bytes: it decides the encoding, then transcodes into UTF-8 when the file
//! is not already UTF-8.
//!
//! # Detection
//!
//! As XML 1.0 appendix F describes, and as legacy's Java parser did:
//!
//! 1. **Byte-order mark.** A UTF-8 BOM (`EF BB BF`) means UTF-8, and is
//!    dropped (it is not content, and an editor shows no column for it).
//!    A UTF-16 or UTF-32 BOM, or the bytes of `<?` in UTF-16 or UTF-32
//!    without one, are refused as unsupported (`XML0002`): no definition
//!    file has ever been written so, and accepting them would widen what
//!    hostile input can reach for no real file.
//! 2. **XML declaration.** Otherwise `encoding="..."` in a leading
//!    `<?xml ...?>` names the encoding, case-insensitively: UTF-8,
//!    ISO-8859-1 (also `latin1`, `Latin-1` and the other IANA aliases) or
//!    US-ASCII. Any other name is refused (`XML0002`), naming it. Legacy
//!    wrote its own user profiles and settings in ISO-8859-1; the pinned
//!    definition corpus is all UTF-8 (89 of its files with a BOM), and no
//!    file in it declares windows-1252, so that is refused too rather than
//!    guessed at.
//! 3. **Neither.** UTF-8.
//!
//! A UTF-8 BOM together with a declaration naming another encoding
//! contradicts itself (`XML0012`, a warning): the BOM wins and the file is
//! read as UTF-8. The three bytes of a BOM are a near-certain sign of
//! UTF-8, typically an editor that re-saved the file as UTF-8 without
//! touching the declaration's text; the warning says so, and bytes that
//! are not UTF-8 after all are still refused (`XML0002`).
//!
//! Bytes that are not valid in the chosen encoding (UTF-8, or US-ASCII
//! above `0x7F`) are refused at the first bad byte (`XML0002`), as legacy's
//! parser refused them; ISO-8859-1 maps every byte to a character, so
//! nothing in it is invalid.
//!
//! # Positions
//!
//! Every `line:col` the reader reports is computed on the decoded text,
//! and refers to the original file all the same: lines are numbered by the
//! same CR / LF / CRLF breaks (decoding never touches them; the reader
//! normalises line ends in values, not in the text it locates), and columns
//! count characters, which transcoding preserves one for one (an ISO-8859-1
//! byte is one character, so `°` is one column in either encoding). Only the
//! BOM is dropped, and it occupies no column.
//!
//! # Allocation
//!
//! UTF-8 input is borrowed, not copied. ISO-8859-1 is transcoded into a
//! string allocated once at its exact final size: one byte per ASCII byte
//! and two per other byte, so at most twice the input, which the input
//! limit has already bounded.

use std::borrow::Cow;

use rr_core::diag::{Code, Span, codes};

use super::locate::Locator;

/// The UTF-8 encoding of U+FEFF, the byte-order mark.
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// How far into the file the XML declaration is looked for. A declaration
/// is a few dozen bytes; one that does not end within this many is not
/// examined for an encoding (quick-xml then reads it as written).
const MAX_DECL_LEN: usize = 1024;

/// An encoding the reader decodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Encoding {
    Utf8,
    Latin1,
    Ascii,
}

impl Encoding {
    /// The encoding called `name` in an XML declaration (case-insensitive;
    /// the IANA name and its registered aliases), if it is supported.
    fn named(name: &str) -> Option<Self> {
        const UTF8: &[&str] = &["utf-8"];
        const LATIN1: &[&str] = &[
            "iso-8859-1",
            "iso8859-1",
            "iso_8859-1",
            "iso_8859-1:1987",
            "latin1",
            "latin-1",
            "l1",
            "iso-ir-100",
            "cp819",
            "ibm819",
            "csisolatin1",
        ];
        const ASCII: &[&str] = &[
            "us-ascii",
            "ascii",
            "iso646-us",
            "ansi_x3.4-1968",
            "us",
            "cp367",
            "ibm367",
            "csascii",
        ];
        let is = |names: &[&str]| names.iter().any(|n| n.eq_ignore_ascii_case(name));
        if is(UTF8) {
            Some(Self::Utf8)
        } else if is(LATIN1) {
            Some(Self::Latin1)
        } else if is(ASCII) {
            Some(Self::Ascii)
        } else {
            None
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Latin1 => "ISO-8859-1",
            Self::Ascii => "US-ASCII",
        }
    }
}

/// A diagnostic decoding found, at a position in the original file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Problem {
    pub(crate) code: Code,
    pub(crate) span: Span,
    pub(crate) message: String,
}

/// Input that decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Decoded<'a> {
    /// The text, without a BOM.
    pub(crate) text: Cow<'a, str>,
    /// The encoding it was read in.
    pub(crate) encoding: Encoding,
    /// A warning to report (a BOM contradicting the declaration).
    pub(crate) warning: Option<Problem>,
}

/// `input` as text, in the encoding its BOM or XML declaration names; an
/// error [`Problem`] when it is not text in a supported encoding.
pub(crate) fn decode(input: &[u8]) -> Result<Decoded<'_>, Problem> {
    if let Some(name) = unsupported_bom(input) {
        return Err(Problem {
            code: codes::XML_ENCODING,
            span: Span::line_col(1, 1),
            message: format!(
                "the file is {name}, an unsupported encoding; definition files are read \
                 in UTF-8, ISO-8859-1 or US-ASCII (re-save it as UTF-8)"
            ),
        });
    }
    let (body, bom) = match input.strip_prefix(UTF8_BOM) {
        Some(body) => (body, true),
        None => (input, false),
    };
    let declared = declared_encoding(body)?;
    let mut warning = None;
    let encoding = match (bom, declared) {
        (_, None) => Encoding::Utf8,
        (true, Some(decl)) => {
            if Encoding::named(decl.name) != Some(Encoding::Utf8) {
                warning = Some(Problem {
                    code: codes::XML_ENCODING_MISMATCH,
                    span: decl.span,
                    message: format!(
                        "the file starts with a UTF-8 byte-order mark but its XML declaration \
                         says encoding=\"{}\"; it is read as UTF-8, as the byte-order mark says",
                        decl.name
                    ),
                });
            }
            Encoding::Utf8
        }
        (false, Some(decl)) => match Encoding::named(decl.name) {
            Some(encoding) => encoding,
            None => {
                return Err(Problem {
                    code: codes::XML_ENCODING,
                    span: decl.span,
                    message: format!(
                        "the XML declaration names encoding \"{}\", which is not supported; \
                         definition files are read in UTF-8, ISO-8859-1 or US-ASCII",
                        decl.name
                    ),
                });
            }
        },
    };
    let text = match encoding {
        Encoding::Utf8 => utf8(body, bom, declared.is_some())?,
        Encoding::Ascii => ascii(body)?,
        Encoding::Latin1 => Cow::Owned(latin1(body)),
    };
    Ok(Decoded {
        text,
        encoding,
        warning,
    })
}

/// The name of the unsupported encoding `input` starts in, judged by a
/// byte-order mark or by how `<?` is encoded (XML 1.0 appendix F).
fn unsupported_bom(input: &[u8]) -> Option<&'static str> {
    const SIGNATURES: &[(&[u8], &str)] = &[
        (b"\x00\x00\xFE\xFF", "UTF-32 (big-endian byte-order mark)"),
        (
            b"\xFF\xFE\x00\x00",
            "UTF-32 (little-endian byte-order mark)",
        ),
        (b"\xFE\xFF", "UTF-16 (big-endian byte-order mark)"),
        (b"\xFF\xFE", "UTF-16 (little-endian byte-order mark)"),
        (
            b"\x00\x00\x00\x3C",
            "UTF-32 (big-endian, without a byte-order mark)",
        ),
        (
            b"\x3C\x00\x00\x00",
            "UTF-32 (little-endian, without a byte-order mark)",
        ),
        (
            b"\x00\x3C\x00\x3F",
            "UTF-16 (big-endian, without a byte-order mark)",
        ),
        (
            b"\x3C\x00\x3F\x00",
            "UTF-16 (little-endian, without a byte-order mark)",
        ),
    ];
    SIGNATURES
        .iter()
        .find(|(signature, _)| input.starts_with(signature))
        .map(|&(_, name)| name)
}

/// The `encoding` pseudo-attribute of an XML declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Declared<'a> {
    name: &'a str,
    /// Where the name starts.
    span: Span,
}

/// The encoding named by the XML declaration `body` starts with, if it has
/// one and names an encoding. A declaration that does not end within
/// [`MAX_DECL_LEN`] bytes, or holds anything but ASCII before its end, is
/// not examined (quick-xml reads it as written). A declaration whose
/// pseudo-attributes are not well-formed is refused (`XML0001`), so an
/// encoding it meant to name is never silently ignored.
fn declared_encoding(body: &[u8]) -> Result<Option<Declared<'_>>, Problem> {
    let Some(rest) = body.strip_prefix(b"<?xml") else {
        return Ok(None);
    };
    if !rest.first().copied().is_some_and(is_space) {
        // `<?xml-stylesheet ...?>` and the like are processing instructions.
        return Ok(None);
    }
    let window = body.get(..MAX_DECL_LEN).unwrap_or(body);
    let Some(end) = window.windows(2).position(|w| w == b"?>") else {
        return Ok(None);
    };
    let Some(decl) = window.get(..end).filter(|d| d.is_ascii()) else {
        return Ok(None);
    };
    // ASCII, so a `str`, and byte offsets in it are character offsets.
    let Ok(decl) = std::str::from_utf8(decl) else {
        return Ok(None);
    };
    let span = |at: usize| Locator::new(decl).span(at);
    let malformed = |at: usize, what: &str| Problem {
        code: codes::XML_MALFORMED,
        span: span(at),
        message: format!("the XML declaration is not well-formed: {what}"),
    };
    let bytes = decl.as_bytes();
    let mut at = "<?xml".len();
    loop {
        let start = at;
        while bytes.get(at).copied().is_some_and(is_space) {
            at += 1;
        }
        if at == decl.len() {
            return Ok(None);
        }
        if at == start {
            return Err(malformed(
                at,
                "expected a space before the next pseudo-attribute",
            ));
        }
        let name_start = at;
        while bytes.get(at).is_some_and(|&b| is_name_byte(b)) {
            at += 1;
        }
        if at == name_start {
            return Err(malformed(at, "expected a pseudo-attribute name"));
        }
        let name = decl.get(name_start..at).unwrap_or_default();
        while bytes.get(at).copied().is_some_and(is_space) {
            at += 1;
        }
        if bytes.get(at) != Some(&b'=') {
            return Err(malformed(at, &format!("expected `=` after `{name}`")));
        }
        at += 1;
        while bytes.get(at).copied().is_some_and(is_space) {
            at += 1;
        }
        let quote = match bytes.get(at) {
            Some(&q @ (b'"' | b'\'')) => q,
            _ => {
                return Err(malformed(
                    at,
                    &format!("the value of `{name}` is not quoted"),
                ));
            }
        };
        at += 1;
        let value_start = at;
        let Some(len) = bytes
            .get(value_start..)
            .and_then(|v| v.iter().position(|&b| b == quote))
        else {
            return Err(malformed(
                value_start.saturating_sub(1),
                &format!("the value of `{name}` is not closed"),
            ));
        };
        at = value_start + len + 1;
        if name == "encoding" {
            let value = decl.get(value_start..value_start + len).unwrap_or_default();
            return Ok(Some(Declared {
                name: value,
                span: span(value_start),
            }));
        }
    }
}

/// `body` as UTF-8, refused at its first invalid byte.
fn utf8(body: &[u8], bom: bool, declared: bool) -> Result<Cow<'_, str>, Problem> {
    match std::str::from_utf8(body) {
        Ok(text) => Ok(Cow::Borrowed(text)),
        Err(e) => {
            let valid_len = e.valid_up_to();
            let bad = body.get(valid_len).copied().unwrap_or_default();
            let hint = if bom || declared {
                ""
            } else {
                "; if it is ISO-8859-1 (Latin-1), say so with \
                 `<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?>` as its first line"
            };
            Err(invalid_byte(body, valid_len, bad, Encoding::Utf8, hint))
        }
    }
}

/// `body` as US-ASCII, refused at its first byte above `0x7F`.
fn ascii(body: &[u8]) -> Result<Cow<'_, str>, Problem> {
    match body.iter().position(|b| !b.is_ascii()) {
        None => Ok(Cow::Borrowed(
            // All ASCII, so valid UTF-8.
            std::str::from_utf8(body).unwrap_or_default(),
        )),
        Some(at) => {
            let bad = body.get(at).copied().unwrap_or_default();
            Err(invalid_byte(body, at, bad, Encoding::Ascii, ""))
        }
    }
}

/// The error for byte `bad` at offset `at` of `body`, whose bytes before
/// it are valid UTF-8.
fn invalid_byte(body: &[u8], at: usize, bad: u8, encoding: Encoding, hint: &str) -> Problem {
    let valid = body
        .get(..at)
        .and_then(|v| std::str::from_utf8(v).ok())
        .unwrap_or_default();
    Problem {
        code: codes::XML_ENCODING,
        span: Locator::new(valid).span(valid.len()),
        message: format!(
            "the file is not valid {} (byte 0x{bad:02X}){hint}",
            encoding.label()
        ),
    }
}

/// `body` as ISO-8859-1: every byte is the character of the same number.
fn latin1(body: &[u8]) -> String {
    let high = body.iter().filter(|b| !b.is_ascii()).count();
    // Exact: one byte per ASCII byte, two per other byte; at most twice the
    // input, which the input limit bounds.
    let mut text = String::with_capacity(body.len().saturating_add(high));
    text.extend(body.iter().map(|&b| char::from(b)));
    text
}

/// XML's `S`: space, tab, CR, LF.
const fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

/// A byte of a pseudo-attribute name (`version`, `encoding`, `standalone`).
const fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(input: &[u8]) -> (String, Encoding) {
        match decode(input) {
            Ok(d) => {
                assert_eq!(d.warning, None, "{input:?}");
                (d.text.into_owned(), d.encoding)
            }
            Err(e) => unreachable!("{input:?}: {e:?}"),
        }
    }

    fn refused(input: &[u8]) -> Problem {
        match decode(input) {
            Ok(d) => unreachable!("{input:?} decoded: {d:?}"),
            Err(e) => e,
        }
    }

    #[test]
    fn utf8_with_and_without_bom_decode_alike() {
        assert_eq!(text(b"<a/>"), ("<a/>".to_owned(), Encoding::Utf8));
        assert_eq!(
            text(b"\xEF\xBB\xBF<a/>"),
            ("<a/>".to_owned(), Encoding::Utf8)
        );
        assert_eq!(text(b""), (String::new(), Encoding::Utf8));
        assert!(matches!(
            decode(b"<a/>").map(|d| d.text),
            Ok(Cow::Borrowed(_))
        ));
    }

    #[test]
    fn the_declaration_names_the_encoding_case_insensitively() {
        for (decl, encoding) in [
            ("UTF-8", Encoding::Utf8),
            ("utf-8", Encoding::Utf8),
            ("ISO-8859-1", Encoding::Latin1),
            ("iso-8859-1", Encoding::Latin1),
            ("Latin-1", Encoding::Latin1),
            ("latin1", Encoding::Latin1),
            ("ISO_8859-1", Encoding::Latin1),
            ("US-ASCII", Encoding::Ascii),
            ("ascii", Encoding::Ascii),
        ] {
            for quote in ['"', '\''] {
                let xml =
                    format!("<?xml version=\"1.0\"\r\n  encoding = {quote}{decl}{quote} ?><a/>");
                assert_eq!(text(xml.as_bytes()).1, encoding, "{xml}");
            }
        }
        // No encoding pseudo-attribute, or no declaration: UTF-8.
        assert_eq!(
            text(b"<?xml version=\"1.0\" standalone=\"yes\"?><a/>").1,
            Encoding::Utf8
        );
        assert_eq!(text(b"<?xml-stylesheet href=\"a\"?><a/>").1, Encoding::Utf8);
        assert_eq!(text(b"<?xml version='1.0'").1, Encoding::Utf8);
    }

    #[test]
    fn latin1_transcodes_each_byte_to_one_character() {
        let input =
            b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a u=\"\xB0C\">\xB5s \xFF\x80</a>";
        let (decoded, encoding) = text(input);
        assert_eq!(encoding, Encoding::Latin1);
        assert!(decoded.ends_with("<a u=\"°C\">µs ÿ\u{80}</a>"), "{decoded}");
        assert_eq!(decoded.chars().count(), input.len());
        // Allocated once, at exactly the size needed.
        let body: Vec<u8> = (0..=255).collect();
        let s = latin1(&body);
        assert_eq!(s.len(), 128 + 2 * 128);
        assert!(s.capacity() <= 2 * body.len());
    }

    #[test]
    fn unsupported_encodings_are_refused_by_name() {
        let e = refused(b"<?xml version=\"1.0\" encoding=\"windows-1252\"?><a/>");
        assert_eq!(e.code, codes::XML_ENCODING);
        assert_eq!(e.span, Span::line_col(1, 31));
        assert!(e.message.contains("\"windows-1252\""), "{}", e.message);
        let e = refused(b"<?xml version=\"1.0\"\n encoding='UTF-16'?><a/>");
        assert_eq!(e.span, Span::line_col(2, 12));
        assert!(e.message.contains("\"UTF-16\""), "{}", e.message);
    }

    #[test]
    fn utf16_and_utf32_are_refused_at_the_start() {
        for input in [
            &b"\xFF\xFE<\x00a\x00/\x00>\x00"[..],
            b"\xFE\xFF\x00<\x00a",
            b"<\x00?\x00x\x00m\x00l\x00",
            b"\x00<\x00?\x00x",
            b"\xFF\xFE\x00\x00<\x00\x00\x00",
            b"\x00\x00\xFE\xFF",
            b"\x00\x00\x00<",
            b"<\x00\x00\x00",
        ] {
            let e = refused(input);
            assert_eq!(
                (e.code, e.span),
                (codes::XML_ENCODING, Span::line_col(1, 1))
            );
            assert!(e.message.contains("UTF-"), "{}", e.message);
        }
    }

    #[test]
    fn a_bom_contradicting_the_declaration_wins_with_a_warning() {
        let d =
            decode(b"\xEF\xBB\xBF<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a>\xC2\xB0</a>");
        let d = d.unwrap_or_else(|e| unreachable!("{e:?}"));
        assert_eq!(d.encoding, Encoding::Utf8);
        assert!(d.text.ends_with("<a>°</a>"));
        let w = d.warning.unwrap_or_else(|| unreachable!());
        assert_eq!(
            (w.code, w.span),
            (codes::XML_ENCODING_MISMATCH, Span::line_col(1, 31))
        );
        // A BOM agreeing with the declaration is no news.
        assert_eq!(
            text(b"\xEF\xBB\xBF<?xml version=\"1.0\" encoding=\"utf-8\"?><a/>").1,
            Encoding::Utf8
        );
        // Still UTF-8 for the bytes, though.
        let e = refused(b"\xEF\xBB\xBF<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a>\xB0</a>");
        assert_eq!(
            (e.code, e.span),
            (codes::XML_ENCODING, Span::line_col(1, 47))
        );
    }

    #[test]
    fn invalid_bytes_are_refused_where_they_are() {
        let e = refused(b"<a>\n\xB0C</a>");
        assert_eq!(
            (e.code, e.span),
            (codes::XML_ENCODING, Span::line_col(2, 1))
        );
        assert!(e.message.contains("0xB0"), "{}", e.message);
        assert!(e.message.contains("ISO-8859-1"), "the hint: {}", e.message);
        let e = refused(b"\xEF\xBB\xBF\xFF");
        assert_eq!(e.span, Span::line_col(1, 1));
        assert!(
            !e.message.contains("encoding=\\\""),
            "no hint with a BOM: {}",
            e.message
        );
        // Columns before the bad byte count characters.
        let e = refused(b"<a b=\"\xC2\xB0\">\xB0</a>");
        assert_eq!(e.span, Span::line_col(1, 10));
        let e = refused(b"<?xml version=\"1.0\" encoding=\"US-ASCII\"?>\r\n<a>\xC2\xB0</a>");
        assert_eq!(e.span, Span::line_col(2, 4));
        assert!(e.message.contains("US-ASCII"), "{}", e.message);
    }

    #[test]
    fn malformed_declarations_are_refused() {
        for (input, at) in [
            (&b"<?xml version=\"1.0\" encoding=UTF-8?><a/>"[..], 30),
            (b"<?xml version=\"1.0\" encoding\"UTF-8\"?><a/>", 29),
            (b"<?xml version=\"1.0\"encoding=\"UTF-8\"?><a/>", 20),
            (b"<?xml version=\"1.0\" encoding=\"UTF-8?><a/>", 30),
            (b"<?xml version=\"1.0\" =\"UTF-8\"?><a/>", 21),
        ] {
            let e = refused(input);
            assert_eq!(
                (e.code, e.span),
                (codes::XML_MALFORMED, Span::line_col(1, at)),
                "{}: {}",
                String::from_utf8_lossy(input),
                e.message
            );
        }
    }
}
