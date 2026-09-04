//! Hit-and-run: walk the chord of the feasible region through the current point.
//!
//! From a feasible point, pick a direction uniformly on the unit sphere, take the
//! line through the point in that direction, and jump to a uniformly chosen
//! feasible place on it. Repeat. Needs no solver — it is arithmetic over
//! `evaluate` — but it needs a feasible point to start from.
//!
//! # Why this and not the JVM walker
//!
//! `RandomBoundedWalkingImproverPool` took a *half*-ray from a fixed base and
//! placed the result at `base + offset * nextDouble()` — uniform in radius.
//! Volume in `d` dimensions grows as `r^(d-1)`, so uniform-in-radius piles
//! points around the base and leaves the boundary empty. In 200 dimensions
//! essentially all the volume is near the boundary, so that is the whole region
//! missed.
//!
//! Hit-and-run (Smith, 1984) fixes it by construction rather than by correction:
//! take the chord both ways, sample uniformly along it, and make the result the
//! next state. The chain converges to the uniform distribution over the region.
//!
//! # Finding the chord: shrinkage, not bisection
//!
//! The obvious way to find where the region ends along the ray is to bisect
//! inward from the box wall. It is wrong, and instructively so. Binary search
//! finds the *first* boundary only when the predicate is monotone, and
//! feasibility along a line is not: for `(x+2)(x-1) == 0 +/- 1` the region is
//! two bands, and a ray from x = -2 toward the wall at x = 5 passes through the
//! gap and back into the far band, so the search brackets a chord spanning both.
//! Points sampled on it land in the gap, the move is refused, and the chain sits
//! still emitting the same point.
//!
//! Neal's shrinkage procedure (slice sampling, 2003) is the fix, and is cheaper
//! than what it replaces. Start with the whole box chord, sample on it, and if
//! the draw is infeasible shrink that side of the interval *to the draw* and try
//! again. It needs no monotonicity, converges geometrically onto the feasible
//! piece holding the current point, costs one feasibility test per attempt
//! rather than two brackets' worth, and leaves the uniform distribution
//! invariant.
//!
//! # Two kinds of move
//!
//! Pure hit-and-run mixes slowly in high dimensions, and measurably so: in the
//! 200-variable benchmark a move travels about 0.035 through a region 7.07
//! across, because the chord is cut short by the nearest of two hundred walls.
//! That is a random walk needing roughly `(7.07/0.035)^2` — about 40,000 —
//! moves per independent sample, which is the textbook `O(d^2)` and not
//! something a constant factor fixes.
//!
//! So half the moves step along a coordinate axis instead, sweeping the
//! coordinates in order. On a region whose bounds are axis-aligned an axis move
//! resamples that coordinate across its whole feasible range, so one sweep
//! produces an independent point and mixing becomes `O(d)`. Axis moves are
//! useless on a region angled across the coordinates — a diagonal ribbon leaves
//! nowhere to go — which is exactly where the random directions carry the
//! chain, so the two cover each other.
//!
//! Mixing them is sound: both kernels leave the uniform distribution invariant
//! (an axis move is Gibbs on that coordinate), and a random choice between
//! kernels sharing a stationary distribution preserves it.
//!
//! It also means a chain is not confined to its starting component: a first draw
//! landing in another piece is feasible, so it is taken. The proposal is
//! symmetric — the box chord is the same segment from either end — so this is a
//! legitimate move rather than a leak. Crossing still needs both pieces to share
//! a line, which is rare enough that a seed in each remains worth having.

use rand::RngExt;
use rand::rngs::Xoshiro256PlusPlus;

use super::Point;
use super::problem::Problem;

/// How many chains to run at once.
///
/// Emission is round-robin across them, which is the cheap way to decorrelate:
/// consecutive returned points come from different chains. It is also the only
/// way a region in several pieces gets covered, since one chain rarely crosses
/// between them — so this wants to stay comfortably above the number of pieces
/// any problem has.
///
/// Kept modest because every chain pays burn-in, and burn-in is the walker's
/// dominant cost. Decorrelation is mostly [`THINNING_PER_DIMENSION`]'s job.
const CHAIN_COUNT: usize = 8;

/// Steps to discard per dimension when a chain starts, so its output stops
/// depending on where it was seeded.
///
/// Scaled by dimension because mixing is: one move changes the point along a
/// single line, so a chain needs at least on the order of `d` moves before it
/// has explored `d` independent directions. A fixed burn-in that is ample in one
/// dimension leaves a 200-dimensional chain sitting next to its seed.
const BURN_IN_PER_DIMENSION: usize = 16;

