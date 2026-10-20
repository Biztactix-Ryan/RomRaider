//! The resolver: named partial documents with `base` / `extends`
//! references, linearised into a [`Layered`] stack and completed into a
//! typed value.
//!
//! # Model
//!
//! A [`Document`] is one partial document with a [`Name`] and an ordered
//! list of **bases** (a ROM definition's `base`, a profile's `extends`).
//! Documents are collected in a [`Registry`]; a [`Resolver`] turns a name
//! into its [`Lineage`] (the document and every document it inherits from,
//! as a `Layered` stack, weakest first, plus the merged partial) and
//! [`Resolver::resolve`] completes the merged partial into the finished
//! value ([`Complete`]).
//!
//! A document may name **several bases**, listed weakest first like every
//! stack in this module: the last base wins over the earlier ones, and the
//! document itself wins over all of them. Legacy ROM definitions have one
//! `base` (`xml/DOMRomUnmarshaller.java:149-152`), which is the one-base
//! case; link profiles compose independent axes (protocol, adapter, ECU
//! family, ECU ID; D-004), which is where a profile names two bases that
//! share an ancestor: a **diamond** (EC-04).
//!
//! # Linearisation
//!
//! The lineage of a document is its **C3 linearisation** (the rule behind
//! Python's method resolution order, and Dylan's), read weakest first.
//! Writing `S(X)` for a lineage read strongest first, for a document `D`
//! with bases `B₁ … Bₖ` (weakest first):
//!
//! ```text
//! S(D) = [D] + merge(S(Bₖ), …, S(B₁), [Bₖ, …, B₁])
//! ```
//!
//! where `merge` repeatedly takes the first list head that is in no list's
//! tail. What it guarantees, and why it is the rule here:
//!
//! * **Every document is above all of its own ancestors**, in every lineage
//!   it appears in, so an override always wins over what it overrides
//!   (MF-06, through chains of any length).
//! * **Bases keep the order they are listed in**: the later base wins.
//! * **Monotonic**: a base's own lineage appears inside every child's
//!   lineage in the same order, so inheriting from a base never changes how
//!   that base's ancestors combine. A plain depth-first walk keeping each
//!   document's first position has every other property here but not this
//!   one: in `D: [X, B]`, `X: [A2]`, `B: [A1, A2]` it puts `A1` above `A2`
//!   in `D` although `B` alone puts `A2` above `A1`.
//! * **A shared ancestor is applied once**, below everything that inherits
//!   from it (a diamond, EC-04). Applying it again higher up would undo a
//!   sibling's override of it, and would repeat its
//!   [`Appended`](super::Appended) items.
//! * **One base is exactly the legacy chain**: `L(D) = L(B) ++ [D]`.
//!
//! A graph no order can satisfy (`X: [A, B]`, `Y: [B, A]`, `D: [X, Y]`, or
//! one base listed twice) is a [`ResolveError::Inconsistent`], never an
//! arbitrary pick. A lineage holds at most [`MAX_LINEAGE`] documents
//! (definition packs are untrusted input); a larger one is
//! [`ResolveError::TooLarge`].
//!
//! The result depends only on the registry's contents, never on the order
//! documents were inserted: the registry is an ordered map, and the walk
//! follows each document's base list in order.
//!
//! # Sharing
//!
//! A registered document is held in an [`Arc`]; every lineage holds that
//! same `Arc`, never a copy (legacy deep-copied the base, via serialization,
//! for every table that named one: `xml/TableScaleUnmarshaller.java:87-92`).
//! A [`Resolver`] memoises each lineage it computes, also as an `Arc`, so a
//! base shared by many children is linearised and merged once per resolver,
//! and a child whose lineage starts with a base's (always, for a single
//! base) starts from that base's merged partial and applies only the layers
//! above it (associativity, [*Laws*](super#laws)).
//! The resolver is an explicit value owned by the caller: the module holds
//! no cache of its own (`cargo xtask check-globals`).
//!
//! # Iteration
//!
//! The base graph is walked with an explicit stack, not recursion, so a
//! chain thousands of documents deep, or a cycle, costs heap and time but
//! never overflows the call stack (MF-17: legacy recursed through
//! `getBaseRom` / `unmarshallRom`, `xml/DOMRomUnmarshaller.java:149-152`,
//! and caught the `StackOverflowError` a looped base caused,
//! `editor/ecu/OpenImageWorker.java:178-180`). A cycle is a
//! [`ResolveError::Cycle`] naming every document on it.
//!
//! # Names and scopes
//!
//! A [`Name`] is a [`LayerId`] inside a [`Scope`]: the global scope, or a
//! path of segments naming where a local document is defined, such as
//! global → ROM `A2WC400H` → table `Boost Target` (a table-local scaling
//! is `A2WC400H/Boost Target::<id>`). The reader that builds documents
//! chooses the scopes; the resolver gives no meaning to a segment beyond
//! its place in the path.
//!
//! A base reference is a bare id, looked up **lexically**, from the scope
//! of the document naming it outward ([`Registry::lookup`]): the document
//! with that id in the referrer's own scope, else in the scope enclosing
//! it, and so on out to the global scope. So a table's scaling can name a
//! scaling local to the same table, one local to its ROM, or a global
//! one, and the nearest wins. A local name is **never** visible from
//! outside its scope: not from the global scope, not from the scope
//! enclosing it, not from a sibling scope (another table of the same ROM).
//! A reference that finds nothing is an
//! [`UnknownBase`](ResolveError::UnknownBase), which says where the id
//! *is* defined when it exists only in scopes the referrer cannot see
//! (MF-16: legacy kept one flat map of scalings for the whole definition
//! being loaded and registered every table-local `<scaling>` in it as a
//! base for every scaling parsed after it, `xml/TableScaleUnmarshaller.java:276-277` and
//! `:460-461`, beside the file's `<scalingbase>` elements, `:65-77`).
//!
//! References cannot name a scope explicitly: every inheritance reference
//! in the legacy formats is a bare name (a ROM's `base`,
//! `xml/DOMRomUnmarshaller.java:149-152`; a table's `base`,
//! `xml/TableScaleUnmarshaller.java:87-92`; a scaling's `base`, `:415-421`),
//! and so are profile `extends` (D-004). A request from outside the
//! definitions ([`Resolver::lineage`]) names a document exactly, by its
//! full [`Name`].
//!
//! **A base with the document's own id extends the outer definition.** A
//! reference to the referrer's own id skips the referrer itself and is
//! looked up from the scope enclosing it outward ([`Registry::base_of`]), so
//! a table-local `X` with `base="X"` derives from the ROM's or the global
//! `X`, which it then shadows inside its scope: "this table's `X` is the
//! usual `X`, except ...". That is legacy's meaning. A scaling's `base` was
//! looked up in the shared scalings map
//! (`xml/TableScaleUnmarshaller.java:415-421`) before the scaling being
//! parsed was put in it (`:460-461`), so `base` with its own name found the
//! previously defined scaling of that name; a table's `base` was looked up
//! among the tables the ROM already had (`rom.getTableByName`, `:87-92`),
//! before the table being parsed was added
//! (`xml/DOMRomUnmarshaller.java:168-176`), so it found the base ROM's table
//! of that name. With no outer definition the base is an
//! [`UnknownBase`](ResolveError::UnknownBase) saying so, never a cycle: a
//! global document (a ROM whose `base` is its own `xmlid`) has nothing
//! outside it to extend. (Legacy looked such a ROM up among the `<rom>`
//! elements of its file, found itself, and recursed until the stack
//! overflowed, `xml/DOMRomUnmarshaller.java:149-152`, `:201-210`.)
//!
//! Every other reference finds a document in the referrer's own scope or
//! an enclosing one, and a reference to the own id finds one strictly
//! outside it, so a base chain never moves inward: **every cycle lies
//! within one scope** and holds at least two documents. A local `X`
//! extending an outer `X` whose chain loops is reported with the outer
//! loop's path; the local `X` is not on it.
//!
//! **Shadowing is allowed, and reported as a warning.** A local document
//! with the id of a document in an enclosing scope hides that document
//! from every reference inside its scope, and only there; outside, the id
//! still means the outer document. [`Registry::shadowed`] lists every such
//! document that does not extend the one it hides, reported as a
//! [`Warning`](crate::diag::Severity::Warning)
//! ([`SHADOWED_NAME`](codes::SHADOWED_NAME)). A document naming its own id
//! as a base is not listed: it says in its own text that it is a variant of
//! the outer definition, so inside its scope every reference to the id still
//! gets everything the outer document defines, overridden only where the
//! local one says. That is how a pack adjusts one table's copy of a shared
//! scaling, and a warning on every such table would bury the shadowing that
//! is accidental. Not an error, since a local
//! definition taking precedence inside its scope is the point of a scope;
//! not silent, since legacy had one flat namespace in which the later
//! definition replaced the earlier one for every lookup after it
//! (`HashMap.put`, `xml/TableScaleUnmarshaller.java:461`), so a pack written
//! against that resolves differently here exactly where a name is
//! shadowed. Two documents with one name in the *same* scope are a
//! [`DuplicateName`] error, as below.
//!
//! **Case.** Names are compared exactly, byte for byte, in every scope; the
//! resolver never folds case, and neither does [`LayerId`]. Legacy names
//! are case-insensitive: scaling names were lower-cased both when stored
//! and when looked up (`xml/TableScaleUnmarshaller.java:419-421`, `:461`),
//! and a ROM's `xmlid` was matched with `equalsIgnoreCase`
//! (`xml/DOMRomUnmarshaller.java:127`). Folding is the reader's job: a
//! reader of a format whose names ignore case folds every id (documents
//! and references alike) before building names, keeping the original
//! spelling for display in the layer's source text. The fold that matches
//! a format is that format's to choose, and names in other formats
//! (settings keys, profile names) are case-sensitive.
//!
//! # Files
//!
//! A registry holds the documents of every loaded file, and names are
//! **global across files**: a base reference finds its document in whatever
//! file defines it, so a ROM definition in one file can sit on a base in
//! another (MF-17). The file a document came from is its layer's
//! [`source`](Layer::source), used for provenance and nothing else; which
//! file defines a base, and how the documents are split between files,
//! never changes a result.
//!
//! Legacy looked a `base` up only in the file of the definition naming it
//! (`getBaseRom` searches `rootNode.getParentNode()`, the same file's
//! document, `xml/DOMRomUnmarshaller.java:149-152`; a base anywhere else was
//! a `RomNotFoundException`, `:201-213`). Splitting a pack into files was
//! possible only with XInclude splicing them into one document before any
//! lookup (`editor/ecu/OpenImageWorker.java:110`).
//!
//! **A name defined twice is an error.** [`Registry::insert`] keeps the
//! first document and refuses the second with a [`DuplicateName`], reported
//! as an [`Error`](crate::diag::Severity::Error)
//! ([`DUPLICATE_DOCUMENT`](codes::DUPLICATE_DOCUMENT), naming both
//! locations), so the owning load fails closed unless the caller accepts the
//! error, and then gets the first document. Legacy picked silently: it
//! scanned the definition files in the user's settings order and stopped at
//! the first that matched (`editor/ecu/OpenImageWorker.java:219-250`), and
//! within a file took the first `<rom>` whose `xmlid` matched
//! (`xml/DOMRomUnmarshaller.java:97-138`), so reordering the definition list
//! or the file changed which tables a ROM got, without a word. Keeping the
//! first rather than the later document is legacy's choice; reporting it as
//! an error is not, because a duplicate makes every reference to the name
//! mean whatever happened to load first.
//!
//! Names are compared exactly (*Names and scopes*, **Case**).
//!
//! # Diagnostics
//!
//! [`ResolveError::to_diagnostic`] and [`DuplicateName::to_diagnostic`]
//! report a failure as an [`Error`](crate::diag::Severity::Error)
//! [`Diagnostic`] with a `LAY` code ([`ResolveError::code`]) at the document
//! at fault. A cycle names every document on it with its file, in base
//! order: `inheritance cycle: a (a.xml:2:1) -> b (b.xml:2:1) -> a`.
//! [`Shadowed::to_diagnostic`] reports a shadowing local document as a
//! [`Warning`](crate::diag::Severity::Warning) at that document.
//!
//! # Example
//!
//! ```
//! use rr_core::diag::Span;
//! use rr_core::layer::{Appended, Document, Field, Layer, LayerKind, Name, Registry};
//!
//! #[derive(Debug, Clone, Default, PartialEq)]
//! struct Profile {
//!     baud: Field<u32>,
//!     notes: Appended<&'static str>,
//! }
//! rr_core::merge_fields!(Profile { baud, notes });
//!
//! let doc = |id: &str, baud: Option<u32>, note| {
//!     let layer = Layer::new(id.into(), LayerKind::File, format!("{id}.toml"), Span::line_col(1, 1));
//!     let baud = baud.map_or(Field::Unset, Field::Set);
//!     Document::new(layer, Profile { baud, notes: Appended::append(vec![note]) })
//! };
//! // A diamond: `ecu` extends `kline` and `openport`, which both extend `ssm`.
//! let mut registry = Registry::new();
//! for d in [
//!     doc("ssm", Some(4800), "ssm"),
//!     doc("kline", None, "kline").with_base("ssm"),
//!     doc("openport", Some(10400), "openport").with_base("ssm"),
//!     doc("ecu", None, "ecu").with_base("kline").with_base("openport"),
//! ] {
//!     registry.insert(d).unwrap();
//! }
//!
//! let mut resolver = registry.resolver();
//! let ecu = resolver.lineage(&Name::global("ecu")).unwrap();
//! let order: Vec<_> = ecu.names().iter().map(ToString::to_string).collect();
//! assert_eq!(order, ["ssm", "kline", "openport", "ecu"]);
//! assert_eq!(ecu.merged().baud, Field::Set(10400));
//! assert_eq!(ecu.merged().notes.items(), ["ssm", "kline", "openport", "ecu"]);
//! ```

