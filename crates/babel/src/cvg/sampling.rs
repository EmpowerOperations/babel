//! Rejection sampling over the input box.
//!
//! Cheap and unbiased, and it works whenever the feasible region is a decent
//! fraction of the box. It cannot find a measure-zero region — an equality
//! constraint like `x1 == sqrt(x2) +/- 0.0001` is a ribbon that uniform
//! sampling will essentially never land on. That is what the solver is for.
//!
//! Candidates are proposed as a matrix, one column each, filled in bulk from
//! a Xoshiro256++ stream: nine integer operations per draw where the ChaCha
//! stream it replaced spent a block cipher round, and no allocation per
//! candidate where there used to be one `Vec` each. The pool judges the whole
//! matrix with the batched evaluator, which is what made the fill the
//! bottleneck in the first place.

use faer::Mat;
use rand::Rng;
use rand::rngs::Xoshiro256PlusPlus;

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

/// Uniform rejection sampling over the declared box, never narrowed.
///
/// Uniform over the feasible region by construction, which is what makes it
/// both the probe that decides how a pool works and the reference the fairness
/// oracles measure against. An *adaptive* variant that narrowed the box toward
/// points already found existed and was removed on 2026-09-03: it was biased by
/// construction, the walker covers what it covered once a seed exists, and the
/// brute squad is the planned replacement for reaching a seed at all.
pub(crate) struct RandomSampler {
    inputs: Vec<InputVariable>,
    rng: Xoshiro256PlusPlus,
    /// The declared box, as `(low, high)` per variable, in `fill_box`'s shape.
    bounds: Vec<(f64, f64)>,
}

impl RandomSampler {
    pub(crate) fn new(inputs: Vec<InputVariable>, rng: Xoshiro256PlusPlus) -> Self {
        let bounds = inputs
            .iter()
            .map(|input| (input.lower_bound, input.upper_bound))
            .collect();
        Self {
            inputs,
            rng,
            bounds,
        }
    }
}

impl PointSource for RandomSampler {
    fn name(&self) -> &'static str {
        "uniform-sampling"
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
