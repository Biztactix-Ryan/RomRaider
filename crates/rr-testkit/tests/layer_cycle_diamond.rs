//! Cycles never overflow and diamonds resolve deterministically, through
//! the public `Registry` -> `Resolver` -> `ResolveError::to_diagnostic`
//! path (US-RR-22-3; MF-17, EC-04).
//!
//! Legacy looked a ROM definition's `base` up only in the file naming it
//! (`getBaseRom(rootNode.getParentNode(), ...)`,
//! `xml/DOMRomUnmarshaller.java:149-152`, a `RomNotFoundException` when that
//! file has none, `:201-213`) and recursed through `getBaseRom` /
//! `unmarshallRom` for each level, so a looped `base` ran until the stack
//! overflowed and was caught as a `StackOverflowError` with a "looped base"
//! dialog naming nothing (`editor/ecu/OpenImageWorker.java:178-184`).
//!
//! What the other suites already show, and is not repeated here:
//! `layer_cross_file.rs` (cycles of 2..=12 documents in as many files, with
//! and without a lead-in chain, global and scoped, to one Error LAY0001
//! diagnostic with the exact message from every document on the cycle; a
//! 100 000-document cycle and chain), `layered_strategies.rs` (lineage ==
//! `Family::reference_lineage` for random DAGs and diamonds, in either
//! insertion order), `layer_resolution_laws.rs` (deterministic under
//! insertion order, query order, a warm or fresh resolver and the file
//! split), and rr-core's `resolve/tests.rs` (message elision past
//! `MAX_LINEAGE`, a cycle reached through the extend-outer idiom).
//!
//! What is added here:
//!
//! * **No recursion anywhere**: the 100 000-document cycle, chain and
//!   lead-in, down to the diagnostic and through `explain` on the longest
//!   lineage, run on a thread with a [`SMALL_STACK`] stack, so a recursive
//!   walk, message or explanation fails even where a default 2 MiB test
//!   thread would hide it.
//! * **Several cycles in one registry**: each is reported, with its own
//!   path and files, whatever order the documents are asked for in and
//!   whether the resolver is warm.
//! * **A hand-checked diamond** whose branches both set a field, with the
//!   C3 order worked out by hand: which branch wins, and why.
//!
//! Legacy references are to RomRaider's Java sources under
//! `legacy/src/main/java/com/romraider` (read, never run; D-024).

use std::collections::BTreeSet;
use std::sync::Arc;

use proptest::prelude::*;
use rr_core::diag::{Provenance, Severity, Span, codes};
use rr_core::encoding::{Endian, Storage};
use rr_core::layer::{
    Appended, Document, Field, FieldPath, Layer, LayerKind, MAX_LINEAGE, Name, Registry,
    ResolveError, Resolver, Scope, Step,
};
use rr_testkit::strategies::layered::sample::{self, Rom, Table};
use rr_testkit::strategies::layered::{CyclicFamily, cyclic_registry, scoped};

/// The stack the never-overflow checks run on: an eighth of the 2 MiB a
/// test thread gets by default. Nothing in the resolver, its messages or
/// `explain` may recurse per document, so this is ample; one frame per
/// document of a 100 000-document walk is not.
const SMALL_STACK: usize = 256 * 1024;

/// Where a request for a document comes from, in these tests.
fn request() -> Provenance {
    Provenance::new("image.bin", Span::byte_offset(0))
}

/// Runs `f` on a thread with a [`SMALL_STACK`] stack, re-raising its panic.
/// A stack overflow on it aborts the test process: a failure either way.
#[expect(
    clippy::expect_used,
    reason = "test-only helper outside a #[test] fn; failing to spawn fails the calling test"
)]
fn on_small_stack(f: impl FnOnce() + Send + 'static) {
    let worker = std::thread::Builder::new()
        .name("small-stack".into())
        .stack_size(SMALL_STACK)
        .spawn(f)
        .expect("spawn a small-stack thread");
    if let Err(panic) = worker.join() {
        std::panic::resume_unwind(panic);
    }
}

