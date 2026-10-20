//! `explain(field)` reports the winning layer and the overridden values,
//! with provenance, through the public `Registry` -> `Resolver` path
//! (US-RR-22-5; MF-06).
//!
//! What the other suites already show, and is not repeated here: rr-core's
//! `layer/explain/tests.rs` (hand-checked reports for every [`Change`] and
//! [`Action`], resets through replaced containers, keyed lists replaced and
//! removed with their container, appended lists dropped with it, and the
//! explained value against the merge for random stacks);
//! `layer_explain.rs` (the same over the sample ROM on bare stacks, and the
//! winner of `storage` through resolved chains, DAGs and diamonds);
//! `layer_cycle_diamond.rs` (a hand-checked diamond's winner and overridden
//! values with their files, and `explain` over a `MAX_LINEAGE` lineage).
//!
//! What is added here:
//!
//! * **Every leaf, every policy, against an independent oracle.** For
//!   random families (chain, DAG, diamond, each global or moved into local
//!   scopes) of partial or complete sample ROMs, every document's lineage
//!   from the resolver, and every path of the sample type over the whole
//!   key domain (so keys no layer mentions are included): [`Field`]s at
//!   every depth, the [`Nested`] scalings and keyed items, both [`Keyed`]
//!   lists and the [`Appended`] notes. The oracle walks each layer's own
//!   document independently of `explain` and says what that layer did to
//!   the field (nothing, applied its own state, reset it by replacing a
//!   container, removed it by omitting one). Checked:
//!   - the history (the winner, then everything it overrode) is exactly the
//!     layers that did something, strongest first, each with the change the
//!     oracle expects, the value *the very reference into that layer's own
//!     registered document* (pointer-equal), never a copy or another
//!     document's;
//!   - every step's layer is the registered [`Document`]'s layer for that
//!     lineage position, and its provenance is that layer's;
//!   - the explained value is the merged one (a `Field`'s winner's own set
//!     value), the explained presence of a `Nested` is the merged presence,
//!     each [`Action`] agrees with the merge of the lineage up to that
//!     layer, a `Keyed` list's keys are the merged list's, and an
//!     `Appended`'s live contributions and dropped ones split the history at
//!     the strongest replace;
//!   - a field no layer reaches reports "not set by any layer" (`Field`,
//!     `Appended`) or "not mentioned" (`Nested`), with no winner, and the
//!     merge has no value there either;
//!   - every report line names its step's provenance, in history order.
//!
//!   Counters fail the test if a shape, a policy's override, or any
//!   [`Action`] is never exercised.
//! * **MF-06 through the resolver**, hand-checked: a child file overriding
//!   a base file's storage type is the winner, with the base's value listed
//!   as overridden at the base file; a document outside the lineage that
//!   also sets it is never mentioned.
//! * **One realistic report per policy** (a four-file chain, the strongest a
//!   user file), as insta snapshots, checked by hand.
//!
//! **Provenance is layer-level.** A step's provenance is
//! [`Layer::provenance`]: the file, the span where that layer's document
//! starts, and the layer id. Partial documents carry no span per field, so
//! the guarantee is that a value is attributed to the right document (and
//! file and layer), not to the line of the field inside it.
//!
//! Legacy references are to RomRaider's Java sources under
//! `legacy/src/main/java/com/romraider` (read, never run; D-024).

use core::fmt::Debug;
use std::cell::Cell;
use std::sync::Arc;

use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
use rr_core::diag::Span;
use rr_core::encoding::{Endian, Storage};
use rr_core::layer::{
    Action, Appended, Change, Document, Explain, Explanation, Field, FieldPath, Keyed, Layer,
    LayerId, LayerKind, Layered, Lineage, Name, Nested, Registry, Step,
};
use rr_testkit::snapshot;
use rr_testkit::strategies::layered::sample::{
    self, AXIS_NAMES, Axis, Rom, Scaling, TABLE_NAMES, Table,
};
use rr_testkit::strategies::layered::{Family, chain, dag, diamond, scoped};

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

fn tables() -> FieldPath<Rom, Keyed<String, Table>> {
    FieldPath::root().field("tables", |r: &Rom| &r.tables)
}

fn table(t: &str) -> FieldPath<Rom, Table> {
    tables().item(t.to_owned())
}

fn table_scaling(t: &str) -> FieldPath<Rom, Nested<Scaling>> {
    table(t).field("scaling", |x: &Table| &x.scaling)
}

fn axes(t: &str) -> FieldPath<Rom, Keyed<String, Axis>> {
    table(t).field("axes", |x: &Table| &x.axes)
}

fn axis(t: &str, a: &str) -> FieldPath<Rom, Axis> {
    axes(t).item(a.to_owned())
}

fn axis_scaling(t: &str, a: &str) -> FieldPath<Rom, Nested<Scaling>> {
    axis(t, a).field("scaling", |x: &Axis| &x.scaling)
}

fn notes() -> FieldPath<Rom, Appended<String>> {
    FieldPath::root().field("notes", |r: &Rom| &r.notes)
}

// ---------------------------------------------------------------------------
// The oracle: what one layer's own document does to a field
// ---------------------------------------------------------------------------

/// Where a walk of one layer's own document ends, on the way to a
/// container `C`.
enum End<'a, C> {
    /// The container, as the layer states it.
    At(&'a C),
    /// A container on the way is not mentioned (absent or unset).
    Unmentioned,
    /// The layer omits the container at this path text.
    Omitted(String),
}

