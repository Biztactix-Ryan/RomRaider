// US-RR-17-3: a Rejected<T> keeps the value private too; the only way out
// is Rejected::accept_errors.
use rr_core::diag::{Code, Diagnostic, Loaded, Provenance, Span};

fn main() {
    let error = Diagnostic::error(
        Code::new("DEF0001"),
        "bad",
        Provenance::new("defs/base.xml", Span::line_col(1, 1)),
    );
    let Err(rejected) = Loaded::new(7u32, vec![error]).into_result() else {
        return;
    };
    let rr_core::diag::Rejected { value, .. } = rejected;
    let _: u32 = value;
}
