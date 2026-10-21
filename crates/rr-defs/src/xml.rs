//! Hardened XML reading (US-RR-27, SECURITY.md *API Security*, EC-09).
//!
//! Every XML definition format (RomRaider ECU and logger definitions, XDF)
//! is read through this module, so that a malicious or broken file
//! downloaded from a forum cannot crash, hang or exploit the program reading
//! it. [`parse`] turns the bytes of one file into an owned [`Document`] tree,
//! or into diagnostics saying why it cannot:
//!
//! * **No DTD processing.** A DOCTYPE is read past, never processed: the
//!   external DTD is not fetched and declarations are not applied. An
//!   internal subset that declares an entity, or uses a parameter-entity
//!   reference, is refused (`XML0003`): that is how billion-laughs and XXE
//!   files work. Element and attribute-list declarations, which real
//!   definition files carry, are ignored, with a warning (`XML0005`) for an
//!   attribute default legacy's parser would have applied.
//! * **No entity expansion.** Only the five predefined entities (`&lt;`
//!   `&gt;` `&amp;` `&apos;` `&quot;`) and character references (`&#176;`,
//!   `&#xB0;`) are replaced; any other `&name;` is an error (`XML0004`),
//!   never looked up.
//! * **Limits.** Input length, nesting depth, element count, attributes per
//!   element, name and value length and total text are bounded by
//!   [`Limits`], checked before the work they bound, so a hostile file
//!   costs bounded time and memory to refuse.
//! * **Well-formedness.** Mismatched or unclosed tags, duplicate
//!   attributes, a second root, text outside the root and other syntax
//!   errors are refused (`XML0001`).
//!
//! The pinned definition corpus (D-028: the 265 XML files of `[defs]` and
//! the six logger definitions in `logger_v370.zip`) reads with no diagnostic
//! at all; see [`Limits`] for how far its largest files are from each
//! limit.
//!
//! The first error stops the read: a document that is not well-formed has no
//! trustworthy content, so the value of the returned [`Loaded`] is `None`
//! (and withheld anyway, since the load has errors). Warnings do not stop
//! it. Every diagnostic carries the file's `source` name and the 1-based
//! `line:col` of the problem; lines end at LF, CRLF or a lone CR, so a CRLF
//! file reports the same positions as its LF twin.
//!
//! # Why a tree
//!
//! The ECU definition reader (US-RR-29) looks up attributes by name, walks
//! `<rom>`, `<table>`, `<scaling>` and axis children in whatever order a
//! file writes them, and reports warnings at the `line:col` of any element
//! or attribute. A small owned tree with a [`Span`] on every element,
//! attribute, value and text run serves that directly, and the limits make
//! its size a known bound. A streaming API would push the same bookkeeping
//! into every reader; the files (8 MiB at most in the corpus) do not need
//! one.
//!
//! # Encodings and line ends
//!
//! Decoding is a separate first step. The encoding is UTF-8 unless a UTF-8
//! byte-order mark or the XML declaration says otherwise; the declaration
//! may name UTF-8, ISO-8859-1 (`latin1`, `Latin-1`, ...) or US-ASCII,
//! case-insensitively. UTF-16, UTF-32 and any other declared encoding are
//! refused (`XML0002`), as are bytes invalid in the encoding, at the first
//! bad byte; a BOM contradicting the declaration is read as UTF-8 with a
//! warning (`XML0012`). Positions refer to the original file whatever its
//! encoding: columns count characters, which transcoding keeps one for one.
//!
//! Line ends are normalised as XML 1.0 §2.11 requires: CRLF and a lone CR
//! become LF in text, and attribute values are normalised per §3.3.3
//! (each tab, CR, LF or CRLF becomes one space). So the same definition in
//! UTF-8, UTF-8 with a BOM or ISO-8859-1, with LF, CRLF or CR line ends,
//! reads to the same tree, spans included (a CRLF is one line break, and a
//! BOM or an ISO-8859-1 `°` one column or none, as in an editor), as long as
//! each line holds the same characters. [`Document::same_content`]
//! compares two trees ignoring spans, for variants whose lines differ (one
//! without an XML declaration, say).
//!
//! ```
//! use rr_core::diag::Span;
//! use rr_defs::xml;
//!
//! let loaded = xml::parse(
//!     "defs/base.xml",
//!     b"<?xml version=\"1.0\"?>\r\n<roms>\r\n  <rom base=\"16BITBASE\">x &amp; y</rom>\r\n</roms>",
//! );
//! let (doc, diagnostics) = loaded.into_result().unwrap();
//! assert!(diagnostics.is_empty());
//! let doc = doc.unwrap();
//! let rom = doc.root().elements().next().unwrap();
//! assert_eq!(rom.attr("base"), Some("16BITBASE"));
//! assert_eq!(rom.text(), "x & y");
//! assert_eq!(rom.span(), Span::line_col(3, 3));
//! assert_eq!(rom.attribute("base").unwrap().span(), Span::line_col(3, 8));
//! ```
//!
//! Hostile input becomes diagnostics, never a panic:
//!
//! ```
//! use rr_core::diag::codes;
//! use rr_defs::xml;
//!
//! let laughs = b"<?xml version=\"1.0\"?>\n<!DOCTYPE lolz [\n  <!ENTITY lol \"lol\">\n]>\n<lolz>&lol;</lolz>";
//! let loaded = xml::parse("lolz.xml", laughs);
//! assert_eq!(loaded.value(), None);
//! let diag = &loaded.diagnostics()[0];
//! assert_eq!(diag.code(), codes::XML_ENTITY_DECLARED);
//! assert_eq!(diag.provenance().to_string(), "lolz.xml:3:3");
//! ```
//!
//! The committed hostile sample set, `tests/data/xml/hostile/` (billion
//! laughs, XXE in its variants, deep nesting, UTF-16, encodings that lie,
//! ...), is refused file by file with its expected code and `line:col`,
//! within a time bound and a fixed peak of heap (`tests/xml_hostile.rs`);
//! the same files are the `xml` fuzz target's regression inputs
//! (`fuzz/src/xml_target.rs`, docs/fuzzing.md).

