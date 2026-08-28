//! Tree-walking evaluator.
//!
//! Deliberately simple and deliberately permanent. When the flattened tape
//! arrives it will need an oracle, and this is a better one than the Kotlin
//! implementation ever was: same language, same libm, no FFI, and it isolates
//! exactly the layer where the risky optimization lives. Do not delete it when
//! the tape works.

use crate::ast::{AggregateKind, BinaryOp, Block, Expr, Kind, Program, UnaryOp, to_index};
use crate::diagnostics::{BoundKind, ProblemKind, Span};

/// A failure raised during evaluation, before it has been rendered.
///
/// The evaluator deliberately does not build a
/// [`Problem`](crate::diagnostics::Problem): that needs the source text to
/// derive line and column, and threading a string through the evaluation path
/// is exactly what the eventual flattened tape does not want. It reports what
/// it knows — a kind and a location — and [`Bound::evaluate`](crate::Bound)
/// turns that into the public error.
pub(crate) struct RuntimeFault {
    pub kind: ProblemKind,
    pub span: Span,
}

/// Evaluates `program` for a single row.
///
/// * `globals` — values for the expression's statically-referenced symbols,
///   ordered by [`GlobalId`](crate::ast::GlobalId). `bind` guarantees this has
///   one entry per symbol, so indexing it is an internal invariant.
/// * `row` — the full schema-ordered row, which `var[i]` will index into once
///   dynamic lookup lands.
///
/// One flat frame serves the whole tree. Slots are allocated monotonically
/// during translation and never reused, so a nested block cannot clobber a
/// binding of an enclosing one, and no scope bookkeeping is needed here.
///
/// # Errors
/// Returns [`EvalError`] for constructs the evaluator cannot handle. Nothing
/// reachable can fail yet, because translation rejects everything else first.
pub(crate) fn evaluate(
    program: &Program,
    globals: &[f64],
    row: &[f64],
) -> Result<f64, RuntimeFault> {
    let mut frame = vec![f64::NAN; program.frame_size as usize];
    eval_block(&program.body, globals, row, &mut frame)
}

fn eval_block(
    block: &Block,
    globals: &[f64],
    row: &[f64],
    frame: &mut [f64],
) -> Result<f64, RuntimeFault> {
    for assignment in &block.assignments {
        frame[assignment.slot.index()] = eval_expr(&assignment.value, globals, row, frame)?;
    }
    eval_expr(&block.result, globals, row, frame)
}

/// Takes the frame mutably only because [`Kind::Block`] puts a binding block in
/// expression position — which is how lambda bodies will arrive. Expressions
/// themselves never write to it.
fn eval_expr(
    node: &Expr,
    globals: &[f64],
    row: &[f64],
    frame: &mut [f64],
) -> Result<f64, RuntimeFault> {
    Ok(match &node.kind {
        Kind::Literal(value) => *value,
        Kind::Global(id) => globals[id.index()],
        Kind::Local(slot) => frame[slot.index()],
        Kind::Unary { op, arg } => apply_unary(*op, eval_expr(arg, globals, row, frame)?),
        Kind::Binary { op, lhs, rhs } => {
            let lhs = eval_expr(lhs, globals, row, frame)?;
            let rhs = eval_expr(rhs, globals, row, frame)?;
            apply_binary(*op, lhs, rhs)
        }
        Kind::Block(block) => eval_block(block, globals, row, frame)?,

        // `sum(lower, upper, param -> body)`, folded with the kind's identity.
        //
        // Bounds evaluate in source order. The JVM implementation did upper
        // first, which only shows when both are bad and changes which error is
        // reported; source order is less surprising.
        Kind::Aggregate {
            kind,
            lower,
            upper,
            param,
            body,
        } => {
            let lower = eval_bound(lower, BoundKind::Lower, globals, row, frame)?;
            let upper = eval_bound(upper, BoundKind::Upper, globals, row, frame)?;

            let mut accumulated = kind.identity();
            for index in lower..=upper {
                // The "and back" half of the conversion: the loop counter is a
                // real integer, but the body sees an ordinary scalar.
                #[allow(clippy::cast_precision_loss)]
                {
                    frame[param.index()] = index as f64;
                }
                let term = eval_block(body, globals, row, frame)?;
                accumulated = match kind {
                    AggregateKind::Sum => accumulated + term,
                    AggregateKind::Prod => accumulated * term,
                };
            }
            accumulated
        }

        // `var[i]` — one-based, into the whole schema-ordered row rather than
        // the expression's own symbols, so it can read a variable the
        // expression never names.
        Kind::DynamicIndex(subscript) => {
            let value = eval_expr(subscript, globals, row, frame)?;
            let requested_1index = to_index(value).ok_or_else(|| {
                fault(
                    ProblemKind::DynamicIndexNotAnInteger { value },
                    subscript.span,
                )
            })?;

            // One-based, so `var[0]` lands on -1 and this single check covers
            // zero and negatives as well as overrun.
            let position = usize::try_from(requested_1index - 1)
                .ok()
                .filter(|position| *position < row.len())
                .ok_or_else(|| {
                    fault(
                        ProblemKind::DynamicIndexOutOfBounds {
                            requested_1index,
                            available: row.len(),
                        },
                        subscript.span,
                    )
                })?;

            row[position]
        }

        // Translation rejects these before they can reach here.
        Kind::Compare { .. } | Kind::NearEq { .. } => {
            unreachable!("translation never produces {:?}", node.kind)
        }
    })
}

