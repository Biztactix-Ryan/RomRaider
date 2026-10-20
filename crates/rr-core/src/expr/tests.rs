//! Unit and property tests for the expression parser (US-RR-20-6).
//!
//! Precedence and associativity cases cite their source:
//!
//! * **JEP grammar**: the JEP 2.x standard parser grammar (jjdoc output,
//!   `singularsys.com/jep/doc/html/grammar.html`, archived 2008-01-17), whose
//!   production names match `org/nfunk/jep/Parser.class` in
//!   `legacy/lib/common/jep.jar`.
//! * **jep.jar**: read from the class files in `legacy/lib/common/jep.jar`
//!   (constant pools and constructor bytecode; never run, D-024).
//! * **legacy**: RomRaider Java sources under `legacy/src/main/java/com/romraider`.
//! * **corpus**: formulas in the pinned definition corpora (D-028): ECU
//!   definitions from TD-D/SubaruDefs `RomRaider/` at `0b0c0c42`, logger
//!   definitions from the forum `logger_v370.zip`.
//! * **native**: this dialect's own rule (module docs, *Precedence*).

use proptest::prelude::*;

use super::*;
use crate::diag::{Provenance, Severity, Span, codes};

// ---- helpers ----

fn ok(src: &str) -> Expr {
    match parse(src) {
        Ok(e) => e,
        Err(err) => panic!("{src:?} should parse, got {err} at {}", err.span()),
    }
}

fn err(src: &str) -> ParseError {
    match parse(src) {
        Ok(e) => panic!("{src:?} should not parse, got {e}"),
        Err(err) => err,
    }
}

/// `src` fails with `kind` at bytes `range`.
#[track_caller]
fn fails(src: &str, kind: ParseErrorKind, range: core::ops::Range<usize>) {
    let e = err(src);
    assert_eq!(e.kind(), &kind, "{src:?}");
    assert_eq!(e.span().range(), range, "{src:?}: span");
}

fn node(e: &Expr, id: NodeId) -> &Node {
    e.get(id).unwrap()
}

/// The tree as an S-expression: `(op lhs rhs)`, `(neg a)`, `(name args..)`,
/// numbers in `Debug` form, `x`, and references as written.
fn sexpr(e: &Expr) -> String {
    fn go(e: &Expr, id: NodeId, out: &mut String) {
        match *node(e, id).kind() {
            NodeKind::Number(v) => out.push_str(&format!("{v:?}")),
            NodeKind::X => out.push('x'),
            NodeKind::Param(p) => out.push_str(&e.param(p).unwrap().to_string()),
            NodeKind::Unary { op, operand } => {
                out.push_str(match op {
                    UnaryOp::Neg => "(neg ",
                    UnaryOp::Not => "(not ",
                });
                go(e, operand, out);
                out.push(')');
            }
            NodeKind::Binary { op, lhs, rhs } => {
                out.push('(');
                out.push_str(op.symbol());
                out.push(' ');
                go(e, lhs, out);
                out.push(' ');
                go(e, rhs, out);
                out.push(')');
            }
            NodeKind::Call { func, args } => {
                out.push('(');
                out.push_str(func.name());
                for &a in e.args(args) {
                    out.push(' ');
                    go(e, a, out);
                }
                out.push(')');
            }
        }
    }
    let mut out = String::new();
    go(e, e.root(), &mut out);
    out
}

#[track_caller]
fn tree(src: &str, expected: &str) {
    assert_eq!(sexpr(&ok(src)), expected, "{src:?}");
}

// ---- precedence and associativity ----

#[test]
fn power_binds_tighter_than_prefix_minus() {
    // JEP grammar: UnaryExpression ::= <MINUS> UnaryExpression | ... |
    // PowerExpression, so the minus applies to the whole power: -4, not 4.
    tree("-2^2", "(neg (^ 2.0 2.0))");
    tree("-x^2", "(neg (^ x 2.0))");
    // A parenthesised operand is a primary; the minus still wraps the power.
    tree("-(2)^2", "(neg (^ 2.0 2.0))");
    tree("(-2)^2", "(^ (neg 2.0) 2.0)");
    tree("!x^2", "(not (^ x 2.0))");
}

#[test]
fn power_is_right_associative() {
    // JEP grammar: PowerExpression ::= UnaryExpressionNotPlusMinus
    // (<POWER> UnaryExpression)?, and UnaryExpression reaches
    // PowerExpression again: 2^(3^2) = 512.
    tree("2^3^2", "(^ 2.0 (^ 3.0 2.0))");
    tree("2^3^2^1", "(^ 2.0 (^ 3.0 (^ 2.0 1.0)))");
    tree("(2^3)^2", "(^ (^ 2.0 3.0) 2.0)");
}

#[test]
fn power_right_operand_may_carry_a_prefix() {
    // JEP grammar: the right operand of <POWER> is a UnaryExpression.
    tree("2^-1", "(^ 2.0 (neg 1.0))");
    tree("2^-3^2", "(^ 2.0 (neg (^ 3.0 2.0)))");
    tree("2^!x", "(^ 2.0 (not x))");
    tree("2^+3", "(^ 2.0 3.0)");
    tree("2^--3", "(^ 2.0 (neg (neg 3.0)))");
}

#[test]
fn power_binds_tighter_than_multiplication() {
    // JEP grammar: MultiplicativeExpression's operands are UnaryExpressions,
    // which contain PowerExpressions.
    tree("2*3^2", "(* 2.0 (^ 3.0 2.0))");
    tree("3^2*2", "(* (^ 3.0 2.0) 2.0)");
    tree("2^3*4^5", "(* (^ 2.0 3.0) (^ 4.0 5.0))");
}

#[test]
fn prefix_minus_binds_tighter_than_multiplication() {
    // JEP grammar: MultiplicativeExpression ::= UnaryExpression
    // ((<MUL> | <DIV> | <MOD>) UnaryExpression)*; so the operand after `*`
    // may itself start with `-`.
    tree("-x*2", "(* (neg x) 2.0)");
    tree("2*-3", "(* 2.0 (neg 3.0))");
    tree("2/-x%-1", "(% (/ 2.0 (neg x)) (neg 1.0))");
    tree("--x", "(neg (neg x))");
    tree("-!x", "(neg (not x))");
}

#[test]
fn prefix_minus_after_additive_operators() {
    // corpus: `(x--4.5e1)/3.515625e-1` and `x*3.515625e-1+-4.5e1`.
    tree("x--4.5e1", "(- x (neg 45.0))");
    tree("x*3.515625e-1+-4.5e1", "(+ (* x 0.3515625) (neg 45.0))");
    tree(
        "(-x*.000224304213)+7.350001",
        "(+ (* (neg x) 0.000224304213) 7.350001)",
    );
}

#[test]
fn unary_plus_makes_no_node() {
    // JEP grammar: `<PLUS> UnaryExpression` builds no node.
    tree("+x", "x");
    tree("-+x", "(neg x)");
    tree("1++2", "(+ 1.0 2.0)");
    let e = ok("+x");
    assert_eq!(e.node_count(), 1);
    assert_eq!(node(&e, e.root()).span().range(), 0..2);
}

#[test]
fn arithmetic_is_left_associative() {
    // JEP grammar: AdditiveExpression and MultiplicativeExpression are
    // `operand (op operand)*`, which JJTree folds left to right.
    tree("1-2-3", "(- (- 1.0 2.0) 3.0)");
    tree("1-2+3", "(+ (- 1.0 2.0) 3.0)");
    tree("8/4/2", "(/ (/ 8.0 4.0) 2.0)");
    tree("7%4%3", "(% (% 7.0 4.0) 3.0)");
    tree("8/4*2", "(* (/ 8.0 4.0) 2.0)");
    tree("8*4%3", "(% (* 8.0 4.0) 3.0)");
}

