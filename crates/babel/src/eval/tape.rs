//! The evaluator's intermediate representation: a flat list of three-address
//! instructions over virtual registers.
//!
//! A tree walk pays its dispatch once per node per sample. A tape pays it once
//! per instruction per *tile* of samples: each instruction is one loop over a
//! slice of lanes, which is the shape the compiler vectorises. The same tape,
//! read one lane at a time, is also the per-row evaluator the sampler uses.
//!
//! Three-address form rather than a stack, because every consumer wants
//! names: the batched executor wants a destination slice and operand slices,
//! the register allocator wants intervals, and a shader emitter wants
//! `let t7 = t3 * t4;`. A stack machine is a register machine with a fixed
//! implicit allocation, and the implicitness is what would hurt.
//!
//! The semantics are the tree-walker's, to the bit. Every operator goes
//! through [`UnaryOp::apply`] and [`BinaryOp::apply`]; comparisons compute
//! exactly the expressions the walker did, in the same order; and the
//! non-finite check that the walker ran on every node runs here on every
//! checked instruction. `tests/corpus.rs`, `tests/runtime_errors.rs` and
//! `tests/special_values.rs` are the spec.

use crate::ast::{AggregateKind, BinaryOp, CompareOp, UnaryOp};
use crate::diagnostics::{BoundKind, Fault, ProblemKind, Span};

/// A physical register: an index into the frame (per lane) or the register
/// file (per tile).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Reg(pub(crate) u16);

impl Reg {
    pub(crate) const fn index(self) -> usize {
        self.0 as usize
    }
}

/// A register before allocation, by class.
///
/// Constants and locals are *pinned*: a constant lives in one register for
/// the life of the tape, and a local keeps the frame slot the front end gave
/// it, so that an unwritten local reads the NaN it was primed with — the
/// walker's sentinel, preserved. Only temporaries are packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum VReg {
    Const(u16),
    Local(u16),
    Temp(u32),
}

/// How a fold step combines, each arm deferring to the AST's own definition so
/// that the tape cannot drift from the language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Accumulate {
    Sum,
    Prod,
    /// Conjunction under the `<= 0` convention: the worst residual wins.
    /// Java's NaN-propagating `max`, via `BinaryOp::Max`.
    Worst,
}

impl Accumulate {
    pub(crate) fn apply(self, accumulated: f64, term: f64) -> f64 {
        match self {
            Self::Sum => AggregateKind::Sum.combine(accumulated, term),
            Self::Prod => AggregateKind::Prod.combine(accumulated, term),
            Self::Worst => BinaryOp::Max.apply(accumulated, term),
        }
    }

    pub(crate) const fn identity(self) -> f64 {
        match self {
            Self::Sum => AggregateKind::Sum.identity(),
            Self::Prod => AggregateKind::Prod.identity(),
            Self::Worst => f64::NEG_INFINITY,
        }
    }

    pub(crate) const fn from_aggregate(kind: AggregateKind) -> Self {
        match kind {
            AggregateKind::Sum => Self::Sum,
            AggregateKind::Prod => Self::Prod,
        }
    }
}

/// Whether a tape can run a tile at a time.
///
/// A loop with a run-time trip count is different per lane, so a tape that
/// contains one runs lane by lane. Decided once, at lowering, and stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape {
    StraightLine,
    Loops,
}

