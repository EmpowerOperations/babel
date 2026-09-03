//! Allocation: how many registers a tape needs, and which it must not share.

use super::super::tape::Insn;
use super::super::tape_for;

#[test]
fn a_dead_temporary_frees_its_register() {
    // Two loads, two intermediates, one result: the loads die at the second
    // intermediate and the result takes a freed register.
    let tape = tape_for("(x1 + x2) + (x1 - x2)", &["x1", "x2"]);
    assert_eq!(tape.registers, 4, "{:?}", tape.insns);
}

#[test]
fn an_eight_term_addition_chain_needs_at_most_three_temporaries() {
    let names = ["x1", "x2", "x3", "x4", "x5", "x6", "x7", "x8"];
    let tape = tape_for("x1 + x2 + x3 + x4 + x5 + x6 + x7 + x8", &names);
    assert!(
        tape.registers <= 3,
        "{} registers: {:?}",
        tape.registers,
        tape.insns
    );
}

#[test]
fn the_two_hundred_term_fold_needs_fewer_than_eight_registers() {
    let names: Vec<String> = (1..=200).map(|i| format!("x{i}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let tape = tape_for("sum(1, 200, i -> var[i]^2 - 3.0)", &names);
    let temps = tape.registers - tape.first_local() - tape.locals;
    assert!(temps < 8, "{temps} temporaries");
    assert!(tape.consts.len() <= 4, "{:?}", tape.consts);
    // Two hundred loads, and none of the subscripts became a constant.
    assert_eq!(
        tape.insns
            .iter()
            .filter(|i| matches!(i, Insn::Load { .. }))
            .count(),
        200
    );
}

#[test]
fn a_binary_destination_never_aliases_an_operand() {
    for (source, names) in [
        ("x1 * x1 + x1", vec!["x1"]),
        ("(x1 + x2) * (x1 + x2)", vec!["x1", "x2"]),
        ("max(x1, x1 - x2) < x2 * x2", vec!["x1", "x2"]),
        ("x1 == x2 * x2 +/- 0.1", vec!["x1", "x2"]),
    ] {
        let tape = tape_for(source, &names);
        for insn in &tape.insns {
            if let Some(dst) = insn.dst()
                && !matches!(insn, Insn::Combine { .. })
            {
                assert!(
                    !insn.sources().contains(&dst),
                    "{source:?}: {insn:?} writes an operand"
                );
            }
        }
    }
}

#[test]
fn constants_and_locals_are_never_reused() {
    let tape = tape_for("var x = x1 * 2;\nvar y = x + 3;\ny * y - x", &["x1"]);
    assert_eq!(tape.consts, vec![2.0, 3.0]);
    assert_eq!(tape.locals, 2);
    for insn in &tape.insns {
        if let Some(dst) = insn.dst() {
            assert!(
                dst.0 >= tape.first_local(),
                "{insn:?} writes a constant register"
            );
        }
    }
    // The locals are written exactly by their assignments, never by a
    // temporary's allocation: only two instructions target local registers.
    let first_temp = tape.first_local() + tape.locals;
    let writes_to_locals = tape
        .insns
        .iter()
        .filter_map(Insn::dst)
        .filter(|dst| dst.0 >= tape.first_local() && dst.0 < first_temp)
        .count();
    assert_eq!(writes_to_locals, 2);
}
