//! Unit tests of the `<table>` reader (US-RR-29-6): one hand-checked
//! fixture per table kind (minimal XML in, the typed partial document out),
//! axes, inheritance-shaped patches, and every diagnostic with its
//! `line:col`.

use proptest::prelude::*;
use rr_core::diag::{Code, Diagnostic, Span, codes};
use rr_core::encoding::{BitField, Endian, Layout, Storage};
use rr_core::layer::{Appended, Field, Keyed, Layered, Merge, Nested};

use super::super::value::{parse_decimal_int, parse_state_data};
use super::super::{DefinitionFile, RomDoc, RomEntry, ScalingDoc, read};
use super::{AxisDoc, BitFlag, CellsDoc, SwitchState, TABLE_ATTRIBUTES, TableDoc, TableKind};

/// `(code, "line:col")` of each diagnostic.
fn found(diags: &[Diagnostic]) -> Vec<(Code, String)> {
    diags
        .iter()
        .map(|d| {
            let at = match d.provenance().span() {
                Span::LineCol { line, col } => format!("{line}:{col}"),
                Span::ByteOffset(o) => format!("@{o}"),
            };
            (d.code(), at)
        })
        .collect()
}

/// A file of one `<rom>` (with a `base` if `base`) holding `tables`, which
/// start on line 3, column 1.
fn in_rom(base: bool, tables: &str) -> String {
    let base = if base { " base=\"B\"" } else { "" };
    format!("<roms>\n<rom{base}><romid><xmlid>X</xmlid></romid>\n{tables}\n</rom>\n</roms>")
}

/// The only rom of `xml`, which must read with exactly `expected`
/// diagnostics (`(code, "line:col")`).
fn rom_of(xml: &str, expected: &[(Code, &str)]) -> RomEntry {
    let (file, diags) = read("t.xml", xml.as_bytes()).accept_errors();
    let want: Vec<(Code, String)> = expected
        .iter()
        .map(|(c, a)| (*c, (*a).to_owned()))
        .collect();
    assert_eq!(found(&diags), want, "{diags:#?}");
    let mut roms = file.map(DefinitionFile::into_roms).unwrap_or_default();
    assert_eq!(roms.len(), 1, "{xml}");
    roms.remove(0)
}

/// The diagnostics' messages.
fn messages(xml: &str) -> Vec<String> {
    let (_, diags) = read("t.xml", xml.as_bytes()).accept_errors();
    diags.iter().map(|d| d.message().to_owned()).collect()
}

/// The table state of key `key` in `rom`.
fn state<'r>(rom: &'r RomEntry, key: &str) -> &'r Nested<TableDoc> {
    rom.doc()
        .tables
        .get(&key.to_owned())
        .unwrap_or_else(|| panic!("no table {key:?} in {:?}", rom.doc().tables))
}

/// The only table of a base-less rom holding `table`, read with exactly
/// `expected` diagnostics.
fn table(table: &str, expected: &[(Code, &str)]) -> TableDoc {
    let rom = rom_of(&in_rom(false, table), expected);
    assert_eq!(rom.doc().tables.len(), 1);
    let (_, doc) = rom.doc().tables.present().next().expect("a table");
    doc.clone()
}

/// The only table of a rom with a base holding `table`.
fn change(table: &str, expected: &[(Code, &str)]) -> TableDoc {
    let rom = rom_of(&in_rom(true, table), expected);
    let (_, doc) = rom.doc().tables.present().next().expect("a table");
    doc.clone()
}

fn set<T>(value: T) -> Field<T> {
    Field::Set(value)
}

fn text(value: &str) -> Field<String> {
    Field::Set(value.to_owned())
}

/// The scalings of a `<scaling units="...">` of the default category.
fn default_scaling(units: &str) -> Keyed<String, ScalingDoc> {
    let scaling = ScalingDoc {
        units: text(units),
        ..ScalingDoc::default()
    };
    [("default".to_owned(), Nested::Patch(scaling))]
        .into_iter()
        .collect()
}

// ---- values -------------------------------------------------------------

#[test]
fn decimal_ints_read_as_legacy_integer_parse_int() {
    for (text, want) in [
        ("0", 0),
        ("16", 16),
        ("+7", 7),
        ("-3", -3),
        (" 12\n", 12),
        ("2147483647", i32::MAX),
        ("-2147483648", i32::MIN),
        ("000000000000000000000001", 1),
    ] {
        assert_eq!(parse_decimal_int(text), Some(want), "{text:?}");
    }
    for text in [
        "",
        "-",
        "+",
        "1.5",
        "0x10",
        "1e3",
        "2147483648",
        "-2147483649",
        "1 2",
        "--1",
        "99999999999999999999999",
    ] {
        assert_eq!(parse_decimal_int(text), None, "{text:?}");
    }
}

