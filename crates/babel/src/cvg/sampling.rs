//! Rejection sampling over the input box.
//!
//! Cheap and unbiased, and it works whenever the feasible region is a decent
//! fraction of the box. It cannot find a measure-zero region — an equality
//! constraint like `x1 == sqrt(x2) +/- 0.0001` is a ribbon that uniform
//! sampling will essentially never land on. That is what the solver is for.

use rand::RngExt;
use rand::rngs::StdRng;

use super::{InputVariable, Point, PointSource, SearchContext};

/// Whether the sampler narrows its proposal box toward what it has found.
///
/// This is the difference between a sampler that works and a sampler that is
/// *provably* unbiased, and you cannot have both. Rejection sampling is uniform
/// over the feasible region only while the proposal box contains that region —
/// [`Adaptation::Narrowing`] can pull the box in around points already seen and
/// cut off a part that has not been, so its output is fit for seeding and not
/// for measuring against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Adaptation {
    Narrowing,
    None,
}

/// How many candidates to propose per point asked for.
///
/// Proposals are cheap and the pool filters them, so over-proposing is how a
/// narrow-but-not-tiny region still yields points.
const OVER_SAMPLING_FACTOR: usize = 100;

/// How many recent points the adaptive bounds consider.
const RECENT_WINDOW: usize = 1_000;

/// How many points must be known before the bounds narrow at all.
///
/// One point tells you nothing about a region's extent. Narrowing onto it
/// collapses the sampler to a single value and it emits the same point forever
/// — which the benchmark harness catches as duplicates, but only after wasting
/// the batch.
const MINIMUM_TO_ADAPT: usize = 10;

/// The narrowest the sampling range may get, as a fraction of the declared
/// range. Keeps the sampler able to explore past what it has already seen.
const MINIMUM_SPAN_FRACTION: f64 = 0.01;

pub(crate) struct RandomSampler {
    inputs: Vec<InputVariable>,
    rng: StdRng,
    adaptation: Adaptation,
    /// Per-variable sampling range, narrowed toward wherever feasible points
    /// have actually been found. Starts as the declared box.
    bounds: Vec<(f64, f64)>,
}

impl RandomSampler {
    pub(crate) fn new(inputs: Vec<InputVariable>, rng: StdRng, adaptation: Adaptation) -> Self {
        let bounds = inputs
            .iter()
            .map(|input| (input.lower_bound, input.upper_bound))
            .collect();
        Self {
            inputs,
            rng,
            adaptation,
            bounds,
        }
    }

    /// Pulls the sampling range in around the recent accepted points.
    ///
    /// A region occupying 1% of the box yields nothing useful under uniform
    /// sampling; once a few points are known, sampling near them instead turns
    /// the same budget into hits. Bounds never escape the declared box.
    fn adapt(&mut self, existing: &[Point]) {
        if existing.len() < MINIMUM_TO_ADAPT {
            return;
        }
        let recent = &existing[existing.len().saturating_sub(RECENT_WINDOW)..];

        for (index, input) in self.inputs.iter().enumerate() {
            let Some(first) = recent.first().and_then(|point| point.get(index)) else {
                continue;
            };
            let (mut low, mut high) = (*first, *first);
            for point in recent {
                let Some(value) = point.get(index) else {
                    continue;
                };
                low = low.min(*value);
                high = high.max(*value);
            }

            // Widen by half the observed spread so the sampler can still reach
            // past what it has seen — otherwise it collapses onto its own first
            // few hits and never explores. Floored against the declared range so
            // that identical points cannot pin the span to zero.
            let declared = input.upper_bound - input.lower_bound;
            let margin = ((high - low) / 2.0).max(declared * MINIMUM_SPAN_FRACTION);
            self.bounds[index] = (
                (low - margin).max(input.lower_bound),
                (high + margin).min(input.upper_bound),
            );
        }
    }
}

impl PointSource for RandomSampler {
    fn name(&self) -> &'static str {
        match self.adaptation {
            Adaptation::Narrowing => "adaptive-sampling",
            Adaptation::None => "uniform-sampling",
        }
    }

    fn generate(
        &mut self,
        count: usize,
        existing: &[Point],
        _context: &SearchContext<'_>,
    ) -> Vec<Point> {
        if count == 0 {
            return Vec::new();
        }
        if self.adaptation == Adaptation::Narrowing {
            self.adapt(existing);
        }

        (0..count * OVER_SAMPLING_FACTOR)
            .map(|_| {
                self.bounds
                    .iter()
                    .map(|(low, high)| {
                        if low >= high {
                            *low
                        } else {
                            self.rng.random_range(*low..=*high)
                        }
                    })
                    .collect()
            })
            .collect()
    }
}
