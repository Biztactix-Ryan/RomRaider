//! Omitting one item of a keyed list through the public `Registry` /
//! `Resolver` path (US-RR-22-2; MF-07, EC-04).
//!
//! Legacy collected the addresses of a definition's tables in one pass
//! before building them, and that pass stopped at the first table marked
//! `omit="true"` (`xml/TableScaleUnmarshaller.java:330-333`, a `return`
//! inside the loop of `filterFoundRomTables`). Every table after the omitted
//! one was never registered, and two things were lost for it:
//!
//! * a later table the child **overrides** kept the base's address: the
//!   base's own pass registered the base address
//!   (`TableScaleUnmarshaller.java:335-336`, run from
//!   `xml/DOMRomUnmarshaller.java:144` for each definition in the `base`
//!   chain), and that registered address is applied over the child's
//!   `storageaddress` (`TableScaleUnmarshaller.java:171-172`);
//! * a later table the child **adds** (not inherited, not an axis) was
//!   dropped altogether: an unregistered name returns no table
//!   (`TableScaleUnmarshaller.java:105, 136-138`).
//!
//! Here the omit is a [`Keyed`] tombstone and removes exactly its item:
//!
//! * **Property**, three named tests (the omit in the middle, from the
//!   [`omit_in_middle`] strategy, and the first and the last item): a base
//!   list of 3 to 5 complete tables, at the bottom of a generated family
//!   (chain, DAG or diamond, global or in local scopes), and an empty child
//!   on top of the family that omits one item. The resolved list is the
//!   base list minus that item, in the same order, every other table equal
//!   to the base's merged with the child's own patch (none for most), in
//!   the merged document and in the finished value. The same child also, at
//!   random, omits more items, adds a table at the end, omits a name the
//!   list does not have, and is followed by a stronger layer that brings the
//!   omitted item back (EC-04 omit-then-override: it restarts in its old
//!   position with none of the base's fields). Run twice: with the family's
//!   own documents stripped of tables (the expected list is computed
//!   directly) and with them kept (the family patches, replaces and omits
//!   the same tables, and the result is compared with the same registry
//!   whose child omits nothing). Counters fail the test if a shape, a
//!   position or a variant is never exercised.
//! * **Hand-checked**: the legacy case (a child that omits one table,
//!   overrides the address of a later one and adds another); an omit in one
//!   branch of a diamond; an omit of a name the list does not have (a
//!   silent no-op, as in legacy); and the [`Appended`] policy, whose omit
//!   and replace act on the whole list, never one item.
//!
//! Legacy references are to RomRaider's Java sources under
//! `legacy/src/main/java/com/romraider` (read, never run; D-024).

use std::cell::Cell;
use std::collections::BTreeSet;
use std::sync::Arc;

use proptest::prelude::*;
use proptest::sample::select;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
use rr_core::diag::Span;
use rr_core::encoding::{Endian, Storage};
use rr_core::layer::{
    Action, Appended, Complete, Document, Field, FieldPath, Keyed, Layer, LayerId, LayerKind,
    Lineage, Merge, Name, Nested, Registry, ResolveError, Resolved, Resolver,
};
use rr_testkit::strategies::layered::sample::{self, Rom, RomDef, TABLE_NAMES, Table, TableDef};
use rr_testkit::strategies::layered::{Family, chain, dag, diamond, omit_in_middle, scoped};

// ---------------------------------------------------------------------------
// Families and registries
// ---------------------------------------------------------------------------

/// The shape a family was generated as, for coverage counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Chain,
    Dag,
    Diamond,
}

/// Every valid shape, global or moved into local scopes, of sample ROMs
/// (partial or complete).
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

/// The id of the document holding the base list: below every family
/// document (no generated id has this form, `<stem>-<i>`).
const BASE: &str = "tables";
/// The id of the child that omits: on top of the family's last document.
const HEIR: &str = "heir";
/// The id of the stronger layer that brings the omitted item back (EC-04).
const READD: &str = "readd";
/// A table name the base list never has: the child adds it at the end.
const ADDED: &str = "Added Table";
/// A table name no layer defines: the child omits it.
const GHOST: &str = "Ghost Table";

fn layer(id: &str, span: Span) -> Layer {
    Layer::new(
        LayerId::new(id),
        LayerKind::File,
        format!("defs/{id}.xml"),
        span,
    )
}

