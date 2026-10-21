//! The reading context shared by every element reader of the RomRaider
//! format: where diagnostics go, and the checks every element gets (unknown
//! attributes, unknown or unsupported child elements, stray text, repeated
//! elements).
//!
//! Every element and attribute the format does not define is reported, by
//! name and `line:col`, never dropped in silence (MF-23): legacy RomRaider
//! skipped them without a word (`xml/DOMRomUnmarshaller.java:192-195`,
//! `:286-289`). The element readers for tables and scalings (US-RR-29-6,
//! US-RR-29-7) report through the same helpers.

use std::borrow::Cow;

use rr_core::diag::{Code, Diagnostics, Provenance, Span, codes};

use crate::xml::{Attribute, Element, Node};

/// Where diagnostics go while one file is read.
pub(super) struct Ctx<'a> {
    pub(super) source: &'a str,
    pub(super) diags: Diagnostics,
}

impl<'a> Ctx<'a> {
    pub(super) const fn new(source: &'a str) -> Self {
        Self {
            source,
            diags: Diagnostics::new(),
        }
    }

    pub(super) fn provenance(&self, span: Span) -> Provenance {
        Provenance::new(self.source, span)
    }

    pub(super) fn error(&mut self, code: Code, message: String, span: Span) {
        let provenance = self.provenance(span);
        self.diags.error(code, message, provenance);
    }

    pub(super) fn warning(&mut self, code: Code, message: String, span: Span) {
        let provenance = self.provenance(span);
        self.diags.warning(code, message, provenance);
    }

    /// Warns about every attribute of `el` whose name is not in `known`.
    /// Attribute names are case-sensitive, as legacy read them
    /// (`getAttributes().getNamedItem(name)`, `xml/DOMHelper.java:48-51`).
    pub(super) fn check_attributes(&mut self, el: &Element, known: &[&str]) {
        self.check_attributes_as(el, known, &format!("<{}>", el.name()));
    }

    /// [`check_attributes`](Self::check_attributes), naming the element as
    /// `what` (`an axis <table>`) where its name alone would mislead.
    pub(super) fn check_attributes_as(&mut self, el: &Element, known: &[&str], what: &str) {
        for attr in el.attributes() {
            if !known.contains(&attr.name()) {
                self.unknown_attribute(attr, what);
            }
        }
    }

    /// Warns that `attr`, on the element described as `what`, is not part
    /// of the format.
    pub(super) fn unknown_attribute(&mut self, attr: &Attribute, what: &str) {
        let message = format!("unknown attribute `{}` on {what}; ignored", attr.name());
        self.warning(codes::UNKNOWN_ATTRIBUTE, message, attr.span());
    }

    /// Checks an element that says everything in its attributes (such as a
    /// switch's `<state>`): attributes not in `known`, child elements and
    /// text are reported and ignored.
    pub(super) fn attributes_only(&mut self, el: &Element, known: &[&str]) {
        self.check_attributes(el, known);
        self.no_text(el);
        for child in el.elements() {
            self.unknown_element(child, el.name());
        }
    }

    /// Warns that `el`, a child of `<parent>`, is not part of the format.
    pub(super) fn unknown_element(&mut self, el: &Element, parent: &str) {
        self.unknown_element_in(el, &format!("<{parent}>"));
    }

    /// [`unknown_element`](Self::unknown_element), naming the parent as
    /// `what` (`an axis <table>`).
    pub(super) fn unknown_element_in(&mut self, el: &Element, what: &str) {
        let message = format!("unknown element <{}> in {what}; ignored", el.name());
        self.warning(codes::UNKNOWN_ELEMENT, message, el.span());
    }

    /// Warns that `el`, a child of `<parent>`, is valid but not read yet;
    /// `what` says what legacy read it as.
    pub(super) fn unsupported_element(&mut self, el: &Element, parent: &str, what: &str) {
        let message = format!(
            "<{}> in <{parent}> ({what}) is not supported yet; ignored",
            el.name()
        );
        self.warning(codes::UNSUPPORTED_ELEMENT, message, el.span());
    }

    /// Warns that `el` (a child of `<parent>`) repeats an element that may
    /// appear once; the later one wins, as in legacy.
    pub(super) fn duplicate_element(&mut self, el: &Element, parent: &str) {
        let message = format!(
            "<{}> appears more than once in <{parent}>; this later one wins",
            el.name()
        );
        self.warning(codes::DUPLICATE_ELEMENT, message, el.span());
    }

    /// Warns about each text run directly inside `el`, which holds only
    /// elements (or nothing).
    pub(super) fn no_text(&mut self, el: &Element) {
        for node in el.children() {
            if let Node::Text(t) = node {
                let message = format!("text directly inside <{}>; ignored", el.name());
                self.warning(codes::UNEXPECTED_TEXT, message, t.span());
            }
        }
    }

    /// The text of a value element such as `<xmlid>`, exactly as written
    /// (legacy's `unmarshallText`, `xml/DOMHelper.java:30-46`, did not trim),
    /// and where it starts (the element when it is empty). Attributes not in
    /// `known_attributes` and child elements are reported and ignored, as
    /// legacy ignored them.
    pub(super) fn value_text<'e>(
        &mut self,
        el: &'e Element,
        known_attributes: &[&str],
    ) -> (Cow<'e, str>, Span) {
        self.check_attributes(el, known_attributes);
        for child in el.elements() {
            self.unknown_element(child, el.name());
        }
        let span = el
            .children()
            .iter()
            .find_map(Node::as_text)
            .map_or(el.span(), |t| t.span());
        (el.text(), span)
    }
}

/// Whether `el` is called `name`, compared ASCII case-insensitively, as
/// legacy compared every element name of the format (`equalsIgnoreCase`,
/// `xml/DOMRomUnmarshaller.java:89-226`).
pub(super) fn is(el: &Element, name: &str) -> bool {
    el.name().eq_ignore_ascii_case(name)
}

/// The attribute `name` of `el`, unless its value is `NaN`, which legacy
/// read as an absent attribute (`unmarshallAttribute`,
/// `xml/DOMHelper.java:48-51`).
pub(super) fn attribute<'e>(el: &'e Element, name: &str) -> Option<&'e Attribute> {
    el.attribute(name).filter(|a| a.value() != "NaN")
}
