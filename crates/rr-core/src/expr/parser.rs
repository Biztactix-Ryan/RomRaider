//! The Pratt parser (US-RR-20-6).
//!
//! One function, [`Parser::expr`], parses everything that binds at least as
//! tightly as a given [`Precedence`]: it reads an operand (a primary, or a
//! prefix operator and its operand) and then folds infix operators into it
//! while they bind tightly enough. The right operand of a left-associative
//! operator is parsed one level tighter, so `1-2-3` is `(1-2)-3`; the right
//! operand of `^` is parsed at [`Precedence::Unary`], so `2^3^2` is
//! `2^(3^2)` and `2^-3` is allowed. A prefix operator parses its operand at
//! [`Precedence::Unary`] too, which takes in a following `^` but nothing
//! weaker, so `-2^2` is `-(2^2)` and `-2*3` is `(-2)*3`. That reproduces the
//! JEP grammar's `UnaryExpression` / `PowerExpression` pair.
//!
//! Every nested operand goes through [`Parser::expr`], which counts nesting
//! and stops before descending past the depth limit (at most
//! [`MAX_DEPTH`](super::MAX_DEPTH)), so no input can exhaust the stack.
//! Every node goes through [`Parser::push`], which stops at the node and
//! evaluation-stack limits before the arena grows; [`Parser::call`] stops
//! at the argument limit before reading another argument; the lexer checks
//! literal lengths and [`parse`] the formula's length before the first
//! token (the [`limits`](super::limits) module docs).

use super::ast::{
    ArgList, BinaryOp, BitWiseOp, ByteSpan, Expr, Func, Node, NodeId, NodeKind, ParamId, ParamRef,
    Precedence, UnaryOp, stack_after,
};
use super::error::{ParseError, ParseErrorKind};
use super::lexer::{Lexer, Tok, Token};
use super::limits::Limits;

/// The infix operator a token stands for, if any.
const fn binary_op(tok: Tok) -> Option<BinaryOp> {
    let op = match tok {
        Tok::OrOr => BinaryOp::Or,
        Tok::AndAnd => BinaryOp::And,
        Tok::EqEq => BinaryOp::Eq,
        Tok::Ne => BinaryOp::Ne,
        Tok::Lt => BinaryOp::Lt,
        Tok::Gt => BinaryOp::Gt,
        Tok::Le => BinaryOp::Le,
        Tok::Ge => BinaryOp::Ge,
        Tok::Pipe => BinaryOp::BitOr,
        Tok::Amp => BinaryOp::BitAnd,
        Tok::Shl => BinaryOp::Shl,
        Tok::Shr => BinaryOp::Shr,
        Tok::Plus => BinaryOp::Add,
        Tok::Minus => BinaryOp::Sub,
        Tok::Star => BinaryOp::Mul,
        Tok::Slash => BinaryOp::Div,
        Tok::Percent => BinaryOp::Mod,
        Tok::Caret => BinaryOp::Pow,
        _ => return None,
    };
    Some(op)
}

/// JEP standard functions this dialect refuses, with the reason.
const UNSUPPORTED: [(&str, &str); 2] = [
    (
        "rand",
        "a formula must give the same result every time (SECURITY.md Formulas)",
    ),
    ("str", "formulas compute numbers only; there are no strings"),
];

struct Parser<'s> {
    src: &'s str,
    lexer: Lexer<'s>,
    /// The current token, not yet consumed.
    peek: Token,
    limits: Limits,
    /// [`Limits::effective_depth`].
    max_depth: u32,
    depth: u32,
    /// How many values evaluating `nodes` so far leaves on the stack.
    stack: usize,
    nodes: Vec<Node>,
    args: Vec<NodeId>,
    params: Vec<ParamRef>,
    uses_x: bool,
}

/// Parses `src` within `limits`; see [`parse_with`](super::parse_with).
pub(crate) fn parse(src: &str, limits: Limits) -> Result<Expr, ParseError> {
    // Before anything reads the text. `max_len` is a u32, so this also
    // keeps every offset within what a span can address.
    if src.len() > limits.max_len as usize {
        let kind = ParseErrorKind::TooLong {
            len: src.len(),
            limit: limits.max_len,
        };
        let end = u32::try_from(src.len()).unwrap_or(u32::MAX);
        return Err(ParseError::new(kind, ByteSpan::new(0, end)));
    }
    let mut lexer = Lexer::new(src, limits.max_literal_len);
    let peek = lexer.next_token()?;
    if peek.tok == Tok::Eof {
        return Err(ParseError::new(ParseErrorKind::Empty, peek.span));
    }
    let mut p = Parser {
        src,
        lexer,
        peek,
        limits,
        max_depth: limits.effective_depth(),
        depth: 0,
        stack: 0,
        nodes: Vec::new(),
        args: Vec::new(),
        params: Vec::new(),
        uses_x: false,
    };
    p.expr(Precedence::Or)?;
    match p.peek.tok {
        Tok::Eof => Ok(Expr::from_parts(p.nodes, p.args, p.params, p.uses_x)),
        Tok::RParen => Err(p.error_here(ParseErrorKind::UnmatchedClose)),
        other => Err(p.after_operand(ParseErrorKind::ExpectedOperator {
            found: other.describe(),
        })),
    }
}

