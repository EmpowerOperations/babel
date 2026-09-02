//! The per-lane executor's loops, on the cases a straight-line tape cannot
//! reach.

use crate::eval_one;

#[test]
fn an_empty_runtime_range_is_the_identity() {
    let sum = crate::parse("sum(x1, x2, i -> i)").expect("compiles");
    assert_eq!(eval_one(&sum, &[("x1", 5.0), ("x2", 1.0)]).unwrap(), 0.0);
    let prod = crate::parse("prod(x1, x2, i -> i)").expect("compiles");
    assert_eq!(eval_one(&prod, &[("x1", 5.0), ("x2", 1.0)]).unwrap(), 1.0);
}

/// The load cache is restored after a loop body: a variable first read inside
/// a body that never ran must still be loaded for a read after.
#[test]
fn a_runtime_loop_restores_a_cached_load() {
    let expression = crate::parse("sum(x1, x2, i -> x3) + x3").expect("compiles");
    let value = eval_one(&expression, &[("x1", 5.0), ("x2", 1.0), ("x3", 7.0)]).unwrap();
    assert_eq!(value, 7.0);
}

#[test]
fn nested_runtime_loops_nest() {
    let expression = crate::parse("sum(x1, x2, i -> sum(x1, x2, j -> i * j))").expect("compiles");
    // (1 + 2 + 3)^2
    assert_eq!(
        eval_one(&expression, &[("x1", 1.0), ("x2", 3.0)]).unwrap(),
        36.0
    );
}
