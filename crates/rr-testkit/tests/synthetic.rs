//! Tests for the synthetic ROM generator (`rr_testkit::reference::synthetic`,
//! US-RR-10-6).
//!
//! * The manifest is parsed back as TOML text (not via the generator's Rust
//!   types), and decoding the generated ROM with the reference codec, using
//!   only what the manifest says about each table, reproduces every cell.
//! * Generation is deterministic: same plan, same bytes and manifest text.
//! * The standard plan covers min/max of every storage type, the float
//!   specials (quiet and signalling NaN with distinct payloads, ±Inf, ±0,
//!   min/max subnormal), overlapping tables, a table ending exactly at the end
//!   of the ROM, bitfield and Bosch-subtract tables.
//! * Hand-computed bytes for selected cells, written out here from the
//!   formulas and never produced by the codec.
//! * The overlap rule (last write wins) and every plan error, including out of
//!   bounds, are checked on small plans; random plans never panic.

use proptest::prelude::*;
use rr_testkit::reference::synthetic::{
    self, CellManifest, Dims, MARKER_LEN, Manifest, Plan, SyntheticError, TablePlan, Values,
    generate, standard_plan,
};
use rr_testkit::reference::{
    Bitfield, Encoding, Endian, Layout, Raw, ReferenceError, Storage, decode_table,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[expect(
    clippy::expect_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn standard() -> (Vec<u8>, Manifest) {
    generate(&standard_plan()).expect("the standard plan generates")
}

/// FNV-1a 64, written out again here so the manifest's hash is checked
/// against an independent implementation.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
    })
}

#[expect(
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn storage_named(name: &str) -> Storage {
    Storage::ALL
        .into_iter()
        .find(|s| s.name() == name)
        .unwrap_or_else(|| panic!("unknown storage {name:?}"))
}

#[expect(
    clippy::expect_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn int(value: &toml::Value) -> usize {
    usize::try_from(value.as_integer().expect("integer")).expect("non-negative")
}

#[expect(
    clippy::expect_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn hex_u64(text: &str) -> u64 {
    u64::from_str_radix(text.strip_prefix("0x").expect("0x prefix"), 16).expect("hex")
}

/// Whether `decoded` is exactly the manifest's `raw` text for `storage`.
#[expect(
    clippy::expect_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn matches_text(decoded: Raw, text: &str, float_storage: bool) -> bool {
    match decoded {
        Raw::F32(v) if float_storage => text.len() == 10 && hex_u64(text) == u64::from(v.to_bits()),
        Raw::F64(v) if float_storage => text.len() == 18 && hex_u64(text) == v.to_bits(),
        Raw::Int(v) if !float_storage => text.parse::<i128>().expect("decimal") == v,
        _ => false,
    }
}

#[expect(
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn cells<'a>(manifest: &'a Manifest, table: &str) -> &'a [CellManifest] {
    &manifest
        .table(table)
        .unwrap_or_else(|| panic!("no table {table:?}"))
        .cells
}

#[expect(
    clippy::expect_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn bytes_at<'a>(rom: &'a [u8], manifest: &Manifest, table: &str, len: usize) -> &'a [u8] {
    let address = manifest.table(table).expect("table").address;
    &rom[address..address + len]
}

fn raw_texts(manifest: &Manifest, table: &str) -> Vec<String> {
    cells(manifest, table)
        .iter()
        .map(|c| synthetic::raw_text(c.expected))
        .collect()
}

fn clobbered(manifest: &Manifest, table: &str) -> Vec<bool> {
    cells(manifest, table)
        .iter()
        .map(CellManifest::clobbered)
        .collect()
}

fn plan(size: usize, tables: Vec<TablePlan>) -> Plan {
    Plan {
        name: "test".to_owned(),
        size,
        fill: 0xFF,
        tables,
    }
}

fn u8_table(name: &str, address: usize, values: &[i128]) -> TablePlan {
    TablePlan::new(
        name,
        address,
        Encoding::new(Storage::U8, Endian::Big),
        Dims::row(values.len()),
        Values::Explicit(values.iter().copied().map(Raw::Int).collect()),
    )
}

// ---------------------------------------------------------------------------
// Manifest text <-> ROM
// ---------------------------------------------------------------------------