/// A walk of one layer's own document: where it ends, and the outermost
/// container on the way that the layer replaces.
struct Way<'a, C> {
    reset: Option<String>,
    end: End<'a, C>,
}

fn start(doc: &Rom) -> Way<'_, Rom> {
    Way {
        reset: None,
        end: End::At(doc),
    }
}

/// One container further, `text` being its path: a patch walks into it, a
/// replace walks into it and (if nothing outer did) resets everything
/// inherited below, an omit removes it.
fn into<'a, C, D>(
    way: Way<'a, C>,
    text: String,
    get: impl FnOnce(&'a C) -> Option<&'a Nested<D>>,
) -> Way<'a, D> {
    let Way { mut reset, end } = way;
    let end = match end {
        End::At(c) => match get(c) {
            None | Some(Nested::Unset) => End::Unmentioned,
            Some(Nested::Patch(d)) => End::At(d),
            Some(Nested::Replace(d)) => {
                reset.get_or_insert(text);
                End::At(d)
            }
            Some(Nested::Omit) => End::Omitted(text),
        },
        End::Unmentioned => End::Unmentioned,
        End::Omitted(by) => End::Omitted(by),
    };
    Way { reset, end }
}

fn table_way<'a>(doc: &'a Rom, t: &str) -> Way<'a, Table> {
    into(start(doc), format!("tables[{t:?}]"), |r| {
        r.tables.get(&t.to_owned())
    })
}

fn table_scaling_way<'a>(doc: &'a Rom, t: &str) -> Way<'a, Scaling> {
    into(table_way(doc, t), format!("tables[{t:?}].scaling"), |x| {
        Some(&x.scaling)
    })
}

fn axis_way<'a>(doc: &'a Rom, t: &str, a: &str) -> Way<'a, Axis> {
    into(
        table_way(doc, t),
        format!("tables[{t:?}].axes[{a:?}]"),
        |x| x.axes.get(&a.to_owned()),
    )
}

fn axis_scaling_way<'a>(doc: &'a Rom, t: &str, a: &str) -> Way<'a, Scaling> {
    let text = format!("tables[{t:?}].axes[{a:?}].scaling");
    into(axis_way(doc, t, a), text, |x| Some(&x.scaling))
}

/// What a layer did to the field `leaf` of the container `way` reaches:
/// `None` if nothing (no container on the way mentioned and the field
/// itself unset, or not reached).
fn expect<'a, C, L: Explain>(
    way: Way<'a, C>,
    leaf: impl FnOnce(&'a C) -> &'a L,
) -> Option<Change<'a, L>> {
    match (way.end, way.reset) {
        (End::Omitted(by), _) => Some(Change::Removed { by: by.into() }),
        (End::At(c), None) => {
            let l = leaf(c);
            (!l.is_noop()).then_some(Change::Apply(l))
        }
        (End::At(c), Some(by)) => Some(Change::Reset {
            by: by.into(),
            leaf: Some(leaf(c)),
        }),
        (End::Unmentioned, Some(by)) => Some(Change::Reset {
            by: by.into(),
            leaf: None,
        }),
        (End::Unmentioned, None) => None,
    }
}

/// The state of a `Nested` reached through `way` (a keyed item or a
/// nested scaling): the item state itself is the leaf.
fn expect_item<'a, C, P: rr_core::layer::Merge + Clone>(
    way: Way<'a, C>,
    get: impl FnOnce(&'a C) -> Option<&'a Nested<P>>,
) -> Option<Change<'a, Nested<P>>> {
    // An absent key is the same as an unset entry: not mentioned.
    match (way.end, way.reset) {
        (End::Omitted(by), _) => Some(Change::Removed { by: by.into() }),
        (End::At(c), None) => get(c).filter(|n| !n.is_noop()).map(Change::Apply),
        (End::At(c), Some(by)) => Some(Change::Reset {
            by: by.into(),
            leaf: get(c),
        }),
        (End::Unmentioned, Some(by)) => Some(Change::Reset {
            by: by.into(),
            leaf: None,
        }),
        (End::Unmentioned, None) => None,
    }
}

// ---------------------------------------------------------------------------
// Checks
// ---------------------------------------------------------------------------

/// What the property covered: each counter must end above zero.
#[derive(Debug, Default)]
struct Seen {
    chain: Cell<u32>,
    dag: Cell<u32>,
    diamond: Cell<u32>,
    scoped: Cell<u32>,
    /// A `Field` whose winner sets a value over at least one other layer's
    /// set value.
    field_override: Cell<u32>,
    /// MF-06: the winning storage type's signedness differs from an
    /// overridden one's.
    signedness_flip: Cell<u32>,
    /// A `Field` whose winner omits it, over a set value.
    field_omit_wins: Cell<u32>,
    /// A `Field` whose winner reset it by replacing a container.
    field_reset_wins: Cell<u32>,
    /// A `Field` whose winner removed it by omitting a container.
    field_removed_wins: Cell<u32>,
    /// A `Field` no layer reaches.
    field_unset: Cell<u32>,
    /// Each [`Action`] of a nested scaling or keyed item, in `ACTIONS` order.
    actions: [Cell<u32>; 7],
    /// A keyed item modified over an earlier layer's.
    keyed_modified: Cell<u32>,
    /// A keyed item restored after an omit.
    keyed_restored: Cell<u32>,
    /// An `Appended` with a replace dropping earlier contributions.
    appended_dropped: Cell<u32>,
    /// An `Appended` with two or more live contributions.
    appended_stacked: Cell<u32>,
    /// A document that completes: `Resolved::explain` checked too.
    resolved: Cell<u32>,
}

