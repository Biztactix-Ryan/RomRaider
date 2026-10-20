//! Per-field provenance and `explain` over the sample ROM definition
//! (US-RR-22-11).
//!
//! `explain` re-walks a stack for one field path and reports the layer
//! whose change stands and every change it overrode. Here, for random
//! stacks and resolved families of the sample [`Rom`]: the explained value
//! is always the merged one at that path, the winner is always the
//! strongest layer whose own document reaches the field, and an override of
//! signedness in a child names the child, with the base's value as
//! overridden (MF-06; legacy inherited signedness only while the base's
//! storage type was unset, `xml/TableScaleUnmarshaller.java:153`).
//!
//! Legacy references are to RomRaider's Java sources under
//! `legacy/src/main/java/com/romraider` (read, never run; D-024).

use proptest::prelude::*;
use rr_core::encoding::Storage;
use rr_core::layer::{
    Appended, Change, Field, FieldPath, Keyed, Layer, Layered, Merge, Nested, ResolveError, Step,
};
use rr_testkit::strategies::layered::sample::{self, Axis, Rom, Scaling, TABLE_NAMES, Table};
use rr_testkit::strategies::layered::{Family, chain, dag, diamond, layer_named, layer_stack};

fn tables() -> FieldPath<Rom, Keyed<String, Table>> {
    FieldPath::root().field("tables", |r: &Rom| &r.tables)
}

fn table(name: &str) -> FieldPath<Rom, Table> {
    tables().item(name.to_owned())
}

fn storage(name: &str) -> FieldPath<Rom, Field<Storage>> {
    table(name).field("storage", |t: &Table| &t.storage)
}

fn address(name: &str) -> FieldPath<Rom, Field<u32>> {
    table(name).field("address", |t: &Table| &t.address)
}

fn units(name: &str) -> FieldPath<Rom, Field<String>> {
    table(name)
        .field("scaling", |t: &Table| &t.scaling)
        .nested()
        .field("units", |s: &Scaling| &s.units)
}

fn axes(name: &str) -> FieldPath<Rom, Keyed<String, Axis>> {
    table(name).field("axes", |t: &Table| &t.axes)
}

fn axis_storage(name: &str, axis: &str) -> FieldPath<Rom, Field<Storage>> {
    axes(name)
        .item(axis.to_owned())
        .field("storage", |a: &Axis| &a.storage)
}

fn notes() -> FieldPath<Rom, Appended<String>> {
    FieldPath::root().field("notes", |r: &Rom| &r.notes)
}

/// `true` if `rom`'s own document reaches `field` of table `name`: it
/// replaces or omits the table, or patches it with the field not unset.
fn touches<T>(rom: &Rom, name: &str, field: impl Fn(&Table) -> &Field<T>) -> bool {
    match rom.tables.get(&name.to_owned()) {
        None | Some(Nested::Unset) => false,
        Some(Nested::Omit | Nested::Replace(_)) => true,
        Some(Nested::Patch(t)) => !field(t).is_unset(),
    }
}

/// Checks a field explanation against the merge: the value is the merged
/// one, and the winner and history are the layers whose documents reach the
/// field (`reaches`), strongest first.
fn check_field<T: Clone + PartialEq + core::fmt::Debug + 'static>(
    stack: &Layered<Rom>,
    path: &FieldPath<Rom, Field<T>>,
    reaches: impl Fn(&Rom) -> bool,
) -> Result<(), TestCaseError> {
    let merged = stack.merged();
    let e = stack.explain(path);
    prop_assert_eq!(e.value(), path.get(&merged).and_then(Field::get));
    let expected: Vec<&Layer> = stack
        .layers()
        .filter(|(_, doc)| reaches(doc))
        .map(|(layer, _)| layer)
        .rev()
        .collect();
    let history: Vec<&Layer> = e.history().map(Step::layer).collect();
    prop_assert_eq!(&history, &expected);
    prop_assert_eq!(e.winner().map(Step::layer), expected.first().copied());
    prop_assert_eq!(e.overridden().count(), expected.len().saturating_sub(1));
    // Every report names its path and, when a layer set it, its winner.
    let report = e.to_string();
    prop_assert!(report.starts_with(path.as_str()), "{}", report);
    if let Some(w) = e.winner() {
        prop_assert!(report.contains(&w.provenance().to_string()), "{}", report);
    }
    Ok(())
}

