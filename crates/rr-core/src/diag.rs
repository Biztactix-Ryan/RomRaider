//! Diagnostics (concept C15, decision D-019).
//!
//! A [`Diagnostic`] is one problem found while loading or resolving
//! something: a malformed definition, an unknown scaling, a quirk rule that
//! matched nothing, a ROM that does not fit its definition. Library code
//! never throws these, prints them or shows them in a popup (MF-40); it
//! *collects* them and hands them back with whatever it managed to build, so
//! every problem is visible to the caller and assertable in a test (MF-23).
//!
//! A diagnostic is `{ severity, code, message, provenance }`:
//!
//! * [`Severity`] — [`Info`](Severity::Info), [`Warning`](Severity::Warning)
//!   or [`Error`](Severity::Error). Errors make the owning load fail closed.
//! * [`Code`] — a short, stable identifier (e.g. `DEF0001`). Codes are what
//!   tests and tools match on; messages are for humans and may be reworded.
//! * a message — one human-readable sentence.
//! * [`Provenance`] — exactly where the problem came from: the source
//!   (file name or other label), a [`Span`] inside it, and optionally the
//!   layer and rule id that produced the offending value.
//!
//! # Rendering
//!
//! [`Provenance`] renders as `file:line:col` for text sources and as
//! `file@0xOFFSET` (upper-case hex byte offset) for binary sources, followed
//! by the layer and rule when known:
//!
//! ```
//! use rr_core::diag::{Diagnostic, Provenance, Span, codes};
//!
//! let text = Provenance::new("defs/16BITBASE.xml", Span::line_col(42, 7))
//!     .with_layer("base")
//!     .with_rule("table.scaling");
//! assert_eq!(
//!     text.to_string(),
//!     "defs/16BITBASE.xml:42:7 [layer base, rule table.scaling]"
//! );
//!
//! let binary = Provenance::new("A2WC522N.bin", Span::byte_offset(0x1F3A0));
//! assert_eq!(binary.to_string(), "A2WC522N.bin@0x1F3A0");
//!
//! let diag = Diagnostic::error(codes::UNKNOWN_SCALING, "unknown scaling \"psi\"", text);
//! assert_eq!(
//!     diag.to_string(),
//!     "defs/16BITBASE.xml:42:7 [layer base, rule table.scaling]: \
//!      error[DEF0001]: unknown scaling \"psi\""
//! );
//! ```
//!
//! Every rendering is a single line. Source names, layers, rule ids and
//! messages can carry text taken from untrusted input (definition packs,
//! ROMs, XDFs, CSVs), so control characters in them are escaped
//! (`\n`, `\u{1b}`, ...) when displayed; a crafted name cannot forge extra
//! log lines or terminal escape sequences.
//!
//! # Collecting and failing closed
//!
//! A loader records problems in a [`Diagnostics`] collector and ends with
//! [`Diagnostics::finish`], returning a [`Loaded<T>`]: the value plus every
//! diagnostic. If any diagnostic is an [`Error`](Severity::Error), `Loaded`
//! withholds the value — [`Loaded::value`] is `None` and
//! [`Loaded::into_result`] is a [`Rejected`] — until the caller explicitly
//! calls [`accept_errors`](Loaded::accept_errors). Warnings and infos never
//! withhold it. The diagnostics are available either way.
//!
//! # Codes
//!
//! Every code is defined once, with its documentation, in the [`codes`]
//! registry; `docs/diagnostics.md` is generated from it. Use the registry's
//! constants (`codes::UNKNOWN_SCALING`) rather than [`Code::new`] outside
//! tests.

pub mod codes;

use core::fmt;

/// How serious a [`Diagnostic`] is.
///
/// Ordered by seriousness: `Info < Warning < Error`, so the worst severity in
/// a set is its `max()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Worth knowing, nothing is wrong (e.g. a deprecated attribute was read).
    Info,
    /// Something was wrong but was handled with a well-defined fallback; the
    /// result is usable.
    Warning,
    /// The result cannot be trusted. The owning load fails closed: the caller
    /// does not get a usable value without explicitly accepting the errors.
    Error,
}

impl Severity {
    /// `true` for [`Severity::Error`].
    #[must_use]
    pub const fn is_error(self) -> bool {
        matches!(self, Self::Error)
    }

    /// The lower-case name used when rendering: `info`, `warning` or `error`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A stable diagnostic code, e.g. `DEF0001`.
///
/// Codes are `&'static str` so they can only come from constants in the
/// source, never from input. Each code is defined once, in the [`codes`]
/// registry, and never reused for a different meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Code(&'static str);

impl Code {
    /// Wraps a code string. Intended for `const` definitions:
    /// `pub const UNKNOWN_SCALING: Code = Code::new("DEF0001");`
    #[must_use]
    pub const fn new(code: &'static str) -> Self {
        Self(code)
    }

