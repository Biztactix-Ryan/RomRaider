//! Unit and property tests for layers and merge semantics (US-RR-22-7).
//!
//! Legacy references are to RomRaider's Java sources under
//! `legacy/src/main/java/com/romraider` (read, never run; D-024).

use std::sync::Arc;

use proptest::prelude::*;

use super::*;
use crate::diag::Span;
use crate::encoding::{Endian, Storage};

// ---- test documents ----

/// A scaling-like leaf document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Scaling {
    units: Field<u8>,
    expr: Field<u8>,
}
crate::merge_fields!(Scaling { units, expr });

/// A table-like document: the shape `xml/TableScaleUnmarshaller.java`
/// unmarshals, with signedness carried by the storage type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Table {
    storage: Field<Storage>,
    endian: Field<Endian>,
    address: Field<u32>,
    scaling: Nested<Scaling>,
}
crate::merge_fields!(Table {
    storage,
    endian,
    address,
    scaling,
});

/// A ROM-definition-like document, with every policy.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Rom {
    id: Field<u8>,
    tables: Keyed<u8, Table>,
    notes: Appended<u8>,
    defaults: Scaling,
}
crate::merge_fields!(Rom {
    id,
    tables,
    notes,
    defaults,
});

fn layer(id: &str) -> Layer {
    Layer::new(
        id.into(),
        LayerKind::File,
        format!("{id}.xml"),
        Span::line_col(1, 1),
    )
}

fn table(storage: Storage, address: u32) -> Table {
    Table {
        storage: Field::Set(storage),
        address: Field::Set(address),
        ..Table::default()
    }
}

fn scaling(units: u8) -> Scaling {
    Scaling {
        units: Field::Set(units),
        expr: Field::Unset,
    }
}

// ---- strategies ----

// Local on purpose: the shared generators in `rr_testkit::strategies::layered`
// (US-RR-22-12) are built on the `rr_core` library crate, which is a different
// crate from this unit-test build of it, so their `Field`, `Keyed`, ... are
// not this module's types. Integration tests (`tests/`) use the shared ones.

fn arb_field<T: Clone + core::fmt::Debug + 'static>(
    value: impl Strategy<Value = T> + 'static,
) -> BoxedStrategy<Field<T>> {
    prop_oneof![
        Just(Field::Unset),
        value.prop_map(Field::Set),
        Just(Field::Omit)
    ]
    .boxed()
}

fn arb_nested<P: Clone + core::fmt::Debug + 'static>(
    doc: BoxedStrategy<P>,
) -> BoxedStrategy<Nested<P>> {
    prop_oneof![
        Just(Nested::Unset),
        doc.clone().prop_map(Nested::Patch),
        doc.prop_map(Nested::Replace),
        Just(Nested::Omit),
    ]
    .boxed()
}

fn arb_scaling() -> BoxedStrategy<Scaling> {
    (arb_field(0..3u8), arb_field(0..3u8))
        .prop_map(|(units, expr)| Scaling { units, expr })
        .boxed()
}

fn arb_table() -> BoxedStrategy<Table> {
    (
        arb_field(proptest::sample::select(Storage::ALL.to_vec())),
        arb_field(proptest::sample::select(vec![Endian::Big, Endian::Little])),
        arb_field(0..4u32),
        arb_nested(arb_scaling()),
    )
        .prop_map(|(storage, endian, address, scaling)| Table {
            storage,
            endian,
            address,
            scaling,
        })
        .boxed()
}

fn arb_keyed<P: Merge + Clone + core::fmt::Debug + 'static>(
    item: BoxedStrategy<P>,
) -> BoxedStrategy<Keyed<u8, P>> {
    proptest::collection::vec((0..6u8, arb_nested(item)), 0..6)
        .prop_map(|entries| entries.into_iter().collect())
        .boxed()
}

fn arb_appended() -> BoxedStrategy<Appended<u8>> {
    (any::<bool>(), proptest::collection::vec(0..4u8, 0..4))
        .prop_map(|(replace, items)| {
            if replace {
                Appended::replace(items)
            } else {
                Appended::append(items)
            }
        })
        .boxed()
}

