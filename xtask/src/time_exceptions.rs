//! Time-ban exception check: only RealClock may opt out of the std time ban.
//!
//! Proves US-RR-18-4 ("RealClock is the only module allowed to call std time
//! APIs, enforced by the safety lints story"), together with
//! `cargo xtask lint-selftest`, which proves the ban itself fires.
//! `clippy::disallowed_methods` (clippy.toml, denied in `[workspace.lints]`)
//! rejects every call to `std::thread::sleep`, `Instant::now`,
//! `Instant::elapsed`, `SystemTime::now` and `SystemTime::elapsed`; the only
//! way past it is a lint attribute in the source. This check finds every such
//! attribute (parsed with `syn`, including ones inside `cfg_attr` and inside
//! macro bodies) in every `.rs` file under `crates/` and `xtask/`, and allows:
//!
//! * in **production code**, only `#[expect(clippy::disallowed_methods,
//!   reason = "...")]` inside the private `std_time` module of
//!   [`SANCTIONED_FILE`] (RealClock);
//! * in **test code** (a crate's `tests/` or `benches/` directory, or an item
//!   under `#[cfg(test)]` / `#[test]`), an outer `#[expect(...)]` with a
//!   non-empty `reason`. Such a test measures real time to prove that
//!   `VirtualClock` never blocks on it; it does not ship, and `#[expect]`
//!   fails the build once the call is gone.
//!
//! Everywhere, the following are violations: `#[allow]` or `#[warn]` of the
//! lint, an inner (module- or crate-wide) `#![...]` attribute naming it, an
//! `#[expect]` without a reason, and any `allow` / `expect` / `warn` of the
//! groups that contain it (`clippy::all`, `clippy::style`).
//!
//! CI runs this through `xtask/tests/time_exceptions.rs` (`test` job).

use std::fmt;
use std::fs;
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::visit::{self, Visit};

use crate::globals::rust_files;

/// The file holding RealClock's private `std_time` module.
pub const SANCTIONED_FILE: &str = "crates/rr-core/src/clock.rs";

/// The module (inside [`SANCTIONED_FILE`]) whose items may carry the exception.
pub const SANCTIONED_MODULE: &str = "std_time";

/// The lint that carries the time ban.
pub const LINT: &str = "disallowed_methods";

/// Lint groups that include [`LINT`]: suppressing them suppresses it.
pub const GROUPS: &[&str] = &["all", "style"];

/// Where an exception was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Context {
    /// Inside RealClock's `std_time` module, outside test code.
    RealClock,
    /// Test code: `tests/` / `benches/`, or under `#[cfg(test)]` / `#[test]`.
    Test,
    /// Anything else (shipping code).
    Production,
}

/// One lint attribute that weakens the time ban.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Exception {
    /// Path relative to the workspace root, `/`-separated.
    pub path: String,
    /// 1-based line of the attribute.
    pub line: usize,
    /// `allow`, `expect` or `warn`.
    pub level: String,
    /// The lint or group named, e.g. `clippy::disallowed_methods`.
    pub lint: String,
    /// A `#![...]` attribute (applies to a whole module or crate).
    pub inner: bool,
    /// Carries a non-empty `reason = "..."`.
    pub reasoned: bool,
    /// Where it is.
    pub context: Context,
}

impl Exception {
    /// Why this exception is not permitted, or `None` if it is.
    #[must_use]
    pub fn violation(&self) -> Option<&'static str> {
        if self.lint != format!("clippy::{LINT}") {
            return Some("suppressing a lint group that contains clippy::disallowed_methods");
        }
        if self.inner {
            return Some("module- or crate-wide exception");
        }
        if self.level != "expect" {
            return Some(
                "only a local #[expect(..., reason = \"...\")] is accepted, never #[allow]/#[warn]",
            );
        }
        if !self.reasoned {
            return Some("#[expect] without a reason");
        }
        match self.context {
            Context::RealClock | Context::Test => None,
            Context::Production => Some(
                "only rr_core::clock::RealClock (`std_time`) may call std time APIs; use an injected Clock",
            ),
        }
    }
}

impl fmt::Display for Exception {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: #{}[{}({})]",
            self.path,
            self.line,
            if self.inner { "!" } else { "" },
            self.level,
            self.lint
        )?;
        if let Some(why) = self.violation() {
            write!(f, ": {why} (D-010, US-RR-18-4)")?;
        }
        Ok(())
    }
}

/// Whether `path` (relative, `/`-separated) is in a crate's `tests/` or `benches/`.
#[must_use]
pub fn is_test_file(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    let dir = match parts.first() {
        Some(&"crates") => parts.get(2),
        Some(_) => parts.get(1),
        None => None,
    };
    matches!(dir, Some(&"tests" | &"benches"))
}