const ACTIONS: [Action; 7] = [
    Action::Added,
    Action::Restored,
    Action::Modified,
    Action::Replaced,
    Action::Omitted,
    Action::Cleared,
    Action::Removed,
];

fn bump(c: &Cell<u32>, when: bool) {
    if when {
        c.set(c.get() + 1);
    }
}

/// One lineage, with what the checks compare against.
struct Ctx<'l> {
    registry: &'l Registry<Rom>,
    lineage: &'l Lineage<Rom>,
    /// The merge of the lineage's first `i + 1` layers, for each `i`.
    prefixes: Vec<Rom>,
}

impl<'l> Ctx<'l> {
    fn new(registry: &'l Registry<Rom>, lineage: &'l Lineage<Rom>) -> Self {
        let mut prefixes = Vec::new();
        let mut stack = Layered::new();
        for (layer, doc) in lineage.stack().layers() {
            stack.push(layer.clone(), Arc::clone(doc));
            prefixes.push(stack.merged());
        }
        Self {
            registry,
            lineage,
            prefixes,
        }
    }

    fn merged(&self) -> &Rom {
        self.lineage.merged()
    }

    /// The merge of every layer below stack position `i`.
    fn before(&self, i: usize) -> Rom {
        i.checked_sub(1)
            .and_then(|j| self.prefixes.get(j).cloned())
            .unwrap_or_default()
    }
}

/// The checks every policy shares. Returns the stack position of each
/// step, strongest first.
fn check_steps<'e, L>(
    ctx: &Ctx<'_>,
    e: &Explanation<'e, L>,
    path: &FieldPath<Rom, L>,
    oracle: impl for<'a> Fn(&'a Rom) -> Option<Change<'a, L>>,
) -> Result<Vec<usize>, TestCaseError>
where
    L: Explain + Clone + PartialEq + Debug + 'static,
{
    prop_assert_eq!(e.path(), path.as_str());
    let stack: Vec<(&Layer, &Arc<Rom>)> = ctx.lineage.stack().layers().collect();
    let expected: Vec<(usize, Change<'_, L>)> = stack
        .iter()
        .enumerate()
        .filter_map(|(i, (_, doc))| oracle(doc).map(|c| (i, c)))
        .rev()
        .collect();
    let history: Vec<&Step<'e, L>> = e.history().collect();
    prop_assert_eq!(
        history.len(),
        expected.len(),
        "{}: {:?} vs {:?}",
        path,
        history,
        expected
    );
    for (step, (i, change)) in history.iter().zip(&expected) {
        let (layer, doc) = stack[*i];
        // The layer that did it, and what it did, by value...
        prop_assert_eq!(step.layer(), layer, "{}", path);
        prop_assert_eq!(step.change(), change, "{} at {}", path, layer.id());
        // ... and by identity: the value is borrowed from this layer's own
        // document, the registered one, never a copy or another's.
        match (step.change().leaf(), change.leaf()) {
            (Some(got), Some(want)) => prop_assert!(core::ptr::eq(got, want), "{}", path),
            (None, None) => {}
            _ => return Err(TestCaseError::fail(format!("{path}: leaf differs"))),
        }
        let name = &ctx.lineage.names()[*i];
        let registered = ctx
            .registry
            .get(name)
            .ok_or_else(|| TestCaseError::fail(format!("{name} not registered")))?;
        prop_assert!(Arc::ptr_eq(doc, registered.doc()), "{}", name);
        prop_assert_eq!(step.layer(), registered.layer());
        prop_assert_eq!(step.provenance(), registered.layer().provenance());
    }
    // The winner is the strongest, the rest are what it overrode.
    prop_assert_eq!(e.winner(), history.first().copied());
    let overridden: Vec<_> = e.overridden().collect();
    prop_assert_eq!(overridden.as_slice(), history.get(1..).unwrap_or_default());
    Ok(expected.into_iter().map(|(i, _)| i).collect())
}

/// Every line after the first of a `Field` or `Appended` report is a step
/// (or a heading): the steps' lines end in their provenance, in `order`.
fn check_lines<'e, L: 'e>(
    report: &str,
    order: impl Iterator<Item = &'e Step<'e, L>>,
    headings: &[&str],
) -> Result<(), TestCaseError> {
    let lines: Vec<&str> = report
        .lines()
        .skip(1)
        .filter(|l| !headings.contains(l))
        .collect();
    let provenances: Vec<String> = order.map(|s| format!(" at {}", s.provenance())).collect();
    prop_assert_eq!(lines.len(), provenances.len(), "{}", report);
    for (line, p) in lines.iter().zip(&provenances) {
        prop_assert!(line.ends_with(p.as_str()), "{} / {}", line, p);
    }
    Ok(())
}

/// A [`Field`]: the steps, the value, the report, and "unset" when no
/// layer reaches it.
fn check_field<T: Clone + PartialEq + Debug + 'static>(
    ctx: &Ctx<'_>,
    seen: &Seen,
    path: &FieldPath<Rom, Field<T>>,
    oracle: impl for<'a> Fn(&'a Rom) -> Option<Change<'a, Field<T>>>,
    signedness: impl Fn(&T) -> Option<bool>,
) -> Result<(), TestCaseError> {
    let e = ctx.lineage.explain(path);
    check_steps(ctx, &e, path, oracle)?;
    let merged = path.get(ctx.merged());
    prop_assert_eq!(e.value(), merged.and_then(Field::get), "{}", path);
    let winner_leaf = e.winner().and_then(|w| w.change().leaf());
    prop_assert_eq!(e.value(), winner_leaf.and_then(Field::get));
    let report = e.to_string();
    match e.value() {
        Some(v) => prop_assert!(
            report.starts_with(&format!("{path} = {v:?}\n")),
            "{}",
            report
        ),
        None => prop_assert!(
            report.starts_with(&format!("{path}: no value\n")),
            "{}",
            report
        ),
    }
    if e.winner().is_none() {
        prop_assert!(merged.is_none_or(Field::is_unset), "{}", path);
        prop_assert_eq!(
            &report,
            &format!("{path}: no value\n  not set by any layer")
        );
        bump(&seen.field_unset, true);
        return Ok(());
    }
    check_lines(&report, e.history(), &["  overrides:"])?;
    prop_assert_eq!(
        report.contains("\n  overrides:"),
        e.overridden().count() > 0
    );

    let set_below: Vec<&T> = e
        .overridden()
        .filter_map(|s| s.change().leaf().and_then(Field::get))
        .collect();
    let winner = e.winner().map(Step::change);
    bump(
        &seen.field_override,
        e.value().is_some() && !set_below.is_empty(),
    );
    if let Some(v) = e.value() {
        let flips = set_below.iter().any(|b| {
            signedness(b)
                .zip(signedness(v))
                .is_some_and(|(b, v)| b != v)
        });
        bump(&seen.signedness_flip, flips);
    }
    bump(
        &seen.field_omit_wins,
        matches!(winner, Some(Change::Apply(Field::Omit))) && !set_below.is_empty(),
    );
    bump(
        &seen.field_reset_wins,
        matches!(winner, Some(Change::Reset { .. })) && e.overridden().count() > 0,
    );
    bump(
        &seen.field_removed_wins,
        matches!(winner, Some(Change::Removed { .. })) && !set_below.is_empty(),
    );
    Ok(())
}

