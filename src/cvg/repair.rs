//! A feasible point near a given one, deterministically.
//!
//! The consumer is an optimizer that cannot evaluate its objective at an
//! infeasible point — the constraints encode things like mesh validity, and a
//! bad point is a crashed solver rather than a bad number. Every point it
//! proposes that fails the constraints comes here, thousands of times a run,
//! and must come back feasible by the same oracle the search uses, near where
//! it wanted to be, and the same every time it is asked. The contract and the
//! alternatives it displaced are written up in `docs/todo.md` under *Repair
//! for Artemis*; this is the shape that survived.
//!
//! # Two stages, in series
//!
//! **Clamp.** Every coordinate has a conditional slice — the interval it may
//! occupy with the others held where they are, from [`ConstraintSystem::slice`]
//! — and clamping into it is the exact axis projection onto that coordinate's
//! constraints. Clamps are applied cheapest first, cumulatively, and the point
//! is judged after each. Cheapest first matters: the L1 projection onto a
//! half-space moves the single coordinate with the steepest normal component,
//! and greedy-by-cost reproduces it where a sweep in schema order can answer
//! several times farther. A driven coordinate gets no special treatment here —
//! it is one more coordinate with a slice, and if satisfying its equality by
//! moving *it* costs more than moving something it depends on, the cheaper
//! move wins. The settled-mask ordering in `retract` is for travelling along a
//! surface, not for landing on it once.
//!
//! A clamp lands *on* the bound, not near it, because a bound is the answer:
//! Artemis measured coordinates sitting exactly at a bound coming out several
//! times more accurate than ones a tolerance away. The slice is a superset
//! padded outward by a few ulps, so a landing nudges inward by a doubling
//! ladder of ulps until the coordinate's own constraints pass.
//!
//! **Shotgun.** Where clamping cannot land — two bands with a gap between, a
//! disc approached from a corner — the anchors decide. From each of the nearest
//! few anchors that are themselves feasible, bisect along the segment toward
//! the point: the anchor end is feasible, the far end is not, and the feasible
//! end of the final bracket is a point on the boundary between them. The
//! nearest such endpoint is the answer, and the anchor itself is a candidate
//! at `t = 0`, which is what makes "never farther than the nearest anchor" a
//! guarantee rather than a hope. One directed chord per anchor is the whole of
//! what a chain biased toward the point would find, at a thousandth of the
//! cost.
//!
//! **Release.** Both stages can move a coordinate that, in hindsight, did not
//! need to move — a clamp computed against a neighbour that then moved too, a
//! chord that carried every coordinate when one constraint was active. So the
//! last thing done to any answer is to put each moved coordinate back where it
//! was, one at a time, keeping every reversion that stays feasible.
//!
//! # The metric is L1 over box-normalised coordinates
//!
//! Normalised, because the caller thinks in a unit cube and "near" in metres
//! and pascals at once means nothing. L1 rather than L2, because the contrast
//! between nearest and farthest neighbour collapses in high dimension and
//! collapses fastest for the higher norms (Aggarwal, Hinneburg & Keim, 2001);
//! at fifty to two hundred dimensions over a census of thousands, the nearest
//! anchor under L2 is barely nearer than the farthest. And along a segment L1
//! is linear in the parameter, which is what makes the bisection's endpoint
//! provably no farther than its anchor.
//!
//! # What is deliberately not here
//!
//! No randomness — not a draw, not a seed. No gradient: babel has no
//! derivatives, and a finite-difference projection would spend `d + 1`
//! evaluations a step for a quality the shotgun already buys. No solver: an
//! SMT call is milliseconds to seconds where this budget is microseconds, and
//! transcendentals are outside its theories anyway. No brute force: it cannot
//! localise in high dimension, and it would re-solve the find-a-first-point
//! problem the whole module is built around on every call.

use faer::MatRef;

use super::{ConstraintSystem, Point};

/// How many rounds of clamping a point gets before the anchors take over.
///
/// A clamp is computed with the other coordinates held, so a coordinate that
/// moves can open room for another; a few rounds catch that. Most points land
/// in one, and a point that has not landed after this many is one the slices
/// do not describe — a gap the chord has to cross.
const CLAMP_SWEEPS: usize = 8;

