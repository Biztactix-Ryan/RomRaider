//! Mutable-global checker (`cargo xtask check-globals`).
//!
//! ARCHITECTURE.md "Cross-cutting Standards" (D-011, D-019): no `static mut`, and
//! no global singletons holding mutable state. State lives in owned values that
//! are passed explicitly, so every component stays testable and deterministic.
//!
//! What is flagged, in any `.rs` file under `crates/` and `xtask/`:
//!
//! * every `static mut` item, whatever its type;
//! * every `static` whose type mentions an interior-mutability or
//!   synchronisation type ([`is_state_marker`]): `Mutex`, `RwLock`, `Cell`,
//!   `RefCell`, `UnsafeCell`, `Condvar`, `Atomic*`, ... This covers
//!   `static X: Mutex<_>`, `static X: AtomicUsize`, and lazy singletons such as
//!   `static X: LazyLock<Mutex<_>>` / `once_cell::sync::Lazy<RwLock<_>>`;
//! * every `static ref` inside `lazy_static! { ... }` whose type mentions one of
//!   those types;
//! * every `static` inside `thread_local! { ... }` (per-thread global state).
//!
//! Plain read-only statics (`static NAME: &str`, `static TABLE: [u8; 4]`,
//! `static RE: LazyLock<Regex>`) and `const` items are not flagged, except in
//! a [stateless module](STATELESS_PATHS).
//!
//! # Stateless modules
//!
//! A source under [`STATELESS_PATHS`] may hold no `static` of any kind, so
//! also no write-once global (`OnceLock`, `OnceCell`, `LazyLock`, a
//! `lazy_static!` of any type). The encoding module is one (MF-04): legacy
//! kept the byte order in a static shared by every open ROM
//! (`maps/Table.java:40`), and a `static ENDIAN: OnceLock<Endian>` set by the
//! first definition loaded would bring that bug back while passing the
//! mutable-state rule. The expression module is another (MF-25): legacy kept
//! parsed formulas in one global synchronized cache (`util/JEPUtil.java:44`),
//! and so is the conversion module built on it (MF-05): legacy's `Scale`
//! cached inverse results per scale (`maps/Scale.java:44`), and a cached
//! answer differed from an uncached one. The layer module (C1) is one too:
//! a merge is a pure function of its
//! layers, so a resolved-document cache there would make one load depend on
//! what was loaded before it. The pattern module (C2) is another: a
//! compiled pattern is plain data, and a cache of compiled patterns keyed by
//! text would be shared state between unrelated loads. Such a finding is
//! [`Kind::StaticInStatelessModule`]; `const` items stay allowed.
//!
//! A stateless module also names no lock, cell, atomic or lazy/once type
//! anywhere, in a field, a local or a `use` ([`is_sync_marker`]), and makes
//! no `thread_local!`: its values are plain data that any number of threads
//! share without coordinating. This is what makes "evaluation takes no
//! lock" checkable (MF-25, US-RR-20-4): a `Mutex` field caching results
//! inside `Expr` is still `Send + Sync`, so the type system would not catch
//! it. Such a finding is [`Kind::SyncTypeInStatelessModule`], named by the
//! type; a `static` already reported is not reported again for its type.
//!
//! Sources are parsed with `syn`, so text inside string literals and comments
//! never matches. Exceptions live in the allowlist file
//! (`xtask/globals-allowlist.toml`); each entry names a `path` and `item` and
//! must carry a non-empty `justification`. Entries that no longer match
//! anything are reported as stale.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use proc_macro2::{Delimiter, Span, TokenStream, TokenTree};
use serde::Deserialize;
use syn::visit::{self, Visit};

/// Directories (relative to the workspace root) that are scanned.
pub const SCAN_ROOTS: &[&str] = &["crates", "xtask"];

/// Directory names never descended into: build output, the read-only legacy
/// tree, and fixture / test-data directories.
pub const SKIP_DIRS: &[&str] = &[
    "target",
    "legacy",
    ".git",
    "fixture",
    "fixtures",
    "testdata",
    "test-data",
    "test_data",
];

