//! Tree-walking evaluator.
//!
//! Deliberately simple and deliberately permanent. When the flattened tape
//! arrives it will need an oracle, and this is a better one than the Kotlin
//! implementation ever was: same language, same libm, no FFI, and it isolates
//! exactly the layer where the risky optimization lives. Do not delete it when
//! the tape works.

use crate::ast::{BinaryOp, Block, Expr, Kind, Program, UnaryOp};
use crate::diagnostics::EvalError;

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
pub(crate) fn evaluate(program: &Program, globals: &[f64], row: &[f64]) -> Result<f64, EvalError> {
    let mut frame = vec![f64::NAN; program.frame_size as usize];
    eval_block(&program.body, globals, row, &mut frame)
}

fn eval_block(
    block: &Block,
    globals: &[f64],
    row: &[f64],
    frame: &mut [f64],
) -> Result<f64, EvalError> {
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
) -> Result<f64, EvalError> {
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

        // Translation rejects all of these before they can reach here.
        Kind::DynamicIndex(_)
        | Kind::Aggregate { .. }
        | Kind::Compare { .. }
        | Kind::NearEq { .. } => {
            unreachable!("translation never produces {:?}", node.kind)
        }
    })
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
