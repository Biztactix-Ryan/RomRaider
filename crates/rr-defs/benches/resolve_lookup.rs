//! Definition resolve and ROM-ID lookup against their budgets (US-RR-34-9,
//! enforced in CI by US-RR-34-4; MF-18, MF-40, EC-10).
//!
//! `cargo bench -p rr-defs --bench resolve_lookup` prints the numbers;
//! `cargo bench -p rr-defs --bench resolve_lookup -- --check` also checks
//! them against the budgets and exits 1 when one is blown, which is what
//! the `perf-budget` job of `ci.yml` runs. Like `rr-core`'s `expr_eval`
//! bench, a plain `harness = false` benchmark timed with the workspace's
//! sanctioned clock ([`RealClock`], D-010) and `std::hint::black_box`: no
//! benchmark dependency. No test depends on timing; the budget is checked
//! only by that dedicated job, in the release (`bench`) profile, never by
//! the per-target test matrix.
//!
//! # What is measured
//!
//! Everything is built through the public API, from XML generated here (no
//! corpus, no network):
//!
//! * **resolve** (budget [`RESOLVE_BUDGET`](budget::RESOLVE_BUDGET),
//!   200 ms): a [`DefinitionSet`] of a base `<rom>` with [`TABLES`] (5000)
//!   typed tables (3D, 2D and 1D, each with its scaling and axes, laid out
//!   as legacy's bases are) and a child `<rom>` with a ROM ID and a 4 MiB `<filesize>` that places every
//!   one of them (EC-10). The base's scaling vocabulary is about that of the
//!   largest pinned definition ([`FORMULAS`]): 48 formulas, each with its
//!   `to_byte`, on 16-bit and on 8-bit cells, 144 distinct cell formats in
//!   all. One resolution is a fresh [`DefinitionResolver`] resolving the
//!   child into a frozen 5000-table
//!   [`Definition`](rr_defs::resolve::Definition): the `base` chain merged,
//!   every table typed and frozen, every distinct `to_byte` checked against
//!   its `expression` (`CNV0005`), nothing cached from an earlier run. The
//!   same resolution again by the same resolver, with those checks and the
//!   shared parts cached, is printed too, not budgeted.
//! * **lookup** (budget [`LOOKUP_BUDGET`](budget::LOOKUP_BUDGET), 5 ms):
//!   a [`RomIdIndex`] over [`INDEXED`] (2000) identified definitions, their
//!   8-byte IDs at the addresses the pinned definitions use, in about their
//!   proportions
//!   (0x2000, 0x200, 0x2004, 0xE1000, ...), some written as hex, and one
//!   [`RomIdIndex::lookup`] of a 4 MiB image holding one of them: the whole
//!   of what opening a ROM costs to find its definition once the index is
//!   built (MF-40: built once per set version, never per open).
//!
//! Reading the XML and building the index are printed too, for
//! information; they are not budgeted (both happen once per definition-set
//! version, not per resolve or open).
//!
//! With `RR_CORPUS_DIR` set (and the corpus fetched, `cargo xtask
//! fetch-corpus`), it also reports, never checks, the same measurements on
//! the pinned RomRaider definitions and the stock images: the largest
//! definition's resolve, every `<rom>` resolved by one resolver, the index
//! build, and the slowest lookup of a clean image.
//!
//! # The budget check
//!
//! Each measurement is the best of several timed runs (after a warm-up),
//! so a run slowed by another process on a shared runner does not count.
//! `--check` fails a measurement only above its budget times
//! [`TOLERANCE`](budget::TOLERANCE) (1.5): the budgets are for a user's
//! desktop, and a shared 2-4 vCPU CI runner is slower than one by a factor
//! the best-of-N cannot remove. A
//! measurement over its budget but within the tolerance passes with a
//! `::warning` annotation in the job log; one over the tolerance prints an
//! `::error` annotation and makes the run exit 1, which fails the job and
//! with it the required `ci-ok` check.
//!
//! | Measurement | Budget | Fails above |
//! |-------------|--------|-------------|
//! | resolve, 5000-table definition, fresh resolver (MF-18, EC-10) | 200 ms | 300 ms |
//! | ROM-ID lookup, 4 MiB image, 2000 indexed definitions (MF-40) | 5 ms | 7.5 ms |
//!
//! The budgets, the tolerance and that decision live in `support/budget.rs`
//! (US-RR-34-4), shared through `#[path]` with `tests/perf_budget_gate.rs`,
//! which proves with made-up durations (no clock, nothing machine-dependent)
//! that the gate fails above budget × tolerance and passes within it.
//! `xtask/tests/perf_budget_ci_config.rs` pins the rest: the `perf-budget`
//! job exists, is in `ci-ok.needs`, runs exactly this bench with `--check`
//! in the `bench` (release) profile, cannot be skipped or softened, and the
//! budgets and tolerance are these.
//!
//! # Measured
//!
//! On the development machine (Intel Core i5-12600, release profile,
//! US-RR-34-9), best of three runs:
//!
//! | Measurement | Best | Budget |
//! |-------------|------|--------|
//! | resolve, 5000-table definition, fresh resolver | 126 ms | 200 ms |
//! | resolve again, same resolver, not budgeted | 32 ms | |
//! | lookup, 2000 indexed definitions, 4 MiB image | 0.41 µs | 5 ms |
//! | read the 5000-table set's XML (3.8 MB), not budgeted | 222 ms | |
//! | build the 2000-definition index, not budgeted | 0.42 ms | |
//!
//! Most of a fresh resolve is still the `to_byte` check
//! (`rr_defs::romraider::to_raw_mismatch`, once per distinct formulas and
//! encoding), which evaluates both formulas at every one of the 65,536 raw
//! values of a 16-bit cell: about 2 ms per 16-bit cell format, 48 of them
//! about 95 ms of the 126. The rest (merging, typing, freezing 5000 tables)
//! is the 32 ms of a resolver that has already checked them. The check was
//! 9 ms per format, 457 ms per resolve, until its scan stopped spreading
//! each raw value's bits one at a time for a cell with no bitmask holes
//! and stopped setting up a fresh value stack per evaluation (rr-core
//! `conversion::domain`, `ExprForm::to_raw_mismatch`); what it reports is
//! unchanged. The pinned corpus shows the same (`RR_CORPUS_DIR`,
//! 2026-10-22): its largest definition, 1312 tables with 89 cell formats
//! (44 on 16-bit cells), resolves in 100 ms with a fresh resolver (411 ms
//! before) and in 7 ms with one that has checked them; every one of the
//! set's 1969 `<rom>`s through one resolver takes 4.6 s (2.3 ms each). Its
//! 1724 identified definitions index in 0.38 ms, and the slowest lookup of
//! its 499 clean images takes 0.85 µs.
//!
//! The lookup is four orders of magnitude inside its budget: one
//! ordered-map lookup per (address, mask) group, whatever the image size.
//!
//! Reading is not budgeted, but grows faster than linearly with the file:
//! about 23 ms for a 1250-table set (1.0 MB), 58 ms for 2500 tables
//! (1.9 MB), 222 ms for 5000 (3.8 MB) and 1.06 s for 10000 (7.7 MB).