use core::fmt;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use super::{Explain, Explanation, Field, FieldPath, Layer, LayerId, Layered, Merge};
use crate::diag::{Code, Diagnostic, Provenance, codes};

#[cfg(test)]
mod tests;

/// A namespace for document names: the global scope, or a path of
/// segments naming where a local document is defined (a ROM, then a table
/// in it).
///
/// Ordered and compared segment by segment; the global scope is the empty
/// path and sorts first. There is no `Default`: a name's scope is always
/// chosen, never implied.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Scope(Arc<[Arc<str>]>);

impl Scope {
    /// The global scope.
    #[must_use]
    pub fn global() -> Self {
        Self(Arc::from(Vec::new()))
    }

    /// `true` for the global scope.
    #[must_use]
    pub fn is_global(&self) -> bool {
        self.0.is_empty()
    }

    /// The scope named `segment` inside this one.
    #[must_use]
    pub fn child(&self, segment: impl Into<Arc<str>>) -> Self {
        let mut path = self.0.to_vec();
        path.push(segment.into());
        Self(path.into())
    }

    /// The path, outermost segment first. Empty for the global scope.
    pub fn segments(&self) -> impl ExactSizeIterator<Item = &str> {
        self.0.iter().map(|s| &**s)
    }

    /// The number of segments: 0 for the global scope.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.0.len()
    }

    /// `true` if `inner` is this scope or inside it: this scope's path is a
    /// prefix of `inner`'s. The global scope encloses every scope; a scope
    /// does not enclose its siblings or the scopes enclosing it.
    #[must_use]
    pub fn encloses(&self, inner: &Self) -> bool {
        inner.0.starts_with(&self.0)
    }
}

