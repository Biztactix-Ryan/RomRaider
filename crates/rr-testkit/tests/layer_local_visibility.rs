//! A name defined in a local scope is never visible as a global base, through
//! the public `Registry` / `Resolver` path (US-RR-22-4; MF-16).
//!
//! Legacy kept one flat, case-folded map of scalings for a whole definition
//! (`xml/TableScaleUnmarshaller.java:58`). A `<scaling>` written inside a
//! `<table>` is unmarshalled by the same `unmarshallScale`
//! (`TableScaleUnmarshaller.java:274-277`) as a definition-level
//! `<scalingbase>` (`:71-74`), and `unmarshallScale` ends by putting every
//! scaling it builds into that map (`:460-461`,
//! `scales.put(scale.getName().toLowerCase(), scale)`). Every later
//! `base="..."` lookup (`:415-421`, `scales.containsKey` then a deep copy of
//! `scales.get`) sees it, so:
//!
//! * a table-local scaling named only inside table *Fuel Map* is found as
//!   the base of a scaling in table *Timing* (or anywhere after it), where
//!   it should be an unknown base;
//! * a table-local scaling with the name of a `<scalingbase>` *replaces*
//!   it in the map, so every later `base` gets the table's local values
//!   instead of the global ones, and which one a base finds depends on
//!   which was `put` last.
//!
//! Here a base reference is looked up lexically, innermost scope out
//! (`Registry::lookup` / `Registry::base_of`), and the registry is ordered,
//! so insertion order cannot matter:
//!
//! * **Property** ([`mf16_local_names_never_leak_outward`]): any generated
//!   family as background (DAG, shadowing, extend-outer,
//!   unknown-base, each also [`scoped`]), plus a local document `x` in a
//!   ROM / table scope, plain or naming its own id (the extend-outer idiom),
//!   optionally a global `x`, and one referrer naming `x` from each
//!   [`Direction`]: the same scope, a descendant, the parent, a sibling,
//!   the same table name in another ROM, and the global scope. Inserted in
//!   a random order. From the same scope or a descendant the referrer gets
//!   the local `x` (over the global one if it extends it); from anywhere
//!   else it gets the global `x`'s values, never the local ones, or, with no
//!   global `x`, an `UnknownBase` (LAY0002, Error) whose message names the
//!   hidden scope. The background family resolves exactly as it does alone.
//!   Counters fail the test unless every direction meets every outcome.
//! * **Property** over [`unknown_base`]'s MF-16 variant (the global
//!   spelling of a local-only id): the diagnostic names the hidden scope,
//!   and adding a global document with that id, before or after the local
//!   one, makes the reference resolve to the global document, never the
//!   local one.
//! * **Property** over [`extending`] (the extend-outer idiom): references
//!   from the global scope, the outer document's scope and a sibling of the
//!   first extension never see an extension, in any insertion order.
//! * **Hand-checked**: the legacy failure itself, a table-local `AFR`
//!   scaling and a global `AFR` scaling base, inserted both ways round.
//!
//! Legacy references are to RomRaider's Java sources under
//! `legacy/src/main/java/com/romraider` (read, never run; D-024).

use std::cell::RefCell;
use std::collections::BTreeSet;

use proptest::prelude::*;
use proptest::sample::select;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
use rr_core::diag::{Severity, Span, codes};
use rr_core::layer::{
    Document, Field, Layer, LayerId, LayerKind, Name, Registry, ResolveError, Scope,
};
use rr_testkit::strategies::layered::family::TABLE_SEGMENTS;
use rr_testkit::strategies::layered::sample::{self, Rom, Scaling};
use rr_testkit::strategies::layered::{Family, dag, extending, scoped, shadowing, unknown_base};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The id every added document names: no generator produces it.
const X: &str = "mf16-x";

/// The ROM segment of the local scope; no generator uses it.
const ROM: &str = "MF16-ROM";

/// Another ROM, with a table of the same name as the local scope's.
const OTHER_ROM: &str = "MF16-OTHER-ROM";