fn arb_rom() -> BoxedStrategy<Rom> {
    (
        arb_field(0..3u8),
        arb_keyed(arb_table()),
        arb_appended(),
        arb_scaling(),
    )
        .prop_map(|(id, tables, notes, defaults)| Rom {
            id,
            tables,
            notes,
            defaults,
        })
        .boxed()
}

/// Identity and associativity (module docs, *Laws*).
fn monoid_laws<P: Merge + Clone + PartialEq + core::fmt::Debug>(
    a: &P,
    b: &P,
    c: &P,
) -> Result<(), TestCaseError> {
    let empty = P::default();
    prop_assert_eq!(&empty.merged(a), a, "left identity");
    prop_assert_eq!(&a.merged(&empty), a, "right identity");
    prop_assert_eq!(
        a.merged(b).merged(c),
        a.merged(&b.merged(c)),
        "associativity"
    );
    Ok(())
}

/// `a ⊕ a == a` (module docs, *Laws*).
fn idempotent<P: Merge + Clone + PartialEq + core::fmt::Debug>(a: &P) -> Result<(), TestCaseError> {
    prop_assert_eq!(&a.merged(a), a, "idempotence");
    Ok(())
}

proptest! {
    #[test]
    fn field_laws(a in arb_field(0..3u8), b in arb_field(0..3u8), c in arb_field(0..3u8)) {
        monoid_laws(&a, &b, &c)?;
        idempotent(&a)?;
    }

    #[test]
    fn nested_laws(a in arb_nested(arb_scaling()), b in arb_nested(arb_scaling()), c in arb_nested(arb_scaling())) {
        monoid_laws(&a, &b, &c)?;
        idempotent(&a)?;
    }

    #[test]
    fn keyed_laws(a in arb_keyed(arb_scaling()), b in arb_keyed(arb_scaling()), c in arb_keyed(arb_scaling())) {
        monoid_laws(&a, &b, &c)?;
        idempotent(&a)?;
    }

    #[test]
    fn appended_laws(a in arb_appended(), b in arb_appended(), c in arb_appended()) {
        monoid_laws(&a, &b, &c)?;
    }

    #[test]
    fn struct_laws(a in arb_table(), b in arb_table(), c in arb_table()) {
        monoid_laws(&a, &b, &c)?;
        idempotent(&a)?;
    }

    #[test]
    fn document_laws(a in arb_rom(), b in arb_rom(), c in arb_rom()) {
        monoid_laws(&a, &b, &c)?;
    }

    /// Appending is the one policy that is not idempotent, so a document
    /// is idempotent exactly when its appended lists are.
    #[test]
    fn document_idempotent_without_appends(mut a in arb_rom()) {
        a.notes = Appended::default();
        idempotent(&a)?;
    }

    /// The fold over a stack is the left-to-right merge of its documents.
    #[test]
    fn layered_merged_is_the_fold(docs in proptest::collection::vec(arb_rom(), 0..5)) {
        let mut stack = Layered::new();
        let mut fold = Rom::default();
        for (i, doc) in docs.iter().enumerate() {
            stack.push(layer(&format!("L{i}")), doc.clone());
            fold.merge_from(doc);
        }
        prop_assert_eq!(stack.merged(), fold);
    }

    /// MF-06: a child that sets a field wins over the base for every field,
    /// whatever the base and the child's other fields hold. Legacy inherited
    /// signedness only while the base's storage type was unset
    /// (`xml/TableScaleUnmarshaller.java:153`).
    #[test]
    fn mf06_overlay_field_always_wins(base in arb_table(), child in arb_table()) {
        let merged = base.merged(&child);
        if !child.storage.is_unset() {
            prop_assert_eq!(merged.storage, child.storage);
        } else {
            prop_assert_eq!(merged.storage, base.storage);
        }
        if !child.endian.is_unset() {
            prop_assert_eq!(merged.endian, child.endian);
        } else {
            prop_assert_eq!(merged.endian, base.endian);
        }
        if !child.address.is_unset() {
            prop_assert_eq!(merged.address, child.address);
        } else {
            prop_assert_eq!(merged.address, base.address);
        }
        if matches!(child.scaling, Nested::Replace(_) | Nested::Omit) {
            prop_assert_eq!(&merged.scaling, &child.scaling);
        }
        if child.scaling.is_unset() {
            prop_assert_eq!(&merged.scaling, &base.scaling);
        }
    }
}

