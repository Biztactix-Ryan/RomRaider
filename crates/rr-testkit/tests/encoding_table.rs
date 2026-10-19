//! rr-core's table codec (`Encoding::decode_table` / `encode_table`,
//! US-RR-19-10) against the committed synthetic ROM and the independent
//! reference codec (PROJECT.md T2).
//!
//! * Every table of `fixtures/public/synthetic/standard.bin` (the five
//!   Bosch-subtract and nine bitfield tables among them) decodes through
//!   rr-core, using only what `standard.manifest.toml` says about it, to the
//!   manifest's values; writing those values back leaves the ROM unchanged,
//!   and over inverted bytes they decode to the same values again.
//! * On random encodings of every shape (both layouts; no, unsigned or
//!   signed bitfield), random table bytes and random table values, rr-core
//!   and the reference codec agree (`rr_testkit::encoding_oracle`): the same
//!   values from the same bytes, the same bytes from the same values, and a
//!   refusal from both, with nothing written, when a value cannot be stored.
//!   This is the Bosch-subtract hook of `rr_testkit::strategies::encoding`
//!   (`bosch_subtract_encoding` x `table_bytes_for`).
//!
//! This file is a test boundary: the manifest vocabulary is converted to
//! rr-core's types here.

use std::fs;

use proptest::collection::vec;
use proptest::prelude::*;
use rr_core::encoding::{BitField, Encoding, Endian, Layout, Raw, Storage};
use rr_testkit::encoding_oracle::{
    Refusal, check_decode_table, check_encode_table, check_table_bytes,
};
use rr_testkit::fixtures::repo_root;
use rr_testkit::strategies::encoding::{
    any_encoding, bosch_subtract_encoding, invalid_value_for, table_bytes_for, value_for,
};

// ---------------------------------------------------------------------------
// The synthetic ROM
// ---------------------------------------------------------------------------

/// One table of the manifest, in rr-core's vocabulary.
struct ManifestTable {
    name: String,
    address: usize,
    encoding: Encoding,
    /// The manifest's `raw` text of each cell.
    raw: Vec<String>,
}

#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn standard_tables() -> (Vec<u8>, Vec<ManifestTable>) {
    let dir = repo_root()
        .join("fixtures")
        .join("public")
        .join("synthetic");
    let rom = fs::read(dir.join("standard.bin")).expect("standard.bin");
    let text = fs::read_to_string(dir.join("standard.manifest.toml")).expect("manifest");
    let manifest: toml::Table = text.parse().expect("manifest is TOML");
    let text_of = |table: &toml::Table, key: &str| -> String {
        table
            .get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or_else(|| panic!("{key} missing"))
            .to_owned()
    };
    let int_of = |table: &toml::Table, key: &str| -> usize {
        let value = table.get(key).and_then(toml::Value::as_integer);
        usize::try_from(value.unwrap_or_else(|| panic!("{key} missing"))).expect(key)
    };
    let hex = |text: &str| -> u64 {
        u64::from_str_radix(text.strip_prefix("0x").expect("0x hex"), 16).expect("hex")
    };
    let mut out = Vec::new();
    for table in manifest["table"].as_array().expect("[[table]]") {
        let table = table.as_table().expect("table");
        let name = text_of(table, "name");
        let storage_name = text_of(table, "storage");
        let storage = Storage::ALL
            .into_iter()
            .find(|s| s.name() == storage_name)
            .unwrap_or_else(|| panic!("{name}: storage {storage_name:?}"));
        let endian = match text_of(table, "endian").as_str() {
            "big" => Endian::Big,
            "little" => Endian::Little,
            other => panic!("{name}: endian {other:?}"),
        };
        let layout = match text_of(table, "layout").as_str() {
            "default" => Layout::Default,
            "bosch-subtract" => Layout::BoschSubtract,
            other => panic!("{name}: layout {other:?}"),
        };
        // Manifest bitfields are unsigned (the schema has no sign flag).
        let bitfield = table.get("bitfield").map(|field| {
            let field = field.as_table().expect("bitfield table");
            let shift = u32::try_from(int_of(field, "shift")).expect("shift");
            BitField::new(hex(&text_of(field, "mask")), Some(shift), false)
                .unwrap_or_else(|e| panic!("{name}: {e}"))
        });
        let encoding = Encoding::new(storage, endian, bitfield, layout)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let cells = table["cells"].as_array().expect("cells");
        assert_eq!(cells.len(), int_of(table, "cols") * int_of(table, "rows"));
        let address = int_of(table, "address");
        assert_eq!(
            int_of(table, "end"),
            address + cells.len() * storage.cell_bytes()
        );
        let raw = cells
            .iter()
            .map(|cell| text_of(cell.as_table().expect("cell"), "raw"))
            .collect();
        out.push(ManifestTable {
            name,
            address,
            encoding,
            raw,
        });
    }
    (rom, out)
}

