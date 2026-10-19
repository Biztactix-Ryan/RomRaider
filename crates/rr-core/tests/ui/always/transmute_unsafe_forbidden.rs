// US-RR-7-1 "transmute-free": a crate under `#![forbid(unsafe_code)]` cannot
// forge a token even by opting into `unsafe`. Forging with `unsafe` is outside
// the token guarantee; it is a matter of the unsafe-code lint policy and review.
#![forbid(unsafe_code)]

use rr_core::capability::RomWrite;

fn main() {
    let _forged: RomWrite = unsafe { std::mem::transmute::<bool, RomWrite>(true) };
}
