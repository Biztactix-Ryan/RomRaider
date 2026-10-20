//! Unit and property tests for the resolver (US-RR-22-8).
//!
//! Legacy references are to RomRaider's Java sources under
//! `legacy/src/main/java/com/romraider` (read, never run; D-024).

use std::collections::BTreeMap;
use std::sync::Arc;

use proptest::prelude::*;

use super::*;
use crate::diag::{Severity, Span};
use crate::encoding::Storage;
use crate::layer::{Appended, Keyed, LayerKind, Nested};

// ---- test documents ----

/// A table-like document; signedness is carried by the storage type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Table {
    storage: Field<Storage>,
    address: Field<u32>,
}
crate::merge_fields!(Table { storage, address });

/// A ROM-definition-like document, with replace, keyed and append fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Rom {
    id: Field<u8>,
    tables: Keyed<u8, Table>,
    notes: Appended<u8>,
}
crate::merge_fields!(Rom { id, tables, notes });

#[derive(Debug, Clone, PartialEq, Eq)]
struct TableDef {
    storage: Storage,
    address: u32,
}

/// The finished ROM: every required field present.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RomDef {
    id: u8,
    tables: Vec<(u8, TableDef)>,
    notes: Vec<u8>,
}

impl Complete for Rom {
    type Output = RomDef;

    fn complete(&self) -> Result<RomDef, Missing> {
        let mut missing = Missing::new();
        let id = missing.require("id", &self.id).copied();
        let mut tables = Vec::new();
        for (key, t) in self.tables.present() {
            let storage = missing.require(&format!("tables[{key}].storage"), &t.storage);
            let address = missing.require(&format!("tables[{key}].address"), &t.address);
            if let (Some(&storage), Some(&address)) = (storage, address) {
                tables.push((*key, TableDef { storage, address }));
            }
        }
        match id {
            Some(id) if missing.is_empty() => Ok(RomDef {
                id,
                tables,
                notes: self.notes.items().to_vec(),
            }),
            _ => Err(missing),
        }
    }
}

fn layer(id: &str) -> Layer {
    Layer::new(
        id.into(),
        LayerKind::File,
        format!("{id}.xml"),
        Span::line_col(2, 1),
    )
}

fn doc(id: &str, rom: Rom) -> Document<Rom> {
    Document::new(layer(id), rom)
}

fn registry(docs: impl IntoIterator<Item = Document<Rom>>) -> Registry<Rom> {
    let mut out = Registry::new();
    for d in docs {
        out.insert(d).unwrap();
    }
    out
}

fn name(id: &str) -> Name {
    Name::global(id)
}

fn order(lineage: &Lineage<Rom>) -> Vec<String> {
    lineage.names().iter().map(ToString::to_string).collect()
}

fn with_id(id: u8) -> Rom {
    Rom {
        id: Field::Set(id),
        ..Rom::default()
    }
}

fn with_notes(notes: &[u8]) -> Rom {
    Rom {
        notes: Appended::append(notes.to_vec()),
        ..Rom::default()
    }
}

fn table(storage: Option<Storage>, address: Option<u32>) -> Table {
    Table {
        storage: storage.map_or(Field::Unset, Field::Set),
        address: address.map_or(Field::Unset, Field::Set),
    }
}

fn lineage(reg: &Registry<Rom>, id: &str) -> Result<Arc<Lineage<Rom>>, ResolveError> {
    reg.resolver().lineage(&name(id))
}

fn resolve(reg: &Registry<Rom>, id: &str) -> Result<RomDef, ResolveError> {
    reg.resolver().resolve(&name(id)).map(Resolved::into_value)
}

// ---- chains ----

#[test]
fn a_document_without_bases_is_its_own_lineage() {
    let reg = registry([doc("a", with_id(1))]);
    let l = lineage(&reg, "a").unwrap();
    assert_eq!(order(&l), ["a"]);
    assert_eq!(l.name(), &name("a"));
    assert_eq!(l.layer(), &layer("a"));
    assert_eq!(**l.merged(), with_id(1));
    let (top, d) = l.stack().layers().next().unwrap();
    assert_eq!(top, &layer("a"));
    assert!(Arc::ptr_eq(d, reg.get(&name("a")).unwrap().doc()));
}

/// A base chain resolves weakest first: replace, append and keyed fields
/// all follow the chain, and the merged document is the stack's fold.
#[test]
fn a_chain_resolves_weakest_first() {
    let mut base = with_id(1);
    base.notes = Appended::append(vec![1]);
    base.tables.patch(1, table(Some(Storage::U8), Some(0x10)));
    let mut mid = with_notes(&[2]);
    mid.tables.patch(2, table(Some(Storage::U16), Some(0x20)));
    let mut top = with_id(3);
    top.notes = Appended::append(vec![3]);

    let reg = registry([
        doc("top", top).with_base("mid"),
        doc("mid", mid).with_base("base"),
        doc("base", base),
    ]);
    let l = lineage(&reg, "top").unwrap();
    assert_eq!(order(&l), ["base", "mid", "top"]);
    assert_eq!(**l.merged(), l.stack().merged());
    let ids: Vec<&str> = l.stack().layers().map(|(x, _)| x.id().as_str()).collect();
    assert_eq!(ids, ["base", "mid", "top"]);

    assert_eq!(
        resolve(&reg, "top").unwrap(),
        RomDef {
            id: 3,
            tables: vec![
                (
                    1,
                    TableDef {
                        storage: Storage::U8,
                        address: 0x10
                    }
                ),
                (
                    2,
                    TableDef {
                        storage: Storage::U16,
                        address: 0x20
                    }
                ),
            ],
            notes: vec![1, 2, 3],
        }
    );
    // A base resolves on its own, without its children.
    assert_eq!(resolve(&reg, "mid").unwrap().id, 1);
}

/// MF-06: a child's storage type overrides its base's through a chain,
/// switching signedness either way, whatever the base set. Legacy applied a
/// child's sign only while the inherited storage type was unset
/// (`xml/TableScaleUnmarshaller.java:153`).
#[test]
fn mf06_signedness_override_wins_through_a_chain() {
    for (base_storage, child_storage) in [
        (Storage::U16, Storage::I16),
        (Storage::I16, Storage::U16),
        (Storage::U8, Storage::I32),
        (Storage::F32, Storage::U32),
    ] {
        let mut base = with_id(1);
        base.tables.patch(7, table(Some(base_storage), Some(0x100)));
        let mut mid = Rom::default();
        mid.tables.patch(7, table(None, Some(0x200)));
        let mut child = Rom::default();
        child.tables.patch(7, table(Some(child_storage), None));
        let reg = registry([
            doc("base", base),
            doc("mid", mid).with_base("base"),
            doc("child", child).with_base("mid"),
            doc("grandchild", Rom::default()).with_base("child"),
        ]);
        let expect = vec![(
            7,
            TableDef {
                storage: child_storage,
                address: 0x200,
            },
        )];
        assert_eq!(resolve(&reg, "child").unwrap().tables, expect);
        assert_eq!(resolve(&reg, "grandchild").unwrap().tables, expect);
        assert_eq!(
            resolve(&reg, "mid").unwrap().tables[0].1.storage,
            base_storage
        );
    }
}

/// MF-07: omitting one table in the middle of the list, two levels down,
/// removes only that table; every later table keeps its address. Legacy's
/// `filterFoundRomTables` returned at the first omitted table
/// (`xml/TableScaleUnmarshaller.java:330-333`).
#[test]
fn mf07_omit_in_the_middle_through_a_chain_keeps_later_items() {
    let mut base = with_id(1);
    for key in 1..=5u8 {
        base.tables
            .patch(key, table(Some(Storage::U8), Some(u32::from(key) * 0x10)));
    }
    let mut mid = Rom::default();
    mid.tables.omit(3);
    let mut child = Rom::default();
    child.tables.patch(4, table(None, Some(0x444)));
    child.tables.patch(6, table(Some(Storage::I8), Some(0x60)));
    let reg = registry([
        doc("base", base),
        doc("mid", mid).with_base("base"),
        doc("child", child).with_base("mid"),
    ]);

    let tables: Vec<(u8, u32)> = resolve(&reg, "child")
        .unwrap()
        .tables
        .into_iter()
        .map(|(k, t)| (k, t.address))
        .collect();
    assert_eq!(
        tables,
        [(1, 0x10), (2, 0x20), (4, 0x444), (5, 0x50), (6, 0x60)]
    );
    let l = lineage(&reg, "child").unwrap();
    assert!(l.merged().tables.get(&3).is_some_and(Nested::is_omitted));
}

