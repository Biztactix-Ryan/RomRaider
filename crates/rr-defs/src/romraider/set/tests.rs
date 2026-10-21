//! Unit tests of definition sets (US-RR-29-8): `base` chains across files,
//! table merging and `omit`, duplicate and missing names, cycles, and the
//! typed tables of a resolved ROM.

use rr_core::diag::{Code, Diagnostic, Span, codes};
use rr_core::encoding::{Endian, Storage};
use rr_core::layer::{Field, Nested};

use super::super::{read, table_key};
use super::{DefinitionSet, ResolvedRom, RomRef, table_path, xmlid_key};

/// The set of `files` (`(source, xml)`), accepting errors.
fn set_of(files: &[(&str, &str)]) -> (DefinitionSet, Vec<Diagnostic>) {
    DefinitionSet::read_all(files.iter().map(|(s, x)| (*s, x.as_bytes()))).accept_errors()
}

/// `(code, "source:line:col")` of each diagnostic.
fn found(diags: &[Diagnostic]) -> Vec<(Code, String)> {
    diags
        .iter()
        .map(|d| {
            let p = d.provenance();
            let at = match p.span() {
                Span::LineCol { line, col } => format!("{}:{line}:{col}", p.source()),
                Span::ByteOffset(o) => format!("{}@{o}", p.source()),
            };
            (d.code(), at)
        })
        .collect()
}

/// `expected` as owned pairs, to compare with [`found`].
fn want(expected: &[(Code, &str)]) -> Vec<(Code, String)> {
    expected
        .iter()
        .map(|(c, a)| (*c, (*a).to_owned()))
        .collect()
}

/// Resolves `xmlid` in `set` (as [`DefinitionSet::find`] finds it),
/// accepting errors.
fn resolve(set: &DefinitionSet, xmlid: &str) -> (Option<ResolvedRom>, Vec<Diagnostic>) {
    let rom = set.find(xmlid).unwrap();
    set.resolver().resolve(rom).accept_errors()
}

/// The `(source, line)` of each layer of `rom`'s chain, weakest first, by
/// layer id.
fn chain(rom: &ResolvedRom) -> Vec<String> {
    rom.lineage()
        .stack()
        .layers()
        .map(|(l, _)| format!("{} {}{}", l.id(), l.source(), l.span()))
        .collect()
}

/// A base definition, each table on its own line from line 3:
///
/// * `Boost Target`: 3D `uint16`, X axis `uint8` (scaled `rpm`), Y axis
///   `uint16`, scaled `x*0.01` `psi`;
/// * `Fuel Map`: 2D `uint8` with an `X Axis` (`uint16`);
/// * `Idle Speed`, `Rev Limit`: 2D `uint16` with static axes;
/// * `Mode`: a switch with two states.
///
/// No addresses: a child places them.
const BASE: &str = r#"<roms>
<rom><romid><xmlid>32BITBASE</xmlid></romid>
<table type="3D" name="Boost Target" category="Boost" storagetype="uint16" endian="big" sizex="2" sizey="2"><scaling units="psi" expression="x*0.01" format="0.00"/><table type="X Axis" name="RPM" storagetype="uint8"><scaling units="rpm" expression="x*50"/></table><table type="Y Axis" name="Load" storagetype="uint16"/></table>
<table type="2D" name="Fuel Map" storagetype="uint8" sizey="4"><table type="X Axis" name="RPM" storagetype="uint16"/></table>
<table type="2D" name="Idle Speed" storagetype="uint16" sizey="1"><table type="Static Y Axis" name="Idle" sizey="1"><data>idle</data></table></table>
<table type="2D" name="Rev Limit" storagetype="uint16" sizey="1"><table type="Static Y Axis" name="Rev" sizey="1"><data>limit</data></table></table>
<table type="Switch" name="Mode" sizey="1"><state name="on" data="01"/><state name="off" data="00"/></table>
</rom>
</roms>"#;

