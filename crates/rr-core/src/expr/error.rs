//! Parse errors (US-RR-20-6).

use core::fmt;

use super::ast::{Arity, ByteSpan, Func};
use crate::diag::{Code, Diagnostic, Provenance, codes};

/// Why a formula did not parse, and where.
///
/// [`code`](Self::code) is the registry code it is reported with and
/// [`to_diagnostic`](Self::to_diagnostic) makes the
/// [`Error`](crate::diag::Severity::Error) diagnostic. `Display` is the
/// message alone; the location is [`span`](Self::span), a byte range of the
/// formula text, which the caller maps into its own source.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParseError {
    kind: ParseErrorKind,
    span: ByteSpan,
}

impl ParseError {
    pub(crate) const fn new(kind: ParseErrorKind, span: ByteSpan) -> Self {
        Self { kind, span }
    }

    /// What went wrong.
    #[must_use]
    pub const fn kind(&self) -> &ParseErrorKind {
        &self.kind
    }

    /// The formula text at fault.
    #[must_use]
    pub const fn span(&self) -> ByteSpan {
        self.span
    }

    /// The registry code this error is reported with.
    #[must_use]
    pub const fn code(&self) -> Code {
        self.kind.code()
    }

    /// This error as an [`Error`](crate::diag::Severity::Error) diagnostic at
    /// `provenance`, with [`code`](Self::code) and the `Display` message.
    #[must_use]
    pub fn to_diagnostic(&self, provenance: Provenance) -> Diagnostic {
        Diagnostic::error(self.code(), self.to_string(), provenance)
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl core::error::Error for ParseError {}

/// What is wrong with a `[id:unit]` parameter reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RefProblem {
    /// No closing `]`.
    Unterminated,
    /// No `:unit` part, as in `[P8]`.
    MissingUnit,
    /// Nothing between `[` and `:`.
    EmptyId,
    /// The id holds a character other than an ASCII letter, digit or `_`.
    BadIdChar(char),
    /// Nothing between `:` and `]`.
    EmptyUnit,
    /// The unit holds a control character or `[`.
    BadUnitChar(char),
}

impl fmt::Display for RefProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unterminated => f.write_str("it has no closing `]`"),
            Self::MissingUnit => f.write_str("it has no unit; write `[id:unit]`"),
            Self::EmptyId => f.write_str("its parameter id is empty"),
            Self::BadIdChar(c) => write!(
                f,
                "its parameter id contains {:?}; ids are ASCII letters, digits and `_`",
                c
            ),
            Self::EmptyUnit => f.write_str("its unit is empty"),
            Self::BadUnitChar(c) => write!(f, "its unit contains {c:?}"),
        }
    }
}

/// The kinds of [`ParseError`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParseErrorKind {
    /// The formula is empty or only whitespace.
    Empty,
    /// A character that starts no token (`;`, `"`, `$`, a lone `.`, a
    /// non-ASCII letter outside a `[id:unit]` reference, ...).
    UnexpectedChar(char),
    /// A lone `=`: JEP's assignment, which formulas do not use.
    Assignment,
    /// A number literal too large to be a finite `f64` (`1e999`). JEP
    /// would have read it as infinity.
    NumberOutOfRange,
    /// A malformed `[id:unit]` reference.
    BadRef(RefProblem),
    /// An operand (number, `x`, reference, call, `(` or prefix operator) was
    /// expected; `found` describes what was there instead.
    ExpectedOperand {
        /// What was found, e.g. "`)`" or "end of formula".
        found: &'static str,
    },
    /// Two operands with no operator between them (`2x`, `2(x+1)`, `x y`).
    /// JEP's implicit multiplication is off in legacy (it is off by default:
    /// the `<init>()V` constructor in `org/nfunk/jep/JEP.class` stores `false`
    /// to `implicitMul`; and `util/JEPUtil.java:46-82` never calls
    /// `setImplicitMul`), so this is an error there too.
    MissingOperator,
    /// After a complete formula, something that neither continues it nor
    /// ends it (a `,` outside a call).
    ExpectedOperator {
        /// What was found.
        found: &'static str,
    },
    /// Inside parentheses, something other than `)` after the expression (a
    /// `,` outside a call).
    ExpectedClose {
        /// What was found.
        found: &'static str,
    },
    /// A `)` with no matching `(`.
    UnmatchedClose,
    /// A `(` that is never closed.
    UnclosedParen,
    /// A call to a function that does not exist (names are case-sensitive).
    UnknownFunction(String),
    /// A call to a JEP function this dialect refuses.
    UnsupportedFunction {
        /// The function's name.
        name: &'static str,
        /// Why it is refused.
        reason: &'static str,
    },
    /// A function's name used without an argument list (`sqrt + 1`).
    NotCalled(Func),
    /// A call with the wrong number of arguments.
    Arity {
        /// The function.
        func: Func,
        /// How many arguments it was given.
        found: usize,
    },
    /// `BitWise(mask, value, op)` with a literal `op` that is not one of the
    /// codes 1 to 7 ([`BitWiseOp`](super::BitWiseOp)). Legacy returned `0`
    /// for such a call (`util/BitWise.java:86`, `:102-105`).
    BadBitWiseOp,
    /// The formula is longer than [`Limits::max_len`](super::Limits::max_len)
    /// bytes. Checked before anything else, so it costs nothing to refuse;
    /// the span is the whole formula.
    TooLong {
        /// The formula's length, in bytes.
        len: usize,
        /// The limit.
        limit: u32,
    },
    /// A number, name or `[id:unit]` reference longer than
    /// [`Limits::max_literal_len`](super::Limits::max_literal_len) bytes.
    LiteralTooLong {
        /// What kind of token it is ("a number", "a name", "a parameter
        /// reference").
        token: &'static str,
        /// Its length, in bytes.
        len: usize,
        /// The limit.
        limit: u32,
    },
    /// More than [`Limits::max_nodes`](super::Limits::max_nodes) nodes. The
    /// span is the token (operand, operator or function name) whose node
    /// would have been one too many.
    TooManyNodes {
        /// The limit.
        limit: u32,
    },
    /// Parentheses, prefix operators, `^` chains, right operands or call
    /// arguments nested deeper than
    /// [`Limits::max_depth`](super::Limits::max_depth) (itself capped at
    /// [`MAX_DEPTH`](super::MAX_DEPTH)). The span is the first token of the
    /// operand one level too deep; parsing stops there.
    TooDeep {
        /// The limit in force.
        limit: u32,
    },
    /// A call given more than [`Limits::max_args`](super::Limits::max_args)
    /// arguments. The span is the first token of the first argument too
    /// many.
    TooManyArgs {
        /// The function.
        func: Func,
        /// The limit.
        limit: u32,
    },
    /// Evaluating the formula would hold more than
    /// [`Limits::max_stack`](super::Limits::max_stack) values at once
    /// ([`Expr::stack_depth`](super::Expr::stack_depth)). The span is the
    /// operand that would have been one value too many.
    StackTooDeep {
        /// The limit.
        limit: u32,
    },
}