/// EC-04 omit-then-override: a table omitted by a base and brought back by
/// a child starts from nothing, so a field only the omitted table had is
/// missing, and reported, not silently inherited.
#[test]
fn omit_then_override_restarts_the_item_and_reports_what_is_missing() {
    let mut base = with_id(1);
    base.tables.patch(2, table(Some(Storage::U8), Some(0x20)));
    let mut mid = Rom::default();
    mid.tables.omit(2);
    let mut child = Rom::default();
    child.tables.patch(2, table(Some(Storage::I16), None));
    let reg = registry([
        doc("base", base),
        doc("mid", mid).with_base("base"),
        doc("child", child).with_base("mid"),
    ]);
    let err = resolve(&reg, "child").unwrap_err();
    let mut missing = Missing::new();
    missing.push("tables[2].address");
    assert_eq!(
        err,
        ResolveError::Incomplete {
            document: name("child"),
            layer: layer("child"),
            missing,
        }
    );
    assert_eq!(
        err.to_string(),
        "`child` is incomplete: missing required field(s) `tables[2].address`"
    );
}

/// Append and replace through a chain: a replace drops what is inherited
/// and later appends follow it.
#[test]
fn appended_lists_follow_the_chain() {
    let mid = Rom {
        notes: Appended::replace(vec![20, 21]),
        ..Rom::default()
    };
    let reg = registry([
        doc(
            "base",
            Rom {
                notes: Appended::append(vec![1, 2]),
                ..with_id(1)
            },
        ),
        doc("mid", mid).with_base("base"),
        doc("top", with_notes(&[30])).with_base("mid"),
        doc("again", with_notes(&[40])).with_base("top"),
    ]);
    assert_eq!(resolve(&reg, "top").unwrap().notes, [20, 21, 30]);
    assert_eq!(resolve(&reg, "again").unwrap().notes, [20, 21, 30, 40]);
}

// ---- several bases ----

/// EC-04 diamond: `d` extends `b` and `c`, which both extend `a`. `a` is
/// applied once, below both, so its appended items are not repeated and
/// `c` does not undo `b`'s override of it.
#[test]
fn a_diamond_applies_the_shared_ancestor_once() {
    let mut a = with_id(1);
    a.notes = Appended::append(vec![1]);
    a.tables.patch(1, table(Some(Storage::U8), Some(0x10)));
    let mut b = with_notes(&[2]);
    b.tables.patch(1, table(Some(Storage::I16), None));
    let c = with_notes(&[3]);
    let reg = registry([
        doc("d", with_notes(&[4])).with_base("b").with_base("c"),
        doc("c", c).with_base("a"),
        doc("b", b).with_base("a"),
        doc("a", a),
    ]);
    let l = lineage(&reg, "d").unwrap();
    assert_eq!(order(&l), ["a", "b", "c", "d"]);
    let d = resolve(&reg, "d").unwrap();
    assert_eq!(d.notes, [1, 2, 3, 4]);
    assert_eq!(
        d.tables,
        [(
            1,
            TableDef {
                storage: Storage::I16,
                address: 0x10
            }
        )]
    );
}

/// Bases are listed weakest first: the later one wins.
#[test]
fn the_later_base_wins() {
    let reg = registry([
        doc("b", with_id(2)),
        doc("c", with_id(3)),
        doc("bc", Rom::default()).with_base("b").with_base("c"),
        doc("cb", Rom::default()).with_base("c").with_base("b"),
    ]);
    assert_eq!(resolve(&reg, "bc").unwrap().id, 3);
    assert_eq!(resolve(&reg, "cb").unwrap().id, 2);
    assert_eq!(order(&lineage(&reg, "bc").unwrap()), ["b", "c", "bc"]);
    assert_eq!(order(&lineage(&reg, "cb").unwrap()), ["c", "b", "cb"]);
}

/// C3 is monotonic: inheriting `b` keeps `b`'s own order of `a1` and `a2`
/// even though another base reaches `a2` first. A depth-first walk keeping
/// first positions would put `a1` above `a2` here.
#[test]
fn every_base_keeps_its_own_order_in_a_child() {
    let reg = registry([
        doc("a1", with_id(1)),
        doc("a2", with_id(2)),
        doc("x", with_notes(&[9])).with_base("a2"),
        doc("b", Rom::default()).with_base("a1").with_base("a2"),
        doc("d", Rom::default()).with_base("x").with_base("b"),
    ]);
    assert_eq!(order(&lineage(&reg, "b").unwrap()), ["a1", "a2", "b"]);
    assert_eq!(
        order(&lineage(&reg, "d").unwrap()),
        ["a1", "a2", "x", "b", "d"]
    );
    assert_eq!(resolve(&reg, "b").unwrap().id, 2);
    assert_eq!(resolve(&reg, "d").unwrap().id, 2);
    assert_eq!(resolve(&reg, "d").unwrap().notes, [9]);
}

/// Bases whose own lineages order two documents both ways cannot be
/// linearised: an error naming the conflict, not an arbitrary order.
#[test]
fn contradictory_bases_are_inconsistent() {
    let reg = registry([
        doc("a", with_id(1)),
        doc("b", with_id(2)),
        doc("x", Rom::default()).with_base("a").with_base("b"),
        doc("y", Rom::default()).with_base("b").with_base("a"),
        doc("d", Rom::default()).with_base("x").with_base("y"),
    ]);
    let err = lineage(&reg, "d").unwrap_err();
    assert_eq!(
        err,
        ResolveError::Inconsistent {
            document: name("d"),
            layer: layer("d"),
            conflict: vec![name("a"), name("b")],
        }
    );
    assert_eq!(
        err.to_string(),
        "the bases of `d` cannot be ordered: their own bases order a, b both ways"
    );
    assert_eq!(err.provenance(), Some(layer("d").provenance()));
    // Each base on its own is fine.
    assert_eq!(order(&lineage(&reg, "x").unwrap()), ["a", "b", "x"]);
    assert_eq!(order(&lineage(&reg, "y").unwrap()), ["b", "a", "y"]);
}

/// A base listed twice would be both weaker and stronger than itself.
#[test]
fn a_base_listed_twice_is_inconsistent() {
    let reg = registry([
        doc("a", with_id(1)),
        doc("d", Rom::default()).with_base("a").with_base("a"),
    ]);
    assert_eq!(
        lineage(&reg, "d").unwrap_err(),
        ResolveError::Inconsistent {
            document: name("d"),
            layer: layer("d"),
            conflict: vec![name("a")],
        }
    );
}

/// A base reached both directly and through another base is fine: the
/// direct, weaker listing comes first.
#[test]
fn a_base_reached_twice_is_applied_once() {
    let reg = registry([
        doc("a", with_id(1)),
        doc("b", with_notes(&[2])).with_base("a"),
        doc("d", with_notes(&[3])).with_base("a").with_base("b"),
    ]);
    assert_eq!(order(&lineage(&reg, "d").unwrap()), ["a", "b", "d"]);
    assert_eq!(resolve(&reg, "d").unwrap().notes, [2, 3]);
}

// ---- errors ----

#[test]
fn unknown_names_and_bases_are_errors() {
    let reg = registry([doc("a", with_id(1)).with_base("ghost")]);
    let err = lineage(&reg, "nope").unwrap_err();
    assert_eq!(err, ResolveError::Unknown { name: name("nope") });
    assert_eq!(err.to_string(), "no document named `nope`");
    assert_eq!(err.provenance(), None);

    let err = lineage(&reg, "a").unwrap_err();
    assert_eq!(
        err,
        ResolveError::UnknownBase {
            document: name("a"),
            layer: layer("a"),
            base: "ghost".into(),
            hidden: Box::default(),
        }
    );
    assert_eq!(
        err.to_string(),
        "`a` names base `ghost`, which is not defined"
    );
    assert_eq!(err.provenance(), Some(layer("a").provenance()));
}

/// MF-17: a cycle is an error naming every document on it, in base order,
/// from wherever the walk enters it.
#[test]
fn mf17_cycles_name_the_full_path() {
    let reg = registry([
        doc("self", Rom::default()).with_base("self"),
        doc("a", Rom::default()).with_base("b"),
        doc("b", Rom::default()).with_base("c"),
        doc("c", Rom::default()).with_base("a"),
        doc("root", Rom::default()).with_base("ok").with_base("a"),
        doc("ok", with_id(1)),
    ]);
    let cycle = |id: &str| match lineage(&reg, id) {
        Err(ResolveError::Cycle {
            path,
            layers,
            layer,
        }) => {
            let ids: Vec<String> = layers.iter().map(|l| l.id().to_string()).collect();
            assert_eq!(
                ids,
                path[..path.len() - 1]
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            );
            assert_eq!(layers.last(), Some(&layer));
            (
                path.iter().map(ToString::to_string).collect::<Vec<_>>(),
                layer.id().to_string(),
            )
        }
        other => panic!("{id}: expected a cycle, got {other:?}"),
    };
    // Its own id as its base is not a loop: it names an outer definition,
    // and a global document has none.
    assert!(matches!(
        lineage(&reg, "self"),
        Err(ResolveError::UnknownBase { base, .. }) if base.as_str() == "self"
    ));
    assert_eq!(cycle("a").0, ["a", "b", "c", "a"]);
    assert_eq!(cycle("a").1, "c");
    assert_eq!(cycle("b").0, ["b", "c", "a", "b"]);
    // Entered from outside: the path is the cycle alone.
    assert_eq!(cycle("root").0, ["a", "b", "c", "a"]);

    let err = lineage(&reg, "c").unwrap_err();
    assert_eq!(
        err.to_string(),
        "inheritance cycle: c (c.xml:2:1) -> a (a.xml:2:1) -> b (b.xml:2:1) -> c"
    );
    assert_eq!(err.provenance(), Some(layer("b").provenance()));
    // Documents outside the cycle still resolve.
    assert_eq!(resolve(&reg, "ok").unwrap().id, 1);
}