/// Floor for the above.
///
/// High for a floor, because dimension is not the only thing that makes a chain
/// slow. P118's polytope is narrow and its constraints couple the variables in
/// pairs, so the scaled figure came to 240 steps at fifteen dimensions and left
/// the eight chains still clustered around wherever they had started — their
/// per-coordinate centroids disagreeing by four times the sampling error.
///
/// Raising it did *not*, on its own, make the benchmark's two-run comparison
/// agree; that turned out to be a flaw in the comparison rather than in the
/// walker. It is kept because 240 steps is too few on the evidence above,
/// not because it fixed a test.
const MINIMUM_BURN_IN: usize = 2_000;

/// Steps taken between emitted points, per dimension.
///
/// Scaled for the same reason as burn-in, and it is the more important of the
/// two. A move changes the point along one line, so after two moves a
/// 200-dimensional point has been perturbed in two of its two hundred degrees of
/// freedom and is very nearly the point it was. Emitting on that cadence
/// produces a sample that looks converged per-chain and is in fact a handful of
/// points wearing two hundred hats.
///
/// Two per dimension because only half the moves are axis moves, so that is what
/// it takes to complete one sweep of the coordinates.
const THINNING_PER_DIMENSION: usize = 2;

/// How often a move steps along a coordinate axis rather than a random
/// direction. Half and half: see the module documentation for why neither alone
/// is enough.
const AXIS_MOVE_PROBABILITY: f64 = 0.5;

/// Floor for the above, so one-dimensional problems still take a step or two
/// between emissions.
const MINIMUM_THINNING: usize = 2;

/// How many times a move may shrink its interval before giving up.
///
/// Each shrink cuts the interval roughly in half toward the current point, so
/// this is a budget in bits: 64 is past the point where an `f64` interval can
/// still be halved meaningfully. A move that exhausts it stays put.
const SHRINK_LIMIT: usize = 64;

pub(crate) struct HitAndRunWalker {
    rng: Xoshiro256PlusPlus,
    chains: Vec<Chain>,
}

/// A chain's current position, and how far through the coordinate sweep it is.
///
/// The cursor is per chain rather than global so that each chain sweeps every
/// coordinate; sharing one would let chains interleave and leave coordinates
/// untouched.
struct Chain {
    point: Point,
    steps: usize,
}

impl HitAndRunWalker {
    pub(crate) const fn new(rng: Xoshiro256PlusPlus) -> Self {
        Self {
            rng,
            chains: Vec::new(),
        }
    }

    /// Starts any chains that do not exist yet, from points chosen at random
    /// across everything found so far.
    ///
    /// Random rather than the most recent, because a region in several pieces is
    /// only covered if the chains start in several pieces — and the points found
    /// last are liable to be clustered in whichever piece the sampler hit most
    /// recently.
    fn start_chains(&mut self, existing: &[Point], problem: &Problem) {
        let burn_in = MINIMUM_BURN_IN.max(BURN_IN_PER_DIMENSION * existing[0].len());

        while self.chains.len() < CHAIN_COUNT {
            let index = self.rng.random_range(0..existing.len());
            let mut chain = Chain {
                point: existing[index].clone(),
                steps: 0,
            };
            for step in 0..burn_in {
                chain.point = advance(chain.point, step, &mut self.rng, problem);
            }
            chain.steps = burn_in;
            self.chains.push(chain);
        }
    }
}

impl HitAndRunWalker {
    /// Up to `count` more points, walked out from the chains — started, if
    /// they have not been, from points chosen at random across `from`.
    ///
    /// Every point returned is feasible by the chain's invariant; the pool
    /// judges them again anyway, because "never an infeasible one" is its
    /// promise and not this function's. Nothing to walk from is not an error:
    /// on a tight region it is the normal state until a seed exists.
    pub(crate) fn extend(&mut self, problem: &Problem, from: &[Point], count: usize) -> Vec<Point> {
        if count == 0 || from.is_empty() {
            return Vec::new();
        }
        self.start_chains(from, problem);
        let thinning = MINIMUM_THINNING.max(THINNING_PER_DIMENSION * from[0].len());

        // Sequential by nature — a chain cannot take its next step until it has
        // judged this one — so the points are walked one at a time.
        (0..count)
            .map(|emitted| {
                let index = emitted % self.chains.len();
                let mut point = std::mem::take(&mut self.chains[index].point);
                let mut steps = self.chains[index].steps;
                for _ in 0..thinning {
                    point = advance(point, steps, &mut self.rng, problem);
                    steps += 1;
                }
                self.chains[index].point = point.clone();
                self.chains[index].steps = steps;
                point
            })
            .collect()
    }
}

