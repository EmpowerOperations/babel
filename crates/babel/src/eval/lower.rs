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
mod tests {
    //! Lowering: the instruction sequence a source produces.

    use crate::ast::{BinaryOp, Block, CompareOp, Expr, Kind, LocalSlot, Program, UnaryOp};
    use crate::diagnostics::Span;

    use super::super::tape::{Accumulate, Insn, Reg, Tape};
    use super::super::tape_for;

    const R: fn(u16) -> Reg = Reg;

    fn loads(tape: &Tape, input: u32) -> usize {
        tape.insns
            .iter()
            .filter(|i| matches!(i, Insn::Load { input: x, .. } if *x == input))
            .count()
    }

    #[test]
    fn a_sum_of_two_globals_lowers_to_two_loads_and_an_add() {
        let tape = tape_for("x1 + x2", &["x1", "x2"]);
        assert_eq!(
            tape.insns,
            vec![
                Insn::Load {
                    dst: R(0),
                    input: 0
                },
                Insn::Load {
                    dst: R(1),
                    input: 1
                },
                Insn::Binary {
                    dst: R(2),
                    op: BinaryOp::Add,
                    a: R(0),
                    b: R(1)
                },
            ]
        );
        assert_eq!(tape.result, R(2));
    }

    #[test]
    fn a_repeated_global_is_loaded_once() {
        let tape = tape_for("x1 * x1", &["x1"]);
        assert_eq!(loads(&tape, 0), 1);
        assert_eq!(
            tape.insns[1],
            Insn::Binary {
                dst: R(1),
                op: BinaryOp::Mul,
                a: R(0),
                b: R(0)
            }
        );
    }

    #[test]
    fn a_literal_is_a_constant_register_and_no_instruction() {
        let tape = tape_for("3 + 4", &[]);
        assert!(tape.insns.is_empty(), "{:?}", tape.insns);
        assert_eq!(tape.consts, vec![7.0]);
        assert_eq!(tape.result, R(0));

        let tape = tape_for("x1 + 2", &["x1"]);
        assert_eq!(tape.consts, vec![2.0]);
        // Constants come first, so the load lands in register 1.
        assert_eq!(
            tape.insns[0],
            Insn::Load {
                dst: R(1),
                input: 0
            }
        );
    }

    #[test]
    fn equal_constants_share_a_register_but_signed_zeros_do_not() {
        let tape = tape_for("x1 + 2 + 2", &["x1"]);
        assert_eq!(tape.consts, vec![2.0]);

        let tape = tape_for("x1 + 0.0 - -0.0", &["x1"]);
        assert_eq!(tape.consts.len(), 2, "{:?}", tape.consts);
        assert_ne!(tape.consts[0].to_bits(), tape.consts[1].to_bits());
    }

    #[test]
    fn a_valid_literal_subscript_is_a_plain_load() {
        let tape = tape_for("var[2] + x2", &["x1", "x2", "x3"]);
        assert!(!tape.insns.iter().any(|i| matches!(i, Insn::Gather { .. })));
        // `var[2]` and `x2` are the same row, so one load serves both.
        assert_eq!(loads(&tape, 1), 1);
        assert_eq!(tape.consts, Vec::<f64>::new());
    }

    #[test]
    fn an_invalid_literal_subscript_keeps_a_gather() {
        let tape = tape_for("var[0]", &["x1"]);
        assert_eq!(tape.consts, vec![0.0]);
        assert_eq!(
            tape.insns,
            vec![Insn::Gather {
                dst: R(1),
                index: R(0),
                subscript: Span::new(4, 5)
            }]
        );
    }

    #[test]
    fn a_strict_comparison_is_one_compare_instruction() {
        let tape = tape_for("x1 < x2", &["x1", "x2"]);
        assert_eq!(
            tape.insns[2],
            Insn::Compare {
                dst: R(2),
                op: CompareOp::Lt,
                a: R(0),
                b: R(1)
            }
        );
        // The nudge is inside the instruction, not a constant.
        assert!(tape.consts.is_empty());
    }

    #[test]
    fn near_equality_is_one_instruction_with_the_tolerance_as_a_constant() {
        let tape = tape_for("x1 == x2 +/- 0.5", &["x1", "x2"]);
        assert_eq!(tape.consts, vec![0.5]);
        assert_eq!(
            tape.insns[2],
            Insn::NearEq {
                dst: R(3),
                a: R(1),
                b: R(2),
                tolerance: R(0)
            }
        );
    }