mod decode;
mod dtd;
mod limits;
mod locate;
mod tree;

#[cfg(test)]
mod tests;

use std::borrow::Cow;

use quick_xml::Reader;
use quick_xml::escape::{EscapeError, resolve_predefined_entity};
use quick_xml::events::attributes::AttrError;
use quick_xml::events::{BytesCData, BytesRef, BytesStart, BytesText, Event};
use rr_core::diag::{Code, Diagnostics, Loaded, Provenance, Span, codes};

pub use limits::{Limits, MAX_DEPTH};
pub use tree::{Attribute, Document, Element, Node, Text};

use locate::Locator;

/// Reads one XML file with [`Limits::DEFAULT`]. `source` names it in
/// diagnostics (a path, or a label such as `<clipboard>`).
///
/// The value is the document, or `None` when an error stopped the read; see
/// the [module documentation](self) for what is refused.
pub fn parse(source: &str, input: &[u8]) -> Loaded<Option<Document>> {
    parse_with(source, input, &Limits::DEFAULT)
}

/// [`parse`] with other limits.
pub fn parse_with(source: &str, input: &[u8], limits: &Limits) -> Loaded<Option<Document>> {
    let mut diags = Diagnostics::new();
    if input.len() > limits.max_input_len {
        diags.error(
            codes::XML_TOO_LARGE,
            format!(
                "the file is {} bytes, over the {}-byte limit; it was not read",
                input.len(),
                limits.max_input_len
            ),
            Provenance::new(source, Span::line_col(1, 1)),
        );
        return diags.finish(None);
    }
    let decoded = match decode::decode(input) {
        Ok(decoded) => decoded,
        Err(e) => {
            diags.error(e.code, e.message, Provenance::new(source, e.span));
            return diags.finish(None);
        }
    };
    let mut builder = Builder::new(source, &decoded.text, limits);
    if let Some(w) = decoded.warning {
        builder
            .diags
            .warning(w.code, w.message, Provenance::new(source, w.span));
    }
    let document = builder.run().ok();
    builder.diags.finish(document)
}

/// An error has been recorded and the read stops.
#[derive(Debug)]
struct Stop;

/// The text being collected into one [`Text`] node.
struct PendingText {
    offset: usize,
    value: String,
    /// `value` has been counted against [`Limits::max_text_len`]. A run is
    /// counted from its first non-whitespace piece: until then it may be the
    /// indentation between elements, which is not kept.
    counted: bool,
}

