//! Families of named documents with `base` references, for the resolver
//! (`rr_core::layer::{Registry, Resolver}`).
//!
//! A [`Family`] is a list of [`Document`]s whose names are unique, ready for
//! [`Family::registry`]. Generators build a family from an abstract plan
//! (who names whom as a base, which scope each document is in) plus random
//! documents and layer metadata, so every shape works with any partial
//! document type. Shapes refer to documents by index into
//! [`Family::docs`]; [`Family::name`] gives the name.
//!
//! * Names: document `i` has id `<stem>-<i>` ([`ID_STEMS`]), its own file
//!   `defs/<i>-<id>.xml`, any [`LayerKind`] and a random span. Ids are unique
//!   except where a shape deliberately reuses one in another scope
//!   ([`shadowing`], [`unknown_base`]'s MF-16 variant).
//! * Order: bases always come earlier in [`Family::docs`] than the document
//!   naming them, except around a cycle. The registry is an ordered map, so
//!   [`Family::registry_in_order`] can check insertion order changes
//!   nothing.
//! * Oracle: [`Family::reference_lineage`] is an independent C3
//!   linearisation (recursive, list-based, as C3 is usually written), for
//!   comparing the resolver against.
//! * Scopes: every generator puts its documents in the global scope (or a
//!   fixed local scope where the shape is about scopes); [`scoped`] moves
//!   global documents into nested local scopes, keeping each base's scope
//!   an enclosing scope (a prefix) of every document that names it.
//!   References are bare ids, looked up from the referrer's scope outward
//!   as the resolver does ([`Family::bases_of`] is an independent oracle
//!   of that lookup), so moving documents never renames a reference.

use core::fmt::Debug;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;
use std::sync::Arc;

use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::{select, subsequence};
use rr_core::diag::Span;
use rr_core::layer::{Document, DuplicateName, Layer, LayerId, LayerKind, Name, Registry, Scope};

use super::{ID_STEMS, layer_kind, span};

// ---------------------------------------------------------------------------
// Family
// ---------------------------------------------------------------------------

/// Named documents with unique names: one generated registry's contents.
#[derive(Debug, Clone, PartialEq)]
pub struct Family<P> {
    docs: Vec<Document<P>>,
}

impl<P> Family<P> {
    /// The documents, in generation order.
    #[must_use]
    pub fn docs(&self) -> &[Document<P>] {
        &self.docs
    }

    /// The number of documents.
    #[must_use]
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    /// `true` if there are no documents (no generator makes one).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// The name of document `i`. Panics if `i` is out of range.
    #[must_use]
    pub fn name(&self, i: usize) -> &Name {
        self.docs[i].name()
    }

    /// The names of documents `indices`, in that order.
    #[must_use]
    pub fn names(&self, indices: &[usize]) -> Vec<Name> {
        indices.iter().map(|&i| self.name(i).clone()).collect()
    }

    /// The last document's name: for [`chain`] and [`dag`], the most derived
    /// document. Panics on an empty family.
    #[must_use]
    pub fn top(&self) -> &Name {
        self.name(self.docs.len().saturating_sub(1))
    }

    /// The index of the document named `name`.
    #[must_use]
    pub fn index_of(&self, name: &Name) -> Option<usize> {
        self.docs.iter().position(|d| d.name() == name)
    }

    /// Document `i`'s bases as indices, weakest first; `None` for a base
    /// no document in the family has, or none visible from document `i`'s
    /// scope (MF-16).
    ///
    /// The lexical lookup, written independently of the resolver's: of the
    /// documents other than `i` with the base's id whose scope path is a
    /// prefix of document `i`'s, the one with the longest path. Leaving `i`
    /// out is the extend-outer rule: a base with `i`'s own id names the
    /// outer definition of that id, never `i` itself.
    #[must_use]
    pub fn bases_of(&self, i: usize) -> Vec<Option<usize>> {
        let from: Vec<&str> = self.docs[i].name().scope().segments().collect();
        self.docs[i]
            .bases()
            .iter()
            .map(|id| {
                self.docs
                    .iter()
                    .enumerate()
                    .filter(|&(j, d)| j != i && d.name().id() == id)
                    .filter_map(|(j, d)| {
                        let path: Vec<&str> = d.name().scope().segments().collect();
                        from.starts_with(&path).then_some((path.len(), j))
                    })
                    .max()
                    .map(|(_, j)| j)
            })
            .collect()
    }

    /// The documents with the id of document `i`'s base `k` that document
    /// `i` cannot see, as indices in name order: the `hidden` list of the
    /// `UnknownBase` error, when no document with the id is visible.
    #[must_use]
    pub fn hidden_from(&self, i: usize, k: usize) -> Vec<usize> {
        let from: Vec<&str> = self.docs[i].name().scope().segments().collect();
        let id = &self.docs[i].bases()[k];
        let mut out: Vec<usize> = (0..self.len())
            .filter(|&j| {
                let d = self.name(j);
                let path: Vec<&str> = d.scope().segments().collect();
                d.id() == id && !from.starts_with(&path)
            })
            .collect();
        out.sort_by(|&a, &b| self.name(a).cmp(self.name(b)));
        out
    }

