//! Smoke test of the RomRaider `<scaling>` reader and of `build_cells` on
//! the pinned definition corpus (US-RR-29-7): every `<scaling>` and
//! `<scalingbase>` of every pinned ECU definition file reads with no Error
//! diagnostic, and every table or axis that gives its own storage type
//! builds an encoding and conversion with no Error.
//!
//! Not the story's formal acceptance test (US-RR-29-1, which resolves
//! inheritance: `corpus_romraider_loads.rs`). It prints what it found (scalings by category,
//! the storage types they convert, linear and non-linear formulas, how
//! many of the non-linear ones solve over their storage, `to_byte`
//! formulas that disagree with their `expression` (`CNV0005`), warnings by
//! code) so a change shows in the output, and on failure lists every Error
//! by code with the files it is in. Files whose root element is not
//! `<roms>` or `<rom>` (logger and dyno definitions) are skipped.
//!
//! A table that changes an inherited one often leaves its storage type to
//! its base; those are counted, not built (merging is US-RR-29-8's).
//!
//! Corpus-tier (`*::requires_RR_CORPUS_DIR`): skipped without
//! `RR_CORPUS_DIR`, run by CI's `corpus` job.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use rr_core::conversion::Form;
use rr_core::diag::{Provenance, Span};
use rr_core::encoding::Encoding;
use rr_defs::romraider::{self, CellsDoc, ScalingDoc, TableKind, build_cells, to_raw_mismatch};
use rr_defs::xml::Document;
use rr_testkit::corpus::Corpus;
use rr_testkit::corpus::defs::{self, DefsError, DefsPin};

/// Whether `doc` is an ECU definition (root element `<roms>` or `<rom>`, any
/// case) rather than a logger or dyno one.
fn is_ecu_definition(doc: &Document) -> bool {
    let name = doc.root().name();
    name.eq_ignore_ascii_case("roms") || name.eq_ignore_ascii_case("rom")
}

/// One distinct conversion over one distinct encoding: the formulas as
/// written and the encoding.
type Key = (Option<String>, Option<Option<String>>, String, Encoding);

/// What the scalings of the corpus hold.
#[derive(Debug, Default)]
struct Counts {
    scalings: usize,
    bases: usize,
    categories: BTreeMap<String, usize>,
    storages: BTreeMap<String, usize>,
    inherited_storage: usize,
    static_axes: usize,
    built: usize,
    /// Distinct (formulas, encoding) pairs built.
    distinct: HashSet<Key>,
}

/// What the distinct conversions are.
#[derive(Debug, Default)]
struct Kinds {
    linear: usize,
    nonlinear: usize,
    solved: usize,
    not_solved: BTreeMap<String, usize>,
    mismatches: usize,
    mismatch_examples: Vec<String>,
}

fn formula(s: &ScalingDoc) -> (Option<String>, Option<Option<String>>) {
    (
        s.to_real.get().map(|f| f.text().to_owned()),
        s.to_raw
            .get()
            .map(|t| t.as_ref().map(|f| f.text().to_owned())),
    )
}