/// Builds the tree from quick-xml's events, enforcing the limits.
struct Builder<'a> {
    source: &'a str,
    text: &'a str,
    limits: &'a Limits,
    loc: Locator<'a>,
    diags: Diagnostics,
    /// Open elements, outermost first.
    stack: Vec<Element>,
    root: Option<Element>,
    elements: u32,
    attributes: u32,
    /// Bytes of text and attribute values kept so far.
    kept: usize,
    pending: Option<PendingText>,
    /// No event has been read yet (an XML declaration must be first).
    first: bool,
    seen_doctype: bool,
}

impl<'a> Builder<'a> {
    fn new(source: &'a str, text: &'a str, limits: &'a Limits) -> Self {
        Self {
            source,
            text,
            limits,
            loc: Locator::new(text),
            diags: Diagnostics::new(),
            stack: Vec::new(),
            root: None,
            elements: 0,
            attributes: 0,
            kept: 0,
            pending: None,
            first: true,
            seen_doctype: false,
        }
    }

    /// Reads every event into the document.
    fn run(&mut self) -> Result<Document, Stop> {
        let mut reader = Reader::from_str(self.text);
        // quick-xml's defaults already check end names and refuse a bare
        // `&`; say so, so a default change upstream cannot loosen this.
        let config = reader.config_mut();
        config.check_end_names = true;
        config.allow_unmatched_ends = false;
        config.allow_dangling_amp = false;
        config.expand_empty_elements = false;
        config.trim_text(false);
        loop {
            let at = to_usize(reader.buffer_position());
            let event = match reader.read_event() {
                Ok(event) => event,
                Err(e) => {
                    let at = to_usize(reader.error_position());
                    return Err(self.fail(
                        codes::XML_MALFORMED,
                        at,
                        format!("the XML is not well-formed: {e}"),
                    ));
                }
            };
            let first = std::mem::replace(&mut self.first, false);
            match event {
                Event::Start(e) => self.start(at, &e, false)?,
                Event::Empty(e) => self.start(at, &e, true)?,
                Event::End(_) => self.end()?,
                Event::Text(t) => self.text(at, &t)?,
                Event::CData(c) => self.cdata(at, &c)?,
                Event::GeneralRef(r) => self.reference(at, &r)?,
                Event::Decl(_) if !first => {
                    return Err(self.fail(
                        codes::XML_MALFORMED,
                        at,
                        "an XML declaration (`<?xml ...?>`) must be the very first thing in the file"
                            .to_owned(),
                    ));
                }
                Event::DocType(d) => self.doctype(at, &d)?,
                Event::Decl(_) | Event::Comment(_) | Event::PI(_) => {}
                Event::Eof => break,
            }
        }
        if let Some(open) = self.stack.last() {
            let message = format!(
                "the file ends inside element <{}> opened at {}",
                open.name,
                open.span.to_string().trim_start_matches(':')
            );
            return Err(self.fail(codes::XML_MALFORMED, self.text.len(), message));
        }
        match self.root.take() {
            Some(root) => Ok(Document { root }),
            None => Err(self.fail(
                codes::XML_MALFORMED,
                self.text.len(),
                "the file has no root element".to_owned(),
            )),
        }
    }

    /// Records an error at byte `at` and returns the marker that stops the
    /// read.
    fn fail(&mut self, code: Code, at: usize, message: String) -> Stop {
        let span = self.loc.span(at);
        self.diags
            .error(code, message, Provenance::new(self.source, span));
        Stop
    }

    /// The offset of `part` in the text, when it is a slice of it (quick-xml
    /// borrows every event from the input); `None` otherwise.
    fn offset_of(&self, part: &[u8]) -> Option<usize> {
        let start = part
            .as_ptr()
            .addr()
            .checked_sub(self.text.as_ptr().addr())?;
        (start.checked_add(part.len())? <= self.text.len()).then_some(start)
    }

    /// Counts `len` more bytes of kept text against the limit.
    fn keep(&mut self, at: usize, len: usize) -> Result<(), Stop> {
        let total = self.kept.saturating_add(len);
        if total > self.limits.max_text_len {
            return Err(self.fail(
                codes::XML_TEXT_TOO_LONG,
                at,
                format!(
                    "the file holds more than {} bytes of text and attribute values",
                    self.limits.max_text_len
                ),
            ));
        }
        self.kept = total;
        Ok(())
    }

