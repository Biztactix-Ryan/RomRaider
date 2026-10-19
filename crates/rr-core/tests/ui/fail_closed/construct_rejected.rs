// US-RR-17-3: Rejected<T> cannot be built outside rr-core, so code cannot
// fabricate a "rejection" that carries a value without its diagnostics.
use rr_core::diag::Rejected;

fn main() {
    let _ = Rejected {
        value: 7u32,
        diagnostics: Vec::new(),
    };
}
