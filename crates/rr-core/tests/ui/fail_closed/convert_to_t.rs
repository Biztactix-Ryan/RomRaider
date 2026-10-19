// US-RR-17-3: no conversion trait hands out T: no From/Into, no AsRef,
// no Borrow, no IntoIterator over the value.
use std::borrow::Borrow;

use rr_core::diag::{Code, Diagnostic, Loaded, Provenance, Span};

fn main() {
    let error = Diagnostic::error(
        Code::new("DEF0001"),
        "bad",
        Provenance::new("defs/base.xml", Span::line_col(1, 1)),
    );
    let loaded = Loaded::new(vec![1u8, 2], vec![error]);
    let _: &Vec<u8> = loaded.as_ref();
    let _: &Vec<u8> = loaded.borrow();
    for _byte in loaded.clone() {}
    let _: Vec<u8> = loaded.into();
}