    /// Every document document `i` inherits from, directly or not
    /// (including `i` itself only if it is on a cycle).
    #[must_use]
    pub fn ancestors(&self, i: usize) -> BTreeSet<usize> {
        let mut seen = BTreeSet::new();
        let mut todo: Vec<usize> = self.bases_of(i).into_iter().flatten().collect();
        while let Some(b) = todo.pop() {
            if seen.insert(b) {
                todo.extend(self.bases_of(b).into_iter().flatten());
            }
        }
        seen
    }

    /// The lineage of document `i` by an independent C3 linearisation,
    /// weakest first, as indices: what `Resolver::lineage` must give.
    /// `None` if the resolver must refuse it (a cycle, a base that is not
    /// in the family, or bases that cannot be ordered). Ignores
    /// `MAX_LINEAGE`: generated families are far smaller.
    #[must_use]
    pub fn reference_lineage(&self, i: usize) -> Option<Vec<usize>> {
        let graph: Vec<Option<Vec<usize>>> = (0..self.len())
            .map(|d| self.bases_of(d).into_iter().collect())
            .collect();
        let mut memo = BTreeMap::new();
        let mut visiting = BTreeSet::new();
        let mut strongest_first = c3(&graph, i, &mut memo, &mut visiting)?;
        strongest_first.reverse();
        Some(strongest_first)
    }
}

impl<P: Clone> Family<P> {
    /// A registry of every document, inserted in generation order.
    ///
    /// # Errors
    ///
    /// [`DuplicateName`] if two documents share a name, which no generator
    /// produces.
    pub fn registry(&self) -> Result<Registry<P>, Box<DuplicateName>> {
        let order: Vec<usize> = (0..self.len()).collect();
        self.registry_in_order(&order)
    }

    /// A registry of documents `order`, inserted in that order.
    ///
    /// # Errors
    ///
    /// As [`registry`](Self::registry).
    pub fn registry_in_order(&self, order: &[usize]) -> Result<Registry<P>, Box<DuplicateName>> {
        let mut out = Registry::new();
        for &i in order {
            out.insert(self.docs[i].clone())?;
        }
        Ok(out)
    }

    /// The same family with document `i` renamed to `id` (its name and its
    /// layer's id), and every base reference that found document `i`
    /// ([`bases_of`](Self::bases_of)) rewritten to `id`, so that it still
    /// finds it. Every other reference stays as written; document `i`'s own
    /// references to its old id, if any, now name the outer definition of
    /// that id explicitly.
    #[must_use]
    pub fn renamed(&self, i: usize, id: impl Into<LayerId>) -> Self {
        let id = id.into();
        let docs = self
            .docs
            .iter()
            .enumerate()
            .map(|(j, d)| {
                let l = d.layer();
                let layer = if j == i {
                    Layer::new(id.clone(), l.kind(), l.source(), l.span())
                } else {
                    l.clone()
                };
                let fresh =
                    Document::new(layer, Arc::clone(d.doc())).in_scope(d.name().scope().clone());
                d.bases()
                    .iter()
                    .zip(self.bases_of(j))
                    .fold(fresh, |acc, (b, found)| {
                        acc.with_base(if found == Some(i) {
                            id.clone()
                        } else {
                            b.clone()
                        })
                    })
            })
            .collect();
        Self { docs }
    }

    /// The same family with each global document moved into scope
    /// `chain[d]`, where `d` is the larger of `depth[i]` (clamped to the
    /// chain) and the depth of each of its bases, so every base stays in an
    /// enclosing scope of the document naming it. References are ids, so
    /// none changes. Documents already in a local scope stay where they
    /// are, and so do (in the global scope) the documents they inherit
    /// from, which moving could hide from them. `chain` is a list of nested
    /// scopes, outermost (global) first.
    #[must_use]
    pub fn rescoped(&self, depth: &[usize], chain: &[Scope]) -> Self {
        let n = self.len();
        let deepest = chain.len().saturating_sub(1);
        let movable: Vec<bool> = self
            .docs
            .iter()
            .map(|d| d.name().scope().is_global())
            .collect();
        let pinned: BTreeSet<usize> = (0..n)
            .filter(|&i| !movable[i])
            .flat_map(|i| self.ancestors(i))
            .collect();
        let mut at: Vec<usize> = (0..n)
            .map(|i| {
                if pinned.contains(&i) {
                    0
                } else {
                    depth.get(i).copied().unwrap_or(0).min(deepest)
                }
            })
            .collect();
        // A document is at least as deep as each of its (moved) bases. Each
        // round settles one more step of the longest base path, cycles
        // included, so `n` rounds reach the fixed point.
        for _ in 0..n {
            for i in 0..n {
                for b in self.bases_of(i).into_iter().flatten() {
                    if movable[b] && at[b] > at[i] {
                        at[i] = at[b];
                    }
                }
            }
        }
        let names: Vec<Name> = (0..n)
            .map(|i| match (movable[i], chain.get(at[i])) {
                (true, Some(scope)) => Name::new(scope.clone(), self.name(i).id().clone()),
                _ => self.name(i).clone(),
            })
            .collect();
        let docs = self
            .docs
            .iter()
            .zip(&names)
            .map(|(d, name)| {
                let moved = Document::new(d.layer().clone(), Arc::clone(d.doc()))
                    .in_scope(name.scope().clone());
                d.bases().iter().cloned().fold(moved, Document::with_base)
            })
            .collect();
        Self { docs }
    }
}