/// `d` with its document replaced by `doc`: same layer, scope and bases.
fn with_doc<P>(d: &Document<P>, doc: P) -> Document<P> {
    let moved = Document::new(d.layer().clone(), doc).in_scope(d.name().scope().clone());
    d.bases().iter().cloned().fold(moved, Document::with_base)
}

fn registry_of(
    docs: impl IntoIterator<Item = Document<Rom>>,
) -> Result<Registry<Rom>, TestCaseError> {
    let mut out = Registry::new();
    for d in docs {
        out.insert(d)
            .map_err(|e| TestCaseError::fail(e.to_string()))?;
    }
    Ok(out)
}

fn lineage_of(
    resolver: &mut Resolver<'_, Rom>,
    name: &Name,
) -> Result<Arc<Lineage<Rom>>, TestCaseError> {
    resolver
        .lineage(name)
        .map_err(|e| TestCaseError::fail(format!("{name}: {e}")))
}

// ---------------------------------------------------------------------------
// The omit case
// ---------------------------------------------------------------------------

/// Where the primary omit falls in the base list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Position {
    First,
    Middle,
    Last,
}

/// A base list and the child's changes to it.
#[derive(Debug, Clone)]
struct Case {
    /// The base list's keys, in order (3 to 5, distinct).
    keys: Vec<String>,
    /// The base list: every key patched with a complete table.
    base: Keyed<String, Table>,
    /// The child's patch of each item (`None`: not mentioned). Never set
    /// for an omitted position.
    patches: Vec<Option<Table>>,
    /// The position of the primary omit.
    at: usize,
    /// Every omitted position: `at` plus, at random, others (MF-07,
    /// several omits).
    omitted: BTreeSet<usize>,
    /// A complete table the child adds after the list ([`ADDED`]).
    added: Option<Table>,
    /// The child also omits [`GHOST`], a name no layer has.
    ghost: bool,
    /// EC-04: a layer above the child patches `keys[at]` with this table.
    readd: Option<Table>,
}

impl Case {
    /// The child's tables; without the omits of the base's items if
    /// `!omits` (the control: the same child, omitting nothing).
    fn overlay(&self, omits: bool) -> Keyed<String, Table> {
        let mut out = Keyed::new();
        for (j, k) in self.keys.iter().enumerate() {
            if self.omitted.contains(&j) {
                if omits {
                    out.omit(k.clone());
                }
            } else if let Some(p) = &self.patches[j] {
                out.patch(k.clone(), p.clone());
            }
        }
        if self.ghost {
            out.omit(GHOST.to_owned());
        }
        if let Some(t) = &self.added {
            out.patch(ADDED.to_owned(), t.clone());
        }
        out
    }

    /// The base's item at `j`.
    fn base_item(&self, j: usize) -> Option<&Table> {
        self.base.get(&self.keys[j]).and_then(Nested::get)
    }

    /// The keys that end up omitted: every omitted position but a re-added
    /// one.
    fn gone(&self) -> BTreeSet<&String> {
        self.omitted
            .iter()
            .filter(|&&j| !(j == self.at && self.readd.is_some()))
            .map(|&j| &self.keys[j])
            .collect()
    }
}

/// A [`Case`] with the primary omit at `position`. The base list and the
/// child's patches come from [`omit_in_middle`] (complete tables, so every
/// lineage finishes); for the first and last position its omit is moved
/// there, leaving the middle item unmentioned.
fn case(position: Position) -> BoxedStrategy<Case> {
    let names = select(TABLE_NAMES.to_vec()).prop_map(str::to_owned);
    (
        omit_in_middle(names, sample::complete_table(), 3..=5),
        proptest::collection::vec(proptest::bool::weighted(0.2), 5),
        proptest::option::weighted(0.4, sample::complete_table()),
        proptest::bool::weighted(0.3),
        proptest::option::weighted(0.3, sample::complete_table()),
    )
        .prop_map(move |(shape, extra, added, ghost, readd)| {
            let n = shape.keys.len();
            let at = match position {
                Position::First => 0,
                Position::Middle => shape.omitted,
                Position::Last => n - 1,
            };
            let omitted: BTreeSet<usize> = (0..n).filter(|&j| j == at || extra[j]).collect();
            let patches = shape
                .keys
                .iter()
                .enumerate()
                .map(|(j, k)| {
                    let p = shape.overlay.get(k).and_then(Nested::get);
                    p.filter(|_| !omitted.contains(&j)).cloned()
                })
                .collect();
            Case {
                keys: shape.keys,
                base: shape.base,
                patches,
                at,
                omitted,
                added,
                ghost,
                readd,
            }
        })
        .boxed()
}