// ---- MF-06: signedness ----

/// MF-06: a child switches a table between signed and unsigned (and any
/// other storage type) whatever the base's storage type is. Legacy only
/// re-read the sign while the inherited storage type was unset
/// (`if (table.getStorageType() < 1)`, `xml/TableScaleUnmarshaller.java:153`),
/// so a child of a `uint16` base could not become `int16`.
#[test]
fn mf06_child_storage_overrides_base_signedness() {
    for base in Storage::ALL {
        for child in Storage::ALL {
            let mut stack = Layered::new();
            stack.push(layer("base"), table(base, 0x100));
            stack.push(
                layer("child"),
                Table {
                    storage: Field::Set(child),
                    ..Table::default()
                },
            );
            let t = stack.merged();
            assert_eq!(t.storage, Field::Set(child), "{base} -> {child}");
            assert_eq!(
                t.storage.get().map(|s| s.is_signed()),
                Some(child.is_signed())
            );
            assert_eq!(t.address, Field::Set(0x100), "untouched field is inherited");
        }
    }
}

// ---- MF-07: omit in the middle of a list ----

/// MF-07: omitting one table in the middle removes only that table; every
/// later table keeps its address. Legacy's `filterFoundRomTables` hit
/// `return` at the first `omit="true"` table
/// (`xml/TableScaleUnmarshaller.java:330-333`), dropping all later ones.
#[test]
fn mf07_omit_in_the_middle_keeps_later_items() {
    let mut base = Rom::default();
    for (key, addr) in [(1, 0x100), (2, 0x200), (3, 0x300), (4, 0x400)] {
        base.tables.patch(key, table(Storage::U16, addr));
    }
    let mut child = Rom::default();
    child.tables.omit(2);

    let rom = base.merged(&child);
    let present: Vec<_> = rom
        .tables
        .present()
        .map(|(k, t)| (*k, t.address.get().copied()))
        .collect();
    assert_eq!(
        present,
        [(1, Some(0x100)), (3, Some(0x300)), (4, Some(0x400))]
    );
    assert_eq!(rom.tables.get(&2), Some(&Nested::Omit));
    assert_eq!(rom.tables.len(), 4, "the omit stays as a tombstone");
}

/// EC-04 (omit-then-override): an item omitted by one layer and defined
/// again by a later one comes back in its old position, without the fields
/// the omitted base had.
#[test]
fn omit_then_override_restarts_the_item_in_place() {
    let mut base = Keyed::new();
    base.patch(1, scaling(1));
    base.patch(
        2,
        Scaling {
            units: Field::Set(2),
            expr: Field::Set(2),
        },
    );
    base.patch(3, scaling(3));
    let mut omit = Keyed::new();
    omit.omit(2);
    let mut again = Keyed::new();
    again.patch(4, scaling(4));
    again.patch(
        2,
        Scaling {
            units: Field::Unset,
            expr: Field::Set(9),
        },
    );

    let out = base.merged(&omit).merged(&again);
    let present: Vec<_> = out.present().map(|(k, s)| (*k, s.clone())).collect();
    let restarted = Scaling {
        units: Field::Unset,
        expr: Field::Set(9),
    };
    assert_eq!(
        present,
        [
            (1, scaling(1)),
            (2, restarted.clone()),
            (3, scaling(3)),
            (4, scaling(4))
        ]
    );
    assert_eq!(out.get(&2), Some(&Nested::Replace(restarted)));
}

// ---- Field ----

