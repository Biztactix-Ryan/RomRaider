// US-RR-7-1: no token implements serde `Deserialize`, so none can be read back
// from a file, config or IPC message.
use rr_core::capability::{EcuMemoryWrite, EcuReset, RomWrite};

fn deserializable<T: serde::de::DeserializeOwned>() {}

fn main() {
    deserializable::<RomWrite>();
    deserializable::<EcuMemoryWrite>();
    deserializable::<EcuReset>();
}