#[test]
fn state_data_reads_as_legacy_preset_manager() {
    for (text, want) in [
        ("01", vec![1]),
        ("05 00 00", vec![5, 0, 0]),
        ("  42 C8\t00 ", vec![0x42, 0xC8, 0]),
        ("00,00, 0a", vec![0, 0, 0xA]),
        ("1,,2", vec![1, 2]),
        ("+FF", vec![0xFF]),
        ("FFFFFFFF", vec![u32::MAX]),
        ("e400", vec![0xE400]),
        ("", vec![]),
    ] {
        assert_eq!(parse_state_data(text), Some(want), "{text:?}");
    }
    for text in ["+", "0x01", "G", "-1", "1FFFFFFFF", "01;02", "1 +"] {
        assert_eq!(parse_state_data(text), None, "{text:?}");
    }
}

// ---- one fixture per kind ---------------------------------------------------

/// A 1D table with every attribute that applies to it, and the typed
/// document expected, field by field (each from its attribute).
#[test]
fn kat_1d_table() {
    let doc = table(
        "<table type=\"1D\" name=\"Rev Limit\" category=\"Limits\" storageaddress=\"0x2B0E0\" \
         storagetype=\"uint16\" endian=\"big\" sizey=\"1\" userlevel=\"3\" locked=\"true\" \
         logparam=\"P8\" beforeram=\"true\" mask=\"0FF0\" dataLayout=\"default\" flipy=\"false\">\
         <scaling units=\"rpm\"/><description>Fuel cut RPM.</description></table>",
        &[],
    );
    let want = TableDoc {
        name: text("Rev Limit"),
        kind: set(TableKind::OneD),
        category: text("Limits"),
        description: text("Fuel cut RPM."),
        user_level: set(3),
        locked: set(true),
        log_param: text("P8"),
        cells: CellsDoc {
            storage_address: set(0x2_B0E0),
            storage: set(Storage::U16),
            endian: set(Endian::Big),
            bitfield: set(BitField::new(0x0FF0, None, false).unwrap()),
            layout: set(Layout::Default),
            before_ram: set(true),
            size: set(1),
            flip: set(false),
            static_values: Field::Unset,
            scalings: default_scaling("rpm"),
        },
        size_y: set(1),
        flip_y: set(false),
        ..TableDoc::default()
    };
    assert_eq!(doc, want);
}

/// A 2D table with a Y axis read from the image: the axis patches both the
/// one-axis slot and the Y slot; the 2D's data size is its `sizey`.
#[test]
fn kat_2d_table() {
    let doc = table(
        "<table type=\"2D\" name=\"Injector Latency\" category=\"Fueling\" \
         storageaddress=\"2F5C0\" storagetype=\"uint16\" endian=\"little\" sizey=\"8\" \
         userlevel=\"1\">\n\
         <scaling units=\"ms\"/>\n\
         <table type=\"Y Axis\" name=\"Battery Voltage\" storageaddress=\"2F5A0\" \
         storagetype=\"float\" endian=\"little\" logparam=\"P17\"><scaling units=\"V\"/></table>\n\
         <description>Latency.</description></table>",
        &[],
    );
    let axis = AxisDoc {
        name: text("Battery Voltage"),
        log_param: text("P17"),
        cells: CellsDoc {
            storage_address: set(0x2_F5A0),
            storage: set(Storage::F32),
            endian: set(Endian::Little),
            scalings: default_scaling("V"),
            ..CellsDoc::default()
        },
        ..AxisDoc::default()
    };
    let want = TableDoc {
        name: text("Injector Latency"),
        kind: set(TableKind::TwoD),
        category: text("Fueling"),
        description: text("Latency."),
        user_level: set(1),
        cells: CellsDoc {
            storage_address: set(0x2_F5C0),
            storage: set(Storage::U16),
            endian: set(Endian::Little),
            size: set(8),
            scalings: default_scaling("ms"),
            ..CellsDoc::default()
        },
        size_y: set(8),
        axis: Nested::Patch(axis.clone()),
        y_axis: Nested::Patch(axis),
        ..TableDoc::default()
    };
    assert_eq!(doc, want);
}

/// A 2D table with a static axis: its `<data>` values as written, in order.
#[test]
fn kat_2d_table_with_a_static_axis() {
    let doc = table(
        "<table type=\"2D\" name=\"Gear Ratios\" storageaddress=\"100\" storagetype=\"float\" \
         endian=\"big\" sizey=\"3\">\n\
         <table type=\"Static Y Axis\" name=\"Gear\" sizey=\"3\">\
         <data>1st</data><data> 2nd </data><data>3rd</data></table></table>",
        &[],
    );
    let axis = AxisDoc {
        name: text("Gear"),
        cells: CellsDoc {
            size: set(3),
            static_values: set(vec!["1st".to_owned(), " 2nd ".to_owned(), "3rd".to_owned()]),
            ..CellsDoc::default()
        },
        ..AxisDoc::default()
    };
    assert_eq!(doc.axis, Nested::Patch(axis.clone()));
    assert_eq!(doc.y_axis, Nested::Patch(axis));
    assert_eq!(doc.x_axis, Nested::Unset);
    assert_eq!(doc.cells.size, set(3));
}