fn layer(id: &str, file: &str) -> Layer {
    Layer::new(
        LayerId::new(id),
        LayerKind::File,
        file,
        Span::line_col(1, 1),
    )
}

/// Where a resolution was asked for: never where an `UnknownBase` is
/// reported.
fn layer_request() -> rr_core::diag::Provenance {
    layer("request", "request.xml").provenance()
}

fn rom(xmlid: &str) -> Rom {
    Rom {
        xmlid: Field::Set(xmlid.to_owned()),
        ..Rom::default()
    }
}

fn scope(path: &[&str]) -> Scope {
    path.iter().fold(Scope::global(), |s, seg| s.child(*seg))
}

/// A registry of `docs`, inserted in `order`.
fn registry_in<P: Clone>(docs: &[Document<P>], order: &[usize]) -> Result<Registry<P>, String> {
    let mut out = Registry::new();
    for &i in order {
        out.insert(docs[i].clone()).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// The lineage of `name` as names, or the error.
fn lineage<P: rr_core::layer::Merge + Clone>(
    reg: &Registry<P>,
    name: &Name,
) -> Result<Vec<Name>, ResolveError> {
    reg.resolver().lineage(name).map(|l| l.names().to_vec())
}

/// The message `UnknownBase` gives for `document` naming `X`, defined
/// only inside `hidden` (at most three scopes).
fn hidden_message(document: &Name, base: &str, hidden: &[Name]) -> String {
    let scopes: Vec<String> = hidden.iter().map(|n| format!("`{}`", n.scope())).collect();
    format!(
        "`{document}` names base `{base}`, which is not defined here: `{base}` is defined \
         only inside {}; not visible from `{document}`",
        scopes.join(", ")
    )
}

/// Checks `err` is an `UnknownBase` for `document` naming `base` with
/// exactly `hidden`, reported as an LAY0002 Error at `document`'s layer,
/// whose message names every hidden scope.
fn check_hidden(
    err: &ResolveError,
    document: &Name,
    layer: &Layer,
    base: &str,
    hidden: &[Name],
) -> Result<(), TestCaseError> {
    let ResolveError::UnknownBase {
        document: d,
        base: b,
        hidden: h,
        ..
    } = err
    else {
        return Err(TestCaseError::fail(format!("{document}: {err:?}")));
    };
    prop_assert_eq!(d, document);
    prop_assert_eq!(b.as_str(), base);
    prop_assert_eq!(&**h, hidden);
    let diag = err.to_diagnostic(layer_request());
    prop_assert_eq!(diag.code(), codes::UNKNOWN_BASE);
    prop_assert_eq!(diag.severity(), Severity::Error);
    prop_assert_eq!(diag.provenance(), &layer.provenance());
    prop_assert_eq!(diag.message(), hidden_message(document, base, hidden));
    Ok(())
}

// ---------------------------------------------------------------------------
// Directions
// ---------------------------------------------------------------------------

/// Where a reference to the local `x` comes from, relative to the local
/// scope `L`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Direction {
    /// `L` itself.
    Same,
    /// A scope inside `L`.
    Descendant,
    /// The scope directly enclosing `L` (never the global scope).
    Parent,
    /// Another scope in `L`'s parent.
    Sibling,
    /// The same path as `L` under another ROM: legacy's "another table of
    /// the same name".
    Cousin,
    /// The global scope.
    Global,
}

impl Direction {
    const ALL: [Self; 6] = [
        Self::Same,
        Self::Descendant,
        Self::Parent,
        Self::Sibling,
        Self::Cousin,
        Self::Global,
    ];

    /// Whether lexical lookup from this direction sees `L`.
    const fn inside(self) -> bool {
        matches!(self, Self::Same | Self::Descendant)
    }

    /// The referrer's scope, for the local scope `local` (a ROM segment
    /// then one or more segments).
    fn scope(self, local: &[&str]) -> Scope {
        let parent = &local[..local.len() - 1];
        match self {
            Self::Same => scope(local),
            Self::Descendant => scope(local).child("MF16-INNER"),
            Self::Parent => scope(parent),
            Self::Sibling => scope(parent).child("MF16-SIBLING"),
            Self::Cousin => {
                let mut path = local.to_vec();
                path[0] = OTHER_ROM;
                scope(&path)
            }
            Self::Global => Scope::global(),
        }
    }

    fn id(self) -> String {
        format!("mf16-ref-{self:?}").to_lowercase()
    }
}

/// What a referrer resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Outcome {
    /// The local `x` alone.
    Local,
    /// The local `x` extending the global one.
    LocalOverGlobal,
    /// The global `x`, the local one present but unseen.
    Global,
    /// `UnknownBase`, the local `x` hidden.
    Hidden,
    /// `UnknownBase` of the local `x` itself: it extends an `x` no
    /// enclosing scope defines.
    SelfUnknown,
}