/// Sources (relative to the workspace root, `/`-separated) that may hold no
/// `static` item at all (see [Stateless modules](self#stateless-modules)). An
/// entry ending in `/` covers every file under that directory; any other
/// entry names one file.
pub const STATELESS_PATHS: &[&str] = &[
    // Encoding (C3, MF-04): every encoding carries its byte order by value.
    "crates/rr-core/src/encoding.rs",
    "crates/rr-core/src/encoding/",
    // Expr (C4, MF-25): no parser cache or other shared state; legacy kept
    // one global synchronized cache of parsed formulas (`util/JEPUtil.java:44`).
    "crates/rr-core/src/expr.rs",
    "crates/rr-core/src/expr/",
    // Conversion (C4, MF-05, MF-25): no cache of converted or inverted
    // values; legacy's `Scale` kept a per-scale inverse cache whose answers
    // differed from an uncached call (`maps/Scale.java:44`, `:82-85`).
    "crates/rr-core/src/conversion.rs",
    "crates/rr-core/src/conversion/",
    // Layered + Resolver (C1): merging is a pure function of the layers; a
    // cache of resolved documents belongs to the caller, not the module.
    "crates/rr-core/src/layer.rs",
    "crates/rr-core/src/layer/",
    // Pattern (C2): a compiled pattern is plain data, matched without a
    // cache; a per-text cache of compiled patterns belongs to the caller.
    "crates/rr-core/src/pattern.rs",
    "crates/rr-core/src/pattern/",
];

/// Whether `path` (relative to the workspace root, `/`-separated) is under
/// [`STATELESS_PATHS`].
#[must_use]
pub fn is_stateless_path(path: &str) -> bool {
    STATELESS_PATHS.iter().any(|p| {
        if p.ends_with('/') {
            path.starts_with(p)
        } else {
            path == *p
        }
    })
}

/// Default allowlist location, relative to the workspace root.
pub const DEFAULT_ALLOWLIST: &str = "xtask/globals-allowlist.toml";

/// Whether a type identifier marks mutable state when it appears in a static's type.
#[must_use]
pub fn is_state_marker(ident: &str) -> bool {
    ident.starts_with("Atomic")
        || ident.ends_with("Mutex")
        || ident.ends_with("RwLock")
        || matches!(
            ident,
            "Cell" | "RefCell" | "UnsafeCell" | "SyncUnsafeCell" | "Condvar"
        )
}

/// Whether an identifier names interior mutability, synchronisation or
/// lazy/once state, banned anywhere in a [stateless module](self#stateless-modules):
/// every [`is_state_marker`], the `atomic` module, `Once`, `OnceLock`,
/// `OnceCell`, `LazyLock`, `LazyCell`, `Lazy` and `thread_local`.
///
/// `Barrier` is not one: it coordinates threads and holds no data, and the
/// encoding module's concurrency tests use it.
#[must_use]
pub fn is_sync_marker(ident: &str) -> bool {
    is_state_marker(ident)
        || matches!(
            ident,
            "atomic"
                | "Once"
                | "OnceLock"
                | "OnceCell"
                | "LazyLock"
                | "LazyCell"
                | "Lazy"
                | "thread_local"
        )
}

/// What kind of forbidden global a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// `static mut NAME: T = ...;`
    StaticMut,
    /// `static NAME: T = ...;` where `T` mentions a state marker.
    MutableStatic,
    /// `lazy_static! { static ref NAME: T = ...; }` where `T` mentions a state marker.
    LazyStatic,
    /// `thread_local! { static NAME: T = ...; }`
    ThreadLocal,
    /// Any other `static` (`OnceLock`, `LazyLock`, a constant table,
    /// `lazy_static!`) in a source under [`STATELESS_PATHS`].
    StaticInStatelessModule,
    /// A lock, cell, atomic or lazy/once type ([`is_sync_marker`]) named
    /// anywhere in a source under [`STATELESS_PATHS`], outside a `static`
    /// already reported.
    SyncTypeInStatelessModule,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::StaticMut => "static mut",
            Self::MutableStatic => "mutable static",
            Self::LazyStatic => "lazy_static! singleton",
            Self::ThreadLocal => "thread_local! global",
            Self::StaticInStatelessModule => "static in a stateless module",
            Self::SyncTypeInStatelessModule => "lock or cell in a stateless module",
        })
    }
}

/// One forbidden global found in a source file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Finding {
    /// Path relative to the workspace root, `/`-separated.
    pub path: String,
    /// 1-based line of the item's name.
    pub line: usize,
    /// The static's name.
    pub item: String,
    /// What was found.
    pub kind: Kind,
    /// Why it was flagged.
    pub detail: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {} `{}`: {}",
            self.path, self.line, self.kind, self.item, self.detail
        )
    }
}

/// Parse one Rust source file and return every forbidden global in it.
///
/// `path` is only used to label findings. A file that does not parse is an
/// error (fail closed), not an empty result.
pub fn scan_source(path: &str, src: &str) -> Result<Vec<Finding>> {
    let file = syn::parse_file(src).map_err(|e| {
        let at = e.span().start();
        anyhow::anyhow!("{path}:{}:{}: cannot parse: {e}", at.line, at.column + 1)
    })?;
    let mut scanner = Scanner {
        path,
        stateless: is_stateless_path(path),
        in_reported: 0,
        findings: Vec::new(),
    };
    scanner.visit_file(&file);
    let mut findings = scanner.findings;
    findings.sort();
    Ok(findings)
}