#[test]
fn field_accessors() {
    let set = Field::Set(7u8);
    assert_eq!(set.get(), Some(&7));
    assert!(!set.is_unset());
    assert!(!set.is_omitted());

    let unset = Field::<u8>::Unset;
    assert_eq!(unset.get(), None);
    assert!(unset.is_unset());
    assert!(!unset.is_omitted());

    let omit = Field::<u8>::Omit;
    assert_eq!(omit.get(), None);
    assert!(!omit.is_unset());
    assert!(omit.is_omitted());

    assert_eq!(Field::<u8>::default(), Field::Unset);
}

#[test]
fn field_merge_table() {
    use Field::{Omit, Set, Unset};
    let cases: [(Field<u8>, Field<u8>, Field<u8>); 9] = [
        (Unset, Unset, Unset),
        (Unset, Set(2), Set(2)),
        (Unset, Omit, Omit),
        (Set(1), Unset, Set(1)),
        (Set(1), Set(2), Set(2)),
        (Set(1), Omit, Omit),
        (Omit, Unset, Omit),
        (Omit, Set(2), Set(2)),
        (Omit, Omit, Omit),
    ];
    for (base, overlay, want) in cases {
        assert_eq!(base.merged(&overlay), want, "{base:?} ⊕ {overlay:?}");
    }
}

// ---- Nested ----

#[test]
fn nested_accessors() {
    for (n, get, unset, omitted) in [
        (Nested::Unset, None, true, false),
        (Nested::Patch(scaling(1)), Some(scaling(1)), false, false),
        (Nested::Replace(scaling(2)), Some(scaling(2)), false, false),
        (Nested::Omit, None, false, true),
    ] {
        assert_eq!(n.get(), get.as_ref(), "{n:?}");
        assert_eq!(n.is_unset(), unset, "{n:?}");
        assert_eq!(n.is_omitted(), omitted, "{n:?}");
    }
    assert_eq!(Nested::<Scaling>::default(), Nested::Unset);
}

/// Every cell of the table in [`Nested`]'s docs.
#[test]
fn nested_merge_table() {
    use Nested::{Omit, Patch, Replace, Unset};
    let u = Scaling {
        units: Field::Set(1),
        expr: Field::Set(1),
    };
    let v = Scaling {
        units: Field::Set(2),
        expr: Field::Unset,
    };
    let uv = Scaling {
        units: Field::Set(2),
        expr: Field::Set(1),
    };
    let cases = [
        (Unset, Unset, Unset),
        (Unset, Patch(v.clone()), Patch(v.clone())),
        (Unset, Replace(v.clone()), Replace(v.clone())),
        (Unset, Omit, Omit),
        (Patch(u.clone()), Unset, Patch(u.clone())),
        (Patch(u.clone()), Patch(v.clone()), Patch(uv.clone())),
        (Patch(u.clone()), Replace(v.clone()), Replace(v.clone())),
        (Patch(u.clone()), Omit, Omit),
        (Replace(u.clone()), Unset, Replace(u.clone())),
        (Replace(u.clone()), Patch(v.clone()), Replace(uv.clone())),
        (Replace(u.clone()), Replace(v.clone()), Replace(v.clone())),
        (Replace(u.clone()), Omit, Omit),
        (Omit, Unset, Omit),
        (Omit, Patch(v.clone()), Replace(v.clone())),
        (Omit, Replace(v.clone()), Replace(v.clone())),
        (Omit, Omit, Omit),
    ];
    for (base, overlay, want) in cases {
        assert_eq!(base.merged(&overlay), want, "{base:?} ⊕ {overlay:?}");
    }
}

// ---- Keyed ----

#[test]
fn keyed_insert_merges_a_repeated_key_in_place() {
    let mut list = Keyed::new();
    assert!(list.is_empty());
    list.patch(5, scaling(1));
    list.patch(3, scaling(2));
    list.insert(
        5,
        Nested::Patch(Scaling {
            units: Field::Unset,
            expr: Field::Set(4),
        }),
    );
    assert_eq!(list.len(), 2);
    assert!(!list.is_empty());
    let entries: Vec<_> = list.entries().map(|(k, v)| (*k, v.clone())).collect();
    assert_eq!(
        entries,
        [
            (
                5,
                Nested::Patch(Scaling {
                    units: Field::Set(1),
                    expr: Field::Set(4),
                })
            ),
            (3, Nested::Patch(scaling(2))),
        ]
    );
    list.omit(5);
    assert_eq!(list.get(&5), Some(&Nested::Omit));
    assert_eq!(list.get(&3), Some(&Nested::Patch(scaling(2))));
    assert_eq!(list.get(&9), None);
    assert_eq!(list.len(), 2);
}

