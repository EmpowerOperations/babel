//! The evaluator: an `Ast` lowered to a flat tape, run a tile of samples at a
//! time or one row at a time.
//!
//! `tape.rs` is the instruction set, `lower.rs` produces it, `regalloc.rs`
//! packs its temporaries, and the two executors — `tile.rs` for a batch,
//! `lane.rs` for a row — run it. Every tape is straight-line: the front end
//! unrolls `sum` and `prod` or refuses them, so nothing here loops. It replaced a tree-walking evaluator, and was
//! held to that walker's answers bit for bit over a few thousand random and
//! adversarial rows before the walker was deleted; what remains as the spec is
//! the corpus, the runtime-error tests and `tests/special_values.rs`.

mod lane;
mod lower;
mod regalloc;
mod simd;
mod tape;
mod tile;

/// Which instruction set the tile kernels run on here, and how many `f64`
/// lanes that is: `("pulp::x86::v3::V3", 4)` on an AVX2 machine,
/// `("pulp::Scalar", 1)` without one. For the benchmark ledgers' host file.
#[doc(hidden)]
#[must_use]
pub fn simd_isa() -> (&'static str, usize) {
    struct Probe;
    impl pulp::WithSimd for Probe {
        type Output = (&'static str, usize);
        #[inline(always)]
        fn with_simd<S: pulp::Simd>(self, _: S) -> Self::Output {
            (std::any::type_name::<S>(), S::F64_LANES)
        }
    }
    pulp::Arch::new().dispatch(Probe)
}

use faer::{Col, Mat, MatRef};

use crate::diagnostics::{BindError, Fault, Problem, RuntimeProblem};
use crate::{Ast, EvalError, Schema};

use tape::Tape;
use tile::{RegisterFile, TILE};

/// Java's `Double.MIN_NORMAL`, the nudge that makes a *strict* inequality
/// representable when `<= 0` means true.
///
/// It is meant to vanish into rounding at any meaningful magnitude — `(4 - 6) + ε`
/// is exactly `-2.0` — and to survive only when the difference is zero, which is
/// precisely when strict and non-strict differ: `(6 - 6) + ε` is `ε`, which is
/// `> 0`, so `6 > 6` is false.
pub(crate) const EPSILON: f64 = f64::MIN_POSITIVE;

/// Resolves an [`Ast`]'s symbols against a [`Schema`] and lowers it, ready to
/// evaluate.
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

    let tape = lower::lower(&ast.program, &global_positions, schema.len());

    Ok(CompiledExpression {
        tape,
        source: ast.source.clone(),
        schema: schema.clone(),
    })
}

/// An [`Ast`] resolved against a [`Schema`] and lowered, ready to run over a
/// batch.
///
/// **Owned**, not borrowed. It costs one lowering per schema — paid once, at
/// compile time — and buys a type with no lifetime, which matters because
/// `cvg` holds a great many of these inside a worker that outlives every
/// borrow it could have taken.
#[derive(Debug, Clone)]
pub struct CompiledExpression {
    tape: Tape,
    /// Held for diagnostics: a runtime failure renders a caret against it.
    source: String,
    /// Gives the expected row count, and the names a failure reports values by.
    schema: Schema,
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
    /// Runs a tile of [`TILE`] columns at a time. The first failing column, in
    /// order, is the one reported, naming the innermost subexpression that
    /// went wrong.
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
        self.check_width(samples)?;
        let columns = samples.ncols();
        let mut residuals = Col::zeros(columns);

        let mut tiles = Tiles::new(&self.tape, columns);
        while let Some((first, lanes)) = tiles.next_tile(columns) {
            if let Some((lane, fault)) = tiles.run(&self.tape, samples, first, lanes) {
                let column = first + lane;
                let row = column_of(samples, column);
                return Err(self.runtime_failure(&self.tape.fault(fault), Some(column), &row));
            }
            for (lane, &value) in tiles.results(&self.tape, lanes).iter().enumerate() {
                residuals[first + lane] = value;
            }
        }