#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn registry_of<P>(docs: impl IntoIterator<Item = Document<P>>) -> Registry<P> {
    let mut out = Registry::new();
    for d in docs {
        out.insert(d).unwrap();
    }
    out
}

/// The global document `id` in file `defs/<id>.xml`.
fn layer_in_own_file(id: &str) -> Layer {
    Layer::new(
        id.into(),
        LayerKind::File,
        format!("defs/{id}.xml"),
        Span::line_col(1, 1),
    )
}

/// `id`, in its own file, setting its `xmlid`, the address of table
/// `Fuel Map` to `address`, and appending note `id`.
fn numbered(id: &str, address: u32) -> Document<Rom> {
    let mut rom = Rom {
        xmlid: Field::Set(id.to_owned()),
        notes: Appended::append(vec![id.to_owned()]),
        ..Rom::default()
    };
    rom.tables.patch(
        "Fuel Map".to_owned(),
        Table {
            storage: Field::Set(Storage::U8),
            endian: Field::Set(Endian::Big),
            address: Field::Set(address),
            ..Table::default()
        },
    );
    Document::new(layer_in_own_file(id), rom)
}

fn fuel_map_address() -> FieldPath<Rom, Field<u32>> {
    FieldPath::root()
        .field("tables", |r: &Rom| &r.tables)
        .item("Fuel Map".to_owned())
        .field("address", |t: &Table| &t.address)
}

fn fuel_map_storage() -> FieldPath<Rom, Field<Storage>> {
    FieldPath::root()
        .field("tables", |r: &Rom| &r.tables)
        .item("Fuel Map".to_owned())
        .field("storage", |t: &Table| &t.storage)
}