/// The registry: the family (its documents stripped of tables if `strip`),
/// the base list [`BASE`], the child [`HEIR`] in the scope of the family's
/// last document with bases `[BASE, last]` (the base list weakest), and
/// [`READD`] above it if the case re-adds. Returns the name to resolve.
fn registry_for(
    family: &Family<Rom>,
    c: &Case,
    strip: bool,
    omits: bool,
) -> Result<(Registry<Rom>, Name), TestCaseError> {
    let top = &family.docs()[family.len() - 1];
    let scope = top.name().scope().clone();
    let span = top.layer().span();
    let mut docs: Vec<Document<Rom>> = family
        .docs()
        .iter()
        .map(|d| {
            let tables = if strip {
                Keyed::new()
            } else {
                d.doc().tables.clone()
            };
            with_doc(
                d,
                Rom {
                    tables,
                    ..Rom::clone(d.doc())
                },
            )
        })
        .collect();
    docs.push(Document::new(
        layer(BASE, span),
        Rom {
            tables: c.base.clone(),
            ..Rom::default()
        },
    ));
    let heir = Rom {
        xmlid: Field::Set(HEIR.to_owned()),
        tables: c.overlay(omits),
        ..Rom::default()
    };
    docs.push(
        Document::new(layer(HEIR, span), heir)
            .in_scope(scope.clone())
            .with_base(BASE)
            .with_base(top.name().id().clone()),
    );
    let mut target = Name::new(scope.clone(), HEIR);
    if let Some(t) = &c.readd {
        let mut tables = Keyed::new();
        tables.patch(c.keys[c.at].clone(), t.clone());
        let doc = Rom {
            tables,
            ..Rom::default()
        };
        docs.push(
            Document::new(layer(READD, span), doc)
                .in_scope(scope.clone())
                .with_base(HEIR),
        );
        target = Name::new(scope, READD);
    }
    Ok((registry_of(docs)?, target))
}

/// The keys of `rom`'s table list, tombstones included, in order.
fn keys(rom: &Rom) -> Vec<String> {
    rom.tables.entries().map(|(k, _)| k.clone()).collect()
}

/// The present tables of `rom`, in order.
fn present(rom: &Rom) -> Vec<(String, Table)> {
    rom.tables
        .present()
        .map(|(k, t)| (k.clone(), t.clone()))
        .collect()
}

fn finished(tables: &[(String, Table)]) -> Result<Vec<(String, TableDef)>, TestCaseError> {
    tables
        .iter()
        .map(|(k, t)| {
            t.complete()
                .map(|d| (k.clone(), d))
                .map_err(|m| TestCaseError::fail(format!("{k}: {m}")))
        })
        .collect()
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
    /// The omit is at the requested position (for the middle one,
    /// `0 < at < n - 1`).
    position: Cell<u32>,
    /// More than one item omitted.
    several: Cell<u32>,
    /// An item after the omit kept, not mentioned by the child: equal to
    /// the base's (the legacy loss of a later address).
    later_kept: Cell<u32>,
    /// An item after the omit kept and changed by the child's own patch.
    later_modified: Cell<u32>,
    /// An item before the omit kept.
    earlier_kept: Cell<u32>,
    /// A table added after the omit (legacy dropped it).
    added: Cell<u32>,
    /// An omit of a name the list does not have.
    ghost: Cell<u32>,
    /// EC-04: re-added, and different from what inheriting would give.
    readd: Cell<u32>,
    /// With the family's own tables: the omitted item was present in the
    /// control (the omit removed something).
    noisy_removed: Cell<u32>,
    /// With the family's own tables: an item after the omit was kept.
    noisy_later_kept: Cell<u32>,
}

fn bump(c: &Cell<u32>, when: bool) {
    if when {
        c.set(c.get() + 1);
    }
}

