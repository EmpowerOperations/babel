//! The batched executor: one straight-line tape over a tile of samples, each
//! instruction a loop across the lanes.
//!
//! The loops are written as zipped slice iterators with no early exit and no
//! index arithmetic, which is the shape LLVM vectorises without being asked.
//! The operator is chosen *outside* the loop, so that `apply` inlines to the
//! raw operation inside it; the non-finite test is fused into the same pass
//! as an or-reduction, and only a tile that actually produced a non-finite
//! value pays for the slow walk that records which lanes did.
//!
//! Faults are recorded, not raised. NaN flows on through the remaining
//! instructions, every lane keeps its own first fault, and the lowest faulted
//! lane is reported at the end — the walker's "first failing column", with the
//! walker's innermost span, because instruction order is post-order.

use faer::MatRef;

use crate::ast::{BinaryOp, CompareOp, UnaryOp};

use super::lane::{compare, near_eq, resolve_index};
use super::tape::{Accumulate, FaultKind, Insn, LaneFault, Reg, Tape};

/// Lanes per tile.
///
/// A register is `TILE × 8` bytes: 2 KB, so sixteen live registers are 32 KB,
/// inside L1D on every machine in `performance-records/hosts`. It is also the
/// ledgers' batch width, so a batch of 256 is exactly one tile and the
/// 1024-wide brute-squad batches are four. Chosen, not measured; the tile
/// sweep is a later item.
pub(crate) const TILE: usize = 256;

/// `registers × width` doubles, register-major, so a register is one
/// contiguous slice.
pub(crate) struct RegisterFile {
    data: Vec<f64>,
    width: usize,
}

impl RegisterFile {
    /// Primed once: constants broadcast, everything else NaN. A straight-line
    /// tape writes every register it reads before reading it, except a local
    /// the lowerer could not prove assigned — which must read NaN, and does,
    /// because nothing ever writes it.
    pub(crate) fn new(tape: &Tape, width: usize) -> Self {
        let mut data = vec![f64::NAN; tape.registers as usize * width];
        for (index, &value) in tape.consts.iter().enumerate() {
            data[index * width..(index + 1) * width].fill(value);
        }
        Self { data, width }
    }

    pub(crate) fn reg(&self, reg: Reg, lanes: usize) -> &[f64] {
        &self.data[reg.index() * self.width..][..lanes]
    }

    fn reg_mut(&mut self, reg: Reg, lanes: usize) -> &mut [f64] {
        &mut self.data[reg.index() * self.width..][..lanes]
    }

    /// `dst` mutable alongside `a` shared. Requires `dst != a`.
    fn dst_a(&mut self, dst: Reg, a: Reg, lanes: usize) -> (&mut [f64], &[f64]) {
        let width = self.width;
        let (d, before, after) = self.split(dst);
        (
            &mut d[..lanes],
            &pick(before, after, width, dst, a)[..lanes],
        )
    }

    /// `dst` mutable alongside `a` and `b` shared. Requires `dst ∉ {a, b}`;
    /// `a == b` is fine.
    fn dst_a_b(&mut self, dst: Reg, a: Reg, b: Reg, lanes: usize) -> (&mut [f64], &[f64], &[f64]) {
        let width = self.width;
        let (d, before, after) = self.split(dst);
        (
            &mut d[..lanes],
            &pick(before, after, width, dst, a)[..lanes],
            &pick(before, after, width, dst, b)[..lanes],
        )
    }

    /// The destination register split out of the file, with the registers
    /// before and after it as shared slices. The allocator guarantees a
    /// destination never aliases an operand, which is what makes this sound.
    fn split(&mut self, dst: Reg) -> (&mut [f64], &[f64], &[f64]) {
        let start = dst.index() * self.width;
        let (before, rest) = self.data.split_at_mut(start);
        let (d, after) = rest.split_at_mut(self.width);
        (d, before, after)
    }
}

/// Register `reg`'s slice from the two halves a [`RegisterFile::split`] left.
fn pick<'a>(before: &'a [f64], after: &'a [f64], width: usize, dst: Reg, reg: Reg) -> &'a [f64] {
    assert_ne!(reg, dst, "a destination register aliased an operand");
    if reg < dst {
        &before[reg.index() * width..][..width]
    } else {
        &after[(reg.index() - dst.index() - 1) * width..][..width]
    }
}