    /// The code string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// A location inside a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Span {
    /// A position in a text source. Both numbers are **1-based**, as editors
    /// show them; `col` counts characters on the line.
    LineCol {
        /// 1-based line number.
        line: u32,
        /// 1-based column number.
        col: u32,
    },
    /// A position in a binary source (a ROM image, a trace), as a 0-based
    /// byte offset from the start of the source.
    ByteOffset(u64),
}

impl Span {
    /// A text position. `line` and `col` are 1-based.
    #[must_use]
    pub const fn line_col(line: u32, col: u32) -> Self {
        Self::LineCol { line, col }
    }

    /// A binary position: a 0-based byte offset.
    #[must_use]
    pub const fn byte_offset(offset: u64) -> Self {
        Self::ByteOffset(offset)
    }
}

/// Renders as `line:col` for text positions and `@0xOFFSET` for byte
/// offsets — the part that follows the source name in a [`Provenance`].
impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::LineCol { line, col } => write!(f, ":{line}:{col}"),
            Self::ByteOffset(offset) => write!(f, "@{offset:#X}"),
        }
    }
}

/// Where a [`Diagnostic`] came from.
///
/// `source` names the input (a file path, or a label such as `<clipboard>`
/// for inputs that are not files); `span` locates the problem inside it.
/// `layer` and `rule_id` are set when the offending value came out of a
/// layered resolve (C1) or a quirk rule (C8), so the user can see *which*
/// layer or rule to fix, not just which file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Provenance {
    source: String,
    span: Span,
    layer: Option<String>,
    rule_id: Option<String>,
}

impl Provenance {
    /// A provenance with no layer or rule.
    #[must_use]
    pub fn new(source: impl Into<String>, span: Span) -> Self {
        Self {
            source: source.into(),
            span,
            layer: None,
            rule_id: None,
        }
    }

    /// Sets the layer that supplied the offending value.
    #[must_use]
    pub fn with_layer(mut self, layer: impl Into<String>) -> Self {
        self.layer = Some(layer.into());
        self
    }

    /// Sets the id of the rule that produced the offending value.
    #[must_use]
    pub fn with_rule(mut self, rule_id: impl Into<String>) -> Self {
        self.rule_id = Some(rule_id.into());
        self
    }

    /// The source name (file path or label).
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The location inside the source.
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }

    /// The layer that supplied the offending value, if known.
    #[must_use]
    pub fn layer(&self) -> Option<&str> {
        self.layer.as_deref()
    }

    /// The id of the rule that produced the offending value, if known.
    #[must_use]
    pub fn rule_id(&self) -> Option<&str> {
        self.rule_id.as_deref()
    }
}

/// `file:line:col` for text sources, `file@0xOFFSET` for binary sources,
/// then ` [layer L, rule R]` (either part omitted when unknown).
impl fmt::Display for Provenance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_escaped(f, &self.source)?;
        write!(f, "{}", self.span)?;
        match (&self.layer, &self.rule_id) {
            (None, None) => Ok(()),
            (Some(layer), None) => {
                f.write_str(" [layer ")?;
                write_escaped(f, layer)?;
                f.write_str("]")
            }
            (None, Some(rule)) => {
                f.write_str(" [rule ")?;
                write_escaped(f, rule)?;
                f.write_str("]")
            }
            (Some(layer), Some(rule)) => {
                f.write_str(" [layer ")?;
                write_escaped(f, layer)?;
                f.write_str(", rule ")?;
                write_escaped(f, rule)?;
                f.write_str("]")
            }
        }
    }
}

/// One problem found while loading or resolving: collected, never thrown.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Diagnostic {
    severity: Severity,
    code: Code,
    message: String,
    provenance: Provenance,
}

impl Diagnostic {
    /// A diagnostic of any severity.
    #[must_use]
    pub fn new(
        severity: Severity,
        code: Code,
        message: impl Into<String>,
        provenance: Provenance,
    ) -> Self {
        Self {
            severity,
            code,
            message: message.into(),
            provenance,
        }
    }

    /// A [`Severity::Error`] diagnostic.
    #[must_use]
    pub fn error(code: Code, message: impl Into<String>, provenance: Provenance) -> Self {
        Self::new(Severity::Error, code, message, provenance)
    }

    /// A [`Severity::Warning`] diagnostic.
    #[must_use]
    pub fn warning(code: Code, message: impl Into<String>, provenance: Provenance) -> Self {
        Self::new(Severity::Warning, code, message, provenance)
    }

    /// A [`Severity::Info`] diagnostic.
    #[must_use]
    pub fn info(code: Code, message: impl Into<String>, provenance: Provenance) -> Self {
        Self::new(Severity::Info, code, message, provenance)
    }