/// `true` if the item state at `path` in `doc` is present.
fn present_in<P: 'static>(path: &FieldPath<Rom, Nested<P>>, doc: &Rom) -> bool {
    path.get(doc).is_some_and(|n| n.get().is_some())
}

/// A [`Nested`] scaling or keyed item: the steps, each action against the
/// merge up to its layer, the presence and the report.
fn check_nested<P: rr_core::layer::Merge + Clone + PartialEq + Debug + 'static>(
    ctx: &Ctx<'_>,
    seen: &Seen,
    path: &FieldPath<Rom, Nested<P>>,
    oracle: impl for<'a> Fn(&'a Rom) -> Option<Change<'a, Nested<P>>>,
    keyed: bool,
) -> Result<(), TestCaseError> {
    let e = ctx.lineage.explain(path);
    let positions = check_steps(ctx, &e, path, oracle)?;
    let actions = e.actions();
    prop_assert_eq!(actions.len(), positions.len());
    // Weakest first, so each action is checked after the one below it.
    let mut below: Option<Action> = None;
    for ((step, action), &i) in actions.iter().zip(&positions).rev() {
        let before = present_in(path, &ctx.before(i));
        let after = present_in(path, &ctx.prefixes[i]);
        prop_assert_eq!(action.is_present(), after, "{} at {}", path, i);
        let leaf = step.change().leaf();
        let want = match (step.change(), leaf) {
            (Change::Removed { .. }, _) => Action::Removed,
            (_, Some(Nested::Omit)) => Action::Omitted,
            (_, Some(Nested::Unset) | None) => Action::Cleared,
            (Change::Apply(_), Some(Nested::Patch(_))) if before => Action::Modified,
            _ if before => Action::Replaced,
            _ if matches!(below, Some(Action::Omitted | Action::Removed)) => Action::Restored,
            _ => Action::Added,
        };
        prop_assert_eq!(*action, want, "{} at {}", path, i);
        if let Some(k) = ACTIONS.iter().position(|a| a == action) {
            bump(&seen.actions[k], true);
        }
        bump(&seen.keyed_modified, keyed && *action == Action::Modified);
        bump(&seen.keyed_restored, keyed && *action == Action::Restored);
        below = Some(*action);
    }
    prop_assert_eq!(e.is_present(), present_in(path, ctx.merged()), "{}", path);

    let report = e.to_string();
    let status = match actions.first().map(|(_, a)| *a) {
        None => "not mentioned",
        Some(Action::Omitted) => "omitted",
        Some(Action::Cleared) => "unset",
        Some(Action::Removed) => "absent",
        Some(_) => "present",
    };
    let mut lines = report.lines();
    let head = format!("{path}: {status}");
    prop_assert_eq!(lines.next(), Some(head.as_str()));
    for (line, (step, action)) in lines.zip(&actions) {
        prop_assert!(line.starts_with(&format!("  {action}")), "{}", report);
        prop_assert!(
            line.ends_with(&format!(" at {}", step.provenance())),
            "{}",
            report
        );
    }
    prop_assert_eq!(report.lines().count(), 1 + actions.len());
    if actions.is_empty() {
        prop_assert!(!present_in(path, ctx.merged()));
    }
    Ok(())
}

