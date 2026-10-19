// US-RR-7-1: an `EcuReset` cannot be built with a struct literal outside
// rr-core, even from valid scope values.
use rr_core::capability::{EcuId, EcuReset, ResetKind};

fn main() {
    let ecu_id = EcuId::from_bytes(&[0x2F, 0x12, 0x78, 0x52, 0x06]).unwrap();
    let _forged = EcuReset {
        ecu_id,
        kind: ResetKind::Ecu,
    };
}