/// A child of `32BITBASE` in its own file: places `Boost Target` (with a
/// signed storage, line 3), `Fuel Map` (with a `Y Axis` child, line 4) and
/// `Mode` (line 5).
const CHILD: &str = r#"<roms>
<rom base="32BITBASE"><romid><xmlid>A2WC522N</xmlid><internalidaddress>200</internalidaddress><internalidstring>A2WC522N</internalidstring></romid>
<table name="Boost Target" storageaddress="1000" storagetype="int16"><table type="X Axis" storageaddress="1100"/><table type="Y Axis" storageaddress="1200"/></table>
<table name="Fuel Map" storageaddress="2000"><table type="Y Axis" storageaddress="2100"/></table>
<table name="Mode" storageaddress="3000"/>
</rom>
</roms>"#;

// ---- two- and three-level chains -----------------------------------------

#[test]
fn a_child_in_another_file_is_its_base_with_its_own_changes_on_top() {
    let (set, diags) = set_of(&[("base.xml", BASE), ("child.xml", CHILD)]);
    assert_eq!(found(&diags), []);
    let (rom, diags) = resolve(&set, "A2WC522N");
    assert_eq!(found(&diags), []);
    let rom = rom.unwrap();
    assert_eq!(
        chain(&rom),
        ["32BITBASE base.xml:2:1", "A2WC522N child.xml:2:1"]
    );
    let names: Vec<&str> = rom.tables().iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Boost Target", "Fuel Map", "Mode"]);
    assert_eq!(rom.unplaced(), ["Idle Speed", "Rev Limit"]);

    // One attribute overridden (MF-06), every other one inherited.
    let boost = rom.table("boost target").unwrap();
    let cells = boost.cells.as_ref().unwrap();
    assert_eq!(cells.encoding.storage(), Storage::I16);
    assert_eq!(cells.encoding.endian(), Endian::Big);
    assert_eq!(cells.conversion.unit(), Some("psi"));
    assert_eq!(boost.doc.category, Field::Set("Boost".to_owned()));
    assert_eq!(boost.doc.cells.storage_address, Field::Set(0x1000));
    assert_eq!(boost.doc.size_x, Field::Set(2));
    // Its axes: the base's storage and scaling, the child's address.
    let [x, y] = boost.axes.as_slice() else {
        panic!("{:?}", boost.axes)
    };
    assert_eq!(x.doc.name, Field::Set("RPM".to_owned()));
    assert_eq!(x.doc.cells.storage_address, Field::Set(0x1100));
    let x_cells = x.cells.as_ref().unwrap();
    assert_eq!(x_cells.encoding.storage(), Storage::U8);
    assert_eq!(x_cells.conversion.unit(), Some("rpm"));
    assert_eq!(y.doc.cells.storage_address, Field::Set(0x1200));
    assert_eq!(y.cells.as_ref().unwrap().encoding.storage(), Storage::U16);
    // Reported at the child's <table>, in the child's layer.
    assert_eq!(boost.at.to_string(), "child.xml:3:1 [layer A2WC522N]");

    // A switch without a storage type is uint8, as in legacy.
    let mode = rom.table("Mode").unwrap();
    assert_eq!(mode.cells.as_ref().unwrap().encoding.storage(), Storage::U8);
    assert_eq!(mode.doc.states.len(), 2);

    // Identification is the child's own.
    let mut image = vec![0; 0x300];
    image[0x200..0x208].copy_from_slice(b"A2WC522N");
    assert!(rom.identification().unwrap().matches(&image));
    assert_eq!(set.identify(&image), set.find("a2wc522n"));
}

#[test]
fn explain_names_the_definition_and_file_that_set_a_field() {
    let (set, _) = set_of(&[("base.xml", BASE), ("child.xml", CHILD)]);
    let (rom, _) = resolve(&set, "A2WC522N");
    let rom = rom.unwrap();
    let storage = table_path("Boost Target")
        .field("cells", |t: &super::TableDoc| &t.cells)
        .field("storage", |c: &super::CellsDoc| &c.storage);
    assert_eq!(
        rom.lineage().explain(&storage).to_string(),
        "tables[\"boost target\"].cells.storage = I16
  set to I16 at child.xml:2:1 [layer A2WC522N]
  overrides:
    set to U16 at base.xml:2:1 [layer 32BITBASE]"
    );
}

