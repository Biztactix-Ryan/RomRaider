//! Shared bases are never copied, and a resolver merges each one once
//! (US-RR-22-8; ARCHITECTURE.md C1 "Shared bases via Arc with no deep
//! copy").
//!
//! Legacy deep-copied a base table, via serialization, for every table that
//! named it (`xml/TableScaleUnmarshaller.java:87-92`). Here every document
//! carries a probe counting how often it is cloned and how often it is
//! applied as an overlay. The probes use `Rc<Cell>`, which the stateless
//! layer module may not name (`cargo xtask check-globals`), so this test
//! lives outside it.

use std::cell::Cell;
use std::rc::Rc;

use rr_core::diag::Span;
use rr_core::layer::{Document, Field, Layer, LayerKind, Merge, Name, Registry};

/// Counts the clones of its document and the merges that apply it.
#[derive(Debug, Default)]
struct Probe {
    clones: Rc<Cell<u32>>,
    merges: Rc<Cell<u32>>,
}

impl Clone for Probe {
    fn clone(&self) -> Self {
        self.clones.set(self.clones.get() + 1);
        Self {
            clones: Rc::clone(&self.clones),
            merges: Rc::clone(&self.merges),
        }
    }
}

impl Merge for Probe {
    /// Counts `overlay` as applied; the accumulated document keeps its own
    /// probe, so cloning it never counts against a registered document.
    fn merge_from(&mut self, overlay: &Self) {
        overlay.merges.set(overlay.merges.get() + 1);
    }
}

#[derive(Debug, Clone, Default)]
struct Doc {
    value: Field<u32>,
    probe: Probe,
}
rr_core::merge_fields!(Doc { value, probe });

/// Registers `id` with `bases`, returning its probe.
fn add(reg: &mut Registry<Doc>, id: &str, value: u32, bases: &[&str]) -> Probe {
    let layer = Layer::new(
        id.into(),
        LayerKind::File,
        format!("{id}.xml"),
        Span::line_col(1, 1),
    );
    let probe = Probe::default();
    let doc = Doc {
        value: Field::Set(value),
        probe: Probe {
            clones: Rc::clone(&probe.clones),
            merges: Rc::clone(&probe.merges),
        },
    };
    let doc = bases
        .iter()
        .fold(Document::new(layer, doc), |d, &b| d.with_base(b));
    assert!(reg.insert(doc).is_ok(), "{id} registered twice");
    probe
}

fn counts(probe: &Probe) -> (u32, u32) {
    (probe.clones.get(), probe.merges.get())
}

/// A chain shared by three children: every document is applied exactly
/// once and never cloned, however many children inherit from it.
#[test]
fn a_shared_chain_is_merged_once_and_never_cloned() {
    let mut reg = Registry::new();
    let a = add(&mut reg, "a", 1, &[]);
    let b = add(&mut reg, "b", 2, &["a"]);
    let children: Vec<Probe> = ["c1", "c2", "c3"]
        .iter()
        .zip(3..)
        .map(|(id, v)| add(&mut reg, id, v, &["b"]))
        .collect();

    let mut resolver = reg.resolver();
    for (id, value) in [("c1", 3), ("c2", 4), ("c3", 5), ("b", 2), ("a", 1)] {
        let l = resolver.lineage(&Name::global(id)).unwrap();
        assert_eq!(l.merged().value, Field::Set(value), "{id}");
    }
    for probe in [&a, &b].into_iter().chain(&children) {
        assert_eq!(counts(probe), (0, 1));
    }
}

/// A diamond: `d` starts from `b`'s merged document (its lineage begins
/// `d`'s) and applies only `c` and itself on top; `c`, merged once for its
/// own lineage, is applied once more.
#[test]
fn a_diamond_reuses_the_longest_base_prefix() {
    let mut reg = Registry::new();
    let a = add(&mut reg, "a", 1, &[]);
    let b = add(&mut reg, "b", 2, &["a"]);
    let c = add(&mut reg, "c", 3, &["a"]);
    let d = add(&mut reg, "d", 4, &["b", "c"]);
    let l = reg.resolver().lineage(&Name::global("d")).unwrap();
    assert_eq!(l.merged().value, Field::Set(4));
    assert_eq!(
        [counts(&a), counts(&b), counts(&c), counts(&d)],
        [(0, 1), (0, 1), (0, 2), (0, 1)]
    );
}

/// When two base lineages both begin the document's, the longer one is the
/// starting point.
#[test]
fn the_longest_prefix_wins() {
    let mut reg = Registry::new();
    let a = add(&mut reg, "a", 1, &[]);
    let b = add(&mut reg, "b", 2, &["a"]);
    let d = add(&mut reg, "d", 4, &["a", "b"]);
    let l = reg.resolver().lineage(&Name::global("d")).unwrap();
    let order: Vec<String> = l.names().iter().map(ToString::to_string).collect();
    assert_eq!(order, ["a", "b", "d"]);
    assert_eq!(
        [counts(&a), counts(&b), counts(&d)],
        [(0, 1), (0, 1), (0, 1)]
    );
}

/// A fresh resolver starts again: the cache belongs to the resolver, not
/// to the module.
#[test]
fn each_resolver_has_its_own_cache() {
    let mut reg = Registry::new();
    let a = add(&mut reg, "a", 1, &[]);
    for _ in 0..3 {
        reg.resolver().lineage(&Name::global("a")).unwrap();
    }
    assert_eq!(counts(&a), (0, 3));
}