struct Scanner<'p> {
    path: &'p str,
    /// `path` is under [`STATELESS_PATHS`]: every `static` is a finding.
    stateless: bool,
    /// Inside a `static` (or `lazy_static!` / `thread_local!`) that is
    /// reported as a whole, so the types in it are not reported again.
    in_reported: usize,
    findings: Vec<Finding>,
}

/// Why a static in a stateless module is flagged.
const STATELESS_DETAIL: &str = "no `static` of any kind (OnceLock, LazyLock, tables included) \
     in a stateless module; carry the state in the value (MF-04)";

/// Why a lock or cell type in a stateless module is flagged.
const SYNC_DETAIL: &str = "no lock, cell, atomic or lazy/once type in a stateless module: \
     its values are shared across threads without coordinating (MF-04, MF-25)";

impl Scanner<'_> {
    fn push(&mut self, ident: &str, span: Span, kind: Kind, detail: String) {
        self.findings.push(Finding {
            path: self.path.to_owned(),
            line: span.start().line,
            item: ident.to_owned(),
            kind,
            detail,
        });
    }
}

impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_item_static(&mut self, i: &'ast syn::ItemStatic) {
        let name = i.ident.to_string();
        if matches!(i.mutability, syn::StaticMutability::Mut(_)) {
            self.push(
                &name,
                i.ident.span(),
                Kind::StaticMut,
                "`static mut` is banned; pass owned state explicitly".to_owned(),
            );
        } else {
            let mut idents = IdentCollector::default();
            idents.visit_type(&i.ty);
            if let Some(marker) = idents.0.iter().find(|s| is_state_marker(s)) {
                self.push(
                    &name,
                    i.ident.span(),
                    Kind::MutableStatic,
                    format!("type contains `{marker}`: global mutable state is banned"),
                );
            } else if self.stateless {
                self.push(
                    &name,
                    i.ident.span(),
                    Kind::StaticInStatelessModule,
                    STATELESS_DETAIL.to_owned(),
                );
            }
        }
        self.in_reported += 1;
        visit::visit_item_static(self, i);
        self.in_reported -= 1;
    }

    fn visit_ident(&mut self, i: &'ast proc_macro2::Ident) {
        self.flag_sync(i);
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        let last = m.path.segments.last().map(|s| s.ident.to_string());
        let kind = match last.as_deref() {
            Some("lazy_static") => Some(Kind::LazyStatic),
            Some("thread_local") => Some(Kind::ThreadLocal),
            _ => None,
        };
        if let Some(kind) = kind {
            for decl in macro_statics(m.tokens.clone()) {
                let marker = decl.type_idents.iter().find(|s| is_state_marker(s));
                match (kind, marker) {
                    (Kind::ThreadLocal, marker) => {
                        let why = marker.map_or_else(String::new, |m| {
                            format!(" (type contains `{m}`)")
                        });
                        self.push(
                            &decl.name,
                            decl.span,
                            kind,
                            format!("thread-local global state is banned{why}"),
                        );
                    }
                    (_, Some(m)) => self.push(
                        &decl.name,
                        decl.span,
                        kind,
                        format!("type contains `{m}`: lazy global singletons holding mutable state are banned"),
                    ),
                    (_, None) if self.stateless => self.push(
                        &decl.name,
                        decl.span,
                        Kind::StaticInStatelessModule,
                        STATELESS_DETAIL.to_owned(),
                    ),
                    (_, None) => {}
                }
            }
            self.in_reported += 1;
            visit::visit_macro(self, m);
            self.in_reported -= 1;
        } else {
            // `syn` leaves macro bodies as tokens: `vec![Cell::new(0)]`.
            if self.stateless && self.in_reported == 0 {
                for_each_ident(m.tokens.clone(), &mut |i| self.flag_sync(i));
            }
            visit::visit_macro(self, m);
        }
    }
}

impl Scanner<'_> {
    /// Reports `i` if it is an [`is_sync_marker`] in a stateless module.
    fn flag_sync(&mut self, i: &proc_macro2::Ident) {
        if !self.stateless || self.in_reported > 0 {
            return;
        }
        let name = i.to_string();
        if is_sync_marker(&name) {
            self.push(
                &name,
                i.span(),
                Kind::SyncTypeInStatelessModule,
                SYNC_DETAIL.to_owned(),
            );
        }
    }
}

