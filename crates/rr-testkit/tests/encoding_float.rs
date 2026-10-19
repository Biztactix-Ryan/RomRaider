//! rr-core's float classes against the committed synthetic ROM (US-RR-19-9,
//! EC-07).
//!
//! `fixtures/public/synthetic/standard.manifest.toml` records, for every
//! `f32`/`f64` cell of `standard.bin`, its IEEE 754 bit pattern and class
//! (`+qnan`, `-snan`, `+inf`, `-zero`, `+subnormal`, `+normal`, ...),
//! computed by the reference generator. Here the manifest is read as plain
//! TOML and every float cell is decoded through rr-core:
//!
//! * the decoded bits are the manifest's, and re-encoding gives back the ROM
//!   bytes (quiet and signalling NaN payloads included);
//! * [`Raw::float_class`] and the sign bit spell the manifest's class;
//! * the EC-07 gate refuses exactly the NaN, infinite and subnormal cells,
//!   unless special floats are explicitly allowed.
//!
//! This file is a test boundary: the manifest vocabulary is converted to
//! rr-core's types here, without the reference codec.

use std::collections::BTreeSet;
use std::fs;

use rr_core::encoding::{
    EditOperand, Encoding, EncodingError, Endian, FloatClass, Layout, Raw, SpecialFloats, Storage,
};
use rr_testkit::fixtures::repo_root;

/// One float cell of the manifest.
struct FloatCell {
    table: String,
    offset: usize,
    encoding: Encoding,
    bits: u64,
    class: String,
}

#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn standard_float_cells() -> (Vec<u8>, Vec<FloatCell>) {
    let dir = repo_root()
        .join("fixtures")
        .join("public")
        .join("synthetic");
    let rom = fs::read(dir.join("standard.bin")).expect("standard.bin");
    let text = fs::read_to_string(dir.join("standard.manifest.toml")).expect("manifest");
    let manifest: toml::Table = text.parse().expect("manifest is TOML");
    let field = |table: &toml::Table, key: &str| -> String {
        table
            .get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or_else(|| panic!("{key} missing"))
            .to_owned()
    };
    let mut out = Vec::new();
    for table in manifest["table"].as_array().expect("[[table]]") {
        let table = table.as_table().expect("table");
        let storage = match field(table, "storage").as_str() {
            "f32" => Storage::F32,
            "f64" => Storage::F64,
            _ => continue,
        };
        let endian = match field(table, "endian").as_str() {
            "big" => Endian::Big,
            "little" => Endian::Little,
            other => panic!("endian {other:?}"),
        };
        assert_eq!(field(table, "layout"), "default");
        assert!(!table.contains_key("bitfield"));
        let encoding = Encoding::new(storage, endian, None, Layout::Default).expect("encoding");
        for cell in table["cells"].as_array().expect("cells") {
            let cell = cell.as_table().expect("cell");
            let raw = field(cell, "raw");
            let hex = raw.strip_prefix("0x").expect("0x hex");
            assert_eq!(hex.len(), 2 * storage.cell_bytes(), "{raw}");
            out.push(FloatCell {
                table: field(table, "name"),
                offset: usize::try_from(cell["offset"].as_integer().expect("offset"))
                    .expect("offset"),
                encoding,
                bits: u64::from_str_radix(hex, 16).expect("hex"),
                class: field(cell, "class"),
            });
        }
    }
    (rom, out)
}

/// The manifest's spelling of a class: sign then `zero`, `subnormal`,
/// `normal`, `inf`, `qnan` or `snan`.
fn manifest_class(negative: bool, class: FloatClass) -> String {
    let name = match class {
        FloatClass::Zero => "zero",
        FloatClass::Subnormal => "subnormal",
        FloatClass::Normal => "normal",
        FloatClass::Infinite => "inf",
        FloatClass::Nan { quiet: true } => "qnan",
        FloatClass::Nan { quiet: false } => "snan",
    };
    format!("{}{name}", if negative { '-' } else { '+' })
}

#[test]
fn every_float_cell_of_the_synthetic_rom_decodes_to_its_manifest_bits_and_class() {
    let (rom, cells) = standard_float_cells();
    let mut seen = BTreeSet::new();
    for cell in &cells {
        let at = format!("{} @ {:#x}", cell.table, cell.offset);
        let storage = cell.encoding.storage();
        let width = storage.cell_bytes();
        let bytes = &rom[cell.offset..cell.offset + width];
        let raw = cell.encoding.decode(bytes).unwrap();
        assert_eq!(raw.float_bits(), Some(cell.bits), "{at}");
        let class = raw.float_class().unwrap();
        let negative = cell.bits >> (8 * width - 1) == 1;
        match raw {
            Raw::F32(v) => assert_eq!(v.is_sign_negative(), negative, "{at}"),
            Raw::F64(v) => assert_eq!(v.is_sign_negative(), negative, "{at}"),
            Raw::Int(_) => panic!("{at}: not a float"),
        }
        assert_eq!(manifest_class(negative, class), cell.class, "{at}");
        assert_eq!(cell.encoding.encode(raw).unwrap(), bytes, "{at}");
        seen.insert((storage.name(), cell.class.clone()));
    }
    // Every class with both signs, for both widths.
    for storage in ["f32", "f64"] {
        for class in ["zero", "subnormal", "normal", "inf", "qnan", "snan"] {
            for sign in ['+', '-'] {
                assert!(
                    seen.contains(&(storage, format!("{sign}{class}"))),
                    "{storage} {sign}{class} not in the manifest"
                );
            }
        }
    }
}

#[test]
fn the_edit_gate_refuses_exactly_the_special_cells_of_the_synthetic_rom() {
    let (rom, cells) = standard_float_cells();
    let mut refused = 0;
    for cell in &cells {
        let at = format!("{} @ {:#x}", cell.table, cell.offset);
        let width = cell.encoding.storage().cell_bytes();
        let original = rom[cell.offset..cell.offset + width].to_vec();
        let stored = cell.encoding.decode(&original).unwrap();
        // Replace with zero, which is never special.
        let zero = if width == 4 {
            Raw::F32(0.0)
        } else {
            Raw::F64(0.0)
        };
        let special = ["inf", "qnan", "snan", "subnormal"]
            .iter()
            .any(|c| cell.class.ends_with(c));
        let mut bytes = original.clone();
        let result = cell
            .encoding
            .edit_into(zero, &mut bytes, SpecialFloats::Refuse);
        if special {
            refused += 1;
            assert_eq!(
                result,
                Err(EncodingError::SpecialFloat {
                    operand: EditOperand::Stored,
                    kind: stored.kind(),
                    bits: cell.bits,
                    class: stored.float_class().unwrap(),
                }),
                "{at}"
            );
            assert_eq!(bytes, original, "{at}: nothing written");
            // Writing the stored special value back is refused as the new
            // value too.
            assert!(
                stored
                    .check_edit(SpecialFloats::Refuse, EditOperand::New)
                    .is_err()
            );
        } else {
            assert_eq!(result, Ok(()), "{at}");
            assert_eq!(bytes, vec![0; width], "{at}");
        }
        // Explicitly allowed: every cell is editable, and writing the
        // stored value back restores the exact bytes.
        let mut bytes = original.clone();
        cell.encoding
            .edit_into(zero, &mut bytes, SpecialFloats::Allow)
            .unwrap();
        cell.encoding
            .edit_into(stored, &mut bytes, SpecialFloats::Allow)
            .unwrap();
        assert_eq!(bytes, original, "{at}");
    }
    assert!(refused >= 40, "only {refused} special cells");
}