/// Re-decodes `rom` from the manifest **text** alone and checks every cell.
#[expect(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn check_manifest_text(rom: &[u8], text: &str) -> usize {
    let doc: toml::Table = text.parse().expect("manifest is valid TOML");
    assert_eq!(doc["schema"].as_integer(), Some(1));
    assert_eq!(doc["synthetic"].as_bool(), Some(true));
    assert_eq!(int(&doc["rom_size"]), rom.len());
    assert_eq!(
        hex_u64(doc["rom_fnv1a64"].as_str().unwrap()),
        fnv(rom),
        "manifest hash"
    );
    let marker = doc["marker"].as_table().unwrap();
    assert_eq!(int(&marker["length"]), MARKER_LEN);
    let marker_hex: String = rom[..MARKER_LEN]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(marker["hex"].as_str(), Some(marker_hex.as_str()));

    let tables = doc["table"].as_array().expect("[[table]]");
    let mut checked = 0;
    for (order, table) in tables.iter().enumerate() {
        let table = table.as_table().unwrap();
        let name = table["name"].as_str().unwrap();
        assert_eq!(int(&table["write_order"]), order, "{name}");
        let storage = storage_named(table["storage"].as_str().unwrap());
        let endian = match table["endian"].as_str().unwrap() {
            "big" => Endian::Big,
            "little" => Endian::Little,
            other => panic!("{name}: endian {other:?}"),
        };
        let layout = match table["layout"].as_str().unwrap() {
            "default" => Layout::Default,
            "bosch-subtract" => Layout::BoschSubtract,
            other => panic!("{name}: layout {other:?}"),
        };
        let mut encoding = Encoding::new(storage, endian).with_layout(layout);
        if let Some(bitfield) = table.get("bitfield") {
            let bitfield = bitfield.as_table().unwrap();
            let mask = hex_u64(bitfield["mask"].as_str().unwrap());
            let shift = u32::try_from(int(&bitfield["shift"])).unwrap();
            encoding = encoding.with_bitfield(Bitfield::new(mask, Some(shift)));
        }
        let address = int(&table["address"]);
        let count = int(&table["cols"]) * int(&table["rows"]);
        let stride = storage.cell_bytes();
        assert_eq!(int(&table["end"]), address + count * stride, "{name}: end");
        let cells = table["cells"].as_array().unwrap();
        assert_eq!(cells.len(), count, "{name}: cell count");
        let overlapped = !table["overlapped_by"].as_array().unwrap().is_empty();

        let decoded = decode_table(rom, address, count, &encoding)
            .unwrap_or_else(|e| panic!("{name}: decode: {e}"));
        let float_storage = storage.is_float() && encoding.bitfield.is_none();
        for (index, (cell, value)) in cells.iter().zip(decoded).enumerate() {
            let cell = cell.as_table().unwrap();
            assert_eq!(int(&cell["index"]), index, "{name}");
            assert_eq!(int(&cell["offset"]), address + index * stride, "{name}");
            let raw = cell["raw"].as_str().unwrap();
            assert!(
                matches_text(value, raw, float_storage),
                "{name}[{index}]: manifest says {raw}, ROM decodes to {value:?}"
            );
            assert_eq!(cell.contains_key("class"), float_storage, "{name}[{index}]");
            let is_clobbered = cell.get("clobbered").and_then(toml::Value::as_bool) == Some(true);
            assert_eq!(
                cell.contains_key("planned"),
                is_clobbered,
                "{name}[{index}]"
            );
            assert!(
                !is_clobbered || overlapped,
                "{name}[{index}] is clobbered but no later table overlaps it"
            );
            checked += 1;
        }
    }
    checked
}

#[test]
fn decoding_the_standard_rom_reproduces_every_manifest_cell() {
    let (rom, manifest) = standard();
    let checked = check_manifest_text(&rom, &manifest.to_toml());
    let total: usize = manifest.tables.iter().map(|t| t.cells.len()).sum();
    assert_eq!(checked, total);
    assert!(total > 300, "only {total} cells");
}

#[test]
fn unclobbered_cells_hold_exactly_the_planned_value() {
    let (_, manifest) = standard();
    for table in &manifest.tables {
        for cell in &table.cells {
            assert_eq!(
                synthetic::same_bits(cell.expected, cell.planned),
                !cell.clobbered(),
                "{}[{}]",
                table.name,
                cell.index
            );
            if table.overlapped_by.is_empty() {
                assert!(!cell.clobbered(), "{}[{}]", table.name, cell.index);
            }
        }
    }
}