/// C3 of `d` over `graph` (each document's bases, weakest first, `None` if
/// one is unknown), strongest first, memoised; `None` on a cycle, an unknown
/// base or a conflict.
fn c3(
    graph: &[Option<Vec<usize>>],
    d: usize,
    memo: &mut BTreeMap<usize, Option<Vec<usize>>>,
    visiting: &mut BTreeSet<usize>,
) -> Option<Vec<usize>> {
    if let Some(done) = memo.get(&d) {
        return done.clone();
    }
    if !visiting.insert(d) {
        return None;
    }
    let out = c3_merge(graph, d, memo, visiting);
    visiting.remove(&d);
    memo.insert(d, out.clone());
    out
}

/// [`c3`]'s step: `[d] + merge(S(Bₖ), …, S(B₁), [Bₖ, …, B₁])`.
fn c3_merge(
    graph: &[Option<Vec<usize>>],
    d: usize,
    memo: &mut BTreeMap<usize, Option<Vec<usize>>>,
    visiting: &mut BTreeSet<usize>,
) -> Option<Vec<usize>> {
    let bases = graph.get(d)?.as_ref()?;
    let mut seqs: Vec<Vec<usize>> = Vec::new();
    for &b in bases.iter().rev() {
        seqs.push(c3(graph, b, memo, visiting)?);
    }
    seqs.push(bases.iter().rev().copied().collect());
    let mut out = vec![d];
    loop {
        seqs.retain(|s| !s.is_empty());
        if seqs.is_empty() {
            return Some(out);
        }
        let next = seqs
            .iter()
            .map(|s| s[0])
            .find(|c| !seqs.iter().any(|s| s[1..].contains(c)))?;
        out.push(next);
        for s in &mut seqs {
            if s[0] == next {
                s.remove(0);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Plans: shapes before documents are attached
// ---------------------------------------------------------------------------

/// One base reference in a [`Plan`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ref {
    /// Document `i` of the family.
    Doc(usize),
    /// An id no document has: `<referrer id>-missing`.
    Missing,
    /// Document `i`'s id, which only a document in a scope the referrer
    /// cannot see has (MF-16).
    HiddenId(usize),
}

/// The shape of a family: per document, its bases, the document whose id it
/// takes (itself, or another for a deliberate duplicate in another scope)
/// and its scope.
#[derive(Debug, Clone, PartialEq)]
struct Plan {
    bases: Vec<Vec<Ref>>,
    id_of: Vec<usize>,
    scopes: Vec<Scope>,
}

impl Plan {
    /// `n` global documents with no bases.
    fn new(n: usize) -> Self {
        Self {
            bases: vec![Vec::new(); n],
            id_of: (0..n).collect(),
            scopes: vec![Scope::global(); n],
        }
    }

    /// Adds a global document naming `bases`; its index.
    fn push(&mut self, bases: Vec<Ref>) -> usize {
        self.bases.push(bases);
        self.id_of.push(self.id_of.len());
        self.scopes.push(Scope::global());
        self.bases.len() - 1
    }

    fn len(&self) -> usize {
        self.bases.len()
    }

    /// The family: document `i` is `docs[i]` with `metas[i]`'s layer.
    fn build<P>(&self, docs: Vec<P>, metas: &[Meta]) -> Family<P> {
        let ids: Vec<String> = (0..self.len())
            .map(|i| {
                let j = self.id_of[i];
                format!("{}-{j}", metas[j].0)
            })
            .collect();
        let docs = docs
            .into_iter()
            .enumerate()
            .map(|(i, doc)| {
                let (_, kind, span) = metas[i];
                let id = &ids[i];
                let layer = Layer::new(
                    LayerId::new(id.as_str()),
                    kind,
                    format!("defs/{i}-{id}.xml"),
                    span,
                );
                let d = Document::new(layer, doc).in_scope(self.scopes[i].clone());
                self.bases[i].iter().fold(d, |d, r| match *r {
                    Ref::Doc(j) | Ref::HiddenId(j) => d.with_base(ids[j].as_str()),
                    Ref::Missing => d.with_base(format!("{id}-missing")),
                })
            })
            .collect();
        Family { docs }
    }
}

/// Generated layer metadata of one document: id stem, kind, span.
type Meta = (&'static str, LayerKind, Span);

fn meta() -> impl Strategy<Value = Meta> + Clone {
    (select(ID_STEMS.to_vec()), layer_kind(), span())
}

/// Attaches random documents and metadata to each generated plan, which
/// has at most `max` documents.
///
/// The documents are generated alongside the plan, `max` of them, rather
/// than by `prop_flat_map` on its length: a tuple shrinks its first element
/// first, so a failing case shrinks its shape (fewer documents, shorter
/// cycles) before the documents' contents, within proptest's shrink budget.
fn realise<P, X, T>(
    plans: impl Strategy<Value = (Plan, X)> + 'static,
    max: usize,
    doc: impl Strategy<Value = P> + Clone + 'static,
    finish: fn(Family<P>, X) -> T,
) -> BoxedStrategy<T>
where
    P: Clone + Debug + 'static,
    X: Clone + Debug + 'static,
    T: Debug + 'static,
{
    (plans, vec(doc, max), vec(meta(), max))
        .prop_map(move |((plan, x), mut docs, metas)| {
            docs.truncate(plan.len());
            finish(plan.build(docs, &metas), x)
        })
        .boxed()
}

/// A run of `len` documents on top of document `below` (if any), each
/// naming the previous: its indices, bottom first.
fn push_chain(plan: &mut Plan, below: Option<usize>, len: usize) -> Vec<usize> {
    let mut out = Vec::with_capacity(len);
    let mut prev = below;
    for _ in 0..len {
        let i = plan.push(prev.map(Ref::Doc).into_iter().collect());
        out.push(i);
        prev = Some(i);
    }
    out
}

// ---------------------------------------------------------------------------
// Shapes
// ---------------------------------------------------------------------------

/// A generated shape: a [`Family`] plus what the shape promises about it.
/// [`scoped`] works on any shape.
pub trait Shape: Clone + Debug {
    /// The partial document type.
    type Doc: Clone;
    /// The family.
    fn family(&self) -> &Family<Self::Doc>;
    /// The family, to replace (by [`scoped`]).
    fn family_mut(&mut self) -> &mut Family<Self::Doc>;
}

impl<P: Clone + Debug> Shape for Family<P> {
    type Doc = P;
    fn family(&self) -> &Family<P> {
        self
    }
    fn family_mut(&mut self) -> &mut Family<P> {
        self
    }
}

macro_rules! shape {
    ($ty:ident) => {
        impl<P: Clone + Debug> Shape for $ty<P> {
            type Doc = P;
            fn family(&self) -> &Family<P> {
                &self.family
            }
            fn family_mut(&mut self) -> &mut Family<P> {
                &mut self.family
            }
        }
    };
}

/// Valid by name: a chain of `len` (at least 1) documents, each naming the
/// previous as its one base (the legacy `base` chain). The last document is
/// the most derived; every lineage is the chain up to it.
pub fn chain<P: Clone + Debug + 'static>(
    doc: impl Strategy<Value = P> + Clone + 'static,
    len: RangeInclusive<usize>,
) -> BoxedStrategy<Family<P>> {
    let len = (*len.start()).max(1)..=(*len.end()).max(1);
    let max = *len.end();
    let plans = len.prop_map(|n| {
        let mut plan = Plan::new(0);
        push_chain(&mut plan, None, n);
        (plan, ())
    });
    realise(plans, max, doc, |f, ()| f)
}

/// Valid by name: a random DAG of `len` (at least 1) documents. Document `i`
/// names up to three of the documents before it, in a random order, so
/// shared ancestors (diamonds) and several bases are common. Only DAGs C3
/// can linearise are kept (by [`Family::reference_lineage`]), so every
/// document resolves.
pub fn dag<P: Clone + Debug + 'static>(
    doc: impl Strategy<Value = P> + Clone + 'static,
    len: RangeInclusive<usize>,
) -> BoxedStrategy<Family<P>> {
    let len = (*len.start()).max(1)..=(*len.end()).max(1);
    let max = *len.end();
    let graphs = len.prop_flat_map(|n| {
        (0..n)
            .map(|i| {
                let earlier: Vec<usize> = (0..i).collect();
                (subsequence(earlier, 0..=i.min(3)), any::<bool>()).prop_flat_map(
                    |(bases, shuffle)| {
                        if shuffle {
                            Just(bases).prop_shuffle().boxed()
                        } else {
                            Just(bases).boxed()
                        }
                    },
                )
            })
            .collect::<Vec<_>>()
    });
    let plans = graphs
        .prop_filter("C3 can linearise every document", |graph| {
            let graph: Vec<Option<Vec<usize>>> = graph.iter().cloned().map(Some).collect();
            let mut memo = BTreeMap::new();
            (0..graph.len()).all(|d| c3(&graph, d, &mut memo, &mut BTreeSet::new()).is_some())
        })
        .prop_map(|graph| {
            let mut plan = Plan::new(0);
            for bases in graph {
                plan.push(bases.into_iter().map(Ref::Doc).collect());
            }
            (plan, ())
        });
    realise(plans, max, doc, |f, ()| f)
}

/// [`diamond`]: a document whose bases all inherit from one shared
/// ancestor.
#[derive(Debug, Clone, PartialEq)]
pub struct DiamondFamily<P> {
    /// The documents.
    pub family: Family<P>,
    /// The shared ancestor.
    pub apex: usize,
    /// The top's bases, weakest first: each inherits from the apex through
    /// its own chain of one or two documents.
    pub sides: Vec<usize>,
    /// The document with several bases.
    pub top: usize,
}
shape!(DiamondFamily);

/// Valid by name: a diamond (EC-04). An apex (optionally on a base of its
/// own), two or three sides, each a chain of one or two documents on the
/// apex, and a top naming the sides' last documents in a random order. The
/// apex must be applied once, below every side.
pub fn diamond<P: Clone + Debug + 'static>(
    doc: impl Strategy<Value = P> + Clone + 'static,
) -> BoxedStrategy<DiamondFamily<P>> {
    let plans = (any::<bool>(), vec(1..=2_usize, 2..=3))
        .prop_flat_map(|(root, side_lens)| {
            let order: Vec<usize> = (0..side_lens.len()).collect();
            (Just(root), Just(side_lens), Just(order).prop_shuffle())
        })
        .prop_map(|(root, side_lens, order)| {
            let mut plan = Plan::new(0);
            let below = root.then(|| plan.push(Vec::new()));
            let apex = plan.push(below.map(Ref::Doc).into_iter().collect());
            let tips: Vec<usize> = side_lens
                .iter()
                .filter_map(|&len| push_chain(&mut plan, Some(apex), len).last().copied())
                .collect();
            let sides: Vec<usize> = order.iter().map(|&k| tips[k]).collect();
            let top = plan.push(sides.iter().copied().map(Ref::Doc).collect());
            (plan, (apex, sides, top))
        });
    // An optional root, the apex, up to three sides of two, the top.
    realise(
        plans,
        1 + 1 + 3 * 2 + 1,
        doc,
        |family, (apex, sides, top)| DiamondFamily {
            family,
            apex,
            sides,
            top,
        },
    )
}

/// [`cyclic_registry`]: documents whose bases loop.
#[derive(Debug, Clone, PartialEq)]
pub struct CyclicFamily<P> {
    /// The documents.
    pub family: Family<P>,
    /// The documents on the cycle, in base order: each names the next as
    /// its base, the last names the first.
    pub cycle: Vec<usize>,
    /// The document to resolve: the first on the cycle, or a document
    /// outside it whose chain of bases leads into it there.
    pub entry: usize,
}
shape!(CyclicFamily);

impl<P> CyclicFamily<P> {
    /// The `path` of the `Cycle` error resolving [`entry`](Self::entry)
    /// must give: the cycle from its first document, which repeated at the
    /// end. `cycle.len() + 1` names.
    #[must_use]
    pub fn expected_path(&self) -> Vec<Name> {
        let mut path = self.family.names(&self.cycle);
        path.extend(self.cycle.first().map(|&c| self.family.name(c).clone()));
        path
    }

    /// The document whose base closes the cycle (the last on it): the
    /// error's layer.
    #[must_use]
    pub fn closing(&self) -> usize {
        self.cycle.last().copied().unwrap_or(self.entry)
    }
}

/// Invalid by name: a cycle of `k` (at least 2) documents across `k` files,
/// each naming the next as a base. A `k` below 2 is taken as 2: a document
/// naming its own id as a base is no cycle but the extend-outer idiom
/// ([`extending`]), an `UnknownBase` when nothing outside it has the id.
/// Some cycle documents also name, before it, a valid base from a
/// small pool, and the cycle may be entered from a chain of up to two
/// documents outside it. Resolving [`CyclicFamily::entry`] is a `Cycle`
/// error with [`CyclicFamily::expected_path`].
pub fn cyclic_registry<P: Clone + Debug + 'static>(
    doc: impl Strategy<Value = P> + Clone + 'static,
    k: RangeInclusive<usize>,
) -> BoxedStrategy<CyclicFamily<P>> {
    let k = (*k.start()).max(2)..=(*k.end()).max(2);
    // Up to two pool roots, the cycle, up to two lead-in documents.
    let max = 2 + *k.end() + 2;
    let plans = (0..=2_usize, k, 0..=2_usize)
        .prop_flat_map(|(pool, k, lead)| {
            let extra = vec(proptest::option::of(0..pool.max(1)), k + lead);
            (Just(pool), Just(k), Just(lead), extra)
        })
        .prop_map(|(pool, k, lead, extras)| {
            let mut plan = Plan::new(pool);
            let extra = |i: usize| {
                extras
                    .get(i)
                    .copied()
                    .flatten()
                    .filter(|_| pool > 0)
                    .map(Ref::Doc)
            };
            let cycle: Vec<usize> = (pool..pool + k).collect();
            for j in 0..k {
                let next = cycle[(j + 1) % k];
                plan.push(extra(j).into_iter().chain([Ref::Doc(next)]).collect());
            }
            let mut entry = cycle[0];
            for j in 0..lead {
                entry = plan.push(extra(k + j).into_iter().chain([Ref::Doc(entry)]).collect());
            }
            (plan, (cycle, entry))
        });
    realise(plans, max, doc, |family, (cycle, entry)| CyclicFamily {
        family,
        cycle,
        entry,
    })
}