/// A [`Keyed`] list: the steps, the keys (the merged list's, in order),
/// and each item's explanation equal to explaining its path directly.
fn check_keyed<P: rr_core::layer::Merge + Clone + PartialEq + Debug + 'static>(
    ctx: &Ctx<'_>,
    path: &FieldPath<Rom, Keyed<String, P>>,
    oracle: impl for<'a> Fn(&'a Rom) -> Option<Change<'a, Keyed<String, P>>>,
    domain: &[&str],
) -> Result<(), TestCaseError> {
    let e = ctx.lineage.explain(path);
    check_steps(ctx, &e, path, oracle)?;
    let keys: Vec<&String> = path
        .get(ctx.merged())
        .into_iter()
        .flat_map(Keyed::entries)
        .map(|(k, _)| k)
        .collect();
    prop_assert_eq!(e.keys(), keys);
    for k in domain {
        let k = (*k).to_owned();
        prop_assert_eq!(
            e.item(&k),
            ctx.lineage.explain(&path.clone().key(k.clone()))
        );
    }
    let report = e.to_string();
    let head = format!("{path}: {} entries", e.keys().len());
    prop_assert_eq!(report.lines().next(), Some(head.as_str()));
    Ok(())
}

/// The [`Appended`] notes: the steps, the items, and the live and dropped
/// contributions split at the strongest replace.
fn check_appended(ctx: &Ctx<'_>, seen: &Seen) -> Result<(), TestCaseError> {
    let path = notes();
    let e = ctx.lineage.explain(&path);
    check_steps(ctx, &e, &path, |d| expect(start(d), |r| &r.notes))?;
    let merged: Vec<&String> = path
        .get(ctx.merged())
        .map(Appended::items)
        .unwrap_or_default()
        .iter()
        .collect();
    prop_assert_eq!(e.items(), merged.clone());
    let live: Vec<_> = e.live().collect();
    let dropped: Vec<_> = e.dropped().collect();
    let joined: Vec<_> = live.iter().chain(&dropped).copied().collect();
    let history: Vec<_> = e.history().collect();
    prop_assert_eq!(&joined, &history);
    let drops = |s: &Step<'_, Appended<String>>| match s.change() {
        Change::Apply(list) => list.replaces(),
        Change::Reset { .. } | Change::Removed { .. } => true,
    };
    // The weakest live step is the boundary: the one that drops what is
    // below it, unless nothing is below. Every live step above it appends.
    if let Some((boundary, above)) = live.split_last() {
        prop_assert!(dropped.is_empty() || drops(boundary));
        prop_assert!(above.iter().all(|s| !drops(s)));
    }
    let from_live: Vec<&String> = live
        .iter()
        .rev()
        .filter_map(|s| s.change().leaf())
        .flat_map(Appended::items)
        .collect();
    prop_assert_eq!(from_live, merged);
    let report = e.to_string();
    let head = format!("notes = {:?}", e.items());
    prop_assert!(report.starts_with(&head), "{}", report);
    if history.is_empty() {
        prop_assert_eq!(&report, &format!("{head}\n  not set by any layer"));
    } else {
        check_lines(
            &report,
            live.iter().chain(&dropped).copied(),
            &["  dropped:"],
        )?;
        prop_assert_eq!(report.contains("\n  dropped:"), !dropped.is_empty());
    }
    bump(&seen.appended_dropped, !dropped.is_empty());
    bump(
        &seen.appended_stacked,
        live.iter()
            .filter(|s| s.change().leaf().is_some_and(|l| !l.items().is_empty()))
            .count()
            >= 2,
    );
    Ok(())
}

/// `Some(true)` for a signed integer storage type, `Some(false)` for an
/// unsigned one, `None` for a float (MF-06 is about integer signedness).
fn signed(s: &Storage) -> Option<bool> {
    match s.name().as_bytes().first() {
        Some(b'i') => Some(true),
        Some(b'u') => Some(false),
        _ => None,
    }
}

/// No signedness: any other field type.
fn none<T>(_: &T) -> Option<bool> {
    None
}

