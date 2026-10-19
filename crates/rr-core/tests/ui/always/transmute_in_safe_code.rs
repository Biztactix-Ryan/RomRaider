// US-RR-7-1 "transmute-free": safe code cannot conjure a token from raw bytes.
// `transmute`, `zeroed` and `assume_init` are all `unsafe fn`s, so calling them
// outside an `unsafe` block is a hard error (E0133), not a lint.
use std::mem::{self, MaybeUninit};

use rr_core::capability::{EcuReset, RomWrite};

fn main() {
    let _from_bool: RomWrite = mem::transmute::<bool, RomWrite>(true);
    let _zeroed: RomWrite = mem::zeroed();
    let _uninit: EcuReset = MaybeUninit::<EcuReset>::uninit().assume_init();
}