/// One instruction. Generic over the register type so that a tape whose
/// registers have not been allocated cannot be run by mistake.
///
/// "Checked" below means the destination is tested for a finite value and a
/// non-finite one is a [`ProblemKind::NonFiniteValue`] at the instruction's
/// span — the walker's rule, applied per instruction rather than per node,
/// which is the same set of places.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Insn<R> {
    /// `row[input]` into `dst`. Checked, so a non-finite input fails at the
    /// variable that carried it.
    Load {
        dst: R,
        input: u32,
    },
    /// `var x = <leaf>`: a leaf has no instruction of its own to write into
    /// the local, so it is copied. Unchecked — the leaf was.
    Copy {
        dst: R,
        src: R,
    },
    Unary {
        dst: R,
        op: UnaryOp,
        a: R,
    },
    /// `dst` never aliases `a` or `b`; the allocator guarantees it and the
    /// batched executor's slice split depends on it.
    Binary {
        dst: R,
        op: BinaryOp,
        a: R,
        b: R,
    },
    /// The residual of `a op b` under the `<= 0` convention, computed exactly
    /// as the walker computes it, `EPSILON` nudge included. Checked.
    Compare {
        dst: R,
        op: CompareOp,
        a: R,
        b: R,
    },
    /// `a == b +/- tolerance`, the larger of the two one-sided residuals.
    /// `tolerance` is a constant register. Checked.
    NearEq {
        dst: R,
        a: R,
        b: R,
        tolerance: R,
    },
    /// One step of a fold: `dst = how(a, b)`. `dst == a` is allowed and usual,
    /// for in-place accumulation. Checked only when `last`, because the walker
    /// checks a fold's final value and not its intermediate ones: a product
    /// that overflows to infinity at term three and reaches NaN at term five
    /// reports the NaN.
    Combine {
        dst: R,
        how: Accumulate,
        a: R,
        b: R,
        last: bool,
    },
    /// Test `reg` for a finite value and fault at this instruction's span if it
    /// is not. Emitted where the walker checked a node that produced no
    /// instruction here: a local the lowerer cannot prove was assigned, and the
    /// accumulator of a run-time loop.
    Check {
        reg: R,
    },
    /// `var[index]`: one-based into the whole row. The index faults name
    /// `subscript`; a non-finite value read names this instruction. Checked.
    Gather {
        dst: R,
        index: R,
        subscript: Span,
    },
    /// Convert an aggregate bound to an index, or fault with
    /// [`ProblemKind::IllegalAggregateBound`] at this instruction's span. Sits
    /// between the two bound expressions because the walker converts the lower
    /// bound before it evaluates the upper expression.
    Bound {
        reg: R,
        which: BoundKind,
    },
    /// A run-time-bounded aggregate. Seeds `acc` with the identity, and on an
    /// empty range jumps past `end`; otherwise sets `param` and runs the body
    /// up to `end`, which combines and loops back. Per-lane executor only.
    LoopStart {
        lower: R,
        upper: R,
        param: R,
        acc: R,
        kind: AggregateKind,
        end: u32,
    },
    /// `acc = kind.combine(acc, term)`; then the next index, or fall through.
    LoopEnd {
        start: u32,
        acc: R,
        term: R,
    },
}

impl<R: Copy> Insn<R> {
    /// The register this instruction writes, if any.
    pub(crate) fn dst(&self) -> Option<R> {
        match *self {
            Insn::Load { dst, .. }
            | Insn::Copy { dst, .. }
            | Insn::Unary { dst, .. }
            | Insn::Binary { dst, .. }
            | Insn::Compare { dst, .. }
            | Insn::NearEq { dst, .. }
            | Insn::Combine { dst, .. }
            | Insn::Gather { dst, .. } => Some(dst),
            Insn::LoopStart { acc, .. } | Insn::LoopEnd { acc, .. } => Some(acc),
            Insn::Check { .. } | Insn::Bound { .. } => None,
        }
    }

    /// The registers this instruction reads.
    pub(crate) fn sources(&self) -> Vec<R> {
        match *self {
            Insn::Load { .. } => Vec::new(),
            Insn::Copy { src, .. } => vec![src],
            Insn::Unary { a, .. } => vec![a],
            Insn::Binary { a, b, .. } | Insn::Compare { a, b, .. } => vec![a, b],
            Insn::NearEq {
                a, b, tolerance, ..
            } => vec![a, b, tolerance],
            Insn::Combine { a, b, .. } => vec![a, b],
            Insn::Check { reg } | Insn::Bound { reg, .. } => vec![reg],
            Insn::Gather { index, .. } => vec![index],
            Insn::LoopStart {
                lower,
                upper,
                param,
                ..
            } => vec![lower, upper, param],
            Insn::LoopEnd { acc, term, .. } => vec![acc, term],
        }
    }

