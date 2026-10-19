// US-RR-7-1: no token implements `Copy`, so a by-value token is consumed by
// its first use and cannot be duplicated by moving it twice.
use rr_core::capability::{EcuMemoryWrite, EcuReset, RomWrite};

fn consume<T>(_: T) {}

fn rom(token: RomWrite) {
    consume(token);
    consume(token);
}

fn memory(token: EcuMemoryWrite) {
    consume(token);
    consume(token);
}

fn reset(token: EcuReset) {
    consume(token);
    consume(token);
}

fn main() {}