/// [`inconsistent_bases`]: a document whose bases C3 cannot order.
#[derive(Debug, Clone, PartialEq)]
pub struct InconsistentFamily<P> {
    /// The documents.
    pub family: Family<P>,
    /// The document whose bases conflict: the error's `document`.
    pub document: usize,
    /// The document to resolve: [`document`](Self::document) or a chain of
    /// up to two documents above it.
    pub entry: usize,
}
shape!(InconsistentFamily);

/// Invalid by name: bases C3 cannot linearise, one of three ways:
///
/// * crossed: `x: [a, b]`, `y: [b, a]`, `d: [x, y]` (or `[y, x]`);
/// * twice: `d: [a, a]`, a base both weaker and stronger than itself;
/// * indirect, with every base list in generation order: `s: [r1]`,
///   `u: [r1, r2]`, `t: [r2, s]` (whose lineage puts `r2` below `r1`,
///   where `u` puts it above), `d: [u, t]`.
///
/// Optionally entered from a chain of up to two documents above `d`. Every
/// other document resolves; resolving [`InconsistentFamily::entry`] is an
/// `Inconsistent` error for [`InconsistentFamily::document`].
pub fn inconsistent_bases<P: Clone + Debug + 'static>(
    doc: impl Strategy<Value = P> + Clone + 'static,
) -> BoxedStrategy<InconsistentFamily<P>> {
    let plans = (0..3_u8, any::<bool>(), 0..=2_usize).prop_map(|(variant, flip, lead)| {
        let mut plan = Plan::new(0);
        let d = match variant {
            0 => {
                let a = plan.push(Vec::new());
                let b = plan.push(Vec::new());
                let x = plan.push(vec![Ref::Doc(a), Ref::Doc(b)]);
                let y = plan.push(vec![Ref::Doc(b), Ref::Doc(a)]);
                let (p, q) = if flip { (y, x) } else { (x, y) };
                plan.push(vec![Ref::Doc(p), Ref::Doc(q)])
            }
            1 => {
                let a = plan.push(Vec::new());
                plan.push(vec![Ref::Doc(a), Ref::Doc(a)])
            }
            _ => {
                let r1 = plan.push(Vec::new());
                let r2 = plan.push(Vec::new());
                let s = plan.push(vec![Ref::Doc(r1)]);
                let u = plan.push(vec![Ref::Doc(r1), Ref::Doc(r2)]);
                let t = plan.push(vec![Ref::Doc(r2), Ref::Doc(s)]);
                plan.push(vec![Ref::Doc(u), Ref::Doc(t)])
            }
        };
        let entry = push_chain(&mut plan, Some(d), lead)
            .last()
            .copied()
            .unwrap_or(d);
        (plan, (d, entry))
    });
    // The indirect variant's six documents, up to two lead-in documents.
    realise(plans, 6 + 2, doc, |family, (document, entry)| {
        InconsistentFamily {
            family,
            document,
            entry,
        }
    })
}