/// Every check, for every path of the sample type, over one lineage.
fn check_lineage(ctx: &Ctx<'_>, seen: &Seen) -> Result<(), TestCaseError> {
    let xmlid = FieldPath::root().field("xmlid", |r: &Rom| &r.xmlid);
    check_field(ctx, seen, &xmlid, |d| expect(start(d), |r| &r.xmlid), none)?;
    check_keyed(
        ctx,
        &tables(),
        |d| expect(start(d), |r| &r.tables),
        &TABLE_NAMES,
    )?;
    check_appended(ctx, seen)?;
    for t in TABLE_NAMES {
        let key = tables().key(t.to_owned());
        check_nested(
            ctx,
            seen,
            &key,
            |d| expect_item(start(d), |r| r.tables.get(&t.to_owned())),
            true,
        )?;
        let tb = table(t);
        check_field(
            ctx,
            seen,
            &tb.clone().field("storage", |x: &Table| &x.storage),
            |d| expect(table_way(d, t), |x| &x.storage),
            signed,
        )?;
        check_field(
            ctx,
            seen,
            &tb.clone().field("endian", |x: &Table| &x.endian),
            |d| expect(table_way(d, t), |x| &x.endian),
            none::<Endian>,
        )?;
        check_field(
            ctx,
            seen,
            &tb.clone().field("address", |x: &Table| &x.address),
            |d| expect(table_way(d, t), |x| &x.address),
            none::<u32>,
        )?;
        check_nested(
            ctx,
            seen,
            &table_scaling(t),
            |d| expect_item(table_way(d, t), |x| Some(&x.scaling)),
            false,
        )?;
        check_scaling(ctx, seen, &table_scaling(t), |d| table_scaling_way(d, t))?;
        check_keyed(
            ctx,
            &axes(t),
            |d| expect(table_way(d, t), |x| &x.axes),
            &AXIS_NAMES,
        )?;
        for a in AXIS_NAMES {
            check_nested(
                ctx,
                seen,
                &axes(t).key(a.to_owned()),
                |d| expect_item(table_way(d, t), |x| x.axes.get(&a.to_owned())),
                true,
            )?;
            let ax = axis(t, a);
            check_field(
                ctx,
                seen,
                &ax.clone().field("storage", |x: &Axis| &x.storage),
                |d| expect(axis_way(d, t, a), |x| &x.storage),
                signed,
            )?;
            check_field(
                ctx,
                seen,
                &ax.clone().field("address", |x: &Axis| &x.address),
                |d| expect(axis_way(d, t, a), |x| &x.address),
                none::<u32>,
            )?;
            check_field(
                ctx,
                seen,
                &ax.field("len", |x: &Axis| &x.len),
                |d| expect(axis_way(d, t, a), |x| &x.len),
                none::<u16>,
            )?;
            check_nested(
                ctx,
                seen,
                &axis_scaling(t, a),
                |d| expect_item(axis_way(d, t, a), |x| Some(&x.scaling)),
                false,
            )?;
            check_scaling(ctx, seen, &axis_scaling(t, a), |d| {
                axis_scaling_way(d, t, a)
            })?;
        }
    }
    Ok(())
}

/// The three fields of a nested [`Scaling`].
fn check_scaling(
    ctx: &Ctx<'_>,
    seen: &Seen,
    path: &FieldPath<Rom, Nested<Scaling>>,
    way: impl for<'a> Fn(&'a Rom) -> Way<'a, Scaling>,
) -> Result<(), TestCaseError> {
    let s = path.clone().nested();
    check_field(
        ctx,
        seen,
        &s.clone().field("units", |x: &Scaling| &x.units),
        |d| expect(way(d), |x| &x.units),
        none::<String>,
    )?;
    check_field(
        ctx,
        seen,
        &s.clone().field("expr", |x: &Scaling| &x.expr),
        |d| expect(way(d), |x| &x.expr),
        none::<String>,
    )?;
    check_field(
        ctx,
        seen,
        &s.field("format", |x: &Scaling| &x.format),
        |d| expect(way(d), |x| &x.format),
        none::<String>,
    )
}

// ---------------------------------------------------------------------------
// The property
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Chain,
    Dag,
    Diamond,
}

/// Every valid shape, global or moved into local scopes, of partial or
/// complete sample ROMs.
fn families() -> BoxedStrategy<(Kind, Family<Rom>)> {
    let doc = prop_oneof![sample::rom(), sample::complete_rom()].boxed();
    prop_oneof![
        chain(doc.clone(), 1..=5).prop_map(|f| (Kind::Chain, f)),
        scoped(chain(doc.clone(), 1..=5)).prop_map(|f| (Kind::Chain, f)),
        dag(doc.clone(), 1..=6).prop_map(|f| (Kind::Dag, f)),
        scoped(dag(doc.clone(), 1..=6)).prop_map(|f| (Kind::Dag, f)),
        diamond(doc.clone()).prop_map(|d| (Kind::Diamond, d.family)),
        scoped(diamond(doc)).prop_map(|d| (Kind::Diamond, d.family)),
    ]
    .boxed()
}

/// For every document of the family, every path of the sample type: the
/// winner, the overridden values and their provenance are what the
/// independent oracle says, and agree with the merge.
fn check_family(kind: Kind, family: &Family<Rom>, seen: &Seen) -> Result<(), TestCaseError> {
    let registry = family
        .registry()
        .map_err(|e| TestCaseError::fail(e.to_string()))?;
    let mut resolver = registry.resolver();
    for d in family.docs() {
        let lineage = resolver
            .lineage(d.name())
            .map_err(|e| TestCaseError::fail(e.to_string()))?;
        let ctx = Ctx::new(&registry, &lineage);
        check_lineage(&ctx, seen)?;
        // A document that completes explains the same through `Resolved`.
        if let Ok(resolved) = resolver.resolve(d.name()) {
            for path in [table(TABLE_NAMES[0]), table(TABLE_NAMES[2])] {
                let storage = path.field("storage", |x: &Table| &x.storage);
                prop_assert_eq!(resolved.explain(&storage), lineage.explain(&storage));
            }
            prop_assert_eq!(resolved.explain(&notes()), lineage.explain(&notes()));
            bump(&seen.resolved, true);
        }
    }
    bump(&seen.chain, kind == Kind::Chain && family.len() > 1);
    bump(&seen.dag, kind == Kind::Dag && family.len() > 2);
    bump(&seen.diamond, kind == Kind::Diamond);
    bump(&seen.scoped, !family.top().scope().is_global());
    Ok(())
}

