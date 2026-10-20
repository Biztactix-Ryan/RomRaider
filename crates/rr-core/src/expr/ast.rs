//! The typed AST: an arena of [`Node`]s in post-order (US-RR-20-6).
//!
//! An [`Expr`] owns three flat vectors and nothing else:
//!
//! * the nodes, each a [`NodeKind`] plus the [`ByteSpan`] of formula text it
//!   came from. Children are referred to by [`NodeId`] (an index), and every
//!   child is stored **before** its parent, so the root is the last node and a
//!   cycle cannot be built;
//! * the argument lists of function calls, as one shared `Vec<NodeId>` that a
//!   call slices with its [`ArgList`];
//! * the parameter references ([`ParamRef`]) other than `x`, deduplicated, so
//!   a caller binds each one to a value once, by [`ParamId`], before
//!   evaluating.
//!
//! Evaluation (US-RR-20-7) therefore walks indices into vectors that already
//! exist: no allocation, no string lookup (function names are already a
//! [`Func`]), no hashing and no locking.

use core::fmt;
use core::ops::Range;

/// A range of bytes in the formula text: `start..end`, 0-based, end
/// exclusive.
///
/// Spans are byte offsets into the string given to [`parse`](super::parse).
/// A caller reporting a [`Diagnostic`](crate::diag::Diagnostic) adds them to
/// wherever that string came from (an XML attribute, a CSV cell).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ByteSpan {
    start: u32,
    end: u32,
}

impl ByteSpan {
    /// The span `start..end`. An `end` before `start` is taken as `start`,
    /// so a span is never negative.
    #[must_use]
    pub fn new(start: u32, end: u32) -> Self {
        Self {
            start,
            end: end.max(start),
        }
    }

    /// First byte of the span.
    #[must_use]
    pub const fn start(self) -> u32 {
        self.start
    }

    /// One past the last byte of the span.
    #[must_use]
    pub const fn end(self) -> u32 {
        self.end
    }

    /// Length in bytes.
    #[must_use]
    pub const fn len(self) -> u32 {
        self.end - self.start
    }

    /// `true` for a zero-length span (the end of the formula, for example).
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// The span as a `usize` range, for slicing the formula text.
    #[must_use]
    pub const fn range(self) -> Range<usize> {
        self.start as usize..self.end as usize
    }

    /// From the start of `self` to the end of `other`.
    pub(crate) fn to(self, other: Self) -> Self {
        Self::new(self.start, other.end)
    }
}

/// `start..end`.
impl fmt::Display for ByteSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

/// Index of a [`Node`] in its [`Expr`].
///
/// Only the parser makes these, and only for the expression it is building,
/// so an id from one expression means nothing in another; [`Expr::get`]
/// returns `None` for an id it does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u32);

impl NodeId {
    pub(crate) const fn new(index: u32) -> Self {
        Self(index)
    }

    /// The index into [`Expr::nodes`].
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Index of a [`ParamRef`] in [`Expr::params`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ParamId(u32);

impl ParamId {
    pub(crate) const fn new(index: u32) -> Self {
        Self(index)
    }

    /// The index into [`Expr::params`].
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// The arguments of a [`NodeKind::Call`]: a slice of [`Expr::args`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArgList {
    start: u32,
    len: u32,
}

impl ArgList {
    pub(crate) const fn new(start: u32, len: u32) -> Self {
        Self { start, len }
    }

    /// Number of arguments.
    #[must_use]
    pub const fn len(self) -> usize {
        self.len as usize
    }

    /// `true` when the call has no arguments. The parser never builds one:
    /// every [`Func`] takes at least one argument.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    const fn range(self) -> Range<usize> {
        self.start as usize..self.start as usize + self.len as usize
    }
}

/// Binding strength of an operator, weakest first.
///
/// The JEP levels follow the JEP 2.x grammar's productions (see the
/// [module docs](super#precedence)); [`BitOr`](Self::BitOr),
/// [`BitAnd`](Self::BitAnd) and [`Shift`](Self::Shift) are this dialect's
/// native bit operators, which JEP does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Precedence {
    /// `||` (JEP `OrExpression`).
    Or,
    /// `&&` (JEP `AndExpression`).
    And,
    /// `==` `!=` (JEP `EqualExpression`).
    Equality,
    /// `<` `>` `<=` `>=` (JEP `RelationalExpression`).
    Relational,
    /// `|` (native).
    BitOr,
    /// `&` (native).
    BitAnd,
    /// `<<` `>>` (native).
    Shift,
    /// `+` `-` (JEP `AdditiveExpression`).
    Additive,
    /// `*` `/` `%` (JEP `MultiplicativeExpression`).
    Multiplicative,
    /// Prefix `-` and `!` (JEP `UnaryExpression`).
    Unary,
    /// `^` (JEP `PowerExpression`).
    Power,
    /// Numbers, references, calls and parenthesised expressions.
    Primary,
}