/// A 3D table: X and Y axes in their own slots (and both, in order, in the
/// one-axis slot), its two sizes, and its layout flags.
#[test]
fn kat_3d_table() {
    let doc = table(
        "<table type=\"3D\" name=\"Base Timing\" category=\"Ignition\" storageaddress=\"C1A20\" \
         storagetype=\"uint8\" endian=\"big\" sizex=\"16\" sizey=\"18\" swapxy=\"true\" \
         flipx=\"true\" flipy=\"false\" skipCells=\"2\" userlevel=\"2\">\n\
         <table type=\"X Axis\" name=\"Load\" storageaddress=\"C19E0\" storagetype=\"float\"/>\n\
         <table type=\"Y Axis\" name=\"RPM\" storageaddress=\"C1A00\" storagetype=\"float\"/>\n\
         </table>",
        &[],
    );
    let axis = |name: &str, address| AxisDoc {
        name: text(name),
        cells: CellsDoc {
            storage_address: set(address),
            storage: set(Storage::F32),
            ..CellsDoc::default()
        },
        ..AxisDoc::default()
    };
    let (x, y) = (axis("Load", 0xC_19E0), axis("RPM", 0xC_1A00));
    let want = TableDoc {
        name: text("Base Timing"),
        kind: set(TableKind::ThreeD),
        category: text("Ignition"),
        user_level: set(2),
        cells: CellsDoc {
            storage_address: set(0xC_1A20),
            storage: set(Storage::U8),
            endian: set(Endian::Big),
            // Legacy's one-value readings: sizey, flipy.
            size: set(18),
            flip: set(false),
            ..CellsDoc::default()
        },
        size_x: set(16),
        size_y: set(18),
        flip_x: set(true),
        flip_y: set(false),
        swap_xy: set(true),
        skip_cells: set(2),
        // Y merged over X: the later axis child wins, as in a 2D.
        axis: Nested::Patch(x.merged(&y)),
        x_axis: Nested::Patch(x),
        y_axis: Nested::Patch(y),
        ..TableDoc::default()
    };
    assert_eq!(doc, want);
}

/// A switch table: its states by lower-cased name, data as hex cell values.
#[test]
fn kat_switch_table() {
    let doc = table(
        "<table type=\"Switch\" name=\"CEL Flash\" category=\"Switches\" storageaddress=\"1F2C4\" \
         sizey=\"3\">\n\
         <description>Flash the CEL.</description>\n\
         <state name=\"On\" data=\"05 00 FF\"/>\n\
         <state name=\"off\" data=\"00,00, 0a\"/>\n\
         </table>",
        &[],
    );
    let mut states = Keyed::new();
    states.patch(
        "on".to_owned(),
        SwitchState {
            name: text("On"),
            data: set(vec![5, 0, 0xFF]),
        },
    );
    states.patch(
        "off".to_owned(),
        SwitchState {
            name: text("off"),
            data: set(vec![0, 0, 0xA]),
        },
    );
    let want = TableDoc {
        name: text("CEL Flash"),
        kind: set(TableKind::Switch),
        category: text("Switches"),
        description: text("Flash the CEL."),
        cells: CellsDoc {
            storage_address: set(0x1_F2C4),
            size: set(3),
            ..CellsDoc::default()
        },
        size_y: set(3),
        states,
        ..TableDoc::default()
    };
    assert_eq!(doc, want);
}

/// A bitwise switch table: its bits in order.
#[test]
fn kat_bitwise_switch_table() {
    let doc = table(
        "<table type=\"BitwiseSwitch\" name=\"Flags\" storageaddress=\"40\" storagetype=\"uint8\">\
         <bit position=\"0\" name=\"Launch\"/><bit position=\"7\" name=\"Flat Shift\"/></table>",
        &[],
    );
    let want = TableDoc {
        name: text("Flags"),
        kind: set(TableKind::BitwiseSwitch),
        cells: CellsDoc {
            storage_address: set(0x40),
            storage: set(Storage::U8),
            ..CellsDoc::default()
        },
        bits: Appended::append(vec![
            BitFlag {
                name: "Launch".to_owned(),
                position: 0,
            },
            BitFlag {
                name: "Flat Shift".to_owned(),
                position: 7,
            },
        ]),
        ..TableDoc::default()
    };
    assert_eq!(doc, want);
}