// ---------------------------------------------------------------------------
// The direction property
// ---------------------------------------------------------------------------

/// The background family's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Bg {
    Dag,
    ScopedDag,
    Shadowing,
    ScopedShadowing,
    Extending,
    ScopedUnknownBase,
}

fn background() -> impl Strategy<Value = (Bg, Family<Rom>)> {
    prop_oneof![
        dag(sample::rom(), 1..=5).prop_map(|f| (Bg::Dag, f)),
        scoped(dag(sample::rom(), 1..=5)).prop_map(|f| (Bg::ScopedDag, f)),
        shadowing(sample::rom()).prop_map(|s| (Bg::Shadowing, s.family)),
        scoped(shadowing(sample::rom())).prop_map(|s| (Bg::ScopedShadowing, s.family)),
        extending(sample::rom()).prop_map(|s| (Bg::Extending, s.family)),
        scoped(unknown_base(sample::rom())).prop_map(|s| (Bg::ScopedUnknownBase, s.family)),
    ]
}

/// One generated case: the documents and an insertion order.
#[derive(Debug, Clone)]
struct Case {
    bg: Bg,
    /// The background family's document count: `docs[..n]`.
    n: usize,
    local: Vec<&'static str>,
    self_named: bool,
    global: bool,
    docs: Vec<Document<Rom>>,
    order: Vec<usize>,
}

impl Case {
    fn local_scope(&self) -> Scope {
        scope(&self.local)
    }
    fn local_name(&self) -> Name {
        Name::new(self.local_scope(), X)
    }
    /// The index of the local `x` and of the global one, if any.
    fn x_at(&self) -> (usize, Option<usize>) {
        (self.n, self.global.then_some(self.n + 1))
    }
    fn referrer(&self, d: Direction) -> &Document<Rom> {
        let first = self.n + 1 + usize::from(self.global);
        &self.docs[first + Direction::ALL.iter().position(|&x| x == d).unwrap_or(0)]
    }
}

