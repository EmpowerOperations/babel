//! The problem, in the form every strategy needs it: the box, the constraints
//! as written, and the constraints compiled — once.
//!
//! Immutable for the life of a solve. Strategies borrow it; nothing about it
//! is decided at run time.

use faer::MatRef;

use super::{ConstraintSystem, InputVariable, Point, SmtLogic};
use crate::{Ast, CompiledExpression, Schema};

/// A validated [`ConstraintSystem`] plus its compiled constraints and the
/// logic a solver document is emitted under.
///
/// Carries the logic rather than defaulting it at the point of use, so that a
/// document is emitted under the logic the caller chose and not under whatever
/// the worker thread's environment happens to say.
pub(crate) struct Problem {
    inputs: Vec<InputVariable>,
    constraints: Vec<Ast>,
    schema: Schema,
    logic: SmtLogic,
    /// Every constraint compiled once. The previous design rebuilt these on
    /// every batch.
    bounds: Vec<CompiledExpression>,
}

impl Problem {
    /// # Panics
    /// If a constraint does not bind to the box, which [`ConstraintSystem::new`]
    /// has already refused.
    pub(crate) fn new(system: ConstraintSystem, logic: SmtLogic) -> Self {
        let bounds = system
            .constraints
            .iter()
            .map(|constraint| {
                crate::compile(constraint, &system.schema)
                    .expect("`ConstraintSystem::new` proved every constraint binds")
            })
            .collect();
        Self {
            inputs: system.variables,
            constraints: system.constraints,
            schema: system.schema,
            logic,
            bounds,
        }
    }

    pub(crate) fn inputs(&self) -> &[InputVariable] {
        &self.inputs
    }

    pub(crate) fn constraints(&self) -> &[Ast] {
        &self.constraints
    }

    pub(crate) const fn schema(&self) -> &Schema {
        &self.schema
    }

    pub(crate) const fn logic(&self) -> &SmtLogic {
        &self.logic
    }

    /// The constraints as compiled, in order: what a backend over the tape
    /// renders.
    #[cfg_attr(
        not(feature = "gpu"),
        allow(dead_code, reason = "the GPU sieve is the only caller")
    )]
    pub(crate) fn compiled(&self) -> &[CompiledExpression] {
        &self.bounds
    }

    /// The declared box as `(low, high)` per variable: the shape
    /// [`fill_box`](super::sampling::fill_box) takes.
    pub(crate) fn box_bounds(&self) -> Vec<(f64, f64)> {
        self.inputs
            .iter()
            .map(|input| (input.lower_bound, input.upper_bound))
            .collect()
    }

    /// How badly the worst constraint is violated, or `None` if the point is
    /// outside the box or cannot be evaluated.
    ///
    /// [`is_feasible`](Self::is_feasible) asks a yes-or-no question; this asks
    /// *how far*, which is what the `<= 0` convention makes available and what a
    /// repair needs in order to know which way to step.
    pub(crate) fn worst_residual(&self, point: &Point) -> Option<f64> {
        if !self.in_box(point) {
            return None;
        }
        let mut worst = f64::NEG_INFINITY;
        for bound in &self.bounds {
            worst = worst.max(bound.eval_row(point).ok()?);
        }
        Some(worst)
    }

    /// The columns of `candidates` that are inside the box and satisfy every
    /// constraint, in column order, copied out as points.
    ///
    /// The batched twin of [`is_feasible`](Self::is_feasible), for the sources
    /// that propose thousands of independent candidates at once. A candidate
    /// whose evaluation faults — `sqrt` of a negative, a subscript out of
    /// range — is one the constraint does not hold for, exactly as
    /// `is_feasible` treats an `Err` per point.
    pub(crate) fn feasible_columns(&self, candidates: MatRef<'_, f64>) -> Vec<Point> {
        let rows = self.inputs.len();
        if candidates.nrows() != rows {
            return Vec::new();
        }
        let columns = candidates.ncols();

        let mut pass: Vec<bool> = (0..columns)
            .map(|column| {
                self.inputs
                    .iter()
                    .enumerate()
                    .all(|(row, input)| input.contains(candidates[(row, column)]))
            })
            .collect();

        for bound in &self.bounds {
            bound.holds(candidates, &mut pass).expect(
                "`ConstraintSystem::new` proved every constraint binds, and candidates are shaped by the same box",
            );
        }

        (0..columns)
            .filter(|&column| pass[column])
            .map(|column| (0..rows).map(|row| candidates[(row, column)]).collect())
            .collect()
    }

    /// Whether a point is inside the box and satisfies every constraint.
    pub(crate) fn is_feasible(&self, point: &Point) -> bool {
        if !self.in_box(point) {
            return false;
        }
        // `eval_row` rather than a one-column batch. This question is asked one
        // point at a time by nature — the walker cannot propose its next
        // candidate until it has judged this one — and wrapping each point in a
        // matrix cost five times the evaluation: `p118` ran 32s against 6s.
        self.bounds.iter().all(|bound| {
            bound
                .eval_row(point)
                .ok()
                // Babel's boolean rewrite yields a residual whose sign carries
                // the truth value: `<= 0` is satisfied. A non-finite residual is
                // an `Err` and not a pass.
                .is_some_and(|residual| residual <= 0.0)
        })
    }

    /// The points among `points` that are feasible, in order.
    ///
    /// What a caller's hints and a solver's witness go through before they
    /// count as points in hand: neither is trusted, only judged.
    pub(crate) fn keep_feasible(&self, points: Vec<Point>) -> Vec<Point> {
        points
            .into_iter()
            .filter(|point| self.is_feasible(point))
            .collect()
    }

    fn in_box(&self, point: &Point) -> bool {
        point.len() == self.inputs.len()
            && self
                .inputs
                .iter()
                .zip(point)
                .all(|(input, value)| input.contains(*value))
    }
}

