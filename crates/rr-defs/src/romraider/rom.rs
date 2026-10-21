//! `<rom>`: one ROM definition, a partial document on top of its `base`.

use std::collections::BTreeMap;
use std::sync::Arc;

use rr_core::diag::{Span, codes};
use rr_core::layer::{Field, Keyed, Layer, LayerId, LayerKind, Merge};

use super::ctx::{Ctx, is};
use super::romid::{Identification, RomIdDoc, read_romid};
use super::table::{TableDoc, read_table};
use crate::xml::{Element, Node};

/// The partial document (C1) of one `<rom>`: everything it says, to be
/// layered on top of its `base`'s ([`DefinitionSet`](super::DefinitionSet)).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RomDoc {
    /// The `<romid>`, merged field by field over the base's.
    pub romid: RomIdDoc,
    /// The `<table>`s, keyed by [`table_key`](super::table_key) (the name
    /// lower-cased), in document order: each a patch of the inherited table
    /// of that name (or a new table), or omitted (`omit="true"`). Two
    /// `<table>`s of one name in one `<rom>` merge, the later winning, as
    /// in legacy.
    pub tables: Keyed<String, TableDoc>,
}

rr_core::merge_fields!(RomDoc { romid, tables });

/// A `<rom base="...">` reference to the definition this one extends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseRef {
    name: String,
    span: Span,
}

impl BaseRef {
    /// The base's `xmlid`, exactly as written. (Legacy looked it up among
    /// the `xmlid`s of the same file, ignoring ASCII case,
    /// `xml/DOMRomUnmarshaller.java:127`; a [`DefinitionSet`](super::DefinitionSet)
    /// looks it up in every file, its own first.)
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Where the attribute's value starts.
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }
}

/// One `<rom>` as read: its name, its `base`, its partial document and how
/// an image is recognised as it.
#[derive(Debug, Clone, PartialEq)]
pub struct RomEntry {
    source: Arc<str>,
    xmlid: String,
    span: Span,
    base: Option<BaseRef>,
    doc: Arc<RomDoc>,
    identification: Option<Identification>,
    /// Where the last `<table>` of each key is.
    table_spans: BTreeMap<String, Span>,
}

impl RomEntry {
    /// The definition's name: its own `<xmlid>`, never empty.
    #[must_use]
    pub fn xmlid(&self) -> &str {
        &self.xmlid
    }

    /// Where the `<rom>` element starts.
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }

    /// The definition it extends, if any (`base` absent or `none`, in any
    /// case, means none, as in legacy, `xml/DOMRomUnmarshaller.java:144-145`).
    #[must_use]
    pub const fn base(&self) -> Option<&BaseRef> {
        self.base.as_ref()
    }

    /// Everything this `<rom>` itself says.
    #[must_use]
    pub fn doc(&self) -> &RomDoc {
        &self.doc
    }

    /// [`doc`](Self::doc), shared: the document a
    /// [`DefinitionSet`](super::DefinitionSet) layers, never copied.
    #[must_use]
    pub const fn shared_doc(&self) -> &Arc<RomDoc> {
        &self.doc
    }

    /// Where this `<rom>`'s `<table>` of key `key` ([`table_key`](super::table_key))
    /// is: the last one, when two of one name merge. `None` if the `<rom>`
    /// has none.
    #[must_use]
    pub fn table_span(&self, key: &str) -> Option<Span> {
        self.table_spans.get(key).copied()
    }

    /// The source name of the file the `<rom>` is in.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// How an image is recognised as this ROM; `None` for a definition that
    /// only serves as a base (no `internalidstring`), or whose
    /// identification is broken (reported).
    #[must_use]
    pub const fn identification(&self) -> Option<&Identification> {
        self.identification.as_ref()
    }

    /// The layer (C1) this `<rom>` is: id its `xmlid`, kind
    /// [`LayerKind::File`], at the `<rom>` element of its file.
    #[must_use]
    pub fn layer(&self) -> Layer {
        Layer::new(
            LayerId::from(self.xmlid.as_str()),
            LayerKind::File,
            Arc::clone(&self.source),
            self.span,
        )
    }
}

/// Reads one `<rom>` element of the file `source`; `None` (with an error)
/// when it has no name.
pub(super) fn read_rom(ctx: &mut Ctx<'_>, source: &Arc<str>, el: &Element) -> Option<RomEntry> {
    ctx.check_attributes(el, &["base"]);
    let base = el.attribute("base").and_then(|attr| {
        let value = attr.value();
        // Legacy read an attribute whose value is `NaN` as absent
        // (`xml/DOMHelper.java:50`), and `none` as no base.
        (value != "NaN" && !value.eq_ignore_ascii_case("none")).then(|| BaseRef {
            name: value.to_owned(),
            span: attr.value_span(),
        })
    });
    let mut doc = RomDoc::default();
    let mut identification = None;
    let mut romids = 0_usize;
    let mut table_spans = BTreeMap::new();
    for node in el.children() {
        let child = match node {
            Node::Element(child) => child,
            Node::Text(t) => {
                let message = format!("text directly inside <{}>; ignored", el.name());
                ctx.warning(codes::UNEXPECTED_TEXT, message, t.span());
                continue;
            }
        };
        if is(child, "romid") {
            if romids > 0 {
                ctx.duplicate_element(child, el.name());
            }
            let read = read_romid(ctx, child);
            if romids == 0 {
                identification = read.identification;
            }
            romids += 1;
            doc.romid.merge_from(&read.doc);
        } else if is(child, "table") {
            let tables = &doc.tables;
            let earlier = |key: &str| tables.get(&key.to_owned()).is_some();
            if let Some(table) = read_table(ctx, child, base.is_some(), earlier) {
                if doc.tables.get(&table.key).is_some() {
                    ctx.warning(
                        codes::DUPLICATE_ELEMENT,
                        format!(
                            "table `{}` appears more than once in <{}>; read as a change to the \
                             earlier one, this later one's values winning, as in legacy",
                            table.name,
                            el.name()
                        ),
                        child.span(),
                    );
                }
                table_spans.insert(table.key.clone(), child.span());
                doc.tables.insert(table.key, table.state);
            }
        } else if is(child, "checksum") {
            ctx.unsupported_element(child, el.name(), "a checksum to fix on save");
        } else if is(child, "simulation") {
            ctx.unsupported_element(child, el.name(), "simulated table data");
        } else {
            ctx.unknown_element(child, el.name());
        }
    }
    if base.is_some() && doc.romid.obsolete.is_unset() {
        doc.romid.obsolete = Field::Set(false);
    }
    let xmlid = match doc.romid.xmlid.get() {
        Some(x) if !x.is_empty() => x.clone(),
        _ => {
            let message = if romids == 0 {
                "<rom> has no <romid>; it cannot be named or identified".to_owned()
            } else {
                "<rom> has no <xmlid> (or an empty one) in its <romid>; it cannot be named"
                    .to_owned()
            };
            ctx.error(codes::MISSING_XMLID, message, el.span());
            return None;
        }
    };
    Some(RomEntry {
        source: Arc::clone(source),
        xmlid,
        span: el.span(),
        base,
        doc: Arc::new(doc),
        identification,
        table_spans,
    })
}