/// MF-06 and every policy: through the resolver, `explain` reports the
/// winning layer, every value it overrode (strongest first, with the value
/// borrowed from that layer's own registered document) and each one's
/// layer provenance, for every leaf of the sample type; a field no layer
/// reaches reports so. See the module docs for the full list.
#[test]
fn explain_reports_winner_and_overridden_values_with_provenance_for_every_leaf() {
    let seen = Seen::default();
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: 256,
            ..Config::default()
        },
        TestRng::deterministic_rng(RngAlgorithm::ChaCha),
    );
    let result = runner.run(&families(), |(kind, family)| {
        check_family(kind, &family, &seen)
    });
    if let Err(e) = result {
        panic!("{e}");
    }
    let mut counters = vec![
        ("chain", &seen.chain),
        ("dag", &seen.dag),
        ("diamond", &seen.diamond),
        ("scoped", &seen.scoped),
        ("Field: a set value overridden", &seen.field_override),
        ("MF-06: signedness overridden", &seen.signedness_flip),
        (
            "Field: an omit wins over a set value",
            &seen.field_omit_wins,
        ),
        ("Field: a replaced container wins", &seen.field_reset_wins),
        ("Field: an omitted container wins", &seen.field_removed_wins),
        ("Field: set by no layer", &seen.field_unset),
        ("Keyed: an item modified", &seen.keyed_modified),
        ("Keyed: an item restored", &seen.keyed_restored),
        (
            "Appended: a replace dropped contributions",
            &seen.appended_dropped,
        ),
        ("Appended: stacked contributions", &seen.appended_stacked),
        ("Resolved::explain", &seen.resolved),
    ];
    for (a, c) in ACTIONS.iter().zip(&seen.actions) {
        counters.push((a.as_str(), c));
    }
    for (what, c) in counters {
        assert!(c.get() > 0, "never exercised: {what} ({seen:?})");
    }
}

// ---------------------------------------------------------------------------
// Hand-checked
// ---------------------------------------------------------------------------

fn file(id: &str, line: u32, col: u32) -> Layer {
    Layer::new(
        LayerId::new(id),
        LayerKind::File,
        format!("defs/{id}.xml"),
        Span::line_col(line, col),
    )
}

fn with_table(t: &str, table: Table) -> Rom {
    let mut rom = Rom::default();
    rom.tables.patch(t.to_owned(), table);
    rom
}

fn registry_of(docs: impl IntoIterator<Item = Document<Rom>>) -> Registry<Rom> {
    let mut reg = Registry::new();
    for d in docs {
        let name = d.name().clone();
        assert!(reg.insert(d).is_ok(), "{name} is unique");
    }
    reg
}

/// MF-06: a child file that overrides a base file's storage type, signed
/// over unsigned, is the winner, and the base's value is listed as
/// overridden at the base file; the inherited address names the base
/// alone. `32BITBASE` also sets the storage type, in another file, but is
/// not in the lineage, so it is never mentioned. Legacy inherited
/// signedness only while the base's storage type was unset
/// (`xml/TableScaleUnmarshaller.java:153`).
#[test]
fn mf06_signedness_override_through_the_resolver_names_child_and_base_files() {
    let base = with_table(
        "Boost Target",
        Table {
            storage: Field::Set(Storage::U16),
            address: Field::Set(0x300),
            ..Table::default()
        },
    );
    let child = with_table(
        "Boost Target",
        Table {
            storage: Field::Set(Storage::I16),
            ..Table::default()
        },
    );
    let other = with_table(
        "Boost Target",
        Table {
            storage: Field::Set(Storage::I32),
            ..Table::default()
        },
    );
    let reg = registry_of([
        Document::new(file("32BITBASE", 7, 1), other),
        Document::new(file("16BITBASE", 12, 3), base),
        Document::new(file("A2WC522N", 40, 5), child).with_base("16BITBASE"),
    ]);
    let mut resolver = reg.resolver();
    let lineage = resolver
        .lineage(&Name::global("A2WC522N"))
        .expect("the chain resolves");
    let storage = table("Boost Target").field("storage", |t: &Table| &t.storage);
    let e = lineage.explain(&storage);
    assert_eq!(e.value(), Some(&Storage::I16));
    assert_eq!(
        e.to_string(),
        "tables[\"Boost Target\"].storage = I16\n  \
         set to I16 at defs/A2WC522N.xml:40:5 [layer A2WC522N]\n  \
         overrides:\n    \
         set to U16 at defs/16BITBASE.xml:12:3 [layer 16BITBASE]"
    );
    let address = table("Boost Target").field("address", |t: &Table| &t.address);
    assert_eq!(
        lineage.explain(&address).to_string(),
        "tables[\"Boost Target\"].address = 768\n  \
         set to 768 at defs/16BITBASE.xml:12:3 [layer 16BITBASE]"
    );
    let endian = table("Boost Target").field("endian", |t: &Table| &t.endian);
    assert_eq!(
        lineage.explain(&endian).to_string(),
        "tables[\"Boost Target\"].endian: no value\n  not set by any layer"
    );
}

