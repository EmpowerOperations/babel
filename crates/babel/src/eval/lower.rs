//! `ast::Program` → [`Tape`]: one post-order walk emitting instructions in
//! exactly the order the tree-walker evaluated nodes, so that a fault lands
//! on the same span and a fold accumulates in the same order.
//!
//! Register classes are decided here and packed by [`super::regalloc`]:
//! literals become constant registers, `LocalSlot`s become local registers
//! one-to-one, and everything else is a temporary.

use std::collections::HashMap;

use crate::ast::{Block, Expr, Kind, Program, to_index};
use crate::diagnostics::Span;

use super::regalloc::allocate;
use super::tape::{Accumulate, Insn, Tape, VReg};

/// Lowers `program` against a schema of `row_len` variables, with
/// `global_positions[GlobalId]` giving each symbol's row.
pub(crate) fn lower(program: &Program, global_positions: &[u32], row_len: usize) -> Tape {
    let mut lowerer = Lowerer {
        global_positions,
        row_len,
        consts: Vec::new(),
        const_index: HashMap::new(),
        next_temp: 0,
        insns: Vec::new(),
        spans: Vec::new(),
        loads: Vec::new(),
        assigned: vec![false; program.frame_size as usize],
    };
    let result = lowerer.block(&program.body, None);
    lowerer.finish(result, program.frame_size)
}

struct Lowerer<'a> {
    global_positions: &'a [u32],
    row_len: usize,
    consts: Vec<f64>,
    /// Bit pattern → constant register, so `0.0` and `-0.0` stay distinct.
    const_index: HashMap<u64, u16>,
    next_temp: u32,
    insns: Vec<Insn<VReg>>,
    spans: Vec<Span>,
    /// Row position → register already holding it. A non-finite input faults
    /// at its *first* read, which is the cached `Load`'s span; later reads of
    /// the same variable need no instruction.
    loads: Vec<(u32, VReg)>,
    /// Which local slots are definitely written by the time they are read.
    /// A read the lowerer cannot prove gets a `Check`, preserving the walker's
    /// NaN-sentinel semantics for a front-end slot bug.
    assigned: Vec<bool>,
}

