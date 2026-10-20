//! Strategies for Layered + Resolver (concept C1, `rr-core::layer`; story
//! US-RR-22, task US-RR-22-12).
//!
//! Three layers of generators, each built from the one below:
//!
//! * **Field states** (this module). [`field`] / [`field_with`] generate a
//!   [`Field`] as unset, set or omitted, with tunable [`FieldWeights`];
//!   [`nested`], [`keyed`] and [`appended`] generate the other policies.
//!   [`omit_in_middle`] generates the MF-07 shape: a keyed base list and an
//!   overlay that omits one item that is neither the first nor the last.
//! * **Layers and stacks** (this module). [`layer`] generates the metadata of
//!   one partial document, [`layers`] a run of them with unique ids, and
//!   [`layer_stack`] a [`Layered`] stack of random documents.
//! * **Families** ([`family`]): named documents with `base` references,
//!   ready to put in a [`Registry`](rr_core::layer::Registry). Valid shapes
//!   ([`chain`], [`dag`], [`diamond`], [`shadowing`]) always linearise;
//!   invalid shapes say so in their name ([`cyclic_registry`],
//!   [`inconsistent_bases`], [`unknown_base`]) and carry the error the
//!   resolver must report. [`scoped`] moves the documents of any shape into
//!   local scopes, keeping every reference consistent, and [`extending`]
//!   adds local documents that extend an outer one of the same id, with
//!   [`ExtendFamily::explicit`] the same family written without the idiom,
//!   for the scoped namespace tests (US-RR-22-10).
//!
//! [`sample`] holds a representative partial document (a ROM definition of
//! tables with scalings and keyed axes, signedness carried by the storage
//! type) with its [`Complete`](rr_core::layer::Complete) impl and
//! strategies, for the US-RR-22 property tests to share.
//!
//! Generic strategies return [`BoxedStrategy`], which is `Clone`, so one can
//! be passed to another ([`nested`] uses its document strategy twice).
//!
//! The strategies are checked in `crates/rr-testkit/tests/layered_strategies.rs`:
//! valid families always resolve, cyclic ones always report their cycle,
//! diamonds share an ancestor, omits are in the middle, and failures shrink
//! to small cases.

pub mod family;
pub mod sample;

use core::fmt::Debug;
use std::ops::RangeInclusive;

use proptest::collection::{SizeRange, btree_set, vec};
use proptest::prelude::*;
use proptest::sample::select;
use proptest::strategy::Union;
use rr_core::diag::Span;
use rr_core::layer::{Appended, Field, Keyed, Layer, LayerId, LayerKind, Layered, Merge, Nested};

pub use family::{
    CyclicFamily, DiamondFamily, ExtendFamily, Family, InconsistentFamily, ShadowFamily, Shape,
    UnknownBaseFamily, chain, cyclic_registry, dag, diamond, extending, inconsistent_bases, scoped,
    shadowing, unknown_base,
};

// ---------------------------------------------------------------------------
// Field states
// ---------------------------------------------------------------------------

/// Relative weights of the three [`Field`] states in [`field_with`]. A
/// weight of 0 never generates that state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FieldWeights {
    /// Weight of [`Field::Unset`].
    pub unset: u32,
    /// Weight of [`Field::Set`].
    pub set: u32,
    /// Weight of [`Field::Omit`].
    pub omit: u32,
}

impl FieldWeights {
    /// Every state, set twice as often as unset or omitted: the default.
    pub const MIXED: Self = Self {
        unset: 1,
        set: 2,
        omit: 1,
    };
    /// Always set: a required field of a complete document.
    pub const SET: Self = Self {
        unset: 0,
        set: 1,
        omit: 0,
    };
    /// Never unset: an overlay that overrides (sets or omits) the field.
    pub const OVERRIDE: Self = Self {
        unset: 0,
        set: 1,
        omit: 1,
    };
}

/// [`FieldWeights::MIXED`].
impl Default for FieldWeights {
    fn default() -> Self {
        Self::MIXED
    }
}

