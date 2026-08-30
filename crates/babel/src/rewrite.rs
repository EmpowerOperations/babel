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

use std::f64::consts::FRAC_PI_2;

use crate::ast::{
    Assignment, BinaryOp, Block, CompareOp, Expr, Kind, LocalSlot, Program, UnaryOp, to_index,
};
use crate::diagnostics::{BoundKind, Fault, ProblemKind, Span};

/// Java's `Double.MIN_NORMAL`, the nudge that makes a *strict* inequality
/// representable when `<= 0` means true.
///
/// It is meant to vanish into rounding at any meaningful magnitude — `(4 - 6) + ε`
/// is exactly `-2.0` — and to survive only when the difference is zero, which is
/// precisely when strict and non-strict differ: `(6 - 6) + ε` is `ε`, which is
/// `> 0`, so `6 > 6` is false.
pub(crate) const EPSILON: f64 = f64::MIN_POSITIVE;

/// Replaces every subexpression made only of literals with the value it works
/// out to.
///
/// Runs first, and the reason it runs first is that it makes "is this constant?"
/// stop being a question anywhere else. After this pass a statically known value
/// **is** a [`Kind::Literal`], so [`unroll_aggregates`] can ask about its bounds
/// with a pattern match and [`invert_monotone`] can ask about a comparison's
/// other side the same way. Neither needs an evaluator of its own, and there
/// used to be one — `ast::const_eval`, which this pass replaced.
///
/// Bottom-up, so the work is linear: children are folded before their parent is
/// looked at, which means a parent only ever has to combine literals rather than
/// walk a subtree. Values come from [`crate::ast::UnaryOp::apply`] and
/// [`crate::ast::BinaryOp::apply`], the same functions the evaluator calls, so
/// folding cannot change a result — `corpus.rs` pins several constant
/// expressions by value and is the guard on that.
///
/// # Errors
/// A constant subexpression that works out to NaN or an infinity. See
/// [`ProblemKind::NonFiniteConstant`] for why that is refused rather than
/// folded.
pub(crate) fn fold_constants(program: Program) -> Result<Program, Vec<Fault>> {
    let Program { body, frame_size } = program;
    Ok(Program {
        body: fold_block(body)?,
        frame_size,
    })
}

fn fold_block(block: Block) -> Result<Block, Vec<Fault>> {
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
                    value: fold_expr(value)?,
                    span,
                })
            })
            .collect::<Result<_, Vec<Fault>>>()?,
        result: fold_expr(result)?,
    })
}

fn fold_expr(expr: Expr) -> Result<Expr, Vec<Fault>> {
    let Expr { kind, span } = expr;

    let kind = match kind {
        Kind::Unary { op, arg } => {
            let arg = fold_descend(*arg)?;
            match arg.kind {
                Kind::Literal(value) => Kind::Literal(op.apply(value)),
                _ => Kind::Unary { op, arg },
            }
        }
        Kind::Binary { op, lhs, rhs } => {
            let (lhs, rhs) = (fold_descend(*lhs)?, fold_descend(*rhs)?);
            match (&lhs.kind, &rhs.kind) {
                (Kind::Literal(left), Kind::Literal(right)) => {
                    Kind::Literal(op.apply(*left, *right))
                }
                _ => Kind::Binary { op, lhs, rhs },
            }
        }

        Kind::Compare { op, lhs, rhs } => Kind::Compare {
            op,
            lhs: fold_descend(*lhs)?,
            rhs: fold_descend(*rhs)?,
        },
        Kind::NearEq {
            lhs,
            rhs,
            tolerance,
        } => Kind::NearEq {
            lhs: fold_descend(*lhs)?,
            rhs: fold_descend(*rhs)?,
            tolerance,
        },
        Kind::DynamicIndex(index) => Kind::DynamicIndex(fold_descend(*index)?),
        Kind::Aggregate {
            kind,
            lower,
            upper,
            param,
            body,
        } => Kind::Aggregate {
            kind,
            lower: fold_descend(*lower)?,
            upper: fold_descend(*upper)?,
            param,
            body: Box::new(fold_block(*body)?),
        },
        Kind::Block(block) => Kind::Block(Box::new(fold_block(*block)?)),

        // Unrolling runs last, so nothing has produced one of these yet. Folding
        // an unrolled aggregate's terms would be worth real time on the
        // evaluator and is deliberately left to the tape work — moving this pass
        // after unrolling would put it after the boolean rewrite too, where it
        // would collapse a constant comparison onto the strictness epsilon.
        Kind::Fold { kind, terms } => Kind::Fold {
            kind,
            terms: terms
                .into_iter()
                .map(fold_expr)
                .collect::<Result<_, Vec<Fault>>>()?,
        },

        // Literals are checked as well as folded results: babel's `FLOAT` token
        // admits `1.0e400`, which parses straight to an infinity, and refusing
        // `1.0e400 * 1.0` while accepting `1.0e400` would be a rule with a hole
        // in it.
        leaf @ (Kind::Literal(_) | Kind::Global(_) | Kind::Local(_)) => leaf,
    };

    if let Kind::Literal(value) = kind
        && !value.is_finite()
    {
        return Err(vec![Fault {
            kind: ProblemKind::NonFiniteConstant { value },
            span,
        }]);
    }

    Ok(Expr { kind, span })
}

