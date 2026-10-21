//! Body of the `xml` fuzz target (task US-RR-27-7): untrusted bytes, as a
//! definition file downloaded from a forum would supply them, through the
//! hardened XML reader `rr_defs::xml::parse_with` (SECURITY.md *API
//! Security*, EC-09).
//!
//! # Input
//!
//! The whole input is one file. Seeds are small real-shaped definition
//! files; `regressions/xml/` holds the hostile sample set of
//! `crates/rr-defs/tests/data/xml/hostile/` (billion laughs, XXE, deep
//! nesting, ...), byte for byte.
//!
//! # Limits
//!
//! Each input is read twice:
//!
//! * with [`wide`]: [`Limits::DEFAULT`], what shipping code uses, except the
//!   input length, capped at [`MAX_INPUT_LEN`] (1 MiB). The default
//!   limits allow a 64 MiB file and, at the very worst, about half a GiB of
//!   tree, which would trip libFuzzer's `-malloc_limit_mb=512` /
//!   `-rss_limit_mb=2048` on a legitimately large input rather than on a
//!   bug. Under a 1 MiB input the other default limits are unreachable in
//!   bulk (a million elements need 4 MiB of `<a/>`), so the tree stays
//!   within a few tens of MiB, while every limit can still be hit at its
//!   edge (depth 65 is 195 bytes of `<a>`);
//! * with [`tight`]: every limit small (depth 4, 16 elements, 4 attributes,
//!   16-byte names and values, 64 bytes of text), so that small inputs hit
//!   each limit check, and the boundary between "just within" and "just
//!   over" is explored on every input.
//!
//! # What must hold
//!
//! For each read:
//!
//! * every diagnostic is a registered `XML` code with its registered
//!   severity, names the source, and has a `line:col` inside the input;
//! * at most one error, and it is the last diagnostic (the first error
//!   stops the read); the only warnings are `XML0005` and `XML0012`;
//! * the document is there exactly when there is no error (also through
//!   `accept_errors`: no partial tree), and `XML0006` is reported exactly
//!   when the input is over `max_input_len`;
//! * a document obeys every limit it was read under (depth, element and
//!   attribute counts, name and value lengths, total text), has unique
//!   attribute names per element, no whitespace-only or adjacent text
//!   nodes, and spans in document order inside the input;
//! * written back out as plain UTF-8 XML (when every name is a plain XML
//!   name) it reads again, with no diagnostic, to a document with the same
//!   content.
//!
//! Across the two reads: whatever reads under the tight limits reads to the
//! identical document, with identical diagnostics, under the wide ones
//! (raising a limit never changes what a file means).
//!
//! The harness allocates only in proportion to the input.

use std::fmt::Write as _;

use rr_core::diag::{Diagnostic, Severity, Span, codes};
use rr_defs::xml::{Document, Element, Limits, Node, parse_with};

/// The `source` every read reports.
const SOURCE: &str = "fuzz.xml";

/// The longest input read: see the module docs.
pub const MAX_INPUT_LEN: usize = 1 << 20;

/// [`Limits::DEFAULT`] with the input length capped at [`MAX_INPUT_LEN`].
#[must_use]
pub fn wide() -> Limits {
    let mut l = Limits::DEFAULT;
    l.max_input_len = MAX_INPUT_LEN;
    l
}

/// Small limits, every one reachable by a short input.
#[must_use]
pub fn tight() -> Limits {
    let mut l = Limits::DEFAULT;
    l.max_input_len = MAX_INPUT_LEN;
    l.max_depth = 4;
    l.max_elements = 16;
    l.max_attributes = 4;
    l.max_total_attributes = 8;
    l.max_value_len = 16;
    l.max_text_len = 64;
    l
}

/// The `xml` target: see the module docs.
pub fn xml(data: &[u8]) {
    let lines = Lines::new(data);
    let wide = wide();
    let (wide_doc, wide_diags) = read(data, &wide, &lines);
    let tight = tight();
    let (tight_doc, tight_diags) = read(data, &tight, &lines);
    if let Some(t) = &tight_doc {
        assert_eq!(
            wide_doc.as_ref(),
            Some(t),
            "read under tight limits but not identically under wide ones"
        );
        assert_eq!(wide_diags, tight_diags);
    }
    if let Some(doc) = &wide_doc {
        round_trip(doc, &wide);
    }
}