#[test]
fn a_three_level_chain_applies_each_level_over_the_one_below() {
    let family = r#"<roms>
<rom base="32BITBASE"><romid><xmlid>FAMILY</xmlid></romid>
<table name="Boost Target" category="Boost (family)"><scaling units="bar" expression="x*0.001"/></table>
</rom>
<rom base="family"><romid><xmlid>A2WC522N</xmlid></romid>
<table name="Boost Target" storageaddress="1000"><table type="X Axis" storageaddress="1100"/><table type="Y Axis" storageaddress="1200"/></table>
</rom>
</roms>"#;
    let (set, diags) = set_of(&[("base.xml", BASE), ("family.xml", family)]);
    assert_eq!(found(&diags), []);
    let (rom, diags) = resolve(&set, "A2WC522N");
    assert_eq!(found(&diags), []);
    let rom = rom.unwrap();
    assert_eq!(
        chain(&rom),
        [
            "32BITBASE base.xml:2:1",
            "FAMILY family.xml:2:1",
            "A2WC522N family.xml:5:1"
        ]
    );
    let boost = rom.table("Boost Target").unwrap();
    let cells = boost.cells.as_ref().unwrap();
    // The base's storage, the family's scaling and category, the ROM's
    // address.
    assert_eq!(cells.encoding.storage(), Storage::U16);
    assert_eq!(cells.conversion.unit(), Some("bar"));
    assert_eq!(boost.doc.category, Field::Set("Boost (family)".to_owned()));
    assert_eq!(boost.doc.cells.storage_address, Field::Set(0x1000));
    // The family itself resolves too: its table has no address.
    let (family, diags) = resolve(&set, "family");
    assert_eq!(found(&diags), []);
    assert!(family.unwrap().tables().is_empty());
}

// ---- omit ------------------------------------------------------------------

#[test]
fn omit_removes_the_one_inherited_table_it_names() {
    // MF-07: legacy's filter stopped at the first omitted table, so every
    // table after it lost its address.
    let child = r#"<roms>
<rom base="32BITBASE"><romid><xmlid>MID</xmlid></romid>
<table name="Fuel Map" omit="true"/>
</rom>
<rom base="MID"><romid><xmlid>A2WC522N</xmlid></romid>
<table name="Mode" storageaddress="3000"/>
<table name="Idle Speed" storageaddress="4000"/>
<table name="Rev Limit" storageaddress="5000"/>
</rom>
</roms>"#;
    let (set, _) = set_of(&[("base.xml", BASE), ("child.xml", child)]);
    let (rom, diags) = resolve(&set, "A2WC522N");
    assert_eq!(found(&diags), []);
    let rom = rom.unwrap();
    let names: Vec<&str> = rom.tables().iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Idle Speed", "Rev Limit", "Mode"]);
    assert_eq!(rom.unplaced(), ["Boost Target"]);
    assert!(
        rom.doc()
            .tables
            .get(&table_key("Fuel Map"))
            .is_some_and(Nested::is_omitted)
    );
    // Static axes need no encoding.
    let idle = rom.table("Idle Speed").unwrap();
    assert!(idle.axes[0].cells.is_none());
    assert_eq!(
        idle.axes[0].doc.cells.static_values,
        Field::Set(vec!["idle".to_owned()])
    );
}

// ---- axes ------------------------------------------------------------------

#[test]
fn a_2d_table_changed_with_another_axis_letter_changes_its_one_axis() {
    // The pinned corpus has 111 2D tables whose base says `X Axis` and whose
    // child says `Y Axis`: legacy read both into the one axis.
    let (set, _) = set_of(&[("base.xml", BASE), ("child.xml", CHILD)]);
    let (rom, _) = resolve(&set, "A2WC522N");
    let rom = rom.unwrap();
    let fuel = rom.table("Fuel Map").unwrap();
    let [axis] = fuel.axes.as_slice() else {
        panic!("{:?}", fuel.axes)
    };
    assert_eq!(axis.doc.name, Field::Set("RPM".to_owned()));
    assert_eq!(axis.doc.cells.storage_address, Field::Set(0x2100));
    assert_eq!(
        axis.cells.as_ref().unwrap().encoding.storage(),
        Storage::U16
    );
}