impl Precedence {
    /// The next tighter level ([`Primary`](Self::Primary) stays itself).
    #[must_use]
    pub const fn tighter(self) -> Self {
        match self {
            Self::Or => Self::And,
            Self::And => Self::Equality,
            Self::Equality => Self::Relational,
            Self::Relational => Self::BitOr,
            Self::BitOr => Self::BitAnd,
            Self::BitAnd => Self::Shift,
            Self::Shift => Self::Additive,
            Self::Additive => Self::Multiplicative,
            Self::Multiplicative => Self::Unary,
            Self::Unary => Self::Power,
            Self::Power | Self::Primary => Self::Primary,
        }
    }
}

/// A prefix operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    /// `-x`: negation (JEP `UMinus`).
    Neg,
    /// `!x`: logical not, `1` when `x` is `0` and `0` otherwise (JEP `Not`).
    Not,
}

impl UnaryOp {
    /// The operator as written.
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Neg => "-",
            Self::Not => "!",
        }
    }
}

/// An infix operator.
///
/// Comparisons and logical operators give `1` for true and `0` for false, as
/// JEP 2.x does. The native bit operators work on Java `int`s, exactly as
/// `BitWise` does; the evaluator's module docs give the rules
/// ([`Expr::eval`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    /// `||`
    Or,
    /// `&&`
    And,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    Le,
    /// `>=`
    Ge,
    /// `|` (native bitwise or).
    BitOr,
    /// `&` (native bitwise and).
    BitAnd,
    /// `<<` (native shift left).
    Shl,
    /// `>>` (native arithmetic shift right).
    Shr,
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`: remainder with the sign of the dividend (JEP `Modulus`).
    Mod,
    /// `^`: power (JEP `Power`). Never exclusive-or; that is
    /// `BitWise(mask, x, 3)`.
    Pow,
}

impl BinaryOp {
    /// Every operator, in [`Precedence`] order.
    pub const ALL: [Self; 18] = [
        Self::Or,
        Self::And,
        Self::Eq,
        Self::Ne,
        Self::Lt,
        Self::Gt,
        Self::Le,
        Self::Ge,
        Self::BitOr,
        Self::BitAnd,
        Self::Shl,
        Self::Shr,
        Self::Add,
        Self::Sub,
        Self::Mul,
        Self::Div,
        Self::Mod,
        Self::Pow,
    ];

    /// The operator as written.
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Or => "||",
            Self::And => "&&",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Gt => ">",
            Self::Le => "<=",
            Self::Ge => ">=",
            Self::BitOr => "|",
            Self::BitAnd => "&",
            Self::Shl => "<<",
            Self::Shr => ">>",
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Mod => "%",
            Self::Pow => "^",
        }
    }

    /// How tightly the operator binds.
    #[must_use]
    pub const fn precedence(self) -> Precedence {
        match self {
            Self::Or => Precedence::Or,
            Self::And => Precedence::And,
            Self::Eq | Self::Ne => Precedence::Equality,
            Self::Lt | Self::Gt | Self::Le | Self::Ge => Precedence::Relational,
            Self::BitOr => Precedence::BitOr,
            Self::BitAnd => Precedence::BitAnd,
            Self::Shl | Self::Shr => Precedence::Shift,
            Self::Add | Self::Sub => Precedence::Additive,
            Self::Mul | Self::Div | Self::Mod => Precedence::Multiplicative,
            Self::Pow => Precedence::Power,
        }
    }

    /// `true` for `^`, the only right-associative operator: `2^3^2` is
    /// `2^(3^2)`. Every other operator is left-associative.
    #[must_use]
    pub const fn is_right_associative(self) -> bool {
        matches!(self, Self::Pow)
    }

    /// The minimum precedence of the right operand: one level tighter for a
    /// left-associative operator, and [`Precedence::Unary`] for `^`, whose
    /// right operand may carry a prefix operator (`2^-3`) and may itself be
    /// a power (JEP `PowerExpression ::= UnaryExpressionNotPlusMinus
    /// (<POWER> UnaryExpression)?`).
    #[must_use]
    pub const fn rhs_precedence(self) -> Precedence {
        if self.is_right_associative() {
            Precedence::Unary
        } else {
            self.precedence().tighter()
        }
    }
}

