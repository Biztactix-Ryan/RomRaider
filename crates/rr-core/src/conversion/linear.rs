//! Recognising formulas that are linear in `x` (US-RR-24-8): [`detect`].
//! The rules and their precision are in the
//! [conversion module docs](super#linear-formulas).

use crate::expr::eval::{binary, call, unary};
use crate::expr::{BinaryOp, Expr, NodeKind, UnaryOp};

use super::Linear;

/// What a sub-formula is, as far as linearity goes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Term {
    /// Does not involve `x`; its value.
    Const(f64),
    /// `m * x + b`.
    Lin { m: f64, b: f64 },
    /// Involves `x` some other way.
    Other,
}

/// The `m` and `b` of `formula` if it is linear in `x` (see the
/// [module docs](super#linear-formulas)), with `m == 0` for a constant; `None` if it is not
/// linear, or its coefficients are not finite (`x*1e308*10`, `x+1/0`).
pub(super) fn detect(formula: &Expr) -> Option<Linear> {
    let mut terms: Vec<Term> = Vec::with_capacity(formula.node_count());
    let term = |terms: &[Term], id: crate::expr::NodeId| {
        terms.get(id.index()).copied().unwrap_or(Term::Other)
    };
    for node in formula.nodes() {
        let t = match *node.kind() {
            NodeKind::Number(v) => Term::Const(v),
            NodeKind::X => Term::Lin { m: 1.0, b: 0.0 },
            // `ExprForm::new` refuses parameters; one is never linear.
            NodeKind::Param(_) => Term::Other,
            NodeKind::Unary { op, operand } => unary_term(op, term(&terms, operand)),
            NodeKind::Binary { op, lhs, rhs } => {
                binary_term(op, term(&terms, lhs), term(&terms, rhs))
            }
            NodeKind::Call { func, args } => {
                let mut values = Vec::with_capacity(args.len());
                for &arg in formula.args(args) {
                    match term(&terms, arg) {
                        Term::Const(v) => values.push(v),
                        Term::Lin { .. } | Term::Other => break,
                    }
                }
                if values.len() == args.len() {
                    Term::Const(call(func, &values))
                } else {
                    Term::Other
                }
            }
        };
        terms.push(t);
    }
    match terms.last().copied()? {
        Term::Const(b) => Linear::new(0.0, b).ok(),
        Term::Lin { m, b } => Linear::new(m, b).ok(),
        Term::Other => None,
    }
}

fn unary_term(op: UnaryOp, a: Term) -> Term {
    match (op, a) {
        (_, Term::Const(v)) => Term::Const(unary(op, v)),
        (UnaryOp::Neg, Term::Lin { m, b }) => Term::Lin { m: -m, b: -b },
        (UnaryOp::Not, Term::Lin { .. }) | (_, Term::Other) => Term::Other,
    }
}

fn binary_term(op: BinaryOp, lhs: Term, rhs: Term) -> Term {
    use Term::{Const, Lin};
    match (op, lhs, rhs) {
        (_, Const(a), Const(c)) => Const(binary(op, a, c)),
        (BinaryOp::Add, Lin { m, b }, Const(c)) => Lin { m, b: b + c },
        (BinaryOp::Add, Const(c), Lin { m, b }) => Lin { m, b: c + b },
        (BinaryOp::Add, Lin { m, b }, Lin { m: n, b: d }) => Lin { m: m + n, b: b + d },
        (BinaryOp::Sub, Lin { m, b }, Const(c)) => Lin { m, b: b - c },
        (BinaryOp::Sub, Const(c), Lin { m, b }) => Lin { m: -m, b: c - b },
        (BinaryOp::Sub, Lin { m, b }, Lin { m: n, b: d }) => Lin { m: m - n, b: b - d },
        (BinaryOp::Mul, Lin { m, b }, Const(c)) | (BinaryOp::Mul, Const(c), Lin { m, b }) => {
            Lin { m: m * c, b: b * c }
        }
        // Division by zero makes `m` or `b` infinite or NaN, and a
        // coefficient that is not finite never becomes finite again, so
        // `detect` refuses it at the end.
        (BinaryOp::Div, Lin { m, b }, Const(c)) => Lin { m: m / c, b: b / c },
        _ => Term::Other,
    }
}

#[cfg(test)]
mod tests;
