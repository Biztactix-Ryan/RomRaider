// US-RR-17-3: there is no public `value` field on Loaded<T> or Rejected<T>.
use rr_core::diag::{Code, Diagnostic, Loaded, Provenance, Span};

fn main() {
    let error = Diagnostic::error(
        Code::new("DEF0001"),
        "bad",
        Provenance::new("defs/base.xml", Span::line_col(1, 1)),
    );
    let loaded = Loaded::new(7u32, vec![error]);
    let rejected = loaded.clone().into_result().unwrap_err();
    let _: u32 = loaded.value;
    let _: u32 = rejected.value;
}