/// How many arguments a [`Func`] takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arity {
    /// Exactly this many.
    Exact(u8),
    /// From the first to the second number, inclusive.
    Between(u8, u8),
    /// This many or more.
    AtLeast(u8),
}

impl Arity {
    /// `true` when a call with `n` arguments is allowed.
    #[must_use]
    pub const fn accepts(self, n: usize) -> bool {
        match self {
            Self::Exact(k) => n == k as usize,
            Self::Between(lo, hi) => lo as usize <= n && n <= hi as usize,
            Self::AtLeast(lo) => n >= lo as usize,
        }
    }
}

/// `2`, `3 or 4`, `at least 1`.
impl fmt::Display for Arity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exact(k) => write!(f, "{k}"),
            Self::Between(lo, hi) => write!(f, "{lo} or {hi}"),
            Self::AtLeast(lo) => write!(f, "at least {lo}"),
        }
    }
}

/// A function a formula may call, resolved when the formula is parsed.
///
/// The set is JEP 2.x's `addStandardFunctions()` plus RomRaider's `BitWise`,
/// which is what legacy's scaling dialect registered
/// (`util/JEPUtil.java:49-51`; the names are the string constants of
/// `addStandardFunctions` in `org/nfunk/jep/JEP.class` of
/// `legacy/lib/common/jep.jar`). Two standard
/// functions are refused at parse time instead ([`ParseErrorKind`]
/// `UnsupportedFunction`): `rand` (formulas are pure, SECURITY.md Formulas)
/// and `str` (no strings). The complex-number functions were never
/// registered by legacy (`addComplex()` is not called) and are unknown here
/// too. Names are case-sensitive, as in JEP.
///
/// The arities are JEP's: the `numberOfParameters` each `org/nfunk/jep/function`
/// class constructor sets in the jar (`-1` for `Sum`, whose `run` needs at
/// least one, and `If`, whose `checkNumberOfParameters` takes 3 or 4), and 3
/// for `BitWise` (`util/BitWise.java:66-68`).
///
/// [`ParseErrorKind`]: super::ParseErrorKind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Func {
    /// `sin(a)`, radians.
    Sin,
    /// `cos(a)`, radians.
    Cos,
    /// `tan(a)`, radians.
    Tan,
    /// `asin(a)`.
    Asin,
    /// `acos(a)`.
    Acos,
    /// `atan(a)`.
    Atan,
    /// `atan2(y, x)`.
    Atan2,
    /// `sinh(a)`.
    Sinh,
    /// `cosh(a)`.
    Cosh,
    /// `tanh(a)`.
    Tanh,
    /// `asinh(a)`.
    Asinh,
    /// `acosh(a)`.
    Acosh,
    /// `atanh(a)`.
    Atanh,
    /// `log(a)`: base-10 logarithm (JEP `Logarithm`).
    Log,
    /// `ln(a)`: natural logarithm.
    Ln,
    /// `exp(a)`: `e` to the power `a`.
    Exp,
    /// `pow(a, b)`: same as `a^b`.
    Pow,
    /// `sqrt(a)`.
    Sqrt,
    /// `abs(a)`.
    Abs,
    /// `mod(a, b)`: same as `a % b`.
    Mod,
    /// `sum(a, ...)`: the sum of one or more arguments.
    Sum,
    /// `if(cond, a, b)` or `if(cond, a, b, c)`: lazy. With three arguments
    /// `a` when `cond > 0`, else `b` (zero and NaN included); with four, `a`
    /// when `cond > 0`, `b` when `cond < 0`, else (zero or NaN) `c`
    /// (`org/nfunk/jep/function/If.class`, `evaluate`).
    If,
    /// `binom(n, k)`: binomial coefficient.
    Binom,
    /// `BitWise(mask, value, op)`: RomRaider's bit operation on 32-bit
    /// integers; `op` is a [`BitWiseOp`] code (`util/BitWise.java:31-38`, `:49`).
    /// Note the argument order: the mask comes first.
    BitWise,
}