#[test]
fn table_types_ignore_case_and_axis_types_read_as_1d() {
    for (text, kind) in [
        ("1d", TableKind::OneD),
        ("2D", TableKind::TwoD),
        ("3d", TableKind::ThreeD),
        ("SWITCH", TableKind::Switch),
        ("bitwiseswitch", TableKind::BitwiseSwitch),
        ("X Axis", TableKind::OneD),
        ("static y axis", TableKind::OneD),
    ] {
        let doc = change(&format!("<table name=\"T\" type=\"{text}\"/>"), &[]);
        assert_eq!(doc.kind, set(kind), "{text}");
    }
}

#[test]
fn a_1d_or_switch_table_can_have_static_values() {
    let doc = table(
        "<table type=\"1D\" name=\"T\" sizey=\"2\"><data>a</data><data>b</data></table>",
        &[],
    );
    assert_eq!(
        doc.cells.static_values,
        set(vec!["a".to_owned(), "b".to_owned()])
    );
}

// ---- changes to inherited tables ----------------------------------------

/// A table in a `<rom>` with a base says only what it changes: everything
/// else is unset, and an axis without a table type goes by its letter.
#[test]
fn a_change_to_an_inherited_table_is_a_partial_document() {
    let doc = change(
        "<table name=\"Base Timing\" storageaddress=\"C2000\">\n\
         <table type=\"X Axis\" storageaddress=\"C1F00\"/>\n\
         <table type=\"Y Axis\" storageaddress=\"C1F40\"/></table>",
        &[],
    );
    let axis = |a| AxisDoc {
        cells: CellsDoc {
            storage_address: set(a),
            ..CellsDoc::default()
        },
        ..AxisDoc::default()
    };
    let want = TableDoc {
        name: text("Base Timing"),
        cells: CellsDoc {
            storage_address: set(0xC_2000),
            ..CellsDoc::default()
        },
        axis: Nested::Patch(axis(0xC_1F00).merged(&axis(0xC_1F40))),
        x_axis: Nested::Patch(axis(0xC_1F00)),
        y_axis: Nested::Patch(axis(0xC_1F40)),
        ..TableDoc::default()
    };
    assert_eq!(doc, want);
}

/// Merged along the `base` chain, a child's table patches the base's of
/// the same name (ignoring case); a 2D's one axis takes the child's values
/// whichever letter each layer used (as in the pinned corpus); an omitted
/// table is removed.
#[test]
fn tables_merge_over_their_base_by_name() {
    let xml = "<roms>\n\
               <rom><romid><xmlid>B</xmlid></romid>\n\
               <table type=\"2D\" name=\"Idle Target\" storagetype=\"uint8\" endian=\"big\" \
               sizey=\"4\" storageaddress=\"100\"><table type=\"X Axis\" name=\"ECT\" \
               storageaddress=\"200\" storagetype=\"float\"/></table>\n\
               <table type=\"1D\" name=\"Gone\"/>\n\
               </rom>\n\
               <rom base=\"B\"><romid><xmlid>C</xmlid></romid>\n\
               <table name=\"IDLE TARGET\" storageaddress=\"300\">\
               <table type=\"Y Axis\" storageaddress=\"400\"/></table>\n\
               <table name=\"gone\" omit=\"true\"/>\n\
               </rom>\n\
               </roms>";
    let (file, diags) = read("t.xml", xml.as_bytes()).accept_errors();
    assert!(diags.is_empty(), "{diags:#?}");
    let file = file.unwrap();
    let [base, child] = file.roms() else {
        unreachable!("two roms")
    };
    let mut stack: Layered<RomDoc> = Layered::new();
    stack.push(base.layer(), base.doc().clone());
    stack.push(child.layer(), child.doc().clone());
    let merged = stack.merged();
    let present: Vec<&String> = merged.tables.present().map(|(k, _)| k).collect();
    assert_eq!(present, ["idle target"]);
    assert!(
        merged
            .tables
            .get(&"gone".to_owned())
            .is_some_and(Nested::is_omitted)
    );
    let idle = merged.tables.present().next().unwrap().1;
    // The later layer's spelling, address and axis address win; the rest is
    // inherited.
    assert_eq!(idle.name, text("IDLE TARGET"));
    assert_eq!(idle.kind, set(TableKind::TwoD));
    assert_eq!(idle.cells.storage_address, set(0x300));
    assert_eq!(idle.cells.storage, set(Storage::U8));
    assert_eq!(idle.cells.size, set(4));
    let axis = idle.axis.get().unwrap();
    assert_eq!(axis.cells.storage_address, set(0x400));
    assert_eq!(axis.cells.storage, set(Storage::F32));
    assert_eq!(axis.name, text("ECT"));
}

/// A child's `sizex` overrides a base's `sizey` as the one data size of a
/// 1D/2D table, as legacy read it element by element.
#[test]
fn the_one_data_size_is_per_element() {
    let base = table("<table type=\"1D\" name=\"T\" sizey=\"10\"/>", &[]);
    let child = change("<table name=\"T\" sizex=\"5\"/>", &[]);
    let merged = base.merged(&child);
    assert_eq!(merged.cells.size, set(5));
    assert_eq!(merged.size_y, set(10));
    assert_eq!(merged.size_x, set(5));
}