/// [`unknown_base`]: a document naming a base no document has.
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownBaseFamily<P> {
    /// The documents.
    pub family: Family<P>,
    /// The document naming the unknown base: the error's `document`.
    pub document: usize,
    /// The unknown base, as named: an id.
    pub base: LayerId,
    /// For the MF-16 variant, the local document with that id, in a scope
    /// the document naming it cannot see.
    pub local: Option<usize>,
    /// The document to resolve: [`document`](Self::document) or a chain of
    /// up to two documents above it.
    pub entry: usize,
}
shape!(UnknownBaseFamily);

/// Invalid by name: a document naming, among valid bases from a chain of one
/// to three documents, a base that is not registered: either an id no
/// document has, or (MF-16) an id only a document in a local scope the
/// referrer cannot see has. Optionally entered from a chain of up to two documents
/// above it. Resolving [`UnknownBaseFamily::entry`] is an `UnknownBase`
/// error for [`UnknownBaseFamily::document`] and
/// [`UnknownBaseFamily::base`].
pub fn unknown_base<P: Clone + Debug + 'static>(
    doc: impl Strategy<Value = P> + Clone + 'static,
) -> BoxedStrategy<UnknownBaseFamily<P>> {
    let plans = (
        any::<bool>(),
        1..=3_usize,
        select(TABLE_SEGMENTS.to_vec()),
        0..=2_usize,
    )
        .prop_flat_map(|(mf16, valid, segment, lead)| {
            let chosen = subsequence((0..valid).collect::<Vec<_>>(), 0..=valid);
            (Just(mf16), Just(valid), Just(segment), Just(lead), chosen).prop_flat_map(
                |(mf16, valid, segment, lead, chosen)| {
                    let at = 0..=chosen.len();
                    (Just((mf16, valid, segment, lead, chosen)), at)
                },
            )
        })
        .prop_map(|((mf16, valid, segment, lead, chosen), at)| {
            let mut plan = Plan::new(0);
            let chain = push_chain(&mut plan, None, valid);
            let local = mf16.then(|| {
                let l = plan.push(Vec::new());
                plan.scopes[l] = Scope::global().child(LOCAL_SEGMENT).child(segment);
                l
            });
            let ghost = local.map_or(Ref::Missing, Ref::HiddenId);
            let mut bases: Vec<Ref> = chosen.iter().map(|&c| Ref::Doc(chain[c])).collect();
            bases.insert(at, ghost);
            let d = plan.push(bases);
            let entry = push_chain(&mut plan, Some(d), lead)
                .last()
                .copied()
                .unwrap_or(d);
            (plan, (d, at, local, entry))
        });
    // Up to three valid bases, the local document, the faulty one, up to
    // two lead-in documents.
    realise(
        plans,
        3 + 1 + 1 + 2,
        doc,
        |family, (document, at, local, entry)| {
            let base = family.docs[document].bases()[at].clone();
            UnknownBaseFamily {
                family,
                document,
                base,
                local,
                entry,
            }
        },
    )
}