use std::collections::HashSet;
use std::fmt::Write as _;
use std::hint::black_box;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use rr_core::clock::{Clock, RealClock};
use rr_core::diag::Loaded;
use rr_defs::resolve::{Axis, Definition, DefinitionResolver, RomIdIndex, RomMatch};
use rr_defs::romraider::DefinitionSet;

#[path = "support/budget.rs"]
mod budget;

use budget::{LOOKUP_BUDGET, Measurement, RESOLVE_BUDGET, ms};

/// Tables of the definition resolved (EC-10: "definitions with 5k+
/// tables").
const TABLES: usize = 5000;

/// Identified definitions in the lookup index; the pinned RomRaider
/// definitions identify about 1,700.
const INDEXED: usize = 2000;

/// The size of the images and of the definitions' `<filesize>` (EC-10:
/// "large images (≥4 MB)").
const IMAGE_SIZE: usize = 4 * 1024 * 1024;

/// The ID addresses of the indexed definitions and how many of every 100
/// use each: about the pinned definitions' proportions.
const ID_ADDRESSES: [(usize, usize); 7] = [
    (0x2000, 49),
    (0x200, 22),
    (0x2004, 18),
    (0xE_1000, 4),
    (0x8000, 3),
    (0xE_1400, 2),
    (0xE_2000, 2),
];

