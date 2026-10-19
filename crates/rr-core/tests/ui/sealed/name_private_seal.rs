// US-RR-7-1: the `Sealed` supertrait lives in a private module and cannot be
// named (or implemented) from outside rr-core without the
// `confirmation-authority` feature.
struct AlwaysYes;

impl rr_core::capability::seal::Sealed for AlwaysYes {}

fn main() {}