fn record(faults: &mut [Option<LaneFault>], lane: usize, pc: usize, kind: FaultKind) {
    if faults[lane].is_none() {
        faults[lane] = Some(LaneFault {
            insn: u32::try_from(pc).expect("fewer than 2^32 instructions"),
            kind,
        });
    }
}

/// The slow path, taken only after a fused check said some lane went bad.
fn record_non_finite(faults: &mut [Option<LaneFault>], pc: usize, values: &[f64]) {
    for (lane, &value) in values.iter().enumerate() {
        if !value.is_finite() {
            record(faults, lane, pc, FaultKind::NonFinite(value));
        }
    }
}

// The loop bodies. Each returns whether any lane produced a non-finite value;
// the `bad |=` is an or-reduction the vectoriser keeps in a register.

fn unary_loop(dst: &mut [f64], a: &[f64], f: impl Fn(f64) -> f64) -> bool {
    let mut bad = false;
    for (d, &x) in dst.iter_mut().zip(a) {
        let v = f(x);
        *d = v;
        bad |= !v.is_finite();
    }
    bad
}

fn binary_loop(dst: &mut [f64], a: &[f64], b: &[f64], f: impl Fn(f64, f64) -> f64) -> bool {
    let mut bad = false;
    for ((d, &x), &y) in dst.iter_mut().zip(a).zip(b) {
        let v = f(x, y);
        *d = v;
        bad |= !v.is_finite();
    }
    bad
}

fn in_place_loop(dst: &mut [f64], b: &[f64], f: impl Fn(f64, f64) -> f64) -> bool {
    let mut bad = false;
    for (d, &y) in dst.iter_mut().zip(b) {
        let v = f(*d, y);
        *d = v;
        bad |= !v.is_finite();
    }
    bad
}

/// Chooses the operator outside the loop, one arm per variant, so the closure
/// the loop inlines is the raw operation and not a `match` per lane. The
/// match is exhaustive: a new operator fails to compile here rather than
/// silently taking a slow path.
macro_rules! dispatch_unary {
    ($op:expr, $dst:expr, $a:expr, [$($variant:ident),* $(,)?]) => {
        match $op {
            $(UnaryOp::$variant => unary_loop($dst, $a, |x| UnaryOp::$variant.apply(x)),)*
        }
    };
}

macro_rules! dispatch_binary {
    ($op:expr, $dst:expr, $a:expr, $b:expr, [$($variant:ident),* $(,)?]) => {
        match $op {
            $(BinaryOp::$variant => binary_loop($dst, $a, $b, |x, y| BinaryOp::$variant.apply(x, y)),)*
        }
    };
}