// ---- MF-23 -----------------------------------------------------------------

#[test]
fn mf23_a_table_that_cannot_be_resolved_leaves_the_others_resolved() {
    // Legacy removed tables from the map it was iterating over, and used a
    // NullPointerException to drop a broken one (`maps/Rom.java:303-324`).
    // Here an unplaced table is listed, a broken one is a diagnostic, and
    // every other table resolves, in order.
    let base = r#"<roms>
<rom><romid><xmlid>BASE</xmlid></romid>
<table type="2D" name="A" storagetype="uint8"><table type="X Axis" storagetype="uint8"/></table>
<table type="2D" name="No Storage"><table type="X Axis" storagetype="uint8"/></table>
<table type="2D" name="Unplaced" storagetype="uint8"><table type="X Axis" storagetype="uint8"/></table>
<table type="2D" name="Z" storagetype="uint8"><table type="X Axis" storagetype="uint8"/></table>
</rom>
<rom base="BASE"><romid><xmlid>ROM</xmlid></romid>
<table name="A" storageaddress="10"><table type="X Axis" storageaddress="20"/></table>
<table name="No Storage" storageaddress="30"><table type="X Axis" storageaddress="40"/></table>
<table name="Not In Base" storageaddress="50"/>
<table name="Z" storageaddress="60"><table type="X Axis" storageaddress="70"/></table>
</rom>
</roms>"#;
    let (set, diags) = set_of(&[("t.xml", base)]);
    assert_eq!(found(&diags), []);
    let rom = set.find("ROM").unwrap();
    let loaded = set.resolver().resolve(rom);
    // Fail closed: an Error withholds the ROM unless accepted.
    assert!(loaded.value().is_none());
    let (rom, diags) = loaded.accept_errors();
    assert_eq!(
        found(&diags),
        want(&[
            (codes::MISSING_STORAGE_TYPE, "t.xml:10:1"),
            (codes::UNTYPED_TABLE, "t.xml:11:1"),
        ])
    );
    assert!(diags[1].message().contains("`Not In Base`"), "{}", diags[1]);
    let rom = rom.unwrap();
    let names: Vec<&str> = rom.tables().iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["A", "Z"]);
    assert_eq!(rom.unplaced(), ["Unplaced"]);
}

#[test]
fn a_3d_table_missing_an_axis_after_the_merge_is_an_error_at_it() {
    let xml = r#"<roms>
<rom><romid><xmlid>BASE</xmlid></romid>
<table type="2D" name="T" storagetype="uint8"><table type="X Axis" storagetype="uint8"/></table>
</rom>
<rom base="BASE"><romid><xmlid>ROM</xmlid></romid>
<table name="T" type="3D" storageaddress="10"/>
</rom>
</roms>"#;
    let (set, _) = set_of(&[("t.xml", xml)]);
    let (rom, diags) = resolve(&set, "ROM");
    assert_eq!(found(&diags), want(&[(codes::MISSING_AXIS, "t.xml:6:1")]));
    assert!(diags[0].message().contains("Y Axis"), "{}", diags[0]);
    assert!(rom.unwrap().tables().is_empty());
}

// ---- missing bases and cycles -----------------------------------------------

