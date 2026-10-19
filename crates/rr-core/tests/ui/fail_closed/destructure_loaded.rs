// US-RR-17-3: the value of a Loaded<T> is not reachable through its fields,
// so a load with Error diagnostics cannot be unpacked around the gate.
use rr_core::diag::{Code, Diagnostic, Loaded, Provenance, Span};

fn main() {
    let error = Diagnostic::error(
        Code::new("DEF0001"),
        "bad",
        Provenance::new("defs/base.xml", Span::line_col(1, 1)),
    );
    let loaded = Loaded::new(7u32, vec![error]);
    let Loaded { value, .. } = loaded;
    let _: u32 = value;
}
