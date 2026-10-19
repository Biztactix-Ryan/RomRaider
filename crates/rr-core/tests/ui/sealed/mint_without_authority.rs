// US-RR-7-1: without the `confirmation-authority` feature rr-core exposes no
// API that produces a token: the `authority` module (and its `mint_*`
// functions) does not exist, and the internal `mint` module is private.
use rr_core::capability::{ChecksumStatus, RomWriteRequest};

fn main() {
    let request = RomWriteRequest::new(ChecksumStatus::Verified);
    let _a = rr_core::capability::authority::mint_rom_write;
    let _b = rr_core::capability::mint::rom_write;
    let _ = request;
}