/// The path as `a/b`; empty for the global scope.
impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, segment) in self.segments().enumerate() {
            let sep = if i == 0 { "" } else { "/" };
            write!(f, "{sep}{segment}")?;
        }
        Ok(())
    }
}

/// The name of a [`Document`]: a [`LayerId`] in a [`Scope`].
///
/// Renders as the id for a global name and as `a/b::id` for a name in
/// scope `a/b`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name {
    scope: Scope,
    id: LayerId,
}

impl Name {
    /// `id` in `scope`.
    #[must_use]
    pub fn new(scope: Scope, id: impl Into<LayerId>) -> Self {
        Self {
            scope,
            id: id.into(),
        }
    }

    /// `id` in the global scope.
    #[must_use]
    pub fn global(id: impl Into<LayerId>) -> Self {
        Self::new(Scope::global(), id)
    }

    /// The scope.
    #[must_use]
    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    /// The id within the scope.
    #[must_use]
    pub fn id(&self) -> &LayerId {
        &self.id
    }
}

/// A global name.
impl From<&str> for Name {
    fn from(id: &str) -> Self {
        Self::global(id)
    }
}

/// A global name.
impl From<LayerId> for Name {
    fn from(id: LayerId) -> Self {
        Self::global(id)
    }
}

/// `id`, or `a/b::id` in scope `a/b`.
impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.scope.is_global() {
            write!(f, "{}::", self.scope)?;
        }
        write!(f, "{}", self.id)
    }
}

/// One named partial document and the documents it inherits from.
///
/// Its bases are bare ids, looked up from the document's own scope outward
/// (module docs, *Names and scopes*).
#[derive(Debug, Clone, PartialEq)]
pub struct Document<P> {
    name: Name,
    layer: Layer,
    bases: Vec<LayerId>,
    doc: Arc<P>,
}

impl<P> Document<P> {
    /// The document `doc`, named by `layer`'s id in the global scope, with no
    /// bases. `doc` is an owned document or an [`Arc`] of a shared one.
    #[must_use]
    pub fn new(layer: Layer, doc: impl Into<Arc<P>>) -> Self {
        Self {
            name: Name::global(layer.id().clone()),
            layer,
            bases: Vec::new(),
            doc: doc.into(),
        }
    }

    /// The same document, named in `scope` instead.
    #[must_use]
    pub fn in_scope(mut self, scope: Scope) -> Self {
        self.name.scope = scope;
        self
    }