/// Evaluates an aggregate bound and coerces it to an index.
///
/// An empty range (`lower > upper`) is not an error — the fold simply yields
/// the identity, matching the JVM implementation, whose loop did not run.
fn eval_bound(
    bound: &Expr,
    which: BoundKind,
    globals: &[f64],
    row: &[f64],
    frame: &mut [f64],
) -> Result<i64, RuntimeFault> {
    let value = eval_expr(bound, globals, row, frame)?;
    to_index(value).ok_or_else(|| {
        fault(
            ProblemKind::IllegalAggregateBound {
                bound: which,
                value,
            },
            // The bound's own span, not the enclosing aggregate's — otherwise a
            // multi-line `sum` reports its caret on the wrong line.
            bound.span,
        )
    })
}

const fn fault(kind: ProblemKind, span: Span) -> RuntimeFault {
    RuntimeFault { kind, span }
}

fn apply_unary(op: UnaryOp, x: f64) -> f64 {
    match op {
        UnaryOp::Negate => -x,
        UnaryOp::Cos => x.cos(),
        UnaryOp::Sin => x.sin(),
        UnaryOp::Tan => x.tan(),
        UnaryOp::Acos => x.acos(),
        UnaryOp::Asin => x.asin(),
        UnaryOp::Atan => x.atan(),
        UnaryOp::Cosh => x.cosh(),
        UnaryOp::Sinh => x.sinh(),
        UnaryOp::Tanh => x.tanh(),
        UnaryOp::Cot => 1.0 / x.tan(),
        UnaryOp::Ln => x.ln(),
        UnaryOp::Log10 => x.log10(),
        UnaryOp::Abs => x.abs(),
        UnaryOp::Sqrt => x.sqrt(),
        UnaryOp::Cbrt => x.cbrt(),
        UnaryOp::Sqr => x * x,
        UnaryOp::Cube => x * x * x,
        UnaryOp::Ceil => x.ceil(),
        UnaryOp::Floor => x.floor(),
        // Java's Math.signum returns +/-0.0 for +/-0.0 and NaN for NaN;
        // Rust's f64::signum returns 1.0 for +0.0 and -1.0 for -0.0 and NaN.
        // Babel's semantics are Java's, so preserve them.
        UnaryOp::Sgn => {
            if x == 0.0 || x.is_nan() {
                x
            } else {
                x.signum()
            }
        }
    }
}

fn apply_binary(op: BinaryOp, a: f64, b: f64) -> f64 {
    match op {
        BinaryOp::Add => a + b,
        BinaryOp::Sub => a - b,
        BinaryOp::Mul => a * b,
        BinaryOp::Div => a / b,
        BinaryOp::Mod => a % b,
        BinaryOp::Pow => a.powf(b),
        // Java's Math.max/min propagate NaN; Rust's f64::max/min discard it.
        // Babel's semantics are Java's.
        BinaryOp::Max => nan_or(a, b, f64::max),
        BinaryOp::Min => nan_or(a, b, f64::min),
        // log(base, x) == ln(x) / ln(base)
        BinaryOp::LogB => b.ln() / a.ln(),
    }
}

fn nan_or(a: f64, b: f64, f: impl Fn(f64, f64) -> f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        f(a, b)
    }
}

