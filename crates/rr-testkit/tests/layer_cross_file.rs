//! Cross-file references and cycle diagnostics (US-RR-22-9; MF-17).
//!
//! Legacy looked a ROM definition's `base` up only in the file naming it
//! (`xml/DOMRomUnmarshaller.java:149-152`, `RomNotFoundException` at
//! `:213` otherwise) and caught a looped base as a `StackOverflowError`
//! (`editor/ecu/OpenImageWorker.java:178-184`). Here a registry holds the
//! documents of every file, a base resolves in whichever file defines it,
//! the split between files never changes a result, and a cycle across files
//! is one Error diagnostic naming the whole cycle with each document's file.
//!
//! The generated families put every document in a file of its own
//! (`defs/<i>-<id>.xml`); [`one_file`] moves them all into one, which is the
//! single-file version legacy could resolve.

use std::sync::Arc;

use proptest::prelude::*;
use rr_core::diag::{Diagnostics, Provenance, Severity, Span, codes};
use rr_core::layer::{
    Document, Field, Layer, LayerKind, Merge, Name, Registry, ResolveError, Resolver,
};
use rr_testkit::strategies::layered::sample::{self, Rom};
use rr_testkit::strategies::layered::{Family, chain, cyclic_registry, dag, diamond, scoped};

/// Where a request for a document comes from, in these tests: the ROM image
/// being opened.
fn request() -> Provenance {
    Provenance::new("image.bin", Span::byte_offset(0))
}