#[test]
fn a_missing_base_is_an_error_at_the_base_attribute() {
    let xml = r#"<roms>
<rom base="NOPE"><romid><xmlid>MID</xmlid></romid></rom>
<rom base="MID"><romid><xmlid>ROM</xmlid></romid></rom>
</roms>"#;
    let (set, diags) = set_of(&[("t.xml", xml)]);
    assert_eq!(found(&diags), []);
    let (rom, diags) = resolve(&set, "MID");
    assert!(rom.is_none());
    assert_eq!(found(&diags), want(&[(codes::UNKNOWN_BASE, "t.xml:2:12")]));
    assert_eq!(
        diags[0].message(),
        "`MID` names base `NOPE`, which no loaded definition file defines"
    );
    // A definition inheriting from it fails with it, at the same place.
    let (rom, diags) = resolve(&set, "ROM");
    assert!(rom.is_none());
    assert_eq!(found(&diags), want(&[(codes::UNKNOWN_BASE, "t.xml:2:12")]));
    assert!(
        diags[0].message().ends_with("; `ROM` inherits from it"),
        "{}",
        diags[0]
    );
}

#[test]
fn a_base_loop_is_a_cycle_error_naming_every_definition_on_it() {
    // MF-17: legacy recursed until the stack overflowed.
    let a = r#"<roms>
<rom base="B"><romid><xmlid>A</xmlid></romid></rom>
</roms>"#;
    let b = r#"<roms>
<rom base="a"><romid><xmlid>B</xmlid></romid></rom>
</roms>"#;
    let (set, _) = set_of(&[("a.xml", a), ("b.xml", b)]);
    let (rom, diags) = resolve(&set, "A");
    assert!(rom.is_none());
    assert_eq!(
        found(&diags),
        want(&[(codes::INHERITANCE_CYCLE, "b.xml:2:1")])
    );
    assert_eq!(
        diags[0].message(),
        "inheritance cycle: A (a.xml:2:1) -> B (b.xml:2:1) -> A"
    );
}

#[test]
fn a_definition_naming_itself_extends_another_files_definition_of_the_name() {
    let base = r#"<roms>
<rom><romid><xmlid>X</xmlid><make>Subaru</make></romid></rom>
</roms>"#;
    let patch = r#"<roms>
<rom base="X"><romid><xmlid>X</xmlid><model>WRX</model></romid></rom>
</roms>"#;
    let (set, _) = set_of(&[("base.xml", base), ("patch.xml", patch)]);
    let patched = RomRef { file: 1, rom: 0 };
    let (rom, diags) = set.resolver().resolve(patched).accept_errors();
    assert_eq!(found(&diags), []);
    let rom = rom.unwrap();
    assert_eq!(chain(&rom), ["X base.xml:2:1", "X@patch.xml patch.xml:2:1"]);
    assert_eq!(rom.romid().make, Field::Set("Subaru".to_owned()));
    assert_eq!(rom.romid().model, Field::Set("WRX".to_owned()));

    // With no other definition of the name, it is an unknown base.
    let (set, _) = set_of(&[("patch.xml", patch)]);
    let (rom, diags) = resolve(&set, "X");
    assert!(rom.is_none());
    assert_eq!(
        found(&diags),
        want(&[(codes::UNKNOWN_BASE, "patch.xml:2:12")])
    );
    assert!(
        diags[0].message().contains("names itself as its base"),
        "{}",
        diags[0]
    );
}

// ---- duplicate names ----------------------------------------------------------

#[test]
fn two_roms_of_one_name_in_one_file_keep_the_first() {
    let xml = r#"<roms>
<rom><romid><xmlid>BASE</xmlid><make>first</make></romid></rom>
<rom><romid><xmlid>base</xmlid><make>second</make></romid></rom>
<rom base="Base"><romid><xmlid>ROM</xmlid></romid></rom>
</roms>"#;
    let (set, diags) = set_of(&[("t.xml", xml)]);
    assert_eq!(
        found(&diags),
        want(&[(codes::DUPLICATE_XMLID, "t.xml:3:1")])
    );
    assert!(
        diags[0].message().contains("first at t.xml:2:1"),
        "{}",
        diags[0]
    );
    assert_eq!(set.roms().count(), 2);
    let (rom, _) = resolve(&set, "ROM");
    assert_eq!(rom.unwrap().romid().make, Field::Set("first".to_owned()));
}