/// Whether `value` is the manifest's `raw` text: a decimal integer, or a
/// float's bit pattern as `0x` hex of the cell's width.
#[expect(
    clippy::expect_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn matches_raw(value: Raw, text: &str) -> bool {
    let hex = || u64::from_str_radix(text.strip_prefix("0x").expect("0x hex"), 16).expect("hex");
    match value {
        Raw::Int(v) => text.parse::<i128>().expect("decimal") == v,
        Raw::F32(v) => text.len() == 10 && hex() == u64::from(v.to_bits()),
        Raw::F64(v) => text.len() == 18 && hex() == v.to_bits(),
    }
}

#[test]
fn every_table_of_the_synthetic_rom_decodes_to_its_manifest_values() {
    let (rom, tables) = standard_tables();
    let mut bosch = Vec::new();
    let mut bitfield = Vec::new();
    for table in &tables {
        let name = &table.name;
        let encoding = table.encoding;
        let cells = table.raw.len();
        let values = encoding
            .decode_table(&rom[table.address..], cells)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(values.len(), cells, "{name}");
        for (index, (value, raw)) in values.iter().zip(&table.raw).enumerate() {
            assert!(
                matches_raw(*value, raw),
                "{name}[{index}]: manifest says {raw}, rr-core decodes {value:?}"
            );
        }
        let region = &rom[table.address..table.address + cells * encoding.storage().cell_bytes()];
        // The reference codec reads the same values from the same bytes.
        assert!(
            matches!(check_decode_table(encoding, region), Ok(Ok(_))),
            "{name}"
        );
        // Writing the values back over the ROM changes nothing; over every
        // bit inverted, the same values come back.
        let mut copy = rom.clone();
        encoding
            .encode_table(&values, &mut copy[table.address..])
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(copy == rom, "{name}: re-encoding changed the ROM");
        assert!(
            matches!(check_table_bytes(encoding, region), Ok(Ok(_))),
            "{name}"
        );
        if encoding.layout() == Layout::BoschSubtract {
            bosch.push(name.as_str());
        }
        if encoding.bitfield().is_some() {
            bitfield.push(name.as_str());
        }
    }
    assert_eq!(bosch.len(), 5, "Bosch-subtract tables: {bosch:?}");
    assert_eq!(bitfield.len(), 9, "bitfield tables: {bitfield:?}");
}

#[test]
fn the_synthetic_bosch_tables_hold_their_hand_computed_values() {
    // bosch-u8-be-explicit: the plan's values 10, 20, 156 are stored as
    // s2 = 256 - 156 = 100, s1 = 156 - 20 = 136, s0 = 20 - 10 = 10
    // (legacy DataCell.java:219-231).
    let (rom, tables) = standard_tables();
    let table = tables
        .iter()
        .find(|t| t.name == "bosch-u8-be-explicit")
        .unwrap();
    assert_eq!(&rom[table.address..table.address + 3], &[10, 136, 100]);
    assert_eq!(
        table.encoding.decode_table(&rom[table.address..], 3),
        Ok(vec![Raw::Int(10), Raw::Int(20), Raw::Int(156)])
    );
    // bosch-u64-be-extremes: values 1, 1 are stored as s1 = 2^64 - 1 (all
    // ones) and s0 = 1 - 1 = 0; 2^64 - (2^64 - 1) = 1, 1 - 0 = 1.
    let table = tables
        .iter()
        .find(|t| t.name == "bosch-u64-be-extremes")
        .unwrap();
    let region = &rom[table.address..table.address + 16];
    assert_eq!(region, [[0; 8], [0xFF; 8]].concat());
    assert_eq!(
        table.encoding.decode_table(region, 2),
        Ok(vec![Raw::Int(1), Raw::Int(1)])
    );
}

// ---------------------------------------------------------------------------
// Agreement with the reference codec
// ---------------------------------------------------------------------------

/// The same storage, byte order and bitfield in the default layout: how one
/// cell of `encoding` is stored.
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn cell_of(encoding: Encoding) -> Encoding {
    Encoding::new(
        encoding.storage(),
        encoding.endian(),
        encoding.bitfield(),
        Layout::Default,
    )
    .unwrap()
}

