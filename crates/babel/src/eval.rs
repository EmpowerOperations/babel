//! Tree-walking evaluator.
//!
//! Deliberately simple and deliberately permanent. When the flattened tape
//! arrives it will need an oracle, and this is a better one than the Kotlin
//! implementation ever was: same language, same libm, no FFI, and it isolates
//! exactly the layer where the risky optimization lives. Do not delete it when
//! the tape works.

use crate::ast::Program;
use crate::diagnostics::EvalError;

/// Evaluates `program` for a single row.
///
/// * `globals` — values for the expression's statically-referenced symbols,
///   ordered by [`GlobalIdx`](crate::ast::GlobalIdx).
/// * `row` — the full schema-ordered row, which `var[i]` indexes into.
///
/// # Errors
/// Returns [`EvalError`] on out-of-bounds `var[i]`, unusable aggregate bounds,
/// or a `Compare`/`NearEq` node that the boolean rewrite should have removed.
pub(crate) fn evaluate(
    _program: &Program,
    _globals: &[f64],
    _row: &[f64],
) -> Result<f64, EvalError> {
    todo!("V0")
}