#[test]
fn each_file_s_own_copy_of_a_base_serves_that_file() {
    // RomRaider's per-ROM files each carry their own `32BITBASE`.
    let file = |make: &str, rom: &str| {
        format!(
            "<roms>\n<rom><romid><xmlid>32BITBASE</xmlid><make>{make}</make></romid></rom>\n\
             <rom base=\"32BITBASE\"><romid><xmlid>{rom}</xmlid></romid></rom>\n</roms>"
        )
    };
    let one = file("one", "ROM1");
    let two = file("two", "ROM2");
    let three = r#"<roms>
<rom base="32bitbase"><romid><xmlid>ROM3</xmlid></romid></rom>
</roms>"#;
    let (set, diags) = set_of(&[("one.xml", &one), ("two.xml", &two), ("three.xml", three)]);
    assert_eq!(
        found(&diags),
        want(&[(codes::DUPLICATE_XMLID, "two.xml:2:1")])
    );
    assert!(
        diags[0].message().contains("also defined in one.xml:2:1"),
        "{}",
        diags[0]
    );
    let make = |xmlid| resolve(&set, xmlid).0.unwrap().romid().make.clone();
    assert_eq!(make("ROM1"), Field::Set("one".to_owned()));
    assert_eq!(make("ROM2"), Field::Set("two".to_owned()));
    // A file without one gets the first file's, as does a lookup by name.
    assert_eq!(make("ROM3"), Field::Set("one".to_owned()));
    assert_eq!(set.find("32BITBASE"), Some(RomRef { file: 0, rom: 0 }));
    let ids: Vec<String> = set
        .roms()
        .map(|(r, _)| set.id(r).unwrap().to_string())
        .collect();
    assert_eq!(
        ids,
        ["32BITBASE", "ROM1", "32BITBASE@two.xml", "ROM2", "ROM3"]
    );
}

#[test]
fn xmlids_compare_ignoring_case() {
    assert_eq!(xmlid_key("a2wc522n"), "A2WC522N");
    let (set, _) = set_of(&[("base.xml", BASE), ("child.xml", CHILD)]);
    assert_eq!(set.find("a2Wc522N"), Some(RomRef { file: 1, rom: 0 }));
    assert_eq!(set.find("nope"), None);
}

// ---- table and scaling bases ----------------------------------------------------

#[test]
fn a_table_base_starts_the_table_as_a_copy_of_another_table() {
    let xml = r#"<roms>
<rom><romid><xmlid>ROM</xmlid><memmodel endian="little">SH7058</memmodel></romid>
<table type="2D" name="Fuel A" storagetype="uint16" storageaddress="10" category="Fuel"><table type="X Axis" storagetype="uint8" storageaddress="20"/></table>
<table name="Fuel B" base="Fuel A" storageaddress="30"><table type="X Axis" storageaddress="40"/></table>
<table name="Fuel C" base="Nope" type="1D" storagetype="uint8" storageaddress="50"/>
<table name="Fuel D" base="fuel d" type="1D" storagetype="uint8" storageaddress="60"/>
</rom>
</roms>"#;
    let (set, diags) = set_of(&[("t.xml", xml)]);
    assert_eq!(found(&diags), []);
    let (rom, diags) = resolve(&set, "ROM");
    assert_eq!(found(&diags), want(&[(codes::UNKNOWN_BASE, "t.xml:5:1")]));
    let rom = rom.unwrap();
    let names: Vec<&str> = rom.tables().iter().map(|t| t.name.as_str()).collect();
    // `Fuel C` names a table that is not there; `Fuel D` names itself,
    // which changes nothing.
    assert_eq!(names, ["Fuel A", "Fuel B", "Fuel C", "Fuel D"]);
    let b = rom.table("Fuel B").unwrap();
    assert_eq!(b.name, "Fuel B");
    assert_eq!(b.doc.category, Field::Set("Fuel".to_owned()));
    assert_eq!(b.doc.cells.storage_address, Field::Set(0x30));
    let cells = b.cells.as_ref().unwrap();
    assert_eq!(cells.encoding.storage(), Storage::U16);
    // The ROM's memory model's byte order overrides every table's.
    assert_eq!(cells.encoding.endian(), Endian::Little);
    assert_eq!(b.axes[0].doc.cells.storage_address, Field::Set(0x40));
}

