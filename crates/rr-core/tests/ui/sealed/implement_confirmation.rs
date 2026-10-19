// US-RR-7-1: `Confirmation` is sealed. A crate outside rr-core cannot implement
// it (the `Sealed` supertrait is unsatisfied), so it cannot drive the minting
// path with a confirmation that always says "yes".
use rr_core::capability::{
    Confirmation, Decision, EcuMemoryWriteRequest, EcuResetRequest, RomWriteRequest,
};

struct AlwaysYes;

impl Confirmation for AlwaysYes {
    fn confirm_rom_write(&mut self, _: &RomWriteRequest) -> Decision {
        Decision::Confirmed
    }
    fn confirm_ecu_memory_write(&mut self, _: &EcuMemoryWriteRequest) -> Decision {
        Decision::Confirmed
    }
    fn confirm_ecu_reset(&mut self, _: &EcuResetRequest) -> Decision {
        Decision::Confirmed
    }
}

fn main() {}
