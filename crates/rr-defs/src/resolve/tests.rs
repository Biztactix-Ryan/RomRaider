//! Unit tests of frozen definitions (US-RR-34-6): the typed parts, sharing
//! between the definitions of one set (MF-18), and diagnostics passed
//! through.

use std::sync::Arc;

use rr_core::diag::{Code, Diagnostic, codes};
use rr_core::encoding::{Endian, Storage};

use super::{CellSource, Definition, DefinitionResolver, Scaling, Table};
use crate::romraider::{DefinitionSet, Identification, RamOffset, TableKind};

/// `Definition` and its parts can be shared across threads (MF-18: one
/// resolved set serves every open ROM).
const _: () = {
    const fn send_sync<T: Send + Sync>() {}
    send_sync::<Definition>();
    send_sync::<Table>();
    send_sync::<Scaling>();
};

/// The set of `files` (`(source, xml)`), accepting errors.
fn set_of(files: &[(&str, &str)]) -> DefinitionSet {
    let (set, diags) =
        DefinitionSet::read_all(files.iter().map(|(s, x)| (*s, x.as_bytes()))).accept_errors();
    assert_eq!(codes_of(&diags), []);
    set
}

/// The code of each diagnostic.
fn codes_of(diags: &[Diagnostic]) -> Vec<Code> {
    diags.iter().map(Diagnostic::code).collect()
}

/// Resolves `xmlid` of `set` with `resolver`, accepting errors.
fn resolve(
    resolver: &mut DefinitionResolver<'_>,
    set: &DefinitionSet,
    xmlid: &str,
) -> (Option<Definition>, Vec<Diagnostic>) {
    resolver.resolve(set.find(xmlid).unwrap()).accept_errors()
}

/// Resolves `xmlid`, which must resolve with no diagnostic.
fn clean(resolver: &mut DefinitionResolver<'_>, set: &DefinitionSet, xmlid: &str) -> Definition {
    let (def, diags) = resolve(resolver, set, xmlid);
    assert_eq!(codes_of(&diags), [], "{xmlid}");
    def.unwrap()
}

/// A base and two children of it, in one file, with a scaling base in
/// another:
///
/// * `Rev Limit`: 1D `uint16`, placed by the base, scaled `rpm`;
/// * `Boost Target`: 3D `uint16`, no address (each child places it), an X
///   axis `uint8` scaled `rpm`, a Y axis of static values;
/// * `Mode`: a switch with two states, placed by the base;
/// * `Flags`: a bitwise switch, placed by the base;
/// * `Idle`: never placed.
///
/// `ROM B` also changes `Mode`'s category; `ROM C` makes every table
/// little-endian (`<memmodel endian>`).
const DEFS: &str = r#"<roms>
<rom>
 <romid><xmlid>BASE</xmlid><filesize>512kb</filesize><memmodel>SH7058</memmodel><checksummodule>subarudbw</checksummodule><noramoffset/><make>Subaru</make></romid>
 <table type="1D" name="Rev Limit" category="Engine" storagetype="uint16" storageaddress="100" userlevel="2"><scaling name="RevRpm" units="rpm" expression="x*4" to_byte="x/4" format="0" min="0" max="9000" fineincrement="25" coarseincrement="100"/></table>
 <table type="3D" name="Boost Target" category="Boost" storagetype="uint16" sizex="2" sizey="3" swapxy="true"><scaling units="psi" expression="x*0.01" format="0.00"/><table type="X Axis" name="RPM" storagetype="uint8"><scaling name="RevRpm" units="rpm" expression="x*4" to_byte="x/4" format="0" min="0" max="9000" fineincrement="25" coarseincrement="100"/></table><table type="Static Y Axis" name="Gear" sizey="3"><data>1</data><data>2</data><data>3</data></table></table>
 <table type="Switch" name="Mode" category="Misc" storageaddress="300" sizey="1"><state name="On" data="01"/><state name="Off" data="00"/></table>
 <table type="BitwiseSwitch" name="Flags" storagetype="uint8" storageaddress="400" locked="true"><bit position="0" name="A"/><bit position="3" name="D"/></table>
 <table type="1D" name="Idle" storagetype="uint8"/>
