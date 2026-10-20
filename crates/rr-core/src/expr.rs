//! Expressions (concept C4a, decision D-008): formulas parsed once into a
//! typed AST.
//!
//! Definitions carry formulas as text: ROM scalings (`(x-32)/1.8`), logger
//! conversions, and derived logger parameters that combine other parameters
//! (`[P7:psi]-[P24:psi]`). Legacy evaluated them with the JEP library as
//! strings, per cell and per repaint, behind one global `synchronized`
//! cache, in two dialects: the `x` overload registered JEP's standard
//! functions and `BitWise`, the map overload used by derived parameters
//! registered neither (MF-25, `util/JEPUtil.java:46-82`). Derived parameters
//! also rewrote their formula with `String.replace` on every sample
//! (`logger/ecu/definition/EcuDerivedParameterConvertorImpl.java:53-68`).
//!
//! Here a formula is parsed once, by [`parse`], into an [`Expr`]: an arena
//! of [`Node`]s with function calls resolved to a [`Func`] and parameter
//! references collected in [`Expr::params`]. Evaluation, [`Expr::eval`],
//! walks that arena without allocating, looking up strings or taking a lock
//! (US-RR-20-7; the [`eval`] module docs give its numeric rules). There
//! is **one dialect**, the superset of legacy's two: standard functions,
//! `BitWise`, `x` and parameter references are all available everywhere. A
//! formula that worked in either legacy dialect parses here, and a caller
//! that allows fewer references checks [`Expr::params`] /
//! [`Expr::uses_x`] (a ROM scaling, for one, takes only `x`).
//!
//! Formulas come from untrusted definition packs, so parsing is bounded
//! (SECURITY.md *Formulas*): [`parse`] refuses a formula that is too long,
//! has too long a literal, too many nodes or call arguments, nests too
//! deeply or would need too large an evaluation stack, each with its own
//! error and diagnostic code, before doing the work the limit bounds. The
//! [`limits`] module gives the defaults and why; [`parse_with`] takes
//! others.
//!
//! The module holds no `static` of any kind (`cargo xtask check-globals`,
//! `STATELESS_PATHS` in `xtask/src/globals.rs`): the parser's only state is
//! the value it is building.
//!
//! # Syntax
//!
//! The dialect is the JEP 2.x syntax real definitions use, plus native bit
//! operators and `[id:unit]` references:
//!
//! * numbers: `12`, `1.`, `.3515625`, `1.0e2`, `4.8828125e-4` (no sign, no
//!   hexadecimal);
//! * `x`, the value being converted;
//! * parameter references: `[P9:km/h]` (parameter and unit) or a bare `P12`
//!   ([`ParamRef`]);
//! * calls: `if(x>32767,x-65536,x)`, `BitWise(24,x,1)`, ... ([`Func`]);
//! * operators, below. Whitespace (`' '`, tab, CR, LF) is ignored.
//!
//! JEP features no definition uses are refused with a [`ParseError`]
//! rather than half-supported: assignment (`=`), strings, `[...]` lists and
//! the vector operators `.` and `^^`, implicit multiplication (off in legacy
//! too), `;` (JEP stops at one and ignores the rest), `rand` (not pure) and
//! `str`. JEP's constants `pi` and `e` were never registered by legacy
//! (neither `util/JEPUtil.java:49-54` nor `:65-70` calls
//! `addStandardConstants()`); here they read as bare parameter references,
//! like any other name.
//!
//! # Precedence
//!
//! Weakest first. Every operator is left-associative except `^`.
//!
//! | Level | Operators | Source |
//! |-------|-----------|--------|
//! | 1 | `\|\|` | JEP `OrExpression` |
//! | 2 | `&&` | JEP `AndExpression` |
//! | 3 | `==` `!=` | JEP `EqualExpression` |
//! | 4 | `<` `>` `<=` `>=` | JEP `RelationalExpression` |
//! | 5 | `\|` | native |
//! | 6 | `&` | native |
//! | 7 | `<<` `>>` | native |
//! | 8 | `+` `-` | JEP `AdditiveExpression` |
//! | 9 | `*` `/` `%` | JEP `MultiplicativeExpression` |
//! | 10 | prefix `-` `+` `!` | JEP `UnaryExpression` |
//! | 11 | `^`, right-associative | JEP `PowerExpression` |
//!
//! The JEP levels are the productions of JEP's grammar, "low precedence
//! operators at the top, high precedence operators at the bottom" (JEP 2.x
//! documentation, *Grammar*, `singularsys.com/jep/doc/html/grammar.html`,
//! archived at `web.archive.org/web/20080117103812/http://www.singularsys.com/jep/doc/html/grammar.html`).
//! The same production names are in the constant pool of the parser
//! legacy ships (`org/nfunk/jep/Parser.class` in `legacy/lib/common/jep.jar`),
//! so the grammar is the one legacy ran. Its `^` rule is
//! `PowerExpression ::= UnaryExpressionNotPlusMinus (<POWER> UnaryExpression)?`
//! and its prefix rule `UnaryExpression ::= (<MINUS> UnaryExpression) | ... |
//! PowerExpression`, which gives:
//!
//! * `-2^2` is `-(2^2)` = -4: the prefix minus applies to the power;
//! * `2^3^2` is `2^(3^2)` = 512: `^` is right-associative;
//! * `2^-1` is allowed and is 0.5: the right operand may carry a prefix.
//!
//! The native operators `& | << >>` are not JEP (US-RR-20 adds them; JEP's
//! `^` stays power, and exclusive-or is `BitWise(mask, x, 3)`). Shifts bind
//! just below `+ -`, as in C, Java and Rust, so `x >> 8 + 1` shifts by 9.
//! `&` and `|` bind tighter than the comparisons, as in Rust, Python and Go,
//! and unlike C, where `x & 4 == 4` means `x & (4 == 4)`: a mask-and-compare
//! here reads the way it is written. No definition in the corpus uses them,
//! so there is no legacy behaviour to match.
//!
//! # Example
//!
//! ```
//! use rr_core::expr::{self, BinaryOp, NodeKind, UnaryOp};
//!
//! let e = expr::parse("-2^2").unwrap();
//! let NodeKind::Unary { op: UnaryOp::Neg, operand } = *e.get(e.root()).unwrap().kind() else {
//!     unreachable!()
//! };
//! assert!(matches!(
//!     e.get(operand).unwrap().kind(),
//!     NodeKind::Binary { op: BinaryOp::Pow, .. }
//! ));
//!
//! let d = expr::parse("([P9:km/h]/3600)/((P12/[P58:AFR])/761)").unwrap();
//! let ids: Vec<_> = d.params().iter().map(|p| (p.id(), p.unit())).collect();
//! assert_eq!(ids, [("P9", Some("km/h")), ("P12", None), ("P58", Some("AFR"))]);
//! assert!(!d.uses_x());
//!
//! assert_eq!(e.eval(0.0, &[]), Ok(-4.0));
//! assert_eq!(d.eval(f64::NAN, &[90.0, 12.0, 14.7]), Ok((90.0 / 3600.0) / ((12.0 / 14.7) / 761.0)));
//!
//! let err = expr::parse("2x").unwrap_err();
//! assert_eq!(err.span().range(), 1..2);
//! assert_eq!(err.code().as_str(), "EXP0001");
//! ```