/// MF-17: the walk is iterative, so a cycle 100 000 documents long is an
/// error and a chain that long is refused at MAX_LINEAGE, neither of them
/// a stack overflow.
#[test]
fn mf17_deep_chains_and_long_cycles_do_not_overflow() {
    const N: usize = 100_000;
    let chain = |closed: bool| {
        registry((0..N).map(|i| {
            let d = doc(&format!("d{i}"), with_id(1));
            match i {
                0 if closed => d.with_base(format!("d{}", N - 1).as_str()),
                0 => d,
                _ => d.with_base(format!("d{}", i - 1).as_str()),
            }
        }))
    };
    let top = format!("d{}", N - 1);

    // Every document is in a file of its own: the walk crosses N files.
    let request = Provenance::new("rom.bin", Span::byte_offset(0));
    let err = lineage(&chain(true), &top).unwrap_err();
    match &err {
        ResolveError::Cycle { path, layers, .. } => {
            assert_eq!(path.len(), N + 1);
            assert_eq!(path.first(), path.last());
            assert_eq!(layers.len(), N);
            let files: std::collections::BTreeSet<&str> =
                layers.iter().map(Layer::source).collect();
            assert_eq!(files.len(), N);
        }
        other => panic!("expected a cycle, got {other:?}"),
    }
    // The diagnostic names both ends of the cycle and counts the rest,
    // bounded however long the cycle is.
    let diag = err.to_diagnostic(request.clone());
    assert_eq!(diag.code(), codes::INHERITANCE_CYCLE);
    assert!(diag.is_error());
    assert_eq!(diag.provenance(), &layer("d0").provenance());
    let elided = N - 2 * (MAX_LINEAGE / 2);
    assert!(
        diag.message().starts_with(&format!(
            "inheritance cycle: d{} (d{}.xml:2:1) -> d{} (d{}.xml:2:1) -> ",
            N - 1,
            N - 1,
            N - 2,
            N - 2
        )),
        "{}",
        &diag.message()[..200]
    );
    assert!(
        diag.message()
            .contains(&format!(" -> ... {elided} more ... -> "))
    );
    assert!(
        diag.message()
            .ends_with(&format!("-> d0 (d0.xml:2:1) -> d{}", N - 1))
    );
    assert!(diag.message().len() < 64 * MAX_LINEAGE);

    let open = chain(false);
    let err = lineage(&open, &top).unwrap_err();
    assert_eq!(
        err,
        ResolveError::TooLarge {
            document: name(&format!("d{MAX_LINEAGE}")),
            layer: layer(&format!("d{MAX_LINEAGE}")),
        }
    );
    let diag = err.to_diagnostic(request);
    assert_eq!(diag.code(), codes::LINEAGE_TOO_LARGE);
    assert_eq!(
        diag.provenance(),
        &layer(&format!("d{MAX_LINEAGE}")).provenance()
    );
    let mut resolver = open.resolver();
    let longest = resolver
        .lineage(&name(&format!("d{}", MAX_LINEAGE - 1)))
        .unwrap();
    assert_eq!(longest.names().len(), MAX_LINEAGE);
}

#[test]
fn too_large_names_the_limit() {
    let reg = registry(
        (0..=MAX_LINEAGE)
            .map(|i| match i {
                0 => doc("d0", with_id(1)),
                _ => {
                    doc(&format!("d{i}"), Rom::default()).with_base(format!("d{}", i - 1).as_str())
                }
            })
            .collect::<Vec<_>>(),
    );
    let err = lineage(&reg, &format!("d{MAX_LINEAGE}")).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!("`d{MAX_LINEAGE}` inherits from more than 255 documents")
    );
    assert_eq!(
        err.provenance(),
        Some(layer(&format!("d{MAX_LINEAGE}")).provenance())
    );
    assert_eq!(MAX_LINEAGE, 256);
}

/// Completion reports every missing required field, never panics.
#[test]
fn an_incomplete_document_reports_every_missing_field() {
    let mut rom = Rom::default();
    rom.tables.patch(1, table(None, Some(1)));
    rom.tables.patch(2, table(Some(Storage::U8), None));
    rom.tables.omit(3);
    let reg = registry([doc("a", rom)]);
    match resolve(&reg, "a") {
        Err(ResolveError::Incomplete {
            document,
            layer: l,
            missing,
        }) => {
            assert_eq!(document, name("a"));
            assert_eq!(l, layer("a"));
            assert_eq!(
                missing.fields(),
                ["id", "tables[1].storage", "tables[2].address"]
            );
        }
        other => panic!("expected incomplete, got {other:?}"),
    }
    assert_eq!(
        resolve(&reg, "a").unwrap_err().provenance(),
        Some(layer("a").provenance())
    );
}

// ---- sharing and memoisation ----

/// Every lineage holds the registered document's `Arc`, never a copy, and
/// a resolver computes each lineage once.
#[test]
fn bases_are_shared_never_copied() {
    let base = Arc::new(with_id(1));
    let reg = registry([
        Document::new(layer("base"), Arc::clone(&base)),
        doc("c1", with_notes(&[1])).with_base("base"),
        doc("c2", with_notes(&[2])).with_base("base"),
    ]);
    let mut resolver = reg.resolver();
    let first = resolver.lineage(&name("base")).unwrap();
    for child in ["c1", "c2"] {
        let l = resolver.lineage(&name(child)).unwrap();
        let (_, shared) = l.stack().layers().next().unwrap();
        assert!(Arc::ptr_eq(shared, &base), "{child}");
    }
    let again = resolver.lineage(&name("base")).unwrap();
    assert!(Arc::ptr_eq(&first, &again));
    let r = resolver.resolve(&name("c1")).unwrap();
    assert!(Arc::ptr_eq(
        r.lineage(),
        &resolver.lineage(&name("c1")).unwrap()
    ));
    assert_eq!(r.value().notes, [1]);
    assert_eq!(r.clone().into_value(), *r.value());
}

/// A memoised base is reused, not re-walked: a resolver that has already
/// resolved the base gives the same result as a fresh one.
#[test]
fn memoised_and_fresh_resolution_agree() {
    let reg = registry([
        doc("a", with_id(1)),
        doc("b", with_notes(&[2])).with_base("a"),
        doc("c", with_notes(&[3])).with_base("a").with_base("b"),
    ]);
    let mut warm = reg.resolver();
    warm.lineage(&name("b")).unwrap();
    warm.lineage(&name("a")).unwrap();
    let warm_c = warm.lineage(&name("c")).unwrap();
    assert_eq!(*warm_c, *lineage(&reg, "c").unwrap());
    assert_eq!(*Resolver::new(&reg).lineage(&name("c")).unwrap(), *warm_c);
}

// ---- determinism and idempotence ----

fn family() -> Vec<Document<Rom>> {
    let mut a = with_id(1);
    a.tables.patch(1, table(Some(Storage::U8), Some(0x10)));
    a.tables.patch(2, table(Some(Storage::U8), Some(0x20)));
    let mut b = with_notes(&[2]);
    b.tables.omit(1);
    let mut c = with_notes(&[3]);
    c.tables.patch(2, table(Some(Storage::I8), None));
    vec![
        doc("a", a),
        doc("b", b).with_base("a"),
        doc("c", c).with_base("a"),
        doc("d", with_id(4)).with_base("b").with_base("c"),
        doc("e", with_notes(&[5])).with_base("d"),
    ]
}