fn case() -> impl Strategy<Value = Case> {
    (
        background(),
        select(TABLE_SEGMENTS.to_vec()),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_flat_map(|((bg, family), table, deeper, self_named, global)| {
            let mut local = vec![ROM, table];
            if deeper {
                local.push("MF16-SUB");
            }
            let mut docs = family.docs().to_vec();
            let n = docs.len();
            let mut x =
                Document::new(layer(X, "defs/local.xml"), rom("local")).in_scope(scope(&local));
            if self_named {
                x = x.with_base(X);
            }
            docs.push(x);
            if global {
                docs.push(Document::new(layer(X, "defs/global.xml"), rom("global")));
            }
            for d in Direction::ALL {
                let file = format!("defs/{}.xml", d.id());
                docs.push(
                    Document::new(layer(&d.id(), &file), Rom::default())
                        .in_scope(d.scope(&local))
                        .with_base(X),
                );
            }
            let order = Just((0..docs.len()).collect::<Vec<_>>()).prop_shuffle();
            (
                Just(Case {
                    bg,
                    n,
                    local,
                    self_named,
                    global,
                    docs,
                    order: Vec::new(),
                }),
                order,
            )
        })
        .prop_map(|(mut c, order)| {
            c.order = order;
            c
        })
}

/// Checks one referrer and returns what it resolved to.
fn check_direction(c: &Case, reg: &Registry<Rom>, d: Direction) -> Result<Outcome, TestCaseError> {
    let r = c.referrer(d);
    let (r_name, from) = (r.name().clone(), r.name().scope().clone());
    let local = c.local_name();
    let global = Name::global(X);
    let got = lineage(reg, &r_name);
    // The document a lookup from the referrer's scope finds.
    let found = reg
        .lookup(&from, &LayerId::new(X))
        .map(|doc| doc.name().clone());
    if d.inside() {
        prop_assert_eq!(found.as_ref(), Some(&local), "{} from {}", X, from);
        if c.self_named && !c.global {
            let err = got
                .err()
                .ok_or_else(|| TestCaseError::fail(format!("{d:?} resolved")))?;
            let ResolveError::UnknownBase {
                document, hidden, ..
            } = &err
            else {
                return Err(TestCaseError::fail(format!("{d:?}: {err:?}")));
            };
            prop_assert_eq!(document, &local);
            prop_assert!(hidden.is_empty(), "{hidden:?}");
            prop_assert_eq!(
                err.to_string(),
                format!(
                    "`{local}` names its own id `{X}` as its base, which means the `{X}` \
                     of an enclosing scope, and none defines one"
                )
            );
            return Ok(Outcome::SelfUnknown);
        }
        let l = got.map_err(|e| TestCaseError::fail(format!("{d:?}: {e}")))?;
        let full = reg
            .resolver()
            .lineage(&r_name)
            .map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&full.merged().xmlid, &Field::Set("local".to_owned()));
        if c.self_named {
            prop_assert_eq!(l, vec![global, local, r_name]);
            return Ok(Outcome::LocalOverGlobal);
        }
        prop_assert_eq!(l, vec![local, r_name]);
        return Ok(Outcome::Local);
    }
    // Outside the local scope: the local `x` is never found.
    if c.global {
        prop_assert_eq!(found.as_ref(), Some(&global), "{} from {}", X, from);
        let l = got.map_err(|e| TestCaseError::fail(format!("{d:?}: {e}")))?;
        prop_assert!(!l.contains(&local), "{d:?} saw the local {X}: {l:?}");
        prop_assert_eq!(l, vec![global, r_name.clone()]);
        let merged = reg
            .resolver()
            .lineage(&r_name)
            .map(|l| l.merged().xmlid.clone())
            .ok();
        prop_assert_eq!(merged, Some(Field::Set("global".to_owned())));
        return Ok(Outcome::Global);
    }
    prop_assert_eq!(found, None, "{} from {}", X, from);
    let err = got
        .err()
        .ok_or_else(|| TestCaseError::fail(format!("{d:?} resolved")))?;
    check_hidden(&err, &r_name, r.layer(), X, &[local])?;
    Ok(Outcome::Hidden)
}

/// Checks a whole case and records what it covered in `seen`.
fn check_case(c: &Case, seen: &RefCell<Seen>) -> Result<(), TestCaseError> {
    let reg = registry_in(&c.docs, &c.order).map_err(TestCaseError::fail)?;
    let canonical: Vec<usize> = (0..c.docs.len()).collect();
    let in_order = registry_in(&c.docs, &canonical).map_err(TestCaseError::fail)?;
    prop_assert_eq!(&reg, &in_order, "the registry depends on insertion order");
    let mut s = seen.borrow_mut();
    for d in Direction::ALL {
        let outcome = check_direction(c, &reg, d)?;
        s.outcomes.insert((d, outcome));
    }
    // One warm resolver, asked in the opposite order, agrees.
    let mut warm = reg.resolver();
    for d in Direction::ALL.iter().rev() {
        let name = c.referrer(*d).name();
        let a = warm.lineage(name).map(|l| l.names().to_vec());
        prop_assert_eq!(a, lineage(&in_order, name));
    }
    // The background family resolves as it does alone: the local `x` and
    // its referrers leak into none of it.
    let bg: Vec<usize> = (0..c.n).collect();
    let alone = registry_in(&c.docs, &bg).map_err(TestCaseError::fail)?;
    for doc in &c.docs[..c.n] {
        prop_assert_eq!(lineage(&reg, doc.name()), lineage(&alone, doc.name()));
    }
    let (l, g) = c.x_at();
    if let Some(g) = g {
        let pos = |i| c.order.iter().position(|&x| x == i);
        s.local_first |= pos(l) < pos(g);
        s.global_first |= pos(g) < pos(l);
    }
    s.depths.insert(c.local.len());
    s.bgs.insert(c.bg);
    Ok(())
}