#[test]
fn a_scaling_base_is_a_scalingbase_of_the_files_of_the_chain() {
    let xml = r#"<roms>
<scalingbase name="Psi" units="psi" expression="x*2" format="0.0"/>
<rom><romid><xmlid>ROM</xmlid></romid>
<table type="Switch" name="A" storageaddress="10"><scaling base="psi" format="0.00"/></table>
<table type="Switch" name="B" storageaddress="20"><scaling base="bar"/></table>
</rom>
</roms>"#;
    let (set, diags) = set_of(&[("t.xml", xml)]);
    assert_eq!(found(&diags), []);
    let (rom, diags) = resolve(&set, "ROM");
    assert_eq!(
        found(&diags),
        want(&[(codes::UNKNOWN_SCALING, "t.xml:5:1")])
    );
    let rom = rom.unwrap();
    assert_eq!(rom.tables().len(), 1);
    let a = rom.table("A").unwrap();
    let scaling = a.scaling.as_ref().unwrap();
    assert_eq!(scaling.units, Field::Set("psi".to_owned()));
    assert_eq!(scaling.to_real.get().map(|f| f.text()), Some("x*2"));
    let cells = a.cells.as_ref().unwrap();
    assert_eq!(
        cells.conversion.format().map(ToString::to_string),
        Some("0.00".to_owned())
    );
}

#[test]
fn a_to_byte_that_does_not_invert_the_expression_is_a_warning_per_table() {
    let xml = r#"<roms>
<rom><romid><xmlid>ROM</xmlid></romid>
<table type="Switch" name="A" storageaddress="10"><scaling expression="x*2" to_byte="x*2"/></table>
<table type="Switch" name="B" storageaddress="20"><scaling expression="x*2" to_byte="x*2"/></table>
<table type="Switch" name="C" storageaddress="30"><scaling expression="x*2" to_byte="x/2"/></table>
</rom>
</roms>"#;
    let (set, _) = set_of(&[("t.xml", xml)]);
    let (rom, diags) = resolve(&set, "ROM");
    assert_eq!(
        found(&diags),
        want(&[
            (codes::TO_RAW_MISMATCH, "t.xml:3:1"),
            (codes::TO_RAW_MISMATCH, "t.xml:4:1"),
        ])
    );
    assert_eq!(rom.unwrap().tables().len(), 3);
}

// ---- nothing dropped in silence --------------------------------------------------

#[test]
fn unknown_attributes_are_reported_whatever_their_value() {
    // `NaN` is legacy's absent value for a known attribute; an unknown one
    // is still named.
    let xml = r#"<roms>
<rom><romid><xmlid>ROM</xmlid></romid>
<table type="2D" name="T" storagetype="uint8" sizey="NaN" foo="NaN"><scaling units="NaN" bar="NaN"/><table type="X Axis" storagetype="uint8" baz="NaN"/></table>
<table name="U" omit="true"><nope/><description>x</description></table>
</rom>
</roms>"#;
    let (file, diags) = read("t.xml", xml.as_bytes()).accept_errors();
    assert!(file.is_some());
    assert_eq!(
        found(&diags),
        want(&[
            (codes::UNKNOWN_ATTRIBUTE, "t.xml:3:59"),
            (codes::UNKNOWN_ATTRIBUTE, "t.xml:3:90"),
            (codes::UNKNOWN_ATTRIBUTE, "t.xml:3:142"),
            (codes::CONTENT_IGNORED, "t.xml:4:1"),
            (codes::UNKNOWN_ELEMENT, "t.xml:4:29"),
        ])
    );
    for (d, name) in diags
        .iter()
        .zip(["`foo`", "`bar`", "`baz`", "omitted", "<nope>"])
    {
        assert!(d.message().contains(name), "{d}");
    }
}