#[test]
fn resolution_does_not_depend_on_insertion_order() {
    let forward = registry(family());
    let backward = registry(family().into_iter().rev());
    assert_eq!(forward, backward);
    for id in ["a", "b", "c", "d", "e"] {
        assert_eq!(
            *lineage(&forward, id).unwrap(),
            *lineage(&backward, id).unwrap()
        );
        assert_eq!(resolve(&forward, id), resolve(&backward, id));
    }
    // And resolving twice gives the same result.
    let mut resolver = forward.resolver();
    let once = resolver.resolve(&name("e")).unwrap();
    assert_eq!(resolver.resolve(&name("e")).unwrap(), once);
}

/// Resolving an already-resolved document again, as one layer or under an
/// empty child, gives it back (the identity law).
#[test]
fn re_layering_a_resolved_document_is_the_identity() {
    let mut docs = family();
    let reg = registry(family());
    let merged = Rom::clone(lineage(&reg, "e").unwrap().merged());
    docs.push(doc("flat", merged.clone()));
    docs.push(doc("over", Rom::default()).with_base("e"));
    let reg = registry(docs);
    assert_eq!(**lineage(&reg, "flat").unwrap().merged(), merged);
    assert_eq!(**lineage(&reg, "over").unwrap().merged(), merged);
    assert_eq!(resolve(&reg, "flat"), resolve(&reg, "e"));
    assert_eq!(resolve(&reg, "over"), resolve(&reg, "e"));
    assert_eq!(
        resolve(&reg, "e").unwrap(),
        RomDef {
            id: 4,
            tables: vec![(
                2,
                TableDef {
                    storage: Storage::I8,
                    address: 0x20
                }
            )],
            notes: vec![2, 3, 5],
        }
    );
}

// ---- files (MF-17) and diagnostics ----

/// The layer of document `id` in file `file`, starting at `line`.
fn in_file(id: &str, file: &str, line: u32) -> Layer {
    Layer::new(id.into(), LayerKind::File, file, Span::line_col(line, 1))
}

/// MF-17: a definition in one file resolves on a base in another, which
/// legacy could not (`xml/DOMRomUnmarshaller.java:149-152` looked only in
/// the same file); how the documents are split between files changes
/// nothing but provenance.
#[test]
fn mf17_a_base_in_another_file_resolves() {
    let mut base = with_id(1);
    base.tables.patch(1, table(Some(Storage::U16), Some(0x100)));
    base.tables.patch(2, table(Some(Storage::U8), Some(0x200)));
    let mut child = with_id(2);
    child.tables.patch(1, table(Some(Storage::I16), None));
    let split = |base_file: &str, child_file: &str| {
        registry([
            Document::new(in_file("A2WC522N", child_file, 40), child.clone())
                .with_base("16BITBASE"),
            Document::new(in_file("16BITBASE", base_file, 3), base.clone()),
        ])
    };
    let two = split("defs/16bitbase.xml", "defs/a2wc522n.xml");
    let one = split("defs/all.xml", "defs/all.xml");
    let a = two.resolver().resolve(&name("A2WC522N")).unwrap();
    let b = one.resolver().resolve(&name("A2WC522N")).unwrap();
    let signed = TableDef {
        storage: Storage::I16,
        address: 0x100,
    };
    let unsigned = TableDef {
        storage: Storage::U8,
        address: 0x200,
    };
    assert_eq!(
        a.value(),
        &RomDef {
            id: 2,
            tables: vec![(1, signed), (2, unsigned)],
            notes: vec![],
        }
    );
    assert_eq!(a.value(), b.value());
    assert_eq!(a.lineage().names(), b.lineage().names());
    assert_eq!(a.lineage().merged(), b.lineage().merged());
    let sources: Vec<&str> = a
        .lineage()
        .stack()
        .layers()
        .map(|(l, _)| l.source())
        .collect();
    assert_eq!(sources, ["defs/16bitbase.xml", "defs/a2wc522n.xml"]);
}

/// The same name in two files is an error at the second, naming where the
/// first is; the first stays (legacy took the first match silently,
/// `editor/ecu/OpenImageWorker.java:219-250`).
#[test]
fn a_duplicate_across_files_is_an_error_naming_both() {
    let mut reg = registry([Document::new(in_file("SSM", "defs/a.xml", 3), with_id(1))]);
    let err = reg
        .insert(Document::new(in_file("SSM", "defs/b.xml", 9), with_id(2)))
        .unwrap_err();
    assert_eq!(err.code(), codes::DUPLICATE_DOCUMENT);
    let d = err.to_diagnostic();
    assert_eq!(d.severity(), Severity::Error);
    assert_eq!(d.code(), codes::DUPLICATE_DOCUMENT);
    assert_eq!(
        d.provenance(),
        &in_file("SSM", "defs/b.xml", 9).provenance()
    );
    assert_eq!(
        d.message(),
        "`SSM` is already defined in defs/a.xml:3:1 [layer SSM]; this definition is ignored"
    );
    assert_eq!(resolve(&reg, "SSM").unwrap().id, 1);
}

/// `(code, message, provenance)` of `err` as a diagnostic, which is always an
/// error.
fn reported(err: &ResolveError, request: &Provenance) -> (Code, String, Provenance) {
    let d = err.to_diagnostic(request.clone());
    assert_eq!(d.severity(), Severity::Error);
    assert_eq!(d.code(), err.code());
    assert_eq!(d.message(), err.to_string());
    (d.code(), d.message().to_owned(), d.provenance().clone())
}

#[test]
fn every_error_is_an_error_diagnostic_at_the_document_at_fault() {
    let request = Provenance::new("A2WC522N.bin", Span::byte_offset(0x10));
    let at = |id: &str, file: &str, line| in_file(id, file, line).provenance();
    let reg = registry([
        Document::new(in_file("orphan", "o.xml", 4), with_id(1)).with_base("ghost"),
        Document::new(in_file("loop", "l.xml", 5), Rom::default()).with_base("back"),
        Document::new(in_file("back", "k.xml", 3), Rom::default()).with_base("loop"),
        Document::new(in_file("twice", "t.xml", 6), Rom::default())
            .with_base("bare")
            .with_base("bare"),
        Document::new(in_file("bare", "b.xml", 7), Rom::default()),
    ]);
    let lineage_err = |id: &str| reg.resolver().lineage(&name(id)).unwrap_err();
    assert_eq!(
        reported(&lineage_err("nope"), &request),
        (
            codes::UNKNOWN_DOCUMENT,
            "no document named `nope`".to_owned(),
            request.clone()
        )
    );
    assert_eq!(
        reported(&lineage_err("orphan"), &request),
        (
            codes::UNKNOWN_BASE,
            "`orphan` names base `ghost`, which is not defined".to_owned(),
            at("orphan", "o.xml", 4)
        )
    );
    assert_eq!(
        reported(&lineage_err("loop"), &request),
        (
            codes::INHERITANCE_CYCLE,
            "inheritance cycle: loop (l.xml:5:1) -> back (k.xml:3:1) -> loop".to_owned(),
            at("back", "k.xml", 3)
        )
    );
    assert_eq!(
        reported(&lineage_err("twice"), &request).0,
        codes::INCONSISTENT_BASES
    );
    assert_eq!(
        reported(&lineage_err("twice"), &request).2,
        at("twice", "t.xml", 6)
    );
    let incomplete = reg.resolver().resolve(&name("bare")).unwrap_err();
    assert_eq!(
        reported(&incomplete, &request),
        (
            codes::INCOMPLETE_DOCUMENT,
            "`bare` is incomplete: missing required field(s) `id`".to_owned(),
            at("bare", "b.xml", 7)
        )
    );
    let too_large = ResolveError::TooLarge {
        document: name("deep"),
        layer: in_file("deep", "d.xml", 8),
    };
    assert_eq!(reported(&too_large, &request).0, codes::LINEAGE_TOO_LARGE);
    assert_eq!(reported(&too_large, &request).2, at("deep", "d.xml", 8));
}

/// A ring of `n` documents, `c0` -> `c1` -> ... -> `c0`, each in its own
/// file `f<i>.xml`.
fn ring(n: usize) -> Registry<Rom> {
    registry((0..n).map(|i| {
        let id = format!("c{i}");
        Document::new(in_file(&id, &format!("f{i}.xml"), 1), Rom::default())
            .with_base(format!("c{}", (i + 1) % n).as_str())
    }))
}

/// `c<i> (f<i>.xml:1:1) -> ` for each `i`.
fn ring_text(ids: impl IntoIterator<Item = usize>) -> String {
    ids.into_iter()
        .map(|i| format!("c{i} (f{i}.xml:1:1) -> "))
        .collect()
}