/// The family's documents, unchanged except that every one is in the file
/// `defs/pack.xml` (same id, kind, span, scope, bases and document).
fn one_file<P>(family: &Family<P>) -> Vec<Document<P>> {
    family
        .docs()
        .iter()
        .map(|d| {
            let l = d.layer();
            let layer = Layer::new(l.id().clone(), l.kind(), "defs/pack.xml", l.span());
            let moved =
                Document::new(layer, Arc::clone(d.doc())).in_scope(d.name().scope().clone());
            d.bases().iter().cloned().fold(moved, Document::with_base)
        })
        .collect()
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

/// Every document of `family` resolves identically whether its documents
/// are in one file each or all in one file: the same lineage, the same
/// merged partial, the same completion, and layers that differ only in
/// their file.
fn split_changes_nothing(family: &Family<Rom>) -> Result<(), TestCaseError> {
    let files = family
        .registry()
        .map_err(|e| TestCaseError::fail(e.to_string()))?;
    let single = registry_of(one_file(family));
    let (mut many, mut one) = (Resolver::new(&files), Resolver::new(&single));
    for d in family.docs() {
        let a = many.lineage(d.name());
        let b = one.lineage(d.name());
        let (a, b) = match (a, b) {
            (Ok(a), Ok(b)) => (a, b),
            other => return Err(TestCaseError::fail(format!("{}: {other:?}", d.name()))),
        };
        prop_assert_eq!(a.names(), b.names());
        prop_assert_eq!(a.merged(), b.merged());
        prop_assert_eq!(a.stack().len(), b.stack().len());
        for ((la, da), (lb, db)) in a.stack().layers().zip(b.stack().layers()) {
            prop_assert_eq!(
                (la.id(), la.kind(), la.span()),
                (lb.id(), lb.kind(), lb.span())
            );
            prop_assert_eq!(lb.source(), "defs/pack.xml");
            prop_assert!(
                Arc::ptr_eq(da, db),
                "both stacks hold the registered document"
            );
        }
        let (ca, cb) = (many.resolve(d.name()), one.resolve(d.name()));
        prop_assert_eq!(
            ca.as_ref().map(|r| r.value()).map_err(ResolveError::code),
            cb.as_ref().map(|r| r.value()).map_err(ResolveError::code)
        );
    }
    Ok(())
}

/// The message a cycle diagnostic must have: each document on the cycle
/// with its file and span, in base order, then the first again.
fn cycle_message<P>(family: &Family<P>, cycle: &[usize]) -> String {
    let mut out = String::from("inheritance cycle: ");
    for &c in cycle {
        let l = family.docs()[c].layer();
        out.push_str(&format!(
            "{} ({}{}) -> ",
            family.name(c),
            l.source(),
            l.span()
        ));
    }
    if let Some(&first) = cycle.first() {
        out.push_str(&family.name(first).to_string());
    }
    out
}

proptest! {
    /// Valid shapes resolve the same split across files as in one file.
    #[test]
    fn cross_file_chains_resolve_as_in_one_file(family in chain(sample::rom(), 1..=8)) {
        split_changes_nothing(&family)?;
    }

    #[test]
    fn cross_file_dags_resolve_as_in_one_file(
        family in prop_oneof![dag(sample::rom(), 1..=10), scoped(dag(sample::rom(), 1..=10))]
    ) {
        split_changes_nothing(&family)?;
    }

    /// EC-04 across files: the shared ancestor, in a file of its own, is
    /// applied once, below every side, exactly as in one file.
    #[test]
    fn cross_file_diamonds_resolve_as_in_one_file(shape in diamond(sample::rom())) {
        let f = &shape.family;
        split_changes_nothing(f)?;
        let reg = registry_of(f.docs().iter().cloned());
        let top = reg.resolver().lineage(f.name(shape.top));
        let names = top.map_err(|e| TestCaseError::fail(e.to_string()))?.names().to_vec();
        let apex = names.iter().filter(|n| *n == f.name(shape.apex)).count();
        prop_assert_eq!(apex, 1);
    }

    /// MF-17: a cycle of `k` documents in `k` files is exactly one
    /// diagnostic: an Error, code LAY0001, at the document whose base closes
    /// the cycle, naming the expected path in order with each file.
    #[test]
    fn a_cycle_across_files_is_one_error_naming_the_path(
        shape in prop_oneof![cyclic_registry(sample::rom(), 1..=12), scoped(cyclic_registry(sample::rom(), 1..=12))]
    ) {
        let f = &shape.family;
        let reg = registry_of(f.docs().iter().cloned());
        let mut diags = Diagnostics::new();
        let err = reg.resolver().lineage(f.name(shape.entry));
        match &err {
            Err(e @ ResolveError::Cycle { path, .. }) => {
                prop_assert_eq!(path, &shape.expected_path());
                diags.push(e.to_diagnostic(request()));
            }
            other => prop_assert!(false, "expected a cycle, got {other:?}"),
        }
        prop_assert_eq!(diags.len(), 1);
        prop_assert_eq!(diags.error_count(), 1);
        let d = &diags.as_slice()[0];
        prop_assert_eq!(d.severity(), Severity::Error);
        prop_assert_eq!(d.code(), codes::INHERITANCE_CYCLE);
        prop_assert_eq!(d.provenance(), &f.docs()[shape.closing()].layer().provenance());
        prop_assert_eq!(d.message(), cycle_message(f, &shape.cycle));
        // Every document on the cycle reports the same cycle, from itself.
        for (j, &c) in shape.cycle.iter().enumerate() {
            let err = reg.resolver().lineage(f.name(c));
            let mut rotated = shape.cycle[j..].to_vec();
            rotated.extend_from_slice(&shape.cycle[..j]);
            let d = err.as_ref().map_err(|e| e.to_diagnostic(request()));
            prop_assert!(d.is_err(), "{} resolved", f.name(c));
            if let Err(d) = d {
                prop_assert_eq!(d.code(), codes::INHERITANCE_CYCLE);
                prop_assert_eq!(d.message(), cycle_message(f, &rotated));
            }
        }
    }

    /// The same name in two files: the registry keeps whichever was loaded
    /// first and refuses the other with an Error naming both locations;
    /// what resolves is exactly what resolved before the duplicate.
    #[test]
    fn a_duplicate_name_across_files_is_an_error_and_keeps_the_first(
        (family, pick) in dag(sample::rom(), 1..=8).prop_flat_map(|f| {
            let n = f.len();
            (Just(f), 0..n)
        }),
        line in 1..=999_u32,
    ) {
        let original = &family.docs()[pick];
        let l = original.layer();
        let copy_layer = Layer::new(l.id().clone(), LayerKind::User, "user/override.xml", Span::line_col(line, 1));
        let copy = Document::new(copy_layer.clone(), Rom::default())
            .in_scope(original.name().scope().clone());

        let mut reg = registry_of(family.docs().iter().cloned());
        let before = reg.clone();
        let err = reg.insert(copy.clone());
        let dup = match err {
            Err(dup) => dup,
            Ok(()) => return Err(TestCaseError::fail("duplicate accepted")),
        };
        prop_assert_eq!(&dup.name, original.name());
        prop_assert_eq!(&dup.kept, l);
        prop_assert_eq!(&dup.rejected, &copy_layer);
        prop_assert_eq!(&reg, &before);
        let d = dup.to_diagnostic();
        prop_assert_eq!(d.severity(), Severity::Error);
        prop_assert_eq!(d.code(), codes::DUPLICATE_DOCUMENT);
        prop_assert_eq!(d.provenance(), &copy_layer.provenance());
        let kept_at = l.provenance().to_string();
        prop_assert!(d.message().contains(&kept_at), "{}", d.message());

        // Loaded the other way round, the user file's copy is the first and
        // stays; the pack's document is the one refused.
        let mut flipped = Registry::new();
        prop_assert!(flipped.insert(copy).is_ok());
        let mut refused = Vec::new();
        for doc in family.docs() {
            if let Err(dup) = flipped.insert(doc.clone()) {
                refused.push(dup.rejected.clone());
            }
        }
        prop_assert_eq!(refused, vec![l.clone()]);
        let kept = flipped.get(original.name()).map(|d| d.layer().clone());
        prop_assert_eq!(kept, Some(copy_layer));
    }
}

/// A chain of `n` documents `d0` <- `d1` <- ... , each in its own file,
/// `d0` naming `d<n-1>` as its base when `closed`.
fn long_chain(n: usize, closed: bool) -> Registry<Rom> {
    registry_of((0..n).map(|i| {
        let id = format!("d{i}");
        let layer = Layer::new(
            id.as_str().into(),
            LayerKind::File,
            format!("defs/{id}.xml"),
            Span::line_col(1, 1),
        );
        let rom = Rom {
            xmlid: Field::Set(id),
            ..Rom::default()
        };
        let doc = Document::new(layer, rom);
        match (i, closed) {
            (0, true) => doc.with_base(format!("d{}", n - 1).as_str()),
            (0, false) => doc,
            _ => doc.with_base(format!("d{}", i - 1).as_str()),
        }
    }))
}

/// MF-17, never a stack overflow: 100 000 documents in 100 000 files, as a
/// cycle and as a chain, through the public path down to the diagnostic.
#[test]
fn a_hundred_thousand_files_never_overflow() {
    const N: usize = 100_000;
    let top = Name::global(format!("d{}", N - 1).as_str());

    let ring = long_chain(N, true);
    let err = Resolver::new(&ring).lineage(&top).err();
    let Some(err @ ResolveError::Cycle { .. }) = err else {
        panic!("expected a cycle, got {err:?}");
    };
    if let ResolveError::Cycle { path, layers, .. } = &err {
        assert_eq!(path.len(), N + 1);
        assert_eq!(layers.len(), N);
    }
    let d = err.to_diagnostic(request());
    assert_eq!(d.code(), codes::INHERITANCE_CYCLE);
    assert!(d.is_error());
    assert!(
        d.message()
            .starts_with("inheritance cycle: d99999 (defs/d99999.xml:1:1) -> d99998")
    );
    assert!(d.message().ends_with("d0 (defs/d0.xml:1:1) -> d99999"));

    let line = long_chain(N, false);
    let mut resolver = Resolver::new(&line);
    let d = resolver
        .lineage(&top)
        .err()
        .map(|e| e.to_diagnostic(request()))
        .expect("a 100 000 deep chain is too large");
    assert_eq!(d.code(), codes::LINEAGE_TOO_LARGE);
    assert_eq!(d.severity(), Severity::Error);
    // Below the limit, a chain across 256 files resolves and completes.
    let ok = resolver
        .resolve(&Name::global("d255"))
        .map(|r| r.into_value().xmlid);
    assert_eq!(ok, Ok("d255".to_owned()));
    // The merged partial is the fold of the registered documents.
    let mut fold = Rom::default();
    for i in 0..256 {
        if let Some(doc) = line.get(&Name::global(format!("d{i}").as_str())) {
            fold.merge_from(doc.doc());
        }
    }
    let lineage = resolver.lineage(&Name::global("d255")).ok();
    assert_eq!(lineage.map(|l| Rom::clone(l.merged())), Some(fold));
}

/// An unknown name asked for from outside is reported where it was asked
/// for; a base missing from every file is reported at the document naming
/// it.
#[test]
fn unknown_names_and_bases_are_located() {
    let reg = registry_of([Document::new(
        Layer::new(
            "A2WC522N".into(),
            LayerKind::File,
            "defs/a.xml",
            Span::line_col(4, 3),
        ),
        Rom::default(),
    )
    .with_base("16BITBASE")]);
    let unknown = Resolver::new(&reg)
        .lineage(&Name::global("nope"))
        .err()
        .map(|e| e.to_diagnostic(request()));
    let unknown = unknown.expect("unknown");
    assert_eq!(unknown.code(), codes::UNKNOWN_DOCUMENT);
    assert_eq!(unknown.provenance(), &request());

    let base = Resolver::new(&reg)
        .lineage(&Name::global("A2WC522N"))
        .err()
        .map(|e| e.to_diagnostic(request()));
    let base = base.expect("unknown base");
    assert_eq!(base.code(), codes::UNKNOWN_BASE);
    assert_eq!(base.provenance().source(), "defs/a.xml");
    assert_eq!(
        base.message(),
        "`A2WC522N` names base `16BITBASE`, which is not defined"
    );
}
