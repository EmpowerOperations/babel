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

use crate::ast::{
    Assignment, BinaryOp, Block, CompareOp, Expr, Kind, LocalSlot, Program, const_eval, to_index,
};
use crate::diagnostics::{BoundKind, Fault, ProblemKind, Span};

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

        // Nothing produces a fold before unrolling runs, so this is only here
        // to keep the match exhaustive.
        Kind::Fold { kind, terms } => Kind::Fold {
            kind,
            terms: terms.into_iter().map(rewrite_expr).collect(),
        },

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

/// The most terms an aggregate will unroll into.
///
/// A policy knob, not a correctness one: unrolling a million terms is a memory
/// blowup, and the JVM implementation had no cap at all. Generous against the
/// ~200 of the performance fixture and the 9 of rosenbrock, while leaving the
/// pathological cases as run-time loops — an aggregate that large is its own
/// disaster for a solver anyway.
const UNROLL_LIMIT: i64 = 1024;

/// Replaces every aggregate whose bounds are known at compile time with the
/// terms it expands to.
///
/// This is what keeps `sum`/`prod` out of an SMT-LIB translation, where they
/// would otherwise become quantifiers and cost a complexity class. An aggregate
/// whose bounds depend on a variable is left alone and stays a run-time loop.
///
/// # Errors
/// A statically known bound that is not a usable index — NaN, infinite, or
/// fractional — is a compile-time failure rather than one waiting to happen on
/// every evaluation.
pub(crate) fn unroll_aggregates(program: Program) -> Result<Program, Vec<Fault>> {
    let Program { body, frame_size } = program;
    Ok(Program {
        body: unroll_block(body)?,
        frame_size,
    })
}

fn unroll_block(block: Block) -> Result<Block, Vec<Fault>> {
    let Block {
        assignments,
        result,
    } = block;
    Ok(Block {
        assignments: assignments
            .into_iter()
            .map(|Assignment { slot, value, span }| {
                Ok(Assignment {
                    slot,
                    value: unroll_expr(value)?,
                    span,
                })
            })
            .collect::<Result<_, Vec<Fault>>>()?,
        result: unroll_expr(result)?,
    })
}

fn unroll_expr(expr: Expr) -> Result<Expr, Vec<Fault>> {
    let Expr { kind, span } = expr;

    let kind = match kind {
        Kind::Aggregate {
            kind,
            lower,
            upper,
            param,
            body,
        } => {
            // Descend first: an inner aggregate only becomes unrollable once the
            // enclosing parameter has been substituted away, and substitution
            // happens below. Recursing here handles nesting without a fixpoint.
            let lower = unroll_descend(*lower)?;
            let upper = unroll_descend(*upper)?;
            let body = Box::new(unroll_block(*body)?);

            match static_range(&lower, &upper)? {
                Some(range) => Kind::Fold {
                    kind,
                    terms: range
                        .map(|index| {
                            let term = Expr::new(Kind::Block(body.clone()), span);
                            substitute(term, param, index)
                        })
                        .collect(),
                },
                None => Kind::Aggregate {
                    kind,
                    lower,
                    upper,
                    param,
                    body,
                },
            }
        }

        Kind::Unary { op, arg } => Kind::Unary {
            op,
            arg: unroll_descend(*arg)?,
        },
        Kind::Binary { op, lhs, rhs } => Kind::Binary {
            op,
            lhs: unroll_descend(*lhs)?,
            rhs: unroll_descend(*rhs)?,
        },
        Kind::DynamicIndex(index) => Kind::DynamicIndex(unroll_descend(*index)?),
        Kind::Block(block) => Kind::Block(Box::new(unroll_block(*block)?)),
        Kind::Fold { kind, terms } => Kind::Fold {
            kind,
            terms: terms
                .into_iter()
                .map(unroll_expr)
                .collect::<Result<_, Vec<Fault>>>()?,
        },

        leaf @ (Kind::Literal(_) | Kind::Global(_) | Kind::Local(_)) => leaf,

        // Eliminated by `rewrite_booleans`, which runs first.
        Kind::Compare { .. } | Kind::NearEq { .. } => {
            unreachable!("the boolean rewrite runs before unrolling")
        }
    };

    Ok(Expr { kind, span })
}

/// Mirrors `descend` above: takes the child by value so the caller's box is
/// released rather than passed through.
fn unroll_descend(expr: Expr) -> Result<Box<Expr>, Vec<Fault>> {
    Ok(Box::new(unroll_expr(expr)?))
}

/// The range an aggregate covers, when both bounds are statically known.
///
/// `Ok(None)` means at least one bound depends on a variable, so the aggregate
/// has to stay a loop. `Err` means a bound is known *and* unusable.
fn static_range(
    lower: &Expr,
    upper: &Expr,
) -> Result<Option<std::ops::RangeInclusive<i64>>, Vec<Fault>> {
    let (Some(lower_value), Some(upper_value)) = (const_eval(lower), const_eval(upper)) else {
        return Ok(None);
    };

    let bound = |value: f64, which: BoundKind, at: &Expr| {
        to_index(value).ok_or_else(|| {
            vec![Fault {
                kind: ProblemKind::IllegalAggregateBound {
                    bound: which,
                    value,
                },
                span: at.span,
            }]
        })
    };

    let first = bound(lower_value, BoundKind::Lower, lower)?;
    let last = bound(upper_value, BoundKind::Upper, upper)?;

    // Wider than the cap: still correct as a loop, just not worth expanding.
    if last.saturating_sub(first) >= UNROLL_LIMIT {
        return Ok(None);
    }

    Ok(Some(first..=last))
}