impl Func {
    /// Every function.
    pub const ALL: [Self; 24] = [
        Self::Sin,
        Self::Cos,
        Self::Tan,
        Self::Asin,
        Self::Acos,
        Self::Atan,
        Self::Atan2,
        Self::Sinh,
        Self::Cosh,
        Self::Tanh,
        Self::Asinh,
        Self::Acosh,
        Self::Atanh,
        Self::Log,
        Self::Ln,
        Self::Exp,
        Self::Pow,
        Self::Sqrt,
        Self::Abs,
        Self::Mod,
        Self::Sum,
        Self::If,
        Self::Binom,
        Self::BitWise,
    ];

    /// The name a formula calls it by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sin => "sin",
            Self::Cos => "cos",
            Self::Tan => "tan",
            Self::Asin => "asin",
            Self::Acos => "acos",
            Self::Atan => "atan",
            Self::Atan2 => "atan2",
            Self::Sinh => "sinh",
            Self::Cosh => "cosh",
            Self::Tanh => "tanh",
            Self::Asinh => "asinh",
            Self::Acosh => "acosh",
            Self::Atanh => "atanh",
            Self::Log => "log",
            Self::Ln => "ln",
            Self::Exp => "exp",
            Self::Pow => "pow",
            Self::Sqrt => "sqrt",
            Self::Abs => "abs",
            Self::Mod => "mod",
            Self::Sum => "sum",
            Self::If => "if",
            Self::Binom => "binom",
            Self::BitWise => "BitWise",
        }
    }

    /// The function called `name`, if there is one (case-sensitive).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.name() == name)
    }

    /// How many arguments it takes.
    #[must_use]
    pub const fn arity(self) -> Arity {
        match self {
            Self::Atan2 | Self::Pow | Self::Mod | Self::Binom => Arity::Exact(2),
            Self::Sum => Arity::AtLeast(1),
            Self::If => Arity::Between(3, 4),
            Self::BitWise => Arity::Exact(3),
            Self::Sin
            | Self::Cos
            | Self::Tan
            | Self::Asin
            | Self::Acos
            | Self::Atan
            | Self::Sinh
            | Self::Cosh
            | Self::Tanh
            | Self::Asinh
            | Self::Acosh
            | Self::Atanh
            | Self::Log
            | Self::Ln
            | Self::Exp
            | Self::Sqrt
            | Self::Abs => Arity::Exact(1),
        }
    }
}

impl fmt::Display for Func {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The operation selected by the third argument of `BitWise(mask, value, op)`
/// (`util/BitWise.java:31-38`, `:87-101`).
///
/// When `op` is a number literal the parser checks it names one of these;
/// legacy silently returned `0` for any other code (`util/BitWise.java:86`,
/// `:102-105`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BitWiseOp {
    /// `1`: `value & mask`.
    And,
    /// `2`: `value | mask`.
    Or,
    /// `3`: `value ^ mask` (exclusive or).
    Xor,
    /// `4`: `value << mask`.
    Shl,
    /// `5`: `value >> mask`, sign-extending.
    Shr,
    /// `6`: `value >>> mask`, zero-filling.
    Ushr,
    /// `7`: `!value` (complement); the mask is ignored.
    Not,
}

impl BitWiseOp {
    /// The operation with code `code` (1 to 7).
    #[must_use]
    pub const fn from_code(code: f64) -> Option<Self> {
        // Exact comparisons: only the integers 1..=7 name an operation.
        let op = if code == 1.0 {
            Self::And
        } else if code == 2.0 {
            Self::Or
        } else if code == 3.0 {
            Self::Xor
        } else if code == 4.0 {
            Self::Shl
        } else if code == 5.0 {
            Self::Shr
        } else if code == 6.0 {
            Self::Ushr
        } else if code == 7.0 {
            Self::Not
        } else {
            return None;
        };
        Some(op)
    }

    /// The code that selects it.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::And => 1,
            Self::Or => 2,
            Self::Xor => 3,
            Self::Shl => 4,
            Self::Shr => 5,
            Self::Ushr => 6,
            Self::Not => 7,
        }
    }
}