proptest! {
    /// For every table, at every depth and for every policy, the explained
    /// value is the merged value, and the winner is the strongest layer that
    /// set it.
    #[test]
    fn explain_agrees_with_the_merge(stack in layer_stack(sample::rom(), 0..=5)) {
        let merged = stack.merged();
        for name in TABLE_NAMES {
            check_field(&stack, &storage(name), |r| touches(r, name, |t| &t.storage))?;
            check_field(&stack, &address(name), |r| touches(r, name, |t| &t.address))?;
            let u = units(name);
            prop_assert_eq!(stack.explain(&u).value(), u.get(&merged).and_then(Field::get));
            let a = axes(name);
            let e = stack.explain(&a);
            let keys: Vec<_> = a.get(&merged).into_iter().flat_map(Keyed::entries).map(|(k, _)| k).collect();
            prop_assert_eq!(e.keys(), keys);
            for axis in sample::AXIS_NAMES {
                let item = e.item(&axis.to_owned());
                prop_assert_eq!(&item, &stack.explain(&a.clone().key(axis.to_owned())));
                let present = a.get(&merged).and_then(|l| l.get(&axis.to_owned())).is_some_and(|n| n.get().is_some());
                prop_assert_eq!(item.is_present(), present);
                let s = axis_storage(name, axis);
                prop_assert_eq!(stack.explain(&s).value(), s.get(&merged).and_then(Field::get));
            }
            let t = tables().key(name.to_owned());
            let present = merged.tables.present().any(|(k, _)| k == name);
            prop_assert_eq!(stack.explain(&t).is_present(), present);
        }
        let n = notes();
        prop_assert_eq!(stack.explain(&n).items(), merged.notes.items().iter().collect::<Vec<_>>());
        let id = FieldPath::root().field("xmlid", |r: &Rom| &r.xmlid);
        check_field(&stack, &id, |r| !r.xmlid.is_unset())?;
    }

    /// MF-06: a child that sets a table's storage type wins over the base's,
    /// whatever the base holds and whichever way the signedness goes, and
    /// the base's value is listed as overridden with the base's provenance.
    #[test]
    fn mf06_child_signedness_wins_and_base_is_overridden(
        base in sample::complete_table(),
        child in sample::storage(),
        name in proptest::sample::select(TABLE_NAMES.to_vec()),
    ) {
        let mut base_rom = Rom::default();
        base_rom.tables.patch(name.to_owned(), base.clone());
        let mut child_rom = Rom::default();
        child_rom.tables.patch(name.to_owned(), Table { storage: Field::Set(child), ..Table::default() });
        let base_layer = layer_named("16BITBASE", rr_core::layer::LayerKind::File, rr_core::diag::Span::line_col(3, 1));
        let child_layer = layer_named("A2WC522N", rr_core::layer::LayerKind::File, rr_core::diag::Span::line_col(2, 1));
        let mut stack = Layered::new();
        stack.push(base_layer.clone(), base_rom);
        stack.push(child_layer.clone(), child_rom);
        let e = stack.explain(&storage(name));
        prop_assert_eq!(e.value(), Some(&child));
        prop_assert_eq!(e.winner().map(Step::layer), Some(&child_layer));
        let overridden: Vec<_> = e.overridden().map(|s| (s.layer(), s.change().clone())).collect();
        prop_assert_eq!(overridden, vec![(&base_layer, Change::Apply(&base.storage))]);
        let report = e.to_string();
        let expected = format!(
            "tables[{name:?}].storage = {child:?}\n  \
             set to {child:?} at defs/A2WC522N.xml:2:1 [layer A2WC522N]\n  \
             overrides:\n    \
             set to {:?} at defs/16BITBASE.xml:3:1 [layer 16BITBASE]",
            base.storage.get().copied().unwrap_or(Storage::U8),
        );
        prop_assert_eq!(report, expected);
    }

    /// A resolved document explains its own lineage: through chains, DAGs
    /// and diamonds, the explained value is the completed one.
    #[test]
    fn resolved_explains_its_lineage(
        family in prop_oneof![chain(sample::complete_rom(), 1..=6), dag(sample::complete_rom(), 1..=6)],
    ) {
        check_family(&family)?;
    }

    #[test]
    fn diamonds_explain_their_lineage(family in diamond(sample::complete_rom())) {
        check_family(&family.family)?;
    }
}

fn check_family(family: &Family<Rom>) -> Result<(), TestCaseError> {
    let Ok(registry) = family.registry() else {
        return Err(TestCaseError::fail("valid family has unique names"));
    };
    let mut resolver = registry.resolver();
    let resolved = match resolver.resolve(family.top()) {
        Ok(r) => r,
        Err(ResolveError::Incomplete { .. }) => return Ok(()),
        Err(e) => return Err(TestCaseError::fail(e.to_string())),
    };
    let def = resolved.value();
    for (name, t) in &def.tables {
        let e = resolved.explain(&storage(name));
        prop_assert_eq!(e.value(), Some(&t.storage));
        prop_assert_eq!(&e, &resolved.lineage().stack().explain(&storage(name)));
        let winner = e.winner().map(|s| s.layer().id().clone());
        let strongest = resolved
            .lineage()
            .stack()
            .layers()
            .rev()
            .find(|(_, doc)| touches(doc, name, |t| &t.storage))
            .map(|(l, _)| l.id().clone());
        prop_assert_eq!(winner, strongest);
    }
    let e = resolved.explain(&notes());
    prop_assert_eq!(e.items(), def.notes.iter().collect::<Vec<_>>());
    Ok(())
}

/// Explaining is pure: the same stack and path always give the same
/// explanation, and explaining does not change the merge.
#[test]
fn explain_is_deterministic() {
    let mut base = Rom::default();
    base.tables.patch(
        "Boost Target".to_owned(),
        Table {
            storage: Field::Set(Storage::U16),
            ..Table::default()
        },
    );
    let mut stack = Layered::new();
    stack.push(
        layer_named(
            "16BITBASE",
            rr_core::layer::LayerKind::File,
            rr_core::diag::Span::line_col(1, 1),
        ),
        base,
    );
    let before = stack.merged();
    let path = storage("Boost Target");
    assert_eq!(stack.explain(&path), stack.explain(&path.clone()));
    assert_eq!(stack.merged(), before);
    let mut again = before.clone();
    again.merge_from(&Rom::default());
    assert_eq!(again, before);
}
