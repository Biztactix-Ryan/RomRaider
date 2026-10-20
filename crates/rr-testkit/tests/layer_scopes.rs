//! Scoped namespaces (US-RR-22-10; MF-16).
//!
//! A base reference is a bare id, looked up from the referrer's scope
//! outward to the global scope (`Registry::lookup`), so a table can use a
//! scaling local to itself, to its ROM, or a global one, and a local name
//! is never visible from outside its scope. Legacy kept one flat map of
//! scalings for the whole definition and put every table-local `<scaling>`
//! in it (`xml/TableScaleUnmarshaller.java:276-277`, `:460-461`), where
//! every later `base` lookup found it (`:419-421`).
//!
//! A base with the document's own id extends the outer definition of that
//! id (`Registry::base_of`), as legacy's lookup-before-register did
//! (`xml/TableScaleUnmarshaller.java:415-421` before `:460-461`).
//!
//! Legacy references are to RomRaider's Java sources under
//! `legacy/src/main/java/com/romraider` (read, never run; D-024).

use proptest::prelude::*;
use proptest::sample::select;
use rr_core::diag::{Severity, codes};
use rr_core::layer::{Document, Layer, LayerKind, Merge, Name, Registry, ResolveError, Scope};
use rr_testkit::strategies::layered::family::{ROM_SEGMENTS, TABLE_SEGMENTS};
use rr_testkit::strategies::layered::sample::{self, Rom};
use rr_testkit::strategies::layered::{Family, dag, diamond, extending, scoped, shadowing};

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

/// The scope `path`, outermost segment first.
fn scope(path: &[&str]) -> Scope {
    path.iter().fold(Scope::global(), |s, seg| s.child(*seg))
}

/// Every scope a document can be placed in for the visibility tests: the
/// global scope, two ROMs, and two tables in one of them and one in the
/// other, so each scope has a parent, a sibling and an unrelated scope.
fn scope_pool() -> Vec<Scope> {
    vec![
        scope(&[]),
        scope(&["A2WC400H"]),
        scope(&["A2WC400H", "Boost Target"]),
        scope(&["A2WC400H", "Fuel Map"]),
        scope(&["A2WC522N"]),
        scope(&["A2WC522N", "Boost Target"]),
    ]
}

fn layer(id: &str, file: &str) -> Layer {
    Layer::new(
        id.into(),
        LayerKind::File,
        file,
        rr_core::diag::Span::line_col(1, 1),
    )
}