#[test]
fn omit_removes_the_table_and_ignores_the_rest_with_a_warning() {
    let rom = rom_of(
        &in_rom(
            true,
            "<table name=\"A\" omit=\"TRUE\"/>\n<table name=\"B\" omit=\"false\"/>",
        ),
        &[],
    );
    assert!(state(&rom, "a").is_omitted());
    assert!(state(&rom, "b").get().is_some());
    let rom = rom_of(
        &in_rom(
            true,
            "<table name=\"A\" omit=\"true\" storageaddress=\"10\"><table type=\"X Axis\"/></table>",
        ),
        &[(codes::CONTENT_IGNORED, "3:1")],
    );
    assert!(state(&rom, "a").is_omitted());
    rom_of(
        &in_rom(true, "<table name=\"A\" omit=\"true\" category=\"c\"/>"),
        &[(codes::CONTENT_IGNORED, "3:1")],
    );
}

#[test]
fn a_table_base_names_another_table_and_needs_no_type() {
    let doc = table("<table name=\"T\" base=\"Other Table\" sizey=\"2\"/>", &[]);
    assert_eq!(doc.base, text("Other Table"));
    assert_eq!(doc.kind, Field::Unset);
    // `none`, in any case, is no base.
    let doc = change("<table name=\"T\" base=\"None\"/>", &[]);
    assert_eq!(doc.base, Field::Unset);
}

#[test]
fn an_axis_reads_its_description_and_size() {
    let doc = change(
        "<table name=\"T\"><table type=\"X Axis\" description=\"d\" sizex=\"4\" \
         flipx=\"true\"/></table>",
        &[],
    );
    let axis = doc.x_axis.get().unwrap();
    assert_eq!(axis.description, text("d"));
    assert_eq!(axis.cells.size, set(4));
    assert_eq!(axis.cells.flip, set(true));
}

#[test]
fn a_repeated_table_merges_with_a_warning() {
    let rom = rom_of(
        &in_rom(
            false,
            "<table type=\"1D\" name=\"T\" storageaddress=\"10\" category=\"A\"/>\n\
             <table name=\"t\" category=\"B\"/>",
        ),
        &[(codes::DUPLICATE_ELEMENT, "4:1")],
    );
    let doc = state(&rom, "t").get().unwrap();
    assert_eq!(doc.name, text("t"));
    assert_eq!(doc.category, text("B"));
    assert_eq!(doc.cells.storage_address, set(0x10));
    assert_eq!(doc.kind, set(TableKind::OneD));
}

#[test]
fn a_nan_attribute_is_absent_and_beforeram_false_changes_nothing() {
    let doc = change(
        "<table name=\"T\" storageaddress=\"NaN\" userlevel=\"NaN\" beforeram=\"false\"/>",
        &[],
    );
    assert_eq!(doc.cells.storage_address, Field::Unset);
    assert_eq!(doc.user_level, Field::Unset);
    assert_eq!(doc.cells.before_ram, Field::Unset);
}

#[test]
fn a_later_state_of_the_same_name_replaces_the_earlier_with_a_warning() {
    let doc = table(
        "<table type=\"Switch\" name=\"S\">\n<state name=\"on\" data=\"01\"/>\n\
         <state name=\"ON\" data=\"02\"/><state name=\"x\"/></table>",
        &[(codes::DUPLICATE_ELEMENT, "5:1")],
    );
    let on = doc.states.get(&"on".to_owned()).unwrap().get().unwrap();
    assert_eq!(on.name, text("ON"));
    assert_eq!(on.data, set(vec![2]));
    // Legacy's default data is 0.
    let x = doc.states.get(&"x".to_owned()).unwrap().get().unwrap();
    assert_eq!(x.data, set(vec![0]));
}

#[test]
fn every_listed_table_attribute_is_read() {
    for name in TABLE_ATTRIBUTES {
        let value = match *name {
            "name" => continue,
            "type" => "1D",
            "storagetype" => "uint8",
            "endian" => "big",
            "mask" => "F",
            "dataLayout" => "default",
            "omit" | "beforeram" | "flipx" | "flipy" | "swapxy" | "locked" => "false",
            "storageaddress" | "sizex" | "sizey" | "skipCells" | "userlevel" => "1",
            _ => "x",
        };
        change(&format!("<table name=\"T\" {name}=\"{value}\"/>"), &[]);
    }
}

// ---- diagnostics ----------------------------------------------------------

