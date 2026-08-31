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
use babel::{Ast, EvalError, Schema};

fn compile(expr: &str) -> Ast {
    babel::parse(expr)
        .unwrap_or_else(|e| panic!("unexpected compile failure for {expr:?}: {:#?}", e.problems))
}

#[test]
fn dynamic_index_out_of_bounds() {
    let expr = compile("sum(1, 3, i -> var[i] + var[x2] + i) + var[x2]");
    let err = babel::eval_one(&expr, &[("x1", 3.0), ("x2", 4.0)])
        .expect_err("var[4] with only 2 parameters should fail");

    match err {
        EvalError::Runtime(p) => assert_eq!(
            p.problem.kind,
            ProblemKind::DynamicIndexOutOfBounds {
                requested_1index: 4,
                available: 2
            }
        ),
        other => panic!("expected a runtime problem, got {other:?}"),
    }
}

#[test]
fn missing_statically_referenced_symbol_is_reported_at_bind_time() {
    let expr = compile("x1 + x2");

    // The JVM implementation re-checked this on every evaluate(); here it is a
    // property of the binding, so it surfaces once.
    let err =
        babel::compile(&expr, &Schema::new(["x1"])).expect_err("binding without x2 should fail");

    assert_eq!(err.missing, vec!["x2".to_owned()]);
}
/// Used to report `IllegalAggregateBound { Upper, INFINITY }`. The division now
/// fails first, and naming `20/x1` beats naming "the upper bound": the span
/// points at the subexpression that actually went wrong.
#[test]
fn an_infinite_upper_bound_fails_at_the_division() {
    let expr = compile("sum (\n  0,\n  20/x1,\n  i -> i + 2\n)");
    let err =
        babel::eval_one(&expr, &[("x1", 0.0)]).expect_err("20/0 as an upper bound should fail");

    match err {
        EvalError::Runtime(p) => {
            assert_eq!(
                p.problem.kind,
                ProblemKind::NonFiniteValue {
                    value: f64::INFINITY
                }
            );
            // Still the third line, zero-based — the division is the bound.
            assert_eq!(p.problem.line_idx, 2);
        }
        other => panic!("expected a runtime problem, got {other:?}"),
    }
}

#[test]
fn a_nan_lower_bound_fails_at_the_division() {
    let expr = compile("sum(0/x1, 20, i -> i + 2)");
    let err = babel::eval_one(&expr, &[("x1", 0.0)]).expect_err("0/0 as a lower bound should fail");

    match err {
        EvalError::Runtime(p) => {
            // NaN != NaN, so match the shape and check the value separately.
            match p.problem.kind {
                ProblemKind::NonFiniteValue { value } => {
                    assert!(value.is_nan(), "expected NaN, got {value}");
                }
                other => panic!("expected a non-finite value, got {other:?}"),
            }
            assert_eq!(p.problem.line_idx, 0);
        }
        other => panic!("expected a runtime problem, got {other:?}"),
    }
}

/// `IllegalAggregateBound` is now reachable only through a bound that is
/// perfectly finite and still not an index, since neither a NaN nor an infinity
/// can survive long enough to reach one. Pinned so the variant does not quietly
/// become dead.
#[test]
fn a_finite_bound_that_is_not_an_index_still_reports_as_one() {
    for (source, value, bound) in [
        ("sum(1, x1, i -> i)", 2.5, BoundKind::Upper),
        ("sum(x1, 20, i -> i)", 1e300, BoundKind::Lower),
    ] {
        let err = babel::eval_one(&compile(source), &[("x1", value)])
            .expect_err("a bound that is not an index should fail");

        match err {
            EvalError::Runtime(p) => assert_eq!(
                p.problem.kind,
                ProblemKind::IllegalAggregateBound { bound, value },
                "{source} at {value}"
            ),
            other => panic!("expected a runtime problem, got {other:?}"),
        }
    }
}

/// The other side of the rule: a non-finite value handed *in* is caught at the
/// variable rather than travelling into the arithmetic.
#[test]
fn a_non_finite_input_is_refused() {
    let expr = compile("x1 + x2");
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let err = babel::eval_one(&expr, &[("x1", bad), ("x2", 1.0)])
            .expect_err("a non-finite input should be refused");
        match err {
            EvalError::Runtime(p) => assert!(
                matches!(p.problem.kind, ProblemKind::NonFiniteValue { .. }),
                "got {:?} for {bad}",
                p.problem.kind
            ),
            other => panic!("expected a runtime problem, got {other:?}"),
        }
    }
}

/// The case the whole decision turns on. `ln(0)` is negative infinity, and
/// letting it travel is what forced `rewrite::monotone` to carry a domain floor
/// of `u >= 0` where the mathematics asks for `u > 0`.
#[test]
fn a_logarithm_of_zero_is_refused() {
    let err =
        babel::eval_one(&compile("ln(x1)"), &[("x1", 0.0)]).expect_err("ln(0) should be refused");

    match err {
        EvalError::Runtime(p) => assert_eq!(
            p.problem.kind,
            ProblemKind::NonFiniteValue {
                value: f64::NEG_INFINITY
            }
        ),
        other => panic!("expected a runtime problem, got {other:?}"),
    }
}

/// Overflow is a non-finite value like any other, and the case most likely to
/// be a nuisance rather than a service. Pinned so that if it ever needs
/// relaxing, a failing test says where the policy lives.
#[test]
fn overflow_is_refused() {
    let err = babel::eval_one(&compile("x1 * x1"), &[("x1", 1e200)])
        .expect_err("overflow should be refused");

    match err {
        EvalError::Runtime(p) => {
            assert!(matches!(p.problem.kind, ProblemKind::NonFiniteValue { .. }))
        }
        other => panic!("expected a runtime problem, got {other:?}"),
    }
}
