//! The per-lane executor: one sample, scalar registers, loops allowed.
//!
//! This is what `eval_row` runs, and what `eval` falls back to for a tape
//! with a run-time loop. It is not a second implementation of the language:
//! the operators are the AST's own `apply`, and the instruction semantics are
//! shared with the batched executor, which runs the same tape a slice at a
//! time.

use crate::ast::to_index;
use crate::diagnostics::Fault;

use super::EPSILON;
use super::tape::{FaultKind, Insn, LaneFault, Tape};

/// The single place a `var[i]` subscript becomes a row position.
///
/// One-based, so `var[0]` lands on `-1` and the one range check covers zero
/// and negatives as well as overrun — the walker's chain, kept verbatim.
pub(crate) fn resolve_index(value: f64, available: usize) -> Result<usize, FaultKind> {
    let requested_1index = to_index(value).ok_or(FaultKind::NotAnInteger(value))?;
    usize::try_from(requested_1index - 1)
        .ok()
        .filter(|position| *position < available)
        .ok_or(FaultKind::OutOfBounds {
            requested_1index,
            available,
        })
}

/// Evaluates `tape` for one row. `frame` must be [`Tape::prime`]d.
pub(crate) fn run_lane(tape: &Tape, row: &[f64], frame: &mut [f64]) -> Result<f64, Fault> {
    let fault = |pc: usize, kind: FaultKind| {
        tape.fault(LaneFault {
            insn: u32::try_from(pc).expect("fewer than 2^32 instructions"),
            kind,
        })
    };
    let checked = |pc: usize, value: f64| {
        if value.is_finite() {
            Ok(value)
        } else {
            Err(fault(pc, FaultKind::NonFinite(value)))
        }
    };

    // (next index, last index) per open loop.
    let mut loops: Vec<(i64, i64)> = Vec::new();
    let mut pc = 0usize;
    while pc < tape.insns.len() {
        match tape.insns[pc] {
            Insn::Load { dst, input } => {
                frame[dst.index()] = checked(pc, row[input as usize])?;
            }
            Insn::Copy { dst, src } => frame[dst.index()] = frame[src.index()],
            Insn::Unary { dst, op, a } => {
                frame[dst.index()] = checked(pc, op.apply(frame[a.index()]))?;
            }
            Insn::Binary { dst, op, a, b } => {
                frame[dst.index()] = checked(pc, op.apply(frame[a.index()], frame[b.index()]))?;
            }
            Insn::Compare { dst, op, a, b } => {
                frame[dst.index()] = checked(pc, compare(op, frame[a.index()], frame[b.index()]))?;
            }
            Insn::NearEq {
                dst,
                a,
                b,
                tolerance,
            } => {
                frame[dst.index()] = checked(
                    pc,
                    near_eq(frame[a.index()], frame[b.index()], frame[tolerance.index()]),
                )?;
            }
            Insn::Combine {
                dst,
                how,
                a,
                b,
                last,
            } => {
                let value = how.apply(frame[a.index()], frame[b.index()]);
                frame[dst.index()] = if last { checked(pc, value)? } else { value };
            }
            Insn::Check { reg } => {
                checked(pc, frame[reg.index()])?;
            }
            Insn::Gather { dst, index, .. } => {
                let position =
                    resolve_index(frame[index.index()], row.len()).map_err(|k| fault(pc, k))?;
                frame[dst.index()] = checked(pc, row[position])?;
            }
            Insn::Bound { reg, which } => {
                let value = frame[reg.index()];
                if to_index(value).is_none() {
                    return Err(fault(pc, FaultKind::Bound(which, value)));
                }
            }
            Insn::LoopStart {
                lower,
                upper,
                param,
                acc,
                kind,
                end,
            } => {
                let lo = to_index(frame[lower.index()]).expect("Bound validated the lower bound");
                let hi = to_index(frame[upper.index()]).expect("Bound validated the upper bound");
                frame[acc.index()] = kind.identity();
                if lo > hi {
                    // An empty range is the identity, not an error.
                    pc = end as usize + 1;
                    continue;
                }
                loops.push((lo, hi));
                frame[param.index()] = index_as_f64(lo);
            }
            Insn::LoopEnd { start, acc, term } => {
                let Insn::LoopStart { param, kind, .. } = tape.insns[start as usize] else {
                    unreachable!("LoopEnd.start names a LoopStart")
                };
                frame[acc.index()] = kind.combine(frame[acc.index()], frame[term.index()]);
                let open = loops.last_mut().expect("a loop is open");
                open.0 += 1;
                if open.0 <= open.1 {
                    frame[param.index()] = index_as_f64(open.0);
                    pc = start as usize + 1;
                    continue;
                }
                loops.pop();
            }
        }
        pc += 1;
    }
    Ok(frame[tape.result.index()])
}

/// The residual of `a op b` under the `<= 0` convention — the walker's four
/// expressions, verbatim, so the nudge lands in the same place.
pub(crate) fn compare(op: crate::ast::CompareOp, left: f64, right: f64) -> f64 {
    use crate::ast::CompareOp;
    match op {
        CompareOp::Lte => left - right,
        CompareOp::Gte => right - left,
        CompareOp::Lt => (left - right) + EPSILON,
        CompareOp::Gt => (right - left) + EPSILON,
    }
}

/// `|left - right| <= tolerance` as the larger of the two one-sided
/// residuals, through `Max.apply` so NaN propagates as Java's does.
pub(crate) fn near_eq(left: f64, right: f64, tolerance: f64) -> f64 {
    let at_least = (right - tolerance) - left;
    let at_most = left - (right + tolerance);
    crate::ast::BinaryOp::Max.apply(at_least, at_most)
}

/// The "and back" half of the index conversion: the loop counter is an
/// integer, the body sees a scalar. Same cast as the walker made.
#[expect(
    clippy::cast_precision_loss,
    reason = "indices are bounded by to_index's 2^53 limit"
)]
fn index_as_f64(index: i64) -> f64 {
    index as f64
}

#[cfg(test)]
#[path = "lane_tests.rs"]
mod tests;
