//! The seed known-answer fixtures (`fixtures/kat/*.toml`) cover everything
//! the fixture format and the codec support (US-RR-9-3, story US-RR-9:
//! "at least 10 seed fixtures exist covering every storage type the format
//! supports, each reviewed and sourced").
//!
//! The supported set is derived from the code, never from the fixtures:
//!
//! * every storage type of the fixture schema ([`kat::Storage`]) and of
//!   rr-core ([`Storage`]), which must name the same set;
//! * every byte order rr-core accepts for each storage
//!   ([`Encoding::new`] rejects only little-endian `u20`);
//! * a bitfield in each byte order, on a multi-byte storage;
//! * every non-default [`Layout`], unless it is listed in
//!   [`UNCOVERED_LAYOUTS`] with the reason it has no seed yet.
//!
//! The `match`es below are exhaustive on purpose: adding a storage type or a
//! layout to either enum stops this file compiling until the new variant is
//! listed here, and then the coverage test fails until a seed fixture for it
//! exists.
//!
//! "Reviewed" is not machine-checkable: the format has no reviewed-by field,
//! and review happens in the fixture's PR (fixtures/README.md, Review). This
//! file checks the machine-checkable half: every seed loads and passes the
//! validator (so every expected value has a `source`, and every `hand-calc`
//! shows its working).

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use rr_core::encoding::{BitField, Encoding, Endian, Layout, Storage};
use rr_testkit::fixtures::repo_root;
use rr_testkit::kat::{self, Definition, KatKind, validate};

/// Non-default layouts that have no seed fixture yet, with the reason. Remove
/// an entry when its seed lands (the test fails while a covered layout is
/// still listed here).
const UNCOVERED_LAYOUTS: &[(Layout, &str)] = &[(
    Layout::BoschSubtract,
    "a bosch-subtract value depends on every later cell of its table and the \
     schema cannot yet say where the table ends (fixtures/README.md, Open questions)",
)];

/// Minimum number of seed fixtures (story US-RR-9 acceptance criterion).
const MIN_SEEDS: usize = 10;

/// Index of each rr-core storage in [`Storage::ALL`]. Exhaustive: a new
/// variant does not compile until it is added here, to `Storage::ALL`, and
/// to `kat::Storage`.
const fn core_index(storage: Storage) -> usize {
    match storage {
        Storage::U8 => 0,
        Storage::U16 => 1,
        Storage::U20 => 2,
        Storage::U24 => 3,
        Storage::U32 => 4,
        Storage::U64 => 5,
        Storage::I8 => 6,
        Storage::I16 => 7,
        Storage::I24 => 8,
        Storage::I32 => 9,
        Storage::I64 => 10,
        Storage::F32 => 11,
        Storage::F64 => 12,
    }
}

/// Index of each fixture-schema storage in [`kat::Storage::ALL`]. Exhaustive,
/// as [`core_index`].
const fn kat_index(storage: kat::Storage) -> usize {
    match storage {
        kat::Storage::U8 => 0,
        kat::Storage::U16 => 1,
        kat::Storage::U20 => 2,
        kat::Storage::U24 => 3,
        kat::Storage::U32 => 4,
        kat::Storage::U64 => 5,
        kat::Storage::I8 => 6,
        kat::Storage::I16 => 7,
        kat::Storage::I24 => 8,
        kat::Storage::I32 => 9,
        kat::Storage::I64 => 10,
        kat::Storage::F32 => 11,
        kat::Storage::F64 => 12,
    }
}

/// Every rr-core layout. Exhaustive, as [`core_index`].
fn all_layouts() -> Vec<Layout> {
    let all = [Layout::Default, Layout::BoschSubtract];
    for layout in all {
        match layout {
            Layout::Default | Layout::BoschSubtract => {}
        }
    }
    all.to_vec()
}

const ENDIANS: [Endian; 2] = [Endian::Big, Endian::Little];

const fn endian_of(endian: kat::Endian) -> Endian {
    match endian {
        kat::Endian::Big => Endian::Big,
        kat::Endian::Little => Endian::Little,
    }
}

const fn layout_of(layout: kat::Layout) -> Layout {
    match layout {
        kat::Layout::Default => Layout::Default,
        kat::Layout::BoschSubtract => Layout::BoschSubtract,
    }
}

fn seed_paths() -> Vec<PathBuf> {
    let dir = repo_root().join("fixtures").join("kat");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    paths
}

