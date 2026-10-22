// US-RR-34-5: a resolved `Definition`, its parts and its ROM-ID index are
// read through `&self` getters only, even by a caller that owns them: no
// field is public, no getter has a `&mut` twin, there is no `IndexMut` (nor
// `Index`), and no getter lends a `&mut`.
use std::sync::Arc;

use rr_defs::resolve::{Candidate, Definition, RomId, RomIdIndex, Scaling, Table};

fn main() {}

fn owned(mut def: Definition, mut index: RomIdIndex) {
    // Every field is private.
    def.tables.clear();
    def.romid.file_size = Some(0);
    index.candidates.clear();
    // No `&mut` getters.
    let _ = def.romid_mut();
    let _ = def.tables_mut();
    let _ = def.table_mut("rev limit");
    let _ = index.candidates_mut();
    // No `IndexMut` (nor `Index`) by table name.
    def["rev limit"] = Arc::clone(&def.tables()[0]);
    // The getters lend shared references only.
    let _: &mut RomId = def.romid();
    let _: &mut [Arc<Table>] = def.tables();
}

fn parts(table: &Table, scaling: &Scaling, candidate: &Candidate) {
    let _ = table.cells_mut();
    let _ = table.axes_mut();
    let _ = scaling.min;
    let _ = candidate.xmlid;
}