mod ast;
mod error;
pub mod eval;
mod lexer;
pub mod limits;
mod parser;

pub use ast::{
    ArgList, Arity, BinaryOp, BitWiseOp, ByteSpan, Expr, Func, Node, NodeId, NodeKind, ParamId,
    ParamRef, Precedence, UnaryOp,
};
pub use error::{ParseError, ParseErrorKind, RefProblem};
pub use eval::{EvalError, EvalStack, INLINE_STACK};
pub use limits::{Limits, MAX_DEPTH};

/// Parses a formula within [`Limits::DEFAULT`].
///
/// # Errors
///
/// A [`ParseError`] with the [`ParseErrorKind`] and the byte span of the
/// formula text at fault.
pub fn parse(src: &str) -> Result<Expr, ParseError> {
    parser::parse(src, Limits::DEFAULT)
}

/// Parses a formula within `limits` (the [`limits`] module docs say what
/// each bounds).
///
/// # Errors
///
/// As [`parse`]; a formula over a limit gets that limit's
/// [`ParseErrorKind`] (`TooLong`, `LiteralTooLong`, `TooManyNodes`,
/// `TooDeep`, `TooManyArgs`, `StackTooDeep`).
pub fn parse_with(src: &str, limits: Limits) -> Result<Expr, ParseError> {
    parser::parse(src, limits)
}

#[cfg(test)]
mod tests;