/// A cycle of up to MAX_LINEAGE documents is named in full, in base order,
/// each with its file; a longer one names both ends and counts the rest.
#[test]
fn cycle_messages_name_every_document_up_to_the_limit() {
    let half = MAX_LINEAGE / 2;
    // A ring of one is a document naming its own id: not a cycle.
    assert!(matches!(
        ring(1).resolver().lineage(&name("c0")),
        Err(ResolveError::UnknownBase { .. })
    ));
    for n in [2, 3, MAX_LINEAGE - 1, MAX_LINEAGE] {
        let err = ring(n).resolver().lineage(&name("c0")).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("inheritance cycle: {}c0", ring_text(0..n)),
            "n = {n}"
        );
    }
    for (n, more) in [(MAX_LINEAGE + 1, 1), (MAX_LINEAGE + 3, 3)] {
        let err = ring(n).resolver().lineage(&name("c0")).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "inheritance cycle: {}... {more} more ... -> {}c0",
                ring_text(0..half),
                ring_text(n - half..n)
            ),
            "n = {n}"
        );
    }
}

// ---- registry, documents, names, scopes ----

#[test]
fn a_duplicate_name_keeps_the_first_document() {
    let mut reg = Registry::new();
    assert!(reg.is_empty());
    reg.insert(doc("a", with_id(1))).unwrap();
    let second = Document::new(
        Layer::new(
            "a".into(),
            LayerKind::User,
            "user.xml",
            Span::line_col(9, 3),
        ),
        with_id(2),
    );
    let err = reg.insert(second).unwrap_err();
    assert_eq!(err.name, name("a"));
    assert_eq!(err.kept, layer("a"));
    assert_eq!(err.rejected.source(), "user.xml");
    assert_eq!(err.provenance(), err.rejected.provenance());
    assert_eq!(
        err.to_string(),
        format!(
            "`a` is already defined in {}; this definition is ignored",
            layer("a").provenance()
        )
    );
    assert_eq!(reg.len(), 1);
    assert!(!reg.is_empty());
    assert_eq!(resolve(&reg, "a").unwrap().id, 1);
}

#[test]
fn registry_lists_documents_in_name_order() {
    let reg = registry([doc("b", with_id(2)), doc("a", with_id(1))]);
    let ids: Vec<String> = reg.documents().map(|d| d.name().to_string()).collect();
    assert_eq!(ids, ["a", "b"]);
    assert_eq!(reg.len(), 2);
    assert!(Registry::<Rom>::default().is_empty());
    assert!(reg.get(&name("c")).is_none());
}

#[test]
fn document_accessors() {
    let d = doc("a", with_id(1))
        .with_base("b")
        .with_base(LayerId::new("c"))
        .with_base(String::from("d"))
        .in_scope(Scope::global().child("rom"));
    assert_eq!(d.name(), &Name::new(Scope::global().child("rom"), "a"));
    assert_eq!(d.layer(), &layer("a"));
    let bases: Vec<&str> = d.bases().iter().map(LayerId::as_str).collect();
    assert_eq!(bases, ["b", "c", "d"]);
    assert_eq!(**d.doc(), with_id(1));
}

#[test]
fn scopes_and_names() {
    let global = Scope::global();
    assert!(global.is_global());
    assert_eq!(global.segments().len(), 0);
    let local = global.child("A2WC522N").child("Fuel Map");
    assert!(!local.is_global());
    assert_eq!(
        local.segments().collect::<Vec<_>>(),
        ["A2WC522N", "Fuel Map"]
    );
    assert!(global < local);

    let n = Name::new(local.clone(), "scale");
    assert_eq!(n.scope(), &local);
    assert_eq!(n.id().as_str(), "scale");
    assert_eq!(n.to_string(), "A2WC522N/Fuel Map::scale");
    assert_eq!(Name::new(global.child("x"), "y").to_string(), "x::y");
    assert_eq!(Name::from("scale").to_string(), "scale");
    assert_eq!(Name::from(LayerId::new("scale")), Name::global("scale"));
    assert_ne!(n, Name::global("scale"));
}

// ---- scopes (US-RR-22-10) ----

/// `rom/table` scope paths.
fn scope(path: &[&str]) -> Scope {
    path.iter().fold(Scope::global(), |s, seg| s.child(*seg))
}

fn scoped_doc(path: &[&str], id: &str, rom: Rom) -> Document<Rom> {
    doc(id, rom).in_scope(scope(path))
}

fn scoped_name(path: &[&str], id: &str) -> Name {
    Name::new(scope(path), id)
}

fn lineage_of(reg: &Registry<Rom>, n: &Name) -> Result<Vec<String>, ResolveError> {
    reg.resolver().lineage(n).map(|l| order(&l))
}

#[test]
fn scopes_enclose_their_own_path_only() {
    let global = Scope::global();
    let rom = scope(&["A2WC400H"]);
    let table = scope(&["A2WC400H", "Boost Target"]);
    let sibling = scope(&["A2WC400H", "Fuel Map"]);
    let other = scope(&["A2WC522N", "Boost Target"]);
    assert_eq!((global.depth(), rom.depth(), table.depth()), (0, 1, 2));
    for s in [&global, &rom, &table, &sibling, &other] {
        assert!(global.encloses(s));
        assert!(s.encloses(s));
    }
    assert!(rom.encloses(&table));
    assert!(rom.encloses(&sibling));
    assert!(!table.encloses(&rom));
    assert!(!table.encloses(&sibling));
    assert!(!rom.encloses(&other));
    assert!(!table.encloses(&global));
    assert!(!other.encloses(&table));
    assert_eq!(table.to_string(), "A2WC400H/Boost Target");
    assert_eq!(rom.to_string(), "A2WC400H");
    assert_eq!(global.to_string(), "");
}

/// A reference is looked up from the referrer's scope outward: its own
/// scope first, then each enclosing one, then global. The nearest wins.
#[test]
fn lookup_is_innermost_first_out_to_global() {
    let t = ["A2WC400H", "Boost Target"];
    let reg = registry([
        doc("x", with_id(1)),
        scoped_doc(&t[..1], "x", with_id(2)),
        scoped_doc(&t, "x", with_id(3)),
        doc("g", with_id(4)),
        scoped_doc(&t[..1], "r", with_id(5)),
    ]);
    let found = |from: &[&str], id: &str| {
        reg.lookup(&scope(from), &LayerId::new(id))
            .map(|d| d.name().to_string())
    };
    assert_eq!(found(&t, "x").as_deref(), Some("A2WC400H/Boost Target::x"));
    assert_eq!(found(&t[..1], "x").as_deref(), Some("A2WC400H::x"));
    assert_eq!(found(&[], "x").as_deref(), Some("x"));
    assert_eq!(
        found(&["A2WC400H", "Fuel Map"], "x").as_deref(),
        Some("A2WC400H::x")
    );
    assert_eq!(found(&["A2WC522N"], "x").as_deref(), Some("x"));
    // Inner scopes see outer names, at any depth.
    assert_eq!(found(&t, "g").as_deref(), Some("g"));
    assert_eq!(found(&t, "r").as_deref(), Some("A2WC400H::r"));
    // ... but not the other way round.
    assert_eq!(found(&[], "r"), None);
    assert_eq!(found(&["A2WC522N"], "r"), None);
    assert_eq!(found(&t, "nope"), None);
}

/// A table-local document inherits from a scaling local to its table, its
/// ROM, or global, through a chain that crosses scopes.
#[test]
fn an_inner_document_inherits_from_every_enclosing_scope() {
    let t = ["A2WC400H", "Boost Target"];
    let reg = registry([
        doc("base", with_id(1)),
        scoped_doc(&t[..1], "rom_scale", with_notes(&[1])).with_base("base"),
        scoped_doc(&t, "local", with_notes(&[2])).with_base("rom_scale"),
        scoped_doc(&t, "top", with_notes(&[3]))
            .with_base("base")
            .with_base("local"),
    ]);
    let l = reg.resolver().lineage(&scoped_name(&t, "top")).unwrap();
    assert_eq!(
        order(&l),
        [
            "base",
            "A2WC400H::rom_scale",
            "A2WC400H/Boost Target::local",
            "A2WC400H/Boost Target::top"
        ]
    );
    assert_eq!(l.merged().id, Field::Set(1));
    assert_eq!(l.merged().notes.items(), [1, 2, 3]);
}