#[test]
fn malformed_values_are_errors_at_the_value() {
    // `<table name="T" type="1D" ` is 26 characters: the attribute starts at
    // column 27 and its value (after `=` and the quote) further on.
    for (attr, code) in [
        ("storageaddress=\"G0\"", codes::INVALID_ADDRESS),
        ("storageaddress=\"-1\"", codes::INVALID_ADDRESS),
        ("storageaddress=\"100000000\"", codes::INVALID_ADDRESS),
        ("storagetype=\"unit16\"", codes::UNSUPPORTED_STORAGE_TYPE),
        ("storagetype=\"2\"", codes::UNSUPPORTED_STORAGE_TYPE),
        ("endian=\"middle\"", codes::UNKNOWN_ENDIAN),
        ("mask=\"0x0F\"", codes::INVALID_BITFIELD),
        ("mask=\"0\"", codes::INVALID_BITFIELD),
        ("dataLayout=\"zigzag\"", codes::UNKNOWN_LAYOUT),
        ("sizex=\"0\"", codes::INVALID_NUMBER),
        ("sizey=\"-3\"", codes::INVALID_NUMBER),
        ("sizey=\"1.5\"", codes::INVALID_NUMBER),
        ("skipCells=\"-1\"", codes::INVALID_NUMBER),
        ("userlevel=\"high\"", codes::INVALID_NUMBER),
    ] {
        let value_col = 27 + attr.find('"').unwrap() + 1;
        let xml = in_rom(false, &format!("<table name=\"T\" type=\"1D\" {attr}/>"));
        let (file, diags) = read("t.xml", xml.as_bytes()).accept_errors();
        assert_eq!(
            found(&diags),
            [(code, format!("3:{value_col}"))],
            "{attr}: {diags:#?}"
        );
        assert!(diags[0].is_error());
        assert!(read("t.xml", xml.as_bytes()).value().is_none(), "withheld");
        let doc = file.unwrap().into_roms()[0]
            .doc()
            .tables
            .present()
            .next()
            .unwrap()
            .1
            .clone();
        assert_eq!(doc.cells, CellsDoc::default(), "{attr}: nothing set");
        assert_eq!(doc.skip_cells, Field::Unset);
        assert_eq!(doc.user_level, Field::Unset);
    }
}

#[test]
fn userlevel_outside_1_to_5_is_clamped_with_a_warning() {
    // `<table name="T" type="1D" userlevel="` is 37 characters.
    let level = |text: &str, expected: &[(Code, &str)]| {
        table(
            &format!("<table name=\"T\" type=\"1D\" userlevel=\"{text}\"/>"),
            expected,
        )
        .user_level
    };
    assert_eq!(level("5", &[]), set(5));
    assert_eq!(level(" 1 ", &[]), set(1));
    assert_eq!(
        level("7", &[(codes::USER_LEVEL_OUT_OF_RANGE, "3:38")]),
        set(5)
    );
    assert_eq!(
        level("0", &[(codes::USER_LEVEL_OUT_OF_RANGE, "3:38")]),
        set(1)
    );
    assert_eq!(
        level("-9", &[(codes::USER_LEVEL_OUT_OF_RANGE, "3:38")]),
        set(1)
    );
}

#[test]
fn bad_booleans_read_as_false_with_a_warning() {
    let doc = table(
        "<table name=\"T\" type=\"3D\" swapxy=\"yes\" locked=\"1\">\
         <table type=\"X Axis\"/><table type=\"Y Axis\"/></table>",
        &[
            (codes::INVALID_BOOLEAN, "3:35"),
            (codes::INVALID_BOOLEAN, "3:48"),
        ],
    );
    assert_eq!(doc.swap_xy, set(false));
    assert_eq!(doc.locked, set(false));
}

#[test]
fn table_type_name_and_axis_problems_are_errors() {
    // An unknown type, at its value; legacy's numeric codes too.
    for t in ["4D", "4", "1", "Map"] {
        rom_of(
            &in_rom(true, &format!("<table name=\"T\" type=\"{t}\"/>")),
            &[(codes::INVALID_TABLE_TYPE, "3:23")],
        );
    }
    // No type where nothing could supply one: a base-less rom, first use.
    rom_of(
        &in_rom(false, "<table name=\"T\"/>"),
        &[(codes::INVALID_TABLE_TYPE, "3:1")],
    );
    rom_of(
        &in_rom(false, "<table name=\"T\" type=\"none\"/>"),
        &[(codes::INVALID_TABLE_TYPE, "3:1")],
    );
    // ... but fine as a change (a base, or a table met earlier).
    rom_of(&in_rom(true, "<table name=\"T\"/>"), &[]);
    // No name: not read.
    let rom = rom_of(
        &in_rom(
            false,
            "<table type=\"1D\" storageaddress=\"1\"/>\n<table name=\"\"/>",
        ),
        &[
            (codes::MISSING_TABLE_NAME, "3:1"),
            (codes::MISSING_TABLE_NAME, "4:1"),
        ],
    );
    assert!(rom.doc().tables.is_empty());
    // Missing axes in a base-less rom.
    rom_of(
        &in_rom(false, "<table name=\"T\" type=\"2D\"/>"),
        &[(codes::MISSING_AXIS, "3:1")],
    );
    rom_of(
        &in_rom(
            false,
            "<table name=\"T\" type=\"3D\"><table type=\"X Axis\"/></table>",
        ),
        &[(codes::MISSING_AXIS, "3:1")],
    );
    // ... not in a change.
    rom_of(&in_rom(true, "<table name=\"T\" type=\"3D\"/>"), &[]);
    assert_eq!(
        messages(&in_rom(false, "<table name=\"T\" type=\"3D\"/>")),
        [
            "3D table `T` has no its X Axis and its Y Axis, and its <rom> has no base to inherit \
          one from"
        ]
    );
}