/// A reference to a logger parameter: `[id:unit]`, or a bare `id`.
///
/// `[P9:km/h]` asks for parameter `P9` converted to `km/h`; a bare `P12`
/// asks for `P12` in whatever unit is selected. These are the two forms a
/// derived logger parameter uses
/// (`logger/ecu/definition/EcuDerivedParameterConvertorImpl.java:107-125`). `x`, the value being
/// converted, is not a `ParamRef` but [`NodeKind::X`].
///
/// The parser does not know which parameters exist: a ROM scaling, which has
/// only `x`, should refuse any expression whose [`Expr::params`] is not
/// empty, and a derived parameter checks each id against its dependencies.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParamRef {
    id: String,
    unit: Option<String>,
}

impl ParamRef {
    pub(crate) fn new(id: &str, unit: Option<&str>) -> Self {
        Self {
            id: id.to_owned(),
            unit: unit.map(str::to_owned),
        }
    }

    /// The parameter id (`P9`).
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The requested unit (`km/h`), or `None` for a bare reference.
    #[must_use]
    pub fn unit(&self) -> Option<&str> {
        self.unit.as_deref()
    }
}

/// `[id:unit]`, or `id` for a bare reference.
impl fmt::Display for ParamRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.unit {
            Some(unit) => write!(f, "[{}:{unit}]", self.id),
            None => f.write_str(&self.id),
        }
    }
}

/// What a [`Node`] is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NodeKind {
    /// A number literal: always finite and never negative (`-1` is
    /// [`UnaryOp::Neg`] applied to `1`).
    Number(f64),
    /// `x`: the value being converted.
    X,
    /// A parameter reference, `[id:unit]` or a bare `id`.
    Param(ParamId),
    /// A prefix operator applied to `operand`.
    Unary {
        /// The operator.
        op: UnaryOp,
        /// Its operand.
        operand: NodeId,
    },
    /// An infix operator.
    Binary {
        /// The operator.
        op: BinaryOp,
        /// Left operand.
        lhs: NodeId,
        /// Right operand.
        rhs: NodeId,
    },
    /// A function call; its arity has been checked.
    Call {
        /// The function.
        func: Func,
        /// Its arguments, a slice of [`Expr::args`].
        args: ArgList,
    },
}

impl NodeKind {
    /// How tightly this node binds when printed.
    #[must_use]
    pub const fn precedence(&self) -> Precedence {
        match self {
            Self::Number(_) | Self::X | Self::Param(_) | Self::Call { .. } => Precedence::Primary,
            Self::Unary { .. } => Precedence::Unary,
            Self::Binary { op, .. } => op.precedence(),
        }
    }
}

/// One node: what it is and where it came from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Node {
    kind: NodeKind,
    span: ByteSpan,
}

impl Node {
    pub(crate) const fn new(kind: NodeKind, span: ByteSpan) -> Self {
        Self { kind, span }
    }

    /// What the node is.
    #[must_use]
    pub const fn kind(&self) -> &NodeKind {
        &self.kind
    }

    /// The formula text it was parsed from, parentheses around it included.
    #[must_use]
    pub const fn span(&self) -> ByteSpan {
        self.span
    }

    pub(crate) const fn set_span(&mut self, span: ByteSpan) {
        self.span = span;
    }
}

/// A parsed formula. Built only by [`parse`](super::parse), so it always
/// holds a well-formed tree.
///
/// `Display` prints it back as a formula with the fewest parentheses that
/// keep its meaning, which [`parse`](super::parse) reads back to the same
/// tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    nodes: Vec<Node>,
    args: Vec<NodeId>,
    params: Vec<ParamRef>,
    uses_x: bool,
    stack_depth: usize,
}

impl Expr {
    /// Assembles a parsed expression. `nodes` is non-empty and in post-order.
    pub(crate) fn from_parts(
        nodes: Vec<Node>,
        args: Vec<NodeId>,
        params: Vec<ParamRef>,
        uses_x: bool,
    ) -> Self {
        let stack_depth = stack_depth(&nodes);
        Self {
            nodes,
            args,
            params,
            uses_x,
            stack_depth,
        }
    }

    /// How many values evaluating this expression holds at once: the
    /// deepest the evaluator's value stack gets while it walks
    /// [`nodes`](Self::nodes) in order, each leaf pushing one value and each
    /// operator or call replacing its operands with one. Fixed when the
    /// formula is parsed, so [`eval`](Self::eval) knows before it starts how
    /// much room it needs (US-RR-20-7).
    #[must_use]
    pub const fn stack_depth(&self) -> usize {
        self.stack_depth
    }

