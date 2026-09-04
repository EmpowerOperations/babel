//! Rejection sampling over the input box: the brute squad.
//!
//! Cheap and unbiased, and it works whenever the feasible region is a decent
//! fraction of the box — or, given enough proposals, a small one. It cannot
//! find a measure-zero region: an equality constraint like
//! `x1 == sqrt(x2) +/- 0.0001` is a ribbon that uniform sampling will
//! essentially never land on. That is what the solver is for.
//!
//! Candidates are proposed as a matrix, one column each, filled in bulk from
//! a Xoshiro256++ stream: nine integer operations per draw where the ChaCha
//! stream it replaced spent a block cipher round, and no allocation per
//! candidate where there used to be one `Vec` each. The pool judges the whole
//! matrix with the batched evaluator, which is what made the fill the
//! bottleneck in the first place.
//!
//! When the pool's probe lands nothing, [`RandomSampler::search_for_seed`]
//! keeps proposing on every core until a batch lands or a proposal budget is
//! spent. Batches are numbered and each is a pure function of its number, so
//! the point that comes out does not depend on how many threads looked for it.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use faer::Mat;
use rand::rngs::Xoshiro256PlusPlus;
use rand::{Rng, SeedableRng};

use super::{InputVariable, Point, PointSource, SearchContext};

/// How many candidates to propose per point asked for.
///
/// Proposals are cheap and the pool filters them, so over-proposing is how a
/// narrow-but-not-tiny region still yields points. Every candidate proposed is
/// now judged, where the one-at-a-time filter used to stop at `count` hits;
/// lowering this on a decided sampling route is a later knob, because it would
/// change which points come out and re-roll every seeded verdict.
const OVER_SAMPLING_FACTOR: usize = 100;

/// `2^-53`: the top 53 bits of a draw as a double in `[0, 1)`, which is what
/// rand's `StandardUniform` does too.
const UNIT: f64 = 1.0 / 9_007_199_254_740_992.0;

#[expect(
    clippy::cast_precision_loss,
    reason = "53 bits is exactly what an f64 mantissa holds"
)]
fn unit(bits: u64) -> f64 {
    (bits >> 11) as f64 * UNIT
}

/// Fills every column of `candidates` with one draw per row: row `r` takes
/// `low + u * (high - low)` for `(low, high) = bounds[r]`, `u` uniform in
/// `[0, 1)`. A row whose range is empty (`low >= high`) is filled with `low`.
///
/// **Half-open on purpose.** `random_range(low..=high)` reaches `high` with
/// probability `2^-53` per draw and pays a division for it; the difference is
/// measure zero, and the box check downstream is inclusive either way. A value
/// can land a last-place rounding past `high` only when `u` is within an ulp of
/// one, and that check rejects it — exactly as it rejected `random_range`'s own
/// overshoot.
///
/// Column-major: the outer loop is over columns because a column is contiguous
/// and a row is strided; the per-row `(low, scale)` table is computed once.
pub fn fill_box(candidates: &mut Mat<f64>, bounds: &[(f64, f64)], rng: &mut Xoshiro256PlusPlus) {
    assert_eq!(
        candidates.nrows(),
        bounds.len(),
        "one bound per row of the candidate matrix"
    );
    let table: Vec<(f64, f64)> = bounds
        .iter()
        .map(|&(low, high)| {
            if low >= high {
                (low, 0.0)
            } else {
                (low, high - low)
            }
        })
        .collect();

    for column in 0..candidates.ncols() {
        for (value, &(low, scale)) in candidates.col_as_slice_mut(column).iter_mut().zip(&table) {
            *value = low + unit(rng.next_u64()) * scale;
        }
    }
}

/// Bytes of candidate matrix per batch of the brute-force search. Sets the
/// batch width from the row count: under three thousand columns for three
/// variables, the floor for two hundred. Wide enough that the per-batch
/// bookkeeping is noise; narrow enough that two threads' batches share one
/// core's L2 when the thread count is the logical one.
///
/// Measured on the laptop (eight cores, sixteen threads, `x1 > 2` over three
/// variables): at 4 MiB sixteen threads managed 137M proposals a second and
/// were *slower* than eight; at 64 KiB they manage 450M. One thread is 75M
/// at any size, so this is a cache number and not an evaluator one.
const BATCH_BYTES: usize = 64 << 10;