#[test]
fn bad_switch_data_and_bits_are_errors() {
    // `<table name="S" type="Switch"><state name="on" data="` is 53
    // characters.
    for data in ["0x01", "01 G2", "1FFFFFFFF", "-1", "01;02"] {
        let doc = table(
            &format!(
                "<table name=\"S\" type=\"Switch\"><state name=\"on\" data=\"{data}\"/></table>"
            ),
            &[(codes::INVALID_STATE_DATA, "3:54")],
        );
        assert!(doc.states.is_empty(), "{data}");
    }
    // `<table name="B" type="BitwiseSwitch"><bit position="` is 52.
    for position in ["32", "-1", "x"] {
        let doc = table(
            &format!(
                "<table name=\"B\" type=\"BitwiseSwitch\"><bit position=\"{position}\"/></table>"
            ),
            &[(codes::INVALID_NUMBER, "3:53")],
        );
        assert!(doc.bits.items().is_empty());
    }
    let doc = table(
        "<table name=\"B\" type=\"BitwiseSwitch\"><bit name=\"n\"/></table>",
        &[(codes::CONTENT_IGNORED, "3:38")],
    );
    assert!(doc.bits.items().is_empty());
}

/// Every unknown attribute and element is a warning naming it, at its
/// `line:col`, on tables, axes, states and bits.
#[test]
fn unknown_attributes_and_elements_are_warnings_naming_them() {
    let xml = in_rom(
        false,
        "<table name=\"T\" type=\"2D\" level=\"1\" scaling=\"s\">\n\
         <table type=\"Y Axis\" elements=\"4\" userlevel=\"2\"><scalin/>stray</table>\n\
         <state name=\"on\" data=\"1\" extra=\"\"><x/></state>\n\
         <bit position=\"1\" mask=\"1\"/>\n\
         <fish/>\n\
         </table>",
    );
    rom_of(
        &xml,
        &[
            (codes::UNKNOWN_ATTRIBUTE, "3:27"),
            (codes::UNKNOWN_ATTRIBUTE, "3:37"),
            (codes::UNKNOWN_ATTRIBUTE, "4:22"),
            (codes::UNKNOWN_ATTRIBUTE, "4:35"),
            (codes::UNKNOWN_ELEMENT, "4:49"),
            (codes::UNEXPECTED_TEXT, "4:58"),
            (codes::UNKNOWN_ATTRIBUTE, "5:27"),
            (codes::UNKNOWN_ELEMENT, "5:36"),
            (codes::UNKNOWN_ATTRIBUTE, "6:19"),
            (codes::UNKNOWN_ELEMENT, "7:1"),
        ],
    );
    assert_eq!(
        messages(&xml),
        [
            "unknown attribute `level` on <table>; ignored",
            "unknown attribute `scaling` on <table>; ignored",
            "unknown attribute `elements` on an axis <table>; ignored",
            "unknown attribute `userlevel` on an axis <table>; ignored",
            "unknown element <scalin> in an axis <table>; ignored",
            "text directly inside an axis <table>; ignored",
            "unknown attribute `extra` on <state>; ignored",
            "unknown element <x> in <state>; ignored",
            "unknown attribute `mask` on <bit>; ignored",
            "unknown element <fish> in <table>; ignored",
        ]
    );
}

