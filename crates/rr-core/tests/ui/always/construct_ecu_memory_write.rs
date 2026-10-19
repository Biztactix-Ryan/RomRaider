// US-RR-7-1: an `EcuMemoryWrite` cannot be built with a struct literal outside
// rr-core, even from valid scope values.
use rr_core::capability::{AddressRange, EcuId, EcuMemoryWrite};

fn main() {
    let ecu_id = EcuId::from_bytes(&[0x2F, 0x12, 0x78, 0x52, 0x06]).unwrap();
    let range = AddressRange::new(0, u32::MAX).unwrap();
    let _forged = EcuMemoryWrite { ecu_id, range };
}