    /// The same instruction over another register type.
    pub(crate) fn map<S>(self, mut f: impl FnMut(R) -> S) -> Insn<S> {
        match self {
            Insn::Load { dst, input } => Insn::Load { dst: f(dst), input },
            Insn::Copy { dst, src } => Insn::Copy {
                dst: f(dst),
                src: f(src),
            },
            Insn::Unary { dst, op, a } => Insn::Unary {
                dst: f(dst),
                op,
                a: f(a),
            },
            Insn::Binary { dst, op, a, b } => Insn::Binary {
                dst: f(dst),
                op,
                a: f(a),
                b: f(b),
            },
            Insn::Compare { dst, op, a, b } => Insn::Compare {
                dst: f(dst),
                op,
                a: f(a),
                b: f(b),
            },
            Insn::NearEq {
                dst,
                a,
                b,
                tolerance,
            } => Insn::NearEq {
                dst: f(dst),
                a: f(a),
                b: f(b),
                tolerance: f(tolerance),
            },
            Insn::Combine {
                dst,
                how,
                a,
                b,
                last,
            } => Insn::Combine {
                dst: f(dst),
                how,
                a: f(a),
                b: f(b),
                last,
            },
            Insn::Check { reg } => Insn::Check { reg: f(reg) },
            Insn::Gather {
                dst,
                index,
                subscript,
            } => Insn::Gather {
                dst: f(dst),
                index: f(index),
                subscript,
            },
            Insn::Bound { reg, which } => Insn::Bound { reg: f(reg), which },
            Insn::LoopStart {
                lower,
                upper,
                param,
                acc,
                kind,
                end,
            } => Insn::LoopStart {
                lower: f(lower),
                upper: f(upper),
                param: f(param),
                acc: f(acc),
                kind,
                end,
            },
            Insn::LoopEnd { start, acc, term } => Insn::LoopEnd {
                start,
                acc: f(acc),
                term: f(term),
            },
        }
    }
}

/// What went wrong on one lane, in terms the tape can turn into a [`Fault`].
///
/// Recorded rather than raised, because the batched executor keeps going: NaN
/// flows on through the remaining instructions and the lowest faulted lane is
/// reported at the end, which is the walker's "first failing column".
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LaneFault {
    pub(crate) insn: u32,
    pub(crate) kind: FaultKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum FaultKind {
    NonFinite(f64),
    NotAnInteger(f64),
    OutOfBounds {
        requested_1index: i64,
        available: usize,
    },
    Bound(BoundKind, f64),
}

/// A lowered, allocated program.
#[derive(Debug, Clone)]
pub(crate) struct Tape {
    /// Register `i` holds `consts[i]`. Deduplicated by bit pattern, so `0.0`
    /// and `-0.0` are distinct.
    pub(crate) consts: Vec<f64>,
    /// The front end's `frame_size`. Registers `consts.len()..` are the locals,
    /// primed to NaN.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the tests read the register class boundaries")
    )]
    pub(crate) locals: u16,
    /// Constants, locals and packed temporaries together.
    pub(crate) registers: u16,
    pub(crate) insns: Vec<Insn<Reg>>,
    /// `spans[i]` is the node `insns[i]` computes: the span a check reports.
    pub(crate) spans: Vec<Span>,
    pub(crate) result: Reg,
    pub(crate) shape: Shape,
}

impl Tape {
    /// Fills a frame or one lane's worth of a register file: constants in
    /// place, everything else NaN. The NaN is the walker's unwritten-slot
    /// sentinel and is what makes a `Check` on an unassigned local fire.
    pub(crate) fn prime(&self, frame: &mut [f64]) {
        debug_assert_eq!(frame.len(), self.registers as usize);
        frame.fill(f64::NAN);
        frame[..self.consts.len()].copy_from_slice(&self.consts);
    }

    /// The register class boundaries, for the tests.
    #[cfg(test)]
    pub(crate) fn first_local(&self) -> u16 {
        u16::try_from(self.consts.len()).expect("constant count fits u16")
    }

    /// Renders a lane's fault against the tape's spans.
    pub(crate) fn fault(&self, lane: LaneFault) -> Fault {
        let at = lane.insn as usize;
        let subscript = match self.insns[at] {
            Insn::Gather { subscript, .. } => subscript,
            _ => self.spans[at],
        };
        match lane.kind {
            FaultKind::NonFinite(value) => Fault {
                kind: ProblemKind::NonFiniteValue { value },
                span: self.spans[at],
            },
            FaultKind::NotAnInteger(value) => Fault {
                kind: ProblemKind::DynamicIndexNotAnInteger { value },
                span: subscript,
            },
            FaultKind::OutOfBounds {
                requested_1index,
                available,
            } => Fault {
                kind: ProblemKind::DynamicIndexOutOfBounds {
                    requested_1index,
                    available,
                },
                span: subscript,
            },
            FaultKind::Bound(bound, value) => Fault {
                kind: ProblemKind::IllegalAggregateBound { bound, value },
                span: self.spans[at],
            },
        }
    }
}
