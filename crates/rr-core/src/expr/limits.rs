//! Parser limits (US-RR-20-8, SECURITY.md *Formulas*).
//!
//! Formulas come from definition packs, which are untrusted input: a pack
//! can hold any text in a `<scaling>` or `<conversion>` attribute. SECURITY.md
//! requires the expression engine to have "bounded recursion depth and AST
//! size", so that no formula can exhaust the stack, memory or time of the
//! program loading it. [`Limits`] are those bounds. Each is checked before
//! the work it bounds is done, so a formula over a limit costs no more to
//! refuse than a formula at it:
//!
//! * the length, before the first token is read: a 100 MB attribute is
//!   refused without being looked at;
//! * a literal's length, as soon as the literal has been scanned (scanning
//!   reads bytes and allocates nothing);
//! * the nesting depth, before descending another level, so the parser's
//!   recursion stops at the limit and never reaches the end of the stack;
//! * the node count and the evaluation stack, before each node is added, so
//!   the arena grows to at most the limit;
//! * a call's argument count, before each further argument is read.
//!
//! Every limit has its own [`ParseErrorKind`](super::ParseErrorKind) and
//! diagnostic code (`EXP0003` to `EXP0008`), with the span of the token at
//! which it was exceeded.
//!
//! # Defaults
//!
//! [`Limits::DEFAULT`], which [`parse`](super::parse) uses, leaves wide
//! headroom over the formulas of the pinned definition corpus (D-028). The
//! last column is measured by a test over `CORPUS_SAMPLE` in the parser
//! tests, which holds every corpus formula that uses more than `x`, numbers
//! and `+ - * / ( )`, and the same test checks that all of it still parses
//! with every limit cut to a quarter:
//!
//! | Limit | Default | Largest in the corpus |
//! |-------|---------|-----------------------|
//! | [`max_len`](Limits::max_len) | 4096 bytes | 71 bytes |
//! | [`max_literal_len`](Limits::max_literal_len) | 128 bytes | 13 (`.000224304213`) |
//! | [`max_nodes`](Limits::max_nodes) | 1024 | 24 |
//! | [`max_depth`](Limits::max_depth) | 128 levels | 8 |
//! | [`max_args`](Limits::max_args) | 32 | 3 (`if`, `BitWise`) |
//! | [`max_stack`](Limits::max_stack) | 32 ([`INLINE_STACK`]) | 5 |
//!
//! Together they bound what one formula can cost: at most 4 KiB of text,
//! 1024 nodes (24 bytes each), 128 levels of recursion and a 32-value
//! evaluation stack. The evaluation stack limit is [`INLINE_STACK`] by
//! design: every formula parsed with the defaults fits the inline stack, so
//! [`Expr::eval`](super::Expr::eval) never allocates for it. A caller that
//! raises [`max_stack`](Limits::max_stack) gets formulas `eval` gives a heap
//! buffer to, as its documentation says, and can use
//! [`Expr::eval_with`](super::Expr::eval_with) instead.
//!
//! [`parse_with`](super::parse_with) takes other limits, for a caller (a
//! test, a fuzz target) that wants tighter ones or a definition author who
//! needs looser ones. Only the nesting depth has a ceiling no `Limits` can
//! raise, [`MAX_DEPTH`]: that is the limit that keeps the parser's
//! recursion on the stack. The others bound memory, which grows linearly
//! with them, and time, which does too except for the table of distinct
//! parameter references, searched linearly as each is added (quadratic in
//! the node limit; 1024 nodes hold at most 512 distinct references).
//!
//! Measured (release build, one desktop core, US-RR-20-8): a 100 MB formula
//! is refused for its length in under 0.1 µs; with the length limit lifted,
//! a 100 MB run of `(` stops at the depth limit in 5 µs, a million-term
//! `1+1+...` at the node limit in 15 µs, a 1 MB literal in 0.25 ms; the
//! slowest formula within the defaults found, 512 distinct references
//! added together, parses in 0.27 ms. The allocation bounds are tested
//! in `tests/expr_eval_no_alloc.rs`.

use super::eval::INLINE_STACK;

/// The deepest nesting [`parse_with`](super::parse_with) accepts whatever
/// [`Limits::max_depth`] says: parentheses, prefix operators, `^` chains and
/// call arguments, each level counted. A hard ceiling that keeps parsing
/// off the end of the stack (each level is a few stack frames of the
/// recursive-descent parser); [`Limits::DEFAULT`] allows half of it.
pub const MAX_DEPTH: u32 = 256;

/// Bounds on the formulas [`parse_with`](super::parse_with) accepts; see the
/// [module docs](self) for what each costs and why the defaults are what
/// they are.
///
/// Start from [`Limits::DEFAULT`] and change the fields wanted:
///
/// ```
/// use rr_core::expr::{self, Limits, ParseErrorKind};
///
/// let mut limits = Limits::DEFAULT;
/// limits.max_depth = 2;
/// assert!(expr::parse_with("(x)", limits).is_ok());
/// let err = expr::parse_with("((x))", limits).unwrap_err();
/// assert_eq!(*err.kind(), ParseErrorKind::TooDeep { limit: 2 });
/// assert_eq!(err.span().range(), 2..3);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Limits {
    /// The longest formula, in bytes of UTF-8 text, whitespace included.
    pub max_len: u32,
    /// The longest single token that carries text: a number literal
    /// (`4.8828125e-4`), a name (`x`, `P12`, `BitWise`) or a whole
    /// `[id:unit]` reference, brackets included; in bytes.
    pub max_literal_len: u32,
    /// The most [`Node`](super::Node)s the formula's tree may have: one per
    /// number, `x`, reference, operator and call (parentheses and a prefix
    /// `+` make none).
    pub max_nodes: u32,
    /// The deepest nesting: each parenthesis, prefix operator, call
    /// argument and right operand of a binary operator is one level deeper
    /// than what contains it, so `a-(b-c)` is four levels deep. Capped at
    /// [`MAX_DEPTH`].
    pub max_depth: u32,
    /// The most arguments one call may have (`sum` takes any number; every
    /// other function a fixed few).
    pub max_args: u32,
    /// The most values evaluation may hold at once,
    /// [`Expr::stack_depth`](super::Expr::stack_depth).
    pub max_stack: u32,
}

impl Limits {
    /// The limits [`parse`](super::parse) uses.
    pub const DEFAULT: Self = Self {
        max_len: 4096,
        max_literal_len: 128,
        max_nodes: 1024,
        max_depth: 128,
        max_args: 32,
        // INLINE_STACK is 32, so the conversion is exact (a test checks).
        max_stack: INLINE_STACK as u32,
    };

    /// [`max_depth`](Self::max_depth), capped at [`MAX_DEPTH`]: the depth
    /// the parser enforces.
    #[must_use]
    pub fn effective_depth(self) -> u32 {
        self.max_depth.min(MAX_DEPTH)
    }
}

/// [`Limits::DEFAULT`].
impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
pub(super) mod tests;
