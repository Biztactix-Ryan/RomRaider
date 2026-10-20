//! The Layered + Resolver strategies (`rr_testkit::strategies::layered`),
//! US-RR-22-12.
//!
//! The shapes keep their promises: valid families always resolve, to the
//! reference C3 lineage; cyclic ones always report their cycle, with the
//! full path; inconsistent and unknown-base ones report the right document;
//! diamonds really share an ancestor; omit-in-the-middle omits are never at
//! either end; scoped families keep every base in an enclosing scope. And a
//! failing property shrinks to a small case.

use std::collections::BTreeSet;

use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::{Config, RngAlgorithm, TestError, TestRng, TestRunner};
use rr_core::encoding::Storage;
use rr_core::layer::{
    Complete, Field, Layered, Merge, Name, Nested, Registry, ResolveError, Scope,
};
use rr_testkit::strategies::layered::sample::{
    self, Axis, AxisDef, Rom, RomDef, Scaling, ScalingDef, Table, TableDef,
};
use rr_testkit::strategies::layered::{
    Family, FieldWeights, chain, cyclic_registry, dag, diamond, field, field_with,
    inconsistent_bases, layer, layer_stack, layers, omit_in_middle, scoped, shadowing,
    unknown_base,
};

/// `n` values of `strategy` from a deterministic runner.
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn sample<S: Strategy>(strategy: &S, n: usize) -> Vec<S::Value> {
    let mut runner = TestRunner::deterministic();
    (0..n)
        .map(|_| strategy.new_tree(&mut runner).unwrap().current())
        .collect()
}

#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn registry<P: Clone>(family: &Family<P>) -> Registry<P> {
    family.registry().unwrap()
}

/// The lineage of `name` as family indices.
fn lineage_indices<P: Merge + Clone>(
    family: &Family<P>,
    reg: &Registry<P>,
    name: &Name,
) -> Result<Vec<usize>, ResolveError> {
    let l = reg.resolver().lineage(name)?;
    Ok(l.names()
        .iter()
        .filter_map(|n| family.index_of(n))
        .collect())
}

/// Every base of every document that names one in the family is in the
/// same scope as the document or an enclosing one (a prefix of its path).
fn bases_enclose<P>(family: &Family<P>) -> Result<(), TestCaseError> {
    for (i, d) in family.docs().iter().enumerate() {
        let own: Vec<&str> = d.name().scope().segments().collect();
        for b in family.bases_of(i).into_iter().flatten() {
            let base: Vec<&str> = family.name(b).scope().segments().collect();
            prop_assert!(
                own.starts_with(&base),
                "{} names {}",
                d.name(),
                family.name(b)
            );
        }
    }
    Ok(())
}