/// Distinct scaling formulas: the vocabulary of the generated base. Each
/// is shown on 16-bit and on 8-bit cells, so the definition has
/// `2 * FORMULAS` distinct integer cell formats (and float axes): about the
/// vocabulary of the largest pinned RomRaider definition, 89 cell formats,
/// 44 of them on 16-bit cells and 43 on 8-bit ones, for 1312 tables.
const FORMULAS: usize = 48;

/// Units, cycled through by the scalings.
const UNITS: [&str; 8] = [
    "psi",
    "%",
    "RPM",
    "degrees",
    "AFR",
    "ms",
    "g/s",
    "multiplier",
];

/// The `<scaling>` of formula `j`: an `expression` and the `to_byte` that
/// inverts it, in the three shapes legacy definitions mostly use
/// (`x*k`, `(x-o)*k`, `x/k`).
fn scaling(out: &mut String, j: usize) {
    let j = j % FORMULAS;
    let k = 0.0037 * (j + 1) as f64;
    let o = 10 * (j % 7 + 1);
    let (expr, to_byte) = match j % 3 {
        0 => (format!("x*{k:.4}"), format!("x/{k:.4}")),
        1 => (format!("(x-{o})*{k:.4}"), format!("(x/{k:.4})+{o}")),
        _ => (format!("x/{:.2}", 0.5 + k), format!("x*{:.2}", 0.5 + k)),
    };
    let units = UNITS[j % UNITS.len()];
    let _ = write!(
        out,
        r#"<scaling units="{units}" expression="{expr}" to_byte="{to_byte}" format="0.00" fineincrement=".01" coarseincrement=".1"/>"#
    );
}

/// One generated table: its kind and where its cells and axes go.
struct Layout {
    kind: &'static str,
    sizex: usize,
    sizey: usize,
}

const fn layout(i: usize) -> Layout {
    match i % 10 {
        0..=5 => Layout {
            kind: "3D",
            sizex: 16,
            sizey: 16,
        },
        6..=8 => Layout {
            kind: "2D",
            sizex: 1,
            sizey: 16,
        },
        _ => Layout {
            kind: "1D",
            sizex: 1,
            sizey: 1,
        },
    }
}

/// The XML of the resolve set: `BASE` with [`TABLES`] tables, and
/// `CHILD`, identified, placing all of them.
fn resolve_set_xml() -> String {
    let mut base = String::new();
    let mut child = String::new();
    let mut address = 0x1_0000;
    for i in 0..TABLES {
        let l = layout(i);
        let (storage, width) = if (i / FORMULAS).is_multiple_of(2) {
            ("uint16", 2)
        } else {
            ("uint8", 1)
        };
        let cells = l.sizex * l.sizey * width;
        let _ = write!(
            base,
            r#"<table type="{}" name="Table {i}" category="Category {}" storagetype="{storage}" endian="big" sizex="{}" sizey="{}" userlevel="{}" logparam="E{}">"#,
            l.kind,
            i % 40,
            l.sizex,
            l.sizey,
            i % 5 + 1,
            i % 100
        );
        scaling(&mut base, i);
        let _ = write!(
            child,
            r#"<table name="Table {i}" storageaddress="{address:X}">"#
        );
        address += cells;
        let axes: &[(&str, &str, usize)] = match l.kind {
            "3D" => &[
                ("X Axis", "Throttle", l.sizex),
                ("Y Axis", "Engine Speed", l.sizey),
            ],
            "2D" => &[("Y Axis", "Coolant Temp", l.sizey)],
            _ => &[],
        };
        for (n, (kind, name, len)) in axes.iter().enumerate() {
            let _ = write!(
                base,
                r#"<table type="{kind}" name="{name}" storagetype="float" endian="little">"#
            );
            scaling(&mut base, i + n + 1);
            base.push_str("</table>");
            let _ = write!(
                child,
                r#"<table type="{kind}" storageaddress="{address:X}"/>"#
            );
            address += len * 4;
        }
        let _ = writeln!(
            base,
            "<description>Generated table {i} of {TABLES}.</description></table>"
        );
        child.push_str("</table>\n");
    }
    format!(
        "<roms>\n<rom><romid><xmlid>BASE</xmlid><filesize>{}kb</filesize>\
         <memmodel>SH7058</memmodel></romid>\n{base}</rom>\n\
         <rom base=\"BASE\"><romid><xmlid>CHILD</xmlid><internalidaddress>2000</internalidaddress>\
         <internalidstring>CHILD001</internalidstring></romid>\n{child}</rom>\n</roms>\n",
        IMAGE_SIZE / 1024
    )
}

