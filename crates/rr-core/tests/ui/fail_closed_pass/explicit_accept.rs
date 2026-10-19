// US-RR-17-3 positive control: the sanctioned paths compile, so every
// fail_closed case is rejected for the reason its .stderr records rather
// than a broken harness. Getting T out of an errored load needs the
// explicit `accept_errors` call.
use rr_core::diag::{Code, Diagnostic, Loaded, Provenance, Span};

fn main() {
    let error = Diagnostic::error(
        Code::new("DEF0001"),
        "bad",
        Provenance::new("defs/base.xml", Span::line_col(1, 1)),
    );
    let loaded = Loaded::new(7u32, vec![error]);
    assert_eq!(loaded.value(), None);
    let (value, diagnostics) = loaded.clone().accept_errors();
    assert_eq!((value, diagnostics.len()), (7, 1));
    match loaded.into_result() {
        Ok(_) => unreachable!(),
        Err(rejected) => {
            let (value, _) = rejected.accept_errors();
            assert_eq!(value, 7);
        }
    }
}