/// Reads `data` under `limits` and checks the result.
fn read(data: &[u8], limits: &Limits, lines: &Lines) -> (Option<Document>, Vec<Diagnostic>) {
    let loaded = parse_with(SOURCE, data, limits);
    let diags = loaded.diagnostics().to_vec();
    let errors = diags.iter().filter(|d| d.is_error()).count();
    assert!(errors <= 1, "{diags:#?}");
    if errors == 1 {
        assert!(diags.last().is_some_and(Diagnostic::is_error), "{diags:#?}");
    }
    for d in &diags {
        check_diagnostic(d, lines);
    }
    assert_eq!(
        diags.iter().any(|d| d.code() == codes::XML_TOO_LARGE),
        data.len() > limits.max_input_len,
        "{diags:#?}"
    );
    assert_eq!(loaded.value().is_some(), errors == 0);
    let (doc, _) = loaded.accept_errors();
    assert_eq!(doc.is_some(), errors == 0, "{diags:#?}");
    if let Some(doc) = &doc {
        check_document(doc, limits, lines);
    }
    (doc, diags)
}

fn check_diagnostic(d: &Diagnostic, lines: &Lines) {
    let info = codes::lookup(d.code()).unwrap_or_else(|| panic!("unregistered code {d}"));
    assert_eq!(info.severity, d.severity(), "{d}");
    assert!(d.code().as_str().starts_with("XML"), "{d}");
    if d.severity() == Severity::Warning {
        assert!(
            d.code() == codes::XML_DTD_DEFAULT_IGNORED || d.code() == codes::XML_ENCODING_MISMATCH,
            "{d}"
        );
    }
    assert!(!d.message().is_empty(), "{d}");
    assert_eq!(d.provenance().source(), SOURCE);
    lines.check(d.provenance().span(), &d.to_string());
}

/// Line lengths of the raw input, in bytes, lines ending at LF, CRLF or a
/// lone CR (as the reader counts them). A column counts characters, never
/// more than bytes in UTF-8 or ISO-8859-1, so `col <= len + 1`.
struct Lines(Vec<usize>);

impl Lines {
    fn new(data: &[u8]) -> Self {
        let mut lens = vec![0];
        let mut i = 0;
        while i < data.len() {
            match data[i] {
                b'\r' => {
                    if data.get(i + 1) == Some(&b'\n') {
                        i += 1;
                    }
                    lens.push(0);
                }
                b'\n' => lens.push(0),
                _ => {
                    if let Some(last) = lens.last_mut() {
                        *last += 1;
                    }
                }
            }
            i += 1;
        }
        Self(lens)
    }

    fn check(&self, span: Span, what: &str) {
        let Span::LineCol { line, col } = span else {
            panic!("not a line:col span: {what}");
        };
        assert!(line >= 1 && col >= 1, "{what}");
        let len = self
            .0
            .get(line as usize - 1)
            .unwrap_or_else(|| panic!("line {line} past the end ({} lines): {what}", self.0.len()));
        assert!(
            col as usize <= len + 1,
            "column {col} past line {line} ({len} bytes): {what}"
        );
    }
}

/// Totals over a document, checked against its limits.
#[derive(Default)]
struct Totals {
    elements: u64,
    attributes: u64,
    kept: usize,
}

fn check_document(doc: &Document, limits: &Limits, lines: &Lines) {
    let mut totals = Totals::default();
    let mut last = Span::line_col(0, 0);
    check_element(doc.root(), 1, limits, lines, &mut totals, &mut last);
    assert!(totals.elements <= u64::from(limits.max_elements));
    assert!(totals.attributes <= u64::from(limits.max_total_attributes));
    assert!(totals.kept <= limits.max_text_len);
}