#[test]
fn storage_enums_list_every_variant_and_agree() {
    for (i, storage) in Storage::ALL.iter().enumerate() {
        assert_eq!(
            core_index(*storage),
            i,
            "rr-core Storage::ALL order/coverage"
        );
    }
    for (i, storage) in kat::Storage::ALL.iter().enumerate() {
        assert_eq!(kat_index(*storage), i, "kat::Storage::ALL order/coverage");
    }
    let core: BTreeSet<&str> = Storage::ALL.iter().map(|s| s.name()).collect();
    let schema: BTreeSet<&str> = kat::Storage::ALL.iter().map(|s| s.name()).collect();
    assert_eq!(core.len(), Storage::ALL.len(), "duplicate in Storage::ALL");
    assert_eq!(
        core, schema,
        "rr-core and the fixture schema name different storages"
    );
}

#[test]
fn every_seed_fixture_loads_and_passes_the_validator() {
    let paths = seed_paths();
    assert!(
        paths.len() >= MIN_SEEDS,
        "{} seed fixture(s) in fixtures/kat; at least {MIN_SEEDS} are required",
        paths.len()
    );
    let report = validate::validate_files(&paths, None).unwrap();
    assert_eq!(report.files, paths.len());
    let violations: Vec<String> = report.violations.iter().map(ToString::to_string).collect();
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn seed_fixtures_cover_every_supported_storage_endian_bitfield_and_layout() {
    // (storage name, endian, has bitfield, layout) of every seed decode fixture.
    let mut seen: BTreeSet<(&'static str, &'static str, bool, &'static str)> = BTreeSet::new();
    for path in seed_paths() {
        let name = path.display().to_string();
        let fixture = kat::load(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        let KatKind::Decode { definition, .. } = fixture.data.kind else {
            continue; // a checksum fixture has no storage type
        };
        let Definition::Inline { encoding, .. } = definition else {
            panic!("{name}: a seed decode fixture must use an inline encoding");
        };
        seen.insert((
            encoding.storage.name(),
            endian_of(encoding.endian).name(),
            encoding.bitfield.is_some(),
            layout_of(encoding.layout).name(),
        ));
    }

    let mut missing = Vec::new();

    // Every storage, in every byte order rr-core accepts, plain (no bitfield,
    // default layout).
    for storage in Storage::ALL {
        for endian in ENDIANS {
            if Encoding::new(storage, endian, None, Layout::Default).is_err() {
                continue; // not a supported combination (little-endian u20)
            }
            let key = (storage.name(), endian.name(), false, Layout::Default.name());
            if !seen.contains(&key) {
                missing.push(format!("{} {} (plain)", storage.name(), endian.name()));
            }
        }
    }

    // A bitfield in each byte order, on a storage where byte order matters.
    let probe = BitField::new(0x0FF0, None, false).unwrap();
    for endian in ENDIANS {
        let supported = Storage::ALL
            .iter()
            .any(|&s| Encoding::new(s, endian, Some(probe), Layout::Default).is_ok());
        let covered = seen.iter().any(|&(storage, e, bitfield, layout)| {
            bitfield
                && e == endian.name()
                && layout == Layout::Default.name()
                && Storage::ALL
                    .iter()
                    .any(|s| s.name() == storage && s.cell_bytes() > 1)
        });
        if supported && !covered {
            missing.push(format!(
                "bitfield on a multi-byte storage, {}",
                endian.name()
            ));
        }
    }

    // Every non-default layout, unless explicitly exempt.
    for layout in all_layouts() {
        if layout == Layout::Default {
            continue;
        }
        let covered = seen.iter().any(|&(_, _, _, l)| l == layout.name());
        let exempt = UNCOVERED_LAYOUTS.iter().any(|&(l, _)| l == layout);
        assert!(
            !(covered && exempt),
            "layout {} now has a seed fixture: remove it from UNCOVERED_LAYOUTS",
            layout.name()
        );
        if !covered && !exempt {
            missing.push(format!("layout {}", layout.name()));
        }
    }

    assert!(
        missing.is_empty(),
        "supported encodings with no seed fixture in fixtures/kat: {missing:?}\n(seen: {seen:?})"
    );
    eprintln!(
        "seed coverage: {} distinct (storage, endian, bitfield, layout) combinations; \
         uncovered by exemption: {:?}",
        seen.len(),
        UNCOVERED_LAYOUTS
            .iter()
            .map(|(l, _)| l.name())
            .collect::<Vec<_>>()
    );
}