#[test]
fn multiplication_binds_tighter_than_addition() {
    // JEP grammar: AdditiveExpression's operands are
    // MultiplicativeExpressions.
    tree("1+2*3", "(+ 1.0 (* 2.0 3.0))");
    tree("1*2+3", "(+ (* 1.0 2.0) 3.0)");
    tree("1-2%3", "(- 1.0 (% 2.0 3.0))");
    tree("(x-32)/1.8", "(/ (- x 32.0) 1.8)");
    tree("(x*1.8)+32", "(+ (* x 1.8) 32.0)");
}

#[test]
fn comparisons_and_logic_follow_the_jep_levels() {
    // JEP grammar: OrExpression > AndExpression > EqualExpression >
    // RelationalExpression > AdditiveExpression, outermost first.
    tree("x+1>2*x", "(> (+ x 1.0) (* 2.0 x))");
    tree("a<b==c<d", "(== (< a b) (< c d))");
    tree("a==b&&c!=d", "(&& (== a b) (!= c d))");
    tree("a||b&&c", "(|| a (&& b c))");
    tree("a&&b||c", "(|| (&& a b) c)");
    tree("!x==0", "(== (not x) 0.0)");
    // Each level is `operand (op operand)*`: left-associative.
    tree("a<b<c", "(< (< a b) c)");
    tree("a<=b>=c", "(>= (<= a b) c)");
    tree("a==b!=c", "(!= (== a b) c)");
    tree("a||b||c", "(|| (|| a b) c)");
    tree("a&&b&&c", "(&& (&& a b) c)");
    // corpus: `if(x%256==255,0,((x%256)-128)*100/128)`.
    tree(
        "if(x%256==255,0,((x%256)-128)*100/128)",
        "(if (== (% x 256.0) 255.0) 0.0 (/ (* (- (% x 256.0) 128.0) 100.0) 128.0))",
    );
}

#[test]
fn native_bit_operators_sit_between_comparison_and_addition() {
    // native: `|` < `&` < shifts < `+ -`, all above the comparisons.
    tree("x&1==1", "(== (& x 1.0) 1.0)");
    tree("x|y<z", "(< (| x y) z)");
    tree("x|1&2", "(| x (& 1.0 2.0))");
    tree("x&1|2", "(| (& x 1.0) 2.0)");
    tree("x>>4&15", "(& (>> x 4.0) 15.0)");
    tree("x<<1+1", "(<< x (+ 1.0 1.0))");
    tree("x+1<<1", "(<< (+ x 1.0) 1.0)");
    tree("1<<2<<3", "(<< (<< 1.0 2.0) 3.0)");
    tree("x>>1>>2", "(>> (>> x 1.0) 2.0)");
    tree("x&&y|z", "(&& x (| y z))");
    tree("x|y&&z", "(&& (| x y) z)");
    tree("x&y&z", "(& (& x y) z)");
    tree("x|y|z", "(| (| x y) z)");
    tree("-x&1", "(& (neg x) 1.0)");
    tree("2^x>>1", "(>> (^ 2.0 x) 1.0)");
}

#[test]
fn precedence_table_is_pinned() {
    // The levels of the module docs' table, written out by hand.
    let expected: [(BinaryOp, u8); 18] = [
        (BinaryOp::Or, 1),
        (BinaryOp::And, 2),
        (BinaryOp::Eq, 3),
        (BinaryOp::Ne, 3),
        (BinaryOp::Lt, 4),
        (BinaryOp::Gt, 4),
        (BinaryOp::Le, 4),
        (BinaryOp::Ge, 4),
        (BinaryOp::BitOr, 5),
        (BinaryOp::BitAnd, 6),
        (BinaryOp::Shl, 7),
        (BinaryOp::Shr, 7),
        (BinaryOp::Add, 8),
        (BinaryOp::Sub, 8),
        (BinaryOp::Mul, 9),
        (BinaryOp::Div, 9),
        (BinaryOp::Mod, 9),
        (BinaryOp::Pow, 11),
    ];
    let levels = [
        Precedence::Or,
        Precedence::And,
        Precedence::Equality,
        Precedence::Relational,
        Precedence::BitOr,
        Precedence::BitAnd,
        Precedence::Shift,
        Precedence::Additive,
        Precedence::Multiplicative,
        Precedence::Unary,
        Precedence::Power,
        Precedence::Primary,
    ];
    assert_eq!(BinaryOp::ALL.len(), expected.len());
    for (i, (op, level)) in expected.into_iter().enumerate() {
        assert_eq!(BinaryOp::ALL[i], op);
        assert_eq!(op.precedence(), levels[usize::from(level) - 1], "{op:?}");
        assert_eq!(op.is_right_associative(), op == BinaryOp::Pow, "{op:?}");
    }
    // `tighter` walks the levels one by one and stops at Primary.
    for pair in levels.windows(2) {
        assert_eq!(pair[0].tighter(), pair[1]);
        assert!(pair[0] < pair[1]);
    }
    assert_eq!(Precedence::Primary.tighter(), Precedence::Primary);
    // The right operand of `^` is a unary expression; of the rest, one
    // level tighter.
    assert_eq!(BinaryOp::Pow.rhs_precedence(), Precedence::Unary);
    assert_eq!(BinaryOp::Add.rhs_precedence(), Precedence::Multiplicative);
    assert_eq!(BinaryOp::Mod.rhs_precedence(), Precedence::Unary);
}

#[test]
fn every_operator_pair_follows_the_table() {
    // For every pair of operators, `1 a 2 b 3` groups by the table:
    // the tighter operator first, equal levels left to right (except `^`).
    for a in BinaryOp::ALL {
        for b in BinaryOp::ALL {
            let src = format!("1{}2{}3", a.symbol(), b.symbol());
            let (pa, pb) = (a.precedence(), b.precedence());
            let right_first = pb > pa || (pa == pb && a.is_right_associative());
            let expected = if right_first {
                format!("({} 1.0 ({} 2.0 3.0))", a.symbol(), b.symbol())
            } else {
                format!("({} ({} 1.0 2.0) 3.0)", b.symbol(), a.symbol())
            };
            tree(&src, &expected);
        }
    }
}

#[test]
fn every_operator_has_its_symbol() {
    let symbols: Vec<&str> = BinaryOp::ALL.iter().map(|op| op.symbol()).collect();
    assert_eq!(
        symbols,
        [
            "||", "&&", "==", "!=", "<", ">", "<=", ">=", "|", "&", "<<", ">>", "+", "-", "*", "/",
            "%", "^"
        ]
    );
    assert_eq!(UnaryOp::Neg.symbol(), "-");
    assert_eq!(UnaryOp::Not.symbol(), "!");
}

#[test]
fn whitespace_is_ignored() {
    // JEP's skipped characters: " ", "\t", "\n", "\r".
    tree(" ( x -\t32 )\r\n/ 1.8 ", "(/ (- x 32.0) 1.8)");
    // Two-character operators are one token: `< <` is not `<<`.
    tree("x << 1", "(<< x 1.0)");
    fails(
        "x < < 1",
        ParseErrorKind::ExpectedOperand { found: "`<`" },
        4..5,
    );
}

// ---- numbers ----