/// Runs a straight-line tape over `lanes` columns of `samples` starting at
/// `first_column`. Returns the lowest faulted lane, if any.
pub(crate) fn run_tile(
    tape: &Tape,
    samples: MatRef<'_, f64>,
    first_column: usize,
    lanes: usize,
    file: &mut RegisterFile,
    faults: &mut [Option<LaneFault>],
) -> Option<(usize, LaneFault)> {
    debug_assert_eq!(faults.len(), lanes);
    let available = samples.nrows();

    for (pc, insn) in tape.insns.iter().enumerate() {
        match *insn {
            Insn::Load { dst, input } => {
                let d = file.reg_mut(dst, lanes);
                let mut bad = false;
                for (lane, v) in d.iter_mut().enumerate() {
                    let x = samples[(input as usize, first_column + lane)];
                    *v = x;
                    bad |= !x.is_finite();
                }
                if bad {
                    record_non_finite(faults, pc, d);
                }
            }
            Insn::Copy { dst, src } => {
                let (d, s) = file.dst_a(dst, src, lanes);
                d.copy_from_slice(s);
            }
            Insn::Unary { dst, op, a } => {
                let (d, a) = file.dst_a(dst, a, lanes);
                let bad = dispatch_unary!(
                    op,
                    d,
                    a,
                    [
                        Negate, Cos, Sin, Tan, Acos, Asin, Atan, Cosh, Sinh, Tanh, Cot, Ln, Log10,
                        Abs, Sqrt, Cbrt, Sqr, Cube, Ceil, Floor, Sgn,
                    ]
                );
                if bad {
                    record_non_finite(faults, pc, d);
                }
            }
            Insn::Binary { dst, op, a, b } => {
                let (d, a, b) = file.dst_a_b(dst, a, b, lanes);
                let bad =
                    dispatch_binary!(op, d, a, b, [Add, Sub, Mul, Div, Rem, Pow, Max, Min, LogB]);
                if bad {
                    record_non_finite(faults, pc, d);
                }
            }
            Insn::Compare { dst, op, a, b } => {
                let (d, a, b) = file.dst_a_b(dst, a, b, lanes);
                let bad = match op {
                    CompareOp::Lte => binary_loop(d, a, b, |x, y| compare(CompareOp::Lte, x, y)),
                    CompareOp::Gte => binary_loop(d, a, b, |x, y| compare(CompareOp::Gte, x, y)),
                    CompareOp::Lt => binary_loop(d, a, b, |x, y| compare(CompareOp::Lt, x, y)),
                    CompareOp::Gt => binary_loop(d, a, b, |x, y| compare(CompareOp::Gt, x, y)),
                };
                if bad {
                    record_non_finite(faults, pc, d);
                }
            }
            Insn::NearEq {
                dst,
                a,
                b,
                tolerance,
            } => {
                // A constant register holds the same value in every lane.
                let t = file.reg(tolerance, 1)[0];
                let (d, a, b) = file.dst_a_b(dst, a, b, lanes);
                let bad = binary_loop(d, a, b, |x, y| near_eq(x, y, t));
                if bad {
                    record_non_finite(faults, pc, d);
                }
            }
            Insn::Combine {
                dst,
                how,
                a,
                b,
                last,
            } => {
                let bad = if dst == a {
                    let (d, b) = file.dst_a(dst, b, lanes);
                    match how {
                        Accumulate::Sum => in_place_loop(d, b, |x, y| Accumulate::Sum.apply(x, y)),
                        Accumulate::Prod => {
                            in_place_loop(d, b, |x, y| Accumulate::Prod.apply(x, y))
                        }
                        Accumulate::Worst => {
                            in_place_loop(d, b, |x, y| Accumulate::Worst.apply(x, y))
                        }
                    }
                } else {
                    let (d, a, b) = file.dst_a_b(dst, a, b, lanes);
                    match how {
                        Accumulate::Sum => binary_loop(d, a, b, |x, y| Accumulate::Sum.apply(x, y)),
                        Accumulate::Prod => {
                            binary_loop(d, a, b, |x, y| Accumulate::Prod.apply(x, y))
                        }
                        Accumulate::Worst => {
                            binary_loop(d, a, b, |x, y| Accumulate::Worst.apply(x, y))
                        }
                    }
                };
                if last && bad {
                    record_non_finite(faults, pc, file.reg(dst, lanes));
                }
            }
            Insn::Check { reg } => {
                let values = file.reg(reg, lanes);
                if values.iter().any(|v| !v.is_finite()) {
                    record_non_finite(faults, pc, values);
                }
            }
            Insn::Gather { dst, index, .. } => {
                let (d, index) = file.dst_a(dst, index, lanes);
                for (lane, (v, &i)) in d.iter_mut().zip(index).enumerate() {
                    match resolve_index(i, available) {
                        Ok(position) => {
                            let x = samples[(position, first_column + lane)];
                            *v = x;
                            if !x.is_finite() {
                                record(faults, lane, pc, FaultKind::NonFinite(x));
                            }
                        }
                        Err(kind) => {
                            *v = f64::NAN;
                            record(faults, lane, pc, kind);
                        }
                    }
                }
            }
            Insn::Bound { .. } | Insn::LoopStart { .. } | Insn::LoopEnd { .. } => {
                unreachable!("a tape with a loop runs per lane, never as a tile")
            }
        }
    }

    faults
        .iter()
        .enumerate()
        .find_map(|(lane, fault)| fault.map(|f| (lane, f)))
}

#[cfg(test)]
#[path = "tile_tests.rs"]
mod tests;
