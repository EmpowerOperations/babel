//! The batched executor: one straight-line tape over a tile of samples, each
//! instruction one kernel across the lanes.
//!
//! The instruction set is chosen once per tile by `pulp::Arch` — AVX2 where
//! the machine has it, the scalar backend otherwise — and the whole walk runs
//! inside that choice. Each instruction picks a kernel from `simd.rs` by name:
//! a vector kernel for the operators that have one, a `*_scalar` kernel for
//! the rest, so which is which is written down rather than decided by the
//! compiler.
//!
//! Faults are recorded, not raised. Each kernel reports whether any lane went
//! non-finite; only then does the slow path walk the lanes and record which.
//! NaN flows on through the remaining instructions, every lane keeps its own
//! first fault, and the lowest faulted lane is reported at the end — the
//! walker's "first failing column", with the walker's innermost span, because
//! instruction order is post-order.

use faer::MatRef;
use pulp::{Simd, WithSimd};

use crate::ast::{BinaryOp, UnaryOp};

use super::lane::resolve_index;
use super::simd;
use super::tape::{Accumulate, FaultKind, Insn, LaneFault, Reg, Tape};

/// Lanes per tile.
///
/// A register is `TILE × 8` bytes: 2 KB, so sixteen live registers are 32 KB,
/// inside L1D on every machine in `performance-records/hosts`. It is also the
/// ledgers' batch width, so a batch of 256 is exactly one tile and the
/// 1024-wide brute-squad batches are four; and it is a multiple of every lane
/// count pulp has, so a full tile leaves no scalar tail. Chosen, not measured;
/// the tile sweep is a later item.
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
            data[index*width .. (index+1)*width].fill(value);
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

/// The slow path, taken only after a kernel said some lane went bad.
fn record_non_finite(faults: &mut [Option<LaneFault>], pc: usize, values: &[f64]) {
    for (lane, &value) in values.iter().enumerate() {
        if !value.is_finite() {
            record(faults, lane, pc, FaultKind::NonFinite(value));
        }
    }
}

/// One instruction's unary operator, chosen outside the loop. The vector arms
/// name their kernel; the rest are libm, rounding and sign, and stay scalar.
/// Exhaustive, so a new operator fails to compile here rather than silently
/// taking a slow path.
macro_rules! dispatch_unary {
    ($simd:expr, $op:expr, $d:expr, $a:expr) => {
        match $op {
            UnaryOp::Negate => simd::unary(
                $simd,
                $d,
                $a,
                |s, x| s.neg_f64s(x),
                |x| UnaryOp::Negate.apply(x),
            ),
            UnaryOp::Abs => simd::unary(
                $simd,
                $d,
                $a,
                |s, x| s.abs_f64s(x),
                |x| UnaryOp::Abs.apply(x),
            ),
            UnaryOp::Sqrt => simd::unary(
                $simd,
                $d,
                $a,
                |s, x| s.sqrt_f64s(x),
                |x| UnaryOp::Sqrt.apply(x),
            ),
            UnaryOp::Sqr => simd::unary(
                $simd,
                $d,
                $a,
                |s, x| s.mul_f64s(x, x),
                |x| UnaryOp::Sqr.apply(x),
            ),
            UnaryOp::Cube => simd::unary(
                $simd,
                $d,
                $a,
                |s, x| s.mul_f64s(s.mul_f64s(x, x), x),
                |x| UnaryOp::Cube.apply(x),
            ),
            UnaryOp::Cos => simd::unary_scalar($d, $a, |x| UnaryOp::Cos.apply(x)),
            UnaryOp::Sin => simd::unary_scalar($d, $a, |x| UnaryOp::Sin.apply(x)),
            UnaryOp::Tan => simd::unary_scalar($d, $a, |x| UnaryOp::Tan.apply(x)),
            UnaryOp::Acos => simd::unary_scalar($d, $a, |x| UnaryOp::Acos.apply(x)),
            UnaryOp::Asin => simd::unary_scalar($d, $a, |x| UnaryOp::Asin.apply(x)),
            UnaryOp::Atan => simd::unary_scalar($d, $a, |x| UnaryOp::Atan.apply(x)),
            UnaryOp::Cosh => simd::unary_scalar($d, $a, |x| UnaryOp::Cosh.apply(x)),
            UnaryOp::Sinh => simd::unary_scalar($d, $a, |x| UnaryOp::Sinh.apply(x)),
            UnaryOp::Tanh => simd::unary_scalar($d, $a, |x| UnaryOp::Tanh.apply(x)),
            UnaryOp::Cot => simd::unary_scalar($d, $a, |x| UnaryOp::Cot.apply(x)),
            UnaryOp::Ln => simd::unary_scalar($d, $a, |x| UnaryOp::Ln.apply(x)),
            UnaryOp::Log10 => simd::unary_scalar($d, $a, |x| UnaryOp::Log10.apply(x)),
            UnaryOp::Cbrt => simd::unary_scalar($d, $a, |x| UnaryOp::Cbrt.apply(x)),
            UnaryOp::Ceil => simd::unary_scalar($d, $a, |x| UnaryOp::Ceil.apply(x)),
            UnaryOp::Floor => simd::unary_scalar($d, $a, |x| UnaryOp::Floor.apply(x)),
            UnaryOp::Sgn => simd::unary_scalar($d, $a, |x| UnaryOp::Sgn.apply(x)),
        }
    };
}