</rom>
<rom base="BASE">
 <romid><xmlid>ROM A</xmlid><internalidaddress>2000</internalidaddress><internalidstring>A2WC522N</internalidstring><ecuid>1234567890</ecuid></romid>
 <table name="Boost Target" storageaddress="1000"><table type="X Axis" storageaddress="1100"/></table>
</rom>
<rom base="BASE">
 <romid><xmlid>ROM B</xmlid><internalidaddress>2000</internalidaddress><internalidstring>0xA1B2C3</internalidstring><filesize>1mb</filesize><memmodel>SH7055</memmodel><obsolete>true</obsolete></romid>
 <table name="Boost Target" storageaddress="5000"><table type="X Axis" storageaddress="5100"/></table>
 <table name="Mode" category="Changed"/>
</rom>
<rom base="BASE">
 <romid><xmlid>ROM C</xmlid><memmodel endian="little">SH7055</memmodel></romid>
 <table name="Boost Target" storageaddress="6000"><table type="X Axis" storageaddress="6100"/></table>
</rom>
</roms>"#;

// ---- the typed definition ---------------------------------------------------

#[test]
fn romid_is_the_merged_romid_typed_with_the_rom_s_own_identification() {
    let set = set_of(&[("defs.xml", DEFS)]);
    let mut resolver = DefinitionResolver::new(&set);
    let a = clean(&mut resolver, &set, "ROM A");
    assert_eq!(a.xmlid(), "ROM A");
    let id = a.romid();
    assert_eq!(id.xmlid(), "ROM A");
    // Inherited from the base.
    assert_eq!(id.file_size(), Some(512 * 1024));
    assert_eq!(id.mem_model(), Some("SH7058"));
    assert_eq!(id.mem_model_endian(), None);
    assert_eq!(id.ram_offset(), Some(RamOffset::Disabled));
    assert_eq!(id.make(), Some("Subaru"));
    // Its own.
    assert_eq!(id.ecu_id(), Some("1234567890"));
    assert!(!id.obsolete());
    assert_eq!(id.model(), None);
    // The (address, bytes) pattern the ROM-ID index matches.
    let Some(Identification::Bytes(pattern)) = id.identification() else {
        panic!("{:?}", id.identification())
    };
    assert_eq!(pattern.offset(), 0x2000);
    assert_eq!(pattern.len(), 8);
    let mut image = vec![0; 0x3000];
    image[0x2000..0x2008].copy_from_slice(b"A2WC522N");
    assert!(pattern.matches(&image));
    // The checksum spec, by name only.
    assert_eq!(a.checksum().map(|c| c.name()), Some("subarudbw"));

    let b = clean(&mut resolver, &set, "ROM B");
    let id = b.romid();
    assert_eq!(id.file_size(), Some(1024 * 1024));
    assert_eq!(id.mem_model(), Some("SH7055"));
    assert_eq!(id.mem_model_endian(), None);
    assert!(id.obsolete());
    let Some(Identification::Bytes(pattern)) = id.identification() else {
        panic!("{:?}", id.identification())
    };
    assert_eq!(pattern.value(), [0xA1, 0xB2, 0xC3]);

    // A base-only definition resolves, and identifies no image.
    let base = clean(&mut resolver, &set, "BASE");
    assert_eq!(base.romid().identification(), None);
    assert_eq!(base.romid().ecu_id(), None);
}