    /// The same document with `base` added as its strongest base so far:
    /// bases are listed weakest first. `base` is an id, found in the
    /// document's scope or the nearest enclosing scope that has it; the
    /// document's own id is found in the nearest scope *outside* the
    /// document's that has it ([`Registry::base_of`]).
    #[must_use]
    pub fn with_base(mut self, base: impl Into<LayerId>) -> Self {
        self.bases.push(base.into());
        self
    }

    /// The document's name.
    #[must_use]
    pub fn name(&self) -> &Name {
        &self.name
    }

    /// The document's layer metadata.
    #[must_use]
    pub fn layer(&self) -> &Layer {
        &self.layer
    }

    /// The bases, weakest first, as named: ids, not yet looked up.
    #[must_use]
    pub fn bases(&self) -> &[LayerId] {
        &self.bases
    }

    /// The partial document.
    #[must_use]
    pub fn doc(&self) -> &Arc<P> {
        &self.doc
    }
}

/// Every [`Document`] that names can resolve to, by [`Name`].
///
/// Held by id, then scope, so a lexical lookup ([`lookup`](Self::lookup))
/// sees every scope an id is defined in at once.
#[derive(Debug, Clone, PartialEq)]
pub struct Registry<P> {
    docs: BTreeMap<LayerId, BTreeMap<Scope, Document<P>>>,
    len: usize,
}

impl<P> Registry<P> {
    /// An empty registry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            docs: BTreeMap::new(),
            len: 0,
        }
    }

    /// Adds `doc` under its name. Names are global across files: `doc` may
    /// name bases from any file, and documents from any file may name it
    /// (module docs, *Files*). A document with the id of one in an
    /// enclosing or enclosed scope is accepted, and listed by
    /// [`shadowed`](Self::shadowed).
    ///
    /// # Errors
    ///
    /// [`DuplicateName`] if a document of that name (the same id in the same
    /// scope) is already registered, from any file. The registry keeps the
    /// first one, so the result does not depend on which duplicate comes
    /// later; the error is reported as an
    /// [`Error`](crate::diag::Severity::Error)
    /// ([`DuplicateName::to_diagnostic`]).
    pub fn insert(&mut self, doc: Document<P>) -> Result<(), Box<DuplicateName>> {
        let scopes = self.docs.entry(doc.name.id.clone()).or_default();
        match scopes.get(&doc.name.scope) {
            Some(kept) => Err(Box::new(DuplicateName {
                name: doc.name,
                kept: kept.layer.clone(),
                rejected: doc.layer,
            })),
            None => {
                scopes.insert(doc.name.scope.clone(), doc);
                self.len += 1;
                Ok(())
            }
        }
    }

    /// The document named `name`, exactly: that id in that scope.
    #[must_use]
    pub fn get(&self, name: &Name) -> Option<&Document<P>> {
        self.docs.get(&name.id)?.get(&name.scope)
    }

    /// The document a reference to `id` from scope `from` finds: the one
    /// with that id in `from`, else in the nearest scope enclosing `from`
    /// that has one, out to the global scope. A document in any other scope
    /// (inside `from`, or beside it) is never found (MF-16).
    #[must_use]
    pub fn lookup(&self, from: &Scope, id: &LayerId) -> Option<&Document<P>> {
        self.docs
            .get(id)?
            .iter()
            .filter(|(scope, _)| scope.encloses(from))
            .max_by_key(|(scope, _)| scope.depth())
            .map(|(_, doc)| doc)
    }

    /// The document a base reference to `id` in the document named
    /// `referrer` finds: [`lookup`](Self::lookup) from the referrer's scope,
    /// except that a reference to the referrer's own id skips the referrer
    /// and finds the nearest document with that id in a scope enclosing the
    /// referrer's, out to the global scope: the outer definition the
    /// referrer extends (module docs, *Names and scopes*). `None` if there
    /// is none.
    #[must_use]
    pub fn base_of(&self, referrer: &Name, id: &LayerId) -> Option<&Document<P>> {
        let own = (*id == referrer.id).then_some(&referrer.scope);
        self.docs
            .get(id)?
            .iter()
            .filter(|(scope, _)| scope.encloses(&referrer.scope) && Some(*scope) != own)
            .max_by_key(|(scope, _)| scope.depth())
            .map(|(_, doc)| doc)
    }

    /// The names of the documents with `id` that a reference from `from`
    /// cannot see, in name order.
    fn hidden(&self, from: &Scope, id: &LayerId) -> Vec<Name> {
        self.docs
            .get(id)
            .into_iter()
            .flat_map(BTreeMap::values)
            .filter(|doc| !doc.name.scope.encloses(from))
            .map(|doc| doc.name.clone())
            .collect()
    }

    /// Every document whose id is also defined in an enclosing scope, with
    /// the document it shadows there (the nearest), in id order, then scope
    /// order, except a document naming its own id as a base, which extends
    /// the outer document rather than hiding it (module docs, *Names and
    /// scopes*): one [`Shadowed`] per shadowing document, reported as a
    /// [`Warning`](crate::diag::Severity::Warning)
    /// ([`Shadowed::to_diagnostic`]). Depends only on the registry's
    /// contents, never on insertion order.
    #[must_use]
    pub fn shadowed(&self) -> Vec<Shadowed> {
        let mut out = Vec::new();
        for scopes in self.docs.values() {
            for (scope, doc) in scopes {
                if doc.bases.contains(&doc.name.id) {
                    continue;
                }
                let outer = scopes
                    .iter()
                    .filter(|(s, _)| s.depth() < scope.depth() && s.encloses(scope))
                    .max_by_key(|(s, _)| s.depth());
                if let Some((_, outer)) = outer {
                    out.push(Shadowed {
                        name: doc.name.clone(),
                        layer: doc.layer.clone(),
                        shadows: outer.name.clone(),
                        outer: outer.layer.clone(),
                    });
                }
            }
        }
        out
    }

    /// Every document, in id order, then scope order (the global scope
    /// first).
    pub fn documents(&self) -> impl Iterator<Item = &Document<P>> {
        self.docs.values().flat_map(BTreeMap::values)
    }

    /// The number of documents.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// `true` if there are no documents.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<P: Merge + Clone> Registry<P> {
    /// A [`Resolver`] over this registry, with an empty cache.
    #[must_use]
    pub fn resolver(&self) -> Resolver<'_, P> {
        Resolver::new(self)
    }
}