/// A realistic four-file chain: `32BITBASE` (every table), `16BITBASE`
/// (narrows Fuel Map, replaces its scaling, omits Timing, restarts the
/// notes), `A2WC522N` (MF-06 signed Fuel Map, patches the scaling's format,
/// omits axis Y and adds Z, restores Timing, adds Idle Speed) and the
/// user's own override file (moves Fuel Map, omits its scaling).
fn four_file_chain() -> Registry<Rom> {
    let scaling = |units: &str, expr: &str| Scaling {
        units: Field::Set(units.to_owned()),
        expr: Field::Set(expr.to_owned()),
        ..Scaling::default()
    };
    let axis_at = |address| Axis {
        storage: Field::Set(Storage::U16),
        address: Field::Set(address),
        len: Field::Set(16),
        ..Axis::default()
    };
    let notes = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();

    let mut base = Rom {
        xmlid: Field::Set("32BITBASE".to_owned()),
        notes: Appended::append(notes(&["32-bit base"])),
        ..Rom::default()
    };
    let mut fuel = Table {
        storage: Field::Set(Storage::U32),
        endian: Field::Set(Endian::Big),
        address: Field::Set(0x100),
        scaling: Nested::Patch(scaling("ms", "x*0.1")),
        ..Table::default()
    };
    fuel.axes.patch("X".to_owned(), axis_at(0x180));
    fuel.axes.patch("Y".to_owned(), axis_at(0x1a0));
    base.tables.patch("Fuel Map".to_owned(), fuel);
    for (t, address) in [("Timing", 0x200), ("Boost Target", 0x300)] {
        let table = Table {
            storage: Field::Set(Storage::U8),
            endian: Field::Set(Endian::Big),
            address: Field::Set(address),
            ..Table::default()
        };
        base.tables.patch(t.to_owned(), table);
    }

    let mut narrow = Rom {
        xmlid: Field::Set("16BITBASE".to_owned()),
        notes: Appended::replace(notes(&["16-bit"])),
        ..Rom::default()
    };
    let fuel = Table {
        storage: Field::Set(Storage::U16),
        scaling: Nested::Replace(scaling("kPa", "x/4-40")),
        ..Table::default()
    };
    narrow.tables.patch("Fuel Map".to_owned(), fuel);
    narrow.tables.omit("Timing".to_owned());

    let mut rom = Rom {
        xmlid: Field::Set("A2WC522N".to_owned()),
        notes: Appended::append(notes(&["A2WC522N fix"])),
        ..Rom::default()
    };
    let mut fuel = Table {
        storage: Field::Set(Storage::I16),
        scaling: Nested::Patch(Scaling {
            format: Field::Set("0.0".to_owned()),
            ..Scaling::default()
        }),
        ..Table::default()
    };
    fuel.axes.omit("Y".to_owned());
    fuel.axes.patch("Z".to_owned(), axis_at(0x1c0));
    rom.tables.patch("Fuel Map".to_owned(), fuel);
    let timing = Table {
        storage: Field::Set(Storage::I8),
        endian: Field::Set(Endian::Little),
        address: Field::Set(0x2400),
        ..Table::default()
    };
    rom.tables.patch("Timing".to_owned(), timing);
    let idle = Table {
        storage: Field::Set(Storage::U8),
        endian: Field::Set(Endian::Big),
        address: Field::Set(0x500),
        ..Table::default()
    };
    rom.tables.patch("Idle Speed".to_owned(), idle);

    let mut user = Rom {
        notes: Appended::append(notes(&["user"])),
        ..Rom::default()
    };
    let fuel = Table {
        address: Field::Set(0x1100),
        scaling: Nested::Omit,
        ..Table::default()
    };
    user.tables.patch("Fuel Map".to_owned(), fuel);
    let user_layer = Layer::new(
        LayerId::new("user"),
        LayerKind::User,
        "user/A2WC522N.toml",
        Span::line_col(1, 1),
    );

    registry_of([
        Document::new(file("32BITBASE", 2, 1), base),
        Document::new(file("16BITBASE", 5, 3), narrow).with_base("32BITBASE"),
        Document::new(file("A2WC522N", 9, 5), rom).with_base("16BITBASE"),
        Document::new(user_layer, user).with_base("A2WC522N"),
    ])
}

/// One realistic report per policy, from [`four_file_chain`]: `Field`
/// (the MF-06 storage type, with every value it overrode), `Nested` (the
/// Fuel Map scaling: added, replaced, modified, omitted; and the Timing
/// table: added, omitted, restored), `Keyed` (the tables and the Fuel Map
/// axes) and `Appended` (the notes, the 32-bit base's dropped by the 16-bit
/// replace). Each snapshot was checked by hand against the chain.
#[test]
fn one_realistic_report_per_policy() {
    let reg = four_file_chain();
    let mut resolver = reg.resolver();
    let lineage = resolver
        .lineage(&Name::global("user"))
        .expect("the chain resolves");
    let fuel = "Fuel Map";

    let storage = table(fuel).field("storage", |t: &Table| &t.storage);
    let address = table(fuel).field("address", |t: &Table| &t.address);
    let units = table_scaling(fuel)
        .nested()
        .field("units", |s: &Scaling| &s.units);
    let field = [
        lineage.explain(&storage).to_string(),
        lineage.explain(&address).to_string(),
        lineage.explain(&units).to_string(),
    ]
    .join("\n\n");
    let nested = [
        lineage.explain(&table_scaling(fuel)).to_string(),
        lineage
            .explain(&tables().key("Timing".to_owned()))
            .to_string(),
    ]
    .join("\n\n");
    let keyed = [
        lineage.explain(&tables()).to_string(),
        lineage.explain(&axes(fuel)).to_string(),
    ]
    .join("\n\n");
    let appended = lineage.explain(&notes()).to_string();

    snapshot::settings().bind(|| {
        insta::assert_snapshot!("field", field);
        insta::assert_snapshot!("nested", nested);
        insta::assert_snapshot!("keyed", keyed);
        insta::assert_snapshot!("appended", appended);
    });
}