/// One hit-and-run move: sample the line through `from`, shrinking the interval
/// until the draw is feasible.
///
/// Returns `from` unchanged when there is nowhere to go — a degenerate box, or an
/// interval that collapsed before finding anything. A chain that stalls shows up
/// downstream as duplicate points rather than as a wrong answer, which is why the
/// benchmark harness checks for them.
fn advance(from: Point, step: usize, rng: &mut Xoshiro256PlusPlus, problem: &Problem) -> Point {
    let dimensions = from.len();
    let direction = if rng.random_range(0.0..1.0) < AXIS_MOVE_PROBABILITY {
        // Swept in order rather than picked at random: a random scan needs
        // `d ln d` moves to touch every coordinate, a sweep needs `d`.
        let mut axis = vec![0.0; dimensions];
        axis[step % dimensions] = 1.0;
        axis
    } else {
        random_direction(rng, dimensions)
    };
    let (mut lower, mut upper) = box_chord(&from, &direction, problem);

    for _ in 0..SHRINK_LIMIT {
        if lower >= upper {
            break;
        }
        let step = rng.random_range(lower..=upper);
        let candidate: Point = from
            .iter()
            .zip(&direction)
            .map(|(value, component)| value + step * component)
            .collect();

        if problem.is_feasible(&candidate) {
            return candidate;
        }

        // Shrink toward `from`, which is feasible by the chain's invariant, so
        // the interval always still contains a feasible point. `step` cannot be
        // zero here: zero is `from` itself and would have been accepted.
        if step < 0.0 {
            lower = step;
        } else {
            upper = step;
        }
    }
    from
}

/// A point uniformly distributed on the unit sphere.
///
/// Gaussian components divided by the L2 norm — Muller's method. The JVM version
/// used uniform components over a cube, which biases directions toward the
/// cube's corners; it also divided by `abs(sum(components))` rather than the
/// norm, though that error cancelled, since the caller rescaled the direction to
/// the box wall and that rescaling is scale-invariant.
fn random_direction(rng: &mut Xoshiro256PlusPlus, dimensions: usize) -> Vec<f64> {
    loop {
        let components: Vec<f64> = (0..dimensions)
            .map(|_| {
                // Box-Muller. `random_range` is half-open at the top, so
                // `1.0 - u` keeps the logarithm away from zero.
                let u1: f64 = 1.0 - rng.random_range(0.0..1.0);
                let u2: f64 = rng.random_range(0.0..1.0);
                (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
            })
            .collect();

        let norm = components.iter().map(|c| c * c).sum::<f64>().sqrt();
        if norm > 0.0 && norm.is_finite() {
            return components.iter().map(|c| c / norm).collect();
        }
        // All components underflowed to zero. Vanishingly rare, and cheaper to
        // redraw than to reason about.
    }
}

/// How far `from` can travel either side along `direction` and stay in the box.
///
/// The interval brackets zero. This is a pure box calculation — feasibility does
/// not enter into it, because shrinkage is what handles the constraints.
fn box_chord(from: &Point, direction: &[f64], problem: &Problem) -> (f64, f64) {
    let (mut lower, mut upper) = (f64::NEG_INFINITY, f64::INFINITY);
    for (index, input) in problem.inputs().iter().enumerate() {
        let component = direction[index];

        // A zero component means the ray is parallel to this pair of walls and
        // never meets them. Skipping is not just an optimisation: the JVM version
        // divided unguarded, and a point sitting exactly on a bound gives
        // 0.0/0.0, which Java's `Math.min` propagates as NaN and Rust's
        // `f64::min` silently discards. Two different wrong answers.
        if component == 0.0 {
            continue;
        }

        let to_lower = (input.lower_bound - from[index]) / component;
        let to_upper = (input.upper_bound - from[index]) / component;
        let (near, far) = if to_lower < to_upper {
            (to_lower, to_upper)
        } else {
            (to_upper, to_lower)
        };
        lower = lower.max(near);
        upper = upper.min(far);
    }

    // `from` is inside the box, so the interval contains zero — but a point
    // resting exactly on a bound can put it a rounding error the wrong side.
    (lower.min(0.0), upper.max(0.0))
}