#[test]
fn generation_is_deterministic() {
    assert_eq!(standard_plan(), standard_plan());
    let (rom_a, manifest_a) = standard();
    let (rom_b, manifest_b) = standard();
    assert_eq!(rom_a, rom_b);
    assert_eq!(manifest_a.to_toml(), manifest_b.to_toml());
    // No platform paths, CRLF or float text in the manifest.
    let text = manifest_a.to_toml();
    assert!(!text.contains('\r') && !text.contains('\\'));
    // Every raw/planned value is a decimal integer or a 0x bit pattern: never
    // float text such as "NaN", "inf" or "1e-45".
    for key in ["raw = \"", "planned = \""] {
        for piece in text.split(key).skip(1) {
            let value = &piece[..piece.find('"').unwrap()];
            let ok = match value.strip_prefix("0x") {
                Some(hex) => hex
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                None => {
                    let digits = value.strip_prefix('-').unwrap_or(value);
                    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
                }
            };
            assert!(ok, "value {value:?} is not an integer or bit pattern");
        }
    }
}

// ---------------------------------------------------------------------------
// Standard plan coverage
// ---------------------------------------------------------------------------

/// Unclobbered values of every default-layout, non-bitfield table of
/// `storage`, with their endians.
fn plain_values(manifest: &Manifest, storage: Storage) -> (Vec<Raw>, Vec<Endian>) {
    let mut values = Vec::new();
    let mut endians = Vec::new();
    for table in &manifest.tables {
        let e = table.encoding;
        if e.storage == storage && e.layout == Layout::Default && e.bitfield.is_none() {
            endians.push(e.endian);
            values.extend(
                table
                    .cells
                    .iter()
                    .filter(|c| !c.clobbered())
                    .map(|c| c.expected),
            );
        }
    }
    (values, endians)
}

#[test]
fn standard_plan_has_min_and_max_of_every_storage_in_both_byte_orders() {
    let (_, manifest) = standard();
    for storage in Storage::ALL {
        let (values, endians) = plain_values(&manifest, storage);
        let has = |want: Raw| values.iter().any(|v| synthetic::same_bits(*v, want));
        let (min, max) = match storage {
            Storage::F32 => (Raw::F32(f32::MIN), Raw::F32(f32::MAX)),
            Storage::F64 => (Raw::F64(f64::MIN), Raw::F64(f64::MAX)),
            _ => {
                let (min, max) = storage.int_range();
                (Raw::Int(min), Raw::Int(max))
            }
        };
        assert!(has(min), "{storage}: no min");
        assert!(has(max), "{storage}: no max");
        assert!(
            endians.contains(&Endian::Big),
            "{storage}: no big-endian table"
        );
        if storage != Storage::U20 {
            assert!(
                endians.contains(&Endian::Little),
                "{storage}: no little-endian table"
            );
        }
    }
}

/// `(sign, exponent all ones, exponent zero, fraction, quiet)` of a float.
#[expect(
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn float_fields(raw: Raw) -> (bool, bool, bool, u64, bool) {
    match raw {
        Raw::F32(v) => {
            let b = v.to_bits();
            let exp = (b >> 23) & 0xFF;
            let frac = u64::from(b & 0x7F_FFFF);
            (b >> 31 == 1, exp == 0xFF, exp == 0, frac, frac >> 22 == 1)
        }
        Raw::F64(v) => {
            let b = v.to_bits();
            let exp = (b >> 52) & 0x7FF;
            let frac = b & 0xF_FFFF_FFFF_FFFF;
            (b >> 63 == 1, exp == 0x7FF, exp == 0, frac, frac >> 51 == 1)
        }
        Raw::Int(_) => panic!("not a float"),
    }
}

#[test]
fn standard_plan_has_every_float_special() {
    let (_, manifest) = standard();
    for storage in [Storage::F32, Storage::F64] {
        let (values, _) = plain_values(&manifest, storage);
        let fields: Vec<_> = values.iter().map(|v| float_fields(*v)).collect();
        let frac_max = if storage == Storage::F32 {
            0x7F_FFFF
        } else {
            0xF_FFFF_FFFF_FFFF
        };
        let quiet_payloads: std::collections::BTreeSet<u64> = fields
            .iter()
            .filter(|(_, ones, _, frac, quiet)| *ones && *frac != 0 && *quiet)
            .map(|(_, _, _, frac, _)| *frac)
            .collect();
        let signalling_payloads: std::collections::BTreeSet<u64> = fields
            .iter()
            .filter(|(_, ones, _, frac, quiet)| *ones && *frac != 0 && !*quiet)
            .map(|(_, _, _, frac, _)| *frac)
            .collect();
        assert!(
            quiet_payloads.len() >= 2,
            "{storage}: quiet NaN payloads {quiet_payloads:?}"
        );
        assert!(
            signalling_payloads.len() >= 2,
            "{storage}: signalling NaN payloads {signalling_payloads:?}"
        );
        for sign in [false, true] {
            let has = |ones: bool, zero: bool, frac: u64| {
                fields
                    .iter()
                    .any(|f| f.0 == sign && f.1 == ones && f.2 == zero && f.3 == frac)
            };
            assert!(has(true, false, 0), "{storage}: inf sign {sign}");
            assert!(has(false, true, 0), "{storage}: zero sign {sign}");
            assert!(has(false, true, 1), "{storage}: min subnormal sign {sign}");
            assert!(
                has(false, true, frac_max),
                "{storage}: max subnormal sign {sign}"
            );
        }
    }
}