    /// How serious this is.
    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.severity
    }

    /// `true` if this diagnostic is a [`Severity::Error`].
    #[must_use]
    pub const fn is_error(&self) -> bool {
        self.severity.is_error()
    }

    /// The stable code.
    #[must_use]
    pub const fn code(&self) -> Code {
        self.code
    }

    /// The human-readable message (unescaped, as constructed).
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Where the problem came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// `{provenance}: {severity}[{code}]: {message}` on one line, e.g.
/// `defs/base.xml:42:7: error[DEF0001]: unknown scaling "psi"`.
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}[{}]: ", self.provenance, self.severity, self.code)?;
        write_escaped(f, &self.message)
    }
}

/// Number of [`Severity::Error`] diagnostics in `diags`.
fn count_errors(diags: &[Diagnostic]) -> usize {
    diags.iter().filter(|d| d.is_error()).count()
}

/// The most serious severity in `diags`, or `None` if it is empty.
fn worst_severity(diags: &[Diagnostic]) -> Option<Severity> {
    diags.iter().map(Diagnostic::severity).max()
}

/// Gathers [`Diagnostic`]s while something is being loaded.
///
/// A loader creates one, records every problem it finds as it goes, and ends
/// with [`finish`](Self::finish), which pairs whatever it managed to build
/// with everything it found as a fail-closed [`Loaded`]. Diagnostics keep the
/// order they were recorded in.
///
/// ```
/// use rr_core::diag::{Code, Diagnostics, Provenance, Span};
///
/// const ODD_LENGTH: Code = Code::new("DEF0002");
///
/// let mut diags = Diagnostics::new();
/// diags.warning(
///     ODD_LENGTH,
///     "table length 7 is odd",
///     Provenance::new("defs/base.xml", Span::line_col(3, 1)),
/// );
/// assert!(!diags.has_errors());
///
/// let loaded = diags.finish(vec![1u8, 2, 3]);
/// assert_eq!(loaded.value(), Some(&vec![1, 2, 3]));
/// assert_eq!(loaded.diagnostics().len(), 1);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    /// An empty collector.
    #[must_use]
    pub const fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Records a diagnostic.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    /// Records a [`Severity::Error`] diagnostic.
    pub fn error(&mut self, code: Code, message: impl Into<String>, provenance: Provenance) {
        self.push(Diagnostic::error(code, message, provenance));
    }

    /// Records a [`Severity::Warning`] diagnostic.
    pub fn warning(&mut self, code: Code, message: impl Into<String>, provenance: Provenance) {
        self.push(Diagnostic::warning(code, message, provenance));
    }

    /// Records a [`Severity::Info`] diagnostic.
    pub fn info(&mut self, code: Code, message: impl Into<String>, provenance: Provenance) {
        self.push(Diagnostic::info(code, message, provenance));
    }

    /// Number of diagnostics recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// `true` if nothing has been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// `true` if any recorded diagnostic is a [`Severity::Error`], i.e. a
    /// load finished now would fail closed.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.error_count() > 0
    }

    /// Number of [`Severity::Error`] diagnostics recorded.
    #[must_use]
    pub fn error_count(&self) -> usize {
        count_errors(&self.items)
    }

    /// The most serious severity recorded, or `None` if nothing was.
    #[must_use]
    pub fn worst(&self) -> Option<Severity> {
        worst_severity(&self.items)
    }

    /// The diagnostics, in the order they were recorded.
    #[must_use]
    pub fn as_slice(&self) -> &[Diagnostic] {
        &self.items
    }

    /// Iterates over the diagnostics in the order they were recorded.
    pub fn iter(&self) -> core::slice::Iter<'_, Diagnostic> {
        self.items.iter()
    }

    /// The diagnostics as a `Vec`, in the order they were recorded.
    #[must_use]
    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.items
    }

    /// Ends the load: pairs `value` with everything recorded. The result
    /// withholds `value` if any [`Severity::Error`] was recorded; see
    /// [`Loaded`].
    pub fn finish<T>(self, value: T) -> Loaded<T> {
        Loaded::new(value, self.items)
    }
}

impl From<Vec<Diagnostic>> for Diagnostics {
    fn from(items: Vec<Diagnostic>) -> Self {
        Self { items }
    }
}

impl From<Diagnostics> for Vec<Diagnostic> {
    fn from(diags: Diagnostics) -> Self {
        diags.items
    }
}

impl Extend<Diagnostic> for Diagnostics {
    fn extend<I: IntoIterator<Item = Diagnostic>>(&mut self, iter: I) {
        self.items.extend(iter);
    }
}

impl FromIterator<Diagnostic> for Diagnostics {
    fn from_iter<I: IntoIterator<Item = Diagnostic>>(iter: I) -> Self {
        Self {
            items: iter.into_iter().collect(),
        }
    }
}

