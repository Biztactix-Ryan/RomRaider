//! Unit and property tests for per-field provenance and `explain`
//! (US-RR-22-11).
//!
//! Legacy references are to RomRaider's Java sources under
//! `legacy/src/main/java/com/romraider` (read, never run; D-024).

use proptest::prelude::*;

use super::*;
use crate::diag::Span;
use crate::encoding::Storage;
use crate::layer::{Complete, Document, LayerKind, Missing, Name, Registry};

// ---- test documents ----

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Scaling {
    units: Field<u8>,
    expr: Field<u8>,
}
crate::merge_fields!(Scaling { units, expr });

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Table {
    storage: Field<Storage>,
    scaling: Nested<Scaling>,
    axes: Keyed<u8, Scaling>,
    notes: Appended<u8>,
}
crate::merge_fields!(Table {
    storage,
    scaling,
    axes,
    notes
});

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Rom {
    id: Field<u8>,
    tables: Keyed<String, Table>,
    notes: Appended<u8>,
    defaults: Scaling,
}
crate::merge_fields!(Rom {
    id,
    tables,
    notes,
    defaults
});

impl Complete for Rom {
    type Output = u8;

    fn complete(&self) -> Result<u8, Missing> {
        let mut missing = Missing::new();
        missing.require("id", &self.id).copied().ok_or(missing)
    }
}

fn layer(id: &str, line: u32) -> Layer {
    Layer::new(
        id.into(),
        LayerKind::File,
        format!("{id}.xml"),
        Span::line_col(line, 1),
    )
}

/// A stack of `docs`, the layer of the `i`th named `L{i}` at line `i + 1`.
fn stack<P>(docs: Vec<P>) -> Layered<P> {
    let mut out = Layered::new();
    for (i, doc) in docs.into_iter().enumerate() {
        let i = u32::try_from(i).unwrap_or(u32::MAX);
        out.push(layer(&format!("L{i}"), i + 1), doc);
    }
    out
}

/// A ROM whose table `name` is in state `table`.
fn rom_with(name: &str, table: Nested<Table>) -> Rom {
    let mut rom = Rom::default();
    rom.tables.insert(name.to_owned(), table);
    rom
}

fn storage(s: Storage) -> Table {
    Table {
        storage: Field::Set(s),
        ..Table::default()
    }
}

fn tables() -> FieldPath<Rom, Keyed<String, Table>> {
    FieldPath::root().field("tables", |r: &Rom| &r.tables)
}

fn storage_of(name: &str) -> FieldPath<Rom, Field<Storage>> {
    tables()
        .item(name.to_owned())
        .field("storage", |t: &Table| &t.storage)
}

fn units_of(name: &str) -> FieldPath<Rom, Field<u8>> {
    tables()
        .item(name.to_owned())
        .field("scaling", |t: &Table| &t.scaling)
        .nested()
        .field("units", |s: &Scaling| &s.units)
}

fn notes() -> FieldPath<Rom, Appended<u8>> {
    FieldPath::root().field("notes", |r: &Rom| &r.notes)
}

/// The ids of the layers of `steps`, in the order given.
fn ids<'s, L: 's>(steps: impl Iterator<Item = &'s Step<'s, L>>) -> Vec<&'s str> {
    steps.map(|s| s.layer().id().as_str()).collect()
}

// ---- paths ----