/// How many of the nearest anchors get a chord.
///
/// More than one because the nearest anchor by L1 is not always the one whose
/// chord ends nearest — a chord stops at the first boundary it meets, and the
/// second-nearest anchor may see the point across a shorter stretch of
/// infeasible space. Eight is enough that the census's nearest cluster is
/// covered and few enough that a repair stays microseconds.
const ANCHOR_SHOTS: usize = 8;

/// Bisection steps along a chord. Each halves the bracket, so this is a budget
/// in bits and sixty is past where an `f64` parameter in `[0, 1]` can still be
/// halved.
const CHORD_BITS: usize = 60;

/// How far inward a landing may nudge, as a power of two in ulps.
///
/// A slice is padded outward by a few ulps per operator, so a clamp onto its
/// edge lands a hair outside. The nudge doubles from one ulp; twenty doublings
/// is a relative `1e-10`, which is well past any padding a real expression
/// accumulates and still nothing next to the tolerances constraints carry.
const LANDING_LADDER: u32 = 20;

/// A point that satisfies `system`, near `point`, the same every time.
///
/// `anchors` are feasible points the caller believes in, one per column in
/// schema order — the matrix [`FeasibleSamples::take`](super::FeasibleSamples::take)
/// hands out is the intended source. They are judged rather than trusted:
/// an infeasible column is skipped. With no anchors at all the answer is
/// whatever clamping alone reaches, which is enough wherever the constraints
/// name where their feasible side is, and `None` where they do not.
///
/// The result passes the same feasibility check the search uses — inside the
/// box, every constraint `<= 0`, nothing non-finite — and a point that already
/// passes comes back unchanged, so `repair(repair(x)) == repair(x)`. Given the
/// same system, anchors and point, the same output, bit for bit. And never
/// farther from `point`, in L1 over box-normalised coordinates, than the
/// nearest feasible anchor among the few considered.
///
/// # Panics
/// If `point` or `anchors` do not have one entry per variable. That is a
/// caller mixing up systems, not a verdict about the point.
#[must_use]
pub fn repair(system: &ConstraintSystem, anchors: MatRef<'_, f64>, point: &[f64]) -> Option<Point> {
    let dimensions = system.variables().len();
    assert_eq!(
        point.len(),
        dimensions,
        "a point has one coordinate per variable of the system it is repaired against"
    );
    assert_eq!(
        anchors.nrows(),
        dimensions,
        "anchors have one row per variable of the system they anchor"
    );

    let mut current: Point = point.to_vec();
    if system.is_feasible(&current) {
        return Some(current);
    }

    let widths: Vec<f64> = system
        .variables()
        .iter()
        .map(|variable| variable.upper_bound - variable.lower_bound)
        .collect();
    let distance = |a: &[f64], b: &[f64]| -> f64 {
        a.iter()
            .zip(b)
            .zip(&widths)
            .map(|((x, y), width)| {
                if *width > 0.0 {
                    (x - y).abs() / width
                } else {
                    0.0
                }
            })
            .sum()
    };

    for _ in 0..CLAMP_SWEEPS {
        // Every coordinate's clamp, with what it costs, from the point as it
        // stands at the start of the sweep. Cost is measured before any are
        // applied, which is what makes "cheapest first" a statement about the
        // point rather than about the order the coordinates happen to be in.
        let mut clamps: Vec<(f64, usize, f64)> = (0..dimensions)
            .filter_map(|coordinate| {
                let slice = system.slice(&current, coordinate);
                if slice.is_empty() {
                    return None;
                }
                let value = current[coordinate];
                let target = value.clamp(slice.lo(), slice.hi());
                if target == value {
                    return None;
                }
                let cost = if widths[coordinate] > 0.0 {
                    (target - value).abs() / widths[coordinate]
                } else {
                    0.0
                };
                Some((cost, coordinate, target))
            })
            .collect();
        if clamps.is_empty() {
            break;
        }
        clamps.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));

        for (_, coordinate, target) in clamps {
            let value = current[coordinate];
            current[coordinate] = target;

            // The slice's edge is a hair outside the true bound, so step inward
            // — toward where the value came from is *outward* — until the
            // constraints naming this coordinate pass. `is_feasible_after` is
            // exactly that check; it is used here as a filter over one
            // coordinate's constraints and never as the judge, which the full
            // check below remains.
            //
            // The ulp is the box width's, not the value's. A bound at zero has
            // ulps of `5e-324`, and a strict comparison is satisfied only past
            // an absolute `f64::MIN_POSITIVE`, which no ladder of those reaches;
            // the width is the scale the residual's own rounding lives at.
            if !system.is_feasible_after(&current, coordinate) {
                let inward = if target > value { 1.0 } else { -1.0 };
                let magnitude = target.abs().max(widths[coordinate]).max(f64::MIN_POSITIVE);
                let ulp = magnitude.next_up() - magnitude;
                for rung in 0..LANDING_LADDER {
                    let nudged = target + inward * ulp * f64::from(1u32 << rung);
                    current[coordinate] = nudged;
                    if system.is_feasible_after(&current, coordinate) {
                        break;
                    }
                    current[coordinate] = target;
                }
            }

            if system.is_feasible(&current) {
                return Some(released(system, current, point));
            }
        }
    }

    // Anchors nearest to where the point now stands, after whatever clamping
    // achieved; a clamp that did not land still moved the point onto the right
    // side of the constraints it could read, and the chord is shorter for it.
    let mut ranked: Vec<(f64, usize)> = (0..anchors.ncols())
        .map(|column| {
            let anchor: Point = (0..dimensions).map(|row| anchors[(row, column)]).collect();
            (distance(&anchor, &current), column)
        })
        .collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));

    let mut best: Option<(f64, Point)> = None;
    for (_, column) in ranked.into_iter().take(ANCHOR_SHOTS) {
        let anchor: Point = (0..dimensions).map(|row| anchors[(row, column)]).collect();
        if !system.is_feasible(&anchor) {
            continue;
        }

        // `t = 0` is the anchor and feasible; `t = 1` is `current` and is not,
        // or the clamp stage would have returned. Halve the bracket toward
        // wherever feasibility ends. A segment through a gap is bracketed just
        // the same — the feasible end is always a probe that passed, or the
        // anchor itself.
        let mut landed = anchor.clone();
        let (mut lower, mut upper) = (0.0_f64, 1.0_f64);
        for _ in 0..CHORD_BITS {
            let middle = 0.5 * (lower + upper);
            if middle <= lower || middle >= upper {
                break;
            }
            let mut probe: Point = anchor
                .iter()
                .zip(&current)
                .map(|(from, to)| from + middle * (to - from))
                .collect();
            system.settle(&mut probe);
            if system.is_feasible(&probe) {
                lower = middle;
                landed = probe;
            } else {
                upper = middle;
            }
        }

        let reached = distance(&landed, point);
        if best.as_ref().is_none_or(|(nearest, _)| reached < *nearest) {
            best = Some((reached, landed));
        }
    }

    best.map(|(_, landed)| released(system, landed, point))
}

/// `candidate` with every coordinate put back to its value in `original`
/// wherever that stays feasible, one coordinate at a time in schema order.
///
/// Feasible in, feasible out: a reversion is kept only if the whole point
/// still passes, so this can only shorten the distance to `original`, never
/// break the answer. It exists because both stages over-move — a clamp is
/// computed against neighbours that then move too, and a chord carries every
/// coordinate when one constraint was active — and hindsight is one cheap
/// check per coordinate.
fn released(system: &ConstraintSystem, mut candidate: Point, original: &[f64]) -> Point {
    for coordinate in 0..candidate.len() {
        let moved = candidate[coordinate];
        if moved == original[coordinate] {
            continue;
        }
        candidate[coordinate] = original[coordinate];
        if !system.is_feasible(&candidate) {
            candidate[coordinate] = moved;
        }
    }
    candidate
}