/// The family's documents stripped of tables: the base list is the only
/// source, so the expected list is computed directly.
fn check_clean(family: &Family<Rom>, c: &Case, seen: &Seen) -> Result<(), TestCaseError> {
    let (reg, target) = registry_for(family, c, true, true)?;
    let mut resolver = reg.resolver();
    let lineage = lineage_of(&mut resolver, &target)?;
    prop_assert_eq!(
        lineage.names().first(),
        Some(&Name::global(BASE)),
        "the base list is the weakest layer"
    );
    let merged = lineage.merged();

    let mut expect_keys = c.keys.clone();
    let mut expect = Vec::new();
    for (j, k) in c.keys.iter().enumerate() {
        let base = c
            .base_item(j)
            .ok_or_else(|| TestCaseError::fail(format!("{k}: not in the base")))?;
        if j == c.at
            && let Some(again) = &c.readd
        {
            // EC-04: restarts from nothing, in its old position.
            expect.push((k.clone(), again.clone()));
            bump(&seen.readd, base.merged(again) != *again);
        } else if c.omitted.contains(&j) {
            prop_assert_eq!(
                merged.tables.get(k),
                Some(&Nested::Omit),
                "{} stays a tombstone",
                k
            );
        } else {
            let mut t = base.clone();
            match &c.patches[j] {
                Some(p) => {
                    t.merge_from(p);
                    bump(&seen.later_modified, j > c.at && t != *base);
                }
                None => {
                    bump(&seen.later_kept, j > c.at);
                    bump(&seen.earlier_kept, j < c.at);
                }
            }
            expect.push((k.clone(), t));
        }
    }
    if c.ghost {
        expect_keys.push(GHOST.to_owned());
        prop_assert_eq!(merged.tables.get(&GHOST.to_owned()), Some(&Nested::Omit));
    }
    if let Some(t) = &c.added {
        expect_keys.push(ADDED.to_owned());
        expect.push((ADDED.to_owned(), t.clone()));
    }
    prop_assert_eq!(keys(merged), expect_keys, "order kept, tombstones included");
    prop_assert_eq!(
        &present(merged),
        &expect,
        "MF-07: only the omitted items are gone"
    );

    let done = resolver
        .resolve(&target)
        .map_err(|e| TestCaseError::fail(format!("{target}: {e}")))?;
    prop_assert_eq!(&done.value().tables, &finished(&expect)?, "MF-07, finished");

    bump(&seen.several, c.omitted.len() > 1);
    bump(&seen.added, c.added.is_some());
    bump(&seen.ghost, c.ghost);
    Ok(())
}

/// The family's own documents kept (they patch, replace and omit the same
/// tables): the child that omits gives the same list as the child that
/// does not, minus exactly the omitted items.
fn check_noisy(family: &Family<Rom>, c: &Case, seen: &Seen) -> Result<(), TestCaseError> {
    let (with_reg, target) = registry_for(family, c, false, true)?;
    let (control_reg, _) = registry_for(family, c, false, false)?;
    let (mut with_r, mut control_r) = (with_reg.resolver(), control_reg.resolver());
    let with = lineage_of(&mut with_r, &target)?;
    let control = lineage_of(&mut control_r, &target)?;
    let (w, k) = (with.merged(), control.merged());
    prop_assert_eq!(keys(w), keys(k), "same entries, same order");

    let gone = c.gone();
    let again = c.readd.as_ref().map(|t| (&c.keys[c.at], t));
    for ((key, ws), (_, ks)) in w.tables.entries().zip(k.tables.entries()) {
        if gone.contains(key) {
            prop_assert_eq!(ws, &Nested::Omit, "{} omitted", key);
            bump(&seen.noisy_removed, ks.get().is_some());
        } else if let Some((_, t)) = again.filter(|(k, _)| *k == key) {
            prop_assert_eq!(ws, &Nested::Replace(t.clone()), "EC-04: {} restarts", key);
        } else {
            prop_assert_eq!(ws, ks, "{} unchanged by the omit", key);
        }
    }
    let after: BTreeSet<&String> = c.keys[c.at + 1..].iter().collect();
    let expect: Vec<(String, Table)> = present(k)
        .into_iter()
        .filter(|(key, _)| !gone.contains(key))
        .map(|(key, t)| match again {
            Some((r, t)) if *r == key => (key, t.clone()),
            _ => (key, t),
        })
        .collect();
    bump(
        &seen.noisy_later_kept,
        expect.iter().any(|(key, _)| after.contains(key)),
    );
    prop_assert_eq!(
        &present(w),
        &expect,
        "MF-07: only the omitted items are gone"
    );

    let done = with_r.resolve(&target);
    match control_r.resolve(&target) {
        Ok(ctl) => {
            let expect = finished(&expect)?;
            let done = done.map_err(|e| TestCaseError::fail(format!("{target}: {e}")))?;
            prop_assert_eq!(&done.value().tables, &expect, "MF-07, finished");
            prop_assert_eq!(&done.value().xmlid, &ctl.value().xmlid);
        }
        Err(_) => check_missing(done, &gone)?,
    }
    Ok(())
}