/// MF-16: a name defined in a local scope is not visible as a base from
/// the global scope, from the scope enclosing it, or from a sibling scope;
/// the error says where the name is defined. Legacy registered every
/// table-local scaling in the one map `<scalingbase>` lookups use
/// (`xml/TableScaleUnmarshaller.java:276-277`, `:460-461`, looked up at
/// `:419-421`), so a later scaling anywhere in the definition could
/// inherit from it.
#[test]
fn mf16_a_local_name_is_not_a_global_base() {
    let t = ["A2WC400H", "Boost Target"];
    let reg = registry([
        scoped_doc(&t, "scale", with_id(1)),
        doc("global_ref", Rom::default()).with_base("scale"),
        scoped_doc(&t[..1], "rom_ref", Rom::default()).with_base("scale"),
        scoped_doc(&["A2WC400H", "Fuel Map"], "sibling_ref", Rom::default()).with_base("scale"),
        scoped_doc(&t, "inner_ref", Rom::default()).with_base("scale"),
    ]);
    let hidden = vec![scoped_name(&t, "scale")];
    for (from, id) in [
        (&[][..], "global_ref"),
        (&t[..1], "rom_ref"),
        (&["A2WC400H", "Fuel Map"][..], "sibling_ref"),
    ] {
        let document = scoped_name(from, id);
        let err = reg.resolver().lineage(&document).unwrap_err();
        assert_eq!(
            err,
            ResolveError::UnknownBase {
                document: document.clone(),
                layer: layer(id),
                base: "scale".into(),
                hidden: hidden.clone().into(),
            }
        );
        assert_eq!(
            err.to_string(),
            format!(
                "`{document}` names base `scale`, which is not defined here: `scale` is \
                 defined only inside `A2WC400H/Boost Target`; not visible from `{document}`"
            )
        );
        assert_eq!(err.code(), codes::UNKNOWN_BASE);
    }
    // Inside the scope it is found, and the global name never is.
    assert_eq!(
        resolve_name(&reg, &scoped_name(&t, "inner_ref"))
            .unwrap()
            .id,
        1
    );
    assert_eq!(
        lineage(&reg, "scale").unwrap_err(),
        ResolveError::Unknown {
            name: name("scale")
        }
    );
    assert!(reg.get(&name("scale")).is_none());
    assert!(reg.get(&scoped_name(&t, "scale")).is_some());
}

fn resolve_name(reg: &Registry<Rom>, n: &Name) -> Result<RomDef, ResolveError> {
    reg.resolver().resolve(n).map(Resolved::into_value)
}

/// An id in many scopes the referrer cannot see: the message names the
/// first few, in name order, and counts the rest.
#[test]
fn an_unknown_base_names_where_it_is_hidden_boundedly() {
    let tables = ["a", "b", "c", "d", "e"];
    let mut docs: Vec<Document<Rom>> = tables
        .iter()
        .map(|t| scoped_doc(&["rom", t], "scale", with_id(1)))
        .collect();
    docs.push(scoped_doc(&["rom"], "ref", Rom::default()).with_base("scale"));
    let reg = registry(docs);
    let err = lineage_of(&reg, &scoped_name(&["rom"], "ref")).unwrap_err();
    let ResolveError::UnknownBase { hidden, .. } = &err else {
        panic!("{err:?}");
    };
    let expect: Vec<Name> = tables
        .iter()
        .map(|t| scoped_name(&["rom", t], "scale"))
        .collect();
    assert_eq!(&**hidden, &expect[..]);
    assert_eq!(
        err.to_string(),
        "`rom::ref` names base `scale`, which is not defined here: `scale` is defined only \
         inside `rom/a`, `rom/b`, `rom/c` and 2 more scopes; not visible from `rom::ref`"
    );

    let three = registry(
        tables[..3]
            .iter()
            .map(|t| scoped_doc(&["rom", t], "scale", with_id(1)))
            .chain([doc("ref", Rom::default()).with_base("scale")]),
    );
    assert_eq!(
        lineage(&three, "ref").unwrap_err().to_string(),
        "`ref` names base `scale`, which is not defined here: `scale` is defined only \
         inside `rom/a`, `rom/b`, `rom/c`; not visible from `ref`"
    );
}

/// A local document with an outer document's id shadows it inside its
/// scope only; the registry lists it, as a warning naming both. The same
/// id in the same scope is still a duplicate.
#[test]
fn shadowing_is_scoped_and_warned() {
    let t = ["A2WC400H", "Boost Target"];
    let reg = registry([
        doc("x", with_id(1)),
        scoped_doc(&t[..1], "x", with_id(2)),
        scoped_doc(&t, "x", with_id(3)),
        scoped_doc(&["A2WC400H", "Fuel Map"], "y", with_id(9)),
        doc("y", with_id(8)).in_scope(scope(&["other"])),
        doc("outer", Rom::default()).with_base("x"),
        scoped_doc(&t[..1], "mid", Rom::default()).with_base("x"),
        scoped_doc(&t, "inner", Rom::default()).with_base("x"),
        scoped_doc(&["A2WC400H", "Fuel Map"], "beside", Rom::default()).with_base("x"),
    ]);
    let id = |n: &Name| resolve_name(&reg, n).unwrap().id;
    assert_eq!(id(&name("outer")), 1);
    assert_eq!(id(&scoped_name(&t[..1], "mid")), 2);
    assert_eq!(id(&scoped_name(&t, "inner")), 3);
    assert_eq!(id(&scoped_name(&["A2WC400H", "Fuel Map"], "beside")), 2);

    let shadowed = reg.shadowed();
    let pairs: Vec<(String, String)> = shadowed
        .iter()
        .map(|s| (s.name.to_string(), s.shadows.to_string()))
        .collect();
    // Each against the nearest; `y` in two unrelated scopes shadows
    // nothing.
    assert_eq!(
        pairs,
        [
            ("A2WC400H::x".to_owned(), "x".to_owned()),
            (
                "A2WC400H/Boost Target::x".to_owned(),
                "A2WC400H::x".to_owned()
            ),
        ]
    );
    let s = &shadowed[1];
    assert_eq!(s.layer, layer("x"));
    assert_eq!(s.outer, layer("x"));
    assert_eq!(s.provenance(), layer("x").provenance());
    assert_eq!(s.code(), codes::SHADOWED_NAME);
    assert_eq!(
        s.to_string(),
        format!(
            "`A2WC400H/Boost Target::x` shadows `A2WC400H::x` ({}): inside \
             `A2WC400H/Boost Target`, `x` means this definition",
            layer("x").provenance()
        )
    );
    let diag = s.to_diagnostic();
    assert_eq!(diag.severity(), Severity::Warning);
    assert_eq!(diag.code(), codes::SHADOWED_NAME);
    assert_eq!(diag.message(), s.to_string());
    assert_eq!(diag.provenance(), &s.provenance());

    // No shadowing at all: nothing listed.
    assert!(
        registry([doc("a", with_id(1)), doc("b", with_id(1))])
            .shadowed()
            .is_empty()
    );
    // The same name in the same scope is LAY0003, not shadowing.
    let mut dup = registry([scoped_doc(&t, "x", with_id(1))]);
    let err = dup.insert(scoped_doc(&t, "x", with_id(2))).unwrap_err();
    assert_eq!(err.name, scoped_name(&t, "x"));
    assert_eq!(dup.len(), 1);
    assert!(dup.shadowed().is_empty());
}

/// Shadowing distinguishes the shadowed document by its layer: the warning
/// carries both locations.
#[test]
fn a_shadow_names_both_locations() {
    let global = Document::new(in_file("scale", "base.xml", 3), with_id(1));
    let local =
        Document::new(in_file("scale", "rom.xml", 40), with_id(2)).in_scope(scope(&["rom"]));
    let reg = registry([local, global]);
    let shadowed = reg.shadowed();
    assert_eq!(shadowed.len(), 1);
    assert_eq!(shadowed[0].layer, in_file("scale", "rom.xml", 40));
    assert_eq!(shadowed[0].outer, in_file("scale", "base.xml", 3));
    assert_eq!(shadowed[0].to_diagnostic().provenance().source(), "rom.xml");
    assert!(shadowed[0].to_string().contains("base.xml:3:1"));
}