/// The 8-character ROM ID of indexed definition `n`.
fn rom_id(n: usize) -> String {
    format!("Z{n:07}")
}

/// The ID address of indexed definition `n`, by [`ID_ADDRESSES`].
fn id_address(n: usize) -> usize {
    let mut slot = n % 100;
    for (address, share) in ID_ADDRESSES {
        if slot < share {
            return address;
        }
        slot -= share;
    }
    ID_ADDRESSES[0].0
}

/// The XML of the lookup set: an unidentified base and [`INDEXED`]
/// identified children; every fourth ID is written in hex (`0x...`).
fn lookup_set_xml() -> String {
    let mut xml = String::from("<roms>\n<rom><romid><xmlid>BASE</xmlid></romid></rom>\n");
    for n in 0..INDEXED {
        let id = rom_id(n);
        let written = if n % 4 == 0 {
            let hex: String = id.bytes().map(|b| format!("{b:02X}")).collect();
            format!("0x{hex}")
        } else {
            id
        };
        let _ = writeln!(
            xml,
            "<rom base=\"BASE\"><romid><xmlid>DEF {n}</xmlid><internalidaddress>{:X}</internalidaddress>\
             <internalidstring>{written}</internalidstring></romid></rom>",
            id_address(n)
        );
    }
    xml.push_str("</roms>\n");
    xml
}

/// `loaded`'s value, or every diagnostic: the generated XML is meant to
/// load with none, so the bench measures the intended work.
fn clean<T>(loaded: Loaded<T>, what: &str) -> Result<T, String> {
    if loaded.diagnostics().is_empty() {
        return Ok(loaded.accept_errors().0);
    }
    let mut msg = format!("{what} has diagnostics:");
    for d in loaded.diagnostics() {
        let _ = write!(msg, "\n  {d}");
    }
    Err(msg)
}

/// The best of `runs` timings of `f`, after one untimed warm-up.
fn best_of<T>(clock: &RealClock, runs: u32, mut f: impl FnMut() -> T) -> Duration {
    black_box(f());
    (0..runs)
        .map(|_| {
            let start = clock.now();
            let out = black_box(f());
            let took = clock.now().saturating_duration_since(start);
            drop(out);
            took
        })
        .min()
        .unwrap_or(Duration::MAX)
}

/// The best of five per-call timings of `f`, each the mean of a batch
/// sized to take about 20 ms (for an operation far below the clock's
/// resolution).
fn best_per_call<T>(clock: &RealClock, mut f: impl FnMut() -> T) -> Duration {
    let mut n: u32 = 16;
    loop {
        let start = clock.now();
        for _ in 0..n {
            black_box(f());
        }
        if clock.now().saturating_duration_since(start) >= Duration::from_millis(20) || n >= 1 << 24
        {
            break;
        }
        n = n.saturating_mul(2);
    }
    (0..5)
        .map(|_| {
            let start = clock.now();
            for _ in 0..n {
                black_box(f());
            }
            clock.now().saturating_duration_since(start) / n
        })
        .min()
        .unwrap_or(Duration::MAX)
}