/// One instruction's binary operator, chosen outside the loop. `Max` and `Min`
/// go through the NaN-propagating kernels, never pulp's own.
macro_rules! dispatch_binary {
    ($simd:expr, $op:expr, $d:expr, $a:expr, $b:expr) => {
        match $op {
            BinaryOp::Add => simd::binary(
                $simd,
                $d,
                $a,
                $b,
                |s, x, y| s.add_f64s(x, y),
                |x, y| BinaryOp::Add.apply(x, y),
            ),
            BinaryOp::Sub => simd::binary(
                $simd,
                $d,
                $a,
                $b,
                |s, x, y| s.sub_f64s(x, y),
                |x, y| BinaryOp::Sub.apply(x, y),
            ),
            BinaryOp::Mul => simd::binary(
                $simd,
                $d,
                $a,
                $b,
                |s, x, y| s.mul_f64s(x, y),
                |x, y| BinaryOp::Mul.apply(x, y),
            ),
            BinaryOp::Div => simd::binary(
                $simd,
                $d,
                $a,
                $b,
                |s, x, y| s.div_f64s(x, y),
                |x, y| BinaryOp::Div.apply(x, y),
            ),
            BinaryOp::Max => simd::binary($simd, $d, $a, $b, simd::max_nan, simd::max_scalar),
            BinaryOp::Min => simd::binary($simd, $d, $a, $b, simd::min_nan, simd::min_scalar),
            BinaryOp::Rem => simd::binary_scalar($d, $a, $b, |x, y| BinaryOp::Rem.apply(x, y)),
            BinaryOp::Pow => simd::binary_scalar($d, $a, $b, |x, y| BinaryOp::Pow.apply(x, y)),
            BinaryOp::LogB => simd::binary_scalar($d, $a, $b, |x, y| BinaryOp::LogB.apply(x, y)),
        }
    };
}

/// A fold step's combine, chosen outside the loop, in the in-place or the
/// three-address form.
macro_rules! dispatch_combine {
    ($simd:expr, $how:expr, in_place: $d:expr, $b:expr) => {
        match $how {
            Accumulate::Sum => simd::in_place(
                $simd,
                $d,
                $b,
                |s, x, y| s.add_f64s(x, y),
                |x, y| Accumulate::Sum.apply(x, y),
            ),
            Accumulate::Prod => simd::in_place(
                $simd,
                $d,
                $b,
                |s, x, y| s.mul_f64s(x, y),
                |x, y| Accumulate::Prod.apply(x, y),
            ),
            Accumulate::Worst => simd::in_place($simd, $d, $b, simd::max_nan, |x, y| {
                Accumulate::Worst.apply(x, y)
            }),
        }
    };
    ($simd:expr, $how:expr, three_address: $d:expr, $a:expr, $b:expr) => {
        match $how {
            Accumulate::Sum => simd::binary(
                $simd,
                $d,
                $a,
                $b,
                |s, x, y| s.add_f64s(x, y),
                |x, y| Accumulate::Sum.apply(x, y),
            ),
            Accumulate::Prod => simd::binary(
                $simd,
                $d,
                $a,
                $b,
                |s, x, y| s.mul_f64s(x, y),
                |x, y| Accumulate::Prod.apply(x, y),
            ),
            Accumulate::Worst => simd::binary($simd, $d, $a, $b, simd::max_nan, |x, y| {
                Accumulate::Worst.apply(x, y)
            }),
        }
    };
}