    #[test]
    fn a_fold_seeds_from_the_identity_and_checks_only_its_last_step() {
        let tape = tape_for("sum(1, 3, i -> x1 * i)", &["x1"]);
        let identity = tape
            .consts
            .iter()
            .position(|c| c.to_bits() == 0.0f64.to_bits())
            .expect("the identity is a constant");
        let identity = R(u16::try_from(identity).unwrap());
        let combines: Vec<(Reg, Reg, bool)> = tape
            .insns
            .iter()
            .filter_map(|i| match *i {
                Insn::Combine {
                    dst,
                    how: Accumulate::Sum,
                    a,
                    last,
                    ..
                } => Some((dst, a, last)),
                _ => None,
            })
            .collect();
        assert_eq!(combines.len(), 3);
        let acc = combines[0].0;
        assert_eq!(combines[0].1, identity, "the first step reads the identity");
        assert!(combines[1..].iter().all(|(d, a, _)| *d == acc && *a == acc));
        assert_eq!(
            combines.iter().map(|c| c.2).collect::<Vec<_>>(),
            [false, false, true]
        );
    }

    #[test]
    fn an_and_seeds_from_negative_infinity() {
        // `invert_monotone` turns this into `x1 < e^2 and x1 > 0`.
        let tape = tape_for("ln(x1) < 2", &["x1"]);
        assert!(tape.consts.contains(&f64::NEG_INFINITY));
        assert!(tape.insns.iter().any(|i| matches!(
            i,
            Insn::Combine {
                how: Accumulate::Worst,
                ..
            }
        )));
    }

    #[test]
    fn a_local_is_written_in_place_and_read_without_an_instruction() {
        let tape = tape_for("var x = x1 * 2;\nx + x", &["x1"]);
        let local = R(tape.first_local());
        assert_eq!(tape.locals, 1);
        assert!(tape.insns.iter().any(|i| matches!(
            i,
            Insn::Binary {
                dst,
                op: BinaryOp::Mul,
                ..
            } if *dst == local
        )));
        assert!(!tape.insns.iter().any(|i| matches!(i, Insn::Copy { .. })));
        assert!(!tape.insns.iter().any(|i| matches!(i, Insn::Check { .. })));
    }

    #[test]
    fn a_leaf_assignment_becomes_a_copy() {
        let tape = tape_for("var x = x1;\nx + 1", &["x1"]);
        let local = R(tape.first_local());
        assert!(
            tape.insns
                .iter()
                .any(|i| matches!(i, Insn::Copy { dst, .. } if *dst == local))
        );
    }

    /// An aggregate reaches the tape as a flat fold: three terms, three combines,
    /// and nothing that jumps.
    #[test]
    fn an_aggregate_is_a_flat_fold() {
        let tape = tape_for("sum(1, 3, i -> i * x1)", &["x1"]);
        let combines = tape
            .insns
            .iter()
            .filter(|i| matches!(i, Insn::Combine { .. }))
            .count();
        assert_eq!(combines, 3);
    }

    #[test]
    fn every_checked_instruction_carries_the_span_of_its_node() {
        let tape = tape_for("sin(x1) + 2", &["x1"]);
        assert_eq!(
            tape.spans,
            vec![Span::new(4, 6), Span::new(0, 7), Span::new(0, 11)]
        );
        assert!(matches!(
            tape.insns[1],
            Insn::Unary {
                op: UnaryOp::Sin,
                ..
            }
        ));
    }

    /// A front-end slot bug is the only way to reach this; the tape still reports
    /// it as the walker did rather than reading a stale register.
    #[test]
    fn an_unassigned_local_read_gets_a_check_at_the_local() {
        let program = Program {
            body: Block {
                assignments: Vec::new(),
                result: Expr::new(Kind::Local(LocalSlot::from_index(0)), Span::new(0, 1)),
            },
            frame_size: 1,
        };
        let tape = super::lower(&program, &[], 0);
        assert_eq!(tape.insns, vec![Insn::Check { reg: R(0) }]);
        assert_eq!(tape.spans, vec![Span::new(0, 1)]);
        assert_eq!(tape.result, R(0));
    }
}
