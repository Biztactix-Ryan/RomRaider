//! Body of the `pattern` fuzz target (task US-RR-23-5): byte pattern text
//! from untrusted definitions and profiles through
//! `rr_core::pattern::BytePattern::parse`, then matched against untrusted
//! bytes.
//!
//! # Input layout
//!
//! `<pattern text> [ NUL <input bytes> ]`
//!
//! * **Text**: the bytes before the first NUL, as UTF-8. Invalid UTF-8 is
//!   cut at the first bad byte (`Utf8Error::valid_up_to`), so every error
//!   span is a byte range of what the parser saw. A seed is plain pattern
//!   text.
//! * **Input** (optional): the bytes after the NUL, matched against the
//!   parsed pattern. With no NUL the input is empty.
//!
//! # What must hold
//!
//! * A parse error's span is a range of the text on character boundaries,
//!   non-empty unless the text is empty; its column is the number of
//!   characters before it plus one; its code is the kind's, a `PAT` code;
//!   its diagnostic is an `Error` carrying code, message and column.
//!   `TooLong` is returned exactly when the text is over `MAX_TEXT_LEN`.
//! * A parsed pattern is non-empty, ends at or before `usize::MAX`, stores
//!   its value pre-masked, and prints (`Display`) as text that parses back
//!   to an equal pattern which prints the same.
//! * Matching never panics, at any base including `usize::MAX`: `matches_at`
//!   agrees with `check_at`, a match covers bytes that equal the pattern
//!   under its mask, a pattern reaching past the end is `OutOfBounds`, a
//!   `ByteDiffers` names a byte inside the pattern's window that differs in
//!   a checked bit, `find` returns the first matching base, and the same
//!   holds through `Pattern::check` and `Pattern::explain`.
//!
//! The harness allocates only in proportion to the input.

use std::fmt::Write as _;

use rr_core::diag::{Severity, Span};
use rr_core::pattern::{
    BytePattern, MAX_TEXT_LEN, Mismatch, ParseError, ParseErrorKind, Pattern, Subject,
};

/// The `pattern` target: see the module docs.
pub fn pattern(data: &[u8]) {
    let (text, input) = match data.iter().position(|&b| b == 0) {
        Some(nul) => (&data[..nul], &data[nul + 1..]),
        None => (data, &[][..]),
    };
    let text = match std::str::from_utf8(text) {
        Ok(t) => t,
        Err(e) => std::str::from_utf8(&text[..e.valid_up_to()]).unwrap_or(""),
    };
    match BytePattern::parse(text) {
        Ok(p) => {
            assert!(text.len() <= MAX_TEXT_LEN, "over-long text parsed");
            check_pattern(&p);
            check_matching(&p, input);
        }
        Err(e) => check_error(text, &e),
    }
}

fn check_error(text: &str, e: &ParseError) {
    let span = e.span();
    assert!(span.start <= span.end && span.end <= text.len(), "{span:?}");
    assert!(
        text.get(span.clone()).is_some(),
        "span {span:?} off a char boundary"
    );
    assert!(!span.is_empty() || text.is_empty(), "empty span {span:?}");
    let before = text.get(..span.start).unwrap_or("");
    assert_eq!(e.column() as usize, before.chars().count() + 1);
    assert_eq!(e.code(), e.kind().code());
    assert!(e.code().as_str().starts_with("PAT"), "{}", e.code());
    assert_eq!(
        matches!(e.kind(), ParseErrorKind::TooLong { .. }),
        text.len() > MAX_TEXT_LEN
    );
    let d = e.to_diagnostic_at("fuzz", 1, 1);
    assert_eq!(d.severity(), Severity::Error);
    assert_eq!(d.code(), e.code());
    assert_eq!(d.message(), e.to_string());
    assert!(!d.message().is_empty());
    assert_eq!(d.provenance().span(), Span::line_col(1, e.column()));
}

fn check_pattern(p: &BytePattern) {
    assert!(p.len() >= 1);
    assert_eq!(p.value().len(), p.mask().len());
    assert!(p.offset().checked_add(p.len()).is_some());
    assert!(
        p.value().iter().zip(p.mask()).all(|(&v, &m)| v & !m == 0),
        "value not pre-masked"
    );
    let shown = p.to_string();
    let back = BytePattern::parse(&shown)
        .unwrap_or_else(|e| panic!("printed pattern {shown:?} does not parse: {e}"));
    assert_eq!(&back, p, "{shown:?}");
    assert_eq!(back.to_string(), shown);
}

fn check_matching(p: &BytePattern, input: &[u8]) {
    let bases = [0, 1, input.len(), usize::MAX - p.offset(), usize::MAX];
    for base in bases {
        check_at(p, input, base);
    }
    let found = p.find(input);
    if let Some(b) = found {
        assert!(p.matches_at(input, b));
    }
    // `find` is the first matching base. Bound the scan so the harness stays
    // linear in the input for long inputs.
    let limit = found.unwrap_or(input.len()).min(256);
    for base in 0..limit {
        check_at(p, input, base);
        assert!(
            !p.matches_at(input, base),
            "match at {base} before {found:?}"
        );
    }
    if found.is_none() {
        assert!(!p.matches(input));
    }

    let whole = Pattern::from(p.clone());
    let subject = Subject::Bytes(input);
    assert_eq!(whole.matches(subject), p.matches(input));
    assert_eq!(whole.check(subject), p.check(input));
    if let Err(m) = whole.check(subject) {
        let mut s = String::new();
        let _ = write!(s, "{}", whole.explain(&m));
        assert_eq!(s, m.to_string());
    }
    assert!(!whole.matches(Subject::Id("")));
}

fn check_at(p: &BytePattern, input: &[u8], base: usize) {
    let matched = p.matches_at(input, base);
    let checked = p.check_at(input, base);
    assert_eq!(matched, checked.is_ok());
    let window = base
        .checked_add(p.offset())
        .and_then(|start| Some((start, start.checked_add(p.len())?)))
        .filter(|&(_, end)| end <= input.len());
    match (checked, window) {
        (Ok(()), Some((start, end))) => {
            let bytes = &input[start..end];
            assert!(
                bytes
                    .iter()
                    .zip(p.value())
                    .zip(p.mask())
                    .all(|((&b, &v), &m)| b & m == v)
            );
        }
        (
            Err(Mismatch::OutOfBounds {
                base: b,
                offset,
                len,
                input_len,
            }),
            None,
        ) => {
            assert_eq!(
                (b, offset, len, input_len),
                (base, p.offset(), p.len(), input.len())
            );
        }
        (
            Err(Mismatch::ByteDiffers {
                position,
                expected,
                mask,
                found,
            }),
            Some((start, end)),
        ) => {
            assert!((start..end).contains(&position));
            let i = position - start;
            assert_eq!(input[position], found);
            assert_eq!((p.value()[i], p.mask()[i]), (expected, mask));
            assert_ne!(found & mask, expected);
        }
        (other, window) => panic!("check_at({base}) = {other:?} with window {window:?}"),
    }
}
