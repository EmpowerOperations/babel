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