rr_testkit::corpus_test! {
    /// Every `<scaling>` of every pinned ECU definition reads, and every
    /// table or axis with its own storage type builds, with no Error.
    fn every_pinned_ecu_definition_reads_its_scalings_without_errors() -> Result<(), DefsError> {
        let corpus = Corpus::open()?;
        let pin = DefsPin::load(corpus.manifest().path())?;
        let mut files = 0;
        let mut counts = Counts::default();
        let mut warnings: BTreeMap<String, usize> = BTreeMap::new();
        // Error code -> (count, files).
        let mut errors: BTreeMap<String, (usize, BTreeSet<String>)> = BTreeMap::new();
        let mut failures = Vec::new();
        let mut to_check: HashMap<Key, romraider::CellFormat> = HashMap::new();
        for file in pin.files.iter().filter(|f| f.is_xml()) {
            let source = file.corpus_path();
            let bytes = defs::read_def(corpus.dir(), file)?;
            let parsed = rr_defs::xml::parse(&source, &bytes);
            let Some(Some(doc)) = parsed.value() else {
                failures.push(format!("{source}: not well-formed: {:?}", parsed.diagnostics()));
                continue;
            };
            if !is_ecu_definition(doc) {
                continue;
            }
            files += 1;
            let (value, diagnostics) = romraider::read_document(&source, doc).accept_errors();
            for d in &diagnostics {
                if d.is_error() {
                    let entry = errors.entry(d.code().to_string()).or_default();
                    entry.0 += 1;
                    entry.1.insert(source.clone());
                    failures.push(d.to_string());
                } else {
                    *warnings.entry(format!("{}[{}]", d.severity(), d.code())).or_insert(0) += 1;
                }
            }
            let Some(value) = value else {
                failures.push(format!("{source}: no definition and no error"));
                continue;
            };
            counts.bases += value.scaling_bases().len();
            for rom in value.roms() {
                let mem = rom.doc().romid.mem_model_endian.get().copied();
                for (name, table) in rom.doc().tables.present() {
                    let kind = table.kind.get().copied();
                    let mut cells: Vec<(&CellsDoc, Option<TableKind>)> = vec![(&table.cells, kind)];
                    for axis in [&table.x_axis, &table.y_axis] {
                        if let Some(axis) = axis.get() {
                            cells.push((&axis.cells, None));
                        }
                    }
                    for (c, kind) in cells {
                        counts.scalings += c.scalings.len();
                        for (key, _) in c.scalings.entries() {
                            *counts.categories.entry(key.clone()).or_insert(0) += 1;
                        }
                        if c.static_values.get().is_some() {
                            counts.static_axes += 1;
                            continue;
                        }
                        let Some(storage) = c.storage.get() else {
                            counts.inherited_storage += 1;
                            continue;
                        };
                        *counts.storages.entry(storage.name().to_owned()).or_insert(0) += 1;
                        let scaling = romraider::select_scaling(&c.scalings, Some("Metric"));
                        let at = Provenance::new(source.as_str(), Span::line_col(0, 0));
                        let (built, diags) = build_cells(c, kind, scaling, mem, &at).accept_errors();
                        for d in &diags {
                            let entry = errors.entry(d.code().to_string()).or_default();
                            entry.0 += 1;
                            entry.1.insert(source.clone());
                            failures.push(format!("table `{name}` of {}: {d}", rom.xmlid()));
                        }
                        let Some(built) = built else { continue };
                        counts.built += 1;
                        let (to_real, to_raw) = scaling.map(formula).unwrap_or_default();
                        let unit = built.conversion.unit().unwrap_or("").to_owned();
                        let key = (to_real, to_raw, unit, built.encoding);
                        counts.distinct.insert(key.clone());
                        to_check.entry(key).or_insert(built);
                    }
                }
            }
        }
        let mut kinds = Kinds::default();
        for (key, built) in &to_check {
            let form = built.conversion.form();
            if form.linear().is_some() {
                kinds.linear += 1;
            } else {
                kinds.nonlinear += 1;
                if let Ok(range) = built.encoding.raw_range() {
                    match form.inverse(range) {
                        Ok(_) => kinds.solved += 1,
                        Err(e) => {
                            *kinds.not_solved.entry(e.code().to_string()).or_insert(0) += 1;
                        }
                    }
                }
            }
            if matches!(form, Form::Expr(_)) && let Some(m) = to_raw_mismatch(built) {
                kinds.mismatches += 1;
                if kinds.mismatch_examples.len() < 5 {
                    kinds.mismatch_examples.push(format!(
                        "{:?} / {:?} over {}: {m}",
                        key.0, key.1, key.3.storage()
                    ));
                }
            }
        }
        eprintln!(
            "[defs] at {}: {files} ECU definition file(s); {} <scaling>(s) by category {:?}, \
             {} <scalingbase>(s); tables/axes with their own storage {:?}, {} inheriting it, \
             {} static axes; {} built, {} distinct (formulas, encoding): {} linear, {} \
             non-linear ({} solve over their storage, not solved by code {:?}); {} CNV0005 \
             to_byte mismatches, e.g. {:#?}; warnings/infos: {warnings:?}",
            pin.commit,
            counts.scalings,
            counts.categories,
            counts.bases,
            counts.storages,
            counts.inherited_storage,
            counts.static_axes,
            counts.built,
            counts.distinct.len(),
            kinds.linear,
            kinds.nonlinear,
            kinds.solved,
            kinds.not_solved,
            kinds.mismatches,
            kinds.mismatch_examples,
        );
        assert!(files > 0 && counts.scalings > 0, "no scalings in the pinned corpus");
        assert!(
            failures.is_empty(),
            "{} Error diagnostic(s), by code {:?}:\n  {}",
            failures.len(),
            errors,
            failures.join("\n  ")
        );
        Ok(())
    }
}