/// Bounds on the batch width, whatever the row count says.
const MIN_BATCH_COLUMNS: usize = 256;
const MAX_BATCH_COLUMNS: usize = 16_384;

/// What a brute-force search came back with, for the log line and the tests.
#[derive(Debug)]
pub(crate) struct Landing {
    /// The hits of the lowest-indexed batch that had any. Empty when the
    /// budget was spent or the search was abandoned.
    pub points: Vec<Point>,
    /// Candidates judged. Varies with the thread count by the batches the
    /// other threads had in flight or had run ahead to when the winner was
    /// posted — a handful; the points do not vary.
    pub proposed: u64,
}

/// Uniform rejection sampling over the declared box, never narrowed.
///
/// Uniform over the feasible region by construction, which is what makes it
/// the probe that decides how a pool works, the thing that delivers where that
/// probe succeeds, and the reference the fairness oracles measure against.
/// Where the probe lands nothing it is also the brute squad: the same
/// proposals, wider, on every core, for a proposal budget, to land the seed
/// the walker needs.
///
/// An *adaptive* variant that narrowed the box toward points already found
/// existed and was removed on 2026-09-03: it was biased by construction, the
/// walker covers what it covered once a seed exists, and brute force is what
/// reaches a seed at all.
pub(crate) struct RandomSampler {
    inputs: Vec<InputVariable>,
    rng: Xoshiro256PlusPlus,
    /// The declared box, as `(low, high)` per variable, in `fill_box`'s shape.
    bounds: Vec<(f64, f64)>,
    /// Candidates the brute-force search may propose before giving up. Zero
    /// disables it.
    budget: u64,
    /// Threads the brute-force search fans out over. Never changes what is
    /// found, only how soon.
    threads: usize,
}

impl RandomSampler {
    pub(crate) fn new(
        inputs: Vec<InputVariable>,
        rng: Xoshiro256PlusPlus,
        budget: u64,
        threads: usize,
    ) -> Self {
        let bounds = inputs
            .iter()
            .map(|input| (input.lower_bound, input.upper_bound))
            .collect();
        Self {
            inputs,
            rng,
            bounds,
            budget,
            threads: threads.max(1),
        }
    }

    /// One brute-force batch of candidates from the sampler's own stream: the
    /// probe the pool decides its route on. Tens of microseconds for a few
    /// variables, and smaller than the delivery batches, which propose a
    /// hundred candidates per point asked for.
    pub(crate) fn probe(&mut self) -> Mat<f64> {
        let mut candidates = Mat::zeros(self.inputs.len(), self.batch_columns());
        fill_box(&mut candidates, &self.bounds, &mut self.rng);
        candidates
    }

    /// Columns per batch of the brute-force search.
    fn batch_columns(&self) -> usize {
        let rows = self.inputs.len().max(1);
        (BATCH_BYTES / (rows * size_of::<f64>())).clamp(MIN_BATCH_COLUMNS, MAX_BATCH_COLUMNS)
    }

