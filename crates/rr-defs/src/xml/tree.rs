//! The owned tree [`parse`](super::parse) builds.

use std::borrow::Cow;

use rr_core::diag::Span;

/// A well-formed XML document: its root element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub(crate) root: Element,
}

impl Document {
    /// The root element.
    #[must_use]
    pub const fn root(&self) -> &Element {
        &self.root
    }

    /// The root element, by value.
    #[must_use]
    pub fn into_root(self) -> Element {
        self.root
    }

    /// Whether `other` has the same content: equal but for [`Span`]s. Two
    /// files that differ only in encoding, byte-order mark, line ends or
    /// where their lines break read to documents with the same content.
    #[must_use]
    pub fn same_content(&self, other: &Self) -> bool {
        self.root.same_content(&other.root)
    }
}

/// One element: its name, attributes and content, and where it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub(crate) name: String,
    pub(crate) span: Span,
    pub(crate) attributes: Vec<Attribute>,
    pub(crate) children: Vec<Node>,
}

impl Element {
    /// The element name exactly as written (`table`, `xi:include`); names
    /// are case-sensitive, as XML's are.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Where the element starts: its `<`.
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }

    /// The attributes, in document order. Names are unique within an
    /// element (a duplicate is a well-formedness error).
    #[must_use]
    pub fn attributes(&self) -> &[Attribute] {
        &self.attributes
    }

    /// The attribute called `name` (exact, case-sensitive match).
    #[must_use]
    pub fn attribute(&self, name: &str) -> Option<&Attribute> {
        self.attributes.iter().find(|a| a.name == name)
    }

    /// The value of the attribute called `name`.
    #[must_use]
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attribute(name).map(Attribute::value)
    }

    /// The content: child elements and text, in document order. Comments,
    /// processing instructions and whitespace-only text are not kept.
    #[must_use]
    pub fn children(&self) -> &[Node] {
        &self.children
    }

    /// The child elements, in document order.
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(Node::as_element)
    }

    /// The child elements called `name`, in document order.
    pub fn elements_named<'s>(&'s self, name: &'s str) -> impl Iterator<Item = &'s Element> {
        self.elements().filter(move |e| e.name == name)
    }

    /// Whether `other` is the same element, attributes and content, all
    /// compared by name and value only: equal but for [`Span`]s.
    #[must_use]
    pub fn same_content(&self, other: &Self) -> bool {
        self.name == other.name
            && self.attributes.len() == other.attributes.len()
            && self
                .attributes
                .iter()
                .zip(&other.attributes)
                .all(|(a, b)| a.name == b.name && a.value == b.value)
            && self.children.len() == other.children.len()
            && self
                .children
                .iter()
                .zip(&other.children)
                .all(|(a, b)| match (a, b) {
                    (Node::Element(a), Node::Element(b)) => a.same_content(b),
                    (Node::Text(a), Node::Text(b)) => a.value == b.value,
                    _ => false,
                })
    }

    /// The element's own text: its text children joined, child elements'
    /// text not included. Empty when it has none.
    #[must_use]
    pub fn text(&self) -> Cow<'_, str> {
        let mut texts = self.children.iter().filter_map(Node::as_text);
        let Some(first) = texts.next() else {
            return Cow::Borrowed("");
        };
        match texts.next() {
            None => Cow::Borrowed(first.value()),
            Some(second) => {
                let mut joined = String::from(first.value());
                joined.push_str(second.value());
                texts.for_each(|t| joined.push_str(t.value()));
                Cow::Owned(joined)
            }
        }
    }
}

/// One attribute of an [`Element`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub(crate) name: String,
    pub(crate) value: String,
    pub(crate) span: Span,
    pub(crate) value_span: Span,
}

impl Attribute {
    /// The attribute name exactly as written.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The value, unescaped (`&lt;` is `<`, `&#176;` is `°`) and normalised
    /// as XML requires: each tab, CR, LF and CRLF becomes one space.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Where the attribute starts: the first character of its name.
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }

    /// Where the value starts: the first character inside the quotes (the
    /// closing quote for an empty value).
    #[must_use]
    pub const fn value_span(&self) -> Span {
        self.value_span
    }
}

/// A run of character data: text, references and CDATA sections between two
/// pieces of markup other than comments and processing instructions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    pub(crate) value: String,
    pub(crate) span: Span,
}

impl Text {
    /// The text, unescaped, with every CRLF and lone CR turned into LF.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Where the run starts.
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }
}

/// One piece of an element's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A child element.
    Element(Element),
    /// Text that is not whitespace only.
    Text(Text),
}

impl Node {
    /// The element, if this is one.
    #[must_use]
    pub const fn as_element(&self) -> Option<&Element> {
        match self {
            Self::Element(e) => Some(e),
            Self::Text(_) => None,
        }
    }

    /// The text, if this is text.
    #[must_use]
    pub const fn as_text(&self) -> Option<&Text> {
        match self {
            Self::Text(t) => Some(t),
            Self::Element(_) => None,
        }
    }
}