#[cfg(test)]
mod tests {
    /// An empty range folds to the identity rather than erroring or hanging.
    /// Nothing in the corpus has `lower > upper`.
    #[test]
    fn an_empty_aggregate_range_yields_the_identity() {
        let sum = crate::compile("sum(5, 1, i -> i)").expect("should compile");
        assert_eq!(sum.evaluate(&[]).expect("should evaluate"), 0.0);

        let product = crate::compile("prod(5, 1, i -> i)").expect("should compile");
        assert_eq!(product.evaluate(&[]).expect("should evaluate"), 1.0);
    }

    /// A span points at the offending sub-expression, not at the whole
    /// expression. `0/x1` sits at characters 4..8 of `sum(0/x1, 20, i -> i + 2)`.
    #[test]
    fn a_fault_is_located_at_the_offending_sub_expression() {
        let expression = crate::compile("sum(0/x1, 20, i -> i + 2)").expect("should compile");
        let error = expression
            .evaluate(&[("x1", 0.0)])
            .expect_err("0/0 is not a bound");

        match error {
            crate::EvalError::Runtime(problem) => {
                assert_eq!(problem.problem.span, crate::Span::new(4, 8));
                assert_eq!(problem.problem.line_idx, 0);
                assert_eq!(problem.problem.column_idx, 4);
                // Populated at the boundary, which is the only place that knows
                // the schema.
                assert_eq!(problem.parameters, vec![("x1".to_owned(), 0.0)]);
            }
            other => panic!("expected a runtime problem, got {other:?}"),
        }
    }

    /// Offsets count characters, not bytes. `测试` is two characters and six
    /// UTF-8 bytes, so a byte-based span would report 4..10 here.
    #[test]
    fn spans_count_characters_not_bytes() {
        let expression = crate::compile("sum(测试, 20, i -> i)").expect("should compile");
        let error = expression
            .evaluate(&[("测试", 1.5)])
            .expect_err("1.5 is not a bound");

        match error {
            crate::EvalError::Runtime(problem) => {
                assert_eq!(problem.problem.span, crate::Span::new(4, 6));
                assert_eq!(problem.problem.column_idx, 4);
            }
            other => panic!("expected a runtime problem, got {other:?}"),
        }
    }

    /// Subscripts are one-based, so zero is out of range rather than the first
    /// element. The same check covers negatives.
    #[test]
    fn a_zero_subscript_is_out_of_bounds() {
        let expression = crate::compile("var[0]").expect("should compile");
        let error = expression
            .evaluate(&[("x1", 7.0)])
            .expect_err("var[0] is not the first parameter");

        match error {
            crate::EvalError::Runtime(problem) => assert_eq!(
                problem.problem.kind,
                crate::ProblemKind::DynamicIndexOutOfBounds {
                    requested_1index: 0,
                    available: 1,
                }
            ),
            other => panic!("expected a runtime problem, got {other:?}"),
        }
    }

    /// Strict here too, for the same reason as the aggregate bounds: the JVM
    /// implementation rounded, so `var[1.7]` silently became `var[2]`.
    #[test]
    fn a_non_integral_subscript_is_an_error() {
        let expression = crate::compile("var[1.5]").expect("should compile");
        let error = expression
            .evaluate(&[("x1", 7.0), ("x2", 8.0)])
            .expect_err("1.5 is not an index and must not be rounded");

        match error {
            crate::EvalError::Runtime(problem) => assert!(
                matches!(
                    problem.problem.kind,
                    crate::ProblemKind::DynamicIndexNotAnInteger { .. }
                ),
                "expected a non-integer subscript, got {:?}",
                problem.problem.kind
            ),
            other => panic!("expected a runtime problem, got {other:?}"),
        }
    }

    /// Strictness, which is the whole point of `to_index`. The JVM
    /// implementation rounded, so this silently summed 1..=2 instead.
    #[test]
    fn a_non_integral_bound_is_an_error() {
        let expression = crate::compile("sum(1, 1.5, i -> i)").expect("should compile");
        let error = expression
            .evaluate(&[])
            .expect_err("1.5 is not an index and must not be rounded");

        match error {
            crate::EvalError::Runtime(problem) => assert!(
                matches!(
                    problem.problem.kind,
                    crate::ProblemKind::IllegalAggregateBound { .. }
                ),
                "expected an illegal bound, got {:?}",
                problem.problem.kind
            ),
            other => panic!("expected a runtime problem, got {other:?}"),
        }
    }
}
