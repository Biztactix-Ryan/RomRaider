// US-RR-7-1 (with the module docs): no token implements serde `Serialize`
// either, so a token cannot be written out to be replayed later.
use rr_core::capability::{EcuMemoryWrite, EcuReset, RomWrite};

fn serializable<T: serde::Serialize>() {}

fn main() {
    serializable::<RomWrite>();
    serializable::<EcuMemoryWrite>();
    serializable::<EcuReset>();
}