    /// Checks a name or raw value against [`Limits::max_value_len`].
    fn check_len(&mut self, at: usize, what: &str, len: usize) -> Result<(), Stop> {
        if len > self.limits.max_value_len {
            return Err(self.fail(
                codes::XML_VALUE_TOO_LONG,
                at,
                format!(
                    "{what} is {len} bytes long, over the {}-byte limit",
                    self.limits.max_value_len
                ),
            ));
        }
        Ok(())
    }

    fn start(&mut self, at: usize, e: &BytesStart<'_>, empty: bool) -> Result<(), Stop> {
        self.flush_text();
        if self.stack.is_empty() && self.root.is_some() {
            return Err(self.fail(
                codes::XML_MALFORMED,
                at,
                "a second root element; a file has exactly one".to_owned(),
            ));
        }
        let depth = self.stack.len().saturating_add(1);
        let max_depth = self.limits.effective_depth();
        if u32::try_from(depth).map_or(true, |d| d > max_depth) {
            return Err(self.fail(
                codes::XML_TOO_DEEP,
                at,
                format!("elements are nested more than {max_depth} levels deep"),
            ));
        }
        if self.elements >= self.limits.max_elements {
            return Err(self.fail(
                codes::XML_TOO_MANY_ELEMENTS,
                at,
                format!(
                    "the file has more than {} elements",
                    self.limits.max_elements
                ),
            ));
        }
        self.elements += 1;
        let raw_name = e.name();
        let raw_name = raw_name.as_ref();
        let name_at = self.offset_of(raw_name).unwrap_or(at.saturating_add(1));
        self.check_len(name_at, "an element name", raw_name.len())?;
        let name = match std::str::from_utf8(raw_name) {
            Ok(n) if !n.is_empty() => n.to_owned(),
            _ => {
                return Err(self.fail(
                    codes::XML_MALFORMED,
                    at,
                    "an element has no name".to_owned(),
                ));
            }
        };
        let span = self.loc.span(at);
        let attributes = self.attributes(name_at, e)?;
        let element = Element {
            name,
            span,
            attributes,
            children: Vec::new(),
        };
        if empty {
            self.close(element);
        } else {
            self.stack.push(element);
        }
        Ok(())
    }

    /// The attributes of a start tag whose name is at `name_at`.
    fn attributes(&mut self, name_at: usize, e: &BytesStart<'_>) -> Result<Vec<Attribute>, Stop> {
        let mut out = Vec::new();
        for attr in e.attributes() {
            let attr = match attr {
                Ok(a) => a,
                Err(err) => {
                    // AttrError positions are relative to the tag's name.
                    let at = name_at.saturating_add(attr_error_position(&err));
                    return Err(self.fail(
                        codes::XML_MALFORMED,
                        at,
                        format!("the XML is not well-formed: {err}"),
                    ));
                }
            };
            let key = attr.key.as_ref();
            let key_at = self.offset_of(key).unwrap_or(name_at);
            if out.len() >= usize::try_from(self.limits.max_attributes).unwrap_or(usize::MAX) {
                return Err(self.fail(
                    codes::XML_TOO_MANY_ATTRIBUTES,
                    key_at,
                    format!(
                        "an element has more than {} attributes",
                        self.limits.max_attributes
                    ),
                ));
            }
            if self.attributes >= self.limits.max_total_attributes {
                return Err(self.fail(
                    codes::XML_TOO_MANY_ATTRIBUTES,
                    key_at,
                    format!(
                        "the file has more than {} attributes",
                        self.limits.max_total_attributes
                    ),
                ));
            }
            self.attributes += 1;
            self.check_len(key_at, "an attribute name", key.len())?;
            let value_at = self.offset_of(&attr.value).unwrap_or(key_at);
            self.check_len(value_at, "an attribute value", attr.value.len())?;
            let name = match std::str::from_utf8(key) {
                Ok(n) => n.to_owned(),
                Err(_) => {
                    return Err(self.fail(
                        codes::XML_MALFORMED,
                        key_at,
                        "an attribute name is not text".to_owned(),
                    ));
                }
            };
            let value = match attr.normalized_value_with(
                quick_xml::XmlVersion::Implicit1_0,
                1,
                resolve_predefined_entity,
            ) {
                Ok(v) => v.into_owned(),
                Err(err) => return Err(self.value_error(value_at, &err)),
            };
            self.keep(value_at, value.len())?;
            let span = self.loc.span(key_at);
            let value_span = self.loc.span(value_at);
            out.push(Attribute {
                name,
                value,
                span,
                value_span,
            });
        }
        Ok(out)
    }