impl IntoIterator for Diagnostics {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a> IntoIterator for &'a Diagnostics {
    type Item = &'a Diagnostic;
    type IntoIter = core::slice::Iter<'a, Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

/// The result of a load: a value plus every [`Diagnostic`] found building it
/// — the `(T, Vec<Diagnostic>)` of C15, made fail-closed.
///
/// If any diagnostic is a [`Severity::Error`] the value cannot be trusted,
/// and `Loaded` withholds it: [`value`](Self::value) returns `None` and
/// [`into_result`](Self::into_result) returns [`Rejected`]. The only way to
/// get the value out regardless is to say so, with
/// [`accept_errors`](Self::accept_errors) (here or on the [`Rejected`]).
/// There is no unconditional getter, no `Deref` and no public field, so
/// using an errored value is always a visible, greppable decision.
///
/// With only [`Warning`](Severity::Warning) and [`Info`](Severity::Info)
/// diagnostics the value is available normally. The diagnostics are always
/// available, whichever way the value goes.
///
/// ```
/// use rr_core::diag::{Code, Diagnostics, Provenance, Span};
///
/// const UNKNOWN_SCALING: Code = Code::new("DEF0001");
///
/// let mut diags = Diagnostics::new();
/// diags.error(
///     UNKNOWN_SCALING,
///     "unknown scaling \"psi\"",
///     Provenance::new("defs/base.xml", Span::line_col(42, 7)),
/// );
/// let loaded = diags.finish("partial definition");
///
/// // Fail closed: the normal accessors withhold the value...
/// assert_eq!(loaded.value(), None);
/// let rejected = loaded.into_result().unwrap_err();
/// assert_eq!(rejected.error_count(), 1);
///
/// // ...until the caller explicitly accepts the errors.
/// let (value, diagnostics) = rejected.accept_errors();
/// assert_eq!(value, "partial definition");
/// assert_eq!(diagnostics[0].code(), UNKNOWN_SCALING);
/// ```
///
/// The value is not reachable through the fields:
///
/// ```compile_fail,E0451
/// use rr_core::diag::Loaded;
///
/// let loaded = Loaded::clean(1u8);
/// let Loaded { value, .. } = loaded; // error: fields are private
/// ```
///
/// A loader that builds on a nested load folds the nested diagnostics into
/// its own collector, so a nested error makes the outer load fail closed
/// too:
///
/// ```
/// use rr_core::diag::{Code, Diagnostics, Loaded, Provenance, Span};
///
/// fn load_inner() -> Loaded<u32> {
///     let mut diags = Diagnostics::new();
///     diags.error(
///         Code::new("DEF0001"),
///         "bad",
///         Provenance::new("inner.xml", Span::line_col(1, 1)),
///     );
///     diags.finish(7)
/// }
///
/// let mut outer = Diagnostics::new();
/// let (inner, inner_diags) = load_inner().accept_errors();
/// outer.extend(inner_diags);
/// let loaded = outer.finish(inner * 2);
/// assert!(loaded.has_errors());
/// assert_eq!(loaded.value(), None);
/// ```
#[must_use = "a load result carries diagnostics; inspect them or the value"]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Loaded<T> {
    value: T,
    diagnostics: Vec<Diagnostic>,
}

impl<T> Loaded<T> {
    /// Pairs a value with the diagnostics found building it.
    pub fn new(value: T, diagnostics: impl IntoIterator<Item = Diagnostic>) -> Self {
        Self {
            value,
            diagnostics: diagnostics.into_iter().collect(),
        }
    }

    /// A value loaded with no diagnostics at all.
    pub const fn clean(value: T) -> Self {
        Self {
            value,
            diagnostics: Vec::new(),
        }
    }