#[test]
fn standard_plan_has_overlaps_end_of_rom_bitfield_and_bosch_tables() {
    let (rom, manifest) = standard();
    let overlapped = manifest
        .tables
        .iter()
        .filter(|t| !t.overlapped_by.is_empty())
        .count();
    assert!(overlapped >= 3, "{overlapped} overlapped tables");
    assert!(
        manifest
            .tables
            .iter()
            .any(|t| t.cells.iter().any(CellManifest::clobbered)),
        "no clobbered cell"
    );
    let at_end: Vec<&str> = manifest
        .tables
        .iter()
        .filter(|t| t.end() == rom.len())
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(at_end, ["end-of-rom-f64-be-specials"]);
    assert!(manifest.tables.iter().all(|t| t.end() <= rom.len()));
    let bitfields = manifest
        .tables
        .iter()
        .filter(|t| t.encoding.bitfield.is_some())
        .count();
    let bosch = manifest
        .tables
        .iter()
        .filter(|t| t.encoding.layout == Layout::BoschSubtract)
        .count();
    assert!(bitfields >= 5, "{bitfields} bitfield tables");
    assert!(bosch >= 4, "{bosch} bosch tables");
    assert!(
        manifest.tables.iter().any(|t| t.dims.rows > 1),
        "no 2-D table"
    );
}

// ---------------------------------------------------------------------------
// Hand-computed bytes (never produced by the codec)
// ---------------------------------------------------------------------------

#[test]
fn rom_is_marked_synthetic() {
    let (rom, manifest) = standard();
    // "RomRaider-RS SYNTHETIC ROM v1" then three NULs.
    let mut want = b"RomRaider-RS SYNTHETIC ROM v1".to_vec();
    want.extend([0, 0, 0]);
    assert_eq!(&rom[..32], want.as_slice());
    assert!(manifest.to_toml().contains("\nsynthetic = true\n"));
    assert!(manifest.tables.iter().all(|t| t.address >= 32));
}

#[test]
fn hand_computed_integer_cells() {
    let (rom, m) = standard();
    // u64 BE extremes: 0 = 00 x8; 2^64-1 = FF x8 = 18446744073709551615.
    let mut want = vec![0x00; 8];
    want.extend([0xFF; 8]);
    assert_eq!(bytes_at(&rom, &m, "u64-be-extremes", 16), want.as_slice());
    assert_eq!(
        raw_texts(&m, "u64-be-extremes"),
        ["0", "18446744073709551615"]
    );
    // i24 LE extremes: -2^23 = -8388608 = 0x800000 -> 00 00 80;
    // 2^23-1 = 8388607 = 0x7FFFFF -> FF FF 7F.
    assert_eq!(
        bytes_at(&rom, &m, "i24-le-extremes", 6),
        [0x00, 0x00, 0x80, 0xFF, 0xFF, 0x7F]
    );
    assert_eq!(raw_texts(&m, "i24-le-extremes"), ["-8388608", "8388607"]);
    // i16 BE specials start 0, 1: 00 00 00 01.
    assert_eq!(bytes_at(&rom, &m, "i16-be-specials", 4), [0, 0, 0, 1]);
    // u20 extremes (bit 19 sign-extends), fill 0xFF keeps the opcode nibble:
    // -2^19 = -524288 -> imm20 0x80000 -> 8F 00 00;
    // 2^19-1 = 524287 -> imm20 0x7FFFF -> 7F FF FF.
    assert_eq!(
        bytes_at(&rom, &m, "u20-be-extremes", 6),
        [0x8F, 0x00, 0x00, 0x7F, 0xFF, 0xFF]
    );
    assert_eq!(raw_texts(&m, "u20-be-extremes"), ["-524288", "524287"]);
}