/// The distinct cell formats of `definition` (its tables' and axes'): the
/// fresh resolver checks each integer one's `to_byte` once.
fn cell_formats(definition: &Definition) -> usize {
    let mut seen = HashSet::new();
    for t in definition.tables() {
        for cells in std::iter::once(t.cells()).chain(t.axes().iter().map(Axis::cells)) {
            if let Some(f) = cells.format() {
                seen.insert(Arc::as_ptr(f));
            }
        }
    }
    seen.len()
}

/// The resolve measurement: the best time to resolve the 5000-table
/// definition with a fresh resolver.
fn resolve(clock: &RealClock) -> Result<Duration, String> {
    let xml = resolve_set_xml();
    let read = best_of(clock, 3, || {
        DefinitionSet::read_all([("bench.xml", xml.as_bytes())])
    });
    let set = clean(
        DefinitionSet::read_all([("bench.xml", xml.as_bytes())]),
        "the generated 5000-table set",
    )?;
    let child = set.find("CHILD").ok_or("the generated set has no CHILD")?;
    let definition = clean(
        DefinitionResolver::new(&set).resolve(child),
        "resolving CHILD",
    )?
    .ok_or("CHILD resolved to no definition")?;
    if definition.tables().len() != TABLES || !definition.unplaced().is_empty() {
        return Err(format!(
            "CHILD resolved to {} tables, {} unplaced; expected {TABLES} placed",
            definition.tables().len(),
            definition.unplaced().len()
        ));
    }
    let took = best_of(clock, 10, || DefinitionResolver::new(&set).resolve(child));
    let mut warm = DefinitionResolver::new(&set);
    let warmed = best_of(clock, 10, || warm.resolve(child));
    println!(
        "  read the set's XML ({:.1} MB), not budgeted: {}",
        xml.len() as f64 / 1e6,
        ms(read)
    );
    println!(
        "  resolve, {TABLES}-table definition ({} cell formats), fresh resolver, best of 10: {}",
        cell_formats(&definition),
        ms(took)
    );
    println!(
        "  resolve again, same resolver (to_byte checks and parts cached), not budgeted: {}",
        ms(warmed)
    );
    Ok(took)
}

/// The lookup measurement: the best per-lookup time of a 4 MiB image in an
/// index of [`INDEXED`] definitions.
fn lookup(clock: &RealClock) -> Result<Duration, String> {
    let xml = lookup_set_xml();
    let set = clean(
        DefinitionSet::read_all([("bench.xml", xml.as_bytes())]),
        "the generated lookup set",
    )?;
    let build = best_of(clock, 5, || RomIdIndex::new(&set));
    let index = RomIdIndex::new(&set);
    if index.candidates().len() != INDEXED {
        return Err(format!(
            "the index has {} definitions, expected {INDEXED}",
            index.candidates().len()
        ));
    }
    // The last definition at the most common address.
    let target = (0..INDEXED)
        .rev()
        .find(|&n| id_address(n) == ID_ADDRESSES[0].0)
        .ok_or("no definition at the first ID address")?;
    let mut image = vec![0xFF_u8; IMAGE_SIZE];
    let at = id_address(target);
    let id = rom_id(target);
    image
        .get_mut(at..at + id.len())
        .ok_or("the ID does not fit the image")?
        .copy_from_slice(id.as_bytes());
    let want = format!("DEF {target}");
    match index.lookup(&image) {
        RomMatch::Unique(c) if c.xmlid() == want => {}
        other => return Err(format!("lookup found {other:?}, expected {want}")),
    }
    let took = best_per_call(clock, || index.lookup(black_box(&image)));
    println!(
        "  build the index of {INDEXED} definitions ({} ID addresses), not budgeted: {}",
        ID_ADDRESSES.len(),
        ms(build)
    );
    println!(
        "  lookup, {} MiB image, best of 5 batches: {}",
        IMAGE_SIZE / (1024 * 1024),
        ms(took)
    );
    Ok(took)
}