/// A failure to finish never names an omitted table.
fn check_missing(
    done: Result<Resolved<RomDef, Rom>, ResolveError>,
    gone: &BTreeSet<&String>,
) -> Result<(), TestCaseError> {
    match done {
        Ok(_) => Ok(()),
        Err(ResolveError::Incomplete { missing, .. }) => {
            for path in missing.fields() {
                for k in gone {
                    prop_assert!(!path.starts_with(&format!("tables[{k}].")), "{}", path);
                }
            }
            Ok(())
        }
        Err(e) => Err(TestCaseError::fail(e.to_string())),
    }
}

/// Runs both checks for the omit at `position` on a deterministic runner
/// and fails if anything was never exercised.
#[expect(
    clippy::panic,
    reason = "a property failure is the calling test's failure"
)]
fn mf07_property(position: Position) {
    let seen = Seen::default();
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: 384,
            ..Config::default()
        },
        TestRng::deterministic_rng(RngAlgorithm::ChaCha),
    );
    let result = runner.run(&(families(), case(position)), |((kind, family), c)| {
        let n = c.keys.len();
        let at_position = match position {
            Position::First => c.at == 0,
            Position::Middle => 0 < c.at && c.at < n - 1,
            Position::Last => c.at == n - 1,
        };
        prop_assert!(at_position, "{:?} at {} of {}", position, c.at, n);
        prop_assert!(c.omitted.contains(&c.at));
        check_clean(&family, &c, &seen)?;
        check_noisy(&family, &c, &seen)?;
        bump(&seen.position, at_position);
        bump(&seen.chain, kind == Kind::Chain && family.len() > 1);
        bump(&seen.dag, kind == Kind::Dag);
        bump(&seen.diamond, kind == Kind::Diamond);
        bump(&seen.scoped, !family.top().scope().is_global());
        Ok(())
    });
    if let Err(e) = result {
        panic!("{e}");
    }
    let Seen {
        chain,
        dag,
        diamond,
        scoped,
        position: at,
        several,
        later_kept,
        later_modified,
        earlier_kept,
        added,
        ghost,
        readd,
        noisy_removed,
        noisy_later_kept,
    } = &seen;
    let mut counters = vec![
        ("chain", chain),
        ("dag", dag),
        ("diamond", diamond),
        ("scoped", scoped),
        ("omit at the position", at),
        ("several omits", several),
        ("added after the omit", added),
        ("omit of an absent name", ghost),
        ("EC-04 re-added", readd),
        ("noisy: removed a present item", noisy_removed),
    ];
    if position != Position::Last {
        counters.push(("later item kept", later_kept));
        counters.push(("later item modified", later_modified));
        counters.push(("noisy: later item kept", noisy_later_kept));
    }
    if position != Position::First {
        counters.push(("earlier item kept", earlier_kept));
    }
    for (what, c) in counters {
        assert!(c.get() > 0, "never exercised: {what} ({seen:?})");
    }
}

/// MF-07: omitting one item in the middle of a list (0 < i < n - 1,
/// generated by [`omit_in_middle`]) removes only that item; every later
/// item is kept, with its own fields (the analogue of legacy's lost table
/// addresses, `xml/TableScaleUnmarshaller.java:330-333`), through chains,
/// DAGs and diamonds, global or scoped.
#[test]
fn mf07_omit_in_the_middle_removes_only_that_item() {
    mf07_property(Position::Middle);
}

/// MF-07 boundary: omitting the first item keeps every later one.
#[test]
fn mf07_omit_of_the_first_item_removes_only_it() {
    mf07_property(Position::First);
}