#[derive(Debug, Default)]
struct Seen {
    outcomes: BTreeSet<(Direction, Outcome)>,
    bgs: BTreeSet<Bg>,
    depths: BTreeSet<usize>,
    local_first: bool,
    global_first: bool,
}

/// MF-16 over every reference direction: a local name resolves from its
/// own scope and the scopes inside it, and nowhere else: from the parent,
/// a sibling, the same table in another ROM and the global scope it is an
/// `UnknownBase` naming where it is defined, or, when a global document
/// has the same id, that global document's values (legacy's
/// `xml/TableScaleUnmarshaller.java:460-461` put made the local one win,
/// whenever it came later). Insertion order, resolution order and the
/// background family change nothing.
#[test]
fn mf16_local_names_never_leak_outward() {
    let seen = RefCell::new(Seen::default());
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: 256,
            ..Config::default()
        },
        TestRng::deterministic_rng(RngAlgorithm::ChaCha),
    );
    if let Err(e) = runner.run(&case(), |c| check_case(&c, &seen)) {
        panic!("{e}");
    }
    let seen = seen.into_inner();
    // Every direction meets every outcome its side of the scope allows.
    let mut want = BTreeSet::new();
    for d in Direction::ALL {
        let outcomes: &[Outcome] = if d.inside() {
            &[
                Outcome::Local,
                Outcome::LocalOverGlobal,
                Outcome::SelfUnknown,
            ]
        } else {
            &[Outcome::Global, Outcome::Hidden]
        };
        want.extend(outcomes.iter().map(|&o| (d, o)));
    }
    assert_eq!(seen.outcomes, want);
    assert_eq!(seen.bgs.len(), 6, "{:?}", seen.bgs);
    assert_eq!(seen.depths, [2, 3].into());
    assert!(seen.local_first && seen.global_first, "{seen:?}");
}

// ---------------------------------------------------------------------------
// unknown_base's MF-16 variant
// ---------------------------------------------------------------------------

proptest! {
    /// MF-16 on [`unknown_base`]'s local-only variant: a document naming,
    /// by its bare (global) spelling, an id only a local document has gets
    /// an `UnknownBase` whose diagnostic names the local scope; given a
    /// global document with that id as well, inserted before or after
    /// everything else, it resolves to the global document and never the
    /// local one.
    #[test]
    fn mf16_a_local_only_id_is_unknown_until_defined_globally(
        shape in prop_oneof![unknown_base(sample::rom()), scoped(unknown_base(sample::rom()))]
            .prop_filter("the MF-16 variant", |s| s.local.is_some()),
        global_first in any::<bool>(),
    ) {
        let f = &shape.family;
        let l = shape.local.ok_or_else(|| TestCaseError::fail("no local"))?;
        let local = f.name(l).clone();
        prop_assert!(!local.scope().is_global());
        prop_assert_eq!(local.id(), &shape.base);
        let doc = &f.docs()[shape.document];
        let reg = f.registry().map_err(|e| TestCaseError::fail(e.to_string()))?;
        let err = lineage(&reg, f.name(shape.entry))
            .err()
            .ok_or_else(|| TestCaseError::fail("resolved"))?;
        check_hidden(&err, doc.name(), doc.layer(), shape.base.as_str(), std::slice::from_ref(&local))?;

        // The same id defined globally: everything outside the local scope
        // now finds the global document, wherever it is inserted.
        let mut docs = f.docs().to_vec();
        let g = Document::new(layer(shape.base.as_str(), "defs/global.xml"), rom("global"));
        let global = g.name().clone();
        docs.push(g);
        let n = docs.len();
        let order: Vec<usize> = if global_first {
            std::iter::once(n - 1).chain(0..n - 1).collect()
        } else {
            (0..n).collect()
        };
        let with = registry_in(&docs, &order).map_err(TestCaseError::fail)?;
        for i in [shape.document, shape.entry] {
            let got = lineage(&with, f.name(i)).map_err(|e| TestCaseError::fail(e.to_string()))?;
            prop_assert!(got.contains(&global), "{got:?}");
            prop_assert!(!got.contains(&local), "{} saw {local}: {got:?}", f.name(i));
        }
        // The local document itself is untouched.
        prop_assert_eq!(lineage(&with, &local), lineage(&reg, &local));
    }
}