#[test]
fn number_literal_forms() {
    // JEP token rules (INTEGER_LITERAL, FLOATING_POINT_LITERAL, EXPONENT);
    // the leading-dot and exponent forms are all in the corpus.
    for (src, value) in [
        ("12", 12.0),
        ("0012", 12.0),
        ("1.", 1.0),
        (".5", 0.5),
        (".3515625", 0.3515625),
        ("1.5e-3", 0.0015),
        ("2E4", 20000.0),
        ("1e+5", 100_000.0),
        ("1.e2", 100.0),
        (".5e1", 5.0),
        ("4.8828125e-4", 0.000_488_281_25),
        ("1.0e2", 100.0),
        ("0.1", 0.1),
        ("2147483648", 2_147_483_648.0),
    ] {
        let e = ok(src);
        assert_eq!(e.node_count(), 1, "{src}");
        let NodeKind::Number(v) = *node(&e, e.root()).kind() else {
            panic!("{src} is not a number");
        };
        assert_eq!(v.to_bits(), f64::to_bits(value), "{src}");
        assert_eq!(node(&e, e.root()).span().range(), 0..src.len(), "{src}");
    }
}

#[test]
fn number_literal_errors() {
    // Overflow to infinity is refused (JEP would read +Infinity).
    fails("1e999", ParseErrorKind::NumberOutOfRange, 0..5);
    fails("x+1e400*2", ParseErrorKind::NumberOutOfRange, 2..7);
    // An incomplete exponent is not part of the number: the `e` starts a
    // name, which JEP then rejects for want of an operator.
    fails("1e", ParseErrorKind::MissingOperator, 1..2);
    fails("1e+", ParseErrorKind::MissingOperator, 1..2);
    fails("1E-x", ParseErrorKind::MissingOperator, 1..2);
    // legacy: hex "will cause a parsing error" (util/BitWise.java:53-54).
    fails("0x18", ParseErrorKind::MissingOperator, 1..4);
    fails("1..5", ParseErrorKind::MissingOperator, 2..4);
    fails("1.5.3", ParseErrorKind::MissingOperator, 3..5);
    fails(".", ParseErrorKind::UnexpectedChar('.'), 0..1);
    fails("x.y", ParseErrorKind::UnexpectedChar('.'), 1..2);
    fails(".e5", ParseErrorKind::UnexpectedChar('.'), 0..1);
}

#[test]
fn a_huge_but_finite_literal_is_kept() {
    let e = ok("1.7976931348623157e308");
    assert_eq!(*node(&e, e.root()).kind(), NodeKind::Number(f64::MAX));
    let e = ok("5e-324");
    assert_eq!(
        *node(&e, e.root()).kind(),
        NodeKind::Number(f64::from_bits(1))
    );
}

// ---- references ----

#[test]
fn x_is_the_converted_value() {
    let e = ok("x");
    assert_eq!(*node(&e, e.root()).kind(), NodeKind::X);
    assert!(e.uses_x());
    assert!(e.params().is_empty());
    assert!(!ok("1+2").uses_x());
    // Names are case-sensitive, as in JEP: `X` is a parameter.
    let e = ok("X");
    assert!(!e.uses_x());
    assert_eq!(e.params()[0].id(), "X");
}

#[test]
fn bracket_and_bare_references() {
    // corpus: `([P9:km/h]/3600)/((P12/[P58:AFR])/761)`; legacy reads the
    // unit up to the first `]` and treats a bare id as the parameter itself
    // (logger/ecu/definition/EcuDerivedParameterConvertorImpl.java:109-114,
    // :123-124).
    let e = ok("([P9:km/h]/3600)/((P12/[P58:AFR])/761)");
    let params: Vec<(&str, Option<&str>)> = e.params().iter().map(|p| (p.id(), p.unit())).collect();
    assert_eq!(
        params,
        [("P9", Some("km/h")), ("P12", None), ("P58", Some("AFR"))]
    );
    assert!(!e.uses_x());
    tree(
        "[P229:µs]/(2777.77/(P8/60))",
        "(/ [P229:µs] (/ 2777.77 (/ P8 60.0)))",
    );
    tree("[P7:psi]-[P24:psi]", "(- [P7:psi] [P24:psi])");
    // Units may hold spaces and punctuation; ids are identifier characters.
    let e = ok("[_a1:deg C (x)] + [E33:%]");
    assert_eq!(e.params()[0].id(), "_a1");
    assert_eq!(e.params()[0].unit(), Some("deg C (x)"));
    assert_eq!(e.params()[1].unit(), Some("%"));
    assert_eq!(e.params()[0].to_string(), "[_a1:deg C (x)]");
    assert_eq!(e.params()[1].to_string(), "[E33:%]");
    // A ref token covers the brackets.
    let e = ok("[P9:km/h]");
    assert_eq!(node(&e, e.root()).span().range(), 0..9);
}

