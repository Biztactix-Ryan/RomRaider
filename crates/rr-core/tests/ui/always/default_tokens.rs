// US-RR-7-1: no token implements `Default`.
use rr_core::capability::{EcuMemoryWrite, EcuReset, RomWrite};

fn main() {
    let _rom: RomWrite = Default::default();
    let _memory: EcuMemoryWrite = Default::default();
    let _reset: EcuReset = Default::default();
}