impl<P> Default for Registry<P> {
    fn default() -> Self {
        Self::new()
    }
}

/// [`Registry::insert`] of a name that is already registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateName {
    /// The name.
    pub name: Name,
    /// The document that stays registered.
    pub kept: Layer,
    /// The document that was not added.
    pub rejected: Layer,
}

impl DuplicateName {
    /// Where the rejected document is.
    #[must_use]
    pub fn provenance(&self) -> Provenance {
        self.rejected.provenance()
    }

    /// The registry code this is reported with:
    /// [`DUPLICATE_DOCUMENT`](codes::DUPLICATE_DOCUMENT).
    #[must_use]
    pub const fn code(&self) -> Code {
        codes::DUPLICATE_DOCUMENT
    }

    /// This as an [`Error`](crate::diag::Severity::Error) diagnostic at the
    /// rejected document, the message naming where the kept one is.
    #[must_use]
    pub fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic::error(self.code(), self.to_string(), self.provenance())
    }
}

impl fmt::Display for DuplicateName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is already defined in {}; this definition is ignored",
            self.name,
            self.kept.provenance()
        )
    }
}

impl std::error::Error for DuplicateName {}

/// A local document with the id of a document in an enclosing scope
/// ([`Registry::shadowed`]): inside its scope, a reference to the id finds
/// it instead of the outer one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shadowed {
    /// The shadowing document's name.
    pub name: Name,
    /// Its layer.
    pub layer: Layer,
    /// The document it shadows: the same id in the nearest enclosing scope
    /// that has it.
    pub shadows: Name,
    /// That document's layer.
    pub outer: Layer,
}

impl Shadowed {
    /// Where the shadowing document is.
    #[must_use]
    pub fn provenance(&self) -> Provenance {
        self.layer.provenance()
    }

    /// The registry code this is reported with:
    /// [`SHADOWED_NAME`](codes::SHADOWED_NAME).
    #[must_use]
    pub const fn code(&self) -> Code {
        codes::SHADOWED_NAME
    }

    /// This as a [`Warning`](crate::diag::Severity::Warning) diagnostic at
    /// the shadowing document, the message naming the shadowed one and
    /// where it is.
    #[must_use]
    pub fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic::warning(self.code(), self.to_string(), self.provenance())
    }
}

