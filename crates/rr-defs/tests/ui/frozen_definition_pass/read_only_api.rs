// US-RR-34-5 positive control: reading a resolved `Definition` and its
// ROM-ID index the supported way compiles and runs, from many threads at
// once, so each compile-fail case fails for the reason its `.stderr`
// records, not because the harness cannot build anything. The public
// resolved types meet the bound of `interior_mutability_fails_the_bound.rs`.
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::sync::Arc;
use std::thread;

use rr_defs::resolve::{
    Axis, Candidate, CellSource, Cells, ChecksumSpecRef, Definition, DefinitionResolver, RomId,
    RomIdIndex, RomMatch, Scaling, SwitchState, Table,
};
use rr_defs::romraider::{CellFormat, DefinitionSet};

const fn frozen<T: Send + Sync + UnwindSafe + RefUnwindSafe>() {}

const XML: &[u8] = br#"<roms>
  <rom>
    <romid><xmlid>BASE</xmlid><filesize>192kb</filesize></romid>
    <table type="1D" name="Rev Limit" storagetype="uint16" storageaddress="100">
      <scaling units="rpm" expression="x*4"/>
    </table>
  </rom>
  <rom base="BASE">
    <romid><xmlid>ROM A</xmlid><internalidaddress>4</internalidaddress>
      <internalidstring>ROMA</internalidstring></romid>
  </rom>
</roms>"#;

fn main() {
    frozen::<Definition>();
    frozen::<RomId>();
    frozen::<ChecksumSpecRef>();
    frozen::<Table>();
    frozen::<Axis>();
    frozen::<Cells>();
    frozen::<CellSource>();
    frozen::<CellFormat>();
    frozen::<Scaling>();
    frozen::<SwitchState>();
    frozen::<RomIdIndex>();
    frozen::<Candidate>();
    frozen::<RomMatch<'static>>();

    let (set, _) = DefinitionSet::read_all([("defs.xml", XML)]).into_result().unwrap();
    let index = RomIdIndex::new(&set);
    let RomMatch::Unique(candidate) = index.lookup(b"....ROMA....") else {
        panic!("no unique match");
    };
    let rom = candidate.rom();
    let def = DefinitionResolver::new(&set).resolve(rom).into_result().unwrap().0.unwrap();
    let def = Arc::new(def);

    // Every thread reads the same definition; nothing to lock.
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let def = Arc::clone(&def);
            thread::spawn(move || {
                let limit = def.table("rev limit").unwrap();
                (def.romid().file_size(), limit.cells().address())
            })
        })
        .collect();
    for reader in readers {
        assert_eq!(reader.join().unwrap(), (Some(192 * 1024), Some(0x100)));
    }

    // A caller's own copy can be replaced wholesale; the shared one, and
    // the tables both share, are unchanged.
    let mut copy = Definition::clone(&def);
    assert!(Arc::ptr_eq(&copy.tables()[0], &def.tables()[0]));
    let other = DefinitionResolver::new(&set)
        .resolve(set.find("base").unwrap())
        .into_result()
        .unwrap()
        .0
        .unwrap();
    copy = other;
    assert_eq!(copy.xmlid(), "BASE");
    assert_eq!(def.xmlid(), "ROM A");
    assert_eq!(def.table("Rev Limit").unwrap().cells().address(), Some(0x100));
}