/// Mirrors `descend` below: takes the child by value so the caller's box is
/// released rather than passed through.
fn fold_descend(expr: Expr) -> Result<Box<Expr>, Vec<Fault>> {
    Ok(Box::new(fold_expr(expr)?))
}

/// Rewrites `f(u) op c` into a comparison on `u`, for the strictly monotone `f`
/// that no solver will reason about.
///
/// `2 < ln(x1)` becomes `x1 > e^2`; `20 > 2^x5` becomes `x5 < log2(20)`. The
/// bound is computed here, in `f64`, so what reaches the emitter is linear in
/// `u` and wants no logarithm from the solver at all. Both are real constraints
/// in the CVG corpus and are the whole reason this exists — Z3 has no logarithm
/// under any spelling, and inverting through `^` does not work either, since a
/// variable exponent answers `unknown` even with the other side pinned.
///
/// It outlives its own motivation, too: restricting `a ^ b` to an integer `b`
/// would make `2^x5` a compile error, and this rewrites it away first.
///
/// Infallible. Anything it cannot invert it leaves exactly as it found it, to be
/// reported through `Document::untranslated` as before.
pub(crate) fn invert_monotone(program: Program) -> Program {
    let Program { body, frame_size } = program;
    Program {
        body: invert_block(body),
        frame_size,
    }
}

fn invert_block(block: Block) -> Block {
    let Block {
        assignments,
        result,
    } = block;
    Block {
        assignments: assignments
            .into_iter()
            .map(|Assignment { slot, value, span }| Assignment {
                slot,
                value: invert_expr(value),
                span,
            })
            .collect(),
        result: invert_expr(result),
    }
}

fn invert_expr(node: Expr) -> Expr {
    let Expr { kind, span } = node;

    let kind = match kind {
        Kind::Compare { op, lhs, rhs } => {
            return invert_comparison(op, invert_expr(*lhs), invert_expr(*rhs), span);
        }

        Kind::Unary { op, arg } => Kind::Unary {
            op,
            arg: Box::new(invert_expr(*arg)),
        },
        Kind::Binary { op, lhs, rhs } => Kind::Binary {
            op,
            lhs: Box::new(invert_expr(*lhs)),
            rhs: Box::new(invert_expr(*rhs)),
        },
        Kind::NearEq {
            lhs,
            rhs,
            tolerance,
        } => Kind::NearEq {
            lhs: Box::new(invert_expr(*lhs)),
            rhs: Box::new(invert_expr(*rhs)),
            tolerance,
        },
        Kind::DynamicIndex(index) => Kind::DynamicIndex(Box::new(invert_expr(*index))),
        Kind::Aggregate {
            kind,
            lower,
            upper,
            param,
            body,
        } => Kind::Aggregate {
            kind,
            lower: Box::new(invert_expr(*lower)),
            upper: Box::new(invert_expr(*upper)),
            param,
            body: Box::new(invert_block(*body)),
        },
        Kind::Block(block) => Kind::Block(Box::new(invert_block(*block))),
        Kind::Fold { kind, terms } => Kind::Fold {
            kind,
            terms: terms.into_iter().map(invert_expr).collect(),
        },

        leaf @ (Kind::Literal(_) | Kind::Global(_) | Kind::Local(_)) => leaf,
    };

    Expr { kind, span }
}