/// A base with the document's own id extends the outer definition of
/// that id, at every level: table -> ROM -> global, and table -> global
/// where the ROM has none. Legacy looked a scaling's `base` up before
/// registering the scaling (`xml/TableScaleUnmarshaller.java:415-421`,
/// `:460-461`) and a table's `base` among the tables the ROM already had
/// (`:87-92`, `xml/DOMRomUnmarshaller.java:168-176`), so the own name found
/// the earlier definition.
#[test]
fn a_base_of_its_own_id_extends_the_outer_definition() {
    let t = ["A2WC400H", "Boost Target"];
    let bare = ["A2WC522N", "Fuel Map"];
    let x = Rom {
        notes: Appended::append(vec![1]),
        ..with_id(1)
    };
    let reg = registry([
        doc("x", x),
        doc("g", with_notes(&[0])),
        scoped_doc(&t[..1], "x", with_notes(&[2])).with_base("x"),
        scoped_doc(&t, "x", with_notes(&[3])).with_base("x"),
        scoped_doc(&bare, "x", with_notes(&[4]))
            .with_base("g")
            .with_base("x"),
        scoped_doc(&t, "user", with_notes(&[5])).with_base("x"),
        scoped_doc(&t[..1], "rom_user", with_notes(&[6])).with_base("x"),
        doc("global_user", with_notes(&[7])).with_base("x"),
    ]);
    let got = |path: &[&str], id: &str| lineage_of(&reg, &scoped_name(path, id)).unwrap();
    assert_eq!(got(&t[..1], "x"), ["x", "A2WC400H::x"]);
    assert_eq!(
        got(&t, "x"),
        ["x", "A2WC400H::x", "A2WC400H/Boost Target::x"]
    );
    assert_eq!(got(&bare, "x"), ["g", "x", "A2WC522N/Fuel Map::x"]);
    // Every other reference to the id finds the innermost visible one,
    // which holds the outer ones.
    assert_eq!(
        got(&t, "user"),
        [
            "x",
            "A2WC400H::x",
            "A2WC400H/Boost Target::x",
            "A2WC400H/Boost Target::user"
        ]
    );
    assert_eq!(
        got(&t[..1], "rom_user"),
        ["x", "A2WC400H::x", "A2WC400H::rom_user"]
    );
    assert_eq!(got(&[], "global_user"), ["x", "global_user"]);
    let user = resolve_name(&reg, &scoped_name(&t, "user")).unwrap();
    assert_eq!((user.id, user.notes), (1, vec![1, 2, 3, 5]));

    // The lookup itself: the own id skips the referrer, nothing else does.
    let base = |referrer: &Name, id: &str| {
        reg.base_of(referrer, &LayerId::new(id))
            .map(|d| d.name().to_string())
    };
    assert_eq!(
        base(&scoped_name(&t, "x"), "x").as_deref(),
        Some("A2WC400H::x")
    );
    assert_eq!(base(&scoped_name(&t[..1], "x"), "x").as_deref(), Some("x"));
    assert_eq!(base(&scoped_name(&bare, "x"), "x").as_deref(), Some("x"));
    assert_eq!(base(&name("x"), "x"), None);
    assert_eq!(
        base(&scoped_name(&t, "user"), "x").as_deref(),
        Some("A2WC400H/Boost Target::x")
    );
    assert_eq!(
        base(&scoped_name(&t, "x"), "user").as_deref(),
        Some("A2WC400H/Boost Target::user")
    );
    assert_eq!(base(&scoped_name(&t, "x"), "g").as_deref(), Some("g"));
    assert_eq!(base(&name("g"), "x").as_deref(), Some("x"));

    // Extending is not shadowing: no warning for any of them.
    assert!(reg.shadowed().is_empty(), "{:?}", reg.shadowed());
}

/// LAY0008 is for shadowing that does not extend: a local document that
/// hides an outer one without naming its id is warned about, beside one
/// that extends it, which is not; one naming its own id among other bases
/// still extends.
#[test]
fn only_unrelated_shadowing_is_warned() {
    let reg = registry([
        doc("x", with_id(1)),
        doc("y", with_id(2)),
        doc("g", with_id(3)),
        scoped_doc(&["rom"], "x", with_id(4)).with_base("g"),
        scoped_doc(&["rom"], "y", with_id(5))
            .with_base("y")
            .with_base("g"),
        scoped_doc(&["rom", "t"], "x", with_id(6)),
        scoped_doc(&["rom", "t"], "y", with_id(7)).with_base("y"),
    ]);
    let pairs: Vec<(String, String)> = reg
        .shadowed()
        .iter()
        .map(|s| (s.name.to_string(), s.shadows.to_string()))
        .collect();
    assert_eq!(
        pairs,
        [
            ("rom::x".to_owned(), "x".to_owned()),
            ("rom/t::x".to_owned(), "rom::x".to_owned()),
        ]
    );
    assert_eq!(
        lineage_of(&reg, &scoped_name(&["rom", "t"], "y")).unwrap(),
        ["y", "g", "rom::y", "rom/t::y"]
    );
}

/// A base with the document's own id and no outer definition is an
/// unknown base, never a cycle: a global document has nothing to extend,
/// and a local one sees only enclosing scopes. (Legacy recursed on a ROM
/// whose `base` was its own `xmlid` until the stack overflowed,
/// `xml/DOMRomUnmarshaller.java:149-152`, `:201-210`.)
#[test]
fn a_base_of_its_own_id_with_no_outer_definition_is_unknown() {
    let reg = registry([
        doc("self", with_id(1)).with_base("self"),
        scoped_doc(&["rom"], "x", with_id(1)).with_base("x"),
        scoped_doc(&["other"], "x", with_id(2)),
        scoped_doc(&["rom", "t"], "x", with_id(3)).with_base("x"),
    ]);
    let err = lineage(&reg, "self").unwrap_err();
    assert_eq!(
        err,
        ResolveError::UnknownBase {
            document: name("self"),
            layer: layer("self"),
            base: "self".into(),
            hidden: Box::default(),
        }
    );
    assert_eq!(
        err.to_string(),
        "`self` names its own id `self` as its base, which means the `self` of an \
         enclosing scope, and none defines one"
    );
    assert_eq!(err.code(), codes::UNKNOWN_BASE);
    assert_eq!(err.provenance(), Some(layer("self").provenance()));

    let rom_x = scoped_name(&["rom"], "x");
    let err = lineage_of(&reg, &rom_x).unwrap_err();
    assert_eq!(
        err,
        ResolveError::UnknownBase {
            document: rom_x.clone(),
            layer: layer("x"),
            base: "x".into(),
            hidden: vec![
                scoped_name(&["other"], "x"),
                scoped_name(&["rom", "t"], "x")
            ]
            .into(),
        }
    );
    assert_eq!(
        err.to_string(),
        "`rom::x` names its own id `x` as its base, which means the `x` of an enclosing \
         scope, and none defines one here: `x` is defined only inside `other`, `rom/t`; not visible \
         from `rom::x`"
    );
    // The table's `x` extends the ROM's, whose own base is the fault.
    assert_eq!(lineage_of(&reg, &scoped_name(&["rom", "t"], "x")), Err(err));
}

/// A loop is still a cycle when it is reached through an outer definition
/// a local one extends: reported with the loop's own path, which lies in
/// one scope (the extending document is not on it).
#[test]
fn a_cycle_through_an_outer_definition_is_still_a_cycle() {
    let t = ["rom", "t"];
    let reg = registry([
        doc("x", Rom::default()).with_base("a"),
        doc("a", Rom::default()).with_base("x"),
        scoped_doc(&["other"], "x", Rom::default()).with_base("x"),
        scoped_doc(&t[..1], "x", Rom::default()).with_base("a"),
        scoped_doc(&t[..1], "a", Rom::default()).with_base("x"),
        scoped_doc(&t, "x", Rom::default()).with_base("x"),
    ]);
    let cycle = |n: &Name| match lineage_of(&reg, n) {
        Err(ResolveError::Cycle {
            path,
            layers,
            layer,
        }) => {
            assert_eq!(layers.len() + 1, path.len());
            assert_eq!(layers.last(), Some(&layer));
            path.iter().map(ToString::to_string).collect::<Vec<_>>()
        }
        other => panic!("{n}: expected a cycle, got {other:?}"),
    };
    assert_eq!(cycle(&scoped_name(&["other"], "x")), ["x", "a", "x"]);
    assert_eq!(cycle(&scoped_name(&t, "x")), ["rom::x", "rom::a", "rom::x"]);
    assert_eq!(
        cycle(&scoped_name(&t[..1], "a")),
        ["rom::a", "rom::x", "rom::a"]
    );
    assert_eq!(
        lineage_of(&reg, &scoped_name(&t, "x"))
            .unwrap_err()
            .to_string(),
        "inheritance cycle: rom::x (x.xml:2:1) -> rom::a (a.xml:2:1) -> rom::x"
    );
}

/// Names are case-sensitive: the resolver never folds case (readers of
/// case-insensitive formats fold before building names).
#[test]
fn names_are_case_sensitive() {
    let reg = registry([
        doc("Scale", with_id(1)),
        doc("scale", with_id(2)),
        doc("ref", Rom::default()).with_base("SCALE"),
    ]);
    assert_eq!(reg.len(), 3);
    assert_eq!(resolve(&reg, "Scale").unwrap().id, 1);
    assert_eq!(resolve(&reg, "scale").unwrap().id, 2);
    assert!(
        matches!(lineage(&reg, "ref"), Err(ResolveError::UnknownBase { hidden, .. }) if hidden.is_empty())
    );
    assert!(reg.shadowed().is_empty());
}