/// MF-07 boundary: omitting the last item keeps every earlier one.
#[test]
fn mf07_omit_of_the_last_item_removes_only_it() {
    mf07_property(Position::Last);
}

// ---------------------------------------------------------------------------
// Hand-checked resolutions
// ---------------------------------------------------------------------------

fn table_at(address: u32) -> Table {
    Table {
        storage: Field::Set(Storage::U16),
        endian: Field::Set(Endian::Big),
        address: Field::Set(address),
        ..Table::default()
    }
}

fn def_at(address: u32) -> TableDef {
    TableDef {
        storage: Storage::U16,
        endian: Endian::Big,
        address,
        scaling: None,
        axes: Vec::new(),
    }
}

/// A definition with id `xmlid` and the given tables, all patched.
fn rom(xmlid: Option<&str>, tables: &[(&str, Table)]) -> Rom {
    let mut out = Rom {
        xmlid: xmlid.map_or(Field::Unset, |id| Field::Set(id.to_owned())),
        ..Rom::default()
    };
    for (name, t) in tables {
        out.tables.patch((*name).to_owned(), t.clone());
    }
    out
}

fn doc(id: &str, rom: Rom) -> Document<Rom> {
    Document::new(layer(id, Span::line_col(1, 1)), rom)
}

fn names_and_addresses(def: &RomDef) -> Vec<(&str, u32)> {
    def.tables
        .iter()
        .map(|(k, t)| (k.as_str(), t.address))
        .collect()
}

fn tables_path() -> FieldPath<Rom, Keyed<String, Table>> {
    FieldPath::root().field("tables", |r: &Rom| &r.tables)
}

/// The base 16-bit definition legacy's example shapes use: four tables.
fn base_16bit() -> Rom {
    rom(
        Some("16BITBASE"),
        &[
            ("Fuel Map", table_at(0x100)),
            ("Timing", table_at(0x200)),
            ("Boost Target", table_at(0x300)),
            ("Idle Speed", table_at(0x400)),
        ],
    )
}

/// MF-07, the legacy case: a child omits its second table, moves a later
/// one and adds a new one after it. Legacy's address pass stopped at the
/// omit (`xml/TableScaleUnmarshaller.java:330-333`), so the moved table got
/// the base's address back (`:171-172`) and the added one was dropped
/// (`:105, 136-138`). Here both keep the child's definition.
#[test]
fn mf07_legacy_case_later_tables_keep_their_override_and_addition() {
    let mut child = rom(Some("A2WC522N"), &[("Rev Limit", table_at(0x500))]);
    child.tables.omit("Timing".to_owned());
    child.tables.patch(
        "Idle Speed".to_owned(),
        Table {
            address: Field::Set(0x4400),
            ..Table::default()
        },
    );
    let reg = registry_of([
        doc("16BITBASE", base_16bit()),
        doc("A2WC522N", child).with_base("16BITBASE"),
    ])
    .unwrap();
    let done = reg.resolver().resolve(&Name::global("A2WC522N")).unwrap();
    assert_eq!(
        names_and_addresses(done.value()),
        [
            ("Fuel Map", 0x100),
            ("Boost Target", 0x300),
            ("Idle Speed", 0x4400),
            ("Rev Limit", 0x500),
        ],
        "base order kept, the added table appended"
    );
    assert_eq!(
        done.value().tables[2].1,
        def_at(0x4400),
        "the rest inherited"
    );
    let address = tables_path()
        .item("Idle Speed".to_owned())
        .field("address", |t: &Table| &t.address);
    let e = done.explain(&address);
    assert_eq!(e.value(), Some(&0x4400));
    assert_eq!(
        e.winner().map(|s| s.layer().id().as_str()),
        Some("A2WC522N")
    );
    assert_eq!(e.overridden().count(), 1, "over the base's 0x400");
}