/// A strictly monotone function, and what it takes to invert a comparison
/// against one.
struct Monotone {
    /// `f`-inverse, to be applied to the constant side. A closure rather than a
    /// `fn` pointer because `a ^ u` has to carry its base.
    inverse: Box<dyn Fn(f64) -> f64>,
    /// Whether `f` preserves the comparison. A decreasing one reverses it.
    increasing: bool,
    /// The closure of the values `f` can produce.
    ///
    /// Checked before inverting, and not an optimisation: `atan(x) > 2` is
    /// unsatisfiable, and `x > tan(2)` — which is `x > -2.18` — is very much not
    /// the same claim.
    range: (f64, f64),
    /// The values `f` accepts, when it does not accept all of them. Every
    /// entry in the table is either total or bounded below, so a floor and
    /// whether it is attainable says the whole of it.
    domain_floor: Option<(f64, CompareOp)>,
}

/// The invertible application at `expr`, if it is one, paired with its argument.
///
/// The argument is a whole subtree, not a variable: `ln(x1 + x2) > 2` inverts to
/// `x1 + x2 > e^2` as readily as the one-variable case.
///
/// `abs`, `sqr`, `sgn` and the trigonometric functions are absent because they
/// are not monotone. `cos` on `[0, pi]` is, but knowing that needs a bound on
/// the argument, which is causalization's problem rather than this pass's.
fn monotone(expr: &Expr) -> Option<(Monotone, &Expr)> {
    const ALL: (f64, f64) = (f64::NEG_INFINITY, f64::INFINITY);

    fn total(inverse: impl Fn(f64) -> f64 + 'static, range: (f64, f64)) -> Monotone {
        Monotone {
            inverse: Box::new(inverse),
            increasing: true,
            range,
            domain_floor: None,
        }
    }
    /// `ln` and `log10`, whose floor is **zero inclusive** and not what the
    /// mathematics would suggest. Babel evaluates `ln(0)` to negative infinity
    /// rather than NaN, so zero satisfies any upper bound — and a guard reading
    /// `u > 0` would be *narrower* than the constraint it replaced, which is the
    /// direction that turns a sound `unsat` into a wrong one. Matching the
    /// evaluator is the requirement; matching the textbook is not.
    fn logarithmic(inverse: impl Fn(f64) -> f64 + 'static) -> Monotone {
        Monotone {
            inverse: Box::new(inverse),
            increasing: true,
            range: ALL,
            domain_floor: Some((0.0, CompareOp::Gte)),
        }
    }

    match &expr.kind {
        Kind::Unary { op, arg } => {
            let inversion = match op {
                UnaryOp::Ln => logarithmic(f64::exp),
                UnaryOp::Log10 => logarithmic(|c| 10.0_f64.powf(c)),
                UnaryOp::Sqrt => Monotone {
                    inverse: Box::new(|c| c * c),
                    increasing: true,
                    range: (0.0, f64::INFINITY),
                    domain_floor: Some((0.0, CompareOp::Gte)),
                },
                UnaryOp::Cbrt => total(|c| c * c * c, ALL),
                UnaryOp::Cube => total(f64::cbrt, ALL),
                UnaryOp::Sinh => total(f64::asinh, ALL),
                UnaryOp::Tanh => total(f64::atanh, (-1.0, 1.0)),
                UnaryOp::Atan => total(f64::tan, (-FRAC_PI_2, FRAC_PI_2)),
                _ => return None,
            };
            Some((inversion, arg.as_ref()))
        }

        // `a ^ u` for a positive constant base other than 1. Decreasing below
        // one, which is the only row in the table that reverses a comparison.
        Kind::Binary {
            op: BinaryOp::Pow,
            lhs,
            rhs,
        } => {
            let Kind::Literal(base) = lhs.kind else {
                return None;
            };
            // No NaN check needed: `fold_constants` runs first and refuses a
            // non-finite literal, so every literal reaching this pass is finite.
            // `1 ^ u` is excluded because it is constant rather than monotone,
            // and exact equality is the right test — the base is a literal the
            // author wrote, not the result of a computation.
            if base <= 0.0 || base == 1.0 {
                return None;
            }
            Some((
                Monotone {
                    inverse: Box::new(move |c| c.ln() / base.ln()),
                    increasing: base > 1.0,
                    range: (0.0, f64::INFINITY),
                    domain_floor: None,
                },
                rhs.as_ref(),
            ))
        }

        _ => None,
    }
}