// ---------------------------------------------------------------------------
// The extend-outer idiom
// ---------------------------------------------------------------------------

/// MF-16 on [`extending`]: the extensions of an outer document (local
/// documents with its id naming that id as a base) are never seen from
/// outside their scopes. From the outer document's scope (the shape's
/// `outside` document) and from a sibling of the first extension, a
/// reference to the id finds the outer document alone; from the global
/// scope, the outer document if it is global, else an `UnknownBase` naming
/// the outer document's and every extension's scope. Insertion order
/// changes nothing. Counters fail the test unless both global-scope
/// outcomes and both extension depths occur.
#[test]
fn mf16_the_extend_outer_idiom_never_exposes_an_extension() {
    let seen = RefCell::new(BTreeSet::new());
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: 256,
            ..Config::default()
        },
        TestRng::deterministic_rng(RngAlgorithm::ChaCha),
    );
    let strategy = extending(sample::rom()).prop_flat_map(|s| {
        let n = s.family.len() + 2;
        (Just(s), Just((0..n).collect::<Vec<_>>()).prop_shuffle())
    });
    let result = runner.run(&strategy, |(shape, order)| {
        let f = &shape.family;
        let outer = f.name(shape.outer).clone();
        let id = outer.id().clone();
        let ext: Vec<Name> = f.names(&shape.extensions);
        let sibling_scope = outer.scope().child("MF16-SIBLING");
        let mut docs = f.docs().to_vec();
        let from_global = Document::new(layer("mf16-from-global", "defs/g.xml"), Rom::default())
            .with_base(id.clone());
        let from_sibling = Document::new(layer("mf16-from-sibling", "defs/s.xml"), Rom::default())
            .in_scope(sibling_scope)
            .with_base(id.clone());
        let (g, s) = (from_global.name().clone(), from_sibling.name().clone());
        docs.push(from_global.clone());
        docs.push(from_sibling);
        let reg = registry_in(&docs, &order).map_err(TestCaseError::fail)?;
        let canonical: Vec<usize> = (0..docs.len()).collect();
        prop_assert_eq!(
            &reg,
            &registry_in(&docs, &canonical).map_err(TestCaseError::fail)?
        );
        let base = lineage(&reg, &outer);
        let mut s_seen = seen.borrow_mut();
        s_seen.insert(format!("levels {}", ext.len()));
        let mut referrers = vec![(f.name(shape.outside).clone(), "outside"), (s, "sibling")];
        if outer.scope().is_global() {
            referrers.push((g.clone(), "global"));
        } else {
            // Every document with the id is outside the global scope.
            let mut hidden = vec![outer.clone()];
            hidden.extend(ext.iter().cloned());
            hidden.sort();
            let err = lineage(&reg, &g)
                .err()
                .ok_or_else(|| TestCaseError::fail("resolved from the global scope"))?;
            check_hidden(&err, &g, from_global.layer(), id.as_str(), &hidden)?;
            s_seen.insert("global: hidden".to_owned());
        }
        for (r, what) in referrers {
            let got = lineage(&reg, &r);
            if let Ok(l) = &got {
                prop_assert!(
                    ext.iter().all(|e| !l.contains(e)),
                    "{what} saw an extension: {l:?}"
                );
            }
            match &base {
                Ok(b) => {
                    let mut want = b.clone();
                    want.push(r.clone());
                    prop_assert_eq!(got, Ok(want), "{}", what);
                    s_seen.insert(format!("{what}: outer"));
                }
                Err(_) => prop_assert!(got.is_err(), "{what}: {got:?}"),
            }
        }
        Ok(())
    });
    if let Err(e) = result {
        panic!("{e}");
    }
    let seen = seen.into_inner();
    let want: BTreeSet<String> = [
        "levels 1",
        "levels 2",
        "outside: outer",
        "sibling: outer",
        "global: outer",
        "global: hidden",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(seen, want);
}