/// MF-07 in a diamond: one branch omits a table, the other moves a later
/// one; the top, in either base order, has every table but the omitted
/// one, the later one at the other branch's address.
#[test]
fn mf07_omit_in_one_branch_of_a_diamond_keeps_the_rest() {
    let mut omits = rom(None, &[]);
    omits.tables.omit("Timing".to_owned());
    let moves = rom(
        None,
        &[(
            "Boost Target",
            Table {
                address: Field::Set(0x3300),
                ..Table::default()
            },
        )],
    );
    let reg = registry_of([
        doc("a", base_16bit()),
        doc("b", omits).with_base("a"),
        doc("c", moves).with_base("a"),
        doc("bc", rom(None, &[])).with_base("b").with_base("c"),
        doc("cb", rom(None, &[])).with_base("c").with_base("b"),
    ])
    .unwrap();
    let mut r = reg.resolver();
    for top in ["bc", "cb"] {
        let done = r.resolve(&Name::global(top)).unwrap();
        assert_eq!(
            names_and_addresses(done.value()),
            [
                ("Fuel Map", 0x100),
                ("Boost Target", 0x3300),
                ("Idle Speed", 0x400)
            ],
            "{top}"
        );
    }
}

/// An omit of a name the list does not have is a silent no-op: the
/// finished value is the base's, and nothing is reported (no error, no
/// shadow warning: the resolver has no other warning). The tombstone stays
/// in the merged list, and `explain` names the layer that omitted it.
/// Legacy did the same: an omitted table is removed only if one was
/// inherited (`xml/DOMRomUnmarshaller.java:177-181`, after
/// `xml/TableScaleUnmarshaller.java:82-85`), else skipped without a message.
#[test]
fn omit_of_an_absent_name_is_a_silent_no_op() {
    let mut child = rom(None, &[]);
    child.tables.omit(GHOST.to_owned());
    let reg = registry_of([
        doc("16BITBASE", base_16bit()),
        doc("child", child).with_base("16BITBASE"),
    ])
    .unwrap();
    assert!(reg.shadowed().is_empty());
    let mut r = reg.resolver();
    let base = r.resolve(&Name::global("16BITBASE")).unwrap();
    let done = r.resolve(&Name::global("child")).unwrap();
    assert_eq!(done.value(), base.value());
    let merged = done.lineage().merged();
    assert_eq!(merged.tables.len(), 5);
    assert_eq!(merged.tables.get(&GHOST.to_owned()), Some(&Nested::Omit));
    let e = done.explain(&tables_path().key(GHOST.to_owned()));
    let actions: Vec<_> = e
        .actions()
        .into_iter()
        .map(|(s, a)| (s.layer().id().as_str(), a))
        .collect();
    assert_eq!(actions, [("child", Action::Omitted)]);
    assert!(!e.is_present());
}

/// The [`Appended`] policy has no per-item omit: its items have no key to
/// name one by. An overlay drops the inherited list as a whole (`omit`,
/// a `replace` of nothing) or replaces it (`replace`, the only way to drop
/// one item: restate the others), and later appends follow what it left.
/// Per-item omit is the [`Keyed`] policy, shown alongside: the same layers
/// omit one table and keep the rest.
#[test]
fn appended_lists_are_omitted_or_replaced_whole_never_per_item() {
    let notes = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let mut base = base_16bit();
    base.notes = Appended::append(notes(&["a", "b", "c"]));
    let mut dropped = rom(None, &[]);
    dropped.notes = Appended::omit();
    dropped.tables.omit("Timing".to_owned());
    let mut restated = rom(None, &[]);
    restated.notes = Appended::replace(notes(&["a", "c"]));
    restated.tables.omit("Timing".to_owned());
    let reg = registry_of([
        doc("base", base),
        doc("dropped", dropped).with_base("base"),
        doc("restated", restated).with_base("base"),
        doc(
            "after",
            Rom {
                notes: Appended::append(notes(&["d"])),
                ..Rom::default()
            },
        )
        .with_base("dropped"),
    ])
    .unwrap();
    let mut r = reg.resolver();
    let mut resolve = |id: &str| r.resolve(&Name::global(id)).unwrap().into_value();
    let keyed = [
        ("Fuel Map", 0x100),
        ("Boost Target", 0x300),
        ("Idle Speed", 0x400),
    ];
    let dropped = resolve("dropped");
    assert!(dropped.notes.is_empty(), "omit drops every item");
    assert_eq!(
        names_and_addresses(&dropped),
        keyed,
        "the keyed omit drops one"
    );
    let restated = resolve("restated");
    assert_eq!(
        restated.notes,
        ["a", "c"],
        "replace: the survivors restated"
    );
    assert_eq!(names_and_addresses(&restated), keyed);
    assert_eq!(resolve("after").notes, ["d"], "appends follow the omit");
}