/// [`shadowing`]: a local document with the same id as a global one. The
/// registry lists the local one as shadowing the global one
/// (`Registry::shadowed`), unless [`scoped`] moved the global one into a
/// scope that does not enclose the local one.
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowFamily<P> {
    /// The documents.
    pub family: Family<P>,
    /// The global document.
    pub global: usize,
    /// The local document, in a local scope, with the global one's id.
    pub local: usize,
    /// A global document naming the shared id: its lineage holds the
    /// global document, never the local one (MF-16).
    pub outer: usize,
    /// A document in the local scope naming the same id: its lineage holds
    /// the local document, which shadows the global one there, and the
    /// global one only if the local one [`extends`](Self::extends) it.
    pub inner: usize,
    /// The local document names the shared id as its base: it extends the
    /// global document (the extend-outer idiom; lineage `[global, local]`)
    /// and is not listed as shadowing it.
    pub extends: bool,
}
shape!(ShadowFamily);

/// Valid by name: an id defined both globally and in a local scope (a
/// table-local scaling with the id of a global one), with a document outside
/// the scope and a document inside it both naming that id: the outer one
/// finds the global document, the inner one the local document (MF-16;
/// legacy registered table-local scalings as global bases,
/// `xml/TableScaleUnmarshaller.java:461`). In half the cases the local
/// document [extends](ShadowFamily::extends) the global one by naming its
/// own id; [`scoped`] then keeps the global document global, where the local
/// one sees it.
pub fn shadowing<P: Clone + Debug + 'static>(
    doc: impl Strategy<Value = P> + Clone + 'static,
) -> BoxedStrategy<ShadowFamily<P>> {
    let plans = (select(TABLE_SEGMENTS.to_vec()), any::<bool>()).prop_map(|(segment, extends)| {
        let local_scope = Scope::global().child(LOCAL_SEGMENT).child(segment);
        let mut plan = Plan::new(0);
        let global = plan.push(Vec::new());
        // `Ref::Doc(global)` is the global document's id: the local one's own.
        let local = plan.push(extends.then_some(Ref::Doc(global)).into_iter().collect());
        plan.id_of[local] = global;
        plan.scopes[local] = local_scope.clone();
        let outer = plan.push(vec![Ref::Doc(global)]);
        let inner = plan.push(vec![Ref::Doc(local)]);
        plan.scopes[inner] = local_scope;
        (plan, (global, local, outer, inner, extends))
    });
    realise(
        plans,
        4,
        doc,
        |family, (global, local, outer, inner, extends)| ShadowFamily {
            family,
            global,
            local,
            outer,
            inner,
            extends,
        },
    )
}