/// Checks `e` at nesting `depth` and everything in it. `last` is the
/// latest span seen so far: every span must come strictly after it.
fn check_element(
    e: &Element,
    depth: u32,
    limits: &Limits,
    lines: &Lines,
    totals: &mut Totals,
    last: &mut Span,
) {
    assert!(depth <= limits.effective_depth(), "depth {depth}");
    totals.elements += 1;
    assert!(!e.name().is_empty());
    assert!(e.name().len() <= limits.max_value_len);
    advance(last, e.span(), lines, e.name());
    let attrs = e.attributes();
    assert!(attrs.len() <= limits.max_attributes as usize);
    totals.attributes += attrs.len() as u64;
    for (i, a) in attrs.iter().enumerate() {
        assert!(!a.name().is_empty() && a.name().len() <= limits.max_value_len);
        assert!(
            attrs[..i].iter().all(|b| b.name() != a.name()),
            "duplicate attribute {}",
            a.name()
        );
        advance(last, a.span(), lines, a.name());
        advance(last, a.value_span(), lines, a.name());
        totals.kept += a.value().len();
        assert_eq!(e.attr(a.name()), Some(a.value()));
    }
    let mut previous_text = false;
    for child in e.children() {
        match child {
            Node::Element(c) => {
                previous_text = false;
                check_element(c, depth + 1, limits, lines, totals, last);
            }
            Node::Text(t) => {
                assert!(!previous_text, "adjacent text nodes in <{}>", e.name());
                previous_text = true;
                assert!(
                    !t.value()
                        .bytes()
                        .all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n')),
                    "whitespace-only text kept"
                );
                advance(last, t.span(), lines, "text");
                totals.kept += t.value().len();
            }
        }
    }
    let own: usize = e
        .children()
        .iter()
        .filter_map(Node::as_text)
        .map(|t| t.value().len())
        .sum();
    assert_eq!(e.text().len(), own);
}

/// `span` is inside the input and after `last`; it becomes `last`.
fn advance(last: &mut Span, span: Span, lines: &Lines, what: &str) {
    lines.check(span, what);
    assert!(span > *last, "{what} at {span} not after {last}");
    *last = span;
}

/// Writes `doc` back out and reads it again: same content, no diagnostic.
/// Skipped when a name is not a plain XML name, since the reader keeps
/// whatever quick-xml takes for a name and such a name need not survive
/// being written into a different tag.
fn round_trip(doc: &Document, limits: &Limits) {
    let mut out = String::new();
    if !write_element(&mut out, doc.root()) {
        return;
    }
    // Escaping can make a value up to six times longer (`"` is `&quot;`),
    // and end tags repeat every name; the content limits stay as they were.
    let mut relaxed = *limits;
    relaxed.max_input_len = usize::MAX;
    relaxed.max_value_len = limits.max_value_len.saturating_mul(6);
    let again = parse_with(SOURCE, out.as_bytes(), &relaxed);
    assert_eq!(again.diagnostics(), &[], "re-reading {out:?}");
    let (back, _) = again.accept_errors();
    let back = back.unwrap_or_else(|| panic!("re-reading {out:?}: no document"));
    assert!(back.same_content(doc), "{out:?} read back differently");
}

/// Appends `e` as XML; false if a name is not a plain XML name.
fn write_element(out: &mut String, e: &Element) -> bool {
    if !is_plain_name(e.name()) {
        return false;
    }
    let _ = write!(out, "<{}", e.name());
    for a in e.attributes() {
        if !is_plain_name(a.name()) {
            return false;
        }
        let _ = write!(out, " {}=\"", a.name());
        for c in a.value().chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '"' => out.push_str("&quot;"),
                '\t' | '\n' | '\r' => {
                    let _ = write!(out, "&#{};", u32::from(c));
                }
                c => out.push(c),
            }
        }
        out.push('"');
    }
    out.push('>');
    for child in e.children() {
        match child {
            Node::Element(c) => {
                if !write_element(out, c) {
                    return false;
                }
            }
            Node::Text(t) => {
                for c in t.value().chars() {
                    match c {
                        '&' => out.push_str("&amp;"),
                        '<' => out.push_str("&lt;"),
                        '>' => out.push_str("&gt;"),
                        '\r' => out.push_str("&#13;"),
                        c => out.push(c),
                    }
                }
            }
        }
    }
    let _ = write!(out, "</{}>", e.name());
    true
}

/// An XML `Name` of ASCII letters, digits, `_`, `:`, `-` and `.`, not
/// starting with a digit, `-` or `.`; or any non-ASCII letter.
fn is_plain_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let start = |c: char| {
        c.is_ascii_alphabetic() || c == '_' || c == ':' || (!c.is_ascii() && c.is_alphabetic())
    };
    start(first) && chars.all(|c| start(c) || c.is_ascii_digit() || c == '-' || c == '.')
}
