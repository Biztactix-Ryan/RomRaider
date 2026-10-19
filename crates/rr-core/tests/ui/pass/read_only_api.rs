// US-RR-7-1 positive control: the harness builds ordinary code against rr-core,
// so every compile-fail case fails for its stated reason, not a broken setup.
// Holding and inspecting a token someone else minted is allowed.
#![forbid(unsafe_code)]

use rr_core::capability::{
    AddressRange, ChecksumStatus, EcuId, EcuMemoryWrite, EcuReset, ResetKind, RomWrite,
    ScopeError,
};

fn write_once(token: RomWrite) -> Result<bool, ScopeError> {
    token.check_save(ChecksumStatus::Verified)?;
    Ok(token.checksum_override())
}

fn write_memory(token: &EcuMemoryWrite, ecu: &EcuId) -> Result<AddressRange, ScopeError> {
    token.check_write(ecu, token.range().start(), 1)
}

fn reset(token: EcuReset) -> Result<(), ScopeError> {
    token.check_reset(&token.ecu_id().clone(), ResetKind::Ecu)
}

fn main() {
    let _ = (write_once, write_memory, reset);
}