/// The corpus report, when `RR_CORPUS_DIR` is set: never fails the run.
fn corpus(clock: &RealClock) {
    use rr_testkit::corpus::defs::{self, DefsPin};
    use rr_testkit::corpus::{Corpus, Image};

    if std::env::var_os("RR_CORPUS_DIR").is_none() {
        println!("pinned corpus: skipped (set RR_CORPUS_DIR to report on it)");
        return;
    }
    let corpus = match Corpus::open() {
        Ok(c) => c,
        Err(e) => {
            println!("pinned corpus: skipped ({e})");
            return;
        }
    };
    let pin = match DefsPin::load(corpus.manifest().path()) {
        Ok(p) => p,
        Err(e) => {
            println!("pinned corpus: skipped ({e})");
            return;
        }
    };
    let mut inputs = Vec::new();
    for file in pin.files.iter().filter(|f| f.is_xml()) {
        match defs::read_def(corpus.dir(), file) {
            Ok(bytes) => inputs.push((file.corpus_path(), bytes)),
            Err(e) => {
                println!("pinned corpus: skipped ({e})");
                return;
            }
        }
    }
    let start = clock.now();
    let (set, _) = DefinitionSet::read_all(inputs.iter().map(|(s, b)| (s.as_str(), b.as_slice())))
        .accept_errors();
    let read = clock.now().saturating_duration_since(start);
    println!(
        "pinned corpus ({} XML files, {} <rom>s): read, all files: {}",
        inputs.len(),
        set.roms().count(),
        ms(read)
    );

    let mut resolver = DefinitionResolver::new(&set);
    let start = clock.now();
    let mut largest = None;
    for (rom, _) in set.roms() {
        let (def, _) = resolver.resolve(rom).accept_errors();
        if let Some(def) = def {
            let n = def.tables().len();
            if largest.is_none_or(|(_, m)| n > m) {
                largest = Some((rom, n));
            }
        }
    }
    let all = clock.now().saturating_duration_since(start);
    println!(
        "  resolve every <rom>, one resolver: {} ({:.3} ms each)",
        ms(all),
        all.as_secs_f64() * 1e3 / set.roms().count().max(1) as f64
    );
    if let Some((rom, n)) = largest {
        let took = best_of(clock, 5, || DefinitionResolver::new(&set).resolve(rom));
        let mut warm = DefinitionResolver::new(&set);
        let formats = warm
            .resolve(rom)
            .accept_errors()
            .0
            .map_or(0, |d| cell_formats(&d));
        let warmed = best_of(clock, 5, || warm.resolve(rom));
        println!(
            "  resolve the largest definition ({n} tables, {formats} cell formats), \
             fresh resolver: {}; again, same resolver: {}",
            ms(took),
            ms(warmed)
        );
    }

    let build = best_of(clock, 5, || RomIdIndex::new(&set));
    let index = RomIdIndex::new(&set);
    println!(
        "  build the index ({} identified definitions): {}",
        index.candidates().len(),
        ms(build)
    );
    let (mut images, mut slowest) = (0, Duration::ZERO);
    for file in corpus.manifest().clean() {
        let Ok(Image::Raw(bytes)) = corpus.read(file) else {
            continue;
        };
        images += 1;
        let took = best_per_call(clock, || index.lookup(black_box(&bytes)));
        slowest = slowest.max(took);
    }
    println!(
        "  slowest lookup of {images} clean image(s): {}",
        ms(slowest)
    );
}

fn main() -> ExitCode {
    // `cargo bench` passes `--bench`; only `--check` means anything here.
    let gate = std::env::args().skip(1).any(|a| a == "--check");
    let clock = RealClock::new();
    println!("rr-defs resolve and ROM-ID lookup (US-RR-34-9, US-RR-34-4):");
    let measured = resolve(&clock).and_then(|r| Ok((r, lookup(&clock)?)));
    let (resolved, looked_up) = match measured {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    corpus(&clock);
    if !gate {
        return ExitCode::SUCCESS;
    }
    println!("budgets (--check):");
    let verdict = budget::gate(&[
        Measurement {
            what: "resolve",
            took: resolved,
            budget: RESOLVE_BUDGET,
        },
        Measurement {
            what: "lookup",
            took: looked_up,
            budget: LOOKUP_BUDGET,
        },
    ]);
    for line in &verdict.lines {
        println!("{line}");
    }
    if verdict.passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