impl<'s> Parser<'s> {
    fn error_here(&self, kind: ParseErrorKind) -> ParseError {
        ParseError::new(kind, self.peek.span)
    }

    /// The error for an unexpected current token right after a complete
    /// operand: [`ParseErrorKind::MissingOperator`] if it starts another
    /// operand, else `otherwise`. (It is never an infix operator: `expr`
    /// would have taken it.)
    fn after_operand(&self, otherwise: ParseErrorKind) -> ParseError {
        if self.peek.tok.starts_operand() {
            self.error_here(ParseErrorKind::MissingOperator)
        } else {
            self.error_here(otherwise)
        }
    }

    /// Consumes the current token and returns it.
    fn bump(&mut self) -> Result<Token, ParseError> {
        let next = self.lexer.next_token()?;
        Ok(core::mem::replace(&mut self.peek, next))
    }

    fn text(&self, span: ByteSpan) -> &'s str {
        self.src.get(span.range()).unwrap_or("")
    }

    fn span_of(&self, id: NodeId) -> ByteSpan {
        self.nodes
            .get(id.index())
            .map_or(self.peek.span, Node::span)
    }

    /// Sets node `id`'s span to `span`, which contains it.
    fn widen(&mut self, id: NodeId, span: ByteSpan) {
        if let Some(node) = self.nodes.get_mut(id.index()) {
            node.set_span(span);
        }
    }

    /// Adds a node covering `span`, unless it would be one node too many or
    /// one value too many on the evaluation stack; then the error is at
    /// `at`, the token that made it (the operand, operator or function
    /// name).
    fn push(&mut self, kind: NodeKind, span: ByteSpan, at: ByteSpan) -> Result<NodeId, ParseError> {
        let index = self.nodes.len();
        if index >= self.limits.max_nodes as usize {
            let limit = self.limits.max_nodes;
            return Err(ParseError::new(ParseErrorKind::TooManyNodes { limit }, at));
        }
        let stack = stack_after(self.stack, &kind);
        if stack > self.limits.max_stack as usize {
            let limit = self.limits.max_stack;
            return Err(ParseError::new(ParseErrorKind::StackTooDeep { limit }, at));
        }
        self.stack = stack;
        // Below `max_nodes`, a u32.
        let id = NodeId::new(u32::try_from(index).unwrap_or(u32::MAX));
        self.nodes.push(Node::new(kind, span));
        Ok(id)
    }

    /// Everything binding at least as tightly as `min`.
    fn expr(&mut self, min: Precedence) -> Result<NodeId, ParseError> {
        // Checked before descending: the recursion never goes past it.
        if self.depth >= self.max_depth {
            let limit = self.max_depth;
            return Err(self.error_here(ParseErrorKind::TooDeep { limit }));
        }
        self.depth += 1;
        let mut lhs = self.operand()?;
        while let Some(op) = binary_op(self.peek.tok) {
            if op.precedence() < min {
                break;
            }
            let op_token = self.bump()?;
            let rhs = self.expr(op.rhs_precedence())?;
            let span = self.span_of(lhs).to(self.span_of(rhs));
            lhs = self.push(NodeKind::Binary { op, lhs, rhs }, span, op_token.span)?;
        }
        self.depth -= 1;
        Ok(lhs)
    }

    /// An operand: a primary, or a prefix operator and its operand.
    fn operand(&mut self) -> Result<NodeId, ParseError> {
        let token = self.peek;
        match token.tok {
            Tok::Num(v) => {
                self.bump()?;
                self.push(NodeKind::Number(v), token.span, token.span)
            }
            Tok::Ident => self.name(),
            Tok::Ref { id, unit } => {
                self.bump()?;
                let param = self.param(id, Some(unit));
                self.push(NodeKind::Param(param), token.span, token.span)
            }
            Tok::LParen => {
                self.bump()?;
                let inner = self.expr(Precedence::Or)?;
                match self.peek.tok {
                    Tok::RParen => {}
                    Tok::Eof => {
                        return Err(ParseError::new(ParseErrorKind::UnclosedParen, token.span));
                    }
                    other => {
                        return Err(self.after_operand(ParseErrorKind::ExpectedClose {
                            found: other.describe(),
                        }));
                    }
                }
                let close = self.bump()?;
                self.widen(inner, token.span.to(close.span));
                Ok(inner)
            }
            Tok::Plus => {
                // Unary plus makes no node, as in JEP (the grammar's
                // `<PLUS> UnaryExpression` builds none); the operand just
                // grows to cover the `+`.
                self.bump()?;
                let operand = self.expr(Precedence::Unary)?;
                self.widen(operand, token.span.to(self.span_of(operand)));
                Ok(operand)
            }
            Tok::Minus | Tok::Bang => {
                self.bump()?;
                let operand = self.expr(Precedence::Unary)?;
                let span = token.span.to(self.span_of(operand));
                let op = if token.tok == Tok::Minus {
                    UnaryOp::Neg
                } else {
                    UnaryOp::Not
                };
                self.push(NodeKind::Unary { op, operand }, span, token.span)
            }
            other => Err(self.error_here(ParseErrorKind::ExpectedOperand {
                found: other.describe(),
            })),
        }
    }

    /// The parameter `id` (with `unit`), added to the table on first use.
    fn param(&mut self, id: ByteSpan, unit: Option<ByteSpan>) -> ParamId {
        let id = self.text(id);
        let unit = unit.map(|u| self.text(u));
        let found = self
            .params
            .iter()
            .position(|p| p.id() == id && p.unit() == unit);
        let index = found.unwrap_or_else(|| {
            let param = ParamRef::new(id, unit);
            self.params.push(param);
            self.params.len() - 1
        });
        ParamId::new(u32::try_from(index).unwrap_or(u32::MAX))
    }

    /// A name: `x`, a bare parameter reference, or a function call.
    fn name(&mut self) -> Result<NodeId, ParseError> {
        let token = self.bump()?;
        let name = self.text(token.span);
        let func = Func::from_name(name);
        if self.peek.tok == Tok::LParen {
            return match func {
                Some(func) => self.call(func, token.span),
                None => Err(ParseError::new(self.unknown_function(name), token.span)),
            };
        }
        if let Some(func) = func {
            return Err(ParseError::new(ParseErrorKind::NotCalled(func), token.span));
        }
        if name == "x" {
            self.uses_x = true;
            return self.push(NodeKind::X, token.span, token.span);
        }
        let param = self.param(token.span, None);
        self.push(NodeKind::Param(param), token.span, token.span)
    }

    fn unknown_function(&self, name: &str) -> ParseErrorKind {
        match UNSUPPORTED.iter().find(|(n, _)| *n == name) {
            Some(&(name, reason)) => ParseErrorKind::UnsupportedFunction { name, reason },
            None => ParseErrorKind::UnknownFunction(name.to_owned()),
        }
    }

    /// The argument list of `func`, whose name is at `name`; the current
    /// token is the `(`.
    fn call(&mut self, func: Func, name: ByteSpan) -> Result<NodeId, ParseError> {
        let open = self.bump()?;
        let mut args = Vec::new();
        if self.peek.tok != Tok::RParen {
            loop {
                // Before reading one argument too many.
                if args.len() >= self.limits.max_args as usize {
                    let kind = ParseErrorKind::TooManyArgs {
                        func,
                        limit: self.limits.max_args,
                    };
                    return Err(self.error_here(kind));
                }
                args.push(self.expr(Precedence::Or)?);
                match self.peek.tok {
                    Tok::Comma => {
                        self.bump()?;
                    }
                    Tok::RParen => break,
                    Tok::Eof => {
                        return Err(ParseError::new(ParseErrorKind::UnclosedParen, open.span));
                    }
                    // Infix operators were taken by `expr`, so what is left
                    // here starts another operand.
                    _ => return Err(self.error_here(ParseErrorKind::MissingOperator)),
                }
            }
        }
        let close = self.bump()?;
        let span = name.to(close.span);
        if !func.arity().accepts(args.len()) {
            let kind = ParseErrorKind::Arity {
                func,
                found: args.len(),
            };
            return Err(ParseError::new(kind, span));
        }
        if func == Func::BitWise {
            self.check_bitwise_op(&args)?;
        }
        let start = u32::try_from(self.args.len()).unwrap_or(u32::MAX);
        let len = u32::try_from(args.len()).unwrap_or(u32::MAX);
        self.args.extend_from_slice(&args);
        let args = ArgList::new(start, len);
        self.push(NodeKind::Call { func, args }, span, name)
    }

    /// A literal `op` in `BitWise(mask, value, op)` must name an operation.
    fn check_bitwise_op(&self, args: &[NodeId]) -> Result<(), ParseError> {
        let Some(node) = args.get(2).and_then(|&id| self.nodes.get(id.index())) else {
            return Ok(());
        };
        match node.kind() {
            NodeKind::Number(code) if BitWiseOp::from_code(*code).is_none() => {
                Err(ParseError::new(ParseErrorKind::BadBitWiseOp, node.span()))
            }
            _ => Ok(()),
        }
    }
}
