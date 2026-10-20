//! Resolution laws through the public `Registry` / `Resolver` path
//! (US-RR-22-1; MF-06).
//!
//! The merge laws (`rr_core::layer` module docs, *Laws*) are proptested on
//! `merge` itself in `rr-core`'s unit tests. Here they are checked where a
//! caller meets them: generated families of documents with `base`
//! references (chains, DAGs, diamonds, and each moved into local scopes),
//! put in a registry and resolved.
//!
//! * **Deterministic**: the same documents give the same lineage, the same
//!   merged document and the same finished value (or the same error),
//!   whatever order they are inserted in, whatever order they are asked for
//!   in, from a fresh or a warm resolver, and however they are split between
//!   files.
//! * **Idempotent**: resolving a name twice gives the same result (the same
//!   cached lineage); a resolved document registered again as one layer
//!   resolves to itself; and layering a resolved document on top of itself
//!   is a fixed point for every policy except append. An [`Appended`] list
//!   on itself has its items twice (`a ⊕ a ≠ a`, the one law append does not
//!   keep), which the test asserts rather than skips.
//! * **Override wins**: whenever the strongest layer of a lineage that
//!   mentions a field overrides it, the resolved value is that layer's, for
//!   every policy: replace ([`Field`] of a string, an enum, an integer, a
//!   bool, a float, the storage type), deep-merge ([`Nested`] patch, replace
//!   and omit), keyed item ([`Keyed`] replace and omit) and append replace.
//!   Signedness is the storage type, overridden like any other field (MF-06):
//!   legacy applied a child's sign only while the inherited storage type was
//!   unset (`xml/TableScaleUnmarshaller.java:153`,
//!   `if (table.getStorageType() < 1)`), so a child of a base that set the
//!   storage type kept the base's sign.
//!
//! The override properties run on a deterministic runner and count what
//! they generated, so they fail if a policy, a shape or the legacy case (an
//! inherited storage type of the other signedness) is never exercised.
//!
//! Legacy references are to RomRaider's Java sources under
//! `legacy/src/main/java/com/romraider` (read, never run; D-024).

use std::cell::Cell;
use std::sync::Arc;

use proptest::prelude::*;
use proptest::sample::subsequence;
use proptest::test_runner::{Config, TestRunner};
use rr_core::encoding::{Endian, Storage};
use rr_core::layer::{
    Appended, Document, Field, Keyed, Layer, LayerId, Lineage, Merge, Name, Nested, Registry,
    ResolveError, Resolver,
};
use rr_testkit::strategies::layered::sample::{self, Rom, Scaling, TABLE_NAMES, Table};
use rr_testkit::strategies::layered::{
    Family, FieldWeights, chain, dag, diamond, field_with, keyed, scoped,
};

// ---------------------------------------------------------------------------
// Families
// ---------------------------------------------------------------------------

/// The shape a family was generated as, for coverage counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Chain,
    Dag,
    Diamond,
}

/// Every valid shape, global or moved into local scopes, of `doc`s.
fn families<P: Clone + core::fmt::Debug + 'static>(
    doc: impl Strategy<Value = P> + Clone + 'static,
) -> BoxedStrategy<(Kind, Family<P>)> {
    prop_oneof![
        chain(doc.clone(), 1..=6).prop_map(|f| (Kind::Chain, f)),
        scoped(chain(doc.clone(), 1..=6)).prop_map(|f| (Kind::Chain, f)),
        dag(doc.clone(), 1..=8).prop_map(|f| (Kind::Dag, f)),
        scoped(dag(doc.clone(), 1..=8)).prop_map(|f| (Kind::Dag, f)),
        diamond(doc.clone()).prop_map(|d| (Kind::Diamond, d.family)),
        scoped(diamond(doc)).prop_map(|d| (Kind::Diamond, d.family)),
    ]
    .boxed()
}

/// Sample ROM documents: partial ones (any field state) and complete ones,
/// so some lineages finish and some report what is missing.
fn roms() -> BoxedStrategy<Rom> {
    prop_oneof![sample::rom(), sample::complete_rom()].boxed()
}