impl ParseErrorKind {
    /// The registry code this kind is reported with.
    #[must_use]
    pub const fn code(&self) -> Code {
        match self {
            Self::UnknownFunction(_)
            | Self::UnsupportedFunction { .. }
            | Self::NotCalled(_)
            | Self::Arity { .. }
            | Self::BadBitWiseOp => codes::EXPR_BAD_CALL,
            Self::TooDeep { .. } => codes::EXPR_TOO_COMPLEX,
            Self::TooLong { .. } => codes::EXPR_TOO_LONG,
            Self::LiteralTooLong { .. } => codes::EXPR_LITERAL_TOO_LONG,
            Self::TooManyNodes { .. } => codes::EXPR_TOO_MANY_NODES,
            Self::TooManyArgs { .. } => codes::EXPR_TOO_MANY_ARGS,
            Self::StackTooDeep { .. } => codes::EXPR_STACK_TOO_DEEP,
            Self::Empty
            | Self::UnexpectedChar(_)
            | Self::Assignment
            | Self::NumberOutOfRange
            | Self::BadRef(_)
            | Self::ExpectedOperand { .. }
            | Self::MissingOperator
            | Self::ExpectedOperator { .. }
            | Self::ExpectedClose { .. }
            | Self::UnmatchedClose
            | Self::UnclosedParen => codes::EXPR_SYNTAX,
        }
    }
}

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the formula is empty"),
            Self::UnexpectedChar(c) => write!(f, "unexpected character {c:?}"),
            Self::Assignment => {
                f.write_str("`=` (assignment) is not supported; use `==` to compare")
            }
            Self::NumberOutOfRange => f.write_str("number literal is too large"),
            Self::BadRef(problem) => write!(f, "bad parameter reference: {problem}"),
            Self::ExpectedOperand { found } => {
                write!(
                    f,
                    "expected a number, `x`, a reference, a call or `(`, found {found}"
                )
            }
            Self::MissingOperator => f.write_str(
                "missing operator between two operands (implicit multiplication is not supported)",
            ),
            Self::ExpectedOperator { found } => {
                write!(
                    f,
                    "expected an operator or the end of the formula, found {found}"
                )
            }
            Self::ExpectedClose { found } => write!(f, "expected `)`, found {found}"),
            Self::UnmatchedClose => f.write_str("`)` has no matching `(`"),
            Self::UnclosedParen => f.write_str("`(` is never closed"),
            Self::UnknownFunction(name) => write!(f, "unknown function {name:?}"),
            Self::UnsupportedFunction { name, reason } => {
                write!(f, "function {name:?} is not supported: {reason}")
            }
            Self::NotCalled(func) => {
                let name = func.name();
                write!(f, "{name:?} is a function; call it as `{name}(...)`")
            }
            Self::Arity { func, found } => {
                let arity: Arity = func.arity();
                let plural = match arity {
                    Arity::Exact(1) | Arity::AtLeast(1) => "",
                    _ => "s",
                };
                write!(
                    f,
                    "{}() takes {arity} argument{plural}, given {found}",
                    func.name()
                )
            }
            Self::BadBitWiseOp => f.write_str(
                "the operation of BitWise(mask, value, op) must be an integer from 1 to 7",
            ),
            Self::TooLong { len, limit } => write!(
                f,
                "the formula is {len} bytes long; the limit is {limit} bytes"
            ),
            Self::LiteralTooLong { token, len, limit } => write!(
                f,
                "{token} {len} bytes long is over the limit of {limit} bytes"
            ),
            Self::TooManyNodes { limit } => write!(
                f,
                "the formula has more than {limit} numbers, names, operators and calls; \
                 the limit is {limit}"
            ),
            Self::TooDeep { limit } => write!(
                f,
                "the formula is nested more than {limit} levels deep; the limit is {limit}"
            ),
            Self::TooManyArgs { func, limit } => write!(
                f,
                "{}() is given more than {limit} arguments; the limit is {limit}",
                func.name()
            ),
            Self::StackTooDeep { limit } => write!(
                f,
                "evaluating the formula would hold more than {limit} values at once; \
                 the limit is {limit}"
            ),
        }
    }
}