    /// Every diagnostic, in the order recorded. Available whether or not the
    /// value is.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// `true` if any diagnostic is a [`Severity::Error`], so the value is
    /// withheld.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.error_count() > 0
    }

    /// Number of [`Severity::Error`] diagnostics.
    #[must_use]
    pub fn error_count(&self) -> usize {
        count_errors(&self.diagnostics)
    }

    /// The most serious severity among the diagnostics, or `None` if there
    /// are none.
    #[must_use]
    pub fn worst(&self) -> Option<Severity> {
        worst_severity(&self.diagnostics)
    }

    /// The value, or `None` if any diagnostic is a [`Severity::Error`].
    #[must_use]
    pub fn value(&self) -> Option<&T> {
        if self.has_errors() {
            None
        } else {
            Some(&self.value)
        }
    }

    /// The value and its diagnostics, or [`Rejected`] if any diagnostic is a
    /// [`Severity::Error`].
    ///
    /// # Errors
    ///
    /// [`Rejected`] when the load had at least one error. It still holds the
    /// value and the diagnostics; see [`Rejected::accept_errors`].
    pub fn into_result(self) -> Result<(T, Vec<Diagnostic>), Rejected<T>> {
        if self.has_errors() {
            Err(Rejected {
                value: self.value,
                diagnostics: self.diagnostics,
            })
        } else {
            Ok((self.value, self.diagnostics))
        }
    }

    /// The value and its diagnostics **even if there are errors**. Calling
    /// this is the caller's explicit decision to use a value the loader could
    /// not vouch for; the diagnostics come back with it so they can still be
    /// shown.
    #[must_use = "accepting errors only to drop the value is pointless; use into_diagnostics"]
    pub fn accept_errors(self) -> (T, Vec<Diagnostic>) {
        (self.value, self.diagnostics)
    }

    /// Drops the value and keeps the diagnostics.
    #[must_use]
    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }

    /// Transforms the value, keeping the diagnostics, so the result is
    /// withheld exactly when this one is.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Loaded<U> {
        Loaded {
            value: f(self.value),
            diagnostics: self.diagnostics,
        }
    }
}

/// A load that failed closed: at least one [`Severity::Error`] diagnostic.
///
/// Returned by [`Loaded::into_result`]. It is an error type (it implements
/// [`std::error::Error`] and renders the first error), but it keeps the
/// value and every diagnostic so a caller can still report them all, or
/// explicitly [`accept_errors`](Self::accept_errors).
#[must_use = "a rejected load carries diagnostics that should be reported"]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Rejected<T> {
    value: T,
    diagnostics: Vec<Diagnostic>,
}

impl<T> Rejected<T> {
    /// Every diagnostic (errors and the rest), in the order recorded.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// The [`Severity::Error`] diagnostics only, in the order recorded.
    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter().filter(|d| d.is_error())
    }

    /// Number of [`Severity::Error`] diagnostics (at least one).
    #[must_use]
    pub fn error_count(&self) -> usize {
        count_errors(&self.diagnostics)
    }

    /// The value and its diagnostics despite the errors. See
    /// [`Loaded::accept_errors`].
    #[must_use = "accepting errors only to drop the value is pointless; use into_diagnostics"]
    pub fn accept_errors(self) -> (T, Vec<Diagnostic>) {
        (self.value, self.diagnostics)
    }

    /// Drops the value and keeps the diagnostics.
    #[must_use]
    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }
}

/// `load failed closed with N error(s); first: {diagnostic}` on one line.
impl<T> fmt::Display for Rejected<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self.error_count();
        let plural = if n == 1 { "" } else { "s" };
        write!(f, "load failed closed with {n} error{plural}")?;
        match self.errors().next() {
            Some(first) => write!(f, "; first: {first}"),
            None => Ok(()),
        }
    }
}

impl<T: fmt::Debug> std::error::Error for Rejected<T> {}