/// How many coordinate sweeps a repair gets before it gives up.
///
/// A near-miss is a rounding error, so it yields in one or two passes or it was
/// never a near-miss. This is a cap on wasted work rather than a tuning knob.
const REPAIR_SWEEPS: usize = 4;

/// Nudges a solver's witness back onto the feasible side of `f64`.
///
/// A solver reasons in **exact real arithmetic** and answers with a witness that
/// is exactly on a boundary — asked for `x == pi +/- 0.001` it returns exactly
/// `pi - 0.001`, because a boundary is the simplest solution there is. The pool
/// then re-checks in `f64`, where `pi`, the tolerance, and the subtraction each
/// round, and the point lands a hair outside. Discarding it wastes the entire
/// solver call over an error in the last place.
///
/// This is not a general-purpose repair and does not pretend to be. It is a
/// bounded coordinate sweep: for each variable, try a step of a few ulps each
/// way and keep it if the worst residual falls. That reaches a point which is
/// *barely* outside, which is the only case a solver witness produces. It will
/// not rescue a point that is genuinely infeasible, and it should not.
///
/// Returns `None` when the point cannot be brought inside, which is then the
/// honest answer rather than a silent near-miss.
pub(crate) fn repaired(mut point: Point, problem: &Problem) -> Option<Point> {
    if problem.is_feasible(&point) {
        return Some(point);
    }

    for sweep in 0..REPAIR_SWEEPS {
        let mut improved = false;

        for index in 0..point.len() {
            let before = problem.worst_residual(&point)?;
            let original = point[index];

            // Growing the step across sweeps: an ulp first, because that is what
            // a boundary witness misses by, then wider in case the rounding
            // compounded through a longer expression.
            let step = ulps(original, 1 << (2 * sweep));

            for candidate in [original + step, original - step] {
                point[index] = candidate;
                let better = problem
                    .worst_residual(&point)
                    .is_some_and(|after| after < before);
                if better {
                    improved = true;
                    break;
                }
                point[index] = original;
            }
        }

        if problem.is_feasible(&point) {
            return Some(point);
        }
        if !improved {
            break;
        }
    }

    None
}

/// `count` units in the last place of `value`, as a distance.
///
/// Scaled to the value rather than absolute, because a witness near `1e-9` and
/// one near `1e9` miss by wildly different amounts and the same absolute step
/// would be useless for one and enormous for the other.
fn ulps(value: f64, count: u32) -> f64 {
    let magnitude = if value == 0.0 { 1.0 } else { value.abs() };
    f64::from(count) * (magnitude.next_up() - magnitude)
}

#[cfg(test)]
pub(crate) mod tests {
    use faer::Mat;

    use super::{Problem, repaired};
    use crate::Ast;
    use crate::cvg::{ConstraintSystem, InputVariable, Point, SmtLogic};

    pub(crate) fn compile_all(sources: &[&str]) -> Vec<Ast> {
        sources
            .iter()
            .map(|source| crate::parse(source).unwrap_or_else(|e| panic!("{source:?}: {e}")))
            .collect()
    }

    pub(crate) fn problem(inputs: Vec<InputVariable>, sources: &[&str]) -> Problem {
        let system =
            ConstraintSystem::new(inputs, compile_all(sources)).expect("the fixture binds");
        Problem::new(system, SmtLogic::default())
    }

