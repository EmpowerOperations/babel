//! Tree-walking evaluator.
//!
//! Deliberately simple and deliberately permanent. When the flattened tape
//! arrives it will need an oracle, and this is a better one than the Kotlin
//! implementation ever was: same language, same libm, no FFI, and it isolates
//! exactly the layer where the risky optimization lives. Do not delete it when
//! the tape works.

use faer::{Col, Mat, MatRef};

use crate::ast::{self, BinaryOp, Block, CompareOp, Expr, Kind, Program, to_index};
use crate::diagnostics::{BindError, BoundKind, Fault, Problem, ProblemKind, RuntimeProblem, Span};
use crate::{Ast, EvalError, Schema};

/// Java's `Double.MIN_NORMAL`, the nudge that makes a *strict* inequality
/// representable when `<= 0` means true.
///
/// It is meant to vanish into rounding at any meaningful magnitude — `(4 - 6) + ε`
/// is exactly `-2.0` — and to survive only when the difference is zero, which is
/// precisely when strict and non-strict differ: `(6 - 6) + ε` is `ε`, which is
/// `> 0`, so `6 > 6` is false.
pub(crate) const EPSILON: f64 = f64::MIN_POSITIVE;

/// Resolves an [`Ast`]'s symbols against a [`Schema`], ready to evaluate.
///
/// A free function rather than a method on [`Ast`], because a tree that knows
/// how to compile itself is not a data type. The AST is the shared middle;
/// this module is one of two backends that consume it.
///
/// This is where missing values are reported — once per schema, rather than on
/// every evaluation as the JVM implementation did.
///
/// # Errors
/// Returns [`BindError`] if the schema omits a symbol the expression needs.
pub fn compile(ast: &Ast, schema: &Schema) -> Result<CompiledExpression, BindError> {
    let mut global_positions = Vec::with_capacity(ast.symbols.len());
    let mut missing = Vec::new();

    for symbol in &ast.symbols {
        match schema.names.iter().position(|name| name == symbol) {
            Some(position) => {
                global_positions.push(u32::try_from(position).unwrap_or(u32::MAX));
            }
            None => missing.push(symbol.clone()),
        }
    }

    if !missing.is_empty() {
        return Err(BindError { missing });
    }

    Ok(CompiledExpression {
        program: ast.program.clone(),
        source: ast.source.clone(),
        schema: schema.clone(),
        global_positions,
    })
}

/// An [`Ast`] resolved against a [`Schema`] and ready to run over a batch.
///
/// **Owned**, not borrowed. It costs one clone of the program per schema — paid
/// once, at compile time — and buys a type with no lifetime, which matters
/// because `cvg` holds a great many of these inside a worker that outlives
/// every borrow it could have taken.
///
/// This is also the seam the flattened tape grows from. Nothing outside this
/// module knows what is inside it, so replacing the tree walk with a tape is an
/// implementation change rather than an API one.
#[derive(Debug, Clone)]
pub struct CompiledExpression {
    program: ast::Program,
    /// Held for diagnostics: a runtime failure renders a caret against it.
    source: String,
    /// Gives the expected row count, and the names a failure reports values by.
    schema: Schema,
    /// `ast::GlobalId` -> row index.
    global_positions: Vec<u32>,
}