/// [`extending`]: local documents extending an outer document of the same
/// id, the extend-outer idiom.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtendFamily<P> {
    /// The documents.
    pub family: Family<P>,
    /// The outer document the extensions build on.
    pub outer: usize,
    /// The extending documents, outermost first. Each has the outer
    /// document's id, is in the scope [`EXTEND_SEGMENT`] inside the previous
    /// one's (the first inside the outer document's), and names that id as
    /// a base, so it extends the previous one (the first, the outer
    /// document); some also name another base visible from their scope.
    pub extensions: Vec<usize>,
    /// A document in the innermost extension's scope naming the id: it finds
    /// the innermost extension.
    pub user: usize,
    /// A document in the outer document's scope naming the id: it finds the
    /// outer document, never an extension (MF-16).
    pub outside: usize,
}
shape!(ExtendFamily);

impl<P: Clone> ExtendFamily<P> {
    /// The same family written without the idiom: each extension renamed
    /// (`<id>-explicit-<k>`, innermost first) with
    /// [`Family::renamed`], so it names the outer definition by an id that
    /// is not its own. Resolving it must give what resolving the idiom
    /// gives, document by document (by index).
    #[must_use]
    pub fn explicit(&self) -> Family<P> {
        self.extensions
            .iter()
            .enumerate()
            .rev()
            .fold(self.family.clone(), |f, (k, &e)| {
                let id = format!("{}-explicit-{k}", f.name(e).id());
                f.renamed(e, id.as_str())
            })
    }
}

/// The segment of each scope [`extending`] adds. Neither [`scoped`] nor any
/// other shape uses it.
pub const EXTEND_SEGMENT: &str = "ext";