/// `d` with its document replaced by `doc`: same layer, scope and bases.
fn with_doc<P>(d: &Document<P>, doc: P) -> Document<P> {
    let moved = Document::new(d.layer().clone(), doc).in_scope(d.name().scope().clone());
    d.bases().iter().cloned().fold(moved, Document::with_base)
}

/// `d` unchanged except that its layer's file is `file`.
fn in_file<P>(d: &Document<P>, file: &str) -> Document<P> {
    let l = d.layer();
    let layer = Layer::new(l.id().clone(), l.kind(), file, l.span());
    let moved = Document::new(layer, Arc::clone(d.doc())).in_scope(d.name().scope().clone());
    d.bases().iter().cloned().fold(moved, Document::with_base)
}

/// The id of the empty document [`with_heir`] puts on top: no generated id
/// has this form (`<stem>-<i>`).
const HEIR: &str = "heir";

/// The family's documents, document `top`'s replaced by `doc`, plus an empty
/// document [`HEIR`] in `top`'s scope with `top` as its one base: the
/// override sits one level down a base chain, under a layer that does not
/// mention it.
fn with_heir<P: Default>(family: &Family<P>, top: usize, doc: P) -> (Vec<Document<P>>, Name) {
    let mut docs: Vec<Document<P>> = family
        .docs()
        .iter()
        .map(|d| in_file(d, d.layer().source()))
        .collect();
    docs[top] = with_doc(&family.docs()[top], doc);
    let scope = family.name(top).scope().clone();
    let heir_layer = Layer::new(
        LayerId::new(HEIR),
        family.docs()[top].layer().kind(),
        "defs/heir.xml",
        family.docs()[top].layer().span(),
    );
    docs.push(
        Document::new(heir_layer, P::default())
            .in_scope(scope.clone())
            .with_base(family.name(top).id().clone()),
    );
    (docs, Name::new(scope, HEIR))
}

fn registry_of<P>(
    docs: impl IntoIterator<Item = Document<P>>,
) -> Result<Registry<P>, TestCaseError> {
    let mut out = Registry::new();
    for d in docs {
        out.insert(d)
            .map_err(|e| TestCaseError::fail(e.to_string()))?;
    }
    Ok(out)
}

fn lineage_of<P: Merge + Clone>(
    resolver: &mut Resolver<'_, P>,
    name: &Name,
) -> Result<Arc<Lineage<P>>, TestCaseError> {
    resolver
        .lineage(name)
        .map_err(|e| TestCaseError::fail(format!("{name}: {e}")))
}

/// Every layer of `lineage` but the top one, merged: what the top document
/// inherits.
fn inherited<P: Merge + Clone>(lineage: &Lineage<P>) -> P {
    let mut out = P::default();
    let n = lineage.stack().len();
    for (_, doc) in lineage.stack().layers().take(n.saturating_sub(1)) {
        out.merge_from(doc);
    }
    out
}

// ---------------------------------------------------------------------------
// Deterministic
// ---------------------------------------------------------------------------