#[test]
fn tables_are_typed_with_legacy_defaults_applied() {
    let set = set_of(&[("defs.xml", DEFS)]);
    let mut resolver = DefinitionResolver::new(&set);
    let a = clean(&mut resolver, &set, "ROM A");
    let names: Vec<&str> = a.tables().iter().map(|t| t.name()).collect();
    assert_eq!(names, ["Rev Limit", "Boost Target", "Mode", "Flags"]);
    assert_eq!(a.unplaced(), ["Idle"]);
    assert!(a.table("idle").is_none());

    let rev = a.table("REV LIMIT").unwrap();
    assert_eq!(rev.key(), "rev limit");
    assert_eq!(rev.kind(), TableKind::OneD);
    assert_eq!(rev.category(), Some("Engine"));
    assert_eq!(rev.user_level(), 2);
    assert!(!rev.locked());
    assert!(rev.axes().is_empty());
    assert_eq!(rev.cells().address(), Some(0x100));
    assert!(!rev.cells().flip());
    let CellSource::Image {
        before_ram, format, ..
    } = rev.cells().source()
    else {
        panic!("{:?}", rev.cells().source())
    };
    assert!(!before_ram);
    assert_eq!(format.encoding.storage(), Storage::U16);
    assert_eq!(format.encoding.endian(), Endian::Big);
    assert_eq!(format.conversion.unit(), Some("rpm"));
    let scaling = rev.cells().scaling().unwrap();
    assert_eq!(scaling.name(), Some("RevRpm"));
    assert_eq!(scaling.category(), "Default");
    assert_eq!(scaling.units(), Some("rpm"));
    assert_eq!(scaling.to_real().map(|f| f.text()), Some("x*4"));
    assert_eq!(scaling.to_raw().map(|f| f.text()), Some("x/4"));
    assert_eq!(scaling.format().map(|f| f.pattern()), Some("0"));
    assert_eq!(scaling.min(), Some(0.0));
    assert_eq!(scaling.max(), Some(9000.0));
    assert_eq!(scaling.fine_increment(), Some(25.0));
    assert_eq!(scaling.coarse_increment(), Some(100.0));
    assert!(
        rev.at().to_string().starts_with("defs.xml:4:"),
        "{}",
        rev.at()
    );

    let boost = a.table("Boost Target").unwrap();
    assert_eq!(boost.kind(), TableKind::ThreeD);
    assert_eq!((boost.size_x(), boost.size_y()), (Some(2), Some(3)));
    assert!(boost.swap_xy());
    assert!(!boost.flip_x() && !boost.flip_y());
    assert_eq!(boost.skip_cells(), 0);
    assert_eq!(boost.cells().address(), Some(0x1000));
    let [x, y] = boost.axes() else {
        panic!("{:?}", boost.axes())
    };
    assert_eq!(x.name(), Some("RPM"));
    assert_eq!(x.cells().address(), Some(0x1100));
    assert_eq!(x.cells().format().unwrap().encoding.storage(), Storage::U8);
    assert_eq!(y.name(), Some("Gear"));
    assert_eq!(y.cells().size(), Some(3));
    assert_eq!(
        y.cells().source(),
        &CellSource::Static(vec!["1".to_owned(), "2".to_owned(), "3".to_owned()])
    );
    assert!(y.cells().format().is_none() && y.cells().scaling().is_none());

    let mode = a.table("mode").unwrap();
    assert_eq!(mode.kind(), TableKind::Switch);
    let states: Vec<(&str, &[u32])> = mode.states().iter().map(|s| (s.name(), s.data())).collect();
    assert_eq!(states, [("On", &[1][..]), ("Off", &[0][..])]);
    // A switch's storage defaults to uint8, as in legacy; no scaling is raw.
    let format = mode.cells().format().unwrap();
    assert_eq!(format.encoding.storage(), Storage::U8);
    assert!(mode.cells().scaling().is_none());

    let flags = a.table("flags").unwrap();
    assert!(flags.locked());
    let bits: Vec<(&str, u8)> = flags
        .bits()
        .iter()
        .map(|b| (b.name.as_str(), b.position))
        .collect();
    assert_eq!(bits, [("A", 0), ("D", 3)]);
}

#[test]
fn a_mem_model_endian_is_applied_to_every_table_s_format() {
    let set = set_of(&[("defs.xml", DEFS)]);
    let mut resolver = DefinitionResolver::new(&set);
    let c = clean(&mut resolver, &set, "ROM C");
    assert_eq!(c.romid().mem_model_endian(), Some(Endian::Little));
    let rev = c.table("Rev Limit").unwrap();
    assert_eq!(
        rev.cells().format().unwrap().encoding.endian(),
        Endian::Little
    );
}