/// `a op b` with the sides exchanged: what `b ? a` has to be to mean the same.
const fn mirrored(op: CompareOp) -> CompareOp {
    match op {
        CompareOp::Lt => CompareOp::Gt,
        CompareOp::Lte => CompareOp::Gte,
        CompareOp::Gt => CompareOp::Lt,
        CompareOp::Gte => CompareOp::Lte,
    }
}

fn invert_comparison(op: CompareOp, lhs: Expr, rhs: Expr, span: Span) -> Expr {
    // Orient to `f(u) op c`, whichever side the function turned up on.
    let (op, function, constant) = match (&lhs.kind, &rhs.kind) {
        (_, Kind::Literal(constant)) if monotone(&lhs).is_some() => (op, &lhs, *constant),
        (Kind::Literal(constant), _) if monotone(&rhs).is_some() => (mirrored(op), &rhs, *constant),
        _ => {
            return Expr::new(
                Kind::Compare {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            );
        }
    };

    let unchanged = || {
        Expr::new(
            Kind::Compare {
                op,
                lhs: Box::new(lhs.clone()),
                rhs: Box::new(rhs.clone()),
            },
            span,
        )
    };

    let Some((inversion, argument)) = monotone(function) else {
        unreachable!("the orientation above already established one");
    };

    // Outside what `f` can produce there is nothing to invert — the comparison
    // is constantly true or constantly false, and saying which would need a
    // representation for a constant boolean that does not exist yet.
    if constant < inversion.range.0 || constant > inversion.range.1 {
        return unchanged();
    }

    let op = if inversion.increasing {
        op
    } else {
        mirrored(op)
    };
    let bound = (inversion.inverse)(constant);

    // Catches the open ends of a range — `atanh(1.0)` is infinite — and an
    // inverse that simply overflows, as `exp` does past about 709.
    if !bound.is_finite() {
        return unchanged();
    }

    // One ulp outward, so the region asserted is never *narrower* than the one
    // the constraint describes. `fl(e^2)` is not `e^2`, and on the narrow side
    // an `unsat` would stop implying that the original is unsatisfiable. The
    // sliver this admits is filtered by the pool's own `evaluate`, which is the
    // existing contract for everything a solver proposes.
    let bounds_above = matches!(op, CompareOp::Lt | CompareOp::Lte);
    let bound = if bounds_above {
        bound.next_up()
    } else {
        bound.next_down()
    };

    let inverted = Expr::new(
        Kind::Compare {
            op,
            lhs: Box::new(argument.clone()),
            rhs: Box::new(Expr::new(Kind::Literal(bound), span)),
        },
        span,
    );

    // A lower bound on `u` normally carries the domain with it, because
    // `f`-inverse lands in the domain by definition — `e^c` is positive for
    // every `c`. Bounding `u` from above does not, and neither does a widened
    // bound that has slipped below the floor, which `sqrt(x) > 0` manages.
    let Some((floor, floor_op)) = inversion.domain_floor else {
        return inverted;
    };
    if !bounds_above && bound >= floor {
        return inverted;
    }

    // `and` is `max`: two residuals hold together exactly when the larger of
    // them is `<= 0`. The same encoding `NearEq` lowers to, which is why there
    // is no `Kind::And` to reach for.
    let guard = residual(
        floor_op,
        Box::new(argument.clone()),
        Box::new(Expr::new(Kind::Literal(floor), span)),
        span,
    );
    let both = binary(
        BinaryOp::Max,
        Box::new(residual(
            op,
            Box::new(argument.clone()),
            Box::new(Expr::new(Kind::Literal(bound), span)),
            span,
        )),
        Box::new(guard),
        span,
    );
    Expr::new(
        Kind::Compare {
            op: CompareOp::Lte,
            lhs: Box::new(both),
            rhs: Box::new(Expr::new(Kind::Literal(0.0), span)),
        },
        span,
    )
}

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
        Kind::Compare { op, lhs, rhs } => residual(op, descend(*lhs), descend(*rhs), span).kind,
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

/// `lhs op rhs` as arithmetic that is `<= 0` exactly when the comparison holds.
///
/// The whole of the sign convention, in one place. [`invert_monotone`] needs it
/// too, because a conjunction of constraints is the `max` of their residuals and
/// there is no other way to say "and" — so it has to be able to build one.
fn residual(op: CompareOp, lhs: Box<Expr>, rhs: Box<Expr>, span: Span) -> Expr {
    let (left, right) = match op {
        CompareOp::Lt | CompareOp::Lte => (lhs, rhs),
        CompareOp::Gt | CompareOp::Gte => (rhs, lhs),
    };
    let difference = binary(BinaryOp::Sub, left, right, span);

    match op {
        CompareOp::Lte | CompareOp::Gte => difference,
        CompareOp::Lt | CompareOp::Gt => binary(
            BinaryOp::Add,
            Box::new(difference),
            Box::new(Expr::new(Kind::Literal(EPSILON), span)),
            span,
        ),
    }
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
///
/// "Statically known" is a pattern match rather than an analysis, because
/// [`fold_constants`] has already run: `sum(1, 2+3, …)` arrives here with a
/// literal `5` in it. This function used to evaluate the bound expressions
/// itself, through an `ast::const_eval` that existed only to serve it.
fn static_range(
    lower: &Expr,
    upper: &Expr,
) -> Result<Option<std::ops::RangeInclusive<i64>>, Vec<Fault>> {
    let (Kind::Literal(lower_value), Kind::Literal(upper_value)) = (&lower.kind, &upper.kind)
    else {
        return Ok(None);
    };
    let (lower_value, upper_value) = (*lower_value, *upper_value);

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

    // ---------------------------------------------------------- fold_constants

    /// The whole point of the pass, in one assertion.
    #[test]
    fn a_constant_call_becomes_a_literal() {
        let expression = crate::compile("sin(2.3)").expect("should compile");
        match expression.program.body.result.kind {
            Kind::Literal(value) => assert_eq!(value, 2.3_f64.sin()),
            ref other => panic!("expected a literal, got {other:?}"),
        }
    }

    /// Folding must reach as far as it can and stop exactly where a variable
    /// starts. Half of this tree is knowable and half is not.
    #[test]
    fn folding_stops_at_the_first_variable() {
        let expression = crate::compile("x1 * (2 + 3) + sqrt(16)").expect("should compile");

        // `2 + 3` and `sqrt(16)` both collapse, so what is left is
        // `x1 * 5 + 4` — three leaves and two operators.
        let mut literals = Vec::new();
        collect_literals(&expression.program.body.result, &mut literals);
        assert_eq!(literals, vec![5.0, 4.0]);
    }

    fn collect_literals(node: &Expr, into: &mut Vec<f64>) {
        match &node.kind {
            Kind::Literal(value) => into.push(*value),
            Kind::Unary { arg, .. } => collect_literals(arg, into),
            Kind::Binary { lhs, rhs, .. } => {
                collect_literals(lhs, into);
                collect_literals(rhs, into);
            }
            _ => {}
        }
    }

    /// Folding may not change a value, ever. A folded chain and the same chain
    /// held apart by a variable have to agree to the bit — which they only do if
    /// folding uses the evaluator's own `apply` in the evaluator's own order.
    #[test]
    fn folding_agrees_with_the_evaluator_bit_for_bit() {
        for (folded, held_apart, at) in [
            ("0.1 + 0.2 + 0.3", "x1 + 0.2 + 0.3", 0.1),
            ("1.0 / 3.0 * 3.0", "x1 / 3.0 * 3.0", 1.0),
            ("sin(2.3) * cos(1.1)", "sin(x1) * cos(1.1)", 2.3),
        ] {
            let folded = crate::compile(folded).expect("should compile");
            let held_apart = crate::compile(held_apart).expect("should compile");
            assert_eq!(
                folded.evaluate(&[]).expect("should evaluate"),
                held_apart.evaluate(&[("x1", at)]).expect("should evaluate"),
                "folding changed a value"
            );
        }
    }

    /// A constant that can only ever be NaN or infinite is a mistake, and one
    /// worth reporting where it was written. The last of these is a *literal*
    /// rather than a folded result: babel's `FLOAT` token admits `1.0e400`,
    /// which parses straight to an infinity, so leaves are checked too.
    #[test]
    fn a_non_finite_constant_is_refused() {
        for source in [
            "ln(-1)",
            "sqrt(-1)",
            "1/0",
            "0/0",
            "1.0e400",
            "1.0e400 * 1.0",
        ] {
            let failure =
                crate::compile(source).expect_err(&format!("{source} should not compile"));
            assert!(
                failure
                    .problems
                    .iter()
                    .any(|p| matches!(p.kind, ProblemKind::NonFiniteConstant { .. })),
                "{source:?} reported {:?}",
                failure.problems
            );
        }
    }

    /// Folding runs before unrolling, which is what lets `static_range` be a
    /// pattern match rather than an evaluator. If this goes red, the
    /// simplification in `static_range` lost something.
    #[test]
    fn an_aggregate_bound_that_needed_folding_still_unrolls() {
        let expression = crate::compile("sum(1, 2+3, i -> i)").expect("should compile");

        match &expression.program.body.result.kind {
            Kind::Fold { terms, .. } => assert_eq!(terms.len(), 5),
            other => panic!("expected a flat fold, got {other:?}"),
        }
    }

    // --------------------------------------------------------- invert_monotone

    /// Structure: the function is gone and the variable faces a literal.
    #[test]
    fn a_monotone_function_leaves_the_comparison() {
        // `2 < ln(x1)` is `ln(x1) > 2` is `x1 > e^2`.
        let expression = crate::compile("2 < ln(x1)").expect("should compile");
        assert!(
            !mentions_unary(&expression.program.body.result, UnaryOp::Ln),
            "the logarithm survived: {:?}",
            expression.program.body.result
        );

        // Residual form is `bound - x1 + eps`, so evaluating just past the
        // bound is negative and just short of it is positive.
        let bound = std::f64::consts::E.powi(2);
        assert!(expression.evaluate(&[("x1", bound * 1.001)]).unwrap() < 0.0);
        assert!(expression.evaluate(&[("x1", bound * 0.999)]).unwrap() > 0.0);
    }

    fn mentions_unary(node: &Expr, wanted: UnaryOp) -> bool {
        match &node.kind {
            Kind::Unary { op, arg } => *op == wanted || mentions_unary(arg, wanted),
            Kind::Binary { lhs, rhs, .. } => {
                mentions_unary(lhs, wanted) || mentions_unary(rhs, wanted)
            }
            Kind::Compare { lhs, rhs, .. } => {
                mentions_unary(lhs, wanted) || mentions_unary(rhs, wanted)
            }
            Kind::Fold { terms, .. } => terms.iter().any(|t| mentions_unary(t, wanted)),
            _ => false,
        }
    }

    /// The test that matters. A wrong inverse or a flipped direction survives
    /// every structural assertion and dies here: for each row of the table, the
    /// rewritten constraint has to agree with the function it replaced, at every
    /// sampled point.
    #[test]
    fn every_inversion_agrees_with_the_function_it_replaced() {
        // `source` inverts; `equivalent` says the same thing in a form the pass
        // cannot touch, by putting the constant behind a variable.
        let cases = [
            ("ln(x1) > 2", "ln(x1) > 2 * x2", 1.0),
            ("ln(x1) < 2", "ln(x1) < 2 * x2", 1.0),
            ("log(x1) >= 0.5", "log(x1) >= 0.5 * x2", 1.0),
            ("sqrt(x1) > 3", "sqrt(x1) > 3 * x2", 1.0),
            ("sqrt(x1) <= 3", "sqrt(x1) <= 3 * x2", 1.0),
            ("cbrt(x1) < 2", "cbrt(x1) < 2 * x2", 1.0),
            ("cube(x1) >= 8", "cube(x1) >= 8 * x2", 1.0),
            ("sinh(x1) > 1.5", "sinh(x1) > 1.5 * x2", 1.0),
            ("tanh(x1) < 0.5", "tanh(x1) < 0.5 * x2", 1.0),
            ("atan(x1) > 0.7", "atan(x1) > 0.7 * x2", 1.0),
            ("2 ^ x1 < 20", "2 ^ x1 < 20 * x2", 1.0),
            // The decreasing row, and the only one that reverses the comparison.
            ("0.5 ^ x1 > 8", "0.5 ^ x1 > 8 * x2", 1.0),
        ];

        for (source, equivalent, x2) in cases {
            let inverted = crate::compile(source).expect("should compile");
            let original = crate::compile(equivalent).expect("should compile");

            // The two may differ, but only in one direction. The inverted
            // bound is nudged a ulp outward on purpose, so it can accept a
            // point the original rejects — and must never reject one the
            // original accepts, because that is the direction in which a
            // solver's `unsat` would stop meaning anything.
            let mut widened = 0;
            for step in -200..=200 {
                let x1 = f64::from(step) * 0.1;
                let (Ok(a), Ok(b)) = (
                    inverted.evaluate(&[("x1", x1), ("x2", x2)]),
                    original.evaluate(&[("x1", x1), ("x2", x2)]),
                ) else {
                    continue;
                };
                // NaN on either side means the point is outside the domain, and
                // the pool bins it regardless of what either form says.
                if a.is_nan() || b.is_nan() {
                    continue;
                }
                assert!(
                    !(a > 0.0 && b <= 0.0),
                    "{source:?} rejected x1 = {x1}, which {equivalent:?} accepts —                      the inversion is narrower than the constraint it replaced"
                );
                if a <= 0.0 && b > 0.0 {
                    widened += 1;
                }
            }
            // A grid step landing within a ulp of the boundary picks up the
            // nudge; several would mean the bound itself is in the wrong place.
            assert!(
                widened <= 2,
                "{source:?} accepted {widened} points {equivalent:?} rejects,                  which is more than boundary rounding explains"
            );
        }
    }

    /// `ln(x) < 2` bounds `x` from above, which does not carry `x > 0` with it.
    /// The pass has to say both, and "and" is `max`.
    #[test]
    fn an_upper_bound_keeps_the_domain() {
        let expression = crate::compile("ln(x1) < 2").expect("should compile");

        // Inside the domain and under the bound: satisfied.
        assert!(expression.evaluate(&[("x1", 1.0)]).unwrap() <= 0.0);
        // Over the bound.
        assert!(expression.evaluate(&[("x1", 100.0)]).unwrap() > 0.0);
        // Outside the domain. Without the guard this would read as satisfied,
        // and a solver could then report an `unsat` that is not true.
        assert!(
            expression.evaluate(&[("x1", -5.0)]).unwrap() > 0.0,
            "a negative argument passed a logarithm constraint"
        );
    }

    /// A constant outside what the function can produce is not invertible.
    /// `atan(x) > 2` is unsatisfiable; `x > tan(2)` is `x > -2.18`, which is
    /// almost always true. Refusing is the only safe answer available.
    #[test]
    fn a_constant_outside_the_range_is_left_alone() {
        for source in [
            "atan(x1) > 2",
            "atan(x1) < -2",
            "tanh(x1) > 1.5",
            "sqrt(x1) < -1",
        ] {
            let expression = crate::compile(source).expect("should compile");
            assert!(
                holds_comparison(&expression.program.body.result)
                    || mentions_any_unary(&expression.program.body.result),
                "{source:?} was inverted when it should not have been"
            );
        }

        // And the meaning is unchanged, which is the part that matters.
        let unsatisfiable = crate::compile("atan(x1) > 2").expect("should compile");
        for step in -100..=100 {
            let x1 = f64::from(step);
            assert!(
                unsatisfiable.evaluate(&[("x1", x1)]).unwrap() > 0.0,
                "atan({x1}) > 2 should never hold"
            );
        }
    }

    fn mentions_any_unary(node: &Expr) -> bool {
        match &node.kind {
            Kind::Unary { .. } => true,
            Kind::Binary { lhs, rhs, .. } | Kind::Compare { lhs, rhs, .. } => {
                mentions_any_unary(lhs) || mentions_any_unary(rhs)
            }
            _ => false,
        }
    }

    /// Nothing to invert against. `ln(x1) > x2` has no constant side, so the
    /// pass must leave it for the emitter to report as untranslatable.
    #[test]
    fn a_variable_bound_is_left_alone() {
        let expression = crate::compile("ln(x1) > x2").expect("should compile");
        assert!(
            mentions_unary(&expression.program.body.result, UnaryOp::Ln),
            "a logarithm against a variable was inverted"
        );
    }

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