#[test]
fn references_are_deduplicated_by_id_and_unit() {
    let e = ok("[P1:a]+[P1:b]+P1+[P2:a]+[P1:a]+P1+P2");
    let params: Vec<String> = e.params().iter().map(ToString::to_string).collect();
    assert_eq!(params, ["[P1:a]", "[P1:b]", "P1", "[P2:a]", "P2"]);
    // Every node points at the right entry.
    let refs: Vec<String> = e
        .nodes()
        .iter()
        .filter_map(|n| match n.kind() {
            NodeKind::Param(p) => Some(e.param(*p).unwrap().to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(
        refs,
        ["[P1:a]", "[P1:b]", "P1", "[P2:a]", "[P1:a]", "P1", "P2"]
    );
    let ids: Vec<usize> = e
        .nodes()
        .iter()
        .filter_map(|n| match n.kind() {
            NodeKind::Param(p) => Some(p.index()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, [0, 1, 2, 3, 0, 2, 4]);
}

#[test]
fn jep_constants_are_plain_names() {
    // legacy never calls addStandardConstants() (util/JEPUtil.java:49-54,
    // :65-70).
    let e = ok("pi*e");
    let ids: Vec<&str> = e.params().iter().map(ParamRef::id).collect();
    assert_eq!(ids, ["pi", "e"]);
}

#[test]
fn malformed_references() {
    let bad = |p| ParseErrorKind::BadRef(p);
    fails("[P8]", bad(RefProblem::MissingUnit), 0..4);
    fails("[]", bad(RefProblem::EmptyId), 0..2);
    fails("[:rpm]", bad(RefProblem::EmptyId), 0..2);
    fails("[P8:", bad(RefProblem::Unterminated), 0..4);
    fails("x+[P8:rpm", bad(RefProblem::Unterminated), 2..9);
    fails("[P8", bad(RefProblem::Unterminated), 0..3);
    fails("[", bad(RefProblem::Unterminated), 0..1);
    fails("[P8:]", bad(RefProblem::EmptyUnit), 0..5);
    fails("[P 8:a]", bad(RefProblem::BadIdChar(' ')), 2..3);
    fails("[Pµ:a]", bad(RefProblem::BadIdChar('µ')), 2..4);
    fails("[P8:a\nb]", bad(RefProblem::BadUnitChar('\n')), 5..6);
    fails("[P8:a[b]]", bad(RefProblem::BadUnitChar('[')), 5..6);
    // A stray `]` is just an unexpected character.
    fails("x]", ParseErrorKind::UnexpectedChar(']'), 1..2);
}

// ---- functions ----

#[test]
fn every_function_round_trips_its_name_and_arity() {
    // jep.jar: the names JEP.addStandardFunctions() registers (JEP.class
    // constant pool) and the numberOfParameters each function class sets
    // (-1 = variable: Sum needs >= 1, If 3 or 4); legacy adds BitWise with 3
    // (util/BitWise.java:66-68).
    let expected: [(&str, Arity); 24] = [
        ("sin", Arity::Exact(1)),
        ("cos", Arity::Exact(1)),
        ("tan", Arity::Exact(1)),
        ("asin", Arity::Exact(1)),
        ("acos", Arity::Exact(1)),
        ("atan", Arity::Exact(1)),
        ("atan2", Arity::Exact(2)),
        ("sinh", Arity::Exact(1)),
        ("cosh", Arity::Exact(1)),
        ("tanh", Arity::Exact(1)),
        ("asinh", Arity::Exact(1)),
        ("acosh", Arity::Exact(1)),
        ("atanh", Arity::Exact(1)),
        ("log", Arity::Exact(1)),
        ("ln", Arity::Exact(1)),
        ("exp", Arity::Exact(1)),
        ("pow", Arity::Exact(2)),
        ("sqrt", Arity::Exact(1)),
        ("abs", Arity::Exact(1)),
        ("mod", Arity::Exact(2)),
        ("sum", Arity::AtLeast(1)),
        ("if", Arity::Between(3, 4)),
        ("binom", Arity::Exact(2)),
        ("BitWise", Arity::Exact(3)),
    ];
    assert_eq!(Func::ALL.len(), expected.len());
    for (func, (name, arity)) in Func::ALL.into_iter().zip(expected) {
        assert_eq!(func.name(), name);
        assert_eq!(func.to_string(), name);
        assert_eq!(func.arity(), arity, "{name}");
        assert_eq!(Func::from_name(name), Some(func));
    }
    assert_eq!(Func::from_name("Sin"), None);
    assert_eq!(Func::from_name("bitwise"), None);
    assert_eq!(Func::from_name(""), None);
}

#[test]
fn calls_parse_with_every_valid_arity() {
    for func in Func::ALL {
        let counts: &[usize] = match func.arity() {
            Arity::Exact(n) => &[n as usize][..],
            Arity::Between(3, 4) => &[3, 4],
            Arity::AtLeast(1) => &[1, 2, 5],
            other => panic!("untested arity {other:?}"),
        };
        for &n in counts {
            let args: Vec<String> = (1..=n).map(|i| format!("{i}")).collect();
            let src = format!("{}({})", func.name(), args.join(","));
            let e = ok(&src);
            let NodeKind::Call { func: f, args } = *node(&e, e.root()).kind() else {
                panic!("{src} is not a call");
            };
            assert_eq!(f, func);
            assert_eq!(args.len(), n);
            assert!(!args.is_empty());
            let values: Vec<f64> = e
                .args(args)
                .iter()
                .map(|&a| match *node(&e, a).kind() {
                    NodeKind::Number(v) => v,
                    _ => f64::NAN,
                })
                .collect();
            let want: Vec<f64> = (1..=n).map(|i| i as f64).collect();
            assert_eq!(values, want, "{src}");
            assert_eq!(node(&e, e.root()).span().range(), 0..src.len());
        }
    }
}

#[test]
fn calls_with_the_wrong_arity_fail_with_the_call_span() {
    let arity = |func, found| ParseErrorKind::Arity { func, found };
    fails("sin()", arity(Func::Sin, 0), 0..5);
    fails("x+sqrt(1,2)", arity(Func::Sqrt, 2), 2..11);
    fails("atan2(1)", arity(Func::Atan2, 1), 0..8);
    fails("if(1,2)", arity(Func::If, 2), 0..7);
    fails("if(1,2,3,4,5)", arity(Func::If, 5), 0..13);
    fails("sum()", arity(Func::Sum, 0), 0..5);
    fails("BitWise(1,x)", arity(Func::BitWise, 2), 0..12);
    fails("pow(2)", arity(Func::Pow, 1), 0..6);
}

#[test]
fn nested_calls_keep_their_own_arguments() {
    let e = ok("if(sin(x)>0,sum(1,max2,3),atan2(x,2))");
    assert_eq!(
        sexpr(&e),
        "(if (> (sin x) 0.0) (sum 1.0 max2 3.0) (atan2 x 2.0))"
    );
    // corpus (with the XML entity decoded).
    tree(
        "if(x<2147483647,((x+2147483648)%65536)/8192,(x%65536)/8192)",
        "(if (< x 2147483647.0) (/ (% (+ x 2147483648.0) 65536.0) 8192.0) (/ (% x 65536.0) 8192.0))",
    );
}

#[test]
fn bitwise_checks_a_literal_operation() {
    // legacy: BitWise(mask, value, op), op 1..7 (util/BitWise.java:31-38, :49,
    // :81-83); any other op silently gave 0 (:86, :102-105), which is
    // refused here.
    tree("BitWise(24,x,1)", "(BitWise 24.0 x 1.0)");
    for op in 1..=7 {
        ok(&format!("BitWise(24,x,{op})"));
    }
    fails("BitWise(24,x,0)", ParseErrorKind::BadBitWiseOp, 13..14);
    fails("BitWise(24,x,8)", ParseErrorKind::BadBitWiseOp, 13..14);
    fails("BitWise(24,x,1.5)", ParseErrorKind::BadBitWiseOp, 13..16);
    fails("BitWise(24, x, (9))", ParseErrorKind::BadBitWiseOp, 15..18);
    // A computed operation is the evaluator's to check.
    ok("BitWise(24,x,y)");
    ok("BitWise(24,x,-1)");
    // Only BitWise is checked: other three-argument calls take any literal.
    ok("if(x,1,0)");
    ok("sum(1,2,0)");
}

#[test]
fn bitwise_op_codes() {
    let all = [
        BitWiseOp::And,
        BitWiseOp::Or,
        BitWiseOp::Xor,
        BitWiseOp::Shl,
        BitWiseOp::Shr,
        BitWiseOp::Ushr,
        BitWiseOp::Not,
    ];
    for (i, op) in all.into_iter().enumerate() {
        let code = u8::try_from(i + 1).unwrap();
        assert_eq!(op.code(), code);
        assert_eq!(BitWiseOp::from_code(f64::from(code)), Some(op));
    }
    for bad in [0.0, 8.0, -1.0, 1.5, 6.999, f64::NAN, f64::INFINITY] {
        assert_eq!(BitWiseOp::from_code(bad), None, "{bad}");
    }
}

#[test]
fn unknown_and_unsupported_functions() {
    fails(
        "foo(1)",
        ParseErrorKind::UnknownFunction("foo".to_owned()),
        0..3,
    );
    // Case-sensitive, as in JEP.
    fails(
        "1+Sin(x)",
        ParseErrorKind::UnknownFunction("Sin".to_owned()),
        2..5,
    );
    // `x` is a variable, not a function.
    fails(
        "x(1)",
        ParseErrorKind::UnknownFunction("x".to_owned()),
        0..1,
    );
    // Complex-number functions were never registered by legacy.
    fails(
        "re(x)",
        ParseErrorKind::UnknownFunction("re".to_owned()),
        0..2,
    );
    let e = err("rand()");
    assert!(matches!(
        e.kind(),
        ParseErrorKind::UnsupportedFunction { name: "rand", .. }
    ));
    assert_eq!(e.span().range(), 0..4);
    let e = err("str(1)");
    assert!(matches!(
        e.kind(),
        ParseErrorKind::UnsupportedFunction { name: "str", .. }
    ));
    // A function's name without arguments.
    fails("sqrt+1", ParseErrorKind::NotCalled(Func::Sqrt), 0..4);
    fails("2*if", ParseErrorKind::NotCalled(Func::If), 2..4);
}

#[test]
fn malformed_argument_lists() {
    fails("sin(1", ParseErrorKind::UnclosedParen, 3..4);
    fails(
        "sin(1,",
        ParseErrorKind::ExpectedOperand {
            found: "the end of the formula",
        },
        6..6,
    );
    fails("sin(1 2)", ParseErrorKind::MissingOperator, 6..7);
    fails("sin(1(2))", ParseErrorKind::MissingOperator, 5..6);
    fails(
        "atan2(1,)",
        ParseErrorKind::ExpectedOperand { found: "`)`" },
        8..9,
    );
    fails(
        "atan2(,1)",
        ParseErrorKind::ExpectedOperand { found: "`,`" },
        6..7,
    );
    fails(
        "sin (",
        ParseErrorKind::ExpectedOperand {
            found: "the end of the formula",
        },
        5..5,
    );
    fails("sin (x", ParseErrorKind::UnclosedParen, 4..5);
}

// ---- syntax errors and their spans ----

#[test]
fn syntax_errors_point_at_the_offending_text() {
    fails("", ParseErrorKind::Empty, 0..0);
    fails(" \t\r\n", ParseErrorKind::Empty, 4..4);
    fails(
        "x+",
        ParseErrorKind::ExpectedOperand {
            found: "the end of the formula",
        },
        2..2,
    );
    fails("(x", ParseErrorKind::UnclosedParen, 0..1);
    fails("((x)", ParseErrorKind::UnclosedParen, 0..1);
    fails("x)", ParseErrorKind::UnmatchedClose, 1..2);
    fails("(x))", ParseErrorKind::UnmatchedClose, 3..4);
    fails("()", ParseErrorKind::ExpectedOperand { found: "`)`" }, 1..2);
    fails(
        "(1,2)",
        ParseErrorKind::ExpectedClose { found: "`,`" },
        2..3,
    );
    fails("(1 2)", ParseErrorKind::MissingOperator, 3..4);
    fails(
        "1,2",
        ParseErrorKind::ExpectedOperator { found: "`,`" },
        1..2,
    );
    fails("x=1", ParseErrorKind::Assignment, 1..2);
    fails("x;", ParseErrorKind::UnexpectedChar(';'), 1..2);
    fails("x;garbage", ParseErrorKind::UnexpectedChar(';'), 1..2);
    fails("\"a\"", ParseErrorKind::UnexpectedChar('"'), 0..1);
    fails(
        "x^^2",
        ParseErrorKind::ExpectedOperand { found: "`^`" },
        2..3,
    );
    fails("x ~ 1", ParseErrorKind::UnexpectedChar('~'), 2..3);
    fails(
        "x>>>1",
        ParseErrorKind::ExpectedOperand { found: "`>`" },
        3..4,
    );
    fails("µ", ParseErrorKind::UnexpectedChar('µ'), 0..2);
    fails("x+$", ParseErrorKind::UnexpectedChar('$'), 2..3);
    // Implicit multiplication is off (jep.jar: JEP.<init>()V stores false to
    // implicitMul; legacy never calls setImplicitMul).
    fails("2x", ParseErrorKind::MissingOperator, 1..2);
    fails("2 x", ParseErrorKind::MissingOperator, 2..3);
    fails("2(x)", ParseErrorKind::MissingOperator, 1..2);
    fails("(x)(x)", ParseErrorKind::MissingOperator, 3..4);
    fails("x y", ParseErrorKind::MissingOperator, 2..3);
    fails("x [P8:rpm]", ParseErrorKind::MissingOperator, 2..10);
    fails("x !y", ParseErrorKind::MissingOperator, 2..3);
    fails("1 .5", ParseErrorKind::MissingOperator, 2..4);
}

#[test]
fn every_token_is_named_in_expected_operand_errors() {
    // An operator where an operand belongs names the operator.
    for (src, found) in [
        (")", "`)`"),
        (",", "`,`"),
        ("*", "`*`"),
        ("/", "`/`"),
        ("%", "`%`"),
        ("^", "`^`"),
        ("&&", "`&&`"),
        ("||", "`||`"),
        ("&", "`&`"),
        ("|", "`|`"),
        ("==", "`==`"),
        ("!=", "`!=`"),
        ("<", "`<`"),
        (">", "`>`"),
        ("<=", "`<=`"),
        (">=", "`>=`"),
        ("<<", "`<<`"),
        (">>", "`>>`"),
    ] {
        fails(src, ParseErrorKind::ExpectedOperand { found }, 0..src.len());
    }
    // Prefix operators followed by nothing.
    for src in ["-", "+", "!"] {
        fails(
            src,
            ParseErrorKind::ExpectedOperand {
                found: "the end of the formula",
            },
            1..1,
        );
    }
}

#[test]
fn every_operand_token_after_an_operand_is_a_missing_operator() {
    for (src, range) in [
        ("1 2", 2..3),
        ("1 y", 2..3),
        ("1 [a:b]", 2..7),
        ("1 (2)", 2..3),
        ("1 !2", 2..3),
    ] {
        fails(src, ParseErrorKind::MissingOperator, range);
    }
    // `+` and `-` after an operand are binary operators.
    tree("1 -2", "(- 1.0 2.0)");
    tree("1 +2", "(+ 1.0 2.0)");
}

#[test]
fn token_descriptions() {
    use super::lexer::Tok;
    let unit = ByteSpan::new(0, 0);
    assert_eq!(Tok::Num(1.0).describe(), "a number");
    assert_eq!(Tok::Ident.describe(), "a name");
    assert_eq!(
        Tok::Ref { id: unit, unit }.describe(),
        "a parameter reference"
    );
    assert_eq!(Tok::LParen.describe(), "`(`");
    assert_eq!(Tok::Plus.describe(), "`+`");
    assert_eq!(Tok::Minus.describe(), "`-`");
    assert_eq!(Tok::Bang.describe(), "`!`");
    assert_eq!(Tok::Eof.describe(), "the end of the formula");
    for tok in [
        Tok::Num(0.0),
        Tok::Ident,
        Tok::Ref { id: unit, unit },
        Tok::LParen,
        Tok::Plus,
        Tok::Minus,
        Tok::Bang,
    ] {
        assert!(tok.starts_operand(), "{tok:?}");
    }
    for tok in [
        Tok::RParen,
        Tok::Comma,
        Tok::Star,
        Tok::Caret,
        Tok::Lt,
        Tok::Eof,
    ] {
        assert!(!tok.starts_operand(), "{tok:?}");
    }
}

#[test]
fn error_kinds_map_to_registry_codes() {
    let cases = [
        (ParseErrorKind::Empty, codes::EXPR_SYNTAX),
        (ParseErrorKind::UnexpectedChar(';'), codes::EXPR_SYNTAX),
        (ParseErrorKind::Assignment, codes::EXPR_SYNTAX),
        (ParseErrorKind::NumberOutOfRange, codes::EXPR_SYNTAX),
        (
            ParseErrorKind::BadRef(RefProblem::EmptyId),
            codes::EXPR_SYNTAX,
        ),
        (
            ParseErrorKind::ExpectedOperand { found: "`)`" },
            codes::EXPR_SYNTAX,
        ),
        (ParseErrorKind::MissingOperator, codes::EXPR_SYNTAX),
        (
            ParseErrorKind::ExpectedOperator { found: "`,`" },
            codes::EXPR_SYNTAX,
        ),
        (
            ParseErrorKind::ExpectedClose { found: "`,`" },
            codes::EXPR_SYNTAX,
        ),
        (ParseErrorKind::UnmatchedClose, codes::EXPR_SYNTAX),
        (ParseErrorKind::UnclosedParen, codes::EXPR_SYNTAX),
        (
            ParseErrorKind::UnknownFunction("f".into()),
            codes::EXPR_BAD_CALL,
        ),
        (
            ParseErrorKind::UnsupportedFunction {
                name: "rand",
                reason: "r",
            },
            codes::EXPR_BAD_CALL,
        ),
        (ParseErrorKind::NotCalled(Func::Sin), codes::EXPR_BAD_CALL),
        (
            ParseErrorKind::Arity {
                func: Func::Sin,
                found: 0,
            },
            codes::EXPR_BAD_CALL,
        ),
        (ParseErrorKind::BadBitWiseOp, codes::EXPR_BAD_CALL),
        (
            ParseErrorKind::TooDeep { limit: 1 },
            codes::EXPR_TOO_COMPLEX,
        ),
        (
            ParseErrorKind::TooLong { len: 2, limit: 1 },
            codes::EXPR_TOO_LONG,
        ),
        (
            ParseErrorKind::LiteralTooLong {
                token: "a name",
                len: 2,
                limit: 1,
            },
            codes::EXPR_LITERAL_TOO_LONG,
        ),
        (
            ParseErrorKind::TooManyNodes { limit: 1 },
            codes::EXPR_TOO_MANY_NODES,
        ),
        (
            ParseErrorKind::TooManyArgs {
                func: Func::Sum,
                limit: 1,
            },
            codes::EXPR_TOO_MANY_ARGS,
        ),
        (
            ParseErrorKind::StackTooDeep { limit: 1 },
            codes::EXPR_STACK_TOO_DEEP,
        ),
    ];
    for (kind, code) in cases {
        assert_eq!(kind.code(), code, "{kind:?}");
        let e = ParseError::new(kind, ByteSpan::new(1, 2));
        assert_eq!(e.code(), code);
        assert_eq!(
            codes::lookup(code).map(|i| i.severity),
            Some(Severity::Error)
        );
    }
}

#[test]
fn error_messages() {
    for (src, message) in [
        ("", "the formula is empty"),
        ("x;", "unexpected character ';'"),
        ("x\u{1b}", "unexpected character '\\u{1b}'"),
        (
            "x=1",
            "`=` (assignment) is not supported; use `==` to compare",
        ),
        ("1e999", "number literal is too large"),
        (
            "[P8]",
            "bad parameter reference: it has no unit; write `[id:unit]`",
        ),
        ("[P8:a", "bad parameter reference: it has no closing `]`"),
        ("[:a]", "bad parameter reference: its parameter id is empty"),
        (
            "[P-8:a]",
            "bad parameter reference: its parameter id contains '-'; \
             ids are ASCII letters, digits and `_`",
        ),
        ("[P8:]", "bad parameter reference: its unit is empty"),
        (
            "[P8:\t]",
            "bad parameter reference: its unit contains '\\t'",
        ),
        (
            "x+",
            "expected a number, `x`, a reference, a call or `(`, found the end of the formula",
        ),
        (
            "2x",
            "missing operator between two operands (implicit multiplication is not supported)",
        ),
        (
            "1,2",
            "expected an operator or the end of the formula, found `,`",
        ),
        ("(1,2)", "expected `)`, found `,`"),
        ("x)", "`)` has no matching `(`"),
        ("(x", "`(` is never closed"),
        ("foo(1)", "unknown function \"foo\""),
        (
            "rand()",
            "function \"rand\" is not supported: a formula must give the same result \
             every time (SECURITY.md Formulas)",
        ),
        (
            "str(1)",
            "function \"str\" is not supported: formulas compute numbers only; \
             there are no strings",
        ),
        ("sqrt", "\"sqrt\" is a function; call it as `sqrt(...)`"),
        ("sin(1,2)", "sin() takes 1 argument, given 2"),
        ("atan2(1)", "atan2() takes 2 arguments, given 1"),
        ("if(1)", "if() takes 3 or 4 arguments, given 1"),
        ("sum()", "sum() takes at least 1 argument, given 0"),
        (
            "BitWise(1,x,9)",
            "the operation of BitWise(mask, value, op) must be an integer from 1 to 7",
        ),
        (
            &"(".repeat(300),
            "the formula is nested more than 128 levels deep; the limit is 128",
        ),
        (
            &"1".repeat(200),
            "a number 200 bytes long is over the limit of 128 bytes",
        ),
        (
            &"a".repeat(129),
            "a name 129 bytes long is over the limit of 128 bytes",
        ),
        (
            &format!("[P1:{}]", "u".repeat(200)),
            "a parameter reference 205 bytes long is over the limit of 128 bytes",
        ),
        (
            &"1".repeat(5000),
            "the formula is 5000 bytes long; the limit is 4096 bytes",
        ),
        (
            &vec!["1"; 600].join("+"),
            "the formula has more than 1024 numbers, names, operators and calls; \
             the limit is 1024",
        ),
        (
            &format!("sum({})", vec!["1"; 40].join(",")),
            "sum() is given more than 32 arguments; the limit is 32",
        ),
        (
            &format!("{}1", "1+(".repeat(40)),
            "evaluating the formula would hold more than 32 values at once; \
             the limit is 32",
        ),
    ] {
        let e = err(src);
        assert_eq!(e.to_string(), message, "{src:?}");
        assert_eq!(e.kind().to_string(), message, "{src:?}");
    }
}

#[test]
fn errors_become_error_diagnostics() {
    let e = err("x+foo(1)");
    let d = e.to_diagnostic(Provenance::new("defs/base.xml", Span::line_col(3, 20)));
    assert_eq!(d.severity(), Severity::Error);
    assert_eq!(d.code(), codes::EXPR_BAD_CALL);
    assert_eq!(
        d.to_string(),
        "defs/base.xml:3:20: error[EXP0002]: unknown function \"foo\""
    );
    let as_error: &dyn core::error::Error = &e;
    assert!(as_error.source().is_none());
    assert_eq!(as_error.to_string(), "unknown function \"foo\"");
}

// ---- spans ----

#[test]
fn node_spans_cover_their_text() {
    let e = ok("(x-32)/1.8");
    let root = node(&e, e.root());
    assert_eq!(root.span().range(), 0..10);
    let NodeKind::Binary { lhs, rhs, .. } = *root.kind() else {
        panic!("not binary");
    };
    // Parentheses belong to the node inside them.
    assert_eq!(node(&e, lhs).span().range(), 0..6);
    assert_eq!(node(&e, rhs).span().range(), 7..10);
    let NodeKind::Binary {
        lhs: x, rhs: n32, ..
    } = *node(&e, lhs).kind()
    else {
        panic!("not binary");
    };
    assert_eq!(node(&e, x).span().range(), 1..2);
    assert_eq!(node(&e, n32).span().range(), 3..5);

    let e = ok("  -x  ");
    assert_eq!(node(&e, e.root()).span().range(), 2..4);
    let e = ok("sin( x ) ");
    assert_eq!(node(&e, e.root()).span().range(), 0..8);
    let e = ok("1 + [P8:rpm]");
    assert_eq!(node(&e, e.root()).span().range(), 0..12);
    let e = ok("((1))");
    assert_eq!(node(&e, e.root()).span().range(), 0..5);
}

#[test]
fn byte_span_accessors() {
    let s = ByteSpan::new(3, 7);
    assert_eq!((s.start(), s.end(), s.len()), (3, 7, 4));
    assert!(!s.is_empty());
    assert_eq!(s.range(), 3..7);
    assert_eq!(s.to_string(), "3..7");
    let empty = ByteSpan::new(5, 5);
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);
    // An end before the start is clamped.
    let clamped = ByteSpan::new(9, 2);
    assert_eq!((clamped.start(), clamped.end()), (9, 9));
    assert_eq!(
        ByteSpan::new(1, 2).to(ByteSpan::new(5, 8)),
        ByteSpan::new(1, 8)
    );
}

#[test]
fn arg_lists_and_lookups_outside_the_expression() {
    assert!(ArgList::new(0, 0).is_empty());
    assert!(!ArgList::new(0, 1).is_empty());
    assert_eq!(ArgList::new(2, 3).len(), 3);
    let e = ok("atan2(x,1)");
    // Ids and lists from elsewhere find nothing rather than panicking.
    assert!(e.get(NodeId::new(99)).is_none());
    assert!(e.param(ParamId::new(0)).is_none());
    assert!(e.args(ArgList::new(1, 5)).is_empty());
    assert_eq!(e.args(ArgList::new(1, 1)).len(), 1);
    assert_eq!(NodeId::new(4).index(), 4);
    assert_eq!(ParamId::new(2).index(), 2);
}

#[test]
fn nodes_are_in_post_order_with_the_root_last() {
    let e = ok("if(x>1,-x,sum(1,2))*[P8:rpm]");
    assert_eq!(e.root().index(), e.node_count() - 1);
    assert_eq!(e.nodes().len(), e.node_count());
    check_structure("if(x>1,-x,sum(1,2))*[P8:rpm]", &e);
}

/// Children come before parents, child spans lie inside parent spans, and
/// every span lies inside the source.
fn check_structure(src: &str, e: &Expr) {
    let len = u32::try_from(src.len()).unwrap();
    for (i, n) in e.nodes().iter().enumerate() {
        assert!(n.span().end() <= len, "{src:?}: node {i} span");
        let children: Vec<NodeId> = match *n.kind() {
            NodeKind::Unary { operand, .. } => vec![operand],
            NodeKind::Binary { lhs, rhs, .. } => vec![lhs, rhs],
            NodeKind::Call { args, .. } => e.args(args).to_vec(),
            _ => vec![],
        };
        for c in children {
            assert!(c.index() < i, "{src:?}: child {} of {i}", c.index());
            let cs = node(e, c).span();
            assert!(n.span().start() <= cs.start() && cs.end() <= n.span().end());
        }
    }
}

#[test]
fn arity_accepts_and_renders() {
    assert!(Arity::Exact(2).accepts(2));
    assert!(!Arity::Exact(2).accepts(1));
    assert!(!Arity::Exact(2).accepts(3));
    let between = Arity::Between(3, 4);
    assert!(!between.accepts(2));
    assert!(between.accepts(3));
    assert!(between.accepts(4));
    assert!(!between.accepts(5));
    assert!(!Arity::AtLeast(1).accepts(0));
    assert!(Arity::AtLeast(1).accepts(1));
    assert!(Arity::AtLeast(1).accepts(100));
    assert_eq!(Arity::Exact(2).to_string(), "2");
    assert_eq!(between.to_string(), "3 or 4");
    assert_eq!(Arity::AtLeast(1).to_string(), "at least 1");
}

#[test]
fn node_precedence() {
    let e = ok("-x+sin(1)*[a:b]");
    let prec: Vec<Precedence> = e.nodes().iter().map(|n| n.kind().precedence()).collect();
    assert_eq!(
        prec,
        [
            Precedence::Primary,        // x
            Precedence::Unary,          // -x
            Precedence::Primary,        // 1
            Precedence::Primary,        // sin(1)
            Precedence::Primary,        // [a:b]
            Precedence::Multiplicative, // sin(1)*[a:b]
            Precedence::Additive,       // root
        ]
    );
}

// ---- printing ----

#[test]
fn display_uses_the_fewest_parentheses() {
    for (src, printed) in [
        ("(x-32)/1.8", "(x-32.0)/1.8"),
        ("x-(1-2)", "x-(1.0-2.0)"),
        ("(x-1)-2", "x-1.0-2.0"),
        ("x*(y*z)", "x*(y*z)"),
        ("-(2^2)", "-2.0^2.0"),
        ("(-2)^2", "(-2.0)^2.0"),
        ("(2^3)^2", "(2.0^3.0)^2.0"),
        ("2^(3^2)", "2.0^3.0^2.0"),
        ("2^(-3)", "2.0^-3.0"),
        ("2^(3*4)", "2.0^(3.0*4.0)"),
        ("-(x*2)", "-(x*2.0)"),
        ("(-x)*2", "-x*2.0"),
        ("1+(-x)", "1.0+-x"),
        ("1-(-x)", "1.0--x"),
        ("-(-x)", "--x"),
        ("!(x==0)", "!(x==0.0)"),
        ("(!x)==0", "!x==0.0"),
        ("(x&1)==1", "x&1.0==1.0"),
        ("x&(1==1)", "x&(1.0==1.0)"),
        ("(a||b)&&c", "(a||b)&&c"),
        ("a>(b>>c)", "a>b>>c"),
        ("if( x > 1 , x , 0 )", "if(x>1.0,x,0.0)"),
        ("sum(1,2)+3", "sum(1.0,2.0)+3.0"),
        ("[P9:km/h] * P12", "[P9:km/h]*P12"),
        ("+x", "x"),
        ("1e-7", "1e-7"),
        ("100", "100.0"),
        ("1e20", "1e20"),
    ] {
        let e = ok(src);
        let shown = e.to_string();
        assert_eq!(shown, printed, "{src:?}");
        // What is printed parses back to the same tree.
        assert_eq!(sexpr(&ok(&shown)), sexpr(&e), "{src:?}");
    }
}

// ---- formulas from the definition corpus ----

/// Every formula in the pinned corpus (TD-D/SubaruDefs `RomRaider/` at
/// `0b0c0c42`, D-028) that uses more than `x`, numbers and `+ - * / ( )`,
/// with XML entities decoded, plus the most common simple ones.
pub(super) const CORPUS_SAMPLE: &[&str] = &[
    "if(65536+x/65536>32767,(65536+x/65536)/32768,x/65536/32768)",
    "if(65536+x/65536>32767,(65536+x/65536)/32768*14.64,x/65536/32768*14.64)",
    "if(x<2147483647,((x+2147483648)%65536)/8192,(x%65536)/8192)",
    "if(x<2147483647,((x+2147483648)%65536)/255-128,(x%65536)/255-128)",
    "if(x%256==255,0,((x%256)-128)*100/128)",
    "if(256+x/16777216>127,(256+x/16777216)-128,x/16777216)",
    "([P9:mph]/3600)/((P12/[P58:AFR])/3459)",
    "([P9:mph]/3600)/((P12/[P58:AFR])/2880)",
    "([P9:km/h]/3600)/((P12/[P58:AFR])/761)",
    "[P7:psi]-[P24:psi]",
    "[P7:mmHg]-[P24:mmHg]",
    "[P7:inHg]-[P24:inHg]",
    "[P7:bar]-[P24:bar]",
    "[P229:µs]/(2777.77/(P8/60))",
    "P160/[P7:hPa]",
    "P156*(835-(0.7*([P31:C]-15)))/1000",
    "(3600/[P9:km/h])*((P12/[P58:AFR])/761)*100",
    "(P200*2*8.314472*([P11:C]+273.15))/([P7:kPa]*2.5*28.97)*100",
    "(P200*2*8.314472*([P11:C]+273.15))/([P7:kPa]*2.0*28.97)*100",
    "x%256",
    "(P8*[P21:ms])/1200",
    "x",
    "(x-32)/1.8",
    "(x*1.8)+32",
    "(x*.3515625)-20",
    "(x+20)/.3515625",
    "x/.01933677",
    "(14.7/x-1)/.0078125",
    "14.7/(1+x*.0078125)",
    "-x",
    "(x-7.350001)/-.000224304213",
    "(-x*.000224304213)+7.350001",
    "96560.6/x",
    "x/4.8828125e-4",
    "(x*3.05175781e-3)-1.0e2",
    "x*1.0e-2+0.0",
    "(x--4.5e1)/3.515625e-1",
    "x*3.515625e-1+-4.5e1",
];

#[test]
fn corpus_sample_parses_and_round_trips() {
    for &src in CORPUS_SAMPLE {
        let e = ok(src);
        check_structure(src, &e);
        let again = ok(&e.to_string());
        assert_eq!(sexpr(&again), sexpr(&e), "{src:?}");
    }
    // Spot checks of the trees.
    tree(
        "(x-7.350001)/-.000224304213",
        "(/ (- x 7.350001) (neg 0.000224304213))",
    );
    tree("14.7/(1+x*.0078125)", "(/ 14.7 (+ 1.0 (* x 0.0078125)))");
    tree(
        "P156*(835-(0.7*([P31:C]-15)))/1000",
        "(/ (* P156 (- 835.0 (* 0.7 (- [P31:C] 15.0)))) 1000.0)",
    );
}

// ---- property tests ----

/// A generated tree, rendered fully parenthesised so the text has one
/// reading whatever the precedence rules are.
#[derive(Debug, Clone)]
pub(super) enum Tree {
    Num(f64),
    X,
    Param(&'static str, Option<&'static str>),
    Unary(UnaryOp, Box<Tree>),
    Binary(BinaryOp, Box<Tree>, Box<Tree>),
    Call(Func, Vec<Tree>),
}

impl Tree {
    pub(super) fn source(&self) -> String {
        match self {
            Self::Num(v) => format!("{v:?}"),
            Self::X => "x".to_owned(),
            Self::Param(id, Some(unit)) => format!("[{id}:{unit}]"),
            Self::Param(id, None) => (*id).to_owned(),
            Self::Unary(op, a) => format!("({}{})", op.symbol(), a.source()),
            Self::Binary(op, a, b) => format!("({} {} {})", a.source(), op.symbol(), b.source()),
            Self::Call(f, args) => {
                let args: Vec<String> = args.iter().map(Self::source).collect();
                format!("{}({})", f.name(), args.join(" , "))
            }
        }
    }

    fn sexpr(&self) -> String {
        match self {
            Self::Num(v) => format!("{v:?}"),
            Self::X => "x".to_owned(),
            Self::Param(..) => self.source(),
            Self::Unary(UnaryOp::Neg, a) => format!("(neg {})", a.sexpr()),
            Self::Unary(UnaryOp::Not, a) => format!("(not {})", a.sexpr()),
            Self::Binary(op, a, b) => format!("({} {} {})", op.symbol(), a.sexpr(), b.sexpr()),
            Self::Call(f, args) => {
                let mut s = format!("({}", f.name());
                for a in args {
                    s.push(' ');
                    s.push_str(&a.sexpr());
                }
                s.push(')');
                s
            }
        }
    }
}

fn number() -> impl Strategy<Value = f64> {
    prop_oneof![
        (0u32..100_000).prop_map(f64::from),
        0.0..1e6f64,
        any::<f64>().prop_filter("finite, non-negative", |v| v.is_finite()
            && v.is_sign_positive()),
        Just(0.1),
        Just(5e-324),
        Just(f64::MAX),
    ]
}

fn leaf() -> impl Strategy<Value = Tree> {
    prop_oneof![
        number().prop_map(Tree::Num),
        Just(Tree::X),
        (
            proptest::sample::select(vec!["P8", "P12", "E33", "_a1", "pi"]),
            proptest::option::of(proptest::sample::select(vec![
                "rpm", "km/h", "µs", "%", "deg C"
            ])),
        )
            .prop_map(|(id, unit)| Tree::Param(id, unit)),
    ]
}

pub(super) fn any_tree() -> impl Strategy<Value = Tree> {
    leaf().prop_recursive(6, 64, 4, |inner| {
        prop_oneof![
            (
                proptest::sample::select(vec![UnaryOp::Neg, UnaryOp::Not]),
                inner.clone()
            )
                .prop_map(|(op, a)| Tree::Unary(op, Box::new(a))),
            (
                proptest::sample::select(BinaryOp::ALL.to_vec()),
                inner.clone(),
                inner.clone()
            )
                .prop_map(|(op, a, b)| Tree::Binary(op, Box::new(a), Box::new(b))),
            (
                proptest::sample::select(Func::ALL.to_vec()),
                proptest::collection::vec(inner, 1..5)
            )
                .prop_map(|(f, mut args)| {
                    // Trim or pad to an accepted arity.
                    let n = match f.arity() {
                        Arity::Exact(n) => usize::from(n),
                        Arity::Between(lo, hi) => args.len().clamp(lo.into(), hi.into()),
                        Arity::AtLeast(lo) => args.len().max(lo.into()),
                    };
                    args.resize(n, Tree::X);
                    if f == Func::BitWise {
                        args[2] = Tree::Num(1.0);
                    }
                    Tree::Call(f, args)
                }),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// The parser reads a fully parenthesised rendering as the generated
    /// tree, and its own minimal-parentheses rendering back to the same tree.
    #[test]
    fn printing_round_trips(t in any_tree()) {
        let full = t.source();
        let e = ok(&full);
        prop_assert_eq!(sexpr(&e), t.sexpr());
        check_structure(&full, &e);
        let printed = e.to_string();
        let again = ok(&printed);
        prop_assert_eq!(sexpr(&again), t.sexpr());
        prop_assert_eq!(again.to_string(), printed);
    }

    /// Arbitrary text never panics, and an error's span lies in the text.
    #[test]
    fn arbitrary_text_never_panics(src in any::<String>()) {
        check_any(&src)?;
    }

    /// Formula-like text (operators, digits, brackets) exercises every path.
    #[test]
    fn formula_like_text_never_panics(
        src in proptest::string::string_regex(r"[x0-9.eE+\-*/%^()!&|<>=,\[\]:P µ\t]{0,48}").unwrap()
    ) {
        check_any(&src)?;
    }
}

fn check_any(src: &str) -> Result<(), TestCaseError> {
    match parse(src) {
        Ok(e) => {
            check_structure(src, &e);
            let again = ok(&e.to_string());
            prop_assert_eq!(sexpr(&again), sexpr(&e));
        }
        Err(e) => {
            prop_assert!(e.span().end() as usize <= src.len());
            prop_assert!(src.is_char_boundary(e.span().start() as usize));
            prop_assert!(src.is_char_boundary(e.span().end() as usize));
            prop_assert!(!e.to_string().is_empty());
        }
    }
    Ok(())
}
