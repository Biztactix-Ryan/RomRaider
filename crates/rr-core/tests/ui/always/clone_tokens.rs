// US-RR-7-1: no token implements `Clone`, so one confirmation cannot be cloned
// into a second token.
use rr_core::capability::{EcuMemoryWrite, EcuReset, RomWrite};

fn rom(token: &RomWrite) -> RomWrite {
    token.clone()
}

fn memory(token: &EcuMemoryWrite) -> EcuMemoryWrite {
    token.clone()
}

fn reset(token: &EcuReset) -> EcuReset {
    token.clone()
}

fn main() {}