/// Writes `s`, escaping control characters (`\n`, `\t`, `\u{1b}`, ...) so a
/// rendering always stays on one line and cannot carry terminal escapes.
fn write_escaped(f: &mut fmt::Formatter<'_>, s: &str) -> fmt::Result {
    if !s.chars().any(char::is_control) {
        return f.write_str(s);
    }
    for c in s.chars() {
        if c.is_control() {
            write!(f, "{}", c.escape_default())?;
        } else {
            write!(f, "{c}")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNKNOWN_SCALING: Code = Code::new("DEF0001");

    fn text() -> Provenance {
        Provenance::new("defs/16BITBASE.xml", Span::line_col(42, 7))
    }

    fn binary() -> Provenance {
        Provenance::new("A2WC522N.bin", Span::byte_offset(0x1F3A0))
    }

    #[test]
    fn severity_orders_by_seriousness() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
        let worst = [Severity::Warning, Severity::Error, Severity::Info]
            .into_iter()
            .max();
        assert_eq!(worst, Some(Severity::Error));
    }

    #[test]
    fn severity_renders_lower_case() {
        assert_eq!(Severity::Info.to_string(), "info");
        assert_eq!(Severity::Warning.to_string(), "warning");
        assert_eq!(Severity::Error.to_string(), "error");
    }

    #[test]
    fn only_error_severity_is_error() {
        assert!(!Severity::Info.is_error());
        assert!(!Severity::Warning.is_error());
        assert!(Severity::Error.is_error());
    }

    #[test]
    fn code_round_trips_and_renders() {
        assert_eq!(UNKNOWN_SCALING.as_str(), "DEF0001");
        assert_eq!(UNKNOWN_SCALING.to_string(), "DEF0001");
        assert_eq!(UNKNOWN_SCALING, Code::new("DEF0001"));
        assert_ne!(UNKNOWN_SCALING, Code::new("DEF0002"));
    }

    #[test]
    fn span_renders_line_col_and_hex_offset() {
        assert_eq!(Span::line_col(1, 1).to_string(), ":1:1");
        assert_eq!(Span::byte_offset(0).to_string(), "@0x0");
        assert_eq!(Span::byte_offset(0x1F3A0).to_string(), "@0x1F3A0");
        assert_eq!(
            Span::byte_offset(u64::MAX).to_string(),
            "@0xFFFFFFFFFFFFFFFF"
        );
    }

    #[test]
    fn text_provenance_renders_file_line_col() {
        assert_eq!(text().to_string(), "defs/16BITBASE.xml:42:7");
    }

    #[test]
    fn binary_provenance_renders_offset() {
        assert_eq!(binary().to_string(), "A2WC522N.bin@0x1F3A0");
    }

    #[test]
    fn provenance_renders_layer_and_rule() {
        assert_eq!(
            text().with_layer("base").to_string(),
            "defs/16BITBASE.xml:42:7 [layer base]"
        );
        assert_eq!(
            binary().with_rule("checksum.subaru-dbw").to_string(),
            "A2WC522N.bin@0x1F3A0 [rule checksum.subaru-dbw]"
        );
        assert_eq!(
            text()
                .with_layer("user")
                .with_rule("quirk.ssm-delay")
                .to_string(),
            "defs/16BITBASE.xml:42:7 [layer user, rule quirk.ssm-delay]"
        );
    }

    #[test]
    fn provenance_accessors() {
        let p = text().with_layer("base").with_rule("r1");
        assert_eq!(p.source(), "defs/16BITBASE.xml");
        assert_eq!(p.span(), Span::LineCol { line: 42, col: 7 });
        assert_eq!(p.layer(), Some("base"));
        assert_eq!(p.rule_id(), Some("r1"));
        let bare = binary();
        assert_eq!(bare.span(), Span::ByteOffset(0x1F3A0));
        assert_eq!(bare.layer(), None);
        assert_eq!(bare.rule_id(), None);
    }

    #[test]
    fn diagnostic_renders_provenance_severity_code_message() {
        let d = Diagnostic::error(UNKNOWN_SCALING, "unknown scaling \"psi\"", text());
        assert_eq!(
            d.to_string(),
            "defs/16BITBASE.xml:42:7: error[DEF0001]: unknown scaling \"psi\""
        );
        let w = Diagnostic::warning(Code::new("ROM0003"), "table runs past image end", binary());
        assert_eq!(
            w.to_string(),
            "A2WC522N.bin@0x1F3A0: warning[ROM0003]: table runs past image end"
        );
        let i = Diagnostic::info(
            Code::new("DEF0100"),
            "deprecated attribute",
            text().with_layer("base"),
        );
        assert_eq!(
            i.to_string(),
            "defs/16BITBASE.xml:42:7 [layer base]: info[DEF0100]: deprecated attribute"
        );
    }

    #[test]
    fn diagnostic_constructors_set_severity_and_accessors() {
        let d = Diagnostic::new(Severity::Warning, UNKNOWN_SCALING, "m", text());
        assert_eq!(d.severity(), Severity::Warning);
        assert!(!d.is_error());
        assert_eq!(d.code(), UNKNOWN_SCALING);
        assert_eq!(d.message(), "m");
        assert_eq!(d.provenance(), &text());
        assert_eq!(
            Diagnostic::error(UNKNOWN_SCALING, "m", text()).severity(),
            Severity::Error
        );
        assert!(Diagnostic::error(UNKNOWN_SCALING, "m", text()).is_error());
        assert_eq!(
            Diagnostic::warning(UNKNOWN_SCALING, "m", text()).severity(),
            Severity::Warning
        );
        assert_eq!(
            Diagnostic::info(UNKNOWN_SCALING, "m", text()).severity(),
            Severity::Info
        );
    }

    #[test]
    fn untrusted_text_cannot_break_the_line() {
        let p = Provenance::new("evil\n.xml", Span::line_col(1, 2))
            .with_layer("l\r")
            .with_rule("r\u{1b}[31m");
        let d = Diagnostic::error(UNKNOWN_SCALING, "bad\nname\t\u{0}", p);
        let rendered = d.to_string();
        assert_eq!(
            rendered,
            "evil\\n.xml:1:2 [layer l\\r, rule r\\u{1b}[31m]: error[DEF0001]: bad\\nname\\t\\u{0}"
        );
        assert!(!rendered.chars().any(char::is_control));
        // Accessors return the value as constructed, unescaped.
        assert_eq!(d.message(), "bad\nname\t\u{0}");
    }

    #[test]
    fn non_ascii_text_is_kept() {
        let p = Provenance::new("défs/ß.xml", Span::line_col(3, 4));
        let d = Diagnostic::warning(UNKNOWN_SCALING, "°C → °F", p);
        assert_eq!(d.to_string(), "défs/ß.xml:3:4: warning[DEF0001]: °C → °F");
    }

    // ---- Diagnostics collector and Loaded<T> (US-RR-17-5) ----

    fn err(msg: &str) -> Diagnostic {
        Diagnostic::error(UNKNOWN_SCALING, msg, text())
    }

    fn warn(msg: &str) -> Diagnostic {
        Diagnostic::warning(Code::new("ROM0003"), msg, binary())
    }

    fn info(msg: &str) -> Diagnostic {
        Diagnostic::info(Code::new("DEF0100"), msg, text())
    }

    fn messages(diags: &[Diagnostic]) -> Vec<&str> {
        diags.iter().map(Diagnostic::message).collect()
    }

    #[test]
    fn collector_starts_empty() {
        let d = Diagnostics::new();
        assert!(d.is_empty());
        assert_eq!(d.len(), 0);
        assert!(!d.has_errors());
        assert_eq!(d.error_count(), 0);
        assert_eq!(d.worst(), None);
        assert_eq!(d, Diagnostics::default());
    }

    #[test]
    fn collector_records_in_order_with_severity_helpers() {
        let mut d = Diagnostics::new();
        d.info(Code::new("DEF0100"), "i", text());
        d.warning(Code::new("ROM0003"), "w", binary());
        d.error(UNKNOWN_SCALING, "e", text());
        d.push(warn("w2"));
        assert_eq!(d.len(), 4);
        assert!(!d.is_empty());
        assert_eq!(messages(d.as_slice()), ["i", "w", "e", "w2"]);
        let severities: Vec<Severity> = d.iter().map(Diagnostic::severity).collect();
        assert_eq!(
            severities,
            [
                Severity::Info,
                Severity::Warning,
                Severity::Error,
                Severity::Warning
            ]
        );
        assert_eq!(d.as_slice()[0], info("i"));
        assert_eq!(d.as_slice()[1].provenance(), &binary());
        assert_eq!(d.as_slice()[2], err("e"));
    }

    #[test]
    fn collector_counts_errors_and_worst() {
        let mut d = Diagnostics::new();
        d.push(info("i"));
        assert_eq!(d.worst(), Some(Severity::Info));
        assert!(!d.has_errors());
        d.push(warn("w"));
        assert_eq!(d.worst(), Some(Severity::Warning));
        assert!(!d.has_errors());
        assert_eq!(d.error_count(), 0);
        d.push(err("e1"));
        d.push(info("i2"));
        d.push(err("e2"));
        assert_eq!(d.worst(), Some(Severity::Error));
        assert!(d.has_errors());
        assert_eq!(d.error_count(), 2);
    }

    #[test]
    fn collector_conversions_and_iteration() {
        let v = vec![warn("a"), err("b")];
        let d = Diagnostics::from(v.clone());
        assert_eq!(d.as_slice(), v.as_slice());
        assert_eq!(Vec::from(d.clone()), v);
        assert_eq!(d.clone().into_vec(), v);
        let by_ref: Vec<&str> = (&d).into_iter().map(Diagnostic::message).collect();
        assert_eq!(by_ref, ["a", "b"]);
        let owned: Vec<Diagnostic> = d.into_iter().collect();
        assert_eq!(owned, v);

        let collected: Diagnostics = v.iter().cloned().collect();
        assert_eq!(collected.as_slice(), v.as_slice());

        let mut extended = Diagnostics::new();
        extended.push(info("first"));
        extended.extend(v);
        assert_eq!(messages(extended.as_slice()), ["first", "a", "b"]);
    }

    #[test]
    fn error_withholds_value_from_normal_accessors() {
        // An error anywhere in the list — first, middle or last — fails closed.
        for diags in [
            vec![err("e")],
            vec![err("e"), warn("w")],
            vec![info("i"), err("e"), warn("w")],
            vec![warn("w"), info("i"), err("e")],
        ] {
            let loaded = Loaded::new(42u32, diags.clone());
            assert!(loaded.has_errors());
            assert_eq!(loaded.value(), None);
            assert_eq!(loaded.diagnostics(), diags.as_slice());
            let rejected = loaded.into_result().err();
            let Some(rejected) = rejected else {
                unreachable!("an error diagnostic must reject the load")
            };
            assert_eq!(rejected.diagnostics(), diags.as_slice());
            assert_eq!(rejected.error_count(), 1);
        }
    }

    #[test]
    fn accept_errors_releases_value_and_diagnostics() {
        let diags = vec![warn("w"), err("e")];
        let (value, kept) = Loaded::new("partial", diags.clone()).accept_errors();
        assert_eq!(value, "partial");
        assert_eq!(kept, diags);

        let Err(rejected) = Loaded::new("partial", diags.clone()).into_result() else {
            unreachable!("an error diagnostic must reject the load")
        };
        let (value, kept) = rejected.accept_errors();
        assert_eq!(value, "partial");
        assert_eq!(kept, diags);
    }

    #[test]
    fn warnings_and_infos_do_not_withhold_value() {
        for diags in [
            vec![],
            vec![info("i")],
            vec![warn("w")],
            vec![info("i"), warn("w")],
        ] {
            let loaded = Loaded::new(vec![1u8, 2], diags.clone());
            assert!(!loaded.has_errors());
            assert_eq!(loaded.error_count(), 0);
            assert_eq!(loaded.value(), Some(&vec![1, 2]));
            assert_eq!(loaded.diagnostics(), diags.as_slice());
            let Ok((value, kept)) = loaded.clone().into_result() else {
                unreachable!("no error diagnostic, the load must succeed")
            };
            assert_eq!(value, vec![1, 2]);
            assert_eq!(kept, diags);
            // accept_errors is harmless when there are none.
            assert_eq!(loaded.accept_errors(), (vec![1, 2], diags));
        }
    }

    #[test]
    fn clean_load_has_no_diagnostics() {
        let loaded = Loaded::clean('x');
        assert!(loaded.diagnostics().is_empty());
        assert_eq!(loaded.worst(), None);
        assert_eq!(loaded.value(), Some(&'x'));
        assert_eq!(loaded, Loaded::new('x', Vec::new()));
    }

    #[test]
    fn finish_pairs_value_with_collected_diagnostics() {
        let mut d = Diagnostics::new();
        d.push(warn("w"));
        let ok = d.clone().finish(1u8);
        assert_eq!(ok.value(), Some(&1));
        assert_eq!(ok.worst(), Some(Severity::Warning));
        assert_eq!(ok.diagnostics(), d.as_slice());

        d.push(err("e"));
        let failed = d.clone().finish(1u8);
        assert_eq!(failed.value(), None);
        assert_eq!(failed.worst(), Some(Severity::Error));
        assert_eq!(failed.error_count(), 1);
        assert_eq!(failed.into_diagnostics(), d.into_vec());
    }

    #[test]
    fn into_diagnostics_preserves_everything() {
        let diags = vec![info("i"), err("e"), warn("w")];
        assert_eq!(Loaded::new((), diags.clone()).into_diagnostics(), diags);
        let Err(rejected) = Loaded::new((), diags.clone()).into_result() else {
            unreachable!("an error diagnostic must reject the load")
        };
        assert_eq!(rejected.into_diagnostics(), diags);
    }

    #[test]
    fn map_keeps_the_gate() {
        let ok = Loaded::new(2u32, vec![warn("w")]).map(|v| v * 10);
        assert_eq!(ok.value(), Some(&20));
        assert_eq!(messages(ok.diagnostics()), ["w"]);

        let failed = Loaded::new(2u32, vec![err("e")]).map(|v| v * 10);
        assert_eq!(failed.value(), None);
        assert_eq!(failed.accept_errors(), (20, vec![err("e")]));
    }

    #[test]
    fn rejected_lists_errors_and_renders_first() {
        let diags = vec![warn("w"), err("first"), info("i"), err("second")];
        let Err(rejected) = Loaded::new(0u8, diags).into_result() else {
            unreachable!("an error diagnostic must reject the load")
        };
        assert_eq!(rejected.error_count(), 2);
        let errors: Vec<&str> = rejected.errors().map(Diagnostic::message).collect();
        assert_eq!(errors, ["first", "second"]);
        assert_eq!(rejected.diagnostics().len(), 4);
        assert_eq!(
            rejected.to_string(),
            "load failed closed with 2 errors; first: \
             defs/16BITBASE.xml:42:7: error[DEF0001]: first"
        );

        let Err(single) = Loaded::new(0u8, vec![err("only")]).into_result() else {
            unreachable!("an error diagnostic must reject the load")
        };
        assert_eq!(
            single.to_string(),
            "load failed closed with 1 error; first: \
             defs/16BITBASE.xml:42:7: error[DEF0001]: only"
        );
        let as_error: &dyn std::error::Error = &single;
        assert!(as_error.source().is_none());
    }

    #[test]
    fn rejected_without_errors_renders_without_first() {
        // Unreachable through the public API (into_result only rejects on an
        // error); the rendering still must not misbehave.
        let r = Rejected {
            value: (),
            diagnostics: vec![warn("w")],
        };
        assert_eq!(r.error_count(), 0);
        assert_eq!(r.to_string(), "load failed closed with 0 errors");
    }
}
