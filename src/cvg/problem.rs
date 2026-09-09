//! The problem, in the form every strategy needs it: the box, the constraints
//! as written, and the constraints compiled — once.
//!
//! Immutable for the life of a solve. Strategies borrow it; nothing about it
//! is decided at run time.

use faer::{Mat, MatRef};
use rand::RngExt;
use rand::rngs::Xoshiro256PlusPlus;

use super::incidence::{ConstraintId, Incidence, Row};
use super::interval::Interval;
use super::{ConstraintSystem, InputVariable, Point, SmtLogic, classify, interval};
use crate::ast::GlobalId;
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
    /// Which coordinates are computed from the others, when any are. See
    /// [`classify`](super::classify).
    plan: Option<classify::Plan>,
    /// Which constraints name which coordinates, both ways round.
    ///
    /// `slice` narrows one coordinate per move and needs only the constraints
    /// that mention it; without the graph that is a scan of every constraint
    /// each time, which on two hundred variables under two hundred constraints
    /// is forty thousand scans a sweep to do two hundred narrowings.
    incidence: Incidence,
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
        let plan = classify::plan(&system.constraints, &system.schema);
        // What counts as affected by *any* move, whichever coordinate it
        // touched. Both entries here are soundness rather than efficiency.
        let mut always: Vec<ConstraintId> = Vec::new();

        // A computed subscript reads a column chosen by the point, so nothing
        // static says which and no symbol list names it. Skipping such a
        // constraint would mean skipping one that *had* changed.
        always.extend(
            system
                .constraints
                .iter()
                .enumerate()
                .filter(|(_, constraint)| constraint.contains_dynamic_lookup())
                .map(|(position, _)| ConstraintId(position)),
        );

        // `retract` rewrites every driven coordinate on every move, so whatever
        // names one of those is in play whichever axis was swept. Read off the
        // naming direction, which is why the graph is built before this.
        let incidence = Incidence::of(&system.constraints, &system.schema);
        if let Some(plan) = &plan {
            for driven in plan.driven() {
                always.extend_from_slice(incidence.naming(Row(*driven)));
            }
        }
        let incidence = incidence.with_always(&always);

        Self {
            inputs: system.variables,
            constraints: system.constraints,
            schema: system.schema,
            logic,
            bounds,
            plan,
            incidence,
        }
    }

    /// The interval `coordinate` may take with every other coordinate held at
    /// its value in `point`.
    ///
    /// The declared box, narrowed by each constraint in turn through
    /// [`interval::narrow`]. **A superset of the feasible slice**, so a value
    /// drawn from it is still judged by [`is_feasible`](Self::is_feasible) like
    /// any other candidate — see [`interval`] for why that
    /// leaves the distribution alone.
    ///
    /// Constraints are applied in order and each sees what the previous ones
    /// concluded, so a system narrows further than any one of its constraints
    /// would. Nothing here iterates to a fixpoint: every coordinate but this one
    /// is a point, which leaves a second pass with nothing to tighten.
    pub(crate) fn slice(&self, point: &Point, coordinate: usize) -> Interval {
        self.slice_over(point, coordinate, None)
    }

    /// [`slice`](Self::slice), optionally ignoring constraints that mention a
    /// coordinate whose value is about to change.
    ///
    /// `settled[row]` says the point's value there is one to condition on. A
    /// constraint naming an unsettled coordinate is skipped, because narrowing
    /// against a value that is about to be overwritten conditions on a stale
    /// number — see [`retract`](Self::retract), which is the only caller that
    /// passes anything.
    fn slice_over(
        &self,
        point: &Point,
        coordinate: usize,
        settled: Option<&[bool]>,
    ) -> Interval {
        let input = &self.inputs[coordinate];
        let mut interval = Interval::new(input.lower_bound, input.upper_bound);

        let coordinate = Row(coordinate);
        for id in self.incidence.naming(coordinate) {
            let rows = self.incidence.rows_of(*id);
            if let Some(settled) = settled
                && rows
                    .iter()
                    .any(|row| *row != coordinate && !settled[row.index()])
            {
                continue;
            }
            let wanted = rows
                .iter()
                .position(|row| *row == coordinate)
                .expect("`naming` lists only constraints that name the coordinate");

            // The constraint's symbols, in its own order, as intervals: a point
            // for everything held, and the running narrowing for the one asked
            // about.
            let globals: Vec<Interval> = rows
                .iter()
                .enumerate()
                .map(|(symbol, row)| {
                    if symbol == wanted {
                        interval
                    } else {
                        Interval::point(point[row.index()])
                    }
                })
                .collect();

            let wanted = u32::try_from(wanted).expect("fewer than four billion symbols");
            interval = interval.intersect(interval::narrow(
                &self.constraints[id.index()],
                &globals,
                GlobalId::from_index(wanted),
            ));
            if interval.is_empty() {
                break;
            }
        }

        interval
    }

    /// The coordinates a search may move, or `None` when nothing is driven.
    pub(crate) fn free_coordinates(&self) -> Option<&[usize]> {
        self.plan.as_ref().map(classify::Plan::free)
    }

    /// Recomputes every driven coordinate from the free ones, in place.
    ///
    /// A point on an equality surface leaves it under almost any move, because
    /// the surface has no volume — which is why a walker that moves every
    /// coordinate independently is reduced to jitter around wherever it started.
    /// Driving is the answer: move what is free, and compute the rest.
    ///
    /// # Why this samples rather than evaluates
    ///
    /// The obvious version assigns `y = f(free)` and is **wrong**, because
    /// babel has no bare equality: `y == f(x) +/- t` admits the whole band, and
    /// collapsing it to its centre line throws away a dimension of the feasible
    /// region. On `xi == 10.75 +/- 0.2` over two hundred variables that is not
    /// subtle — every coordinate pins to `10.75` and the pool returns the same
    /// point two hundred times, which is exactly what it did before this
    /// sampled.
    ///
    /// So it draws uniformly from `f(free) ± t`. That is not a fudge, it is a
    /// **Gibbs step**: with the other coordinates held, the feasible slice for a
    /// driven variable *is* that interval, and drawing uniformly from a
    /// conditional slice is the move that leaves the uniform distribution
    /// invariant. Evaluating to the centre would not.
    ///
    /// The slice comes from [`slice`](Self::slice) rather than from the one
    /// equality that defined the coordinate, so **every** constraint mentioning
    /// it has a say. The band `f(free) ± t` is what that equality contributes
    /// and the intersection can only be tighter, which turns draws that used to
    /// land outside the other constraints and be rejected into draws that
    /// cannot.
    ///
    /// Silent about failure by design. A coordinate whose slice comes back
    /// empty is left alone, and the candidate is judged exactly as an
    /// unretracted one would be. **Feasibility is never assumed from a
    /// successful retraction** — a wrong drive costs rejected moves, not wrong
    /// points, and that is what makes this safe to apply without proving it.
    ///
    /// Order still matters, and for the same reason: a driven coordinate may be
    /// defined in terms of another, and `slice` reads the point as it stands,
    /// so computing them out of order reads a stale value.
    pub(crate) fn retract(&self, point: &mut Point, rng: &mut Xoshiro256PlusPlus) {
        let Some(plan) = &self.plan else {
            return;
        };
        // A driven coordinate holds a value that is about to be replaced, so
        // narrowing against a constraint that mentions one still waiting its
        // turn conditions on a stale number. That is not merely wasteful, it
        // **destroys the freedom driving exists to exploit**: on
        // `y == sin(x) +/- t` with `z == y + 1 +/- t`, conditioning `y` on the
        // current `z` pins it within `t` of `z - 1`, and then `z` is pinned
        // within `t` of the new `y`. The pair shuffles by `t` a sweep instead
        // of travelling, and `two_coupled_equalities_are_traversed` measured it
        // as three occupied cells of eighty where twenty-four are wanted.
        //
        // So a coordinate becomes conditionable only once it has been drawn.
        // Everything free is conditionable from the start.
        let mut settled = vec![true; point.len()];
        for driven in plan.driven() {
            settled[*driven] = false;
        }

        for driven in plan.driven().iter().copied() {
            let slice = self.slice_over(point, driven, Some(&settled));
            if !slice.is_empty() {
                point[driven] = if slice.width() > 0.0 {
                    rng.random_range(slice.lo()..=slice.hi())
                } else {
                    slice.lo()
                };
            }
            settled[driven] = true;
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

    /// [`retract`](Self::retract) over every column of a candidate batch.
    ///
    /// Returns immediately when nothing is driven, which is every problem
    /// without an equality — so the batch path pays nothing for this.
    pub(crate) fn retract_columns(&self, candidates: &mut Mat<f64>, rng: &mut Xoshiro256PlusPlus) {
        if self.plan.is_none() {
            return;
        }
        let rows = candidates.nrows();
        for column in 0..candidates.ncols() {
            let mut point: Point = (0..rows).map(|row| candidates[(row, column)]).collect();
            self.retract(&mut point, rng);
            for (row, value) in point.into_iter().enumerate() {
                candidates[(row, column)] = value;
            }
        }
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

    /// [`is_feasible`](Self::is_feasible), for a point that differs from a
    /// **feasible** one only in `moved` and in whatever [`retract`](Self::retract)
    /// recomputed.
    ///
    /// # Why this is exact and not an approximation
    ///
    /// A constraint that names none of the coordinates that moved evaluates to
    /// the number it evaluated to before, and that number was `<= 0` — so
    /// re-deriving it is arithmetic nobody reads. Only the constraints in
    /// `affected[moved]` can have changed, and the box only needs re-checking
    /// where the point actually moved.
    ///
    /// # Why it matters
    ///
    /// The walker judges one candidate per proposal and every axis move touches
    /// one coordinate, so the full check was re-evaluating the whole system to
    /// learn about one column. On `top_corner_200d` that is two hundred
    /// constraints asked in order to consult one, and the walker's cost is
    /// almost entirely this call.
    ///
    /// # The precondition is the caller's
    ///
    /// The point this was derived from **must** have been feasible. `advance`
    /// holds that by the chain's invariant. Anywhere that does not, use
    /// [`is_feasible`](Self::is_feasible).
    pub(crate) fn is_feasible_after(&self, point: &Point, moved: usize) -> bool {
        if point.len() != self.inputs.len() {
            return false;
        }
        if !self.inputs[moved].contains(point[moved]) {
            return false;
        }
        if let Some(plan) = &self.plan {
            for driven in plan.driven() {
                if !self.inputs[*driven].contains(point[*driven]) {
                    return false;
                }
            }
        }

        self.incidence.affected(Row(moved)).iter().all(|id| {
            self.bounds[id.index()]
                .eval_row(point)
                .ok()
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

    use rand::RngExt;
    use rand::rngs::Xoshiro256PlusPlus;
    use rand::SeedableRng;

    use super::super::incidence::{ConstraintId, Row};
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

    /// **The restricted check must answer what the full one answers.**
    ///
    /// `is_feasible_after` skips every constraint that names none of the
    /// coordinates that moved, on the grounds that their residuals cannot have
    /// changed. If that reasoning is wrong anywhere — a missing entry in
    /// `mentions`, a driven coordinate left out of `affected`, a `var[i]`
    /// reading a column nothing declared — the walker starts accepting points
    /// the full check would reject, and every one of them reaches the caller.
    ///
    /// So this runs both and requires them to agree, over fixtures with and
    /// without drives, on candidates built the way `advance` builds them:
    /// take a feasible point, move one coordinate, retract.
    #[test]
    fn the_restricted_feasibility_check_agrees_with_the_full_one() {
        let fixtures: [(Vec<InputVariable>, &[&str]); 4] = [
            (
                vec![
                    InputVariable::new("x1", -10.0, 10.0),
                    InputVariable::new("x2", -10.0, 10.0),
                    InputVariable::new("x3", -10.0, 10.0),
                ],
                // Separable: each constraint names one coordinate, which is the
                // case the skipping is worth the most on.
                &["x1 < 3", "x2 > -4", "x3 < 8"],
            ),
            (
                vec![
                    InputVariable::new("x1", -10.0, 10.0),
                    InputVariable::new("x2", -10.0, 10.0),
                    InputVariable::new("x3", -10.0, 10.0),
                ],
                // Coupled, so most constraints are in play whatever moved.
                &["x1 + x2 < 3", "x2 * x3 > -20", "x1 - x3 < 6"],
            ),
            (
                vec![
                    InputVariable::new("x", -4.0, 4.0),
                    InputVariable::new("y", -8.0, 8.0),
                    InputVariable::new("z", -8.0, 8.0),
                ],
                // Drives: `retract` moves `y` and `z` whatever was swept, so
                // `affected` has to carry the constraints naming them.
                &["y == sin(x) +/- 0.05", "z == y + 1 +/- 0.05", "x + z < 6"],
            ),
            (
                vec![
                    InputVariable::new("x1", -10.0, 10.0),
                    InputVariable::new("x2", -10.0, 10.0),
                ],
                // A subscript reads a column the expression never names.
                &["var[1] + var[2] < 4"],
            ),
        ];

        let mut rng = Xoshiro256PlusPlus::seed_from_u64(0x00A6_9EED);
        for (inputs, sources) in fixtures {
            let bounds: Vec<(f64, f64)> = inputs
                .iter()
                .map(|input| (input.lower_bound, input.upper_bound))
                .collect();
            let problem = problem(inputs, sources);

            let mut compared = 0_usize;
            for _ in 0..20_000 {
                let mut point: Point = bounds
                    .iter()
                    .map(|(low, high)| rng.random_range(*low..=*high))
                    .collect();
                // Retracted first, exactly as the walker reaches a feasible
                // point: on a system of tight equalities the feasible set has
                // no volume, and uniform draws found three of twenty thousand.
                problem.retract(&mut point, &mut rng);
                // The precondition: the restricted check is only sound about a
                // point derived from a feasible one.
                if !problem.is_feasible(&point) {
                    continue;
                }

                let moved = rng.random_range(0..point.len());
                let mut candidate = point.clone();
                candidate[moved] = rng.random_range(bounds[moved].0..=bounds[moved].1);
                problem.retract(&mut candidate, &mut rng);

                assert_eq!(
                    problem.is_feasible_after(&candidate, moved),
                    problem.is_feasible(&candidate),
                    "{sources:?}: moving coordinate {moved} of {point:?} to \
                     {candidate:?} is judged differently by the two checks"
                );
                compared += 1;
            }
            assert!(
                compared > 100,
                "{sources:?}: only {compared} candidates were derived from a \
                 feasible point, so this asserted almost nothing"
            );
        }
    }

    /// A computed subscript reads a column nothing names statically, so the
    /// constraint holding it can never be skipped.
    ///
    /// This is asserted structurally rather than by sampling, because
    /// `var[n]` needs `n` to land on a whole number and a uniform draw almost
    /// never does — the sampling test above would have found no feasible point
    /// to compare, and reported that rather than this.
    #[test]
    fn a_computed_subscript_is_never_skipped() {
        let problem = problem(
            vec![
                InputVariable::new("n", 1.0, 2.0),
                InputVariable::new("x2", -10.0, 10.0),
            ],
            &["var[n] < 4", "x2 < 3"],
        );
        assert!(
            problem.constraints[0].contains_dynamic_lookup(),
            "the fixture stopped exercising a computed subscript"
        );

        for coordinate in 0..2 {
            assert!(
                problem.incidence.affected(Row(coordinate)).contains(&ConstraintId(0)),
                "coordinate {coordinate} may move the column `var[n]` reads"
            );
        }
    }

    /// **The invariant `slice` exists to keep.**
    ///
    /// A feasible point satisfies every constraint, so each of its coordinates
    /// is in that coordinate's true feasible slice — and the narrowing is a
    /// *superset* of that slice, so it must contain the coordinate too. If it
    /// ever does not, the walker is being handed an interval that excludes
    /// where it is standing, and the points it cannot propose are gone from the
    /// answer silently.
    ///
    /// This is the check that separates the two ways narrowing can be wrong.
    /// Too wide only costs a rejected proposal; too narrow biases, and shows up
    /// here.
    #[test]
    fn a_feasible_point_is_inside_every_slice_it_sits_in() {
        let fixtures: [(Vec<InputVariable>, &[&str]); 5] = [
            (
                vec![
                    InputVariable::new("x1", -10.0, 10.0),
                    InputVariable::new("x2", -10.0, 10.0),
                ],
                &["x1 + x2 == 3 +/- 0.1"],
            ),
            (
                vec![
                    InputVariable::new("x1", -10.0, 10.0),
                    InputVariable::new("x2", -10.0, 10.0),
                ],
                &["x1 + x2 < 3", "x1 - x2 > -4"],
            ),
            (
                vec![
                    InputVariable::new("x1", 0.5, 10.0),
                    InputVariable::new("x2", 0.5, 10.0),
                ],
                &["x1 / x2 < 4", "ln(x1) < 2"],
            ),
            (
                vec![
                    InputVariable::new("x1", -10.0, 10.0),
                    InputVariable::new("x2", -10.0, 10.0),
                ],
                &["x1 * x2 == 6 +/- 0.5"],
            ),
            (
                vec![InputVariable::new("x1", 10.0, 11.0)],
                &["x1 > 10.5"],
            ),
        ];

        let mut rng = Xoshiro256PlusPlus::seed_from_u64(0x0051_1CE5);
        for (inputs, sources) in fixtures {
            let bounds: Vec<(f64, f64)> = inputs
                .iter()
                .map(|input| (input.lower_bound, input.upper_bound))
                .collect();
            let problem = problem(inputs, sources);

            let mut feasible_seen = 0_usize;
            for _ in 0..20_000 {
                let point: Point = bounds
                    .iter()
                    .map(|(low, high)| rng.random_range(*low..=*high))
                    .collect();
                if !problem.is_feasible(&point) {
                    continue;
                }
                feasible_seen += 1;
                for coordinate in 0..point.len() {
                    let slice = problem.slice(&point, coordinate);
                    assert!(
                        slice.contains(point[coordinate]),
                        "{sources:?}: the feasible point {point:?} has coordinate \
                         {coordinate} narrowed out of [{}, {}]",
                        slice.lo(),
                        slice.hi()
                    );
                }
            }
            assert!(
                feasible_seen > 0,
                "{sources:?}: no feasible point was drawn, so this asserted nothing"
            );
        }
    }

    /// **`slice` can only tighten what the equality already said.**
    ///
    /// `retract` used to draw from `f(free) ± t`, evaluated from the one
    /// equality that defined the coordinate. It now draws from `slice`, which
    /// starts at the declared box and applies every constraint naming that
    /// coordinate — the defining equality among them. So the new interval is
    /// contained in the old one, and the change is a narrowing rather than a
    /// different claim.
    ///
    /// The band is computed here from **Rust's own `sin`** rather than read off
    /// a `Drive`. That is the point: nothing in production carries the
    /// definition any more, so a test that asked production what the band was
    /// would be asking the thing under test. If this ever fails, `slice` is
    /// admitting values the equality forbids.
    const TOLERANCE: f64 = 0.05;

    #[test]
    fn a_slice_is_never_wider_than_the_band_its_equality_defines() {
        let inputs = vec![
            InputVariable::new("x", -4.0, 4.0),
            InputVariable::new("y", -8.0, 8.0),
            InputVariable::new("z", -8.0, 8.0),
        ];
        let bounds: Vec<(f64, f64)> = inputs
            .iter()
            .map(|input| (input.lower_bound, input.upper_bound))
            .collect();
        let problem = problem(inputs, &["y == sin(x) +/- 0.05", "z == y + 1 +/- 0.05"]);

        /// A driven coordinate and what its equality says its centre is.
        /// `x` is free and has no band, so it is absent.
        type Band = (usize, fn(&Point) -> f64);

        let bands: [Band; 2] = [(1, |point| point[0].sin()), (2, |point| point[1] + 1.0)];

        let mut rng = Xoshiro256PlusPlus::seed_from_u64(0x0000_BA4D);
        for _ in 0..5_000 {
            let point: Point = bounds
                .iter()
                .map(|(low, high)| rng.random_range(*low..=*high))
                .collect();

            for (position, centre_of) in bands {
                let centre = centre_of(&point);
                let slice = problem.slice(&point, position);
                if slice.is_empty() {
                    continue;
                }
                // Narrowing pads outward once per operation it passes through,
                // so the interval exceeds the band by a rounding step per node.
                // The slack is *relative* rather than counted in ulps, because
                // an ulp is not a stable unit across a subtraction that
                // cancels: the padding is added while the intermediate is
                // around `centre`, and the endpoint it lands on can be an order
                // of magnitude smaller, where ulps are an order of magnitude
                // finer. Measured in ulps of the endpoint, two ulps of real
                // padding read as nine.
                let slack = 16.0 * f64::EPSILON * (1.0 + centre.abs() + TOLERANCE);
                assert!(
                    slice.lo() >= centre - TOLERANCE - slack
                        && slice.hi() <= centre + TOLERANCE + slack,
                    "coordinate {position} narrowed to [{}, {}], outside the                      band {centre} +/- {TOLERANCE} its equality defines",
                    slice.lo(),
                    slice.hi()
                );
            }
        }
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