fn for_each_ident(tokens: TokenStream, f: &mut impl FnMut(&proc_macro2::Ident)) {
    for tt in tokens {
        match tt {
            TokenTree::Ident(i) => f(&i),
            TokenTree::Group(g) => for_each_ident(g.stream(), f),
            TokenTree::Punct(_) | TokenTree::Literal(_) => {}
        }
    }
}

/// Collects every identifier in a syntax subtree (used on a static's type).
#[derive(Default)]
struct IdentCollector(Vec<String>);

impl<'ast> Visit<'ast> for IdentCollector {
    fn visit_ident(&mut self, i: &'ast proc_macro2::Ident) {
        self.0.push(i.to_string());
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        collect_idents(m.tokens.clone(), &mut self.0);
        visit::visit_macro(self, m);
    }
}

fn collect_idents(tokens: TokenStream, out: &mut Vec<String>) {
    for tt in tokens {
        match tt {
            TokenTree::Ident(i) => out.push(i.to_string()),
            TokenTree::Group(g) => collect_idents(g.stream(), out),
            TokenTree::Punct(_) | TokenTree::Literal(_) => {}
        }
    }
}

/// A `static` declaration found inside a `lazy_static!` / `thread_local!` body.
struct MacroStatic {
    name: String,
    span: Span,
    type_idents: Vec<String>,
}

/// Extract `[pub] static [ref|mut] NAME: TYPE = ...;` declarations from the
/// top level of a macro body. `'static` lifetimes are not mistaken for items.
fn macro_statics(tokens: TokenStream) -> Vec<MacroStatic> {
    // A braced body (`lazy_static!({ ... })`) is unwrapped once.
    let tts: Vec<TokenTree> = tokens.into_iter().collect();
    if let [TokenTree::Group(g)] = tts.as_slice()
        && g.delimiter() == Delimiter::Brace
    {
        return macro_statics(g.stream());
    }
    let mut out = Vec::new();
    for (idx, tt) in tts.iter().enumerate() {
        let TokenTree::Ident(kw) = tt else { continue };
        if kw != "static" {
            continue;
        }
        if idx > 0 && matches!(&tts[idx - 1], TokenTree::Punct(p) if p.as_char() == '\'') {
            continue; // `'static` lifetime
        }
        let mut j = idx + 1;
        if matches!(tts.get(j), Some(TokenTree::Ident(i)) if i == "ref" || i == "mut") {
            j += 1;
        }
        let Some(TokenTree::Ident(name)) = tts.get(j) else {
            continue;
        };
        j += 1;
        if !matches!(tts.get(j), Some(TokenTree::Punct(p)) if p.as_char() == ':') {
            continue;
        }
        j += 1;
        // Type runs to the first top-level `=` or `;` outside `<...>`.
        let mut depth = 0_usize;
        let mut ty = TokenStream::new();
        let mut prev_dash = false;
        while let Some(tt) = tts.get(j) {
            if let TokenTree::Punct(p) = tt {
                match p.as_char() {
                    '<' => depth += 1,
                    '>' if !prev_dash => depth = depth.saturating_sub(1),
                    '=' | ';' if depth == 0 => break,
                    _ => {}
                }
                prev_dash = p.as_char() == '-';
            } else {
                prev_dash = false;
            }
            ty.extend([tt.clone()]);
            j += 1;
        }
        let mut type_idents = Vec::new();
        collect_idents(ty, &mut type_idents);
        out.push(MacroStatic {
            name: name.to_string(),
            span: name.span(),
            type_idents,
        });
    }
    out
}

/// One allowlist entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllowEntry {
    /// Source path relative to the workspace root, `/`-separated.
    pub path: String,
    /// Name of the static being allowed.
    pub item: String,
    /// Why this global is acceptable. Must be non-empty.
    #[serde(default)]
    pub justification: String,
}

/// The parsed allowlist file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Allowlist {
    /// `[[allow]]` entries.
    #[serde(default)]
    pub allow: Vec<AllowEntry>,
}

impl Allowlist {
    /// Parse allowlist TOML text.
    pub fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).context("parsing globals allowlist")
    }

    /// Read and parse an allowlist file.
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("reading allowlist {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("in {}", path.display()))
    }
}

/// A reason `check-globals` fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// A forbidden global with no (valid) allowlist entry.
    Violation(Finding),
    /// An allowlist entry with an empty justification (it suppresses nothing).
    MissingJustification(AllowEntry),
    /// An allowlist entry that matches no finding.
    Stale(AllowEntry),
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Violation(v) => write!(
                f,
                "{v} (ARCHITECTURE.md Cross-cutting Standards, D-011/D-019; \
                 remove it or add a justified entry to {DEFAULT_ALLOWLIST})"
            ),
            Self::MissingJustification(e) => write!(
                f,
                "allowlist entry {} `{}` has no justification",
                e.path, e.item
            ),
            Self::Stale(e) => write!(
                f,
                "stale allowlist entry {} `{}`: no such global any more; remove it",
                e.path, e.item
            ),
        }
    }
}