#[test]
fn scalings_lists_each_distinct_scaling_once_in_first_use_order() {
    let set = set_of(&[("defs.xml", DEFS)]);
    let mut resolver = DefinitionResolver::new(&set);
    let a = clean(&mut resolver, &set, "ROM A");
    let units: Vec<Option<&str>> = a.scalings().iter().map(|s| s.units()).collect();
    // `Rev Limit` and the boost X axis show one scaling: listed once.
    assert_eq!(units, [Some("rpm"), Some("psi")]);
    let rev = a.table("Rev Limit").unwrap().cells().scaling().unwrap();
    let axis = a.table("Boost Target").unwrap().axes()[0]
        .cells()
        .scaling()
        .unwrap();
    assert!(Arc::ptr_eq(rev, axis));
}

#[test]
fn no_checksummodule_is_no_checksum_spec() {
    let set = set_of(&[(
        "d.xml",
        "<roms><rom><romid><xmlid>X</xmlid></romid></rom></roms>",
    )]);
    let mut resolver = DefinitionResolver::new(&set);
    let x = clean(&mut resolver, &set, "X");
    assert!(x.checksum().is_none());
    assert!(x.tables().is_empty() && x.scalings().is_empty());
}

// ---- shared, not copied (MF-18) ---------------------------------------------

#[test]
fn mf18_two_children_of_one_base_share_every_part_neither_changes() {
    let set = set_of(&[("defs.xml", DEFS)]);
    let mut resolver = DefinitionResolver::new(&set);
    let a = clean(&mut resolver, &set, "ROM A");
    let b = clean(&mut resolver, &set, "ROM B");
    let c = clean(&mut resolver, &set, "ROM A");

    // Inherited unchanged by both: one table.
    let flags = a.table("Flags").unwrap();
    assert!(Arc::ptr_eq(flags, b.table("Flags").unwrap()));
    // Placed by each child: two tables, one scaling.
    let (a_boost, b_boost) = (
        a.table("Boost Target").unwrap(),
        b.table("Boost Target").unwrap(),
    );
    assert!(!Arc::ptr_eq(a_boost, b_boost));
    assert_ne!(a_boost.cells().address(), b_boost.cells().address());
    assert!(Arc::ptr_eq(
        a_boost.cells().scaling().unwrap(),
        b_boost.cells().scaling().unwrap()
    ));
    // Changed by B: its own; the same base table in A still shared.
    assert!(!Arc::ptr_eq(
        a.table("Mode").unwrap(),
        b.table("Mode").unwrap()
    ));
    assert_eq!(b.table("Mode").unwrap().category(), Some("Changed"));
    // Resolving A again hands out the very same parts.
    for (t, again) in a.tables().iter().zip(c.tables()) {
        assert!(Arc::ptr_eq(t, again), "{}", t.name());
    }
}

#[test]
fn a_child_changing_the_byte_order_shares_the_scaling_but_not_the_format() {
    let set = set_of(&[("defs.xml", DEFS)]);
    let mut resolver = DefinitionResolver::new(&set);
    let a = clean(&mut resolver, &set, "ROM A");
    let c = clean(&mut resolver, &set, "ROM C");
    let (a_rev, c_rev) = (a.table("Rev Limit").unwrap(), c.table("Rev Limit").unwrap());
    assert!(!Arc::ptr_eq(a_rev, c_rev));
    assert!(Arc::ptr_eq(
        a_rev.cells().scaling().unwrap(),
        c_rev.cells().scaling().unwrap()
    ));
    assert!(!Arc::ptr_eq(
        a_rev.cells().format().unwrap(),
        c_rev.cells().format().unwrap()
    ));
}

#[test]
fn equal_formats_are_one_allocation_across_definitions() {
    let set = set_of(&[("defs.xml", DEFS)]);
    let mut resolver = DefinitionResolver::new(&set);
    let a = clean(&mut resolver, &set, "ROM A");
    let base = clean(&mut resolver, &set, "BASE");
    let format =
        |d: &Definition| Arc::clone(d.table("Rev Limit").unwrap().cells().format().unwrap());
    assert!(Arc::ptr_eq(&format(&a), &format(&base)));
}