#[test]
fn hand_computed_float_cells() {
    let (rom, m) = standard();
    // f32 BE specials #4: quiet NaN 0x7FC00000 -> 7F C0 00 00.
    let at = m.table("f32-be-specials").unwrap().address;
    assert_eq!(&rom[at + 16..at + 20], [0x7F, 0xC0, 0x00, 0x00]);
    assert_eq!(raw_texts(&m, "f32-be-specials")[4], "0x7fc00000");
    // f32 LE specials #7: signalling NaN 0x7F800001 -> 01 00 80 7F.
    let at = m.table("f32-le-specials").unwrap().address;
    assert_eq!(&rom[at + 28..at + 32], [0x01, 0x00, 0x80, 0x7F]);
    assert_eq!(raw_texts(&m, "f32-le-specials")[7], "0x7f800001");
    // f64 BE extremes: f64::MIN = 0xFFEFFFFFFFFFFFFF.
    assert_eq!(
        bytes_at(&rom, &m, "f64-be-extremes", 8),
        [0xFF, 0xEF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
    );
    // The end-of-ROM table's last cell is 1.0 = 0x3FF0000000000000, in the
    // ROM's last 8 bytes.
    assert_eq!(
        &rom[rom.len() - 8..],
        [0x3F, 0xF0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
    );
    assert_eq!(
        raw_texts(&m, "end-of-rom-f64-be-specials").last().unwrap(),
        "0x3ff0000000000000"
    );
    let text = m.to_toml();
    assert!(text.contains("raw = \"0x7fc00000\", class = \"+qnan\""));
    assert!(text.contains("raw = \"0x7f800001\", class = \"+snan\""));
    assert!(text.contains("raw = \"0x8000000000000000\", class = \"-zero\""));
    assert!(text.contains("raw = \"0x0000000000000001\", class = \"+subnormal\""));
}

#[test]
fn hand_computed_overlaps() {
    let (rom, m) = standard();
    // a = u16 BE [1111 2222 3333 4444] at a..a+8; then b = u8 [AB CD] at a+3;
    // then c = i32 LE -2 (0xFFFFFFFE -> FE FF FF FF) at a+6.
    assert_eq!(
        bytes_at(&rom, &m, "overlap-a-u16-be", 10),
        [0x11, 0x11, 0x22, 0xAB, 0xCD, 0x33, 0xFE, 0xFF, 0xFF, 0xFF]
    );
    // 0x1111 = 4369, 0x22AB = 8875, 0xCD33 = 52531, 0xFEFF = 65279.
    assert_eq!(
        raw_texts(&m, "overlap-a-u16-be"),
        ["4369", "8875", "52531", "65279"]
    );
    assert_eq!(clobbered(&m, "overlap-a-u16-be"), [false, true, true, true]);
    let planned: Vec<String> = cells(&m, "overlap-a-u16-be")
        .iter()
        .map(|c| synthetic::raw_text(c.planned))
        .collect();
    // 0x2222 = 8738, 0x3333 = 13107, 0x4444 = 17476.
    assert_eq!(planned, ["4369", "8738", "13107", "17476"]);
    assert_eq!(
        m.table("overlap-a-u16-be").unwrap().overlapped_by,
        ["overlap-b-u8", "overlap-c-i32-le"]
    );
    assert_eq!(raw_texts(&m, "overlap-b-u8"), ["171", "205"]);
    assert!(m.table("overlap-b-u8").unwrap().overlapped_by.is_empty());
    assert_eq!(raw_texts(&m, "overlap-c-i32-le"), ["-2"]);

    // d = f32 BE 1.0 (3F 80 00 00); e writes 01 into its last byte.
    assert_eq!(
        bytes_at(&rom, &m, "overlap-d-f32-be", 4),
        [0x3F, 0x80, 0x00, 0x01]
    );
    assert_eq!(raw_texts(&m, "overlap-d-f32-be"), ["0x3f800001"]);
    assert_eq!(
        synthetic::raw_text(cells(&m, "overlap-d-f32-be")[0].planned),
        "0x3f800000"
    );

    // Bosch u8 values [10 20 156] store s2 = 256-156 = 100, s1 = 156-20 = 136,
    // s0 = 20-10 = 10 (0A 88 64); f then sets s2 = 20 (0x14), so
    // v2 = 256-20 = 236, v1 = 236-136 = 100, v0 = 100-10 = 90.
    assert_eq!(
        bytes_at(&rom, &m, "bosch-u8-be-explicit", 3),
        [0x0A, 0x88, 0x64]
    );
    assert_eq!(raw_texts(&m, "bosch-u8-be-explicit"), ["10", "20", "156"]);
    assert_eq!(
        bytes_at(&rom, &m, "overlap-bosch-u8", 3),
        [0x0A, 0x88, 0x14]
    );
    assert_eq!(raw_texts(&m, "overlap-bosch-u8"), ["90", "100", "236"]);
    assert_eq!(clobbered(&m, "overlap-bosch-u8"), [true, true, true]);
}

#[test]
fn hand_computed_bosch_extremes() {
    let (rom, m) = standard();
    // Stored cells cycle [0, 0xFFFF]: 00 00 FF FF 00 00 FF FF. Values from the
    // end: v3 = 65536-65535 = 1, v2 = 1-0 = 1, v1 = 1-65535 = -65534, v0 = -65534.
    assert_eq!(
        bytes_at(&rom, &m, "bosch-u16-be-extremes", 8),
        [0, 0, 0xFF, 0xFF, 0, 0, 0xFF, 0xFF]
    );
    assert_eq!(
        raw_texts(&m, "bosch-u16-be-extremes"),
        ["-65534", "-65534", "1", "1"]
    );
    // u64: stored [0, 2^64-1]; v1 = 2^64 - (2^64-1) = 1, v0 = 1 - 0 = 1.
    let mut want = vec![0x00; 8];
    want.extend([0xFF; 8]);
    assert_eq!(
        bytes_at(&rom, &m, "bosch-u64-be-extremes", 16),
        want.as_slice()
    );
    assert_eq!(raw_texts(&m, "bosch-u64-be-extremes"), ["1", "1"]);
}

#[test]
fn hand_computed_shared_bitfields() {
    let (rom, m) = standard();
    // Two u16 BE cells, fill FF FF. Cell 0: hi=0 -> 0FFF, mid=0 -> 000F,
    // lo=0 -> 0000, clash=0xA -> 00A0. Cell 1: hi/mid/lo max -> FFFF,
    // clash=5 -> FF5F.
    assert_eq!(
        bytes_at(&rom, &m, "bits-shared-hi", 4),
        [0x00, 0xA0, 0xFF, 0x5F]
    );
    assert_eq!(raw_texts(&m, "bits-shared-hi"), ["0", "15"]);
    assert_eq!(raw_texts(&m, "bits-shared-lo"), ["0", "15"]);
    assert_eq!(clobbered(&m, "bits-shared-hi"), [false, false]);
    assert_eq!(clobbered(&m, "bits-shared-lo"), [false, false]);
    // mid: (0x00A0 & 0x0FF0) >> 4 = 0x0A = 10; (0xFF5F & 0x0FF0) >> 4 = 0xF5 = 245.
    assert_eq!(raw_texts(&m, "bits-shared-mid"), ["10", "245"]);
    assert_eq!(clobbered(&m, "bits-shared-mid"), [true, true]);
    assert_eq!(raw_texts(&m, "bits-shared-clash"), ["10", "5"]);
    // The disjoint tables still overlap by bytes.
    assert_eq!(
        m.table("bits-shared-hi").unwrap().overlapped_by,
        ["bits-shared-mid", "bits-shared-lo", "bits-shared-clash"]
    );
    // u64 full-mask bitfield max 2^64-1 needs the string mask in TOML.
    assert!(m.to_toml().contains("mask = \"0xffffffffffffffff\""));
}

// ---------------------------------------------------------------------------
// Overlap rule and errors on small plans
// ---------------------------------------------------------------------------

#[test]
fn last_write_wins() {
    let (rom, m) = generate(&plan(
        40,
        vec![u8_table("a", 32, &[1]), u8_table("b", 32, &[2])],
    ))
    .expect("generates");
    assert_eq!(rom[32], 2);
    assert_eq!(raw_texts(&m, "a"), ["2"]);
    assert_eq!(clobbered(&m, "a"), [true]);
    assert_eq!(m.table("a").unwrap().overlapped_by, ["b"]);
    let (rom, m) = generate(&plan(
        40,
        vec![u8_table("b", 32, &[2]), u8_table("a", 32, &[1])],
    ))
    .expect("generates");
    assert_eq!(rom[32], 1);
    assert_eq!(raw_texts(&m, "b"), ["1"]);
    assert_eq!(rom[33..], [0xFF; 7]);
}

#[test]
fn a_table_may_end_exactly_at_the_end_of_the_rom() {
    let table = TablePlan::new(
        "end",
        62,
        Encoding::new(Storage::U16, Endian::Little),
        Dims::scalar(),
        Values::Explicit(vec![Raw::Int(0x1234)]),
    );
    let (rom, m) = generate(&plan(64, vec![table])).expect("fits");
    assert_eq!(rom[62..], [0x34, 0x12]);
    assert_eq!(m.table("end").unwrap().end(), 64);
}

#[expect(
    clippy::expect_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn err(tables: Vec<TablePlan>) -> SyntheticError {
    generate(&plan(64, tables)).expect_err("plan must be rejected")
}

#[test]
fn out_of_bounds_tables_are_errors() {
    let u16_at = |address| {
        TablePlan::new(
            "t",
            address,
            Encoding::new(Storage::U16, Endian::Big),
            Dims::scalar(),
            Values::Extremes,
        )
    };
    // Bounds are checked before values: two u16 cells at 62 end at 66.
    assert!(matches!(
        err(vec![TablePlan {
            dims: Dims::row(2),
            ..u16_at(62)
        }]),
        SyntheticError::OutOfBounds { length: 4, .. }
    ));
    // One byte past the end.
    let one_past = TablePlan {
        values: Values::Explicit(vec![Raw::Int(1)]),
        ..u16_at(63)
    };
    assert_eq!(
        err(vec![one_past]),
        SyntheticError::OutOfBounds {
            table: "t".into(),
            address: 63,
            length: 2,
            size: 64
        }
    );
    assert!(matches!(
        err(vec![u8_table("t", 64, &[1])]),
        SyntheticError::OutOfBounds { .. }
    ));
    assert!(matches!(
        err(vec![u8_table("t", usize::MAX, &[1])]),
        SyntheticError::OutOfBounds { .. }
    ));
    // A huge but representable cell count saturates the byte length.
    let huge = TablePlan {
        dims: Dims::row(usize::MAX),
        ..u16_at(32)
    };
    assert!(matches!(
        err(vec![huge]),
        SyntheticError::OutOfBounds {
            length: usize::MAX,
            ..
        }
    ));
    // Overflowing or empty dims.
    let overflow = TablePlan {
        dims: Dims::grid(usize::MAX, 2),
        ..u16_at(32)
    };
    assert!(matches!(
        err(vec![overflow]),
        SyntheticError::BadDims { .. }
    ));
    let empty = TablePlan {
        dims: Dims::grid(0, 3),
        ..u16_at(32)
    };
    assert!(matches!(err(vec![empty]), SyntheticError::BadDims { .. }));
}

#[test]
fn the_marker_is_protected_and_rom_size_is_bounded() {
    assert_eq!(
        err(vec![u8_table("t", MARKER_LEN - 1, &[1])]),
        SyntheticError::OverlapsMarker {
            table: "t".into(),
            address: 31
        }
    );
    assert!(generate(&plan(64, vec![u8_table("t", MARKER_LEN, &[1])])).is_ok());
    assert_eq!(
        generate(&plan(31, vec![])).unwrap_err(),
        SyntheticError::RomTooSmall { size: 31 }
    );
    assert!(matches!(
        generate(&plan(synthetic::MAX_ROM_SIZE + 1, vec![])).unwrap_err(),
        SyntheticError::RomTooLarge { .. }
    ));
    // An empty plan is just the marker and fill.
    let (rom, m) = generate(&plan(32, vec![])).unwrap();
    assert_eq!(rom.as_slice(), synthetic::MARKER);
    assert!(m.tables.is_empty());
}

#[test]
fn bad_names_and_value_counts_are_errors() {
    for bad in ["", "has space", "quote\"", "ü", &"x".repeat(65)] {
        assert!(
            matches!(
                err(vec![u8_table(bad, 32, &[1])]),
                SyntheticError::InvalidName { .. }
            ),
            "{bad:?}"
        );
    }
    let mut bad_plan = plan(64, vec![]);
    bad_plan.name = "a/b".into();
    assert!(matches!(
        generate(&bad_plan),
        Err(SyntheticError::InvalidName { .. })
    ));
    assert_eq!(
        err(vec![u8_table("t", 32, &[1]), u8_table("t", 40, &[2])]),
        SyntheticError::DuplicateTable { name: "t".into() }
    );
    let mut short = u8_table("t", 32, &[1, 2]);
    short.dims = Dims::row(3);
    assert_eq!(
        err(vec![short]),
        SyntheticError::ValueCount {
            table: "t".into(),
            cells: 3,
            got: 2
        }
    );
    let specials = TablePlan::new(
        "t",
        32,
        Encoding::new(Storage::F32, Endian::Big),
        Dims::row(2),
        Values::Specials,
    );
    assert!(matches!(
        err(vec![specials]),
        SyntheticError::TooFewCells {
            generator: "specials",
            cells: 2,
            ..
        }
    ));
}

#[test]
fn values_the_codec_refuses_are_errors() {
    assert!(matches!(
        err(vec![u8_table("t", 32, &[256])]),
        SyntheticError::Codec {
            source: ReferenceError::TableCell { .. },
            ..
        }
    ));
    let u20_le = TablePlan::new(
        "t",
        32,
        Encoding::new(Storage::U20, Endian::Little),
        Dims::row(2),
        Values::Extremes,
    );
    assert!(matches!(err(vec![u20_le]), SyntheticError::Codec { .. }));
    let float_bitfield = TablePlan::new(
        "t",
        32,
        Encoding::new(Storage::F32, Endian::Big).with_bitfield(Bitfield::new(0xFF, None)),
        Dims::row(2),
        Values::Extremes,
    );
    assert!(matches!(
        err(vec![float_bitfield]),
        SyntheticError::Codec { .. }
    ));
    let bosch_float = TablePlan::new(
        "t",
        32,
        Encoding::new(Storage::F32, Endian::Big).with_layout(Layout::BoschSubtract),
        Dims::row(2),
        Values::Extremes,
    );
    assert!(matches!(
        err(vec![bosch_float]),
        SyntheticError::Unsupported { .. }
    ));
    // A bosch value step that does not fit the cell: 2^8 - 300 < 0.
    let bosch_bad = TablePlan::new(
        "t",
        32,
        Encoding::new(Storage::U8, Endian::Big).with_layout(Layout::BoschSubtract),
        Dims::row(1),
        Values::Explicit(vec![Raw::Int(-300)]),
    );
    assert!(matches!(err(vec![bosch_bad]), SyntheticError::Codec { .. }));
}

// ---------------------------------------------------------------------------
// Random plans
// ---------------------------------------------------------------------------

fn arb_table() -> impl Strategy<Value = TablePlan> {
    (
        0..Storage::ALL.len(),
        any::<bool>(),
        0_u8..4,
        28_usize..100,
        0_usize..12,
        prop::collection::vec(any::<i64>(), 12),
        0_u8..4,
    )
        .prop_map(|(s, little, bosch, address, cells, ints, kind)| {
            let storage = Storage::ALL[s];
            let endian = if little { Endian::Little } else { Endian::Big };
            let layout = if bosch == 0 {
                Layout::BoschSubtract
            } else {
                Layout::Default
            };
            let values = match kind {
                0 => Values::Extremes,
                1 => Values::Specials,
                // Explicit values, mostly in the storage's range.
                _ => Values::Explicit(
                    ints.iter()
                        .take(cells)
                        .map(|v| match storage {
                            Storage::F32 => Raw::F32(f32::from_bits(*v as u32)),
                            Storage::F64 => Raw::F64(f64::from_bits(*v as u64)),
                            _ => {
                                let (min, max) = storage.int_range();
                                Raw::Int(min + i128::from(*v).rem_euclid(max - min + 1))
                            }
                        })
                        .collect(),
                ),
            };
            TablePlan::new(
                "t",
                address,
                Encoding::new(storage, endian).with_layout(layout),
                Dims::row(cells),
                values,
            )
        })
}

proptest! {
    #[test]
    fn random_plans_never_panic_and_their_manifests_hold(
        size in 24_usize..160,
        tables in prop::collection::vec(arb_table(), 1..4),
    ) {
        let tables = tables
            .into_iter()
            .enumerate()
            .map(|(i, t)| TablePlan { name: format!("t{i}"), ..t })
            .collect();
        let plan = plan(size, tables);
        if let Ok((rom, manifest)) = generate(&plan) {
            prop_assert_eq!(rom.len(), size);
            check_manifest_text(&rom, &manifest.to_toml());
            for table in &manifest.tables {
                if table.overlapped_by.is_empty() {
                    prop_assert!(table.cells.iter().all(|c| !c.clobbered()));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Committed output (US-RR-10-7)
// ---------------------------------------------------------------------------

#[test]
fn output_files_are_the_rom_and_its_manifest_text() {
    let plans = synthetic::published_plans();
    assert!(plans.iter().any(|p| p.name == "standard"));
    let mut names = Vec::new();
    for plan in &plans {
        let (rom, manifest) = generate(plan).unwrap();
        let files = synthetic::output_files(plan).unwrap();
        assert_eq!(
            files,
            [
                (format!("{}.bin", plan.name), rom),
                (
                    format!("{}.manifest.toml", plan.name),
                    manifest.to_toml().into_bytes()
                ),
            ]
        );
        assert_eq!(files, synthetic::output_files(plan).unwrap());
        names.extend(files.into_iter().map(|(n, _)| n));
    }
    let count = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), count, "two published plans share a file name");
}

#[test]
fn output_files_reject_a_bad_plan() {
    let bad = plan(16, Vec::new());
    assert!(matches!(
        synthetic::output_files(&bad),
        Err(SyntheticError::RomTooSmall { .. })
    ));
}