/// Replaces every reference to `param` with `index`.
///
/// By slot rather than by name, so a body that shadows the parameter —
/// `sum(1, 3, i -> var i = 5; i)` — binds a different slot and is left alone.
#[allow(clippy::cast_precision_loss)]
fn substitute(expr: Expr, param: LocalSlot, index: i64) -> Expr {
    let Expr { kind, span } = expr;

    let kind = match kind {
        Kind::Local(slot) if slot == param => Kind::Literal(index as f64),

        Kind::Unary { op, arg } => Kind::Unary {
            op,
            arg: Box::new(substitute(*arg, param, index)),
        },
        Kind::Binary { op, lhs, rhs } => Kind::Binary {
            op,
            lhs: Box::new(substitute(*lhs, param, index)),
            rhs: Box::new(substitute(*rhs, param, index)),
        },
        Kind::DynamicIndex(subscript) => {
            Kind::DynamicIndex(Box::new(substitute(*subscript, param, index)))
        }
        Kind::Aggregate {
            kind,
            lower,
            upper,
            param: inner,
            body,
        } => Kind::Aggregate {
            kind,
            lower: Box::new(substitute(*lower, param, index)),
            upper: Box::new(substitute(*upper, param, index)),
            param: inner,
            body: Box::new(substitute_block(*body, param, index)),
        },
        Kind::Block(block) => Kind::Block(Box::new(substitute_block(*block, param, index))),
        Kind::Fold { kind, terms } => Kind::Fold {
            kind,
            terms: terms
                .into_iter()
                .map(|term| substitute(term, param, index))
                .collect(),
        },

        leaf @ (Kind::Literal(_) | Kind::Global(_) | Kind::Local(_)) => leaf,

        Kind::Compare { .. } | Kind::NearEq { .. } => {
            unreachable!("the boolean rewrite runs before unrolling")
        }
    };

    Expr { kind, span }
}

fn substitute_block(block: Block, param: LocalSlot, index: i64) -> Block {
    let Block {
        assignments,
        result,
    } = block;
    Block {
        assignments: assignments
            .into_iter()
            .map(|Assignment { slot, value, span }| Assignment {
                slot,
                value: substitute(value, param, index),
                span,
            })
            .collect(),
        result: substitute(result, param, index),
    }
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
            Kind::Fold { terms, .. } => terms.iter().any(holds_comparison),
            Kind::Literal(_) | Kind::Global(_) | Kind::Local(_) => false,
        }
    }

    fn block_holds(block: &Block) -> bool {
        block.assignments.iter().any(|a| holds_comparison(&a.value))
            || holds_comparison(&block.result)
    }

    /// Unrolling must not change a value. `0.1 * i` is chosen because `f64`
    /// addition is not associative, so a rebalanced tree would disagree here
    /// even though it agrees on the small integers the corpus uses.
    ///
    /// The first expression has literal bounds and unrolls; the second's bounds
    /// depend on `x1` and stay a run-time loop. They must be bit-identical.
    #[test]
    fn unrolling_agrees_with_the_loop_bit_for_bit() {
        let unrolled = crate::compile("sum(1, 3, i -> 0.1*i)").expect("should compile");
        let looped = crate::compile("sum(x1+0, x1+2, i -> 0.1*i)").expect("should compile");

        assert_eq!(
            unrolled.evaluate(&[]).expect("should evaluate"),
            looped.evaluate(&[("x1", 1.0)]).expect("should evaluate"),
        );
    }

    /// A statically bounded aggregate becomes one n-ary node, not a chain.
    /// A chain is what would put a thousand-term unroll back on the stack.
    #[test]
    fn a_static_aggregate_becomes_one_flat_fold() {
        let expression = crate::compile("sum(1, 5, i -> i)").expect("should compile");

        match &expression.program.body.result.kind {
            Kind::Fold { terms, .. } => assert_eq!(terms.len(), 5),
            other => panic!("expected a flat fold, got {other:?}"),
        }
    }

    /// Past the cap an aggregate keeps its loop — still correct, just not
    /// expanded. `sum(1, 2000, ...)` is 2000 terms against a 1024 limit.
    #[test]
    fn an_aggregate_past_the_cap_stays_a_loop() {
        let expression = crate::compile("sum(1, 2000, i -> i)").expect("should compile");

        assert!(
            matches!(expression.program.body.result.kind, Kind::Aggregate { .. }),
            "expected the loop to survive, got {:?}",
            expression.program.body.result.kind
        );
        // And it still evaluates: 1 + 2 + ... + 2000.
        assert_eq!(
            expression.evaluate(&[]).expect("should evaluate"),
            2_001_000.0
        );
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
