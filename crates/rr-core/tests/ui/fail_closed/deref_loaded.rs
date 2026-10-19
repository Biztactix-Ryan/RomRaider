// US-RR-17-3: Loaded<T> does not Deref to T, so `*loaded` / auto-deref
// method calls cannot reach an errored value.
use rr_core::diag::{Code, Diagnostic, Loaded, Provenance, Span};

fn main() {
    let error = Diagnostic::error(
        Code::new("DEF0001"),
        "bad",
        Provenance::new("defs/base.xml", Span::line_col(1, 1)),
    );
    let loaded = Loaded::new(String::from("partial"), vec![error]);
    let _: &String = &*loaded;
    let _: usize = loaded.len();
}
