// US-RR-34-5: whatever a resolved `Definition` or its ROM-ID index lends is
// lent `&`, and its shared parts sit behind `Arc`s that hand out no `&mut`.
// So nothing in one can be written, even by a caller holding it `&mut`.
use std::sync::Arc;

use rr_defs::resolve::{CellSource, Definition, RomIdIndex};

fn main() {}

fn mutate(def: &mut Definition, index: &mut RomIdIndex) {
    let romid = def.romid().clone();
    *def.romid() = romid;
    let table = Arc::clone(&def.tables()[0]);
    def.tables()[0] = Arc::clone(&table);
    // A table shared between definitions is never lent `&mut`.
    let _ = Arc::get_mut(&mut def.tables()[0]);
    // Nor are its cells, written through the `Arc`.
    if let Some(format) = table.cells().format() {
        format.encoding = format.encoding.clone();
    }
    if let CellSource::Image { address, .. } = table.cells().source() {
        *address = None;
    }
    let candidate = index.candidates()[0].clone();
    index.candidates()[0] = candidate;
}
