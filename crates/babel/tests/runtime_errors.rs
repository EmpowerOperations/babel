//! Run-time diagnostics, ported from `BabelRuntimeErrorFixture.kt`.
//!
//! As with the compile-time fixture, only structured fields are asserted; the
//! Kotlin tests compared rendered messages that also embedded the local-variable
//! and parameter maps.
//!
//! One Kotlin case is deliberately **not** ported: `when running with an
//! un-ordered hashmap as globals should eagerly throw`. That test guards a
//! failure mode created by the JVM API taking a `Map<String, Double>` whose
//! iteration order might be arbitrary. [`babel::Schema`] is an ordered
//! `Vec<String>` by construction, so the failure mode does not exist here.

use babel::diagnostics::{BoundKind, ProblemKind};
use babel::{EvalError, Expression, Schema};

fn compile(expr: &str) -> Expression {
    babel::compile(expr)
        .unwrap_or_else(|e| panic!("unexpected compile failure for {expr:?}: {:#?}", e.problems))
}

#[test]
fn dynamic_index_out_of_bounds() {
    let expr = compile("sum(1, 3, i -> var[i] + var[x2] + i) + var[x2]");
    let err = expr
        .evaluate(&[("x1", 3.0), ("x2", 4.0)])
        .expect_err("var[4] with only 2 parameters should fail");

    match err {
        EvalError::Runtime(p) => assert_eq!(
            p.kind,
            ProblemKind::IndexOutOfBounds { requested: 4, available: 2 }
        ),
        other => panic!("expected a runtime problem, got {other:?}"),
    }
}

#[test]
fn missing_statically_referenced_symbol_is_reported_at_bind_time() {
    let expr = compile("x1 + x2");

    // The JVM implementation re-checked this on every evaluate(); here it is a
    // property of the binding, so it surfaces once.
    let err = expr
        .bind(&Schema::new(["x1"]))
        .expect_err("binding without x2 should fail");

    assert_eq!(err.missing, vec!["x2".to_owned()]);
}

#[test]
fn infinite_upper_bound() {
    let expr = compile("sum (\n  0,\n  20/x1,\n  i -> i + 2\n)");
    let err = expr
        .evaluate(&[("x1", 0.0)])
        .expect_err("20/0 as an upper bound should fail");

    match err {
        EvalError::Runtime(p) => {
            assert_eq!(p.kind, ProblemKind::IllegalAggregateBound(BoundKind::Upper));
            assert_eq!(p.detail, "evaluates to Infinity");
            // The bound sits on the third line of the expression.
            assert_eq!(p.line, 3);
        }
        other => panic!("expected a runtime problem, got {other:?}"),
    }
}

#[test]
fn nan_lower_bound_at_runtime() {
    let expr = compile("sum(0/x1, 20, i -> i + 2)");
    let err = expr
        .evaluate(&[("x1", 0.0)])
        .expect_err("0/0 as a lower bound should fail");

    match err {
        EvalError::Runtime(p) => {
            assert_eq!(p.kind, ProblemKind::IllegalAggregateBound(BoundKind::Lower));
            assert_eq!(p.detail, "evaluates to NaN");
            assert_eq!(p.line, 1);
        }
        other => panic!("expected a runtime problem, got {other:?}"),
    }
}