    fn one_variable(source: &str) -> Problem {
        problem(vec![InputVariable::new("x1", 0.0, 10.0)], &[source])
    }

    /// Points as a matrix, one column each: the shape the batched judge takes.
    pub(crate) fn points_to_matrix(points: &[Point], rows: usize) -> Mat<f64> {
        Mat::from_fn(rows, points.len(), |row, column| points[column][row])
    }

    /// A witness one ulp outside is brought in; one genuinely outside is not.
    ///
    /// The first case is what a solver actually produces. Asked for
    /// `x1 == pi +/- 0.001` Z3 answers with the *boundary* — exactly
    /// `pi - 0.001` — because a boundary is the simplest solution there is. It
    /// reasons in exact reals; the pool re-checks in `f64`, where `pi`, the
    /// tolerance and the subtraction each round, and the point lands a hair
    /// outside. Before this existed the whole solver call was thrown away over
    /// that, and `cvg_pools::constants` passed only because the *previous*
    /// encoding happened to make Z3 pick the other edge, where the rounding
    /// went the other way. Luck, not correctness.
    ///
    /// The second case is the one that matters more: repair must not rescue a
    /// point that is simply infeasible, or `Unsatisfiable` stops meaning
    /// anything.
    #[test]
    fn a_boundary_witness_is_repaired_and_a_wrong_one_is_not() {
        let problem = one_variable("x1 == pi +/- 0.001");

        // The value Z3 actually returns, as a decimal parsed back into f64 —
        // not `PI - 0.001`, which Rust computes to a *different* f64 and which
        // happens to land inside. That difference is the entire bug.
        let edge: f64 = "3.140592653589793".parse().expect("a literal");
        assert!(
            !problem.is_feasible(&vec![edge]),
            "this test is pointless unless the boundary really does miss"
        );
        let repaired_edge = repaired(vec![edge], &problem).expect("a near-miss should be repaired");
        assert!(problem.is_feasible(&repaired_edge));
        assert!(
            (repaired_edge[0] - edge).abs() < 1e-12,
            "repair moved the point {} away from the witness, which is not a nudge",
            (repaired_edge[0] - edge).abs()
        );

        assert!(
            repaired(vec![7.0], &problem).is_none(),
            "a point nowhere near the band was 'repaired' into feasibility"
        );
    }

    /// `worst_residual` has to grade, not just judge — a repair steps downhill
    /// and there is no hill in a boolean.
    #[test]
    fn the_worst_residual_is_graded() {
        let problem = one_variable("x1 > 4");

        let near = problem.worst_residual(&vec![3.9]).expect("inside the box");
        let far = problem.worst_residual(&vec![1.0]).expect("inside the box");
        assert!(
            near < far,
            "{near} should be a smaller violation than {far}"
        );
        assert!(problem.worst_residual(&vec![5.0]).is_some_and(|r| r <= 0.0));
        assert!(
            problem.worst_residual(&vec![99.0]).is_none(),
            "outside the box is not a residual"
        );
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
        let problem = problem(
            vec![
                InputVariable::new("x1", 0.0, 10.0),
                InputVariable::new("x2", -5.0, 5.0),
            ],
            &["x1 > 4", "ln(x1) < 2", "x2 * x2 < 5"],
        );

        let points = candidates();
        let matrix = points_to_matrix(&points, 2);
        let batched = problem.feasible_columns(matrix.as_ref());
        let one_at_a_time = problem.keep_feasible(points);

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
        let problem = one_variable("sqrt(x1 - 5) + x1 < 6");

        let points: Vec<Point> = (0..100).map(|i| vec![f64::from(i) * 0.1]).collect();
        let matrix = points_to_matrix(&points, 1);

        // The strict evaluator refuses the batch outright: that is what lenient
        // judging exists to get past.
        let strict = crate::compile(&problem.constraints()[0], problem.schema()).unwrap();
        assert!(strict.eval(matrix.as_ref()).is_err());

        let feasible = problem.feasible_columns(matrix.as_ref());
        assert!(!feasible.is_empty());
        for point in &feasible {
            assert!(point[0] >= 5.0 && point[0] < 6.0, "{point:?}");
        }
        assert_eq!(feasible, problem.keep_feasible(points));
    }

    #[test]
    fn a_candidate_matrix_of_the_wrong_height_yields_nothing() {
        let problem = one_variable("x1 > 1");
        let wrong = Mat::from_fn(2, 5, |_, _| 5.0);
        assert!(problem.feasible_columns(wrong.as_ref()).is_empty());
    }
}