        Ok(residuals)
    }

    /// Narrows `holds` to the columns of `samples` on which this constraint is
    /// satisfied: `holds[c]` stays `true` only if it was, and the residual for
    /// column `c` is `<= 0`, and nothing faulted computing it.
    ///
    /// The pool's question, asked of thousands of independent candidates at
    /// once. A candidate whose evaluation faults — `sqrt` of a negative, a
    /// subscript out of range — is a candidate the constraint does not hold
    /// for, and nobody else's problem; it is judged from the fault table, not
    /// from the value, so an intermediate infinity that a later `atan` would
    /// have absorbed still counts. No value leaves this function.
    ///
    /// # Errors
    /// [`EvalError::RowWidthMismatch`] if `samples` has the wrong number of
    /// rows or `holds` the wrong number of columns.
    pub(crate) fn holds(
        &self,
        samples: MatRef<'_, f64>,
        holds: &mut [bool],
    ) -> Result<(), EvalError> {
        self.check_width(samples)?;
        let columns = samples.ncols();
        if holds.len() != columns {
            return Err(EvalError::RowWidthMismatch {
                expected: columns,
                actual: holds.len(),
            });
        }

        let mut tiles = Tiles::new(&self.tape, columns);
        while let Some((first, lanes)) = tiles.next_tile(columns) {
            tiles.run(&self.tape, samples, first, lanes);
            let verdicts = tiles.results(&self.tape, lanes);
            let faults = &tiles.faults[..lanes];
            for (hold, (&residual, fault)) in holds[first..first + lanes]
                .iter_mut()
                .zip(verdicts.iter().zip(faults))
            {
                *hold &= fault.is_none() && residual <= 0.0;
            }
        }
        Ok(())
    }

    fn check_width(&self, samples: MatRef<'_, f64>) -> Result<(), EvalError> {
        if samples.nrows() == self.schema.len() {
            Ok(())
        } else {
            Err(EvalError::RowWidthMismatch {
                expected: self.schema.len(),
                actual: samples.nrows(),
            })
        }
    }

    /// One residual for one row.
    ///
    /// The per-lane executor over the same tape, for the one consumer that
    /// *cannot* batch: the walker's shrinkage loop cannot propose the next
    /// candidate until it has judged this one, so it is sequential by nature,
    /// and wrapping each point in a one-column matrix cost about five times
    /// the evaluation itself — measured, `p118` at 32s against 6s.
    ///
    /// Not a second implementation. It is the same tape with a different loop
    /// around it, which is why it stays private and why there is still no
    /// scalar entry point in the public API.
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
        let mut frame = vec![0.0; self.tape.registers as usize];
        self.tape.prime(&mut frame);
        lane::run_lane(&self.tape, row, &mut frame)
            .map_err(|fault| self.runtime_failure(&fault, None, row))
    }

    /// The tape, for the lowering and allocation tests.
    #[cfg(test)]
    pub(super) fn tape(&self) -> &Tape {
        &self.tape
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

/// Parses and lowers `source` against `names`, for the tests of the pieces.
#[cfg(test)]
pub(super) fn tape_for(source: &str, names: &[&str]) -> Tape {
    let ast = crate::parse(source).unwrap_or_else(|e| panic!("{source:?}: {e}"));
    compile(&ast, &Schema::new(names.iter().copied()))
        .unwrap_or_else(|e| panic!("{source:?} against {names:?}: {e:?}"))
        .tape()
        .clone()
}

/// The batched walk over a straight-line tape: a register file and a fault
/// table sized for one tile and reused across all of them, two allocations
/// per call.
struct Tiles {
    file: RegisterFile,
    faults: Vec<Option<tape::LaneFault>>,
    next: usize,
}

impl Tiles {
    fn new(tape: &Tape, columns: usize) -> Self {
        let width = columns.min(TILE);
        Self {
            file: RegisterFile::new(tape, width),
            faults: vec![None; width],
            next: 0,
        }
    }

    /// The next tile's first column and width, until the columns run out.
    fn next_tile(&mut self, columns: usize) -> Option<(usize, usize)> {
        if self.next >= columns {
            return None;
        }
        let first = self.next;
        let lanes = (columns - first).min(TILE);
        self.next += lanes;
        Some((first, lanes))
    }

    fn run(
        &mut self,
        tape: &Tape,
        samples: MatRef<'_, f64>,
        first: usize,
        lanes: usize,
    ) -> Option<(usize, tape::LaneFault)> {
        self.faults[..lanes].fill(None);
        tile::run_tile(
            tape,
            samples,
            first,
            lanes,
            &mut self.file,
            &mut self.faults[..lanes],
        )
    }

    fn results(&self, tape: &Tape, lanes: usize) -> &[f64] {
        self.file.reg(tape.result, lanes)
    }
}

/// One column of `samples` as a row, for a diagnostic's parameter list.
fn column_of(samples: MatRef<'_, f64>, column: usize) -> Vec<f64> {
    (0..samples.nrows())
        .map(|index| samples[(index, column)])
        .collect()
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
    /// expression. `0/x1` sits at characters 4..8 of `abs(0/x1)`.
    #[test]
    fn a_fault_is_located_at_the_offending_sub_expression() {
        let expression = crate::parse("abs(0/x1)").expect("should compile");
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
        let expression = crate::parse("abs(测试) + ln(x1)").expect("should compile");
        let error =
            eval_one(&expression, &[("测试", 1.5), ("x1", 0.0)]).expect_err("ln(0) should fault");

        match error {
            crate::EvalError::Runtime(problem) => {
                // `ln(x1)` starts after `abs(测试) + `, ten characters in.
                assert_eq!(problem.problem.span, crate::Span::new(10, 16));
                assert_eq!(problem.problem.column_idx, 10);
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
}