/// The registry lists documents by id, then scope; `get` is exact.
#[test]
fn registry_lists_scoped_documents_by_id_then_scope() {
    let reg = registry([
        scoped_doc(&["rom"], "a", with_id(1)),
        doc("b", with_id(2)),
        doc("a", with_id(3)),
        scoped_doc(&["rom", "t"], "a", with_id(4)),
    ]);
    let names: Vec<String> = reg.documents().map(|d| d.name().to_string()).collect();
    assert_eq!(names, ["a", "rom::a", "rom/t::a", "b"]);
    assert_eq!(reg.len(), 4);
    assert!(reg.get(&scoped_name(&["rom"], "b")).is_none());
    assert!(reg.get(&scoped_name(&["rom"], "c")).is_none());
    assert_eq!(
        reg.get(&scoped_name(&["rom", "t"], "a"))
            .map(|d| d.layer().id().as_str()),
        Some("a")
    );
}

#[test]
fn missing_fields_render_and_accumulate() {
    let mut m = Missing::new();
    assert!(m.is_empty());
    assert_eq!(m, Missing::default());
    assert_eq!(m.to_string(), "missing required field(s)");
    assert_eq!(m.require("a", &Field::Set(5)), Some(&5));
    assert!(m.is_empty());
    assert_eq!(m.require::<u8>("b", &Field::Unset), None);
    assert_eq!(m.require::<u8>("c", &Field::Omit), None);
    m.push(String::from("d"));
    assert!(!m.is_empty());
    assert_eq!(m.fields(), ["b", "c", "d"]);
    assert_eq!(m.to_string(), "missing required field(s) `b`, `c`, `d`");
    let err: &dyn std::error::Error = &m;
    assert!(err.source().is_none());
}

#[test]
fn errors_are_std_errors() {
    let err: Box<dyn std::error::Error> = Box::new(ResolveError::Unknown { name: name("x") });
    assert_eq!(err.to_string(), "no document named `x`");
    let mut reg = registry([doc("x", with_id(1))]);
    let dup: Box<dyn std::error::Error> = reg.insert(doc("x", with_id(2))).unwrap_err();
    assert!(dup.to_string().starts_with("`x` is already defined"));
}

// ---- properties ----

// Local on purpose: the shared generators in `rr_testkit::strategies::layered`
// (US-RR-22-12) are built on the `rr_core` library crate, which is a different
// crate from this unit-test build of it, so their `Field`, `Keyed`, ... are
// not this module's types. Integration tests (`tests/`) use the shared ones.

/// A random acyclic family: document `i` names some of `0..i` as bases, in
/// a random order. Each comes with its partial document.
fn arb_family() -> impl Strategy<Value = Vec<(Vec<usize>, Rom)>> {
    (1..8usize).prop_flat_map(|n| {
        (0..n)
            .map(|i| {
                let bases = proptest::sample::subsequence((0..i).collect::<Vec<_>>(), 0..=i.min(3))
                    .prop_shuffle();
                (bases, arb_rom())
            })
            .collect::<Vec<_>>()
    })
}

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

fn arb_rom() -> BoxedStrategy<Rom> {
    let storage = prop_oneof![Just(Storage::U8), Just(Storage::I8), Just(Storage::U16)];
    let item = (arb_field(storage), arb_field(0..4u32))
        .prop_map(|(storage, address)| Table { storage, address });
    let state = prop_oneof![
        Just(Nested::Unset),
        item.clone().prop_map(Nested::Patch),
        item.prop_map(Nested::Replace),
        Just(Nested::Omit),
    ];
    let tables = proptest::collection::vec((0..4u8, state), 0..3)
        .prop_map(|entries| entries.into_iter().collect::<Keyed<_, _>>());
    let notes =
        (any::<bool>(), proptest::collection::vec(0..9u8, 0..3)).prop_map(|(replace, items)| {
            if replace {
                Appended::replace(items)
            } else {
                Appended::append(items)
            }
        });
    (arb_field(0..4u8), tables, notes)
        .prop_map(|(id, tables, notes)| Rom { id, tables, notes })
        .boxed()
}

fn family_registry(family: &[(Vec<usize>, Rom)], order: &[usize]) -> Registry<Rom> {
    registry(order.iter().map(|&i| {
        let (bases, rom) = &family[i];
        bases
            .iter()
            .fold(doc(&format!("d{i}"), rom.clone()), |d, b| {
                d.with_base(format!("d{b}").as_str())
            })
    }))
}

/// An independent C3: recursive, list-based, strongest first, as it is
/// usually written. `None` if the bases cannot be linearised.
fn reference_c3(family: &[(Vec<usize>, Rom)], d: usize) -> Option<Vec<usize>> {
    let bases = &family[d].0;
    let mut seqs: Vec<Vec<usize>> = bases
        .iter()
        .rev()
        .map(|&b| reference_c3(family, b))
        .collect::<Option<_>>()?;
    seqs.push(bases.iter().rev().copied().collect());
    let mut out = vec![d];
    loop {
        seqs.retain(|s| !s.is_empty());
        if seqs.is_empty() {
            return Some(out);
        }
        let candidate = seqs
            .iter()
            .map(|s| s[0])
            .find(|c| !seqs.iter().any(|s| s[1..].contains(c)))?;
        out.push(candidate);
        for s in &mut seqs {
            if s[0] == candidate {
                s.remove(0);
            }
        }
    }
}

fn dn(i: usize) -> Name {
    name(&format!("d{i}"))
}

fn is_subsequence(needle: &[Name], haystack: &[Name]) -> bool {
    let mut rest = haystack.iter();
    needle.iter().all(|n| rest.any(|h| h == n))
}

proptest! {
    /// Resolution is the reference C3 linearisation, merged in order; every
    /// document sits above its own bases, in their listed order, and every
    /// base's lineage appears in the child's in the same order; a document's
    /// own replace field always wins (MF-06).
    #[test]
    fn resolution_is_c3_merged_in_order(family in arb_family()) {
        let ids: Vec<usize> = (0..family.len()).collect();
        let reg = family_registry(&family, &ids);
        let mut resolver = reg.resolver();
        for d in ids.iter().copied() {
            let got = resolver.lineage(&dn(d));
            match reference_c3(&family, d) {
                None => {
                    let is_inconsistent = matches!(got, Err(ResolveError::Inconsistent { .. }));
                    prop_assert!(is_inconsistent, "d{d}: {got:?}");
                }
                Some(mut expect) => {
                    let l = got.unwrap();
                    expect.reverse();
                    let expect: Vec<Name> = expect.iter().map(|&i| dn(i)).collect();
                    prop_assert_eq!(l.names(), &expect[..]);
                    // The merged partial is the fold of the registered documents.
                    let mut fold = Rom::default();
                    for n in &expect {
                        fold.merge_from(reg.get(n).unwrap().doc());
                    }
                    prop_assert_eq!(&**l.merged(), &fold);
                    prop_assert_eq!(&**l.merged(), &l.stack().merged());
                    // Ancestry, local order, monotonicity.
                    let pos = |n: &Name| l.names().iter().position(|x| x == n).unwrap();
                    let bases = &family[d].0;
                    for w in bases.windows(2) {
                        prop_assert!(pos(&dn(w[0])) < pos(&dn(w[1])));
                    }
                    for &b in bases {
                        let base = resolver.lineage(&dn(b)).unwrap();
                        prop_assert!(is_subsequence(base.names(), l.names()));
                        prop_assert!(pos(&dn(b)) < pos(l.name()));
                    }
                    // MF-06: the document's own field wins over every base.
                    let own = &family[d].1.id;
                    if !own.is_unset() {
                        prop_assert_eq!(&l.merged().id, own);
                    }
                }
            }
        }
    }

    /// Insertion order, resolution order and a warm cache change nothing;
    /// resolving twice gives the same result; re-layering a resolved
    /// document as a single layer gives it back.
    #[test]
    fn resolution_is_deterministic_and_idempotent(
        (family, shuffled, queries) in arb_family().prop_flat_map(|f| {
            let ids: Vec<usize> = (0..f.len()).collect();
            (Just(f), Just(ids.clone()).prop_shuffle(), Just(ids).prop_shuffle())
        })
    ) {
        let ids: Vec<usize> = (0..family.len()).collect();
        let a = family_registry(&family, &ids);
        let b = family_registry(&family, &shuffled);
        prop_assert_eq!(&a, &b);
        let mut warm = b.resolver();
        let mut results = BTreeMap::new();
        for &q in &queries {
            let n = dn(q);
            let fresh = a.resolver().lineage(&n);
            let cached = warm.lineage(&n);
            prop_assert_eq!(fresh.as_deref(), cached.as_deref());
            let again = warm.lineage(&n);
            prop_assert_eq!(again.as_deref(), cached.as_deref());
            results.insert(q, cached);
        }
        for (q, l) in results {
            if let Ok(l) = l {
                let flat = registry([doc("flat", Rom::clone(l.merged()))]);
                prop_assert_eq!(&*lineage(&flat, "flat").unwrap().merged().clone(), &**l.merged(), "d{}", q);
            }
        }
    }
}