#[test]
fn sharing_is_invisible_a_definition_is_the_same_value_from_any_resolver() {
    let set = set_of(&[("defs.xml", DEFS)]);
    let mut warm = DefinitionResolver::new(&set);
    clean(&mut warm, &set, "ROM A");
    clean(&mut warm, &set, "BASE");
    let shared = clean(&mut warm, &set, "ROM B");
    let alone = clean(&mut DefinitionResolver::new(&set), &set, "ROM B");
    assert_eq!(shared, alone);
}

#[test]
fn copies_of_a_base_in_two_files_do_not_share_tables() {
    // Each file's own copy of the base serves that file: a table is shared
    // only with one from the same <table> element.
    let child = |id: &str| {
        format!(
            r#"<roms>
<rom><romid><xmlid>BASE</xmlid></romid>
<table type="1D" name="Rev Limit" storagetype="uint16" storageaddress="100"/>
</rom>
<rom base="BASE"><romid><xmlid>{id}</xmlid></romid></rom>
</roms>"#
        )
    };
    let (one, two) = (child("ONE"), child("TWO"));
    let (set, diags) =
        DefinitionSet::read_all([("one.xml", one.as_bytes()), ("two.xml", two.as_bytes())])
            .accept_errors();
    assert_eq!(codes_of(&diags), [codes::DUPLICATE_XMLID]);
    let mut resolver = DefinitionResolver::new(&set);
    let one = clean(&mut resolver, &set, "ONE");
    let two = clean(&mut resolver, &set, "TWO");
    let (a, b) = (
        one.table("Rev Limit").unwrap(),
        two.table("Rev Limit").unwrap(),
    );
    assert!(!Arc::ptr_eq(a, b));
    assert_eq!(a.at().source(), "one.xml");
    assert_eq!(b.at().source(), "two.xml");
}

// ---- diagnostics passed through ---------------------------------------------

#[test]
fn a_chain_that_cannot_be_built_is_no_definition_with_its_error() {
    let set = set_of(&[(
        "d.xml",
        r#"<roms><rom base="MISSING"><romid><xmlid>X</xmlid></romid></rom></roms>"#,
    )]);
    let mut resolver = DefinitionResolver::new(&set);
    let loaded = resolver.resolve(set.find("X").unwrap());
    assert!(loaded.has_errors());
    assert_eq!(codes_of(loaded.diagnostics()), [codes::UNKNOWN_BASE]);
    let (def, _) = loaded.accept_errors();
    assert!(def.is_none());
}

#[test]
fn a_table_with_an_error_is_left_out_and_withholds_the_definition() {
    let set = set_of(&[(
        "d.xml",
        r#"<roms><rom><romid><xmlid>X</xmlid></romid>
<table type="1D" name="No Storage" storageaddress="10"/>
<table type="1D" name="Fine" storagetype="uint8" storageaddress="20"/>
</rom></roms>"#,
    )]);
    let mut resolver = DefinitionResolver::new(&set);
    let loaded = resolver.resolve(set.find("X").unwrap());
    assert_eq!(loaded.value(), None);
    assert_eq!(
        codes_of(loaded.diagnostics()),
        [codes::MISSING_STORAGE_TYPE]
    );
    let (def, _) = loaded.accept_errors();
    let names: Vec<String> = def
        .unwrap()
        .tables()
        .iter()
        .map(|t| t.name().to_owned())
        .collect();
    assert_eq!(names, ["Fine"]);
}

#[test]
fn a_preferred_scaling_category_is_the_one_shown() {
    let set = set_of(&[(
        "d.xml",
        r#"<roms><rom><romid><xmlid>X</xmlid></romid>
<table type="1D" name="T" storagetype="uint8" storageaddress="20"><scaling units="psi" expression="x"/><scaling category="Metric" units="kPa" expression="x*6.9"/></table>
</rom></roms>"#,
    )]);
    let mut resolver = DefinitionResolver::new(&set).with_preferred_scaling("Metric");
    let x = clean(&mut resolver, &set, "X");
    let scaling = x.table("T").unwrap().cells().scaling().unwrap();
    assert_eq!(
        (scaling.category(), scaling.units()),
        ("Metric", Some("kPa"))
    );
}