impl fmt::Display for Shadowed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` shadows `{}` ({}): inside `{}`, `{}` means this definition",
            self.name,
            self.shadows,
            self.outer.provenance(),
            self.name.scope,
            self.name.id
        )
    }
}

/// A document and everything it inherits from, linearised (module docs,
/// *Linearisation*), with the stack merged.
#[derive(Debug, Clone, PartialEq)]
pub struct Lineage<P> {
    name: Name,
    layer: Layer,
    names: Vec<Name>,
    stack: Layered<P>,
    merged: Arc<P>,
}

impl<P> Lineage<P> {
    /// The document this is the lineage of.
    #[must_use]
    pub fn name(&self) -> &Name {
        &self.name
    }

    /// That document's layer metadata.
    #[must_use]
    pub fn layer(&self) -> &Layer {
        &self.layer
    }

    /// The name of each layer of [`stack`](Self::stack), in the same order:
    /// weakest first, the document itself last. Each name appears once.
    #[must_use]
    pub fn names(&self) -> &[Name] {
        &self.names
    }

    /// The linearised stack, weakest first; each layer's document is the
    /// registered [`Arc`], not a copy.
    #[must_use]
    pub fn stack(&self) -> &Layered<P> {
        &self.stack
    }

    /// The stack merged: equal to [`Layered::merged`] of
    /// [`stack`](Self::stack).
    #[must_use]
    pub fn merged(&self) -> &Arc<P> {
        &self.merged
    }

    /// Which layers of the lineage set the field at `path`, and what each
    /// one overrode: [`Layered::explain`] of [`stack`](Self::stack).
    #[must_use]
    pub fn explain<L: Explain>(&self, path: &FieldPath<P, L>) -> Explanation<'_, L> {
        self.stack.explain(path)
    }
}

/// The completion step of resolution: turning a merged partial document
/// into the finished value.
///
/// A required field that no layer set is reported in [`Missing`], never a
/// panic.
pub trait Complete: Merge + Clone {
    /// The finished value.
    type Output;

    /// The finished value, or every required field that is missing.
    ///
    /// # Errors
    ///
    /// [`Missing`] naming each required field that is unset or omitted.
    fn complete(&self) -> Result<Self::Output, Missing>;
}

/// The required fields a merged document lacks ([`Complete::complete`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Missing {
    fields: Vec<String>,
}

impl Missing {
    /// No fields missing yet.
    #[must_use]
    pub const fn new() -> Self {
        Self { fields: Vec::new() }
    }

    /// Records the field at `path` (`address`, `tables[A].storage`) as
    /// missing.
    pub fn push(&mut self, path: impl Into<String>) {
        self.fields.push(path.into());
    }

    /// The value of `field` if it is set; otherwise records `path` as
    /// missing and gives `None`.
    pub fn require<'a, T>(&mut self, path: &str, field: &'a Field<T>) -> Option<&'a T> {
        let value = field.get();
        if value.is_none() {
            self.push(path);
        }
        value
    }

    /// The missing fields, in the order recorded.
    #[must_use]
    pub fn fields(&self) -> &[String] {
        &self.fields
    }

    /// `true` if no field is missing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

/// `missing required field(s) `a`, `b``.
impl fmt::Display for Missing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("missing required field(s)")?;
        for (i, field) in self.fields.iter().enumerate() {
            let sep = if i == 0 { " " } else { ", " };
            write!(f, "{sep}`{field}`")?;
        }
        Ok(())
    }
}

impl std::error::Error for Missing {}

/// The most documents one [`Lineage`] may hold: a document and everything
/// it inherits from. Real inheritance is a handful of levels deep (a ROM
/// definition on a family base on a 16- or 32-bit base; four profile axes,
/// D-004); the limit bounds the work and memory an untrusted definition
/// pack can demand.
pub const MAX_LINEAGE: usize = 256;

/// Why a name could not be resolved.
///
/// Each variant that concerns a document carries that document's [`Layer`],
/// so it can be reported as a diagnostic at the document
/// ([`provenance`](Self::provenance)).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResolveError {
    /// No document has the requested name.
    Unknown {
        /// The requested name.
        name: Name,
    },
    /// A document names a base that no document in its scope or an
    /// enclosing one has, or names its own id and no enclosing scope
    /// defines it ([`Registry::base_of`]).
    UnknownBase {
        /// The document naming the base.
        document: Name,
        /// Its layer.
        layer: Layer,
        /// The base it names.
        base: LayerId,
        /// The documents with that id in scopes the document cannot see
        /// (MF-16), in name order; empty if no document has the id. Boxed,
        /// to keep the error small.
        hidden: Box<[Name]>,
    },
    /// Following bases leads back to a document already being resolved.
    Cycle {
        /// The documents on the cycle, in base order, the first repeated at
        /// the end: `[A, B, A]` for `A` → `B` → `A`. At least two documents,
        /// all in one scope: a base with the document's own id is never the
        /// document itself (module docs, *Names and scopes*).
        path: Vec<Name>,
        /// The layer of each document on the cycle, in `path` order without
        /// the repeat (one fewer than `path`): where each one is, in
        /// whichever file.
        layers: Vec<Layer>,
        /// The layer of the document whose base closes the cycle (the
        /// next-to-last name of `path`, the last of `layers`).
        layer: Layer,
    },
    /// A document's bases cannot be linearised: its base list, or its
    /// bases' own lineages, order some documents both ways (module docs,
    /// *Linearisation*).
    Inconsistent {
        /// The document whose bases conflict.
        document: Name,
        /// Its layer.
        layer: Layer,
        /// The documents that could not be ordered: the candidates left
        /// when the linearisation stopped, strongest first.
        conflict: Vec<Name>,
    },
    /// A lineage would hold more than [`MAX_LINEAGE`] documents.
    TooLarge {
        /// The document whose lineage is too large.
        document: Name,
        /// Its layer.
        layer: Layer,
    },
    /// The merged document lacks required fields.
    Incomplete {
        /// The document being resolved.
        document: Name,
        /// Its layer.
        layer: Layer,
        /// The missing fields.
        missing: Missing,
    },
}

impl ResolveError {
    /// Where to report the error: the document whose bases or fields are at
    /// fault. `None` for an unknown requested name, which no document
    /// mentions.
    #[must_use]
    pub fn provenance(&self) -> Option<Provenance> {
        match self {
            Self::Unknown { .. } => None,
            Self::UnknownBase { layer, .. }
            | Self::Cycle { layer, .. }
            | Self::Inconsistent { layer, .. }
            | Self::TooLarge { layer, .. }
            | Self::Incomplete { layer, .. } => Some(layer.provenance()),
        }
    }

    /// The registry code the error is reported with: one per variant, in
    /// the `LAY` domain.
    #[must_use]
    pub const fn code(&self) -> Code {
        match self {
            Self::Unknown { .. } => codes::UNKNOWN_DOCUMENT,
            Self::UnknownBase { .. } => codes::UNKNOWN_BASE,
            Self::Cycle { .. } => codes::INHERITANCE_CYCLE,
            Self::Inconsistent { .. } => codes::INCONSISTENT_BASES,
            Self::TooLarge { .. } => codes::LINEAGE_TOO_LARGE,
            Self::Incomplete { .. } => codes::INCOMPLETE_DOCUMENT,
        }
    }

    /// This error as an [`Error`](crate::diag::Severity::Error) diagnostic,
    /// with [`code`](Self::code) and the `Display` message, at
    /// [`provenance`](Self::provenance): the document at fault (for a
    /// cycle, the one whose base closes it).
    ///
    /// `request` is where the name being resolved was asked for (the ROM
    /// image being opened, a command-line argument): the location of an
    /// [`Unknown`](Self::Unknown) name, which no document mentions. Every
    /// other variant ignores it.
    #[must_use]
    pub fn to_diagnostic(&self, request: Provenance) -> Diagnostic {
        let at = self.provenance().unwrap_or(request);
        Diagnostic::error(self.code(), self.to_string(), at)
    }
}

/// Writes `names` separated by `sep`.
fn write_names(f: &mut fmt::Formatter<'_>, names: &[Name], sep: &str) -> fmt::Result {
    for (i, name) in names.iter().enumerate() {
        let sep = if i == 0 { "" } else { sep };
        write!(f, "{sep}{name}")?;
    }
    Ok(())
}

/// How many scopes [`write_hidden`] names before a count of the rest.
const HIDDEN_SHOWN: usize = 3;

/// Writes, if `hidden` is not empty, where `id` is defined instead:
/// `` here: `id` is defined only inside `a/b`, `c`; not visible from `doc` ``,
/// naming at most [`HIDDEN_SHOWN`] scopes so the message stays bounded.
fn write_hidden(
    f: &mut fmt::Formatter<'_>,
    document: &Name,
    id: &LayerId,
    hidden: &[Name],
) -> fmt::Result {
    if hidden.is_empty() {
        return Ok(());
    }
    write!(f, " here: `{id}` is defined only inside ")?;
    for (i, name) in hidden.iter().take(HIDDEN_SHOWN).enumerate() {
        let sep = if i == 0 { "" } else { ", " };
        write!(f, "{sep}`{}`", name.scope)?;
    }
    if hidden.len() > HIDDEN_SHOWN {
        write!(f, " and {} more scopes", hidden.len() - HIDDEN_SHOWN)?;
    }
    write!(f, "; not visible from `{document}`")
}

/// How many documents of a cycle [`write_cycle`] names at each end of a
/// cycle longer than [`MAX_LINEAGE`].
const CYCLE_SHOWN: usize = MAX_LINEAGE / 2;

/// Writes the cycle `path` as `a (a.xml:2:1) -> b (b.xml:7:1) -> a`: each
/// document on it with where it is (`layers`), then the first again.
///
/// A cycle of up to [`MAX_LINEAGE`] documents is written in full. A longer
/// one (possible only in a crafted pack: no document may inherit from that
/// many) is written as its first and last [`CYCLE_SHOWN`] documents around
/// a count of the rest, so the message stays bounded; the error's `path`
/// still holds every document.
fn write_cycle(f: &mut fmt::Formatter<'_>, path: &[Name], layers: &[Layer]) -> fmt::Result {
    let n = layers.len();
    let elide = n > MAX_LINEAGE;
    for (i, (name, layer)) in path.iter().zip(layers).enumerate() {
        if elide && i == CYCLE_SHOWN {
            write!(f, "... {} more ... -> ", n - 2 * CYCLE_SHOWN)?;
        }
        if elide && (CYCLE_SHOWN..n - CYCLE_SHOWN).contains(&i) {
            continue;
        }
        write!(f, "{name} ({}{}) -> ", layer.source(), layer.span())?;
    }
    path.last().map_or(Ok(()), |first| write!(f, "{first}"))
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown { name } => write!(f, "no document named `{name}`"),
            Self::UnknownBase {
                document,
                base,
                hidden,
                ..
            } => {
                if base == document.id() {
                    write!(
                        f,
                        "`{document}` names its own id `{base}` as its base, which means \
                         the `{base}` of an enclosing scope, and none defines one"
                    )?;
                } else {
                    write!(f, "`{document}` names base `{base}`, which is not defined")?;
                }
                write_hidden(f, document, base, hidden)
            }
            Self::Cycle { path, layers, .. } => {
                f.write_str("inheritance cycle: ")?;
                write_cycle(f, path, layers)
            }
            Self::Inconsistent {
                document, conflict, ..
            } => {
                write!(
                    f,
                    "the bases of `{document}` cannot be ordered: their own bases order "
                )?;
                write_names(f, conflict, ", ")?;
                f.write_str(" both ways")
            }
            Self::TooLarge { document, .. } => write!(
                f,
                "`{document}` inherits from more than {} documents",
                MAX_LINEAGE - 1
            ),
            Self::Incomplete {
                document, missing, ..
            } => write!(f, "`{document}` is incomplete: {missing}"),
        }
    }
}

impl std::error::Error for ResolveError {}

/// A resolved document: the finished value and the lineage it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved<T, P> {
    value: T,
    lineage: Arc<Lineage<P>>,
}

impl<T, P> Resolved<T, P> {
    /// The finished value.
    #[must_use]
    pub fn value(&self) -> &T {
        &self.value
    }

    /// The finished value, without the lineage.
    #[must_use]
    pub fn into_value(self) -> T {
        self.value
    }

    /// The lineage the value was completed from: the stack every field's
    /// winning layer can be found in.
    #[must_use]
    pub fn lineage(&self) -> &Arc<Lineage<P>> {
        &self.lineage
    }

    /// Which layers set the field at `path`, and what each one overrode:
    /// [`Lineage::explain`].
    #[must_use]
    pub fn explain<L: Explain>(&self, path: &FieldPath<P, L>) -> Explanation<'_, L> {
        self.lineage.explain(path)
    }
}

/// One layer of a lineage under construction: its name, metadata and
/// document.
type Entry<'a, P> = (&'a Name, &'a Layer, &'a Arc<P>);

/// Resolves names in a [`Registry`], memoising every lineage it computes.
///
/// Created by [`Registry::resolver`]. The cache lives as long as the
/// resolver and is shared by every name it resolves: a base common to many
/// documents is linearised and merged once.
#[derive(Debug)]
pub struct Resolver<'r, P> {
    registry: &'r Registry<P>,
    memo: BTreeMap<Name, Arc<Lineage<P>>>,
}

impl<'r, P: Merge + Clone> Resolver<'r, P> {
    /// A resolver over `registry`, with an empty cache.
    #[must_use]
    pub fn new(registry: &'r Registry<P>) -> Self {
        Self {
            registry,
            memo: BTreeMap::new(),
        }
    }

    /// The lineage of `name`.
    ///
    /// # Errors
    ///
    /// [`ResolveError::Unknown`] if `name` is not registered;
    /// [`ResolveError::UnknownBase`], [`ResolveError::Cycle`],
    /// [`ResolveError::Inconsistent`] or [`ResolveError::TooLarge`] for the
    /// first document in its lineage, in walk order, whose bases are at
    /// fault.
    pub fn lineage(&mut self, name: &Name) -> Result<Arc<Lineage<P>>, ResolveError> {
        if let Some(done) = self.memo.get(name) {
            return Ok(Arc::clone(done));
        }
        let registry = self.registry;
        let root = registry
            .get(name)
            .ok_or_else(|| ResolveError::Unknown { name: name.clone() })?;

        // Depth-first walk with an explicit stack: one frame per document
        // whose lineage is not known yet, holding the bases still to visit.
        // `on_path` indexes the frames by name, to detect a cycle.
        let mut path = vec![(root, root.bases.iter())];
        let mut on_path = BTreeMap::from([(&root.name, 0)]);
        while let Some((doc, bases)) = path.last_mut() {
            let doc: &'r Document<P> = doc;
            match bases.next() {
                Some(base) => {
                    let next = registry.base_of(&doc.name, base).ok_or_else(|| {
                        ResolveError::UnknownBase {
                            document: doc.name.clone(),
                            layer: doc.layer.clone(),
                            base: base.clone(),
                            hidden: registry.hidden(&doc.name.scope, base).into(),
                        }
                    })?;
                    if self.memo.contains_key(&next.name) {
                        continue;
                    }
                    if let Some(&at) = on_path.get(&next.name) {
                        let on_cycle = || path.iter().skip(at).map(|(d, _)| *d);
                        let mut cycle: Vec<Name> = on_cycle().map(|d| d.name.clone()).collect();
                        cycle.push(next.name.clone());
                        return Err(ResolveError::Cycle {
                            path: cycle,
                            layers: on_cycle().map(|d| d.layer.clone()).collect(),
                            layer: doc.layer.clone(),
                        });
                    }
                    on_path.insert(&next.name, path.len());
                    path.push((next, next.bases.iter()));
                }
                None => {
                    // Every base of `doc` is memoised: post-order.
                    path.pop();
                    on_path.remove(&doc.name);
                    let lineage = Arc::new(self.build(doc)?);
                    self.memo.insert(doc.name.clone(), Arc::clone(&lineage));
                    if path.is_empty() {
                        return Ok(lineage);
                    }
                }
            }
        }
        // The loop returns when it pops the root frame, the last one.
        Err(ResolveError::Unknown { name: name.clone() })
    }

    /// The lineage of `doc`, whose bases' lineages are all memoised.
    fn build(&self, doc: &'r Document<P>) -> Result<Lineage<P>, ResolveError> {
        // Every base was found and memoised before `doc`'s frame was
        // popped, so none is skipped here.
        let bases: Vec<&Lineage<P>> = doc
            .bases
            .iter()
            .filter_map(|b| self.registry.base_of(&doc.name, b))
            .filter_map(|d| self.memo.get(&d.name).map(|l| &**l))
            .collect();
        let entries = linearise(doc, &bases)?;
        let mut names = Vec::with_capacity(entries.len());
        let mut stack = Layered::new();
        for (name, layer, d) in entries {
            names.push(name.clone());
            stack.push(layer.clone(), Arc::clone(d));
        }

        // Start from the merged document of the longest base lineage that
        // begins this one (for a single base, its whole lineage), and apply
        // only the layers above it.
        let start = bases
            .iter()
            .filter(|b| names.starts_with(&b.names))
            .max_by_key(|b| b.names.len());
        let (mut merged, prefix) = match start {
            Some(base) => ((*base.merged).clone(), base.names.len()),
            None => (P::default(), 0),
        };
        for (_, d) in stack.layers().skip(prefix) {
            merged.merge_from(d);
        }

        Ok(Lineage {
            name: doc.name.clone(),
            layer: doc.layer.clone(),
            names,
            stack,
            merged: Arc::new(merged),
        })
    }

    /// `name` resolved: its lineage, merged and completed.
    ///
    /// # Errors
    ///
    /// As [`lineage`](Self::lineage), and [`ResolveError::Incomplete`] if
    /// the merged document lacks required fields.
    pub fn resolve(&mut self, name: &Name) -> Result<Resolved<P::Output, P>, ResolveError>
    where
        P: Complete,
    {
        let lineage = self.lineage(name)?;
        match lineage.merged.complete() {
            Ok(value) => Ok(Resolved { value, lineage }),
            Err(missing) => Err(ResolveError::Incomplete {
                document: lineage.name.clone(),
                layer: lineage.layer.clone(),
                missing,
            }),
        }
    }
}

/// The C3 linearisation of `doc` over its bases' lineages (module docs,
/// *Linearisation*), weakest first, of at most [`MAX_LINEAGE`] documents.
///
/// Works strongest first, as C3 is usually written: the input lists are
/// each base's lineage reversed, strongest base first, then the base list
/// itself, strongest first. `tails` counts, per name, the lists that hold it
/// anywhere but at their head; a head with no count is a valid candidate.
fn linearise<'a, P>(
    doc: &'a Document<P>,
    bases: &[&'a Lineage<P>],
) -> Result<Vec<Entry<'a, P>>, ResolveError> {
    let entries = |l: &'a Lineage<P>| {
        l.names
            .iter()
            .zip(&l.stack.layers)
            .map(|(name, (layer, d))| (name, layer, d))
    };
    let mut lists: Vec<VecDeque<Entry<'a, P>>> = bases
        .iter()
        .rev()
        .map(|l| entries(l).rev().collect())
        .collect();
    lists.push(
        bases
            .iter()
            .rev()
            .filter_map(|l| entries(l).next_back())
            .collect(),
    );
    let mut tails: BTreeMap<&Name, usize> = BTreeMap::new();
    for (name, _, _) in lists.iter().flat_map(|l| l.iter().skip(1)) {
        *tails.entry(name).or_default() += 1;
    }

    let mut out = vec![(&doc.name, &doc.layer, &doc.doc)];
    loop {
        lists.retain(|l| !l.is_empty());
        if lists.is_empty() {
            break;
        }
        let candidate = lists
            .iter()
            .filter_map(VecDeque::front)
            .find(|(name, _, _)| !tails.contains_key(name))
            .copied();
        let Some(next) = candidate else {
            return Err(ResolveError::Inconsistent {
                document: doc.name.clone(),
                layer: doc.layer.clone(),
                conflict: heads(&lists),
            });
        };
        out.push(next);
        // Stops as soon as the limit is passed, without merging the rest:
        // the work an oversized lineage costs is bounded by the limit too.
        if out.len() > MAX_LINEAGE {
            return Err(ResolveError::TooLarge {
                document: doc.name.clone(),
                layer: doc.layer.clone(),
            });
        }
        for list in &mut lists {
            if list.front().is_some_and(|(name, _, _)| *name == next.0) {
                list.pop_front();
                if let Some((head, _, _)) = list.front() {
                    match tails.get_mut(head) {
                        Some(count) if *count > 1 => *count -= 1,
                        _ => {
                            tails.remove(head);
                        }
                    }
                }
            }
        }
    }
    out.reverse();
    Ok(out)
}

/// The distinct heads of `lists`, in list order.
fn heads<P>(lists: &[VecDeque<Entry<'_, P>>]) -> Vec<Name> {
    let mut out: Vec<Name> = Vec::new();
    for (name, _, _) in lists.iter().filter_map(VecDeque::front) {
        if !out.contains(name) {
            out.push((*name).clone());
        }
    }
    out
}