/// Every attribute in one source file that weakens the time ban.
pub fn scan_source(path: &str, src: &str) -> Result<Vec<Exception>> {
    let file = syn::parse_file(src).map_err(|e| {
        let at = e.span().start();
        anyhow::anyhow!("{path}:{}:{}: cannot parse: {e}", at.line, at.column + 1)
    })?;
    let mut s = Scanner {
        path,
        test_file: is_test_file(path),
        test_depth: 0,
        mods: Vec::new(),
        found: Vec::new(),
    };
    s.visit_file(&file);
    s.found.sort();
    Ok(s.found)
}

struct Scanner<'p> {
    path: &'p str,
    test_file: bool,
    test_depth: usize,
    mods: Vec<String>,
    found: Vec<Exception>,
}

impl Scanner<'_> {
    fn context(&self) -> Context {
        if self.test_file || self.test_depth > 0 {
            Context::Test
        } else if self.path == SANCTIONED_FILE && self.mods == [SANCTIONED_MODULE] {
            Context::RealClock
        } else {
            Context::Production
        }
    }

    fn record(&mut self, line: usize, inner: bool, level: &str, tokens: TokenStream) {
        if level == "cfg_attr" {
            // `cfg_attr(cond, allow(..), ..)`: treat as applied (fail closed).
            let mut it = tokens.into_iter().peekable();
            while let Some(tt) = it.next() {
                if let TokenTree::Ident(i) = &tt
                    && let Some(TokenTree::Group(g)) = it.peek()
                    && g.delimiter() == Delimiter::Parenthesis
                {
                    let name = i.to_string();
                    let inner_tokens = g.stream();
                    self.record(line, inner, &name, inner_tokens);
                }
            }
            return;
        }
        if !matches!(level, "allow" | "expect" | "warn") {
            return;
        }
        let flat = flatten(tokens);
        let reasoned = flat.windows(3).any(|w| {
            w[0] == Tok::Ident("reason".into())
                && w[1] == Tok::Punct('=')
                && matches!(&w[2], Tok::Lit(s) if s.trim_matches('"').trim() != "")
        });
        for w in flat.windows(4) {
            if let [
                Tok::Ident(tool),
                Tok::Punct(':'),
                Tok::Punct(':'),
                Tok::Ident(lint),
            ] = w
                && tool == "clippy"
                && (lint == LINT || GROUPS.contains(&lint.as_str()))
            {
                self.found.push(Exception {
                    path: self.path.to_owned(),
                    line,
                    level: level.to_owned(),
                    lint: format!("clippy::{lint}"),
                    inner,
                    reasoned,
                    context: self.context(),
                });
            }
        }
    }

    /// Lint attributes written inside a macro body (`#[..]` / `#![..]`).
    fn scan_macro_tokens(&mut self, tokens: TokenStream) {
        let mut it = tokens.into_iter().peekable();
        while let Some(tt) = it.next() {
            match tt {
                TokenTree::Punct(p) if p.as_char() == '#' => {
                    let line = p.span().start().line;
                    let inner =
                        matches!(it.peek(), Some(TokenTree::Punct(b)) if b.as_char() == '!');
                    if inner {
                        it.next();
                    }
                    if let Some(TokenTree::Group(g)) = it.peek()
                        && g.delimiter() == Delimiter::Bracket
                    {
                        let mut a = g.stream().into_iter();
                        if let (Some(TokenTree::Ident(level)), Some(TokenTree::Group(args))) =
                            (a.next(), a.next())
                        {
                            self.record(line, inner, &level.to_string(), args.stream());
                        }
                    }
                }
                TokenTree::Group(g) => self.scan_macro_tokens(g.stream()),
                _ => {}
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Ident(String),
    Punct(char),
    Lit(String),
}

fn flatten(tokens: TokenStream) -> Vec<Tok> {
    let mut out = Vec::new();
    for tt in tokens {
        match tt {
            TokenTree::Ident(i) => out.push(Tok::Ident(i.to_string())),
            TokenTree::Punct(p) => out.push(Tok::Punct(p.as_char())),
            TokenTree::Literal(l) => out.push(Tok::Lit(l.to_string())),
            TokenTree::Group(g) => out.extend(flatten(g.stream())),
        }
    }
    out
}

fn is_test_attr(a: &syn::Attribute) -> bool {
    let path = a.path();
    if path.segments.last().is_some_and(|s| s.ident == "test") {
        return true; // #[test], #[tokio::test], ...
    }
    if !path.is_ident("cfg") {
        return false;
    }
    let syn::Meta::List(list) = &a.meta else {
        return false;
    };
    let toks = flatten(list.tokens.clone());
    // `cfg(test)` / `cfg(all(test, ...))`; `cfg(not(test))` ships.
    toks.contains(&Tok::Ident("test".into())) && !toks.contains(&Tok::Ident("not".into()))
}

fn item_attrs(i: &syn::Item) -> &[syn::Attribute] {
    match i {
        syn::Item::Const(x) => &x.attrs,
        syn::Item::Enum(x) => &x.attrs,
        syn::Item::ExternCrate(x) => &x.attrs,
        syn::Item::Fn(x) => &x.attrs,
        syn::Item::ForeignMod(x) => &x.attrs,
        syn::Item::Impl(x) => &x.attrs,
        syn::Item::Macro(x) => &x.attrs,
        syn::Item::Mod(x) => &x.attrs,
        syn::Item::Static(x) => &x.attrs,
        syn::Item::Struct(x) => &x.attrs,
        syn::Item::Trait(x) => &x.attrs,
        syn::Item::TraitAlias(x) => &x.attrs,
        syn::Item::Type(x) => &x.attrs,
        syn::Item::Union(x) => &x.attrs,
        syn::Item::Use(x) => &x.attrs,
        _ => &[],
    }
}

impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_item(&mut self, i: &'ast syn::Item) {
        let test = item_attrs(i).iter().any(is_test_attr);
        self.test_depth += usize::from(test);
        let module = if let syn::Item::Mod(m) = i {
            self.mods.push(m.ident.to_string());
            true
        } else {
            false
        };
        visit::visit_item(self, i);
        if module {
            self.mods.pop();
        }
        self.test_depth -= usize::from(test);
    }

    fn visit_impl_item_fn(&mut self, i: &'ast syn::ImplItemFn) {
        let test = i.attrs.iter().any(is_test_attr);
        self.test_depth += usize::from(test);
        visit::visit_impl_item_fn(self, i);
        self.test_depth -= usize::from(test);
    }

    fn visit_attribute(&mut self, a: &'ast syn::Attribute) {
        if let syn::Meta::List(list) = &a.meta
            && let Some(level) = a.path().segments.last()
        {
            let line = level.ident.span().start().line;
            let inner = matches!(a.style, syn::AttrStyle::Inner(_));
            self.record(line, inner, &level.ident.to_string(), list.tokens.clone());
        }
        visit::visit_attribute(self, a);
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        self.scan_macro_tokens(m.tokens.clone());
        visit::visit_macro(self, m);
    }
}

/// Outcome of a scan of a workspace tree.
#[derive(Debug)]
pub struct Report {
    /// Number of `.rs` files scanned.
    pub files: usize,
    /// Every attribute that weakens the time ban, permitted or not.
    pub exceptions: Vec<Exception>,
}

impl Report {
    /// The exceptions that are not permitted (empty = pass).
    #[must_use]
    pub fn violations(&self) -> Vec<&Exception> {
        self.exceptions
            .iter()
            .filter(|e| e.violation().is_some())
            .collect()
    }
}

/// Scan every `.rs` file under `root/crates` and `root/xtask`.
pub fn run(root: &Path) -> Result<Report> {
    if !root.join("Cargo.toml").is_file() {
        bail!("{} is not a workspace root (no Cargo.toml)", root.display());
    }
    let files = rust_files(root)?;
    let mut exceptions = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(root)
            .unwrap_or(file)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let src =
            fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
        exceptions.extend(scan_source(&rel, &src)?);
    }
    Ok(Report {
        files: files.len(),
        exceptions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPECT: &str = "#[expect(clippy::disallowed_methods, reason = \"why\")]";

    fn scan(path: &str, src: &str) -> Vec<Exception> {
        scan_source(path, src).unwrap()
    }

    fn violations(path: &str, src: &str) -> Vec<String> {
        scan(path, src)
            .iter()
            .filter(|e| e.violation().is_some())
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn realclock_std_time_expect_is_permitted() {
        let src = format!("mod std_time {{\n    {EXPECT}\n    pub fn now() {{}}\n}}\n");
        let e = scan(SANCTIONED_FILE, &src);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].context, Context::RealClock);
        assert_eq!(e[0].line, 2);
        assert!(e[0].violation().is_none());
    }

    #[test]
    fn same_expect_elsewhere_in_production_code_fails() {
        let src = format!("{EXPECT}\npub fn now() {{}}\n");
        let v = violations("crates/rr-comms/src/lib.rs", &src);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].starts_with("crates/rr-comms/src/lib.rs:1: #[expect(clippy::disallowed_methods)]: only rr_core::clock::RealClock"), "{v:?}");
        // Outside the std_time module, even in clock.rs.
        let v = violations(
            SANCTIONED_FILE,
            &format!("mod other {{ {EXPECT} fn f() {{}} }}\n"),
        );
        assert_eq!(v.len(), 1, "{v:?}");
        // A nested module named std_time elsewhere is not RealClock's.
        let v = violations(
            "crates/rr-live/src/lib.rs",
            &format!("mod std_time {{ {EXPECT} fn f() {{}} }}\n"),
        );
        assert_eq!(v.len(), 1, "{v:?}");
    }

    #[test]
    fn allow_warn_inner_and_unreasoned_fail_even_in_realclock_and_tests() {
        for (path, src) in [
            (
                SANCTIONED_FILE,
                "mod std_time { #[allow(clippy::disallowed_methods)] fn f() {} }\n",
            ),
            (
                "crates/rr-a/tests/t.rs",
                "#[allow(clippy::disallowed_methods)]\nfn f() {}\n",
            ),
            (
                "crates/rr-a/tests/t.rs",
                "#[warn(clippy::disallowed_methods)]\nfn f() {}\n",
            ),
            (
                "crates/rr-a/tests/t.rs",
                "#![expect(clippy::disallowed_methods, reason = \"x\")]\n",
            ),
            (
                "crates/rr-a/src/lib.rs",
                "#[cfg(test)]\nmod tests { #![allow(clippy::disallowed_methods)] }\n",
            ),
            (
                "crates/rr-a/tests/t.rs",
                "#[expect(clippy::disallowed_methods)]\nfn f() {}\n",
            ),
            (
                "crates/rr-a/tests/t.rs",
                "#[expect(clippy::disallowed_methods, reason = \"\")]\nfn f() {}\n",
            ),
            (
                "crates/rr-a/tests/t.rs",
                "#[allow(clippy::all)]\nfn f() {}\n",
            ),
            ("crates/rr-a/src/lib.rs", "#![allow(clippy::style)]\n"),
        ] {
            assert_eq!(violations(path, src).len(), 1, "{path}: {src}");
        }
    }

    #[test]
    fn reasoned_expect_in_test_code_is_permitted() {
        for (path, src) in [
            (
                "crates/rr-testkit/tests/clock.rs",
                format!("{EXPECT}\nfn real() {{}}\n"),
            ),
            ("xtask/tests/x.rs", format!("{EXPECT}\nfn real() {{}}\n")),
            (
                "crates/rr-a/benches/b.rs",
                format!("{EXPECT}\nfn real() {{}}\n"),
            ),
            (
                "crates/rr-core/src/clock/virtual_clock.rs",
                format!("#[cfg(test)]\nmod tests {{\n    {EXPECT}\n    fn real() {{}}\n}}\n"),
            ),
            (
                "crates/rr-a/src/lib.rs",
                format!("#[test]\n{EXPECT}\nfn t() {{}}\n"),
            ),
            (
                "crates/rr-a/src/lib.rs",
                format!("#[cfg(all(test, unix))]\nmod t {{ {EXPECT} fn f() {{}} }}\n"),
            ),
        ] {
            let e = scan(path, &src);
            assert_eq!(e.len(), 1, "{path}");
            assert_eq!(e[0].context, Context::Test, "{path}");
            assert!(e[0].violation().is_none(), "{path}");
        }
    }

    #[test]
    fn cfg_not_test_is_production() {
        let src = format!("#[cfg(not(test))]\nmod m {{ {EXPECT} fn f() {{}} }}\n");
        assert_eq!(violations("crates/rr-a/src/lib.rs", &src).len(), 1);
        // A `tests` directory below src/ is not a test crate.
        assert!(!is_test_file("crates/rr-a/src/tests/x.rs"));
        assert!(is_test_file("crates/rr-a/tests/x.rs"));
    }

    #[test]
    fn cfg_attr_and_macro_bodies_are_seen() {
        let v = violations(
            "crates/rr-a/src/lib.rs",
            "#[cfg_attr(unix, allow(clippy::disallowed_methods))]\nfn f() {}\n",
        );
        assert_eq!(v.len(), 1, "{v:?}");
        let v = violations(
            "crates/rr-a/src/lib.rs",
            &format!("some_macro! {{\n    {EXPECT}\n    fn f() {{}}\n}}\n"),
        );
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].starts_with("crates/rr-a/src/lib.rs:2:"), "{v:?}");
    }

    #[test]
    fn unrelated_lints_and_text_are_ignored() {
        let src = "#[expect(clippy::panic, reason = \"x\")]\nfn f() {}\n// #[allow(clippy::disallowed_methods)]\nconst S: &str = \"#[allow(clippy::disallowed_methods)]\";\n";
        assert!(scan("crates/rr-a/src/lib.rs", src).is_empty());
    }
}