/// A [`Field`]: unset, set to a `value` or omitted, weighted
/// [`FieldWeights::MIXED`]. Shrinks towards unset, then set.
pub fn field<T: Clone + Debug + 'static>(
    value: impl Strategy<Value = T> + 'static,
) -> BoxedStrategy<Field<T>> {
    field_with(value, FieldWeights::MIXED)
}

/// A [`Field`] with the states weighted by `weights`. All-zero weights
/// generate only [`Field::Unset`].
pub fn field_with<T: Clone + Debug + 'static>(
    value: impl Strategy<Value = T> + 'static,
    weights: FieldWeights,
) -> BoxedStrategy<Field<T>> {
    let options: Vec<(u32, BoxedStrategy<Field<T>>)> = [
        (weights.unset, Just(Field::Unset).boxed()),
        (weights.set, value.prop_map(Field::Set).boxed()),
        (weights.omit, Just(Field::Omit).boxed()),
    ]
    .into_iter()
    .filter(|(w, _)| *w > 0)
    .collect();
    if options.is_empty() {
        return Just(Field::Unset).boxed();
    }
    Union::new_weighted(options).boxed()
}

/// A [`Nested`] document: unset, a patch or a replace of a `doc`, or
/// omitted. Shrinks towards unset, then patch.
pub fn nested<P: Clone + Debug + 'static>(
    doc: impl Strategy<Value = P> + Clone + 'static,
) -> BoxedStrategy<Nested<P>> {
    prop_oneof![
        1 => Just(Nested::Unset),
        2 => doc.clone().prop_map(Nested::Patch),
        1 => doc.prop_map(Nested::Replace),
        1 => Just(Nested::Omit),
    ]
    .boxed()
}

/// A [`Keyed`] list of `len` entries (before repeated keys merge), each a
/// [`nested`] state of an `item` under a `key`. A small key domain makes
/// layers hit each other's items.
pub fn keyed<K, P>(
    key: impl Strategy<Value = K> + 'static,
    item: impl Strategy<Value = P> + Clone + 'static,
    len: impl Into<SizeRange>,
) -> BoxedStrategy<Keyed<K, P>>
where
    K: Ord + Clone + Debug + 'static,
    P: Merge + Clone + Debug + 'static,
{
    vec((key, nested(item)), len)
        .prop_map(|entries| entries.into_iter().collect())
        .boxed()
}

/// An [`Appended`] list: `len` `item`s appended to, or replacing, the
/// inherited list (a replace of no items is the omit).
pub fn appended<T: Clone + Debug + 'static>(
    item: impl Strategy<Value = T> + 'static,
    len: impl Into<SizeRange>,
) -> BoxedStrategy<Appended<T>> {
    (any::<bool>(), vec(item, len))
        .prop_map(|(replace, items)| {
            if replace {
                Appended::replace(items)
            } else {
                Appended::append(items)
            }
        })
        .boxed()
}

/// The MF-07 shape ([`omit_in_middle`]): a base list of present items and an
/// overlay that omits exactly one of them, neither the first nor the last.
#[derive(Debug, Clone, PartialEq)]
pub struct OmitInMiddle<K, P> {
    /// Every item patched, under distinct keys, in [`keys`](Self::keys)
    /// order.
    pub base: Keyed<K, P>,
    /// The omit of `keys[omitted]`, plus patches of some other items (never
    /// a replace or omit, so every other item stays present).
    pub overlay: Keyed<K, P>,
    /// The base's keys, in order.
    pub keys: Vec<K>,
    /// The position in [`keys`](Self::keys) of the omitted item:
    /// `0 < omitted < keys.len() - 1`.
    pub omitted: usize,
}