    /// Wide-batch rejection sampling on every core until a batch lands or the
    /// budget runs out.
    ///
    /// **The batch is the unit of randomness.** Batch `k` draws from a stream
    /// seeded by `(base, k)` and nothing else, and the result is the lowest
    /// numbered batch with a hit, so the points that come out are a function
    /// of the seed and the budget alone. Threads change how fast, never what.
    /// Threads that had run ahead finish the batch they are on, which is the
    /// only way `proposed` depends on the thread count.
    ///
    /// `abandon` is asked between batches on the calling thread, which is how
    /// a caller that dropped the [`solve`](super::ConstraintSolver::solve)
    /// future gets its cores back within a batch.
    ///
    /// Draws one value from the sampler's own stream for `base`. The pool
    /// reaches here only after an empty probe, after which it never asks this
    /// sampler for a batch again, so the probe and the delivery streams are
    /// exactly what they were before this existed.
    pub(crate) fn search_for_seed(
        &mut self,
        context: &SearchContext<'_>,
        abandon: &mut dyn FnMut() -> bool,
    ) -> Landing {
        let base = self.rng.next_u64();
        let columns = self.batch_columns();
        let batches = self.budget.div_ceil(columns as u64);
        let rows = self.inputs.len();

        let winner = AtomicU64::new(u64::MAX);
        let best: Mutex<Option<(u64, Vec<Point>)>> = Mutex::new(None);
        let abandoned = AtomicBool::new(false);
        let proposed = AtomicU64::new(0);

        // One thread's share of the batches: `first, first + stride, ...`,
        // stopping at the budget, at a batch past a known winner, or on
        // abandonment. `poll` is `Some` only on the calling thread.
        let work = |first: u64, stride: u64, mut poll: Option<&mut dyn FnMut() -> bool>| {
            let mut candidates = Mat::zeros(rows, columns);
            let mut k = first;
            while k < batches && k <= winner.load(Ordering::Acquire) {
                if abandoned.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(poll) = poll.as_deref_mut()
                    && poll()
                {
                    abandoned.store(true, Ordering::Relaxed);
                    return;
                }

                let mut rng = Xoshiro256PlusPlus::seed_from_u64(base ^ k);
                fill_box(&mut candidates, &self.bounds, &mut rng);
                let hits = context.feasible_columns(candidates.as_ref());
                proposed.fetch_add(columns as u64, Ordering::Relaxed);

                if !hits.is_empty() {
                    winner.fetch_min(k, Ordering::AcqRel);
                    let mut best = best
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if best.as_ref().is_none_or(|(held, _)| k < *held) {
                        *best = Some((k, hits));
                    }
                }
                k += stride;
            }
        };

        let stride = self.threads as u64;
        std::thread::scope(|scope| {
            for first in 1..stride {
                let work = &work;
                scope.spawn(move || work(first, stride, None));
            }
            work(0, stride, Some(abandon));
        });

        let points = best
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .map(|(_, points)| points)
            .unwrap_or_default();
        Landing {
            points,
            proposed: proposed.into_inner(),
        }
    }
}

impl PointSource for RandomSampler {
    fn name(&self) -> &'static str {
        "brute-squad"
    }

    fn generate(
        &mut self,
        count: usize,
        _existing: &[Point],
        _context: &SearchContext<'_>,
    ) -> Mat<f64> {
        if count == 0 {
            return Mat::zeros(self.inputs.len(), 0);
        }

        let mut candidates = Mat::zeros(self.inputs.len(), count * OVER_SAMPLING_FACTOR);
        fill_box(&mut candidates, &self.bounds, &mut self.rng);
        candidates
    }
}

#[cfg(test)]
mod tests {
    use faer::Mat;
    use rand::SeedableRng;
    use rand::rngs::Xoshiro256PlusPlus;

    use super::{UNIT, fill_box, unit};

    #[test]
    fn unit_spans_zero_to_just_below_one() {
        assert_eq!(unit(0), 0.0);
        assert_eq!(unit(1 << 11), UNIT);
        assert_eq!(unit(u64::MAX), 1.0 - UNIT);
        assert!(unit(u64::MAX) < 1.0);
    }

    #[test]
    fn fill_box_stays_inside_the_box_and_reaches_both_ends() {
        let bounds = [(-3.0, 7.0), (0.0, 1.0)];
        let mut candidates = Mat::zeros(2, 200_000);
        fill_box(
            &mut candidates,
            &bounds,
            &mut Xoshiro256PlusPlus::seed_from_u64(1),
        );

        for (row, &(low, high)) in bounds.iter().enumerate() {
            let values = (0..candidates.ncols()).map(|c| candidates[(row, c)]);
            let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
            for v in values {
                assert!(
                    v >= low && v < high,
                    "row {row}: {v} outside [{low}, {high})"
                );
                min = min.min(v);
                max = max.max(v);
            }
            let span = high - low;
            assert!(min < low + 1e-4 * span, "row {row}: never came near {low}");
            assert!(
                max > high - 1e-4 * span,
                "row {row}: never came near {high}"
            );
        }
    }

    #[test]
    fn fill_box_is_a_pure_function_of_the_seed() {
        let bounds = [(0.0, 10.0), (-1.0, 1.0), (5.0, 6.0)];
        let mut a = Mat::zeros(3, 1_000);
        let mut b = Mat::zeros(3, 1_000);
        fill_box(&mut a, &bounds, &mut Xoshiro256PlusPlus::seed_from_u64(42));
        fill_box(&mut b, &bounds, &mut Xoshiro256PlusPlus::seed_from_u64(42));
        for c in 0..1_000 {
            for r in 0..3 {
                assert_eq!(a[(r, c)].to_bits(), b[(r, c)].to_bits());
            }
        }
    }

