//! AST-to-AST rewrites.
//!
//! Each pass is `fn(Program) -> Program`, taking ownership. Passing trees by
//! value means unchanged subtrees are *moved* rather than cloned or refcounted,
//! which is why the AST needs no persistent collections to be effectively
//! immutable.
//!
//! # The boolean convention
//!
//! Babel has no boolean values at run time. A comparison lowers to arithmetic
//! whose *sign* carries the truth value: `<= 0` is true, `> 0` is false. That is
//! the canonical `g(x) <= 0` constraint form, so a violated constraint reports
//! how badly it was violated rather than merely that it was.

use crate::ast::{Assignment, BinaryOp, Block, CompareOp, Expr, Kind, Program};
use crate::diagnostics::Span;

/// Java's `Double.MIN_NORMAL`, the nudge that makes a *strict* inequality
/// representable when `<= 0` means true.
///
/// It is meant to vanish into rounding at any meaningful magnitude — `(4 - 6) + ε`
/// is exactly `-2.0` — and to survive only when the difference is zero, which is
/// precisely when strict and non-strict differ: `(6 - 6) + ε` is `ε`, which is
/// `> 0`, so `6 > 6` is false.
const EPSILON: f64 = f64::MIN_POSITIVE;

/// Eliminates every [`Kind::Compare`] and [`Kind::NearEq`], replacing them with
/// arithmetic under the sign convention above.
///
/// | source | becomes |
/// |---|---|
/// | `a <= b` | `a - b` |
/// | `a >= b` | `b - a` |
/// | `a < b` | `(a - b) + ε` |
/// | `a > b` | `(b - a) + ε` |
/// | `a == b +/- t` | `max((b - t) - a, a - (b + t))` |
///
/// The evaluator relies on this being total: it treats those two variants as
/// unreachable.
pub(crate) fn rewrite_booleans(program: Program) -> Program {
    let Program { body, frame_size } = program;
    Program {
        body: rewrite_block(body),
        frame_size,
    }
}

fn rewrite_block(block: Block) -> Block {
    let Block {
        assignments,
        result,
    } = block;
    Block {
        assignments: assignments
            .into_iter()
            .map(|Assignment { slot, value, span }| Assignment {
                slot,
                value: rewrite_expr(value),
                span,
            })
            .collect(),
        result: rewrite_expr(result),
    }
}

fn rewrite_expr(node: Expr) -> Expr {
    let Expr { kind, span } = node;

    let kind = match kind {
        Kind::Compare { op, lhs, rhs } => {
            let (lhs, rhs) = (descend(*lhs), descend(*rhs));
            let (left, right) = match op {
                CompareOp::Lt | CompareOp::Lte => (lhs, rhs),
                CompareOp::Gt | CompareOp::Gte => (rhs, lhs),
            };

            let difference = Kind::Binary {
                op: BinaryOp::Sub,
                lhs: left,
                rhs: right,
            };

            match op {
                CompareOp::Lte | CompareOp::Gte => difference,
                CompareOp::Lt | CompareOp::Gt => Kind::Binary {
                    op: BinaryOp::Add,
                    lhs: Box::new(Expr::new(difference, span)),
                    rhs: Box::new(Expr::new(Kind::Literal(EPSILON), span)),
                },
            }
        }
        Kind::NearEq {
            lhs,
            rhs,
            tolerance,
        } => {
            let (lhs, rhs) = (descend(*lhs), descend(*rhs));
            let tolerance = || Box::new(Expr::new(Kind::Literal(tolerance), span));

            // Both operands appear twice in the output, so one deep clone of each is
            // unavoidable with owned subtrees. It happens once, at compile time.
            let lower_bound = binary(BinaryOp::Sub, rhs.clone(), tolerance(), span);
            let upper_bound = binary(BinaryOp::Add, rhs, tolerance(), span);

            let at_least = binary(BinaryOp::Sub, Box::new(lower_bound), lhs.clone(), span);
            let at_most = binary(BinaryOp::Sub, lhs, Box::new(upper_bound), span);

            Kind::Binary {
                op: BinaryOp::Max,
                lhs: Box::new(at_least),
                rhs: Box::new(at_most),
            }
        }

        Kind::Unary { op, arg } => Kind::Unary {
            op,
            arg: descend(*arg),
        },
        Kind::Binary { op, lhs, rhs } => Kind::Binary {
            op,
            lhs: descend(*lhs),
            rhs: descend(*rhs),
        },
        Kind::DynamicIndex(index) => Kind::DynamicIndex(descend(*index)),
        Kind::Aggregate {
            kind,
            lower,
            upper,
            param,
            body,
        } => Kind::Aggregate {
            kind,
            lower: descend(*lower),
            upper: descend(*upper),
            param,
            body: Box::new(rewrite_block(*body)),
        },
        Kind::Block(block) => Kind::Block(Box::new(rewrite_block(*block))),

        leaf @ (Kind::Literal(_) | Kind::Global(_) | Kind::Local(_)) => leaf,
    };

    Expr { kind, span }
}

/// Rewrites a child and re-boxes it. Takes the node by value rather than by
/// `Box` so the caller's allocation is released rather than passed through —
/// a fresh box per rewritten node is the cost of building trees functionally.
fn descend(node: Expr) -> Box<Expr> {
    Box::new(rewrite_expr(node))
}

fn binary(op: BinaryOp, lhs: Box<Expr>, rhs: Box<Expr>, span: Span) -> Expr {
    Expr::new(Kind::Binary { op, lhs, rhs }, span)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holds_comparison(node: &Expr) -> bool {
        match &node.kind {
            Kind::Compare { .. } | Kind::NearEq { .. } => true,
            Kind::Unary { arg, .. } => holds_comparison(arg),
            Kind::Binary { lhs, rhs, .. } => holds_comparison(lhs) || holds_comparison(rhs),
            Kind::DynamicIndex(index) => holds_comparison(index),
            Kind::Aggregate {
                lower, upper, body, ..
            } => holds_comparison(lower) || holds_comparison(upper) || block_holds(body),
            Kind::Block(block) => block_holds(block),
            Kind::Literal(_) | Kind::Global(_) | Kind::Local(_) => false,
        }
    }

    fn block_holds(block: &Block) -> bool {
        block.assignments.iter().any(|a| holds_comparison(&a.value))
            || holds_comparison(&block.result)
    }

    /// The invariant `eval.rs` depends on, where it is otherwise guarded only by
    /// an `unreachable!`.
    #[test]
    fn no_comparison_survives_the_rewrite() {
        for source in [
            "4 < 6",
            "6 > 6",
            "1.0e200 <= 1.0e200",
            "1.0e200 >= 1.0e200",
            "x1 == x2 +/- 0.15",
            "(4 < 6)",
        ] {
            let expression = crate::compile(source)
                .unwrap_or_else(|e| panic!("compile failed for {source:?}: {e}"));
            assert!(
                !block_holds(&expression.program.body),
                "a comparison survived the rewrite of {source:?}"
            );
        }
    }
}