// ---------------------------------------------------------------------------
// The legacy failure
// ---------------------------------------------------------------------------

/// A scaling document: `expr` set, or (a user of a base) only `units`.
fn scaling(id: &str, file: &str, expr: Option<&str>) -> Document<Scaling> {
    let doc = Scaling {
        expr: expr.map_or(Field::Unset, |e| Field::Set(e.to_owned())),
        units: Field::Set("AFR".to_owned()),
        ..Scaling::default()
    };
    Document::new(layer(id, file), doc)
}

/// The legacy case by hand: a definition with a global `AFR` scaling base,
/// a table *Fuel Map* defining its own `AFR` scaling, and a table *Timing*
/// whose scaling names base `AFR`. Legacy's `scales.put` for Fuel Map's
/// table-local `AFR` (`xml/TableScaleUnmarshaller.java:276-277` then
/// `:460-461`) replaced the global entry, and Timing's lookup
/// (`:419-421`) deep-copied Fuel Map's expression; without the global
/// `AFR` it still found Fuel Map's. Here Timing gets the global `AFR`'s
/// expression in every insertion order, and an `UnknownBase` naming
/// `A2WC522N/Fuel Map` without it, while Fuel Map's own scaling still gets
/// its local `AFR`.
#[test]
fn mf16_legacy_table_local_scaling_does_not_replace_the_global_one() {
    let fuel = scope(&["A2WC522N", "Fuel Map"]);
    let timing = scope(&["A2WC522N", "Timing"]);
    let file = "defs/A2WC522N.xml";
    let global = scaling("AFR", "defs/base.xml", Some("x*0.1"));
    let local = scaling("AFR", file, Some("14.7/x")).in_scope(fuel.clone());
    let fuel_user = scaling("Fuel Map", file, None)
        .in_scope(fuel.clone())
        .with_base("AFR");
    let timing_user = scaling("Timing", file, None)
        .in_scope(timing)
        .with_base("AFR");
    let (t, fu) = (timing_user.name().clone(), fuel_user.name().clone());
    let docs = [global, local, fuel_user, timing_user];
    let expr = |l: &rr_core::layer::Lineage<Scaling>| l.merged().expr.clone();

    // Global first (legacy: the local `put` later wins), local first, and
    // the users first.
    for order in [[0, 1, 2, 3], [1, 2, 3, 0], [3, 2, 1, 0]] {
        let reg = registry_in(&docs, &order).expect("unique names");
        let l = reg.resolver().lineage(&t).expect("Timing resolves");
        assert_eq!(l.names(), [Name::global("AFR"), t.clone()], "{order:?}");
        assert_eq!(expr(&l), Field::Set("x*0.1".to_owned()), "{order:?}");
        let f = reg.resolver().lineage(&fu).expect("Fuel Map resolves");
        assert_eq!(
            f.names(),
            [Name::new(fuel.clone(), "AFR"), fu.clone()],
            "{order:?}"
        );
        assert_eq!(expr(&f), Field::Set("14.7/x".to_owned()), "{order:?}");
    }

    // No global `AFR`: Timing's base is unknown, and the message says where
    // an `AFR` is defined instead.
    let reg = registry_in(&docs, &[1, 2, 3]).expect("unique names");
    let err = reg
        .resolver()
        .lineage(&t)
        .expect_err("Fuel Map's AFR is local");
    assert_eq!(err.code(), codes::UNKNOWN_BASE);
    assert_eq!(
        err.to_string(),
        "`A2WC522N/Timing::Timing` names base `AFR`, which is not defined here: `AFR` is \
         defined only inside `A2WC522N/Fuel Map`; not visible from `A2WC522N/Timing::Timing`"
    );
    assert!(reg.resolver().lineage(&fu).is_ok());
}
