//! Inert write markers for the T6 write-gating check (SECURITY.md
//! "Enforcement", D-013, story US-RR-7).
//!
//! Every public function in `rr-rom` and `rr-comms` that writes — overwrites
//! a ROM file, writes ECU memory, resets an ECU — carries [`macro@rr_write`]
//! and must take a capability token from `rr_core::capability` (`RomWrite`,
//! `EcuMemoryWrite` or `EcuReset`) as a parameter, by value, `&` or `&mut`:
//!
//! ```ignore
//! use rr_write_marker::rr_write;
//!
//! #[rr_write]
//! pub fn save(image: &RomImage, path: &Path, token: RomWrite) -> Result<(), SaveError> { ... }
//! ```
//!
//! A public function that *looks* like a write to the heuristic net in
//! `cargo xtask check-writes` (a write-ish verb in its name, or a `&mut`
//! link / image / journal / file parameter) but is not one carries
//! [`macro@rr_not_write`] with a non-empty justification instead:
//!
//! ```ignore
//! #[rr_not_write("reads only: sends a read-memory request over the link")]
//! pub fn read_memory(link: &mut dyn Link, addr: u32) -> Result<Vec<u8>, Error> { ... }
//! ```
//!
//! # Why a proc-macro crate
//!
//! The check is a source scan (`xtask/src/writes.rs`, run as
//! `cargo xtask check-writes` and by `xtask/tests/t6_write_gating.rs`), so the
//! marker only needs to be *visible in the source* and *accepted by the
//! compiler*. Stable Rust offers no inert custom attribute (`#[rr::write]`
//! tool attributes are unstable), and the alternatives are weaker:
//!
//! * a naming or doc-comment convention is not compiler-checked — a typo
//!   silently drops the marker, and nothing stops it being put on a struct;
//! * a marker *trait* cannot be attached to a free function or method at all.
//!
//! A pass-through attribute macro is compiler-checked (a misspelt
//! `#[rr_wirte]` is a compile error, not a silently unmarked function), is
//! greppable, and costs one dependency-free crate. Both macros return the
//! item unchanged; they only reject misuse (arguments to `rr_write`, a missing
//! or empty justification on `rr_not_write`, or use on something that is not
//! a function) at compile time. The real token check lives in xtask, where it
//! can also see *unmarked* functions.

use proc_macro::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};

/// Marks a write-performing function. The function must take a capability
/// token (`RomWrite`, `EcuMemoryWrite` or `EcuReset`); `cargo xtask
/// check-writes` fails if it does not. Expands to the function unchanged.
///
/// Takes no arguments.
#[proc_macro_attribute]
pub fn rr_write(attr: TokenStream, item: TokenStream) -> TokenStream {
    if let Some(tt) = attr.into_iter().next() {
        return compile_error(
            tt.span(),
            "#[rr_write] takes no arguments; the capability token goes in the function's parameters",
        );
    }
    if !is_fn(&item) {
        return compile_error(
            Span::call_site(),
            "#[rr_write] marks write-performing functions and methods only",
        );
    }
    item
}

/// Declares that a public function the `check-writes` heuristic would suspect
/// of writing does not write. Requires one non-empty string literal saying
/// why, e.g. `#[rr_not_write("reads only")]`. Expands to the function
/// unchanged.
#[proc_macro_attribute]
pub fn rr_not_write(attr: TokenStream, item: TokenStream) -> TokenStream {
    let tts: Vec<TokenTree> = attr.into_iter().collect();
    let ok = match tts.as_slice() {
        [TokenTree::Literal(lit)] => is_nonempty_str(&lit.to_string()),
        _ => false,
    };
    if !ok {
        let span = tts.first().map_or_else(Span::call_site, TokenTree::span);
        return compile_error(
            span,
            "#[rr_not_write] needs one non-empty string literal justification, e.g. #[rr_not_write(\"reads only\")]",
        );
    }
    if !is_fn(&item) {
        return compile_error(
            Span::call_site(),
            "#[rr_not_write] applies to functions and methods only",
        );
    }
    item
}

/// Whether a literal's source text is a plain string literal with
/// non-whitespace content (`"..."`).
fn is_nonempty_str(src: &str) -> bool {
    src.len() >= 2
        && src.starts_with('"')
        && src.ends_with('"')
        && !src[1..src.len() - 1].trim().is_empty()
}

/// Whether the item is a function: a top-level `fn` keyword appears before
/// any brace-delimited body.
fn is_fn(item: &TokenStream) -> bool {
    for tt in item.clone() {
        match tt {
            TokenTree::Ident(i) if i.to_string() == "fn" => return true,
            TokenTree::Group(g) if g.delimiter() == Delimiter::Brace => return false,
            _ => {}
        }
    }
    false
}

/// `compile_error!("msg");` at `span`, built without parsing (nothing to unwrap).
fn compile_error(span: Span, msg: &str) -> TokenStream {
    let mut lit = Literal::string(msg);
    lit.set_span(span);
    let mut args = Group::new(Delimiter::Parenthesis, TokenTree::Literal(lit).into());
    args.set_span(span);
    let mut bang = Punct::new('!', Spacing::Alone);
    bang.set_span(span);
    let mut semi = Punct::new(';', Spacing::Alone);
    semi.set_span(span);
    [
        TokenTree::Ident(Ident::new("compile_error", span)),
        TokenTree::Punct(bang),
        TokenTree::Group(args),
        TokenTree::Punct(semi),
    ]
    .into_iter()
    .collect()
}
