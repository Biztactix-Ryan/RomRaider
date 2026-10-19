// US-RR-7-1: a `RomWrite` cannot be built with a struct literal outside rr-core
// (its field is private and there is no public constructor).
use rr_core::capability::RomWrite;

fn main() {
    let _forged = RomWrite {
        checksum_override: true,
    };
}