    /// The root node: the last one.
    #[must_use]
    pub fn root(&self) -> NodeId {
        // `nodes` is never empty (parse fails on an empty formula), so this
        // is always a valid id; saturating keeps it total regardless.
        let last = self.nodes.len().saturating_sub(1);
        NodeId(u32::try_from(last).unwrap_or(u32::MAX))
    }

    /// Every node, children before parents; the root is last.
    #[must_use]
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// The node `id`, or `None` if this expression has no such node.
    #[must_use]
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.index())
    }

    /// Number of nodes.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// The argument ids of a call, in order (empty for an `ArgList` from
    /// another expression that does not fit this one).
    #[must_use]
    pub fn args(&self, list: ArgList) -> &[NodeId] {
        self.args.get(list.range()).unwrap_or(&[])
    }

    /// The parameter references other than `x`, each once, in order of first
    /// appearance.
    #[must_use]
    pub fn params(&self) -> &[ParamRef] {
        &self.params
    }

    /// The parameter `id`, or `None` if this expression has no such
    /// parameter.
    #[must_use]
    pub fn param(&self, id: ParamId) -> Option<&ParamRef> {
        self.params.get(id.index())
    }

    /// `true` if `x` appears in the formula.
    #[must_use]
    pub const fn uses_x(&self) -> bool {
        self.uses_x
    }

    /// Writes node `id`, in parentheses if it binds less tightly than `min`.
    fn write_node(&self, f: &mut fmt::Formatter<'_>, id: NodeId, min: Precedence) -> fmt::Result {
        let Some(node) = self.get(id) else {
            return Ok(());
        };
        let parens = node.kind.precedence() < min;
        if parens {
            f.write_str("(")?;
        }
        match node.kind {
            // Debug, not Display: `1e-7`, not `0.0000001`; both read back
            // exactly, Debug is shorter for very large and small values.
            NodeKind::Number(v) => write!(f, "{v:?}")?,
            NodeKind::X => f.write_str("x")?,
            NodeKind::Param(p) => match self.param(p) {
                Some(param) => write!(f, "{param}")?,
                None => f.write_str("?")?,
            },
            NodeKind::Unary { op, operand } => {
                f.write_str(op.symbol())?;
                self.write_node(f, operand, Precedence::Unary)?;
            }
            NodeKind::Binary { op, lhs, rhs } => {
                let lhs_min = if op.is_right_associative() {
                    op.precedence().tighter()
                } else {
                    op.precedence()
                };
                self.write_node(f, lhs, lhs_min)?;
                f.write_str(op.symbol())?;
                self.write_node(f, rhs, op.rhs_precedence())?;
            }
            NodeKind::Call { func, args } => {
                write!(f, "{func}(")?;
                for (i, &arg) in self.args(args).iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    self.write_node(f, arg, Precedence::Or)?;
                }
                f.write_str(")")?;
            }
        }
        if parens {
            f.write_str(")")?;
        }
        Ok(())
    }
}

/// The deepest the value stack gets evaluating `nodes` in order (see
/// [`Expr::stack_depth`]). Saturating arithmetic keeps it total; a
/// well-formed post-order arena never needs it.
fn stack_depth(nodes: &[Node]) -> usize {
    let mut len: usize = 0;
    let mut max: usize = 0;
    for node in nodes {
        len = stack_after(len, &node.kind);
        max = max.max(len);
    }
    max
}

/// How many values the evaluator's stack holds after a node of `kind`, when
/// it held `len` before: a leaf pushes one, an operator or call replaces its
/// operands with one. The parser applies it as it adds each node, to stop
/// at [`Limits::max_stack`](super::Limits::max_stack).
pub(crate) const fn stack_after(len: usize, kind: &NodeKind) -> usize {
    match kind {
        NodeKind::Number(_) | NodeKind::X | NodeKind::Param(_) => len.saturating_add(1),
        NodeKind::Unary { .. } => len,
        NodeKind::Binary { .. } => len.saturating_sub(1),
        NodeKind::Call { args, .. } => len.saturating_sub(args.len()).saturating_add(1),
    }
}

/// The formula with the fewest parentheses that keep its meaning, and no
/// spaces: `(x-32)/1.8`, `-2^2`, `if(x>1,x,0)`.
impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write_node(f, self.root(), Precedence::Or)
    }
}
