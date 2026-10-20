//! Layered documents (concept C1): one inheritance engine.
//!
//! Every configurable thing is built the same way: from an ordered stack of
//! **partial documents**, each saying only what it changes. A ROM definition
//! sits on its `base` definition; a link profile is protocol → adapter → ECU
//! family → ECU ID (D-004); settings are defaults → user file → command line
//! (D-014). Legacy solved this four times, differently each time, and got it
//! subtly wrong: a child definition inherited a table's signedness only
//! while the base's storage type was unset
//! (MF-06, `xml/TableScaleUnmarshaller.java:153`), and one `omit="true"`
//! table ended the scan for every table after it (MF-07,
//! `xml/TableScaleUnmarshaller.java:330-333`).
//!
//! This module holds the types a partial document is made of and the one
//! merge rule for each, and the resolver that turns `base` / `extends`
//! references into a stack ([`Registry`], [`Resolver`]; see
//! [*Resolution*](#resolution)).
//!
//! # Layers
//!
//! A [`Layer`] is the metadata of one partial document: a stable
//! [`LayerId`], a [`LayerKind`] saying where it comes from, and the source
//! name and [`Span`] of the document, for provenance and diagnostics
//! ([`Layer::provenance`]). A [`Layered<P>`] is the stack: layers in
//! precedence order, weakest first, each document held in an [`Arc`] so a
//! base shared by many children is never deep-copied.
//! [`Layered::merged`] folds the stack into one document.
//!
//! # Fields and policies
//!
//! A partial document is a plain struct whose fields are **field states**;
//! the type of a field is its merge policy:
//!
//! | Field type | Policy | States |
//! |------------|--------|--------|
//! | [`Field<T>`] | replace: the overlay's value wins | unset / set / omit |
//! | [`Nested<P>`] | deep-merge: the overlay patches the inherited document | unset / patch / replace / omit |
//! | [`Keyed<K, P>`] | keyed list: per-item deep-merge, omit by key, new items appended | one [`Nested<P>`] per key |
//! | [`Appended<T>`] | append: the overlay's items follow the inherited ones | append / replace / omit |
//! | any `P: Merge` | deep-merge, cannot be omitted | as `P` |
//!
//! "Unset" (not mentioned: inherit) and "omit" (explicitly removed) are
//! different states, so a child can remove what its base defines.
//! Signedness is a [`Storage`](crate::encoding::Storage) in a
//! `Field<Storage>`, overridden like every other field (MF-06). Omitting one
//! item of a [`Keyed`] list removes that item and nothing else (MF-07).
//!
//! A struct of field states implements [`Merge`] field by field with
//! [`merge_fields!`](crate::merge_fields), which refuses to compile if a
//! field is left out: a field that silently does not inherit is the bug
//! class this module exists to remove.
//!
//! # Resolution
//!
//! A [`Document`] is a named partial document with an ordered list of
//! bases. A [`Resolver`] over a [`Registry`] of them linearises a name's
//! bases into a [`Lineage`] (a `Layered` stack holding every document it
//! inherits from, each once, by C3 linearisation), merges it, and
//! [completes](Complete) the result into the finished type, reporting a
//! missing required field, an unknown base or a cycle as a
//! [`ResolveError`], which [`ResolveError::to_diagnostic`] reports as an
//! Error diagnostic with a `LAY` code. Names are global across every file
//! the registry is filled from, so a base may be in any of them (MF-17); a
//! name defined twice is a [`DuplicateName`]. Names live in nested
//! [`Scope`]s: a base reference is looked up from the referrer's scope
//! outward, so a local name (a table-local scaling) is never visible
//! outside its scope (MF-16). A local document naming its own id as a base
//! extends the outer document of that name, as in legacy; any other local
//! name that shadows an outer one is listed by [`Registry::shadowed`] for a
//! warning. The walk is iterative,
//! registered documents are shared by `Arc`, and each lineage is computed
//! once per resolver. The rule and its reasons are in the [`resolve`]
//! module docs; its items are re-exported here.
//!
//! # Provenance
//!
//! [`Layered::explain`] (and [`Lineage::explain`], [`Resolved::explain`])
//! says which layer set one field and what it overrode: the winning
//! layer's [`Provenance`], then every earlier change, strongest first, for
//! a field named by a typed [`FieldPath`] such as
//! `tables["Boost Target"].scaling.units`. It walks the stack again for
//! that one field, so a merge records nothing and costs nothing extra. The
//! [`explain`] module docs have the model.
//!
//! # Laws
//!
//! For every type here, and every struct built from them with
//! `merge_fields!`, writing `a ⊕ b` for `a.merged(&b)` (`b` on top of `a`)
//! and `∅` for `Default::default()`:
//!
//! * **Pure**: `a ⊕ b` depends only on `a` and `b`. No I/O, no global
//!   state, no hash-order iteration; equal inputs give equal outputs.
//! * **Identity**: `∅ ⊕ a == a` and `a ⊕ ∅ == a`. An empty layer changes
//!   nothing, and resolving an already-resolved document again as one layer
//!   gives it back.
//! * **Associative**: `(a ⊕ b) ⊕ c == a ⊕ (b ⊕ c)`. A shared base can be
//!   merged once and reused under any number of children, and two overlays
//!   can be combined before they are applied, with the same result.
//! * **Override wins**: for a [`Field`], an overlay that is set or omitted
//!   is the result, whatever the base holds and whatever the other fields
//!   hold (MF-06).
//! * **Idempotent** for every policy except append: `a ⊕ a == a`. Appending
//!   a list to itself repeats its items, so a document with an
//!   [`Appended`] field is idempotent only where that field is empty or a
//!   replace.
//!
//! Merging is not commutative: the upper layer wins.
//!
//! The module holds no `static` of any kind (`cargo xtask check-globals`,
//! `STATELESS_PATHS` in `xtask/src/globals.rs`).
//!
//! # Example
//!
//! ```
//! use rr_core::diag::Span;
//! use rr_core::encoding::Storage;
//! use rr_core::layer::{Field, Keyed, Layer, LayerKind, Layered, Merge, Nested};
//!
//! #[derive(Debug, Clone, Default, PartialEq)]
//! struct Table {
//!     storage: Field<Storage>,
//!     address: Field<u32>,
//! }
//! rr_core::merge_fields!(Table { storage, address });
//!
//! #[derive(Debug, Clone, Default, PartialEq)]
//! struct Rom {
//!     tables: Keyed<String, Table>,
//! }
//! rr_core::merge_fields!(Rom { tables });
//!
//! let mut base = Rom::default();
//! for (name, addr) in [("A", 0x100), ("B", 0x200), ("C", 0x300)] {
//!     let t = Table { storage: Field::Set(Storage::U16), address: Field::Set(addr) };
//!     base.tables.patch(name.to_owned(), t);
//! }
//! let mut child = Rom::default();
//! let signed = Table { storage: Field::Set(Storage::I16), ..Table::default() };
//! child.tables.patch("A".to_owned(), signed);
//! child.tables.omit("B".to_owned());
//!
//! let file = |id: &str, line| {
//!     Layer::new(id.into(), LayerKind::File, format!("{id}.xml"), Span::line_col(line, 1))
//! };
//! let mut stack = Layered::new();
//! stack.push(file("16BITBASE", 3), base);
//! stack.push(file("A2WC522N", 2), child);
//!
//! let rom = stack.merged();
//! let tables: Vec<_> = rom
//!     .tables
//!     .present()
//!     .map(|(name, t)| (name.as_str(), t.storage.get().copied(), t.address.get().copied()))
//!     .collect();
//! assert_eq!(
//!     tables,
//!     [("A", Some(Storage::I16), Some(0x100)), ("C", Some(Storage::U16), Some(0x300))]
//! );
//! assert!(rom.tables.get(&"B".to_owned()).is_some_and(Nested::is_omitted));
//! ```