#[test]
fn path_text_and_get() {
    let root = FieldPath::<Rom, Rom>::root();
    assert_eq!(root.as_str(), "");
    assert_eq!(root.to_string(), "(document)");
    let units = units_of("Boost Target");
    assert_eq!(units.as_str(), r#"tables["Boost Target"].scaling.units"#);
    assert_eq!(units.to_string(), units.as_str());
    assert_eq!(
        format!("{:?}", units.clone()),
        r#"FieldPath("tables[\"Boost Target\"].scaling.units")"#
    );
    let defaults = FieldPath::root()
        .field("defaults", |r: &Rom| &r.defaults)
        .field("expr", |s: &Scaling| &s.expr);
    assert_eq!(defaults.as_str(), "defaults.expr");

    let mut rom = rom_with(
        "A",
        Nested::Patch(Table {
            scaling: Nested::Replace(Scaling {
                units: Field::Set(7),
                ..Scaling::default()
            }),
            ..storage(Storage::I16)
        }),
    );
    rom.defaults.expr = Field::Set(3);
    rom.tables.omit("B".to_owned());
    assert_eq!(units_of("A").get(&rom), Some(&Field::Set(7)));
    assert_eq!(storage_of("A").get(&rom), Some(&Field::Set(Storage::I16)));
    assert_eq!(defaults.get(&rom), Some(&Field::Set(3)));
    assert_eq!(root.get(&rom), Some(&rom));
    // An absent key, an unset nested document, an omitted item: no field.
    assert_eq!(storage_of("C").get(&rom), None);
    assert_eq!(storage_of("B").get(&rom), None);
    assert_eq!(tables().key("B".to_owned()).get(&rom), Some(&Nested::Omit));
    let plain = rom_with("A", Nested::Patch(storage(Storage::U8)));
    assert_eq!(units_of("A").get(&plain), None);
}

#[test]
fn is_noop_per_policy() {
    assert!(Field::<u8>::Unset.is_noop());
    assert!(!Field::Set(1_u8).is_noop());
    assert!(!Field::<u8>::Omit.is_noop());
    assert!(Nested::<Scaling>::Unset.is_noop());
    assert!(!Nested::<Scaling>::Omit.is_noop());
    assert!(Keyed::<u8, Scaling>::new().is_noop());
    let mut keyed = Keyed::new();
    keyed.insert(1_u8, Nested::<Scaling>::Unset);
    assert!(!keyed.is_noop());
    assert!(Appended::<u8>::append(Vec::new()).is_noop());
    assert!(!Appended::append(vec![1_u8]).is_noop());
    assert!(!Appended::<u8>::omit().is_noop());
    assert!(!Appended::replace(vec![1_u8]).is_noop());
}

// ---- Field ----

/// MF-06: a child overriding a table's signedness wins, whatever the base's
/// storage type; legacy inherited signedness only while the base's storage
/// type was unset (`xml/TableScaleUnmarshaller.java:153`).
#[test]
fn mf06_signedness_override_names_the_child_and_the_base_value() {
    let mut s = Layered::new();
    s.push(
        layer("16BITBASE", 3),
        rom_with("A", Nested::Patch(storage(Storage::U16))),
    );
    s.push(
        layer("A2WC522N", 2),
        rom_with("A", Nested::Patch(storage(Storage::I16))),
    );
    let e = s.explain(&storage_of("A"));
    assert_eq!(e.path(), r#"tables["A"].storage"#);
    assert_eq!(e.value(), Some(&Storage::I16));
    let winner = e.winner().map(Step::layer).map(Layer::id);
    assert_eq!(winner, Some(&"A2WC522N".into()));
    assert_eq!(ids(e.overridden()), ["16BITBASE"]);
    assert_eq!(ids(e.history()), ["A2WC522N", "16BITBASE"]);
    let base = e.overridden().next().map(Step::change);
    assert_eq!(base, Some(&Change::Apply(&Field::Set(Storage::U16))));
    assert_eq!(
        e.winner().map(Step::provenance),
        Some(
            s.layers()
                .nth(1)
                .map(|(l, _)| l.provenance())
                .unwrap_or_else(|| unreachable!())
        )
    );
    assert_eq!(
        e.to_string(),
        "tables[\"A\"].storage = I16\n  \
         set to I16 at A2WC522N.xml:2:1 [layer A2WC522N]\n  \
         overrides:\n    \
         set to U16 at 16BITBASE.xml:3:1 [layer 16BITBASE]"
    );
    assert_eq!(
        e.value(),
        storage_of("A").get(&s.merged()).and_then(Field::get)
    );
}

#[test]
fn field_never_set() {
    let s = stack(vec![
        Rom::default(),
        rom_with("A", Nested::Patch(Table::default())),
    ]);
    let e = s.explain(&storage_of("A"));
    assert_eq!(e.value(), None);
    assert!(e.winner().is_none());
    assert_eq!(e.history().len(), 0);
    assert_eq!(
        e.to_string(),
        "tables[\"A\"].storage: no value\n  not set by any layer"
    );
    let unset = stack(vec![Field::<u8>::Unset]);
    let root = unset.explain(&FieldPath::root());
    assert_eq!(
        root.to_string(),
        "(document): no value\n  not set by any layer"
    );
}

#[test]
fn field_omitted_cleared_and_removed() {
    let set = |s| rom_with("A", Nested::Patch(storage(s)));
    let s = stack(vec![
        set(Storage::U8),
        rom_with(
            "A",
            Nested::Patch(Table {
                storage: Field::Omit,
                ..Table::default()
            }),
        ),
        set(Storage::U16),
        rom_with("A", Nested::Replace(Table::default())),
        rom_with("A", Nested::Replace(storage(Storage::I8))),
        rom_with("A", Nested::Omit),
        // A patch of nothing over the omit: brings the table back, not the
        // field.
        rom_with("A", Nested::Patch(Table::default())),
    ]);
    let e = s.explain(&storage_of("A"));
    assert_eq!(e.value(), None);
    assert_eq!(ids(e.history()), ["L5", "L4", "L3", "L2", "L1", "L0"]);
    assert_eq!(
        e.to_string(),
        "tables[\"A\"].storage: no value\n  \
         removed (tables[\"A\"] omitted) at L5.xml:6:1 [layer L5]\n  \
         overrides:\n    \
         set to I8 (tables[\"A\"] replaced) at L4.xml:5:1 [layer L4]\n    \
         cleared (tables[\"A\"] replaced) at L3.xml:4:1 [layer L3]\n    \
         set to U16 at L2.xml:3:1 [layer L2]\n    \
         omitted at L1.xml:2:1 [layer L1]\n    \
         set to U8 at L0.xml:1:1 [layer L0]"
    );
    let by: Arc<str> = r#"tables["A"]"#.into();
    let changes: Vec<_> = e.history().map(Step::change).cloned().collect();
    assert_eq!(
        changes,
        [
            Change::Removed { by: by.clone() },
            Change::Reset {
                by: by.clone(),
                leaf: Some(&Field::Set(Storage::I8))
            },
            Change::Reset {
                by,
                leaf: Some(&Field::Unset)
            },
            Change::Apply(&Field::Set(Storage::U16)),
            Change::Apply(&Field::Omit),
            Change::Apply(&Field::Set(Storage::U8)),
        ]
    );
    assert_eq!(storage_of("A").get(&s.merged()), Some(&Field::Unset));

    // Each state of the winner, alone.
    let top = |docs: Vec<Rom>| {
        let s = stack(docs);
        let e = s.explain(&storage_of("A"));
        (e.value().copied(), e.to_string())
    };
    assert_eq!(
        top(vec![
            set(Storage::U8),
            rom_with("A", Nested::Replace(Table::default()))
        ]),
        (
            None,
            "tables[\"A\"].storage: no value\n  \
             cleared (tables[\"A\"] replaced) at L1.xml:2:1 [layer L1]\n  \
             overrides:\n    set to U8 at L0.xml:1:1 [layer L0]"
                .to_owned()
        )
    );
    assert_eq!(
        top(vec![
            set(Storage::U8),
            rom_with("A", Nested::Replace(storage(Storage::I32)))
        ])
        .0,
        Some(Storage::I32)
    );
    let omit = Table {
        storage: Field::Omit,
        ..Table::default()
    };
    assert_eq!(
        top(vec![rom_with("A", Nested::Replace(omit))]),
        (
            None,
            "tables[\"A\"].storage: no value\n  \
             omitted (tables[\"A\"] replaced) at L0.xml:1:1 [layer L0]"
                .to_owned()
        )
    );
}

#[test]
fn reset_names_the_outermost_replaced_container() {
    let scaled = |units| Table {
        scaling: Nested::Replace(Scaling {
            units: Field::Set(units),
            ..Scaling::default()
        }),
        ..Table::default()
    };
    let s = stack(vec![
        rom_with("A", Nested::Patch(scaled(1))),
        rom_with("A", Nested::Replace(scaled(2))),
        // Replaces the table without a scaling: the field goes with it.
        rom_with("A", Nested::Replace(Table::default())),
        rom_with(
            "A",
            Nested::Patch(Table {
                scaling: Nested::Omit,
                ..Table::default()
            }),
        ),
    ]);
    let e = s.explain(&units_of("A"));
    let changes: Vec<_> = e.history().map(Step::change).cloned().collect();
    assert_eq!(
        changes,
        [
            Change::Removed {
                by: r#"tables["A"].scaling"#.into()
            },
            Change::Reset {
                by: r#"tables["A"]"#.into(),
                leaf: None
            },
            Change::Reset {
                by: r#"tables["A"]"#.into(),
                leaf: Some(&Field::Set(2))
            },
            Change::Reset {
                by: r#"tables["A"].scaling"#.into(),
                leaf: Some(&Field::Set(1))
            },
        ]
    );
    assert_eq!(changes[1].leaf(), None);
    assert_eq!(changes[2].leaf(), Some(&Field::Set(2)));
    assert_eq!(changes[0].leaf(), None);
    assert_eq!(
        e.to_string(),
        "tables[\"A\"].scaling.units: no value\n  \
         removed (tables[\"A\"].scaling omitted) at L3.xml:4:1 [layer L3]\n  \
         overrides:\n    \
         cleared (tables[\"A\"] replaced) at L2.xml:3:1 [layer L2]\n    \
         set to 2 (tables[\"A\"] replaced) at L1.xml:2:1 [layer L1]\n    \
         set to 1 (tables[\"A\"].scaling replaced) at L0.xml:1:1 [layer L0]"
    );
}

// ---- Nested ----

#[test]
fn nested_actions_and_status() {
    let table = |s| Nested::Patch(storage(s));
    let docs = vec![
        rom_with("A", table(Storage::U8)),                     // added
        rom_with("A", table(Storage::U16)),                    // modified
        rom_with("A", Nested::Replace(storage(Storage::U32))), // replaced
        rom_with("A", Nested::Omit),                           // omitted
        rom_with("A", table(Storage::I8)),                     // restored
        rom_with("A", Nested::Omit),                           // omitted
        rom_with("A", Nested::Replace(storage(Storage::I16))), // restored
    ];
    let s = stack(docs);
    let e = s.explain(&tables().key("A".to_owned()));
    let actions: Vec<_> = e.actions().into_iter().map(|(_, a)| a).collect();
    use Action::*;
    assert_eq!(
        actions,
        [
            Restored, Omitted, Restored, Omitted, Replaced, Modified, Added
        ]
    );
    assert!(e.is_present());
    assert_eq!(
        e.to_string(),
        "tables[\"A\"]: present\n  \
         restored at L6.xml:7:1 [layer L6]\n  \
         omitted at L5.xml:6:1 [layer L5]\n  \
         restored at L4.xml:5:1 [layer L4]\n  \
         omitted at L3.xml:4:1 [layer L3]\n  \
         replaced at L2.xml:3:1 [layer L2]\n  \
         modified at L1.xml:2:1 [layer L1]\n  \
         added at L0.xml:1:1 [layer L0]"
    );

    // Each resolved state, by the strongest action.
    let status = |docs: Vec<Rom>| {
        let s = stack(docs);
        let e = s.explain(&tables().key("A".to_owned()));
        (e.is_present(), e.to_string())
    };
    assert_eq!(
        status(vec![]),
        (false, "tables[\"A\"]: not mentioned".to_owned())
    );
    assert_eq!(
        status(vec![rom_with("A", Nested::Replace(Table::default()))]),
        (
            true,
            "tables[\"A\"]: present\n  added at L0.xml:1:1 [layer L0]".to_owned()
        )
    );
    assert_eq!(
        status(vec![
            rom_with("A", table(Storage::U8)),
            rom_with("A", Nested::Omit)
        ]),
        (
            false,
            "tables[\"A\"]: omitted\n  \
             omitted at L1.xml:2:1 [layer L1]\n  \
             added at L0.xml:1:1 [layer L0]"
                .to_owned()
        )
    );
}

#[test]
fn nested_actions_through_containers() {
    let path = tables()
        .item("A".to_owned())
        .field("scaling", |t: &Table| &t.scaling);
    let scaled = |n: Nested<Scaling>| Table {
        scaling: n,
        ..Table::default()
    };
    let patch = || Nested::Patch(Scaling::default());
    let s = stack(vec![
        rom_with("A", Nested::Replace(scaled(patch()))), // added (reset, nothing before)
        rom_with("A", Nested::Replace(scaled(patch()))), // replaced (reset, present)
        rom_with("A", Nested::Replace(Table::default())), // cleared
        rom_with("A", Nested::Replace(scaled(Nested::Omit))), // omitted
        rom_with("A", Nested::Replace(scaled(patch()))), // restored (reset, gone)
        rom_with("A", Nested::Omit),                     // removed
        rom_with("A", Nested::Patch(scaled(patch()))),   // restored
        rom_with("A", Nested::Omit),                     // removed
    ]);
    let e = s.explain(&path);
    use Action::*;
    let actions: Vec<_> = e.actions().into_iter().map(|(_, a)| a).collect();
    assert_eq!(
        actions,
        [
            Removed, Restored, Removed, Restored, Omitted, Cleared, Replaced, Added
        ]
    );
    assert!(!e.is_present());
    assert!(e.to_string().starts_with(
        "tables[\"A\"].scaling: absent\n  \
         removed (tables[\"A\"] omitted) at L7.xml:8:1 [layer L7]\n  \
         restored at L6.xml:7:1 [layer L6]"
    ));
    let cleared = stack(vec![
        rom_with("A", Nested::Patch(scaled(patch()))),
        rom_with("A", Nested::Replace(Table::default())),
    ]);
    let e = cleared.explain(&path);
    assert_eq!(
        e.to_string(),
        "tables[\"A\"].scaling: unset\n  \
         cleared (tables[\"A\"] replaced) at L1.xml:2:1 [layer L1]\n  \
         added at L0.xml:1:1 [layer L0]"
    );
    let omitted = stack(vec![rom_with("A", Nested::Patch(scaled(Nested::Omit)))]);
    assert_eq!(
        omitted.explain(&path).to_string(),
        "tables[\"A\"].scaling: omitted\n  omitted at L0.xml:1:1 [layer L0]"
    );
}

#[test]
fn action_names() {
    use Action::*;
    let all = [
        Added, Restored, Modified, Replaced, Omitted, Cleared, Removed,
    ];
    let names: Vec<_> = all.iter().map(ToString::to_string).collect();
    assert_eq!(
        names,
        [
            "added", "restored", "modified", "replaced", "omitted", "cleared", "removed"
        ]
    );
    let present: Vec<_> = all.iter().map(|a| a.is_present()).collect();
    assert_eq!(present, [true, true, true, true, false, false, false]);
}

// ---- Keyed ----

/// MF-07: omitting one item in the middle keeps every later one, and
/// `explain` says which layer added, modified and omitted each.
#[test]
fn keyed_items_added_modified_omitted() {
    let mut base = Rom::default();
    for (name, s) in [("A", Storage::U8), ("B", Storage::U16), ("C", Storage::U32)] {
        base.tables.patch(name.to_owned(), storage(s));
    }
    let mut child = Rom::default();
    child.tables.omit("B".to_owned());
    child.tables.patch("C".to_owned(), storage(Storage::I32));
    child.tables.insert("E".to_owned(), Nested::Unset);
    child.tables.patch("D".to_owned(), storage(Storage::I8));
    let s = stack(vec![base, Rom::default(), child]);
    let e = s.explain(&tables());
    assert_eq!(e.keys(), ["A", "B", "C", "E", "D"]);
    for key in ["A", "B", "C", "D", "E"] {
        assert_eq!(
            e.item(&key.to_owned()),
            s.explain(&tables().key(key.to_owned()))
        );
    }
    assert_eq!(e.item(&"E".to_owned()).history().len(), 0);
    assert_eq!(
        e.to_string(),
        "tables: 5 entries\n  \
         [\"A\"]: present\n    \
         added at L0.xml:1:1 [layer L0]\n  \
         [\"B\"]: omitted\n    \
         omitted at L2.xml:3:1 [layer L2]\n    \
         added at L0.xml:1:1 [layer L0]\n  \
         [\"C\"]: present\n    \
         modified at L2.xml:3:1 [layer L2]\n    \
         added at L0.xml:1:1 [layer L0]\n  \
         [\"E\"]: not mentioned\n  \
         [\"D\"]: present\n    \
         added at L2.xml:3:1 [layer L2]"
    );
    let keys: Vec<_> = s
        .merged()
        .tables
        .entries()
        .map(|(k, _)| k.clone())
        .collect();
    assert_eq!(e.keys(), keys.iter().collect::<Vec<_>>());
}

#[test]
fn keyed_list_replaced_and_removed_with_its_container() {
    let axes = |keys: &[u8]| {
        let mut t = Table::default();
        for k in keys {
            t.axes.patch(*k, Scaling::default());
        }
        t
    };
    let path = tables()
        .item("A".to_owned())
        .field("axes", |t: &Table| &t.axes);
    let s = stack(vec![
        rom_with("A", Nested::Patch(axes(&[1, 2]))),
        rom_with("A", Nested::Replace(axes(&[3, 1]))),
        rom_with("A", Nested::Patch(axes(&[4]))),
    ]);
    let e = s.explain(&path);
    assert_eq!(e.keys(), [&3, &1, &4]);
    let item = e.item(&2);
    assert_eq!(
        item.history()
            .map(Step::change)
            .cloned()
            .collect::<Vec<_>>(),
        [
            Change::Reset {
                by: r#"tables["A"]"#.into(),
                leaf: None
            },
            Change::Apply(&Nested::Patch(Scaling::default())),
        ]
    );
    assert_eq!(item, s.explain(&path.clone().key(2)));
    assert_eq!(item.path(), r#"tables["A"].axes[2]"#);

    let s = stack(vec![
        rom_with("A", Nested::Patch(axes(&[1, 2]))),
        rom_with("A", Nested::Replace(Table::default())),
    ]);
    assert!(s.explain(&path).keys().is_empty());
    let s = stack(vec![
        rom_with("A", Nested::Patch(axes(&[1, 2]))),
        rom_with("A", Nested::Omit),
    ]);
    let e = s.explain(&path);
    assert!(e.keys().is_empty());
    assert_eq!(
        e.item(&1)
            .history()
            .map(Step::change)
            .cloned()
            .collect::<Vec<_>>(),
        [
            Change::Removed {
                by: r#"tables["A"]"#.into()
            },
            Change::Apply(&Nested::Patch(Scaling::default())),
        ]
    );
    assert_eq!(e.item(&1), s.explain(&path.clone().key(1)));
    assert_eq!(e.to_string(), "tables[\"A\"].axes: 0 entries");
}

// ---- Appended ----

#[test]
fn appended_contributions_and_replace_boundaries() {
    let notes_of = |a: Appended<u8>| Rom {
        notes: a,
        ..Rom::default()
    };
    let s = stack(vec![
        notes_of(Appended::append(vec![1])),
        notes_of(Appended::replace(vec![2])),
        notes_of(Appended::append(vec![])),
        notes_of(Appended::append(vec![3, 4])),
        notes_of(Appended::append(vec![5])),
    ]);
    let e = s.explain(&notes());
    assert_eq!(e.items(), [&2, &3, &4, &5]);
    assert_eq!(
        e.items(),
        s.merged().notes.items().iter().collect::<Vec<_>>()
    );
    assert_eq!(ids(e.live()), ["L4", "L3", "L1"]);
    assert_eq!(ids(e.dropped()), ["L0"]);
    assert_eq!(
        e.to_string(),
        "notes = [2, 3, 4, 5]\n  \
         appended [5] at L4.xml:5:1 [layer L4]\n  \
         appended [3, 4] at L3.xml:4:1 [layer L3]\n  \
         replaced with [2] at L1.xml:2:1 [layer L1]\n  \
         dropped:\n    \
         appended [1] at L0.xml:1:1 [layer L0]"
    );

    // Without a replace, every contribution is live.
    let s = stack(vec![
        notes_of(Appended::append(vec![1])),
        notes_of(Appended::append(vec![2])),
    ]);
    let e = s.explain(&notes());
    assert_eq!(e.items(), [&1, &2]);
    assert_eq!(ids(e.dropped()), Vec::<&str>::new());
    assert_eq!(
        e.to_string(),
        "notes = [1, 2]\n  \
         appended [2] at L1.xml:2:1 [layer L1]\n  \
         appended [1] at L0.xml:1:1 [layer L0]"
    );
    let s = stack(vec![Rom::default()]);
    assert_eq!(
        s.explain(&notes()).to_string(),
        "notes = []\n  not set by any layer"
    );
}

#[test]
fn appended_dropped_with_its_container() {
    let path = tables()
        .item("A".to_owned())
        .field("notes", |t: &Table| &t.notes);
    let noted = |items: Vec<u8>| Table {
        notes: Appended::append(items),
        ..Table::default()
    };
    let s = stack(vec![
        rom_with("A", Nested::Patch(noted(vec![1]))),
        rom_with("A", Nested::Replace(noted(vec![2]))),
        rom_with("A", Nested::Patch(noted(vec![3]))),
        rom_with("A", Nested::Replace(Table::default())),
        rom_with("A", Nested::Patch(noted(vec![4]))),
        rom_with("A", Nested::Omit),
        rom_with("A", Nested::Patch(noted(vec![5]))),
    ]);
    let e = s.explain(&path);
    assert_eq!(e.items(), [&5]);
    assert_eq!(
        e.to_string(),
        "tables[\"A\"].notes = [5]\n  \
         appended [5] at L6.xml:7:1 [layer L6]\n  \
         removed (tables[\"A\"] omitted) at L5.xml:6:1 [layer L5]\n  \
         dropped:\n    \
         appended [4] at L4.xml:5:1 [layer L4]\n    \
         cleared (tables[\"A\"] replaced) at L3.xml:4:1 [layer L3]\n    \
         appended [3] at L2.xml:3:1 [layer L2]\n    \
         set to [2] (tables[\"A\"] replaced) at L1.xml:2:1 [layer L1]\n    \
         appended [1] at L0.xml:1:1 [layer L0]"
    );
    let s = stack(vec![
        rom_with("A", Nested::Patch(noted(vec![1]))),
        rom_with("A", Nested::Replace(noted(vec![2]))),
        rom_with("A", Nested::Patch(noted(vec![3]))),
    ]);
    assert_eq!(s.explain(&path).items(), [&2, &3]);
    assert_eq!(
        s.explain(&path).items(),
        path.get(&s.merged())
            .map(Appended::items)
            .unwrap_or_default()
            .iter()
            .collect::<Vec<_>>()
    );
}

// ---- lineages ----

#[test]
fn lineage_and_resolved_explain_their_stack() {
    let mut registry = Registry::new();
    let doc = |id: &str, line, rom: Rom| Document::new(layer(id, line), rom);
    let base = Rom {
        id: Field::Set(1),
        ..rom_with("A", Nested::Patch(storage(Storage::U16)))
    };
    let child = Rom {
        id: Field::Set(2),
        ..rom_with("A", Nested::Patch(storage(Storage::I16)))
    };
    let inserted = [
        registry.insert(doc("16BITBASE", 3, base)),
        registry.insert(doc("A2WC522N", 2, child).with_base("16BITBASE")),
    ];
    assert!(inserted.iter().all(Result::is_ok));
    let mut resolver = registry.resolver();
    let Ok(resolved) = resolver.resolve(&Name::global("A2WC522N")) else {
        unreachable!("the family resolves")
    };
    assert_eq!(*resolved.value(), 2);
    let path = storage_of("A");
    let e = resolved.explain(&path);
    assert_eq!(e, resolved.lineage().stack().explain(&path));
    assert_eq!(e, resolved.lineage().explain(&path));
    assert_eq!(e.value(), Some(&Storage::I16));
    assert_eq!(ids(e.history()), ["A2WC522N", "16BITBASE"]);
    let id = resolved.explain(&FieldPath::root().field("id", |r: &Rom| &r.id));
    assert_eq!(id.value(), Some(&2));
}

// ---- properties ----

fn arb_field(values: u8) -> BoxedStrategy<Field<u8>> {
    prop_oneof![
        Just(Field::Unset),
        (0..values).prop_map(Field::Set),
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
    (arb_field(3), arb_field(3))
        .prop_map(|(units, expr)| Scaling { units, expr })
        .boxed()
}

fn arb_appended() -> BoxedStrategy<Appended<u8>> {
    (any::<bool>(), proptest::collection::vec(0..4_u8, 0..3))
        .prop_map(|(r, items)| {
            if r {
                Appended::replace(items)
            } else {
                Appended::append(items)
            }
        })
        .boxed()
}

fn arb_table() -> BoxedStrategy<Table> {
    let storages = prop_oneof![Just(Storage::U16), Just(Storage::I16)];
    let storage = prop_oneof![
        Just(Field::Unset),
        storages.prop_map(Field::Set),
        Just(Field::Omit)
    ];
    let axes = proptest::collection::vec((0..3_u8, arb_nested(arb_scaling())), 0..3)
        .prop_map(|entries| entries.into_iter().collect::<Keyed<u8, Scaling>>());
    (storage, arb_nested(arb_scaling()), axes, arb_appended())
        .prop_map(|(storage, scaling, axes, notes)| Table {
            storage,
            scaling,
            axes,
            notes,
        })
        .boxed()
}

fn arb_rom() -> BoxedStrategy<Rom> {
    let names = prop_oneof![Just("A".to_owned()), Just("B".to_owned())];
    (
        arb_field(3),
        proptest::collection::vec((names, arb_nested(arb_table())), 0..3),
        arb_appended(),
    )
        .prop_map(|(id, tables, notes)| Rom {
            id,
            tables: tables.into_iter().collect(),
            notes,
            defaults: Scaling::default(),
        })
        .boxed()
}

proptest! {
    /// The explained value is the resolved one, at every depth and for
    /// every policy, and the winner is the strongest layer the path
    /// reaches; dropping every layer above the winner changes nothing.
    #[test]
    fn explain_agrees_with_the_merge(docs in proptest::collection::vec(arb_rom(), 0..6)) {
        let s = stack(docs);
        let merged = s.merged();
        let storage = storage_of("A");
        let e = s.explain(&storage);
        prop_assert_eq!(e.value(), storage.get(&merged).and_then(Field::get));
        let units = units_of("B");
        prop_assert_eq!(s.explain(&units).value(), units.get(&merged).and_then(Field::get));
        let n = notes();
        let notes_items = n.get(&merged).map(Appended::items).unwrap_or_default();
        prop_assert_eq!(s.explain(&n).items(), notes_items.iter().collect::<Vec<_>>());
        let axes = tables().item("A".to_owned()).field("axes", |t: &Table| &t.axes);
        let ea = s.explain(&axes);
        let keys: Vec<_> = axes.get(&merged).into_iter().flat_map(Keyed::entries).map(|(k, _)| k).collect();
        prop_assert_eq!(ea.keys(), keys);
        for k in 0..3 {
            let present = axes.get(&merged).and_then(|l| l.get(&k)).is_some_and(|n| n.get().is_some());
            prop_assert_eq!(ea.item(&k).is_present(), present);
            prop_assert_eq!(ea.item(&k), s.explain(&axes.clone().key(k)));
        }
        let table = tables().key("A".to_owned());
        let present = table.get(&merged).is_some_and(|n| n.get().is_some());
        prop_assert_eq!(s.explain(&table).is_present(), present);

        // The winner is the strongest layer whose document reaches the
        // field, and the stack up to it gives the same value.
        let touches = |rom: &Rom| match rom.tables.get(&"A".to_owned()) {
            None | Some(Nested::Unset) => false,
            Some(Nested::Omit | Nested::Replace(_)) => true,
            Some(Nested::Patch(t)) => !t.storage.is_unset(),
        };
        let layers: Vec<_> = s.layers().collect();
        let strongest = layers.iter().rposition(|(_, doc)| touches(doc));
        let winner = e.winner().map(|w| w.layer().id().clone());
        prop_assert_eq!(winner, strongest.map(|i| layers[i].0.id().clone()));
        let touching = layers.iter().filter(|(_, doc)| touches(doc)).count();
        prop_assert_eq!(e.history().len(), touching);
        if let Some(i) = strongest {
            let mut prefix = Layered::new();
            for (layer, doc) in s.layers().take(i + 1) {
                prefix.push(layer.clone(), Arc::clone(doc));
            }
            let truncated = prefix.merged();
            prop_assert_eq!(storage.get(&truncated).and_then(Field::get), e.value());
        }
    }
}