/// `d0` <- `d1` <- ... <- `d<n-1>`, each in its own file; `d0` names
/// `first_base` when there is one.
fn chain_of(n: usize, first_base: Option<&str>) -> Vec<Document<Rom>> {
    (0..n)
        .map(|i| {
            let address = u32::try_from(i).unwrap_or(u32::MAX);
            let d = numbered(&format!("d{i}"), address);
            match (i, first_base) {
                (0, Some(b)) => d.with_base(b),
                (0, None) => d,
                _ => d.with_base(format!("d{}", i - 1)),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Never a stack overflow
// ---------------------------------------------------------------------------

const N: usize = 100_000;

/// MF-17: a cycle of 100 000 documents in 100 000 files, on a small stack,
/// is one Error LAY0001 whose `path` keeps every document while the
/// message names the first and last `MAX_LINEAGE / 2` with their files
/// around a count of the rest, closing back on the first.
#[test]
fn mf17_a_hundred_thousand_file_cycle_on_a_small_stack() {
    on_small_stack(|| {
        let top = format!("d{}", N - 1);
        let reg = registry_of(chain_of(N, Some(top.as_str())));
        let err = Resolver::new(&reg).lineage(&Name::global(top.as_str()));
        let Err(err @ ResolveError::Cycle { .. }) = err else {
            panic!("expected a cycle, got {err:?}");
        };
        let ResolveError::Cycle { path, layers, .. } = &err else {
            unreachable!()
        };
        assert_eq!(path.len(), N + 1);
        assert_eq!(path.first(), path.last());
        let distinct: BTreeSet<&Name> = path.iter().collect();
        assert_eq!(distinct.len(), N, "every document once, the first repeated");
        let files: BTreeSet<&str> = layers.iter().map(Layer::source).collect();
        assert_eq!(files.len(), N, "each document in its own file");

        let d = err.to_diagnostic(request());
        assert_eq!(d.severity(), Severity::Error);
        assert_eq!(d.code(), codes::INHERITANCE_CYCLE);
        assert_eq!(d.provenance(), &layer_in_own_file("d0").provenance());
        let half = MAX_LINEAGE / 2;
        let named = |ids: &mut dyn Iterator<Item = usize>| -> String {
            ids.map(|i| format!("d{i} (defs/d{i}.xml:1:1) -> "))
                .collect()
        };
        let expected = format!(
            "inheritance cycle: {}... {} more ... -> {}{top}",
            named(&mut (N - half..N).rev()),
            N - 2 * half,
            named(&mut (0..half).rev()),
        );
        assert_eq!(d.message(), expected);
        assert_eq!(d.message().matches("(defs/").count(), 2 * half);
    });
}

/// MF-17: a chain of 100 000 documents, each in its own file, on a small
/// stack: the top is refused at `MAX_LINEAGE` (LAY0005, an Error at the
/// first document over the limit), the longest allowed lineage resolves,
/// and `explain` on it walks all `MAX_LINEAGE` layers, strongest first,
/// each with its file.
#[test]
fn mf17_a_hundred_thousand_file_chain_and_its_explanation_on_a_small_stack() {
    on_small_stack(|| {
        let reg = registry_of(chain_of(N, None));
        let mut resolver = reg.resolver();
        let err = resolver.lineage(&Name::global(format!("d{}", N - 1).as_str()));
        let d = err.err().map(|e| e.to_diagnostic(request()));
        let d = d.expect("a 100 000 deep chain is too large");
        assert_eq!(d.severity(), Severity::Error);
        assert_eq!(d.code(), codes::LINEAGE_TOO_LARGE);
        let over = format!("d{MAX_LINEAGE}");
        assert_eq!(d.provenance(), &layer_in_own_file(&over).provenance());

        let deepest = format!("d{}", MAX_LINEAGE - 1);
        let resolved = resolver.resolve(&Name::global(deepest.as_str()));
        let resolved = resolved.expect("the longest allowed lineage resolves");
        assert_eq!(resolved.value().xmlid, deepest);
        assert_eq!(resolved.lineage().names().len(), MAX_LINEAGE);

        let e = resolved.explain(&fuel_map_address());
        let expected_address = u32::try_from(MAX_LINEAGE - 1).ok();
        assert_eq!(e.value().copied(), expected_address);
        let files: Vec<&str> = e.history().map(|s| s.layer().source()).collect();
        let expected: Vec<String> = (0..MAX_LINEAGE)
            .rev()
            .map(|i| format!("defs/d{i}.xml"))
            .collect();
        assert_eq!(
            files, expected,
            "every layer, strongest first, with its file"
        );
        assert_eq!(e.overridden().count(), MAX_LINEAGE - 1);
        let report = e.to_string();
        assert!(report.contains("set to 0 at defs/d0.xml"), "{report}");
    });
}

/// MF-17: a cycle reached through a 100 000-document lead-in chain across
/// files, on a small stack: the error's path is the cycle alone (the
/// lead-in is not on it, nor in the message), and the documents below the
/// lead-in's entry into the cycle are the only ones reported.
#[test]
fn mf17_a_cycle_behind_a_hundred_thousand_file_lead_in_on_a_small_stack() {
    on_small_stack(|| {
        let mut docs = chain_of(N, Some("x"));
        docs.push(numbered("x", 1).with_base("y"));
        docs.push(numbered("y", 2).with_base("x"));
        let reg = registry_of(docs);
        let mut resolver = reg.resolver();
        let top = Name::global(format!("d{}", N - 1).as_str());
        for asked in [top, Name::global("d0")] {
            let err = resolver.lineage(&asked);
            let Err(err @ ResolveError::Cycle { .. }) = err else {
                panic!("{asked}: expected a cycle, got {err:?}");
            };
            let ResolveError::Cycle { path, .. } = &err else {
                unreachable!()
            };
            let path: Vec<String> = path.iter().map(ToString::to_string).collect();
            assert_eq!(path, ["x", "y", "x"], "asked for {asked}");
            let d = err.to_diagnostic(request());
            assert_eq!(d.severity(), Severity::Error);
            assert_eq!(d.code(), codes::INHERITANCE_CYCLE);
            assert_eq!(
                d.message(),
                "inheritance cycle: x (defs/x.xml:1:1) -> y (defs/y.xml:1:1) -> x"
            );
            assert_eq!(d.provenance(), &layer_in_own_file("y").provenance());
        }
    });
}

// ---------------------------------------------------------------------------
// Several cycles in one registry
// ---------------------------------------------------------------------------

/// `shape` with every document renamed `z-<id>` (references rewritten to
/// follow) and moved to file `other/<file>`, as plain documents: a second,
/// disjoint copy of the shape's graph, in files of its own.
fn disjoint_copy(shape: &CyclicFamily<Rom>) -> Vec<Document<Rom>> {
    let mut family = shape.family.clone();
    for i in 0..family.len() {
        let id = format!("z-{}", family.name(i).id());
        family = family.renamed(i, id);
    }
    family
        .docs()
        .iter()
        .map(|d| {
            let l = d.layer();
            let file = format!("other/{}", l.source());
            let layer = Layer::new(l.id().clone(), l.kind(), file, l.span());
            let moved =
                Document::new(layer, Arc::clone(d.doc())).in_scope(d.name().scope().clone());
            d.bases().iter().cloned().fold(moved, Document::with_base)
        })
        .collect()
}

/// The message a cycle through `docs[cycle[0]]`, `docs[cycle[1]]`, ... must
/// have.
fn cycle_message<P>(docs: &[Document<P>], cycle: &[usize]) -> String {
    let mut out = String::from("inheritance cycle: ");
    for &c in cycle {
        let l = docs[c].layer();
        out.push_str(&format!(
            "{} ({}{}) -> ",
            docs[c].name(),
            l.source(),
            l.span()
        ));
    }
    if let Some(&first) = cycle.first() {
        out.push_str(&docs[first].name().to_string());
    }
    out
}

/// What resolving document `i` of `docs` must report, for `docs` holding
/// shape `a` at indices `0..` and its disjoint copy `b` from `offset`: the
/// cycle rotated to start at `i` for a document on a cycle, the shape's
/// cycle from its first document for one that leads into it, `None` for
/// one that resolves.
fn expected_cycle(
    a: &CyclicFamily<Rom>,
    b: &CyclicFamily<Rom>,
    offset: usize,
    i: usize,
) -> Option<Vec<usize>> {
    let (shape, at) = if i < offset { (a, 0) } else { (b, offset) };
    let local = i - at;
    let cycle: Vec<usize> = shape.cycle.iter().map(|c| c + at).collect();
    if let Some(j) = shape.cycle.iter().position(|&c| c == local) {
        let mut rotated = cycle[j..].to_vec();
        rotated.extend_from_slice(&cycle[..j]);
        return Some(rotated);
    }
    let reaches = shape
        .family
        .ancestors(local)
        .iter()
        .any(|x| shape.cycle.contains(x));
    reaches.then_some(cycle)
}

fn cyclic() -> BoxedStrategy<CyclicFamily<Rom>> {
    prop_oneof![
        cyclic_registry(sample::rom(), 2..=6),
        scoped(cyclic_registry(sample::rom(), 2..=6)),
    ]
    .boxed()
}

proptest! {
    /// MF-17: two disjoint cycles (each of 2..=6 documents in as many
    /// files, global or scoped, with pool bases and lead-in chains) in one
    /// registry. Asked for in any order, from a fresh resolver and from one
    /// warm resolver shared by every query, each document reports exactly
    /// its own cycle (Error LAY0001, the full path with files), or resolves
    /// if it reaches neither, and over all documents both cycles are
    /// reported: neither hides the other, whatever is asked first.
    #[test]
    fn mf17_disjoint_cycles_are_each_reported_in_any_query_order(
        (a, b, query) in (cyclic(), cyclic()).prop_flat_map(|(a, b)| {
            let n = a.family.len() + b.family.len();
            let order: Vec<usize> = (0..n).collect();
            (Just(a), Just(b), Just(order).prop_shuffle())
        })
    ) {
        let offset = a.family.len();
        let docs: Vec<Document<Rom>> =
            a.family.docs().iter().cloned().chain(disjoint_copy(&b)).collect();
        let reg = registry_of(docs.iter().cloned());
        let mut warm = reg.resolver();
        let mut reported = BTreeSet::new();
        for &q in &query {
            let n = docs[q].name();
            let expect = expected_cycle(&a, &b, offset, q);
            let fresh = reg.resolver().lineage(n);
            let cached = warm.lineage(n);
            prop_assert_eq!(fresh.as_deref(), cached.as_deref(), "{}", n);
            match (&expect, &fresh) {
                (None, Ok(_)) => {}
                (Some(cycle), Err(e @ ResolveError::Cycle { path, .. })) => {
                    let names: Vec<&Name> = cycle.iter().chain(&cycle[..1]).map(|&c| docs[c].name()).collect();
                    prop_assert_eq!(path.iter().collect::<Vec<_>>(), names, "{}", n);
                    let d = e.to_diagnostic(request());
                    prop_assert_eq!(d.severity(), Severity::Error);
                    prop_assert_eq!(d.code(), codes::INHERITANCE_CYCLE);
                    prop_assert_eq!(d.message(), cycle_message(&docs, cycle));
                    let members: BTreeSet<&Name> = path.iter().collect();
                    reported.insert(members.into_iter().cloned().collect::<Vec<_>>());
                }
                (expect, got) => {
                    return Err(TestCaseError::fail(format!("{n}: expected {expect:?}, got {got:?}")));
                }
            }
        }
        let cycle_of = |shape: &CyclicFamily<Rom>, at: usize| -> Vec<Name> {
            let members: BTreeSet<&Name> = shape.cycle.iter().map(|c| docs[c + at].name()).collect();
            members.into_iter().cloned().collect()
        };
        let both: BTreeSet<Vec<Name>> = [cycle_of(&a, 0), cycle_of(&b, offset)].into();
        prop_assert_eq!(reported, both);
    }
}

/// MF-17, by hand: two cycles across files, a document that names a base
/// on each, and a table-local `x` extending (self-named base) the global
/// `x` of one of them. A document reaching both reports the one its walk
/// meets first (its weaker base, listed first), never a merge of the two;
/// the extension reports the global loop, which it is not on; and every
/// document reports the same thing however the others were asked first.
#[test]
fn mf17_two_cycles_and_a_document_reaching_both() {
    let local = Scope::global().child("A2WC400H").child("Boost Target");
    let reg = registry_of([
        numbered("x", 1).with_base("a"),
        numbered("a", 2).with_base("x"),
        numbered("p", 3).with_base("q"),
        numbered("q", 4).with_base("r"),
        numbered("r", 5).with_base("p"),
        numbered("xp", 6).with_base("x").with_base("p"),
        numbered("px", 7).with_base("p").with_base("x"),
        Document::new(
            Layer::new(
                "x".into(),
                LayerKind::File,
                "defs/rom.xml",
                Span::line_col(9, 5),
            ),
            Rom::default(),
        )
        .in_scope(local.clone())
        .with_base("x"),
    ]);
    let xa = "inheritance cycle: x (defs/x.xml:1:1) -> a (defs/a.xml:1:1) -> x";
    let pqr = "inheritance cycle: p (defs/p.xml:1:1) -> q (defs/q.xml:1:1) \
               -> r (defs/r.xml:1:1) -> p";
    let local_x = Name::new(local, "x");
    let cases: [(Name, &str); 5] = [
        (Name::global("xp"), xa),
        (Name::global("px"), pqr),
        (local_x, xa),
        (Name::global("x"), xa),
        (Name::global("p"), pqr),
    ];
    let report = |r: &mut Resolver<'_, Rom>, n: &Name| {
        let d = r.lineage(n).err().map(|e| e.to_diagnostic(request()));
        let d = d.unwrap_or_else(|| panic!("{n} resolved"));
        assert_eq!(d.severity(), Severity::Error, "{n}");
        assert_eq!(d.code(), codes::INHERITANCE_CYCLE, "{n}");
        d.message().to_owned()
    };
    // Every order of the five queries, on one warm resolver each.
    let mut order: Vec<usize> = (0..cases.len()).collect();
    loop {
        let mut warm = reg.resolver();
        for &i in &order {
            let (n, expected) = &cases[i];
            assert_eq!(report(&mut warm, n), *expected, "{n} after {order:?}");
        }
        if !next_permutation(&mut order) {
            break;
        }
    }
}

/// Steps `v` to its next permutation in lexicographic order; `false` (and
/// `v` sorted again) after the last.
fn next_permutation(v: &mut [usize]) -> bool {
    let Some(i) = (1..v.len()).rev().find(|&i| v[i - 1] < v[i]) else {
        v.reverse();
        return false;
    };
    let Some(j) = (i..v.len()).rev().find(|&j| v[j] > v[i - 1]) else {
        return false;
    };
    v.swap(i - 1, j);
    v[i..].reverse();
    true
}

// ---------------------------------------------------------------------------
// A diamond, by hand
// ---------------------------------------------------------------------------

/// `id` in its own file, with table `Fuel Map` patched to `table` and note
/// `id` appended.
fn with_table(id: &str, xmlid: Option<&str>, table: Table) -> Document<Rom> {
    let mut rom = Rom {
        xmlid: xmlid.map_or(Field::Unset, |x| Field::Set(x.to_owned())),
        notes: Appended::append(vec![id.to_owned()]),
        ..Rom::default()
    };
    rom.tables.patch("Fuel Map".to_owned(), table);
    Document::new(layer_in_own_file(id), rom)
}

/// The hand-checked diamond, each document in its own file:
///
/// ```text
///            base      Fuel Map: U16, big-endian, 0x1000
///           /    \
///       left1    right  storage I8 (signed)  |  storage U32
///         |       |
///       left2     |     address 0x2000
///           \    /
///      top: [left2, right]    pot: [right, left2]
/// ```
fn diamond_docs() -> Vec<Document<Rom>> {
    let set = |s: Option<Storage>, address: Option<u32>| Table {
        storage: s.map_or(Field::Unset, Field::Set),
        address: address.map_or(Field::Unset, Field::Set),
        ..Table::default()
    };
    vec![
        with_table(
            "base",
            Some("BASE"),
            Table {
                endian: Field::Set(Endian::Big),
                ..set(Some(Storage::U16), Some(0x1000))
            },
        ),
        with_table("left1", None, set(Some(Storage::I8), None)).with_base("base"),
        with_table("left2", None, set(None, Some(0x2000))).with_base("left1"),
        with_table("right", None, set(Some(Storage::U32), None)).with_base("base"),
        with_table("top", Some("TOP"), Table::default())
            .with_base("left2")
            .with_base("right"),
        with_table("pot", Some("POT"), Table::default())
            .with_base("right")
            .with_base("left2"),
    ]
}

/// EC-04 / MF-06 by hand. C3, strongest first, with
/// `S(left2) = [left2, left1, base]` and `S(right) = [right, base]`:
///
/// * `S(top) = [top] + merge([right, base], [left2, left1, base],
///   [right, left2])`: `right` (in no tail), then `base` is skipped (in the
///   tail of `left2`'s list) for `left2`, `left1`, then `base`. Weakest
///   first: `base, left1, left2, right, top`. The storage type is `right`'s
///   `U32`: the later base wins over the earlier base's whole side, even
///   though `left1`, which set `I8`, is no further from `base`. The address
///   is `left2`'s `0x2000`: `right` does not set one, and `base` is applied
///   once, below both sides, so it cannot come back above `left2` and undo
///   it.
/// * `S(pot)`, the bases the other way: `base, right, left1, left2, pot`;
///   the storage type is `left1`'s signed `I8`, overriding `right`'s
///   unsigned `U32` and `base`'s `U16` (MF-06: signedness is the storage
///   type, overridden like any other field).
///
/// The same in all 720 insertion orders, from a fresh and a warm resolver,
/// with `base` once in each lineage, and `explain` names the winner and
/// what it overrode, strongest first, each with its file.
#[test]
fn ec04_a_diamond_with_both_branches_overriding_resolves_by_c3() {
    let docs = diamond_docs();
    let names = |l: &rr_core::layer::Lineage<Rom>| -> Vec<String> {
        l.names().iter().map(ToString::to_string).collect()
    };
    let mut order: Vec<usize> = (0..docs.len()).collect();
    let mut first: Option<Registry<Rom>> = None;
    let mut permutations = 0;
    loop {
        let reg = registry_of(order.iter().map(|&i| docs[i].clone()));
        if let Some(f) = &first {
            assert_eq!(&reg, f, "insertion order {order:?}");
        }
        for warm_first in [false, true] {
            let mut r = reg.resolver();
            if warm_first {
                // Warm the cache with each side, then ask for the tops.
                for side in ["left2", "right", "pot"] {
                    assert!(r.lineage(&Name::global(side)).is_ok());
                }
            }
            let top = r.resolve(&Name::global("top")).expect("top resolves");
            assert_eq!(
                names(top.lineage()),
                ["base", "left1", "left2", "right", "top"]
            );
            let fuel = &top.value().tables;
            assert_eq!(fuel.len(), 1);
            let t = &fuel[0].1;
            assert_eq!(
                (t.storage, t.endian, t.address),
                (Storage::U32, Endian::Big, 0x2000)
            );
            assert_eq!(
                top.value().notes,
                ["base", "left1", "left2", "right", "top"]
            );
            assert_eq!(top.value().xmlid, "TOP");

            let pot = r.resolve(&Name::global("pot")).expect("pot resolves");
            assert_eq!(
                names(pot.lineage()),
                ["base", "right", "left1", "left2", "pot"]
            );
            let t = &pot.value().tables[0].1;
            assert_eq!(
                (t.storage, t.endian, t.address),
                (Storage::I8, Endian::Big, 0x2000)
            );

            for (resolved, winner, overridden) in [
                (
                    &top,
                    ("right", Storage::U32),
                    [("left1", Storage::I8), ("base", Storage::U16)],
                ),
                (
                    &pot,
                    ("left1", Storage::I8),
                    [("right", Storage::U32), ("base", Storage::U16)],
                ),
            ] {
                let e = resolved.explain(&fuel_map_storage());
                let step = |s: &Step<'_, Field<Storage>>| {
                    let set = s.change().leaf().and_then(Field::get).copied();
                    (
                        s.layer().id().to_string(),
                        s.layer().source().to_owned(),
                        set,
                    )
                };
                let expect =
                    |(id, s): (&str, Storage)| (id.to_owned(), format!("defs/{id}.xml"), Some(s));
                assert_eq!(e.winner().map(step), Some(expect(winner)));
                let got: Vec<_> = e.overridden().map(step).collect();
                let want: Vec<_> = overridden.into_iter().map(expect).collect();
                assert_eq!(got, want);
            }
        }
        first.get_or_insert(reg);
        permutations += 1;
        if !next_permutation(&mut order) {
            break;
        }
    }
    assert_eq!(permutations, 720);
}