impl CompiledExpression {
    #[must_use]
    pub const fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Evaluates one residual per column of `samples`.
    ///
    /// **One column per sample, one row per schema variable.** That is the shape
    /// `cvg` produces, so a generated batch is directly an input matrix with no
    /// transpose — and it is the orientation `faer` stores contiguously, so a
    /// column is a sample laid out end to end.
    ///
    /// There is no single-sample entry point. There was, and it had no users
    /// outside its own tests: everything that evaluates babel evaluates many
    /// points at once, so a scalar path was a second implementation to keep
    /// correct for nobody's benefit. `Mat::from_fn(n, 1, …)` covers the
    /// one-off case.
    ///
    /// # Errors
    /// [`EvalError::RowWidthMismatch`] if `samples` has the wrong number of
    /// rows, or [`EvalError::Runtime`] naming the column and the subexpression
    /// where evaluation failed.
    pub fn eval(&self, samples: MatRef<'_, f64>) -> Result<Col<f64>, EvalError> {
        if samples.nrows() != self.schema.len() {
            return Err(EvalError::RowWidthMismatch {
                expected: self.schema.len(),
                actual: samples.nrows(),
            });
        }

        // Both buffers are reused across columns: two allocations per call
        // rather than two per sample, which is the whole reason this method
        // takes a batch.
        let mut row = vec![0.0; samples.nrows()];
        let mut globals = vec![0.0; self.global_positions.len()];
        let mut residuals = Col::zeros(samples.ncols());

        for column in 0..samples.ncols() {
            for (index, value) in row.iter_mut().enumerate() {
                *value = samples[(index, column)];
            }
            residuals[column] = self
                .residual(&row, &mut globals)
                .map_err(|fault| self.runtime_failure(&fault, Some(column), &row))?;
        }

        Ok(residuals)
    }

    /// One residual for one row, with the caller supplying the globals buffer.
    ///
    /// The body of [`eval`](Self::eval)'s loop, factored out so that the one
    /// consumer that *cannot* batch does not have to fake a batch to reach it.
    /// That consumer is the walker: its shrinkage loop cannot propose the next
    /// candidate until it has judged this one, so it is sequential by nature and
    /// wrapping each point in a one-column matrix cost about five times the
    /// evaluation itself — measured, `p118` at 32s against 6s.
    ///
    /// This is not a second implementation. It is the same walk the batch path
    /// runs; only the loop around it differs, which is why it stays private and
    /// why there is still no scalar entry point in the public API.
    fn residual(&self, row: &[f64], globals: &mut [f64]) -> Result<f64, Fault> {
        for (slot, &position) in globals.iter_mut().zip(&self.global_positions) {
            *slot = row[position as usize];
        }
        evaluate(&self.program, globals, row)
    }

    /// One residual for one row, for a caller that has no buffer to lend.
    ///
    /// # Errors
    /// As [`eval`](Self::eval), without a column to name.
    pub(crate) fn eval_row(&self, row: &[f64]) -> Result<f64, EvalError> {
        if row.len() != self.schema.len() {
            return Err(EvalError::RowWidthMismatch {
                expected: self.schema.len(),
                actual: row.len(),
            });
        }
        let mut globals = vec![0.0; self.global_positions.len()];
        self.residual(row, &mut globals)
            .map_err(|fault| self.runtime_failure(&fault, None, row))
    }

    /// Renders a [`Fault`] into the failure a caller sees.
    ///
    /// The evaluator reports a kind and a location; rendering needs the source,
    /// which it deliberately does not carry. Building the [`Problem`] here keeps
    /// line and column derived from `span.start` in the one place that does it
    /// for syntax errors too.
    fn runtime_failure(&self, fault: &Fault, column: Option<usize>, row: &[f64]) -> EvalError {
        EvalError::Runtime(Box::new(RuntimeProblem {
            problem: Problem::new(fault.kind.clone(), &self.source, fault.span),
            sample: column,
            // Needs a slot-to-name table the AST deliberately discards.
            locals: Vec::new(),
            parameters: self
                .schema
                .names()
                .iter()
                .cloned()
                .zip(row.iter().copied())
                .collect(),
        }))
    }
}