pub mod explain;
mod field;
mod list;
pub mod resolve;
#[cfg(test)]
mod tests;

use core::fmt;
use std::sync::Arc;

use crate::diag::{Provenance, Span};

pub use explain::{Action, Change, Explain, Explanation, FieldPath, Step};
pub use field::{Field, Nested};
pub use list::{Appended, Keyed};
pub use resolve::{
    Complete, Document, DuplicateName, Lineage, MAX_LINEAGE, Missing, Name, Registry, ResolveError,
    Resolved, Resolver, Scope, Shadowed,
};

/// The merge rule of a partial document: how one layer applies on top of
/// another.
///
/// `Default::default()` is the empty layer, the one that changes nothing.
/// Implementations obey the laws in the [module docs](self#laws); every type
/// in this module does, and so does a struct of them implemented with
/// [`merge_fields!`](crate::merge_fields).
pub trait Merge: Default {
    /// Applies `overlay` on top of `self`, in place: `self` is the lower
    /// (inherited) layer, `overlay` the upper one, which wins.
    fn merge_from(&mut self, overlay: &Self);

    /// `self` with `overlay` applied on top, as a new value.
    #[must_use]
    fn merged(&self, overlay: &Self) -> Self
    where
        Self: Clone,
    {
        let mut out = self.clone();
        out.merge_from(overlay);
        out
    }
}

/// Implements [`Merge`] for a struct of field states, field by field.
///
/// `merge_fields!(Type { a, b, c })` merges each listed field with its own
/// [`Merge`] impl, so the field's type is its policy. Every field of the
/// struct must be listed: the generated code destructures the overlay
/// without `..`, so a field left out is a compile error, not a field that
/// silently stops inheriting.
///
/// ```compile_fail,E0027
/// use rr_core::layer::Field;
///
/// #[derive(Default)]
/// struct Table {
///     storage: Field<u8>,
///     address: Field<u32>,
/// }
/// rr_core::merge_fields!(Table { storage }); // `address` is not merged
/// ```
///
/// A declarative macro rather than a derive: the policy lives in the field
/// types, so the macro only has to list the fields, and a derive would need
/// a proc-macro crate (and `syn` / `quote`) in `rr-core`'s dependency graph,
/// which takes no dependencies at all (ARCHITECTURE.md *Crate Map*).
#[macro_export]
macro_rules! merge_fields {
    ($ty:ty { $($field:ident),+ $(,)? }) => {
        impl $crate::layer::Merge for $ty {
            fn merge_from(&mut self, overlay: &Self) {
                let Self { $($field),+ } = overlay;
                $( $crate::layer::Merge::merge_from(&mut self.$field, $field); )+
            }
        }
    };
}