proptest! {
    /// The same registry resolves to the same lineage and value whatever
    /// the insertion order (a random permutation), the order names are
    /// asked for, a fresh or warm resolver, and the split between files
    /// (every document in its own file, or all in one).
    #[test]
    fn resolution_is_deterministic(
        (family, insert, query) in families(roms()).prop_flat_map(|(_, f)| {
            let ids: Vec<usize> = (0..f.len()).collect();
            (Just(f), Just(ids.clone()).prop_shuffle(), Just(ids).prop_shuffle())
        })
    ) {
        let forward = family.registry().map_err(|e| TestCaseError::fail(e.to_string()))?;
        let shuffled = family
            .registry_in_order(&insert)
            .map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&forward, &shuffled);
        let one_file = registry_of(family.docs().iter().map(|d| in_file(d, "defs/pack.xml")))?;
        let mut warm = shuffled.resolver();
        let mut single = one_file.resolver();
        for &q in &query {
            let n = family.name(q);
            let fresh = forward.resolver().lineage(n);
            let cached = warm.lineage(n);
            prop_assert_eq!(fresh.as_deref(), cached.as_deref(), "{}", n);
            let split = single.lineage(n);
            let (Ok(l), Ok(s)) = (&fresh, &split) else {
                return Err(TestCaseError::fail(format!("{n}: {fresh:?} / {split:?}")));
            };
            prop_assert_eq!(l.names(), s.names());
            prop_assert_eq!(l.merged(), s.merged());
            let value = forward.resolver().resolve(n);
            prop_assert_eq!(&value, &warm.resolve(n));
            let split = single.resolve(n);
            prop_assert_eq!(
                value.as_ref().map(|r| r.value()).map_err(ResolveError::code),
                split.as_ref().map(|r| r.value()).map_err(ResolveError::code)
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Idempotent
// ---------------------------------------------------------------------------

proptest! {
    /// Resolving twice gives the same value and the same cached lineage; a
    /// resolved document registered as one layer resolves to itself, value
    /// included; and that document as a child of itself is a fixed point,
    /// except for appended notes: append is the one policy that is not
    /// idempotent (`rr_core::layer` *Laws*), so its items come out twice.
    #[test]
    fn resolution_is_idempotent((_, family) in families(roms())) {
        let reg = family.registry().map_err(|e| TestCaseError::fail(e.to_string()))?;
        let mut resolver = reg.resolver();
        for d in family.docs() {
            let n = d.name();
            let once = resolver.resolve(n);
            let twice = resolver.resolve(n);
            prop_assert_eq!(&once, &twice);
            if let (Ok(a), Ok(b)) = (&once, &twice) {
                prop_assert!(Arc::ptr_eq(a.lineage(), b.lineage()));
            }

            let merged = Rom::clone(lineage_of(&mut resolver, n)?.merged());
            let flat = Layer::new(LayerId::new("flat"), d.layer().kind(), "defs/flat.xml", d.layer().span());
            let again = Layer::new(LayerId::new("again"), d.layer().kind(), "defs/again.xml", d.layer().span());
            let fixed = registry_of([
                Document::new(flat, merged.clone()),
                Document::new(again, merged.clone()).with_base("flat"),
            ])?;
            let mut r = fixed.resolver();
            let flat_name = Name::global("flat");
            let as_one = lineage_of(&mut r, &flat_name)?;
            prop_assert_eq!(&**as_one.merged(), &merged);
            prop_assert_eq!(
                r.resolve(&flat_name).map(|x| x.into_value()).map_err(|e| e.code()),
                once.map(|x| x.into_value()).map_err(|e| e.code())
            );

            let on_itself = Rom::clone(lineage_of(&mut r, &Name::global("again"))?.merged());
            if merged.notes.replaces() || merged.notes.items().is_empty() {
                prop_assert_eq!(&on_itself, &merged, "a ⊕ a == a");
            } else {
                // The append exclusion: an appending list on itself repeats.
                let twice: Vec<String> =
                    merged.notes.items().iter().chain(merged.notes.items()).cloned().collect();
                prop_assert_eq!(on_itself.notes.items(), &twice[..]);
                prop_assert!(!on_itself.notes.replaces());
                let strip = |r: &Rom| Rom { notes: Appended::default(), ..r.clone() };
                prop_assert_eq!(strip(&on_itself), strip(&merged), "idempotent but for append");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Override wins: every policy of the sample ROM, injected in a child
// ---------------------------------------------------------------------------

/// The overrides a child makes, one per policy, each on its own field.
#[derive(Debug, Clone)]
struct Overrides {
    /// Replace, on the document: set or omitted.
    xmlid: Field<String>,
    /// The table whose replace fields are overridden, in a patch that
    /// leaves its scaling and axes random.
    fields_of: String,
    /// MF-06: the storage type, so the signedness, always set.
    storage: Storage,
    endian: Field<Endian>,
    address: Field<u32>,
    rest: Table,
    /// The table whose scaling is overridden (deep-merge): patch, replace
    /// or omit, in a patch of the table.
    scaling_of: String,
    scaling: Nested<Scaling>,
    /// The table replaced or omitted as a whole (keyed item).
    item: String,
    item_state: Nested<Table>,
    /// Append policy: the child replaces the inherited notes.
    notes: Vec<String>,
}

impl Overrides {
    /// The child document: exactly these overrides.
    fn child(&self) -> Rom {
        let mut tables = Keyed::new();
        tables.patch(
            self.fields_of.clone(),
            Table {
                storage: Field::Set(self.storage),
                endian: self.endian,
                address: self.address,
                ..self.rest.clone()
            },
        );
        tables.patch(
            self.scaling_of.clone(),
            Table {
                scaling: self.scaling.clone(),
                ..Table::default()
            },
        );
        tables.insert(self.item.clone(), self.item_state.clone());
        Rom {
            xmlid: self.xmlid.clone(),
            tables,
            notes: Appended::replace(self.notes.clone()),
        }
    }
}

fn overrides() -> BoxedStrategy<Overrides> {
    let text = |v: &'static [&'static str]| proptest::sample::select(v).prop_map(str::to_owned);
    let rest = (
        prop_oneof![
            Just(Nested::Unset),
            sample::scaling().prop_map(Nested::Patch)
        ],
        keyed(text(&sample::AXIS_NAMES), sample::axis(), 0..=2),
    )
        .prop_map(|(scaling, axes)| Table {
            scaling,
            axes,
            ..Table::default()
        });
    let names = subsequence(TABLE_NAMES.to_vec(), 3).prop_shuffle();
    (
        field_with(
            text(&["A2WC522N", "16BITBASE", "child"]),
            FieldWeights::OVERRIDE,
        ),
        names,
        sample::storage(),
        field_with(sample::endian(), FieldWeights::OVERRIDE),
        field_with(sample::address(), FieldWeights::OVERRIDE),
        rest,
        prop_oneof![
            sample::scaling().prop_map(Nested::Patch),
            sample::scaling().prop_map(Nested::Replace),
            Just(Nested::Omit),
        ],
        prop_oneof![
            sample::table().prop_map(Nested::Replace),
            Just(Nested::Omit)
        ],
        proptest::collection::vec(text(&["child", "fix"]), 0..=2),
    )
        .prop_map(
            |(xmlid, names, storage, endian, address, rest, scaling, item_state, notes)| {
                Overrides {
                    xmlid,
                    fields_of: names[0].to_owned(),
                    storage,
                    endian,
                    address,
                    rest,
                    scaling_of: names[1].to_owned(),
                    scaling,
                    item: names[2].to_owned(),
                    item_state,
                    notes,
                }
            },
        )
        .boxed()
}

/// What the override cases covered: each counter must end above zero.
#[derive(Debug, Default)]
struct Seen {
    chain: Cell<u32>,
    diamond: Cell<u32>,
    /// MF-06, the legacy case: the inherited storage type is set, with the
    /// other signedness, and the child's still wins. Both directions.
    to_signed: Cell<u32>,
    to_unsigned: Cell<u32>,
    /// Each override changed an inherited value (non-vacuous).
    xmlid_set: Cell<u32>,
    xmlid_omit: Cell<u32>,
    endian_set: Cell<u32>,
    endian_omit: Cell<u32>,
    address_set: Cell<u32>,
    address_omit: Cell<u32>,
    scaling_patch: Cell<u32>,
    scaling_replace: Cell<u32>,
    scaling_omit: Cell<u32>,
    item_replace: Cell<u32>,
    item_omit: Cell<u32>,
    notes_replace: Cell<u32>,
    /// MF-06 in the finished value: a child whose only change is a
    /// table's storage type, over an inherited storage type of the other
    /// signedness, resolved to a finished table of the child's storage.
    completed: Cell<u32>,
}

fn bump(c: &Cell<u32>, when: bool) {
    if when {
        c.set(c.get() + 1);
    }
}

/// The table `name` of `rom`, if present.
fn table<'a>(rom: &'a Rom, name: &str) -> Option<&'a Table> {
    rom.tables.get(&name.to_owned()).and_then(Nested::get)
}

/// The child's overrides win in `merged`, for every policy.
fn check_wins(merged: &Rom, ov: &Overrides) -> Result<(), TestCaseError> {
    prop_assert_eq!(&merged.xmlid, &ov.xmlid);
    let t = table(merged, &ov.fields_of);
    prop_assert!(t.is_some(), "a patched table is present");
    if let Some(t) = t {
        prop_assert_eq!(t.storage, Field::Set(ov.storage), "MF-06");
        prop_assert_eq!(t.endian, ov.endian);
        prop_assert_eq!(t.address, ov.address);
    }
    let scaling = table(merged, &ov.scaling_of).map(|t| &t.scaling);
    match &ov.scaling {
        Nested::Patch(p) => {
            let got = scaling.and_then(Nested::get);
            prop_assert!(got.is_some(), "a patched scaling is present");
            if let Some(got) = got {
                for (mine, theirs) in [
                    (&p.units, &got.units),
                    (&p.expr, &got.expr),
                    (&p.format, &got.format),
                ] {
                    if !mine.is_unset() {
                        prop_assert_eq!(theirs, mine);
                    }
                }
            }
        }
        state => prop_assert_eq!(scaling, Some(state)),
    }
    prop_assert_eq!(merged.tables.get(&ov.item), Some(&ov.item_state));
    prop_assert_eq!(&merged.notes, &Appended::replace(ov.notes.clone()));
    Ok(())
}

fn counts_overrides(inherited: &Rom, ov: &Overrides, seen: &Seen) {
    let before = table(inherited, &ov.fields_of);
    if let Some(b) = before.and_then(|t| t.storage.get()) {
        bump(&seen.to_signed, !b.is_signed() && ov.storage.is_signed());
        bump(&seen.to_unsigned, b.is_signed() && !ov.storage.is_signed());
    }
    let xmlid_changed = inherited.xmlid.get().is_some() && inherited.xmlid != ov.xmlid;
    bump(&seen.xmlid_set, xmlid_changed && !ov.xmlid.is_omitted());
    bump(&seen.xmlid_omit, xmlid_changed && ov.xmlid.is_omitted());
    if let Some(b) = before {
        let endian = b.endian.get().is_some() && b.endian != ov.endian;
        bump(&seen.endian_set, endian && !ov.endian.is_omitted());
        bump(&seen.endian_omit, endian && ov.endian.is_omitted());
        let address = b.address.get().is_some() && b.address != ov.address;
        bump(&seen.address_set, address && !ov.address.is_omitted());
        bump(&seen.address_omit, address && ov.address.is_omitted());
    }
    if let Some(b) = table(inherited, &ov.scaling_of).and_then(|t| t.scaling.get()) {
        let changed = match &ov.scaling {
            Nested::Patch(p) => b.merged(p) != *b,
            Nested::Replace(p) => p != b,
            Nested::Unset | Nested::Omit => true,
        };
        let c = match ov.scaling {
            Nested::Patch(_) => &seen.scaling_patch,
            Nested::Replace(_) => &seen.scaling_replace,
            Nested::Unset | Nested::Omit => &seen.scaling_omit,
        };
        bump(c, changed);
    }
    if let Some(b) = table(inherited, &ov.item) {
        let c = match ov.item_state {
            Nested::Omit => &seen.item_omit,
            _ => &seen.item_replace,
        };
        bump(c, ov.item_state.get() != Some(b));
    }
    bump(
        &seen.notes_replace,
        !inherited.notes.items().is_empty() && inherited.notes.items() != &ov.notes[..],
    );
}

/// MF-06 and every other policy: a child (the most derived document of a
/// chain, the multi-base top of a diamond, the last of a DAG, global or in
/// local scopes) that overrides a field wins over everything it inherits,
/// in its own lineage and in an empty document on top of it, and in the
/// finished value. The child's patch of a table sets the storage type and
/// leaves its other fields random, so the signedness wins whatever the
/// base's storage type and the other fields hold; legacy kept the base's
/// sign whenever the base had set a storage type
/// (`xml/TableScaleUnmarshaller.java:153`).
#[test]
fn mf06_a_child_override_wins_for_every_policy() {
    let seen = Seen::default();
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: 512,
            ..Config::default()
        },
        proptest::test_runner::TestRng::deterministic_rng(
            proptest::test_runner::RngAlgorithm::ChaCha,
        ),
    );
    let result = runner.run(&(families(roms()), overrides()), |((kind, family), ov)| {
        let top = family.len() - 1;
        let (docs, heir) = with_heir(&family, top, ov.child());
        let reg = registry_of(docs)?;
        let mut resolver = reg.resolver();
        let child = lineage_of(&mut resolver, family.name(top))?;
        let own = child.stack().layers().last().map(|(_, d)| &**d);
        prop_assert_eq!(own, Some(&ov.child()), "the child is the top layer");
        check_wins(child.merged(), &ov)?;
        let above = lineage_of(&mut resolver, &heir)?;
        check_wins(above.merged(), &ov)?;
        if let Ok(done) = resolver.resolve(&heir) {
            let def = done.value();
            let t = def.tables.iter().find(|(n, _)| *n == ov.fields_of);
            prop_assert_eq!(t.map(|(_, t)| t.storage), Some(ov.storage), "MF-06");
            if let Field::Set(id) = &ov.xmlid {
                prop_assert_eq!(&def.xmlid, id);
            }
            prop_assert_eq!(&def.notes, &ov.notes);
        }
        // MF-06 alone, in the finished value: the child's own document plus
        // a patch of one table's storage type.
        let mut only_sign = Rom::clone(family.docs()[top].doc());
        let sign = Table {
            storage: Field::Set(ov.storage),
            ..Table::default()
        };
        only_sign.tables.patch(ov.fields_of.clone(), sign);
        let (docs, heir) = with_heir(&family, top, only_sign);
        let reg = registry_of(docs)?;
        let mut resolver = reg.resolver();
        if let Ok(done) = resolver.resolve(&heir) {
            let t = done.value().tables.iter().find(|(n, _)| *n == ov.fields_of);
            prop_assert_eq!(t.map(|(_, t)| t.storage), Some(ov.storage), "MF-06");
            let below = inherited(&*lineage_of(&mut resolver, family.name(top))?);
            let before = table(&below, &ov.fields_of).and_then(|t| t.storage.get().copied());
            bump(
                &seen.completed,
                before.is_some_and(|b| b.is_signed() != ov.storage.is_signed()),
            );
        }
        bump(&seen.chain, kind == Kind::Chain && family.len() > 1);
        bump(&seen.diamond, kind == Kind::Diamond);
        counts_overrides(&inherited(&child), &ov, &seen);
        Ok(())
    });
    if let Err(e) = result {
        panic!("{e}");
    }
    let Seen {
        chain,
        diamond,
        to_signed,
        to_unsigned,
        xmlid_set,
        xmlid_omit,
        endian_set,
        endian_omit,
        address_set,
        address_omit,
        scaling_patch,
        scaling_replace,
        scaling_omit,
        item_replace,
        item_omit,
        notes_replace,
        completed,
    } = &seen;
    for (what, c) in [
        ("chain", chain),
        ("diamond", diamond),
        ("unsigned -> signed (MF-06)", to_signed),
        ("signed -> unsigned (MF-06)", to_unsigned),
        ("xmlid set", xmlid_set),
        ("xmlid omit", xmlid_omit),
        ("endian set", endian_set),
        ("endian omit", endian_omit),
        ("address set", address_set),
        ("address omit", address_omit),
        ("scaling patch", scaling_patch),
        ("scaling replace", scaling_replace),
        ("scaling omit", scaling_omit),
        ("item replace", item_replace),
        ("item omit", item_omit),
        ("notes replace", notes_replace),
        ("completed", completed),
    ] {
        assert!(c.get() > 0, "never exercised: {what} ({seen:?})");
    }
}

// ---------------------------------------------------------------------------
// Override wins: scalar fields of every type, strongest setter in a lineage
// ---------------------------------------------------------------------------

/// A settings-like document of replace fields of the scalar types the
/// sample ROM lacks (bool, signed integer, float), plus an enum and a
/// string.
#[derive(Debug, Clone, Default, PartialEq)]
struct Knobs {
    enabled: Field<bool>,
    offset: Field<i64>,
    gain: Field<f64>,
    order: Field<Endian>,
    label: Field<String>,
}
rr_core::merge_fields!(Knobs {
    enabled,
    offset,
    gain,
    order,
    label
});

fn knobs() -> BoxedStrategy<Knobs> {
    (
        field_with(any::<bool>(), FieldWeights::MIXED),
        field_with(-3..=3_i64, FieldWeights::MIXED),
        field_with(
            prop_oneof![Just(-0.5), Just(0.0), Just(1.5), Just(1e9)],
            FieldWeights::MIXED,
        ),
        field_with(sample::endian(), FieldWeights::MIXED),
        field_with(
            proptest::sample::select(vec!["a", "b", "c"]).prop_map(str::to_owned),
            FieldWeights::MIXED,
        ),
    )
        .prop_map(|(enabled, offset, gain, order, label)| Knobs {
            enabled,
            offset,
            gain,
            order,
            label,
        })
        .boxed()
}

/// Per field: (the strongest layer setting it was not the top document
/// and overrode another value, the top document overrode another value).
type KnobSeen = [(Cell<u32>, Cell<u32>); 5];

/// Field `f` of the merged lineage is the state of the strongest layer that
/// mentions it, or unset if none does; counts non-vacuous overrides.
fn strongest_wins<T: Clone + PartialEq + core::fmt::Debug>(
    lineage: &Lineage<Knobs>,
    f: impl Fn(&Knobs) -> &Field<T>,
    seen: &(Cell<u32>, Cell<u32>),
) -> Result<(), TestCaseError> {
    let mentions: Vec<(usize, &Field<T>)> = lineage
        .stack()
        .layers()
        .enumerate()
        .rev()
        .map(|(i, (_, d))| (i, f(d)))
        .filter(|(_, v)| !v.is_unset())
        .collect();
    let expect = mentions.first().map_or(Field::Unset, |(_, v)| (*v).clone());
    prop_assert_eq!(f(lineage.merged()), &expect);
    if let [(winner, w), rest @ ..] = &mentions[..]
        && rest.iter().any(|(_, v)| v != w)
    {
        let top = *winner + 1 == lineage.stack().len();
        bump(if top { &seen.1 } else { &seen.0 }, true);
    }
    Ok(())
}

/// Override wins for replace fields of every scalar type: through chains,
/// DAGs and diamonds (global or scoped), every field of every resolved
/// document is the state of the strongest layer in its lineage that sets
/// or omits it, whatever the others hold; an empty document on top changes
/// nothing.
#[test]
fn the_strongest_setter_wins_for_every_scalar_type() {
    let seen: KnobSeen = Default::default();
    let mut runner = TestRunner::new_with_rng(
        Config::default(),
        proptest::test_runner::TestRng::deterministic_rng(
            proptest::test_runner::RngAlgorithm::ChaCha,
        ),
    );
    let result = runner.run(&families(knobs()), |(_, family)| {
        let reg = family
            .registry()
            .map_err(|e| TestCaseError::fail(e.to_string()))?;
        let mut resolver = reg.resolver();
        for d in family.docs() {
            let l = lineage_of(&mut resolver, d.name())?;
            strongest_wins(&l, |k| &k.enabled, &seen[0])?;
            strongest_wins(&l, |k| &k.offset, &seen[1])?;
            strongest_wins(&l, |k| &k.gain, &seen[2])?;
            strongest_wins(&l, |k| &k.order, &seen[3])?;
            strongest_wins(&l, |k| &k.label, &seen[4])?;
        }
        let top = family.len() - 1;
        let top_doc = Knobs::clone(family.docs()[top].doc());
        let (docs, heir) = with_heir(&family, top, top_doc);
        let reg = registry_of(docs)?;
        let mut r = reg.resolver();
        let (above, below) = (
            lineage_of(&mut r, &heir)?,
            lineage_of(&mut r, family.name(top))?,
        );
        prop_assert_eq!(above.merged(), below.merged());
        Ok(())
    });
    if let Err(e) = result {
        panic!("{e}");
    }
    for (field, (below, top)) in ["enabled", "offset", "gain", "order", "label"]
        .iter()
        .zip(&seen)
    {
        assert!(below.get() > 0, "{field}: never won from below the top");
        assert!(top.get() > 0, "{field}: never overridden by the top");
    }
}