/// The lineage of `name` as indices into `family`.
fn indices<P: Merge + Clone>(
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

/// The last document of `family` whose scope encloses `from`: one a
/// reference from `from` can see.
fn visible_from(family: &Family<Rom>, from: &Scope) -> Option<usize> {
    (0..family.len())
        .rev()
        .find(|&i| family.name(i).scope().encloses(from))
}

/// Adds to `family`'s registry a local document `scale` in `local` and a
/// document `ref` in `from` naming it (and, as its weaker base, the
/// [`visible_from`] document, if any): the name of `ref`.
fn with_local_and_ref(family: &Family<Rom>, local: &Scope, from: &Scope) -> (Registry<Rom>, Name) {
    let mut docs: Vec<Document<Rom>> = family.docs().to_vec();
    docs.push(
        Document::new(layer("scale", "defs/local.xml"), Rom::default()).in_scope(local.clone()),
    );
    let r = visible_from(family, from)
        .into_iter()
        .fold(
            Document::new(layer("ref", "defs/ref.xml"), Rom::default()).in_scope(from.clone()),
            |d, v| d.with_base(family.name(v).id().clone()),
        )
        .with_base("scale");
    let name = r.name().clone();
    docs.push(r);
    (registry_of(docs), name)
}

/// The visibility rule on one local document and one reference: found iff
/// the local document's scope encloses the reference's (its own scope or
/// an enclosing one); otherwise an `UnknownBase` naming where it is.
fn check_visibility(
    family: &Family<Rom>,
    local: &Scope,
    from: &Scope,
) -> Result<(), TestCaseError> {
    let (reg, r) = with_local_and_ref(family, local, from);
    let got = reg.resolver().lineage(&r);
    let scale = Name::new(local.clone(), "scale");
    if local.encloses(from) {
        let l = got.map_err(|e| TestCaseError::fail(format!("{local} from {from}: {e}")))?;
        prop_assert_eq!(l.names().iter().rev().nth(1), Some(&scale));
        // An outer document is found from any depth inside it.
        if let Some(v) = visible_from(family, from) {
            prop_assert!(l.names().contains(family.name(v)));
        }
        return Ok(());
    }
    let err = match got {
        Err(err) => err,
        Ok(l) => {
            let names: Vec<String> = l.names().iter().map(ToString::to_string).collect();
            return Err(TestCaseError::fail(format!(
                "{local} from {from}: resolved {names:?}"
            )));
        }
    };
    let ResolveError::UnknownBase {
        document,
        base,
        hidden,
        ..
    } = &err
    else {
        return Err(TestCaseError::fail(format!("{local} from {from}: {err:?}")));
    };
    prop_assert_eq!(document, &r);
    prop_assert_eq!(base.as_str(), "scale");
    prop_assert_eq!(&**hidden, &[scale][..]);
    let diag = err.to_diagnostic(layer("x", "x").provenance());
    prop_assert_eq!(diag.code(), codes::UNKNOWN_BASE);
    prop_assert_eq!(diag.severity(), Severity::Error);
    prop_assert_eq!(diag.provenance().source(), "defs/ref.xml");
    let inside = format!("defined only inside `{local}`; not visible from `{r}`");
    prop_assert!(diag.message().ends_with(&inside), "{}", diag.message());
    Ok(())
}

/// MF-16, exhaustively over the scope pool: a local name is visible from
/// its own scope and every scope inside it, never from its parent, a
/// sibling, an unrelated scope or the global scope. Legacy made every
/// table-local scaling a base for the whole definition
/// (`xml/TableScaleUnmarshaller.java:460-461`).
#[test]
fn mf16_a_local_name_is_visible_only_inside_its_scope() {
    let mut runner = proptest::test_runner::TestRunner::deterministic();
    let family = dag(sample::rom(), 1..=3)
        .new_tree(&mut runner)
        .map(|t| proptest::strategy::ValueTree::current(&t));
    let family = family.expect("a family");
    let pool = scope_pool();
    let mut seen = (0, 0);
    for local in pool.iter().filter(|s| !s.is_global()) {
        for from in &pool {
            if local.encloses(from) {
                seen.0 += 1;
            } else {
                seen.1 += 1;
            }
            if let Err(e) = check_visibility(&family, local, from) {
                panic!("{e}");
            }
        }
    }
    // Visible: each local scope from itself (5), and each ROM scope from
    // its tables (2 + 1). Every other pair is hidden.
    assert_eq!(seen, (8, 22));
}

proptest! {
    /// MF-16 on generated families in nested scopes: whatever the family
    /// and wherever the local document and the reference are, the local
    /// document is found exactly when its scope encloses the reference's,
    /// and an inner scope always sees the global (outer) documents.
    #[test]
    fn mf16_visibility_is_lexical(
        family in prop_oneof![dag(sample::rom(), 1..=6), scoped(dag(sample::rom(), 1..=6))],
        local in select(scope_pool()[1..].to_vec()),
        from in select(scope_pool()),
    ) {
        check_visibility(&family, &local, &from)?;
    }

    /// A local document with a global one's id shadows it inside its scope
    /// only: the inner document gets the local one (and, when the local one
    /// extends the global one by naming its own id, the global one below
    /// it), the outer document the global one; the registry warns (LAY0008)
    /// exactly when the global document's scope encloses the local one's
    /// and the local one does not extend it, whatever the insertion order.
    #[test]
    fn shadowing_is_innermost_inside_and_warned(
        shape in prop_oneof![shadowing(sample::rom()), scoped(shadowing(sample::rom()))]
    ) {
        let f = &shape.family;
        let reg = f.registry().map_err(|e| TestCaseError::fail(e.to_string()))?;
        let inner = if shape.extends {
            vec![shape.global, shape.local, shape.inner]
        } else {
            vec![shape.local, shape.inner]
        };
        prop_assert_eq!(indices(f, &reg, f.name(shape.inner)), Ok(inner));
        prop_assert_eq!(indices(f, &reg, f.name(shape.outer)), Ok(vec![shape.global, shape.outer]));
        let (g, l) = (f.name(shape.global), f.name(shape.local));
        if shape.extends {
            prop_assert!(g.scope().is_global(), "scoped keeps an extended document visible");
        }
        let shadowed = reg.shadowed();
        if g.scope().encloses(l.scope()) && !shape.extends {
            prop_assert_eq!(shadowed.len(), 1);
            let s = &shadowed[0];
            prop_assert_eq!(&s.name, l);
            prop_assert_eq!(&s.shadows, g);
            prop_assert_eq!(&s.layer, f.docs()[shape.local].layer());
            prop_assert_eq!(&s.outer, f.docs()[shape.global].layer());
            let d = s.to_diagnostic();
            prop_assert_eq!(d.severity(), Severity::Warning);
            prop_assert_eq!(d.code(), codes::SHADOWED_NAME);
            prop_assert_eq!(d.provenance(), &f.docs()[shape.local].layer().provenance());
        } else {
            prop_assert!(shadowed.is_empty(), "{shadowed:?}");
        }
        let reversed: Vec<usize> = (0..f.len()).rev().collect();
        let backward = f.registry_in_order(&reversed).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(backward.shadowed(), shadowed);
    }

    /// Moving documents into nested scopes, each base staying in an
    /// enclosing scope of its referrers, changes no resolution when no two
    /// documents share an id: the same lineage (by document), the same
    /// merged document, the same finished value, no shadowing; and the
    /// result is deterministic (insertion order, a warm cache and resolving
    /// twice change nothing).
    #[test]
    fn scoping_without_collisions_changes_no_resolution(
        family in prop_oneof![
            dag(sample::rom(), 1..=8),
            diamond(sample::rom()).prop_map(|d| d.family),
        ],
        depth in proptest::collection::vec(0..=2_usize, 13),
        rom in select(ROM_SEGMENTS.to_vec()),
        table in select(TABLE_SEGMENTS.to_vec()),
    ) {
        let rom_scope = Scope::global().child(rom);
        let chain = [Scope::global(), rom_scope.clone(), rom_scope.child(table)];
        let moved = family.rescoped(&depth, &chain);
        let flat = family.registry().map_err(|e| TestCaseError::fail(e.to_string()))?;
        let reg = moved.registry().map_err(|e| TestCaseError::fail(e.to_string()))?;
        let reversed: Vec<usize> = (0..moved.len()).rev().collect();
        let backward = moved.registry_in_order(&reversed).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&backward, &reg);
        prop_assert!(reg.shadowed().is_empty());
        let mut warm = backward.resolver();
        for i in 0..family.len() {
            let a = flat.resolver().lineage(family.name(i));
            let b = reg.resolver().lineage(moved.name(i));
            let (a, b) = match (a, b) {
                (Ok(a), Ok(b)) => (a, b),
                (a, b) => {
                    return Err(TestCaseError::fail(format!("{}: {a:?} vs {b:?}", moved.name(i))));
                }
            };
            let by_index = |f: &Family<Rom>, names: &[Name]| -> Vec<Option<usize>> {
                names.iter().map(|n| f.index_of(n)).collect()
            };
            prop_assert_eq!(by_index(&family, a.names()), by_index(&moved, b.names()));
            prop_assert_eq!(a.merged(), b.merged());
            let fa = flat.resolver().resolve(family.name(i)).map(|r| r.into_value()).ok();
            let fb = reg.resolver().resolve(moved.name(i)).map(|r| r.into_value()).ok();
            prop_assert_eq!(fa, fb);
            let cached = warm.lineage(moved.name(i)).map_err(|e| TestCaseError::fail(e.to_string()))?;
            let again = warm.lineage(moved.name(i)).map_err(|e| TestCaseError::fail(e.to_string()))?;
            prop_assert_eq!(&*cached, &*b);
            prop_assert_eq!(&*again, &*b);
        }
    }

    /// The extend-outer idiom is exactly the explicit form: for random
    /// scoped families with one or two levels of local documents naming
    /// their own id as a base, resolving every document gives the same
    /// lineage (by document), the same merged document and the same
    /// finished value, or the same error, as the family with each extension
    /// renamed and its base pointed at the outer name explicitly. Both match
    /// the reference C3 oracle; neither warns (LAY0008).
    #[test]
    fn extending_an_outer_name_equals_naming_it_explicitly(shape in extending(sample::rom())) {
        let f = &shape.family;
        let e = shape.explicit();
        prop_assert_eq!(e.len(), f.len());
        for d in e.docs() {
            prop_assert!(!d.bases().contains(d.name().id()), "{} still names itself", d.name());
        }
        for &x in &shape.extensions {
            prop_assert_eq!(f.name(x).id(), f.name(shape.outer).id());
            prop_assert!(f.docs()[x].bases().contains(f.name(x).id()));
            prop_assert_ne!(e.name(x).id(), f.name(x).id());
        }
        let reg = f.registry().map_err(|err| TestCaseError::fail(err.to_string()))?;
        let explicit = e.registry().map_err(|err| TestCaseError::fail(err.to_string()))?;
        prop_assert!(reg.shadowed().is_empty(), "{:?}", reg.shadowed());
        prop_assert!(explicit.shadowed().is_empty());
        for i in 0..f.len() {
            let a = indices(f, &reg, f.name(i));
            let b = indices(&e, &explicit, e.name(i));
            let reference = f.reference_lineage(i);
            prop_assert_eq!(a.as_ref().ok(), reference.as_ref(), "{}: {:?}", f.name(i), a);
            match (&a, &b) {
                (Ok(a), Ok(b)) => prop_assert_eq!(a, b, "{}", f.name(i)),
                (Err(a), Err(b)) => prop_assert_eq!(a.code(), b.code(), "{}", f.name(i)),
                _ => prop_assert!(false, "{}: {a:?} vs {b:?}", f.name(i)),
            }
            let ma = reg.resolver().lineage(f.name(i)).map(|l| l.merged().clone()).ok();
            let mb = explicit.resolver().lineage(e.name(i)).map(|l| l.merged().clone()).ok();
            prop_assert_eq!(ma, mb);
            let va = reg.resolver().resolve(f.name(i)).map(|r| r.into_value()).ok();
            let vb = explicit.resolver().resolve(e.name(i)).map(|r| r.into_value()).ok();
            prop_assert_eq!(va, vb);
        }
        // Inside, the id means the innermost extension, over every outer
        // one; outside, the outer document alone (MF-16).
        if let Ok(user) = indices(f, &reg, f.name(shape.user)) {
            let at = |i: usize| user.iter().position(|&x| x == i);
            let chain: Vec<Option<usize>> = std::iter::once(shape.outer)
                .chain(shape.extensions.iter().copied())
                .chain([shape.user])
                .map(at)
                .collect();
            prop_assert!(chain.iter().all(Option::is_some), "{user:?}");
            prop_assert!(chain.windows(2).all(|w| w[0] < w[1]), "{user:?}");
        }
        if let Ok(outside) = indices(f, &reg, f.name(shape.outside)) {
            prop_assert!(outside.contains(&shape.outer));
            prop_assert!(shape.extensions.iter().all(|x| !outside.contains(x)));
        }
    }
}

/// The extend-outer shape covers what it claims: one and two levels, outer
/// documents in the global, ROM and table scopes, second bases on either
/// side, and mostly families that resolve.
#[test]
fn extending_covers_every_level() {
    let mut runner = proptest::test_runner::TestRunner::deterministic();
    let strategy = extending(sample::rom());
    let mut levels = std::collections::BTreeSet::new();
    let mut outer_depths = std::collections::BTreeSet::new();
    let (mut resolved, mut second) = (0, 0);
    for _ in 0..200 {
        let shape = proptest::strategy::ValueTree::current(
            &strategy.new_tree(&mut runner).expect("a shape"),
        );
        let f = &shape.family;
        levels.insert(shape.extensions.len());
        outer_depths.insert(f.name(shape.outer).scope().depth());
        second += shape
            .extensions
            .iter()
            .filter(|&&x| f.docs()[x].bases().len() == 2)
            .count();
        let reg = f.registry().expect("unique names");
        if reg.resolver().lineage(f.name(shape.user)).is_ok() {
            resolved += 1;
        }
    }
    assert_eq!(levels, [1, 2].into());
    assert_eq!(outer_depths, [0, 1, 2].into());
    assert!(second > 0);
    assert!(resolved > 150, "{resolved}");
}