/// The table values whose stored cells are `stored`, written out from the
/// layout's definition: the cells themselves, or for Bosch subtract
/// `value[i] = 2^(8W) - (s[i] + ... + s[N-1])`. A non-integer stored value
/// (only generated to be refused) is kept as it is.
fn values_of_stored(encoding: Encoding, stored: &[Raw]) -> Vec<Raw> {
    if encoding.layout() == Layout::Default {
        return stored.to_vec();
    }
    let total = 1_i128 << (8 * encoding.storage().cell_bytes());
    (0..stored.len())
        .map(|i| match stored[i] {
            Raw::Int(_) => Raw::Int(stored[i..].iter().fold(total, |acc, s| match s {
                Raw::Int(s) => acc - s,
                Raw::F32(_) | Raw::F64(_) => acc,
            })),
            other => other,
        })
        .collect()
}

/// An encoding and the stored cells of a table of 1 to 6 cells, each one
/// the cell encoding can store, over its full range.
fn table_of_storable_cells() -> impl Strategy<Value = (Encoding, Vec<Raw>, Vec<u8>)> {
    any_encoding().prop_flat_map(|encoding| {
        let cell = cell_of(encoding);
        (
            Just(encoding),
            vec(value_for(&cell), 1..=6),
            table_bytes_for(&cell, 6..=6),
        )
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    #[test]
    fn table_bytes_decode_the_same_and_write_back(
        (encoding, bytes) in any_encoding()
            .prop_flat_map(|e| (Just(e), table_bytes_for(&e, 0..=6))),
    ) {
        let decoded = check_table_bytes(encoding, &bytes);
        prop_assert!(matches!(decoded, Ok(Ok(_))), "{decoded:?}");
    }

    #[test]
    fn bosch_table_bytes_decode_the_same_and_write_back(
        (encoding, bytes) in bosch_subtract_encoding()
            .prop_flat_map(|e| (Just(e), table_bytes_for(&e, 1..=8))),
    ) {
        let decoded = check_table_bytes(encoding, &bytes);
        prop_assert!(matches!(decoded, Ok(Ok(_))), "{decoded:?}");
    }

    #[test]
    fn every_table_of_storable_cells_round_trips_and_agrees(
        (encoding, stored, start) in table_of_storable_cells(),
    ) {
        let values = values_of_stored(encoding, &stored);
        let written = check_encode_table(encoding, &values, &start);
        prop_assert!(matches!(written, Ok(Ok(_))), "{written:?}");
    }

    #[test]
    fn a_table_with_one_unstorable_cell_is_refused_by_both(
        ((encoding, mut stored, start), (index, bad)) in table_of_storable_cells()
            .prop_flat_map(|(e, stored, start)| {
                let len = stored.len();
                ((Just((e, stored, start))), (0..len, invalid_value_for(&cell_of(e))))
            }),
    ) {
        stored[index] = bad;
        let values = values_of_stored(encoding, &stored);
        let refused = check_encode_table(encoding, &values, &start);
        prop_assert!(
            matches!(
                refused,
                Ok(Err(Refusal::OutOfRange | Refusal::BitFieldOutOfRange | Refusal::KindMismatch))
            ),
            "{values:?}: {refused:?}"
        );
    }

    #[test]
    fn arbitrary_bosch_values_are_stored_or_refused_by_both(
        (encoding, values, start) in bosch_subtract_encoding().prop_flat_map(|e| {
            // Within +-2^100, where no step overflows i128 in the reference.
            let value = -(1_i128 << 100)..=1_i128 << 100;
            (Just(e), vec(value.prop_map(Raw::Int), 1..=4), table_bytes_for(&e, 4..=4))
        }),
    ) {
        let outcome = check_encode_table(encoding, &values, &start);
        prop_assert!(outcome.is_ok(), "{outcome:?}");
    }

    #[test]
    fn a_table_that_does_not_fit_is_refused_by_both(
        (encoding, stored, start) in table_of_storable_cells(),
        cut in 1_usize..=8,
    ) {
        let values = values_of_stored(encoding, &stored);
        let needed = values.len() * encoding.storage().cell_bytes();
        let short = &start[..needed.saturating_sub(cut)];
        prop_assert_eq!(
            check_encode_table(encoding, &values, short),
            Ok(Err(Refusal::ShortInput))
        );
    }
}