/// One expression at one point, as a one-column batch.
///
/// `#[doc(hidden)]` because it is a convenience for tests and one-offs, not a
/// path anything should evaluate through in a loop — the batch API exists
/// precisely so that nothing has to. Kept in the crate rather than duplicated
/// across four test files, and it is what proves a batch of one still agrees
/// with what the scalar evaluator used to return.
///
/// # Errors
/// Whatever [`compile`] or [`CompiledExpression::eval`] would return.
#[doc(hidden)]
pub fn eval_one(ast: &Ast, inputs: &[(&str, f64)]) -> Result<f64, EvalError> {
    let schema = Schema::new(inputs.iter().map(|(name, _)| *name));
    let compiled = compile(ast, &schema)?;
    let sample = Mat::from_fn(inputs.len(), 1, |row, _| inputs[row].1);
    Ok(compiled.eval(sample.as_ref())?[0])
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
pub(crate) fn evaluate(program: &Program, globals: &[f64], row: &[f64]) -> Result<f64, Fault> {
    let mut frame = vec![f64::NAN; program.frame_size as usize];
    eval_block(&program.body, globals, row, &mut frame)
}

fn eval_block(
    block: &Block,
    globals: &[f64],
    row: &[f64],
    frame: &mut [f64],
) -> Result<f64, Fault> {
    for assignment in &block.assignments {
        frame[assignment.slot.index()] = eval_expr(&assignment.value, globals, row, frame)?;
    }
    eval_expr(&block.result, globals, row, frame)
}

/// Takes the frame mutably only because [`Kind::Block`] puts a binding block in
/// expression position — which is how lambda bodies will arrive. Expressions
/// themselves never write to it.
fn eval_expr(node: &Expr, globals: &[f64], row: &[f64], frame: &mut [f64]) -> Result<f64, Fault> {
    let value = match &node.kind {
        Kind::Literal(value) => *value,
        Kind::Global(id) => globals[id.index()],
        Kind::Local(slot) => frame[slot.index()],
        Kind::Unary { op, arg } => op.apply(eval_expr(arg, globals, row, frame)?),
        Kind::Binary { op, lhs, rhs } => {
            let lhs = eval_expr(lhs, globals, row, frame)?;
            let rhs = eval_expr(rhs, globals, row, frame)?;
            op.apply(lhs, rhs)
        }
        Kind::Block(block) => eval_block(block, globals, row, frame)?,

        // An unrolled aggregate. Left-to-right from the identity, exactly as
        // the loop above accumulates, so unrolling cannot change a result.
        Kind::Fold { kind, terms } => {
            let mut accumulated = kind.identity();
            for term in terms {
                accumulated = kind.combine(accumulated, eval_expr(term, globals, row, frame)?);
            }
            accumulated
        }

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
                accumulated = kind.combine(accumulated, term);
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

        // ---- the boolean convention, which is this backend's alone ----
        //
        // Babel has no boolean values at run time. A comparison evaluates to
        // arithmetic whose *sign* carries the truth value: `<= 0` is true. So a
        // violated constraint reports how badly it was violated rather than
        // merely that it was, which is the canonical `g(x) <= 0` form an
        // optimizer wants.
        //
        // Computed here rather than rewritten into a tree beforehand, because
        // what is wanted is a *number*. The tree version cost an `Add` node per
        // strict comparison and, for `NearEq`, evaluated each side twice.
        Kind::Compare { op, lhs, rhs } => {
            let left = eval_expr(lhs, globals, row, frame)?;
            let right = eval_expr(rhs, globals, row, frame)?;
            match op {
                CompareOp::Lte => left - right,
                CompareOp::Gte => right - left,
                // Strictness rides on a nudge that vanishes into rounding at any
                // meaningful magnitude and survives only when the difference is
                // exactly zero — which is precisely where strict and non-strict
                // differ. `(4 - 6) + eps` is exactly `-2.0`; `(6 - 6) + eps` is
                // `eps`, which is `> 0`, so `6 > 6` is false.
                CompareOp::Lt => (left - right) + EPSILON,
                CompareOp::Gt => (right - left) + EPSILON,
            }
        }

        // `|a - b| <= t`, as the larger of the two one-sided residuals.
        Kind::NearEq {
            lhs,
            rhs,
            tolerance,
        } => {
            let left = eval_expr(lhs, globals, row, frame)?;
            let right = eval_expr(rhs, globals, row, frame)?;
            let at_least = (right - tolerance) - left;
            let at_most = left - (right + tolerance);
            // Through `apply` so that Java's NaN propagation stays defined in
            // exactly one place, next to `max` itself.
            BinaryOp::Max.apply(at_least, at_most)
        }

        // Conjunction is `max`: every term holds exactly when the largest
        // residual does. No identity worth naming — an empty `And` cannot be
        // built, since the only producer emits two terms.
        Kind::And { terms } => {
            let mut worst = f64::NEG_INFINITY;
            for term in terms {
                worst = BinaryOp::Max.apply(worst, eval_expr(term, globals, row, frame)?);
            }
            worst
        }
    };

    // Every node, not only the ones that can produce a non-finite value. The
    // producing set is nearly everything once infinities are in play — `0/0`,
    // `inf - inf`, `0 * inf`, `sqrt` of a negative — so checking selectively
    // would save little and miss two cases worth catching:
    //
    //   * a non-finite *input*, which fails here at its `Kind::Global` rather
    //     than travelling to wherever it first matters;
    //   * an unwritten frame slot, since `evaluate` fills the frame with NaN as
    //     a sentinel and reading one is a slot-allocation bug.
    //
    // A `Kind::Literal` cannot fail this: `rewrite::fold_constants` refuses a
    // non-finite constant at compile time. The two checks are the same rule in
    // the two phases that can see it.
    //
    // This is the whole of the policy, in one place, so relaxing it — should a
    // real use for a saturating infinity turn up — is a one-line change here
    // rather than an excavation.
    if !value.is_finite() {
        return Err(fault(ProblemKind::NonFiniteValue { value }, node.span));
    }

    Ok(value)
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
) -> Result<i64, Fault> {
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

const fn fault(kind: ProblemKind, span: Span) -> Fault {
    Fault { kind, span }
}

#[cfg(test)]
mod tests {
    use super::eval_one;

    /// An empty range folds to the identity rather than erroring or hanging.
    /// Nothing in the corpus has `lower > upper`.
    #[test]
    fn an_empty_aggregate_range_yields_the_identity() {
        let sum = crate::parse("sum(5, 1, i -> i)").expect("should compile");
        assert_eq!(eval_one(&sum, &[]).expect("should evaluate"), 0.0);

        let product = crate::parse("prod(5, 1, i -> i)").expect("should compile");
        assert_eq!(eval_one(&product, &[]).expect("should evaluate"), 1.0);
    }

    /// A span points at the offending sub-expression, not at the whole
    /// expression. `0/x1` sits at characters 4..8 of `sum(0/x1, 20, i -> i + 2)`.
    #[test]
    fn a_fault_is_located_at_the_offending_sub_expression() {
        let expression = crate::parse("sum(0/x1, 20, i -> i + 2)").expect("should compile");
        let error = eval_one(&expression, &[("x1", 0.0)]).expect_err("0/0 is not a bound");

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
        let expression = crate::parse("sum(测试, 20, i -> i)").expect("should compile");
        let error = eval_one(&expression, &[("测试", 1.5)]).expect_err("1.5 is not a bound");

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
        let expression = crate::parse("var[0]").expect("should compile");
        let error =
            eval_one(&expression, &[("x1", 7.0)]).expect_err("var[0] is not the first parameter");

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
        let expression = crate::parse("var[1.5]").expect("should compile");
        let error = eval_one(&expression, &[("x1", 7.0), ("x2", 8.0)])
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
    ///
    /// This exercises the *run-time* path: `x1` is a variable, so the bound
    /// cannot be folded and the aggregate stays a loop. With a literal bound the
    /// unroller now rejects it at compile time instead.
    #[test]
    fn a_non_integral_bound_is_an_error() {
        let expression = crate::parse("sum(x1, 20, i -> i)").expect("should compile");
        let error = eval_one(&expression, &[("x1", 1.5)])
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