    /// Reports why an attribute value at `value_at` would not unescape.
    fn value_error(&mut self, value_at: usize, err: &quick_xml::Error) -> Stop {
        match err {
            quick_xml::Error::Escape(EscapeError::UnrecognizedEntity(range, name)) => {
                // The range covers the name, after the `&`.
                let at = value_at.saturating_add(range.start.saturating_sub(1));
                self.unknown_entity(at, name)
            }
            quick_xml::Error::Escape(EscapeError::UnterminatedEntity(range)) => self.fail(
                codes::XML_MALFORMED,
                value_at.saturating_add(range.start),
                "an `&` in an attribute value starts no reference (write `&amp;`)".to_owned(),
            ),
            other => self.fail(
                codes::XML_MALFORMED,
                value_at,
                format!("an attribute value is not well-formed: {other}"),
            ),
        }
    }

    fn unknown_entity(&mut self, at: usize, name: &str) -> Stop {
        self.fail(
            codes::XML_UNKNOWN_ENTITY,
            at,
            format!(
                "unknown entity `&{name};`: only `&lt;`, `&gt;`, `&amp;`, `&apos;`, \
                 `&quot;` and character references are allowed"
            ),
        )
    }

    fn end(&mut self) -> Result<(), Stop> {
        self.flush_text();
        // quick-xml checks the name matches the open element; with nothing
        // open it reports the end tag as unmatched before we get here.
        if let Some(element) = self.stack.pop() {
            self.close(element);
        }
        Ok(())
    }

    /// Puts a finished element into its parent, or makes it the root.
    fn close(&mut self, element: Element) {
        match self.stack.last_mut() {
            Some(parent) => parent.children.push(Node::Element(element)),
            None => self.root = Some(element),
        }
    }

    /// Ends the text run being collected: kept as a [`Text`] node unless it
    /// is whitespace only.
    fn flush_text(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        if !pending.counted {
            // Whitespace only.
            return;
        }
        let span = self.loc.span(pending.offset);
        if let Some(parent) = self.stack.last_mut() {
            parent.children.push(Node::Text(Text {
                value: pending.value,
                span,
            }));
        }
    }

    /// Adds character data at `at` to the current text run. Outside the root
    /// only whitespace is allowed.
    fn push_text(&mut self, at: usize, piece: &str, what: &str) -> Result<(), Stop> {
        if self.stack.is_empty() {
            if piece.bytes().all(is_space) {
                return Ok(());
            }
            let first = piece.bytes().position(|b| !is_space(b)).unwrap_or_default();
            return Err(self.fail(
                codes::XML_MALFORMED,
                at.saturating_add(first),
                format!("{what} outside the root element"),
            ));
        }
        let (pending_len, counted) = self
            .pending
            .as_ref()
            .map_or((0, false), |p| (p.value.len(), p.counted));
        let count = if counted {
            Some(piece.len())
        } else if piece.bytes().all(is_space) {
            None
        } else {
            // The first real text: the whitespace before it is kept too.
            Some(pending_len.saturating_add(piece.len()))
        };
        if let Some(len) = count {
            self.keep(at, len)?;
        }
        let pending = self.pending.get_or_insert_with(|| PendingText {
            offset: at,
            value: String::new(),
            counted: false,
        });
        pending.value.push_str(piece);
        pending.counted |= count.is_some();
        Ok(())
    }

    fn text(&mut self, at: usize, t: &BytesText<'_>) -> Result<(), Stop> {
        match t.xml10_content() {
            Ok(content) => self.push_text(at, &content, "text"),
            Err(e) => Err(self.fail(
                codes::XML_ENCODING,
                at,
                format!("text is not valid UTF-8: {e}"),
            )),
        }
    }

    fn cdata(&mut self, at: usize, c: &BytesCData<'_>) -> Result<(), Stop> {
        if self.stack.is_empty() {
            return Err(self.fail(
                codes::XML_MALFORMED,
                at,
                "a CDATA section outside the root element".to_owned(),
            ));
        }
        match c.xml10_content() {
            Ok(content) => self.push_text(at, &content, "a CDATA section"),
            Err(e) => Err(self.fail(
                codes::XML_ENCODING,
                at,
                format!("a CDATA section is not valid UTF-8: {e}"),
            )),
        }
    }