/// What legacy ignored where it is is a warning, and is not read.
#[test]
fn content_with_no_effect_where_it_is_is_a_warning() {
    // An axis on a table that has none.
    let doc = table(
        "<table name=\"T\" type=\"Switch\">\n<table type=\"X Axis\" storageaddress=\"1\"/></table>",
        &[(codes::CONTENT_IGNORED, "4:1")],
    );
    assert!(doc.x_axis.is_unset() && doc.axis.is_unset());
    // A child table that is not an axis (as a misplaced table in the
    // pinned corpus), or a dead numeric axis type.
    for child in ["<table name=\"Checksum Fix\"/>", "<table type=\"4\"/>"] {
        let doc = change(
            &format!("<table name=\"T\">\n{child}</table>"),
            &[(codes::CONTENT_IGNORED, "4:1")],
        );
        assert!(doc.axis.is_unset(), "{child}");
    }
    // Static values on a 3D table; an axis inside an axis.
    let doc = table(
        "<table name=\"T\" type=\"3D\">\n<data>1</data>\n<table type=\"X Axis\">\
         <table type=\"Y Axis\"/></table><table type=\"Y Axis\"/></table>",
        &[
            (codes::CONTENT_IGNORED, "4:1"),
            (codes::CONTENT_IGNORED, "5:22"),
        ],
    );
    assert_eq!(doc.cells.static_values, Field::Unset);
    assert!(doc.x_axis.get().is_some() && doc.y_axis.get().is_some());
    // A 2D table's second axis is read into its one axis, with a warning.
    let doc = table(
        "<table name=\"T\" type=\"2D\">\n<table type=\"X Axis\" name=\"A\" storageaddress=\"1\"/>\n\
         <table type=\"Y Axis\" name=\"B\"/></table>",
        &[(codes::DUPLICATE_ELEMENT, "5:1")],
    );
    let axis = doc.axis.get().unwrap();
    assert_eq!(axis.name, text("B"));
    assert_eq!(axis.cells.storage_address, set(1));
    // A 3D table's repeated X axis.
    table(
        "<table name=\"T\" type=\"3D\">\n<table type=\"X Axis\"/>\n<table type=\"X Axis\"/>\n\
         <table type=\"Y Axis\"/></table>",
        &[(codes::DUPLICATE_ELEMENT, "5:1")],
    );
}

#[test]
fn a_static_axis_whose_value_count_differs_from_its_size_warns() {
    // From its own size.
    table(
        "<table name=\"T\" type=\"2D\" sizey=\"2\"><table type=\"Static Y Axis\" sizey=\"2\">\n\
         <data>1</data></table></table>",
        &[(codes::STATIC_AXIS_SIZE, "4:1")],
    );
    // From its table's size along it (as six tables in the pinned corpus).
    let xml = in_rom(
        false,
        "<table name=\"T\" type=\"2D\" sizey=\"3\"><table type=\"Static Y Axis\" sizey=\"1\">\n\
         <data>1</data></table></table>",
    );
    rom_of(&xml, &[(codes::STATIC_AXIS_SIZE, "4:1")]);
    assert_eq!(
        messages(&xml),
        ["this static axis has 1 <data> value(s) but its table has 3 cell(s) along it"]
    );
    // A 3D's axes go by its sizex and sizey.
    for (x, y, at) in [("3", "1", "4:1"), ("2", "2", "5:1")] {
        table(
            &format!(
                "<table name=\"T\" type=\"3D\" sizex=\"{x}\" sizey=\"{y}\"><table \
                 type=\"Static X Axis\">\n<data>1</data><data>2</data></table><table \
                 type=\"Static Y Axis\">\n<data>1</data></table></table>"
            ),
            &[(codes::STATIC_AXIS_SIZE, at)],
        );
    }
    // A 3D's X axis goes by its sizex.
    table(
        "<table name=\"T\" type=\"3D\" sizex=\"2\" sizey=\"1\"><table type=\"Static X Axis\">\n\
         <data>1</data><data>2</data></table><table type=\"Static Y Axis\">\n<data>1</data>\
         </table></table>",
        &[],
    );
}

// ---- never panics ---------------------------------------------------------

/// Attribute text: numbers, hex, names and odd.
fn attr_text() -> impl Strategy<Value = String> {
    prop_oneof![
        "[-+ 0-9a-fA-FxX,]{0,10}",
        "(1D|2D|3D|Switch|BitwiseSwitch|X Axis|Static Y Axis|true|false|NaN|none|uint8|float)",
        any::<String>().prop_map(|s| {
            s.chars()
                .filter(|c| !matches!(c, '<' | '&' | '"') && !c.is_control())
                .collect()
        }),
    ]
}

proptest! {
    #[test]
    fn any_table_attribute_values_read_without_panicking(
        base in any::<bool>(),
        values in proptest::collection::vec(attr_text(), 12),
        axis_type in attr_text(),
        data in attr_text(),
    ) {
        let names = [
            "type", "storageaddress", "storagetype", "endian", "mask", "dataLayout",
            "sizex", "sizey", "userlevel", "skipCells", "flipx", "omit",
        ];
        let attrs: String = names
            .iter()
            .zip(&values)
            .map(|(n, v)| format!(" {n}=\"{v}\""))
            .collect();
        let xml = in_rom(
            base,
            &format!(
                "<table name=\"T\"{attrs}><table type=\"{axis_type}\" sizey=\"{data}\">\
                 <data>{data}</data></table><state name=\"s\" data=\"{data}\"/>\
                 <bit position=\"{data}\"/></table>"
            ),
        );
        let _ = read("p.xml", xml.as_bytes()).accept_errors();
    }
}