    #[test]
    fn an_empty_range_fills_with_its_lower_bound() {
        let mut candidates = Mat::zeros(2, 50);
        fill_box(
            &mut candidates,
            &[(5.0, 5.0), (6.0, 5.0)],
            &mut Xoshiro256PlusPlus::seed_from_u64(3),
        );
        for c in 0..50 {
            assert_eq!(candidates[(0, c)], 5.0);
            assert_eq!(candidates[(1, c)], 6.0);
        }
    }
}

#[cfg(test)]
mod brute_force_tests {
    //! The budgeted search: what it finds is a function of the seed and the
    //! budget, and of nothing about the machine.

    use std::time::{Duration, Instant};

    use rand::SeedableRng;
    use rand::rngs::Xoshiro256PlusPlus;

    use super::super::{InputVariable, SearchContext};
    use super::{Landing, RandomSampler};
    use crate::{Ast, Schema};

    const SEED: u64 = 0xB2_07_E5_90_AD;

    fn fixture(source: &str) -> (Vec<InputVariable>, Vec<Ast>, Schema) {
        let inputs = vec![InputVariable::new("x1", 0.0, 1.0)];
        let constraints = vec![crate::parse(source).expect("fixture should parse")];
        let schema = Schema::new(["x1"]);
        (inputs, constraints, schema)
    }

    fn search(
        source: &str,
        budget: u64,
        threads: usize,
        abandon: &mut dyn FnMut() -> bool,
    ) -> (Landing, usize) {
        let (inputs, constraints, schema) = fixture(source);
        let context = SearchContext::new(&inputs, &constraints, &schema);
        let mut sampler = RandomSampler::new(
            inputs.clone(),
            Xoshiro256PlusPlus::seed_from_u64(SEED),
            budget,
            threads,
        );
        let columns = sampler.batch_columns();
        (sampler.search_for_seed(&context, abandon), columns)
    }

    /// One in a million: the probe-sized first batch misses and the loop has
    /// to run tens of batches. Eight threads must land exactly the points one
    /// thread lands, because the winner is the lowest-numbered batch with a
    /// hit and a batch is a function of its number.
    #[test]
    fn the_same_seed_lands_the_same_points_on_one_thread_and_on_eight() {
        let (alone, columns) = search("x1 > 0.999999", 100_000_000, 1, &mut || false);
        let (crowd, _) = search("x1 > 0.999999", 100_000_000, 8, &mut || false);

        assert!(
            !alone.points.is_empty(),
            "one thread found nothing: {alone:?}"
        );
        for point in &alone.points {
            assert!(point[0] > 0.999_999, "{point:?}");
        }
        assert_eq!(alone.points, crowd.points);
        // Every batch below the winner is judged whoever runs it; the
        // overrun is whatever the other threads were doing at the time, which
        // depends on scheduling and is a handful of batches.
        assert!(crowd.proposed >= alone.proposed);
        assert!(
            crowd.proposed - alone.proposed <= 4 * 8 * columns as u64,
            "eight threads overran by {} batches",
            (crowd.proposed - alone.proposed) / columns as u64
        );
    }

    #[test]
    fn the_budget_bounds_the_proposals() {
        let (landing, columns) = search("x1 > 2", 100_000, 4, &mut || false);
        assert!(landing.points.is_empty());
        assert!(landing.proposed >= 100_000, "{}", landing.proposed);
        assert!(
            landing.proposed <= 100_000 + 4 * columns as u64,
            "{} proposals against a budget of 100,000",
            landing.proposed
        );
    }

    #[test]
    fn a_zero_budget_proposes_nothing() {
        let (landing, _) = search("x1 > 0.5", 0, 4, &mut || false);
        assert!(landing.points.is_empty());
        assert_eq!(landing.proposed, 0);
    }

    /// The caller changes its mind mid-search: the loop stops within a batch
    /// on every thread, not at the budget.
    #[test]
    fn an_abandoned_search_stops_within_a_round() {
        let started = Instant::now();
        let mut abandon = || started.elapsed() > Duration::from_millis(50);
        let (landing, columns) = search("x1 > 2", u64::MAX, 4, &mut abandon);
        let took = started.elapsed();

        assert!(landing.points.is_empty());
        assert!(
            took < Duration::from_secs(1),
            "abandoned search ran {took:?}"
        );
        // Far below anything the budget would allow: a few thousand batches
        // at most in the time it had.
        assert!(
            landing.proposed < 100_000 * columns as u64,
            "{} proposals after abandonment",
            landing.proposed
        );
    }
}