    /// A `&...;` reference in text at `at`.
    fn reference(&mut self, at: usize, r: &BytesRef<'_>) -> Result<(), Stop> {
        if self.stack.is_empty() {
            return Err(self.fail(
                codes::XML_MALFORMED,
                at,
                "a reference outside the root element".to_owned(),
            ));
        }
        let name: Cow<'_, str> = match r.decode() {
            Ok(n) => n,
            Err(e) => {
                return Err(self.fail(
                    codes::XML_ENCODING,
                    at,
                    format!("a reference is not valid UTF-8: {e}"),
                ));
            }
        };
        if r.is_char_ref() {
            return match r.resolve_char_ref() {
                Ok(Some(c)) => {
                    let mut buf = [0; 4];
                    self.push_text(at, c.encode_utf8(&mut buf), "a reference")
                }
                Ok(None) | Err(_) => Err(self.fail(
                    codes::XML_MALFORMED,
                    at,
                    format!("`&{name};` is not a valid character reference"),
                )),
            };
        }
        match resolve_predefined_entity(&name) {
            Some(s) => self.push_text(at, s, "a reference"),
            None => Err(self.unknown_entity(at, &name)),
        }
    }

    /// A DOCTYPE at `at`: scanned for what must be refused, never applied.
    fn doctype(&mut self, at: usize, d: &BytesText<'_>) -> Result<(), Stop> {
        if self.seen_doctype || self.root.is_some() || !self.stack.is_empty() {
            return Err(self.fail(
                codes::XML_MALFORMED,
                at,
                "a DOCTYPE must come once, before the root element".to_owned(),
            ));
        }
        self.seen_doctype = true;
        let raw: &[u8] = d;
        let content_at = self.offset_of(raw).unwrap_or(at);
        let Ok(content) = std::str::from_utf8(raw) else {
            return Err(self.fail(
                codes::XML_ENCODING,
                at,
                "the DOCTYPE is not valid UTF-8".to_owned(),
            ));
        };
        for finding in dtd::scan(content) {
            match finding {
                dtd::Finding::Entity { offset, name } => {
                    let name = if name.is_empty() {
                        String::new()
                    } else {
                        format!(" `{name}`")
                    };
                    return Err(self.fail(
                        codes::XML_ENTITY_DECLARED,
                        content_at.saturating_add(offset),
                        format!(
                            "the DTD declares an entity{name}; entity declarations are \
                             refused (they enable entity-expansion and external-entity attacks)"
                        ),
                    ));
                }
                dtd::Finding::ParameterReference { offset } => {
                    return Err(self.fail(
                        codes::XML_ENTITY_DECLARED,
                        content_at.saturating_add(offset),
                        "the DTD uses a parameter-entity reference; entities are never expanded"
                            .to_owned(),
                    ));
                }
                dtd::Finding::Malformed { offset, message } => {
                    return Err(self.fail(
                        codes::XML_MALFORMED,
                        content_at.saturating_add(offset),
                        message,
                    ));
                }
                dtd::Finding::DefaultIgnored { offset, element } => {
                    let span = self.loc.span(content_at.saturating_add(offset));
                    self.diags.warning(
                        codes::XML_DTD_DEFAULT_IGNORED,
                        format!(
                            "the DTD gives attributes of <{element}> a default value, which is \
                             not applied; elements without the attribute are read as lacking it"
                        ),
                        Provenance::new(self.source, span),
                    );
                }
            }
        }
        Ok(())
    }
}

/// The position an [`AttrError`] reports, relative to the tag's name.
const fn attr_error_position(err: &AttrError) -> usize {
    match *err {
        AttrError::ExpectedEq(p)
        | AttrError::ExpectedValue(p)
        | AttrError::UnquotedValue(p)
        | AttrError::ExpectedQuote(p, _)
        | AttrError::Duplicated(p, _) => p,
    }
}

/// A reader position as an offset; positions never exceed the input length,
/// which is a `usize`.
fn to_usize(pos: u64) -> usize {
    usize::try_from(pos).unwrap_or(usize::MAX)
}

/// XML's `S`: space, tab, CR, LF.
const fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}