impl Lowerer<'_> {
    fn emit(&mut self, insn: Insn<VReg>, span: Span) {
        self.insns.push(insn);
        self.spans.push(span);
    }

    fn temp(&mut self) -> VReg {
        let t = VReg::Temp(self.next_temp);
        self.next_temp += 1;
        t
    }

    fn constant(&mut self, value: f64) -> VReg {
        let bits = value.to_bits();
        if let Some(&index) = self.const_index.get(&bits) {
            return VReg::Const(index);
        }
        let index = u16::try_from(self.consts.len()).expect("fewer than 65536 constants");
        self.consts.push(value);
        self.const_index.insert(bits, index);
        VReg::Const(index)
    }

    /// Delivers a value that already lives in `value` to `dst` if one was
    /// asked for. Only a leaf ever needs this: an instruction-producing node
    /// writes `dst` directly.
    fn place(&mut self, value: VReg, dst: Option<VReg>, span: Span) -> VReg {
        match dst {
            Some(dst) if dst != value => {
                self.emit(Insn::Copy { dst, src: value }, span);
                dst
            }
            _ => value,
        }
    }

    fn load(&mut self, input: u32, span: Span) -> VReg {
        if let Some(&(_, reg)) = self.loads.iter().find(|(i, _)| *i == input) {
            return reg;
        }
        let dst = self.temp();
        self.emit(Insn::Load { dst, input }, span);
        self.loads.push((input, dst));
        dst
    }

    fn local(slot: crate::ast::LocalSlot) -> VReg {
        VReg::Local(u16::try_from(slot.index()).expect("fewer than 65536 locals"))
    }

    fn block(&mut self, block: &Block, dst: Option<VReg>) -> VReg {
        for assignment in &block.assignments {
            let slot = Self::local(assignment.slot);
            self.expr(&assignment.value, Some(slot));
            self.assigned[assignment.slot.index()] = true;
        }
        self.expr(&block.result, dst)
    }

    /// Lowers `expr`, into `dst` if given, and returns the register holding
    /// the value.
    fn expr(&mut self, expr: &Expr, dst: Option<VReg>) -> VReg {
        let span = expr.span;
        match &expr.kind {
            Kind::Literal(value) => {
                let c = self.constant(*value);
                self.place(c, dst, span)
            }
            Kind::Global(id) => {
                let reg = self.load(self.global_positions[id.index()], span);
                self.place(reg, dst, span)
            }
            Kind::Local(slot) => {
                let reg = Self::local(*slot);
                if !self.assigned[slot.index()] {
                    self.emit(Insn::Check { reg }, span);
                }
                self.place(reg, dst, span)
            }
            Kind::DynamicIndex(subscript) => {
                // A literal subscript that names a real row is a plain load,
                // so `sum(1, 200, i -> var[i]…)` does not spend two hundred
                // constant registers on its indices. An invalid literal keeps
                // the gather, so it still faults at run time as the walker did.
                if let Kind::Literal(value) = subscript.kind
                    && let Some(index) = to_index(value)
                    && index >= 1
                    && usize::try_from(index - 1).is_ok_and(|p| p < self.row_len)
                {
                    let position = u32::try_from(index - 1).expect("checked against row_len");
                    let reg = self.load(position, span);
                    return self.place(reg, dst, span);
                }
                let index = self.expr(subscript, None);
                let dst = dst.unwrap_or_else(|| self.temp());
                self.emit(
                    Insn::Gather {
                        dst,
                        index,
                        subscript: subscript.span,
                    },
                    span,
                );
                dst
            }
            Kind::Unary { op, arg } => {
                let a = self.expr(arg, None);
                let dst = dst.unwrap_or_else(|| self.temp());
                self.emit(Insn::Unary { dst, op: *op, a }, span);
                dst
            }
            Kind::Binary { op, lhs, rhs } => {
                let a = self.expr(lhs, None);
                let b = self.expr(rhs, None);
                let dst = dst.unwrap_or_else(|| self.temp());
                self.emit(Insn::Binary { dst, op: *op, a, b }, span);
                dst
            }
            Kind::Compare { op, lhs, rhs } => {
                let a = self.expr(lhs, None);
                let b = self.expr(rhs, None);
                let dst = dst.unwrap_or_else(|| self.temp());
                self.emit(Insn::Compare { dst, op: *op, a, b }, span);
                dst
            }
            Kind::NearEq {
                lhs,
                rhs,
                tolerance,
            } => {
                let a = self.expr(lhs, None);
                let b = self.expr(rhs, None);
                let tolerance = self.constant(*tolerance);
                let dst = dst.unwrap_or_else(|| self.temp());
                self.emit(
                    Insn::NearEq {
                        dst,
                        a,
                        b,
                        tolerance,
                    },
                    span,
                );
                dst
            }
            Kind::Fold { kind, terms } => {
                self.fold(Accumulate::from_aggregate(*kind), terms, dst, span)
            }
            Kind::And { terms } => self.fold(Accumulate::Worst, terms, dst, span),
            Kind::Block(block) => self.block(block, dst),
            Kind::Aggregate { .. } => {
                unreachable!("`unroll_aggregates` turns every aggregate into a `Fold` or fails")
            }
        }
    }

    /// Left to right from the identity, one `Combine` per term, checked only
    /// on the last — the walker checked the fold's value, not each step.
    fn fold(&mut self, how: Accumulate, terms: &[Expr], dst: Option<VReg>, span: Span) -> VReg {
        let identity = self.constant(how.identity());
        if terms.is_empty() {
            return self.place(identity, dst, span);
        }
        let acc = dst.unwrap_or_else(|| self.temp());
        let last = terms.len() - 1;
        for (k, term) in terms.iter().enumerate() {
            let b = self.expr(term, None);
            let a = if k == 0 { identity } else { acc };
            self.emit(
                Insn::Combine {
                    dst: acc,
                    how,
                    a,
                    b,
                    last: k == last,
                },
                span,
            );
        }
        acc
    }

    fn finish(self, result: VReg, frame_size: u32) -> Tape {
        let consts = u16::try_from(self.consts.len()).expect("fewer than 65536 constants");
        let locals = u16::try_from(frame_size).expect("fewer than 65536 locals");
        let (insns, result, registers) = allocate(self.insns, result, consts, locals);
        Tape {
            consts: self.consts,
            locals,
            registers,
            insns,
            spans: self.spans,
            result,
        }
    }
}

#[cfg(test)]
#[path = "lower_tests.rs"]
mod tests;