/// Apply the allowlist to findings. Empty result means the check passes.
#[must_use]
pub fn check(findings: &[Finding], allow: &Allowlist) -> Vec<Problem> {
    let mut problems = Vec::new();
    let valid: Vec<&AllowEntry> = allow
        .allow
        .iter()
        .filter(|e| !e.justification.trim().is_empty())
        .collect();
    for e in allow
        .allow
        .iter()
        .filter(|e| e.justification.trim().is_empty())
    {
        problems.push(Problem::MissingJustification(e.clone()));
    }
    let matches = |e: &AllowEntry, f: &Finding| e.path == f.path && e.item == f.item;
    for f in findings {
        if !valid.iter().any(|e| matches(e, f)) {
            problems.push(Problem::Violation(f.clone()));
        }
    }
    for e in &allow.allow {
        if !findings.iter().any(|f| matches(e, f)) {
            problems.push(Problem::Stale(e.clone()));
        }
    }
    problems
}

/// Every `.rs` file under [`SCAN_ROOTS`] of `root`, skipping [`SKIP_DIRS`], sorted.
pub fn rust_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for r in SCAN_ROOTS {
        let dir = root.join(r);
        if dir.is_dir() {
            walk(&dir, &mut out)?;
        }
    }
    out.sort();
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            let name = entry.file_name();
            if !SKIP_DIRS.iter().any(|s| name == **s) {
                walk(&path, out)?;
            }
        } else if ft.is_file() && path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// Scan every source file under `root` (see [`rust_files`]).
pub fn scan_tree(root: &Path) -> Result<Vec<Finding>> {
    let mut findings = Vec::new();
    for file in rust_files(root)? {
        let rel = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let src =
            fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
        findings.extend(scan_source(&rel, &src)?);
    }
    Ok(findings)
}

/// Outcome of a full run over a tree.
#[derive(Debug)]
pub struct Report {
    /// Number of `.rs` files scanned.
    pub files: usize,
    /// Every forbidden global found, before the allowlist.
    pub findings: Vec<Finding>,
    /// What makes the check fail (empty = pass).
    pub problems: Vec<Problem>,
}