/// [`OmitInMiddle`]: `len` (at least 3) items under distinct `key`s, in a
/// random order. `key` must have at least `len.end()` distinct values.
pub fn omit_in_middle<K, P>(
    key: impl Strategy<Value = K> + 'static,
    item: impl Strategy<Value = P> + Clone + 'static,
    len: RangeInclusive<usize>,
) -> BoxedStrategy<OmitInMiddle<K, P>>
where
    K: Ord + Clone + Debug + 'static,
    P: Merge + Clone + Debug + 'static,
{
    let len = (*len.start()).max(3)..=(*len.end()).max(3);
    btree_set(key, len)
        .prop_map(|keys| keys.into_iter().collect::<Vec<_>>())
        .prop_shuffle()
        .prop_flat_map(move |keys| {
            let n = keys.len();
            let base_items = vec(item.clone(), n);
            let overlay_items = vec(proptest::option::of(item.clone()), n);
            (Just(keys), 1..n - 1, base_items, overlay_items)
        })
        .prop_map(|(keys, omitted, base_items, overlay_items)| {
            let mut base = Keyed::new();
            for (k, item) in keys.iter().zip(base_items) {
                base.patch(k.clone(), item);
            }
            let mut overlay = Keyed::new();
            for (i, (k, item)) in keys.iter().zip(overlay_items).enumerate() {
                if i == omitted {
                    overlay.omit(k.clone());
                } else if let Some(item) = item {
                    overlay.patch(k.clone(), item);
                }
            }
            OmitInMiddle {
                base,
                overlay,
                keys,
                omitted,
            }
        })
        .boxed()
}

// ---------------------------------------------------------------------------
// Layers and stacks
// ---------------------------------------------------------------------------

/// Id stems for generated layers and documents: the kinds of name real
/// definitions, profiles and settings use.
pub const ID_STEMS: [&str; 8] = [
    "16BITBASE",
    "32BITBASE",
    "A2WC522N",
    "SSM",
    "KLINE",
    "OPENPORT",
    "defaults",
    "user",
];

/// Every [`LayerKind`].
pub const LAYER_KINDS: [LayerKind; 4] = [
    LayerKind::Builtin,
    LayerKind::File,
    LayerKind::User,
    LayerKind::Cli,
];

/// A [`LayerKind`].
pub fn layer_kind() -> impl Strategy<Value = LayerKind> + Clone {
    select(LAYER_KINDS.to_vec())
}

/// Where a document starts: line `1..=999`, column `1..=80`.
pub fn span() -> impl Strategy<Value = Span> + Clone {
    (1..=999_u32, 1..=80_u32).prop_map(|(line, col)| Span::line_col(line, col))
}

/// The layer whose id is `id`, of `kind`, in `defs/<id>.xml` at `span`.
#[must_use]
pub fn layer_named(id: &str, kind: LayerKind, span: Span) -> Layer {
    Layer::new(LayerId::new(id), kind, format!("defs/{id}.xml"), span)
}

/// One layer: an [`ID_STEMS`] id, any kind, a span in its own file.
pub fn layer() -> impl Strategy<Value = Layer> + Clone {
    (select(ID_STEMS.to_vec()), layer_kind(), span())
        .prop_map(|(id, kind, span)| layer_named(id, kind, span))
}

/// Exactly `n` layers with unique ids (`<stem>-<i>`), each in its own file.
fn layers_exact(n: usize) -> impl Strategy<Value = Vec<Layer>> + Clone {
    vec((select(ID_STEMS.to_vec()), layer_kind(), span()), n).prop_map(|metas| {
        metas
            .into_iter()
            .enumerate()
            .map(|(i, (stem, kind, span))| layer_named(&format!("{stem}-{i}"), kind, span))
            .collect()
    })
}

/// `len` layers, weakest first, with unique ids (`<stem>-<i>`), each in its
/// own file.
pub fn layers(len: RangeInclusive<usize>) -> BoxedStrategy<Vec<Layer>> {
    len.prop_flat_map(layers_exact).boxed()
}

/// A [`Layered`] stack of `len` random `doc`s, on [`layers`] with unique
/// ids.
pub fn layer_stack<P: Clone + Debug + 'static>(
    doc: impl Strategy<Value = P> + Clone + 'static,
    len: RangeInclusive<usize>,
) -> BoxedStrategy<Layered<P>> {
    len.prop_flat_map(move |n| (layers_exact(n), vec(doc.clone(), n)))
        .prop_map(|(layers, docs)| {
            let mut stack = Layered::new();
            for (layer, doc) in layers.into_iter().zip(docs) {
                stack.push(layer, doc);
            }
            stack
        })
        .boxed()
}