/// The stable identity of a [`Layer`]: a definition's `xmlid`, a profile
/// name, `defaults`. Compared and ordered as a string.
///
/// Cheap to clone (a shared string), since per-field provenance names the
/// layer a value came from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LayerId(Arc<str>);

impl LayerId {
    /// A layer id.
    #[must_use]
    pub fn new(id: impl Into<Arc<str>>) -> Self {
        Self(id.into())
    }

    /// The id as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for LayerId {
    fn from(id: &str) -> Self {
        Self::new(id)
    }
}

impl From<String> for LayerId {
    fn from(id: String) -> Self {
        Self::new(id)
    }
}

/// The id as text.
impl fmt::Display for LayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where a layer's document comes from.
///
/// Metadata for provenance and `explain`, never precedence: the order of a
/// [`Layered`] stack alone decides which layer wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum LayerKind {
    /// Compiled into the program: settings defaults, a protocol's default
    /// timing.
    Builtin,
    /// A document loaded from a definition pack: a ROM definition or its
    /// base, a link or device profile, a checksum spec, a logger parameter
    /// set.
    File,
    /// A file the user owns: the settings file, a user override of a pack.
    User,
    /// A command-line override.
    Cli,
}

impl LayerKind {
    /// Lower-case name: `builtin`, `file`, `user` or `cli`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::File => "file",
            Self::User => "user",
            Self::Cli => "cli",
        }
    }
}

/// The name, as [`LayerKind::as_str`].
impl fmt::Display for LayerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The metadata of one partial document in a [`Layered`] stack.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Layer {
    id: LayerId,
    kind: LayerKind,
    source: Arc<str>,
    span: Span,
}

impl Layer {
    /// A layer whose document is in `source` (a file path or other label,
    /// such as `<defaults>` or `--set`) at `span`, where the document starts
    /// (for a definition, its `<rom>` element).
    #[must_use]
    pub fn new(id: LayerId, kind: LayerKind, source: impl Into<Arc<str>>, span: Span) -> Self {
        Self {
            id,
            kind,
            source: source.into(),
            span,
        }
    }

    /// The layer's id.
    #[must_use]
    pub fn id(&self) -> &LayerId {
        &self.id
    }

    /// Where the document comes from.
    #[must_use]
    pub fn kind(&self) -> LayerKind {
        self.kind
    }

    /// The source name: a file path or other label.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Where the document starts in [`source`](Self::source).
    #[must_use]
    pub fn span(&self) -> Span {
        self.span
    }

    /// The document's provenance, naming this layer: the location a
    /// diagnostic about the document points at.
    #[must_use]
    pub fn provenance(&self) -> Provenance {
        Provenance::new(&*self.source, self.span).with_layer(self.id.as_str())
    }
}

/// An ordered stack of partial documents: `Layered<T>` of ARCHITECTURE.md
/// C1.
///
/// Layers are in precedence order, **weakest first**: each one applies on
/// top of everything pushed before it. Documents are held in an [`Arc`], so
/// a base shared by many stacks is pushed by reference, not copied.
#[derive(Debug, Clone, PartialEq)]
pub struct Layered<P> {
    layers: Vec<(Layer, Arc<P>)>,
}

impl<P> Layered<P> {
    /// An empty stack.
    #[must_use]
    pub const fn new() -> Self {
        Self { layers: Vec::new() }
    }

    /// Pushes `doc` as the new top layer, which wins over every layer below.
    /// `doc` is an owned document or an [`Arc`] of a shared one.
    pub fn push(&mut self, layer: Layer, doc: impl Into<Arc<P>>) {
        self.layers.push((layer, doc.into()));
    }

    /// The layers, weakest first.
    pub fn layers(&self) -> impl DoubleEndedIterator<Item = (&Layer, &Arc<P>)> + ExactSizeIterator {
        self.layers.iter().map(|(layer, doc)| (layer, doc))
    }

    /// The number of layers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    /// `true` if there are no layers.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }
}

impl<P: Merge + Clone> Layered<P> {
    /// Every layer merged in order: `∅ ⊕ l₀ ⊕ l₁ ⊕ … ⊕ lₙ`. An empty stack
    /// gives the empty document.
    #[must_use]
    pub fn merged(&self) -> P {
        let mut out = P::default();
        for (_, doc) in &self.layers {
            out.merge_from(doc);
        }
        out
    }
}

impl<P> Default for Layered<P> {
    fn default() -> Self {
        Self::new()
    }
}