#[test]
fn keyed_present_skips_unset_and_omitted() {
    let list: Keyed<u8, Scaling> = [
        (1, Nested::Patch(scaling(1))),
        (2, Nested::Unset),
        (3, Nested::Omit),
        (4, Nested::Replace(scaling(4))),
    ]
    .into_iter()
    .collect();
    assert_eq!(list.len(), 4);
    let present: Vec<_> = list.present().map(|(k, s)| (*k, s.clone())).collect();
    assert_eq!(present, [(1, scaling(1)), (4, scaling(4))]);
}

#[test]
fn keyed_from_iter_merges_repeated_keys() {
    let list: Keyed<u8, Scaling> = [
        (1, Nested::Patch(scaling(1))),
        (1, Nested::Omit),
        (2, Nested::Patch(scaling(2))),
    ]
    .into_iter()
    .collect();
    let entries: Vec<_> = list.entries().map(|(k, v)| (*k, v.clone())).collect();
    assert_eq!(entries, [(1, Nested::Omit), (2, Nested::Patch(scaling(2)))]);
}

#[test]
fn keyed_merge_keeps_positions_and_appends_new_keys() {
    let base: Keyed<u8, Scaling> = [1, 2, 3]
        .into_iter()
        .map(|k| (k, Nested::Patch(scaling(k))))
        .collect();
    let overlay: Keyed<u8, Scaling> = [
        (9, Nested::Patch(scaling(9))),
        (2, Nested::Patch(scaling(20))),
        (7, Nested::Patch(scaling(7))),
    ]
    .into_iter()
    .collect();
    let out = base.merged(&overlay);
    let keys: Vec<_> = out.present().map(|(k, s)| (*k, s.units)).collect();
    assert_eq!(
        keys,
        [
            (1, Field::Set(1)),
            (2, Field::Set(20)),
            (3, Field::Set(3)),
            (9, Field::Set(9)),
            (7, Field::Set(7)),
        ]
    );
    assert_eq!(Keyed::<u8, Scaling>::default(), Keyed::new());
}

// ---- Appended ----

#[test]
fn appended_constructors_and_merge() {
    let base = Appended::append(vec![1u8, 2]);
    assert_eq!(base.items(), [1, 2]);
    assert!(!base.replaces());
    let replace = Appended::replace(vec![9]);
    assert!(replace.replaces());
    assert!(Appended::<u8>::omit().replaces());
    assert!(Appended::<u8>::omit().items().is_empty());
    assert_eq!(Appended::<u8>::default(), Appended::append(Vec::new()));

    assert_eq!(
        base.merged(&Appended::append(vec![3])),
        Appended::append(vec![1, 2, 3])
    );
    assert_eq!(base.merged(&replace), Appended::replace(vec![9]));
    assert_eq!(base.merged(&Appended::omit()), Appended::omit());
    assert_eq!(
        replace.merged(&Appended::append(vec![3])),
        Appended::replace(vec![9, 3])
    );
    assert_eq!(
        base.merged(&base),
        Appended::append(vec![1, 2, 1, 2]),
        "append is not idempotent"
    );
}

// ---- merge_fields! ----