/// Scan `root` and apply the allowlist at `allowlist`.
pub fn run(root: &Path, allowlist: &Path) -> Result<Report> {
    if !root.join("Cargo.toml").is_file() {
        bail!("{} is not a workspace root (no Cargo.toml)", root.display());
    }
    let allow = Allowlist::load(allowlist)?;
    let files = rust_files(root)?.len();
    let findings = scan_tree(root)?;
    let problems = check(&findings, &allow);
    Ok(Report {
        files,
        findings,
        problems,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(src: &str) -> Vec<Finding> {
        scan_source("crates/x/src/lib.rs", src).unwrap()
    }

    fn one(src: &str) -> Finding {
        let f = scan(src);
        assert_eq!(f.len(), 1, "expected exactly one finding, got {f:?}");
        f.into_iter().next().unwrap()
    }

    #[test]
    fn static_mut_is_flagged_with_file_line_item() {
        let f = one("// header\n\nstatic mut COUNTER: u32 = 0;\n");
        assert_eq!(f.kind, Kind::StaticMut);
        assert_eq!(f.item, "COUNTER");
        assert_eq!(f.line, 3);
        assert_eq!(f.path, "crates/x/src/lib.rs");
        assert!(
            f.to_string()
                .starts_with("crates/x/src/lib.rs:3: static mut `COUNTER`"),
            "{f}"
        );
    }

    #[test]
    fn static_mut_of_plain_type_inside_fn_is_flagged() {
        let f = one("fn f() {\n    static mut BUF: [u8; 4] = [0; 4];\n}\n");
        assert_eq!(
            (f.kind, f.item.as_str(), f.line),
            (Kind::StaticMut, "BUF", 2)
        );
    }

    #[test]
    fn static_mutex_is_flagged() {
        let f =
            one("use std::sync::Mutex;\nstatic STATE: Mutex<Vec<u8>> = Mutex::new(Vec::new());\n");
        assert_eq!(
            (f.kind, f.item.as_str(), f.line),
            (Kind::MutableStatic, "STATE", 2)
        );
        assert!(f.detail.contains("Mutex"), "{}", f.detail);
    }

    #[test]
    fn lazylock_of_mutex_is_flagged() {
        let f = one(
            "static REG: std::sync::LazyLock<std::sync::Mutex<Vec<u32>>> =\n    std::sync::LazyLock::new(|| std::sync::Mutex::new(Vec::new()));\n",
        );
        assert_eq!(
            (f.kind, f.item.as_str(), f.line),
            (Kind::MutableStatic, "REG", 1)
        );
    }

    #[test]
    fn once_cell_lazy_of_rwlock_is_flagged() {
        let f = one(
            "static CFG: once_cell::sync::Lazy<parking_lot::RwLock<u8>> = Lazy::new(Default::default);\n",
        );
        assert_eq!(f.kind, Kind::MutableStatic);
        assert!(f.detail.contains("RwLock"), "{}", f.detail);
    }

    #[test]
    fn atomic_static_is_flagged() {
        let f = one(
            "use std::sync::atomic::AtomicUsize;\n\nstatic NEXT_ID: AtomicUsize = AtomicUsize::new(0);\n",
        );
        assert_eq!(
            (f.kind, f.item.as_str(), f.line),
            (Kind::MutableStatic, "NEXT_ID", 3)
        );
    }

    #[test]
    fn lazy_static_mutex_is_flagged() {
        let f = one(
            "lazy_static::lazy_static! {\n    /// doc\n    pub static ref CACHE: Mutex<HashMap<u32, String>> = Mutex::new(HashMap::new());\n}\n",
        );
        assert_eq!(
            (f.kind, f.item.as_str(), f.line),
            (Kind::LazyStatic, "CACHE", 3)
        );
    }

    #[test]
    fn lazy_static_read_only_is_not_flagged() {
        assert!(
            scan("lazy_static! {\n    static ref NAMES: Vec<&'static str> = vec![\"a\"];\n}\n")
                .is_empty()
        );
    }

    #[test]
    fn thread_local_refcell_is_flagged() {
        let f = one(
            "thread_local! {\n    static SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };\n}\n",
        );
        assert_eq!(
            (f.kind, f.item.as_str(), f.line),
            (Kind::ThreadLocal, "SCRATCH", 2)
        );
        assert!(f.detail.contains("RefCell"), "{}", f.detail);
    }

    #[test]
    fn thread_local_inside_fn_and_several_decls() {
        let f = scan(
            "fn f() {\n    std::thread_local! {\n        static A: Cell<u8> = Cell::new(0);\n        static B: &'static str = \"x\";\n    }\n}\n",
        );
        let names: Vec<_> = f.iter().map(|f| (f.item.as_str(), f.line)).collect();
        assert_eq!(names, [("A", 3), ("B", 4)]);
    }

    #[test]
    fn stateless_paths_cover_the_encoding_expr_conversion_layer_and_pattern_modules_only() {
        for p in [
            "crates/rr-core/src/encoding.rs",
            "crates/rr-core/src/encoding/codec.rs",
            "crates/rr-core/src/encoding/codec/tests.rs",
            "crates/rr-core/src/expr.rs",
            "crates/rr-core/src/expr/parser.rs",
            "crates/rr-core/src/conversion.rs",
            "crates/rr-core/src/conversion/lut.rs",
            "crates/rr-core/src/layer.rs",
            "crates/rr-core/src/layer/list.rs",
            "crates/rr-core/src/pattern.rs",
            "crates/rr-core/src/pattern/parse.rs",
            "crates/rr-core/src/pattern/parse/tests.rs",
        ] {
            assert!(is_stateless_path(p), "{p}");
        }
        for p in [
            "crates/rr-core/src/encoding_x.rs",
            "crates/rr-core/src/encodings/codec.rs",
            "crates/rr-core/src/expression.rs",
            "crates/rr-core/src/exprs/parser.rs",
            "crates/rr-core/src/conversions.rs",
            "crates/rr-core/src/conversion_x/lut.rs",
            "crates/rr-core/src/layers.rs",
            "crates/rr-core/src/layered/list.rs",
            "crates/rr-core/src/patterns.rs",
            "crates/rr-core/src/pattern_x/bytes.rs",
            "crates/rr-core/src/diag.rs",
            "crates/rr-testkit/src/encoding_oracle.rs",
            "xtask/src/globals.rs",
        ] {
            assert!(!is_stateless_path(p), "{p}");
        }
    }

    #[test]
    fn every_static_in_a_stateless_module_is_flagged() {
        // MF-04: a write-once global byte order passes the mutable-state
        // rule, but not the encoding module's no-static rule.
        let src = "\
use std::sync::{LazyLock, OnceLock};
const MAX: usize = 4;
static ENDIAN: OnceLock<Endian> = OnceLock::new();
static TABLE: [u8; 4] = [1, 2, 3, 4];
static RE: LazyLock<Vec<u8>> = LazyLock::new(Vec::new);
lazy_static! { static ref DEFAULT: Endian = Endian::Big; }
fn f() -> &'static str { static INNER: &str = \"x\"; INNER }
static mut HITS: u32 = 0;
static FLAG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
thread_local! { static T: u8 = 0; }
";
        let got = |path: &str| -> Vec<(String, Kind)> {
            scan_source(path, src)
                .unwrap()
                .into_iter()
                .map(|f| (f.item, f.kind))
                .collect()
        };
        let named = |v: &[(&str, Kind)]| -> Vec<(String, Kind)> {
            let mut v: Vec<_> = v.iter().map(|&(n, k)| (n.to_owned(), k)).collect();
            v.sort();
            v
        };
        let mut stateless = got("crates/rr-core/src/encoding/codec.rs");
        stateless.sort();
        assert_eq!(
            stateless,
            named(&[
                ("DEFAULT", Kind::StaticInStatelessModule),
                ("ENDIAN", Kind::StaticInStatelessModule),
                ("FLAG", Kind::MutableStatic),
                ("HITS", Kind::StaticMut),
                ("INNER", Kind::StaticInStatelessModule),
                // The `use` names them outside any static.
                ("LazyLock", Kind::SyncTypeInStatelessModule),
                ("OnceLock", Kind::SyncTypeInStatelessModule),
                ("RE", Kind::StaticInStatelessModule),
                ("T", Kind::ThreadLocal),
                ("TABLE", Kind::StaticInStatelessModule),
            ])
        );
        let mut elsewhere = got("crates/rr-core/src/diag.rs");
        elsewhere.sort();
        assert_eq!(
            elsewhere,
            named(&[
                ("FLAG", Kind::MutableStatic),
                ("HITS", Kind::StaticMut),
                ("T", Kind::ThreadLocal),
            ])
        );
        let f = scan_source(
            "crates/rr-core/src/encoding.rs",
            "static ENDIAN: OnceLock<Endian> = OnceLock::new();\n",
        )
        .unwrap();
        assert_eq!(f.len(), 1, "{f:?}");
        assert!(
            f[0].to_string().starts_with(
                "crates/rr-core/src/encoding.rs:1: static in a stateless module `ENDIAN`"
            ),
            "{}",
            f[0]
        );
    }

    #[test]
    fn a_lock_or_cell_anywhere_in_a_stateless_module_is_flagged() {
        // MF-25: a cache behind a lock inside the formula type is still
        // `Send + Sync`; only the source says it locks.
        let src = "\
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
pub struct Expr {
    nodes: Vec<u8>,
    cache: Mutex<Vec<(f64, f64)>>,
}
fn eval(e: &Expr) -> f64 {
    let hits = std::cell::Cell::new(0);
    let v = vec![core::cell::RefCell::new(0.0)];
    let once: std::sync::OnceLock<u8> = std::sync::OnceLock::new();
    let barrier = std::sync::Barrier::new(1);
    0.0
}
";
        let got = |path: &str| -> Vec<(String, usize)> {
            scan_source(path, src)
                .unwrap()
                .into_iter()
                .inspect(|f| assert_eq!(f.kind, Kind::SyncTypeInStatelessModule, "{f}"))
                .map(|f| (f.item, f.line))
                .collect()
        };
        let mut found = got("crates/rr-core/src/expr/ast.rs");
        found.sort();
        assert_eq!(
            found,
            [
                ("AtomicU64".to_owned(), 2),
                ("Cell".to_owned(), 8),
                ("Mutex".to_owned(), 1),
                ("Mutex".to_owned(), 5),
                ("OnceLock".to_owned(), 10),
                ("OnceLock".to_owned(), 10),
                ("RefCell".to_owned(), 9),
                ("atomic".to_owned(), 2),
            ]
        );
        let f = &scan_source("crates/rr-core/src/expr/ast.rs", src).unwrap()[0];
        assert!(
            f.to_string().starts_with(
                "crates/rr-core/src/expr/ast.rs:1: lock or cell in a stateless module `Mutex`"
            ),
            "{f}"
        );
        // Elsewhere the same source holds no global, so nothing.
        assert_eq!(got("crates/rr-core/src/diag.rs"), []);
    }

    #[test]
    fn sync_markers() {
        for m in [
            "Mutex",
            "RwLock",
            "Cell",
            "RefCell",
            "AtomicBool",
            "atomic",
            "Once",
            "OnceLock",
            "OnceCell",
            "LazyLock",
            "LazyCell",
            "Lazy",
            "thread_local",
        ] {
            assert!(is_sync_marker(m), "{m}");
        }
        for m in ["Barrier", "Arc", "Vec", "once", "Ordering", "Expr"] {
            assert!(!is_sync_marker(m), "{m}");
        }
    }

    #[test]
    fn allowed_constants_are_not_flagged() {
        let src = "\
static NAME: &str = \"romraider\";
const MAX: usize = 4;
static TABLE: [u8; 4] = [1, 2, 3, 4];
pub static GREETING: &'static str = \"hi\";
static RE: std::sync::LazyLock<Vec<u8>> = std::sync::LazyLock::new(Vec::new);
fn f() -> &'static str { static INNER: &str = \"x\"; INNER }
";
        assert!(scan(src).is_empty(), "{:?}", scan(src));
    }

    #[test]
    fn patterns_in_strings_and_comments_are_ignored() {
        let src = "// static mut X: u8 = 0;\nconst S: &str = \"static mut Y: Mutex<u8>\";\n/// lazy_static! { static ref Z: Mutex<u8> = ..; }\nfn f() {}\n";
        assert!(scan(src).is_empty());
    }

    #[test]
    fn unparseable_source_is_an_error() {
        let err = scan_source("crates/x/src/bad.rs", "fn (").unwrap_err();
        assert!(err.to_string().contains("crates/x/src/bad.rs"), "{err}");
    }

    #[test]
    fn state_markers() {
        for m in [
            "Mutex",
            "RwLock",
            "RefCell",
            "Cell",
            "UnsafeCell",
            "AtomicBool",
            "AtomicU64",
            "ReentrantMutex",
            "Condvar",
        ] {
            assert!(is_state_marker(m), "{m}");
        }
        for m in ["OnceLock", "LazyLock", "Vec", "str", "OnceCell", "Arc"] {
            assert!(!is_state_marker(m), "{m}");
        }
    }

    fn allowlist(text: &str) -> Allowlist {
        Allowlist::parse(text).unwrap()
    }

    const PLANTED: &str = "static mut COUNTER: u32 = 0;\n";

    #[test]
    fn unallowlisted_finding_is_a_violation() {
        let p = check(&scan(PLANTED), &Allowlist::default());
        assert!(
            matches!(p.as_slice(), [Problem::Violation(f)] if f.item == "COUNTER"),
            "{p:?}"
        );
    }

    #[test]
    fn allowlisted_planted_item_passes() {
        let allow = allowlist(
            "[[allow]]\npath = \"crates/x/src/lib.rs\"\nitem = \"COUNTER\"\njustification = \"FFI callback context, see D-0xx\"\n",
        );
        assert_eq!(check(&scan(PLANTED), &allow), []);
    }

    #[test]
    fn allowlist_entry_for_other_file_does_not_suppress() {
        let allow = allowlist(
            "[[allow]]\npath = \"crates/y/src/lib.rs\"\nitem = \"COUNTER\"\njustification = \"x\"\n",
        );
        let p = check(&scan(PLANTED), &allow);
        assert!(
            p.iter().any(|p| matches!(p, Problem::Violation(_))),
            "{p:?}"
        );
        assert!(p.iter().any(|p| matches!(p, Problem::Stale(_))), "{p:?}");
    }

    #[test]
    fn entry_without_justification_is_an_error_and_suppresses_nothing() {
        for body in ["", "justification = \"   \"\n"] {
            let allow = allowlist(&format!(
                "[[allow]]\npath = \"crates/x/src/lib.rs\"\nitem = \"COUNTER\"\n{body}"
            ));
            let p = check(&scan(PLANTED), &allow);
            assert!(
                p.iter()
                    .any(|p| matches!(p, Problem::MissingJustification(_))),
                "{p:?}"
            );
            assert!(
                p.iter().any(|p| matches!(p, Problem::Violation(_))),
                "{p:?}"
            );
        }
    }

    #[test]
    fn stale_entry_is_reported() {
        let allow = allowlist(
            "[[allow]]\npath = \"crates/x/src/lib.rs\"\nitem = \"GONE\"\njustification = \"was needed once\"\n",
        );
        let p = check(&[], &allow);
        assert!(
            matches!(p.as_slice(), [Problem::Stale(e)] if e.item == "GONE"),
            "{p:?}"
        );
        assert!(p[0].to_string().contains("stale"), "{}", p[0]);
    }

    #[test]
    fn allowlist_rejects_unknown_fields() {
        assert!(
            Allowlist::parse("[[allow]]\npath = \"a\"\nitem = \"b\"\nreason = \"c\"\n").is_err()
        );
        assert_eq!(allowlist("# empty\n"), Allowlist::default());
    }
}