/// One tile's worth of work, dispatched once onto the chosen instruction set.
struct TileRun<'a> {
    tape: &'a Tape,
    samples: MatRef<'a, f64>,
    first_column: usize,
    lanes: usize,
    file: &'a mut RegisterFile,
    faults: &'a mut [Option<LaneFault>],
}

impl WithSimd for TileRun<'_> {
    type Output = Option<(usize, LaneFault)>;

    #[inline(always)]
    fn with_simd<S: Simd>(self, simd: S) -> Self::Output {
        run_tile_with(simd, self)
    }
}

/// Runs a straight-line tape over `lanes` columns of `samples` starting at
/// `first_column`, on the best instruction set this machine has. Returns the
/// lowest faulted lane, if any.
pub(crate) fn run_tile(
    tape: &Tape,
    samples: MatRef<'_, f64>,
    first_column: usize,
    lanes: usize,
    file: &mut RegisterFile,
    faults: &mut [Option<LaneFault>],
) -> Option<(usize, LaneFault)> {
    run_tile_on(
        pulp::Arch::new(),
        tape,
        samples,
        first_column,
        lanes,
        file,
        faults,
    )
}

/// [`run_tile`] on a given backend — the seam the tests use to force the
/// scalar one and compare.
pub(crate) fn run_tile_on(
    arch: pulp::Arch,
    tape: &Tape,
    samples: MatRef<'_, f64>,
    first_column: usize,
    lanes: usize,
    file: &mut RegisterFile,
    faults: &mut [Option<LaneFault>],
) -> Option<(usize, LaneFault)> {
    debug_assert_eq!(faults.len(), lanes);
    arch.dispatch(TileRun {
        tape,
        samples,
        first_column,
        lanes,
        file,
        faults,
    })
}

/// The instruction walk, monomorphised per backend.
#[inline(always)]
fn run_tile_with<S: Simd>(simd: S, run: TileRun<'_>) -> Option<(usize, LaneFault)> {
    let TileRun {
        tape,
        samples,
        first_column,
        lanes,
        file,
        faults,
    } = run;
    let available = samples.nrows();

    for (pc, insn) in tape.insns.iter().enumerate() {
        match *insn {
            Insn::Load { dst, input } => {
                let d = file.reg_mut(dst, lanes);
                if simd::load_strided(d, samples, input as usize, first_column) {
                    record_non_finite(faults, pc, d);
                }
            }
            Insn::Copy { dst, src } => {
                let (d, s) = file.dst_a(dst, src, lanes);
                d.copy_from_slice(s);
            }
            Insn::Unary { dst, op, a } => {
                let (d, a) = file.dst_a(dst, a, lanes);
                if dispatch_unary!(simd, op, d, a) {
                    record_non_finite(faults, pc, d);
                }
            }
            Insn::Binary { dst, op, a, b } => {
                let (d, a, b) = file.dst_a_b(dst, a, b, lanes);
                if dispatch_binary!(simd, op, d, a, b) {
                    record_non_finite(faults, pc, d);
                }
            }
            Insn::Compare { dst, op, a, b } => {
                let (d, a, b) = file.dst_a_b(dst, a, b, lanes);
                let bad = simd::binary(
                    simd,
                    d,
                    a,
                    b,
                    |s, x, y| simd::compare(s, op, x, y),
                    |x, y| super::lane::compare(op, x, y),
                );
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
                let bad = simd::binary(
                    simd,
                    d,
                    a,
                    b,
                    |s, x, y| simd::near_eq(s, x, y, s.splat_f64s(t)),
                    |x, y| super::lane::near_eq(x, y, t),
                );
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
                    dispatch_combine!(simd, how, in_place: d, b)
                } else {
                    let (d, a, b) = file.dst_a_b(dst, a, b, lanes);
                    dispatch_combine!(simd, how, three_address: d, a, b)
                };
                if last && bad {
                    record_non_finite(faults, pc, file.reg(dst, lanes));
                }
            }
            Insn::Check { reg } => {
                let values = file.reg(reg, lanes);
                if simd::any_non_finite(simd, values) {
                    record_non_finite(faults, pc, values);
                }
            }
            Insn::Gather { dst, index, .. } => {
                // Per lane by nature: each lane reads its own row. Scalar.
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