/// The macro merges every field with its own policy.
#[test]
fn merge_fields_merges_every_field() {
    let mut base = Rom {
        id: Field::Set(1),
        notes: Appended::append(vec![1]),
        defaults: scaling(1),
        ..Rom::default()
    };
    base.tables.patch(1, table(Storage::U8, 1));
    let mut child = Rom {
        id: Field::Set(2),
        notes: Appended::append(vec![2]),
        defaults: Scaling {
            units: Field::Unset,
            expr: Field::Set(2),
        },
        ..Rom::default()
    };
    child.tables.patch(2, table(Storage::I8, 2));

    let mut want_tables = Keyed::new();
    want_tables.patch(1, table(Storage::U8, 1));
    want_tables.patch(2, table(Storage::I8, 2));
    assert_eq!(
        base.merged(&child),
        Rom {
            id: Field::Set(2),
            tables: want_tables,
            notes: Appended::append(vec![1, 2]),
            defaults: Scaling {
                units: Field::Set(1),
                expr: Field::Set(2),
            },
        }
    );
}

// ---- Layered ----

#[test]
fn layered_empty_stack() {
    let stack = Layered::<Rom>::new();
    assert!(stack.is_empty());
    assert_eq!(stack.len(), 0);
    assert_eq!(stack.merged(), Rom::default());
    assert_eq!(Layered::<Rom>::default(), stack);
}

#[test]
fn layered_upper_layer_wins_and_order_is_kept() {
    let mut stack = Layered::new();
    stack.push(layer("base"), Field::Set(1u8));
    stack.push(layer("mid"), Field::Unset);
    stack.push(layer("top"), Field::Set(3u8));
    assert_eq!(stack.len(), 3);
    assert!(!stack.is_empty());
    let ids: Vec<_> = stack
        .layers()
        .map(|(l, _)| l.id().as_str().to_owned())
        .collect();
    assert_eq!(ids, ["base", "mid", "top"]);
    assert_eq!(stack.merged(), Field::Set(3));
}

/// A base shared by two stacks is held by reference, never copied.
#[test]
fn layered_shares_bases_without_copying() {
    let mut base = Rom::default();
    base.tables.patch(1, table(Storage::U16, 0x100));
    let base = Arc::new(base);

    let mut a = Layered::new();
    let mut b = Layered::new();
    a.push(layer("base"), Arc::clone(&base));
    b.push(layer("base"), Arc::clone(&base));
    a.push(
        layer("a"),
        Rom {
            id: Field::Set(1),
            ..Rom::default()
        },
    );
    b.push(
        layer("b"),
        Rom {
            id: Field::Set(2),
            ..Rom::default()
        },
    );

    let shared = |s: &Layered<Rom>| s.layers().next().map(|(_, d)| Arc::clone(d));
    assert!(shared(&a).is_some_and(|d| Arc::ptr_eq(&d, &base)));
    assert!(shared(&b).is_some_and(|d| Arc::ptr_eq(&d, &base)));
    assert_eq!(a.merged().id, Field::Set(1));
    assert_eq!(b.merged().id, Field::Set(2));
    assert_eq!(a.merged().tables, base.tables);
}

// ---- layer metadata ----

#[test]
fn layer_id_is_text() {
    let id = LayerId::new("16BITBASE");
    assert_eq!(id.as_str(), "16BITBASE");
    assert_eq!(id.to_string(), "16BITBASE");
    assert_eq!(LayerId::from("16BITBASE"), id);
    assert_eq!(LayerId::from(String::from("16BITBASE")), id);
    assert!(LayerId::from("A") < LayerId::from("B"));
}

#[test]
fn layer_kind_names() {
    for (kind, name) in [
        (LayerKind::Builtin, "builtin"),
        (LayerKind::File, "file"),
        (LayerKind::User, "user"),
        (LayerKind::Cli, "cli"),
    ] {
        assert_eq!(kind.as_str(), name);
        assert_eq!(kind.to_string(), name);
    }
}

#[test]
fn layer_metadata_and_provenance() {
    let l = Layer::new(
        "A2WC522N".into(),
        LayerKind::User,
        "defs/A2WC522N.xml",
        Span::line_col(12, 3),
    );
    assert_eq!(l.id().as_str(), "A2WC522N");
    assert_eq!(l.kind(), LayerKind::User);
    assert_eq!(l.source(), "defs/A2WC522N.xml");
    assert_eq!(l.span(), Span::line_col(12, 3));
    assert_eq!(
        l.provenance().to_string(),
        "defs/A2WC522N.xml:12:3 [layer A2WC522N]"
    );
}