/// A valid family: every document resolves, to the reference C3 lineage,
/// whatever order the registry was filled in.
fn resolves_to_reference<P: Merge + Clone + PartialEq + core::fmt::Debug>(
    family: &Family<P>,
) -> Result<(), TestCaseError> {
    let reg = registry(family);
    let reversed: Vec<usize> = (0..family.len()).rev().collect();
    let backward = family.registry_in_order(&reversed);
    prop_assert_eq!(backward.as_ref(), Ok(&reg));
    for i in 0..family.len() {
        let expect = family.reference_lineage(i);
        prop_assert!(
            expect.is_some(),
            "{} has no reference lineage",
            family.name(i)
        );
        let got = lineage_indices(family, &reg, family.name(i));
        prop_assert_eq!(
            got.as_ref().ok(),
            expect.as_ref(),
            "{}: {:?}",
            family.name(i),
            got
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Field states, layers, stacks
// ---------------------------------------------------------------------------

#[test]
fn field_weights_choose_the_states() {
    let states = |w| {
        sample(&field_with(0..3_u8, w), 300)
            .into_iter()
            .map(|f| match f {
                Field::Unset => 0,
                Field::Set(_) => 1,
                Field::Omit => 2,
            })
            .collect::<BTreeSet<u8>>()
    };
    assert_eq!(states(FieldWeights::MIXED), BTreeSet::from([0, 1, 2]));
    assert_eq!(states(FieldWeights::SET), BTreeSet::from([1]));
    assert_eq!(states(FieldWeights::OVERRIDE), BTreeSet::from([1, 2]));
    let none = FieldWeights {
        unset: 0,
        set: 0,
        omit: 0,
    };
    assert_eq!(states(none), BTreeSet::from([0]));
    assert_eq!(FieldWeights::default(), FieldWeights::MIXED);
    assert_eq!(states(FieldWeights::MIXED), {
        let f = field(0..3_u8);
        sample(&f, 300)
            .into_iter()
            .map(|f| u8::from(!f.is_unset()) + u8::from(f.is_omitted()))
            .collect::<BTreeSet<u8>>()
    });
}

#[test]
fn layers_reach_every_kind_and_span() {
    let kinds: BTreeSet<_> = sample(&layer(), 200).iter().map(|l| l.kind()).collect();
    assert_eq!(kinds.len(), 4);
}

proptest! {
    #[test]
    fn layer_ids_are_unique_and_files_distinct(ls in layers(0..=8)) {
        prop_assert!(ls.len() <= 8);
        let ids: BTreeSet<&str> = ls.iter().map(|l| l.id().as_str()).collect();
        let files: BTreeSet<&str> = ls.iter().map(|l| l.source()).collect();
        prop_assert_eq!(ids.len(), ls.len());
        prop_assert_eq!(files.len(), ls.len());
    }

    #[test]
    fn layer_stacks_have_the_asked_length_and_unique_ids(
        stack in layer_stack(sample::table(), 1..=5)
    ) {
        prop_assert!((1..=5).contains(&stack.len()));
        let ids: BTreeSet<&str> = stack.layers().map(|(l, _)| l.id().as_str()).collect();
        prop_assert_eq!(ids.len(), stack.len());
        let mut fold = Table::default();
        for (_, d) in stack.layers() {
            fold.merge_from(d);
        }
        prop_assert_eq!(stack.merged(), fold);
    }

    /// The omit is never the first or last item, the base holds every key
    /// once, in order, and the overlay removes nothing else.
    #[test]
    fn omit_in_middle_omits_neither_end(
        shape in omit_in_middle(proptest::sample::select(sample::TABLE_NAMES.to_vec()), sample::table(), 3..=5)
    ) {
        let n = shape.keys.len();
        prop_assert!((3..=5).contains(&n));
        prop_assert!(0 < shape.omitted && shape.omitted < n - 1);
        let base_keys: Vec<_> = shape.base.present().map(|(k, _)| *k).collect();
        prop_assert_eq!(&base_keys, &shape.keys);
        let omitted = shape.keys[shape.omitted];
        for (k, state) in shape.overlay.entries() {
            if *k == omitted {
                prop_assert!(state.is_omitted());
            } else {
                prop_assert!(matches!(state, Nested::Patch(_)), "{k}: {state:?}");
            }
        }
        prop_assert!(shape.overlay.get(&omitted).is_some_and(Nested::is_omitted));
    }
}

// ---------------------------------------------------------------------------
// Valid families
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn chains_resolve_in_order(family in chain(sample::rom(), 1..=6)) {
        let n = family.len();
        prop_assert!((1..=6).contains(&n));
        let reg = registry(&family);
        let top = lineage_indices(&family, &reg, family.top());
        prop_assert_eq!(top, Ok((0..n).collect::<Vec<_>>()));
        resolves_to_reference(&family)?;
    }

    #[test]
    fn dags_always_resolve(family in dag(sample::rom(), 1..=8)) {
        resolves_to_reference(&family)?;
    }

    /// The apex is an ancestor of every side, so a real shared ancestor, and
    /// the top's lineage holds it once, below every side.
    #[test]
    fn diamonds_share_an_ancestor(shape in diamond(sample::rom())) {
        let f = &shape.family;
        prop_assert!(shape.sides.len() >= 2);
        let top_bases: Vec<usize> = f.bases_of(shape.top).into_iter().flatten().collect();
        prop_assert_eq!(&top_bases, &shape.sides);
        for &s in &shape.sides {
            prop_assert!(f.ancestors(s).contains(&shape.apex));
        }
        let reg = registry(f);
        let top = lineage_indices(f, &reg, f.name(shape.top));
        prop_assert!(top.is_ok(), "{top:?}");
        let top = top.unwrap_or_default();
        let at = |i: usize| top.iter().position(|&x| x == i);
        prop_assert_eq!(top.iter().filter(|&&x| x == shape.apex).count(), 1);
        for &s in &shape.sides {
            prop_assert!(at(shape.apex) < at(s));
        }
        resolves_to_reference(f)?;
    }

    /// MF-16 shape: the global and local documents share an id but not a
    /// name; the outer document inherits the global one only, the inner
    /// document the local one, and the global one below it only when the
    /// local one extends it (names its own id as its base).
    #[test]
    fn shadowing_keeps_the_two_names_apart(shape in shadowing(sample::rom())) {
        let f = &shape.family;
        let (g, l) = (f.name(shape.global), f.name(shape.local));
        prop_assert_eq!(g.id(), l.id());
        prop_assert!(g.scope().is_global());
        prop_assert!(!l.scope().is_global());
        prop_assert_eq!(f.name(shape.inner).scope(), l.scope());
        let reg = registry(f);
        let outer = lineage_indices(f, &reg, f.name(shape.outer));
        let inner = lineage_indices(f, &reg, f.name(shape.inner));
        prop_assert_eq!(outer, Ok(vec![shape.global, shape.outer]));
        let expect = if shape.extends {
            vec![shape.global, shape.local, shape.inner]
        } else {
            vec![shape.local, shape.inner]
        };
        prop_assert_eq!(inner, Ok(expect));
        prop_assert_eq!(f.docs()[shape.local].bases().contains(l.id()), shape.extends);
        resolves_to_reference(f)?;
        bases_enclose(f)?;
    }

    /// Scoping moves documents but keeps every promise of the shape, and
    /// every base in an enclosing scope.
    #[test]
    fn scoped_valid_families_still_resolve(
        family in scoped(dag(sample::rom(), 1..=8)),
        shape in scoped(diamond(sample::rom())),
        shadow in scoped(shadowing(sample::rom())),
    ) {
        for f in [&family, &shape.family, &shadow.family] {
            bases_enclose(f)?;
            resolves_to_reference(f)?;
        }
        let f = &shadow.family;
        prop_assert_eq!(f.name(shadow.global).id(), f.name(shadow.local).id());
        prop_assert_ne!(f.name(shadow.global), f.name(shadow.local));
    }

    /// Valid documents from `complete_*` complete on their own; the sample
    /// documents' merge is the stack's fold.
    #[test]
    fn complete_documents_complete(
        rom in sample::complete_rom(),
        table in sample::complete_table(),
        axis in sample::complete_axis(),
        scaling in sample::complete_scaling(),
    ) {
        prop_assert!(rom.complete().is_ok(), "{:?}", rom.complete());
        prop_assert!(table.complete().is_ok(), "{:?}", table.complete());
        prop_assert!(axis.complete().is_ok());
        prop_assert!(scaling.complete().is_ok());
        prop_assert_eq!(Rom::default().merged(&rom), rom.clone());
        let mut stack = Layered::new();
        stack.push(rr_core::layer::Layer::new("a".into(), rr_core::layer::LayerKind::File, "a.xml", rr_core::diag::Span::line_col(1, 1)), rom.clone());
        prop_assert_eq!(stack.merged(), rom);
    }
}

// ---------------------------------------------------------------------------
// Invalid families
// ---------------------------------------------------------------------------

proptest! {
    /// MF-17 shape: resolving the entry, or any document on the cycle, is a
    /// `Cycle` error whose path is the whole cycle (`k + 1` names); the
    /// documents off the cycle that do not reach it still resolve.
    #[test]
    fn cyclic_registries_always_report_the_cycle(
        shape in prop_oneof![cyclic_registry(sample::rom(), 2..=5), scoped(cyclic_registry(sample::rom(), 2..=5))]
    ) {
        let f = &shape.family;
        let k = shape.cycle.len();
        prop_assert!((2..=5).contains(&k));
        let files: BTreeSet<&str> = shape.cycle.iter().map(|&c| f.docs()[c].layer().source()).collect();
        prop_assert_eq!(files.len(), k, "one file per cycle document");
        let reg = registry(f);
        match reg.resolver().lineage(f.name(shape.entry)) {
            Err(ResolveError::Cycle { path, layers, layer }) => {
                prop_assert_eq!(path.len(), k + 1);
                prop_assert_eq!(&path, &shape.expected_path());
                prop_assert_eq!(&layer, f.docs()[shape.closing()].layer());
                let on_cycle: Vec<_> = shape.cycle.iter().map(|&c| f.docs()[c].layer().clone()).collect();
                prop_assert_eq!(&layers, &on_cycle);
            }
            other => prop_assert!(false, "expected a cycle, got {other:?}"),
        }
        for &c in &shape.cycle {
            let got = reg.resolver().lineage(f.name(c));
            let is_k_cycle = matches!(&got, Err(ResolveError::Cycle { path, .. }) if path.len() == k + 1);
            prop_assert!(is_k_cycle, "{}: {got:?}", f.name(c));
            prop_assert!(f.reference_lineage(c).is_none());
        }
        for i in 0..f.len() {
            let reaches_cycle = i == shape.entry
                || shape.cycle.contains(&i)
                || f.ancestors(i).iter().any(|a| shape.cycle.contains(a));
            if !reaches_cycle {
                prop_assert!(reg.resolver().lineage(f.name(i)).is_ok());
            }
        }
        bases_enclose(f)?;
    }

    #[test]
    fn inconsistent_bases_are_inconsistent(
        shape in prop_oneof![inconsistent_bases(sample::rom()), scoped(inconsistent_bases(sample::rom()))]
    ) {
        let f = &shape.family;
        let reg = registry(f);
        match reg.resolver().lineage(f.name(shape.entry)) {
            Err(ResolveError::Inconsistent { document, .. }) => {
                prop_assert_eq!(&document, f.name(shape.document));
            }
            other => prop_assert!(false, "expected inconsistent, got {other:?}"),
        }
        prop_assert!(f.reference_lineage(shape.document).is_none());
        // Everything below the conflict resolves.
        for i in f.ancestors(shape.document) {
            prop_assert!(reg.resolver().lineage(f.name(i)).is_ok());
            prop_assert!(f.reference_lineage(i).is_some());
        }
    }

    #[test]
    fn unknown_bases_are_unknown(
        shape in prop_oneof![unknown_base(sample::rom()), scoped(unknown_base(sample::rom()))]
    ) {
        let f = &shape.family;
        let reg = registry(f);
        let from = f.name(shape.document).scope();
        prop_assert!(reg.lookup(from, &shape.base).is_none());
        let at = f.docs()[shape.document].bases().iter().position(|b| *b == shape.base);
        prop_assert!(at.is_some());
        let hidden = at.map(|k| f.names(&f.hidden_from(shape.document, k))).unwrap_or_default();
        match reg.resolver().lineage(f.name(shape.entry)) {
            Err(ResolveError::UnknownBase { document, base, hidden: got, .. }) => {
                prop_assert_eq!(&document, f.name(shape.document));
                prop_assert_eq!(&base, &shape.base);
                prop_assert_eq!(&*got, &hidden[..]);
            }
            other => prop_assert!(false, "expected an unknown base, got {other:?}"),
        }
        match shape.local {
            Some(l) => {
                // MF-16: the base is the id of a local document the referrer
                // cannot see.
                prop_assert_eq!(&shape.base, f.name(l).id());
                prop_assert!(!f.name(l).scope().encloses(from));
                prop_assert_eq!(hidden, vec![f.name(l).clone()]);
                prop_assert!(reg.resolver().lineage(f.name(l)).is_ok());
            }
            None => prop_assert!(hidden.is_empty()),
        }
    }
}

#[test]
fn every_variant_is_generated() {
    let unknown = sample(&unknown_base(sample::rom()), 100);
    assert!(unknown.iter().any(|s| s.local.is_some()));
    assert!(unknown.iter().any(|s| s.local.is_none()));
    let inconsistent: BTreeSet<usize> = sample(&inconsistent_bases(sample::rom()), 100)
        .iter()
        .map(|s| s.family.len() - (s.entry - s.document))
        .collect();
    // Crossed (5 documents), twice (2) and indirect (6).
    assert_eq!(inconsistent, BTreeSet::from([2, 5, 6]));
    let cyclic = sample(&cyclic_registry(sample::rom(), 1..=3), 100);
    // A requested `k` of 1 is clamped to 2: one self-named document is not
    // a cycle.
    assert!(cyclic.iter().all(|s| s.cycle.len() >= 2));
    assert!(cyclic.iter().any(|s| s.cycle.len() == 2));
    assert!(cyclic.iter().any(|s| !s.cycle.contains(&s.entry)));
    let dags = sample(&dag(sample::rom(), 4..=8), 100);
    let several_bases = |f: &Family<Rom>| (0..f.len()).any(|i| f.bases_of(i).len() >= 2);
    assert!(dags.iter().any(several_bases));
    let scoped_dags = sample(&scoped(dag(sample::rom(), 4..=8)), 100);
    let depths: BTreeSet<usize> = scoped_dags
        .iter()
        .flat_map(|f| f.docs().iter().map(|d| d.name().scope().segments().len()))
        .collect();
    assert_eq!(depths, BTreeSet::from([0, 1, 2]));
}

// ---------------------------------------------------------------------------
// The sample document
// ---------------------------------------------------------------------------

/// Missing required fields are reported at their full path, in every
/// present table and axis; omitted ones are not reported.
#[test]
fn sample_rom_reports_missing_fields_by_path() {
    let mut rom = Rom::default();
    let mut fuel = Table {
        storage: Field::Set(Storage::I16),
        endian: Field::Unset,
        address: Field::Set(0x100),
        scaling: Nested::Patch(Scaling::default()),
        ..Table::default()
    };
    fuel.axes.patch(
        "X".to_owned(),
        Axis {
            storage: Field::Set(Storage::U8),
            ..Axis::default()
        },
    );
    rom.tables.patch("Fuel Map".to_owned(), fuel);
    rom.tables.patch("Timing".to_owned(), Table::default());
    rom.tables.omit("Timing".to_owned());
    let missing = rom.complete().err().unwrap_or_default();
    assert_eq!(
        missing.fields(),
        [
            "xmlid",
            "tables[Fuel Map].endian",
            "tables[Fuel Map].scaling.expr",
            "tables[Fuel Map].axes[X].address",
            "tables[Fuel Map].axes[X].len",
        ]
    );
}

#[test]
fn sample_rom_completes_into_its_definition() {
    let scaling = Scaling {
        units: Field::Set("rpm".into()),
        expr: Field::Set("x".into()),
        format: Field::Omit,
    };
    let mut rom = Rom {
        xmlid: Field::Set("A2WC522N".into()),
        ..Rom::default()
    };
    let mut table = Table {
        storage: Field::Set(Storage::U16),
        endian: Field::Set(rr_core::encoding::Endian::Big),
        address: Field::Set(0x200),
        ..Table::default()
    };
    table.axes.patch(
        "Y".into(),
        Axis {
            storage: Field::Set(Storage::I8),
            address: Field::Set(0x300),
            len: Field::Set(4),
            scaling: Nested::Replace(scaling),
        },
    );
    rom.tables.patch("Idle Speed".into(), table);
    let scaling_def = ScalingDef {
        units: Some("rpm".into()),
        expr: "x".into(),
        format: None,
    };
    assert_eq!(
        rom.complete(),
        Ok(RomDef {
            xmlid: "A2WC522N".into(),
            tables: vec![(
                "Idle Speed".into(),
                TableDef {
                    storage: Storage::U16,
                    endian: rr_core::encoding::Endian::Big,
                    address: 0x200,
                    scaling: None,
                    axes: vec![(
                        "Y".into(),
                        AxisDef {
                            storage: Storage::I8,
                            address: 0x300,
                            len: 4,
                            scaling: Some(scaling_def),
                        }
                    )],
                }
            )],
            notes: Vec::new(),
        })
    );
    assert!(Scaling::default().complete().is_err());
    assert!(Axis::default().complete().is_err());
    assert!(Table::default().complete().is_err());
}

// ---------------------------------------------------------------------------
// Shrinking
// ---------------------------------------------------------------------------

/// The minimal failing input proptest finds for `property` over `strategy`,
/// from a fixed seed (with the default shrink budget).
fn minimal_failure<S: Strategy>(
    strategy: &S,
    property: impl Fn(S::Value) -> Result<(), TestCaseError>,
) -> Option<S::Value> {
    let config = Config {
        failure_persistence: None,
        ..Config::default()
    };
    let rng = TestRng::deterministic_rng(RngAlgorithm::ChaCha);
    let mut runner = TestRunner::new_with_rng(config, rng);
    match runner.run(strategy, property) {
        Err(TestError::Fail(_, value)) => Some(value),
        _ => None,
    }
}

/// A deliberately false property ("no chain is longer than 3", "no
/// document anywhere sets a signed storage type") fails, and proptest
/// shrinks the counterexample to the smallest one: these tests pass
/// because the shrunk case is small.
#[test]
fn failures_shrink_to_small_cases() {
    let longest = minimal_failure(&chain(sample::rom(), 1..=12), |f| {
        prop_assert!(f.len() <= 3);
        Ok(())
    });
    let longest = longest.map(|f| {
        let docs: Vec<Rom> = f.docs().iter().map(|d| Rom::clone(d.doc())).collect();
        (f.len(), docs)
    });
    assert_eq!(longest, Some((4, vec![Rom::default(); 4])));

    let signed = |t: &Table| t.storage.get().is_some_and(|s| s.is_signed());
    let stack = minimal_failure(&layer_stack(sample::table(), 1..=6), |stack| {
        prop_assert!(!stack.layers().any(|(_, t)| signed(t)));
        Ok(())
    });
    let stack = stack.map(|s| s.layers().map(|(_, t)| Table::clone(t)).collect::<Vec<_>>());
    // One layer setting only a signed storage. Which signed storage it
    // shrinks to depends on the target's pointer width (I8 on 64-bit, U20
    // on i686), so only signedness is pinned.
    let [table] = stack.as_deref().unwrap_or_default() else {
        panic!("not shrunk to one layer: {stack:?}");
    };
    assert!(signed(table), "{table:?}");
    assert_eq!(
        Table {
            storage: Field::Unset,
            ..table.clone()
        },
        Table::default()
    );

    // A cyclic family shrinks to two documents naming each other.
    let cycle = minimal_failure(&cyclic_registry(sample::rom(), 2..=5), |shape| {
        prop_assert!(shape.cycle.len() > 5);
        Ok(())
    });
    assert_eq!(cycle.map(|s| (s.family.len(), s.cycle.len())), Some((2, 2)));
}

/// Scope chains stay inside their own segments: no scoped document is ever
/// placed inside the fixed local scope of `shadowing` / `unknown_base`.
#[test]
fn scoped_never_enters_the_local_scope() {
    for shape in sample(&scoped(shadowing(sample::rom())), 50) {
        let local = shape.family.name(shape.local).scope().clone();
        for (i, d) in shape.family.docs().iter().enumerate() {
            let in_local = d.name().scope().segments().next()
                == Some(rr_testkit::strategies::layered::family::LOCAL_SEGMENT);
            assert_eq!(
                in_local,
                i == shape.local || i == shape.inner,
                "{}",
                d.name()
            );
            if in_local {
                assert_eq!(d.name().scope(), &local);
            }
        }
    }
    assert!(Scope::global().is_global());
}