/// Valid or not by its base family: a [`scoped`] [`dag`] of one to five
/// documents, with one to two levels of the extend-outer idiom on one of its
/// documents (the outer one): a document with its id in a scope inside its
/// own naming that id as a base, then optionally another inside that one
/// doing the same (table -> ROM -> global and deeper), each maybe naming a
/// second base visible from its scope, weaker or stronger. A second base
/// can make a document's bases impossible to order (`Inconsistent`); the
/// shape's promise is [`ExtendFamily::explicit`], not that it resolves.
pub fn extending<P: Clone + Debug + 'static>(
    doc: impl Strategy<Value = P> + Clone + 'static,
) -> BoxedStrategy<ExtendFamily<P>> {
    let extra = (
        proptest::option::of(any::<proptest::sample::Index>()),
        any::<bool>(),
    );
    (
        scoped(dag(doc.clone(), 1..=5)),
        any::<proptest::sample::Index>(),
        vec(extra, 1..=2),
        vec(doc, 4),
    )
        .prop_map(|(family, outer, extras, docs)| {
            let o = outer.index(family.len());
            let id = family.name(o).id().clone();
            let mut out = family.docs;
            let mut scope = out[o].name().scope().clone();
            let mut extensions = Vec::new();
            // `docs` has one document per slot: extensions 0 and 1, then the
            // user (2) and the outside document (3).
            let doc = |slot: usize, file: String, id: &str, scope: &Scope| {
                let layer = Layer::new(
                    LayerId::new(id),
                    LayerKind::File,
                    file,
                    Span::line_col(1, 1),
                );
                Document::new(layer, docs[slot].clone()).in_scope(scope.clone())
            };
            for (k, (second, stronger)) in extras.into_iter().enumerate() {
                scope = scope.child(EXTEND_SEGMENT);
                let visible: Vec<LayerId> = out
                    .iter()
                    .filter(|d| d.name().scope().encloses(&scope) && *d.name().id() != id)
                    .map(|d| d.name().id().clone())
                    .collect();
                let second = second
                    .filter(|_| !visible.is_empty())
                    .map(|i| i.get(&visible).clone());
                let mut bases = vec![id.clone()];
                match (second, stronger) {
                    (Some(b), true) => bases.push(b),
                    (Some(b), false) => bases.insert(0, b),
                    (None, _) => {}
                }
                let d = bases.into_iter().fold(
                    doc(k, format!("defs/ext-{k}.xml"), id.as_str(), &scope),
                    Document::with_base,
                );
                extensions.push(out.len());
                out.push(d);
            }
            let user = out.len();
            let u = doc(2, "defs/user.xml".to_owned(), &format!("{id}-user"), &scope);
            out.push(u.with_base(id.clone()));
            let outside = out.len();
            let outer_scope = out[o].name().scope().clone();
            let x = doc(
                3,
                "defs/outside.xml".to_owned(),
                &format!("{id}-outside"),
                &outer_scope,
            );
            out.push(x.with_base(id.clone()));
            ExtendFamily {
                family: Family { docs: out },
                outer: o,
                extensions,
                user,
                outside,
            }
        })
        .boxed()
}

// ---------------------------------------------------------------------------
// Scopes
// ---------------------------------------------------------------------------

/// The outer segment of the fixed local scope [`shadowing`] and
/// [`unknown_base`] use. [`scoped`] never uses it, so no moved document is
/// ever inside that scope.
pub const LOCAL_SEGMENT: &str = "local";

/// Outer segments of the scopes [`scoped`] moves documents into: ROM ids.
pub const ROM_SEGMENTS: [&str; 3] = ["A2WC522N", "16BITBASE", "rom"];

/// Inner segments of local scopes: table names.
pub const TABLE_SEGMENTS: [&str; 3] = ["Fuel Map", "Timing", "table"];

/// `shape` with its global documents moved into local scopes
/// ([`Family::rescoped`]): each into the global scope, a ROM scope, or a
/// table scope inside it, never shallower than any of its bases, so a base
/// is always in an enclosing scope of the document naming it, where the
/// lexical lookup finds it. References are ids and stay as they are, so the
/// shape's promises (lineages, cycles, errors, by index) still hold: ids
/// are unique except in the fixed local scope, which no moved document
/// enters, so no reference finds a different document.
pub fn scoped<T: Shape + 'static>(shape: impl Strategy<Value = T> + 'static) -> BoxedStrategy<T> {
    shape
        .prop_flat_map(|shape| {
            let n = shape.family().len();
            (
                Just(shape),
                vec(0..=2_usize, n),
                select(ROM_SEGMENTS.to_vec()),
                select(TABLE_SEGMENTS.to_vec()),
            )
        })
        .prop_map(|(mut shape, depth, rom, table)| {
            let rom_scope = Scope::global().child(rom);
            let chain = [Scope::global(), rom_scope.clone(), rom_scope.child(table)];
            let moved = shape.family().rescoped(&depth, &chain);
            *shape.family_mut() = moved;
            shape
        })
        .boxed()
}
