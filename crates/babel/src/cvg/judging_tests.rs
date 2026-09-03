//! Batched judging of candidates against the per-point question it replaced.

use faer::Mat;

use super::{InputVariable, Point, SearchContext};
use crate::{Ast, Schema};

fn context_for<'a>(
    inputs: &'a [InputVariable],
    constraints: &'a [Ast],
    schema: &'a Schema,
) -> SearchContext<'a> {
    SearchContext::new(inputs, constraints, schema)
}

fn compile_all(sources: &[&str]) -> Vec<Ast> {
    sources
        .iter()
        .map(|source| crate::parse(source).unwrap_or_else(|e| panic!("{source:?}: {e}")))
        .collect()
}

/// A grid of candidates, some deliberately outside the box.
fn candidates() -> Vec<Point> {
    let mut points = Vec::new();
    for i in 0..40 {
        let x1 = f64::from(i) * 0.3 - 1.0; // -1 .. 10.7, past both ends of 0..10
        let x2 = f64::from(i % 7) - 3.0;
        points.push(vec![x1, x2]);
    }
    points
}

#[test]
fn batched_judging_agrees_with_per_point_is_feasible() {
    let inputs = vec![
        InputVariable::new("x1", 0.0, 10.0),
        InputVariable::new("x2", -5.0, 5.0),
    ];
    let constraints = compile_all(&["x1 > 4", "ln(x1) < 2", "x2 * x2 < 5"]);
    let schema = Schema::new(["x1", "x2"]);
    let context = context_for(&inputs, &constraints, &schema);

    let points = candidates();
    let matrix = super::points_to_matrix(&points, 2);
    let batched = context.feasible_columns(matrix.as_ref());
    let one_at_a_time: Vec<Point> = points
        .iter()
        .filter(|point| context.is_feasible(point))
        .cloned()
        .collect();

    assert!(
        !batched.is_empty(),
        "the grid should contain feasible points"
    );
    assert_eq!(batched, one_at_a_time);
}

/// `sqrt(x1 - 5)` is NaN for every candidate below five, and the front end
/// leaves it alone because the root is not the whole side of the comparison
/// (`ln(x1 - 5) < 0` would be inverted into plain bounds and never fault).
/// Those candidates are infeasible; the batch still returns the ones above.
#[test]
fn a_faulting_candidate_is_infeasible_rather_than_fatal() {
    let inputs = vec![InputVariable::new("x1", 0.0, 10.0)];
    let constraints = compile_all(&["sqrt(x1 - 5) + x1 < 6"]);
    let schema = Schema::new(["x1"]);
    let context = context_for(&inputs, &constraints, &schema);

    let points: Vec<Point> = (0..100).map(|i| vec![f64::from(i) * 0.1]).collect();
    let matrix = super::points_to_matrix(&points, 1);

    // The strict evaluator refuses the batch outright: that is what lenient
    // judging exists to get past.
    let strict = crate::compile(&constraints[0], &schema).unwrap();
    assert!(strict.eval(matrix.as_ref()).is_err());

    let feasible = context.feasible_columns(matrix.as_ref());
    assert!(!feasible.is_empty());
    for point in &feasible {
        assert!(point[0] >= 5.0 && point[0] < 6.0, "{point:?}");
    }
    let expected: Vec<Point> = points
        .iter()
        .filter(|point| context.is_feasible(point))
        .cloned()
        .collect();
    assert_eq!(feasible, expected);
}

#[test]
fn a_candidate_matrix_of_the_wrong_height_yields_nothing() {
    let inputs = vec![InputVariable::new("x1", 0.0, 10.0)];
    let constraints = compile_all(&["x1 > 1"]);
    let schema = Schema::new(["x1"]);
    let context = context_for(&inputs, &constraints, &schema);
    let wrong = Mat::from_fn(2, 5, |_, _| 5.0);
    assert!(context.feasible_columns(wrong.as_ref()).is_empty());
}
