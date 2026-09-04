//! What the search has in hand, and what it cost to get.
//!
//! The pool's state is a *value*, not a field. Every step of the worker takes
//! a [`Progress`], adds what it found, and hands it back; nothing changes
//! behind a caller's back and there is nothing to lock. What the pool does
//! next — which route delivers, whether to escalate — is read off the value.

use super::Point;

/// One round of uniform proposals: what landed and what it cost.
///
/// Returned by every rung that proposes candidates at random — the probe, the
/// delivery batches, brute force — and absorbed into a [`Progress`]. The
/// walker does not produce one: its output is not a trial of the region.
#[derive(Debug, Default)]
pub(crate) struct Trial {
    /// The feasible points kept from this round.
    pub(crate) points: Vec<Point>,
    /// Candidates judged to get them.
    pub(crate) proposed: u64,
}

/// How the pool delivers, once the probe has said which.
///
/// Plain rejection sampling is not a fallback — where it works it is the *best*
/// option available, being unbiased by construction, needing no burn-in, and
/// having none of a Markov chain's trouble with regions in several pieces. So it
/// gets asked first, and the fancier machinery only runs where it has to.
///
/// The measured case for this: `parabolic_roots_narrowing` under the walker
/// returned 2000 points worth 87 independent ones, because a chain cannot cross
/// the gap between the two bands and so the split between them was decided by
/// which band each chain happened to start in. Plain sampling reaches that
/// region perfectly well and has no such problem.
///
/// Pinned by the first trial and never revisited. The problem is static, so
/// a later batch disagreeing with the first is noise, not news; if a case ever
/// shows otherwise, that is the test to write before making this dynamic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    /// Plain sampling reaches the region often enough. Nothing else runs.
    Sampling,
    /// It does not. Seed by whatever lands — the probe, the solver, brute
    /// force — and deliver with the walker.
    Walking,
}

/// Everything the search has in hand.
///
/// A value: every method takes `self` and gives it back, so the worker's loop
/// reads `progress = progress.absorb(trial)` and there is no other way to
/// change it.
#[derive(Debug, Default)]
pub(crate) struct Progress {
    /// Every feasible point in hand, from any source. What the walker starts
    /// from, and what the first batch is drawn from.
    points: Vec<Point>,
    /// Uniform candidates judged so far, and how many points were kept from
    /// them. The walker's output and the solver's witness count as points,
    /// never as trials, so this stays a statement about the region.
    proposed: u64,
    landed: usize,
    /// See [`Route`]. `None` only before the opening has probed.
    route: Option<Route>,
}

impl Progress {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    /// Takes in a round of uniform proposals: its points, and its cost.
    #[must_use]
    pub(crate) fn absorb(mut self, trial: Trial) -> Self {
        self.proposed += trial.proposed;
        self.landed += trial.points.len();
        self.points.extend(trial.points);
        self
    }

    /// Takes in points that were not proposed at random — walked to, or
    /// handed over by a solver — so they count as points and not as trials.
    #[must_use]
    pub(crate) fn extend(mut self, points: Vec<Point>) -> Self {
        self.points.extend(points);
        self
    }

    /// Settles the route. The first call wins; later calls change nothing.
    #[must_use]
    pub(crate) fn pin(mut self, route: Route) -> Self {
        self.route.get_or_insert(route);
        self
    }

    /// # Panics
    /// Before the opening has pinned a route, which it does before anything is
    /// delivered.
    pub(crate) fn route(&self) -> Route {
        self.route
            .expect("the opening pins the route before anything is delivered")
    }

    pub(crate) fn points(&self) -> &[Point] {
        &self.points
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub(crate) const fn proposed(&self) -> u64 {
        self.proposed
    }

    pub(crate) const fn landed(&self) -> usize {
        self.landed
    }
}

#[cfg(test)]
mod tests {
    use super::{Progress, Route, Trial};

    fn trial(points: usize, proposed: u64) -> Trial {
        Trial {
            points: (0..points)
                .map(|i| vec![f64::from(u8::try_from(i).unwrap())])
                .collect(),
            proposed,
        }
    }

    #[test]
    fn absorbing_counts_trials_and_extending_does_not() {
        let progress = Progress::empty()
            .absorb(trial(2, 1_000))
            .extend(vec![vec![9.0], vec![8.0], vec![7.0]])
            .absorb(trial(1, 500));

        assert_eq!(progress.points().len(), 6);
        assert_eq!(progress.proposed(), 1_500);
        assert_eq!(progress.landed(), 3, "walked points are not trials");
    }

    #[test]
    fn an_empty_trial_still_cost_its_proposals() {
        let progress = Progress::empty().absorb(trial(0, 2_730));
        assert!(progress.is_empty());
        assert_eq!(progress.proposed(), 2_730);
        assert_eq!(progress.landed(), 0);
    }

    #[test]
    fn the_first_pin_wins() {
        let progress = Progress::empty().pin(Route::Walking).pin(Route::Sampling);
        assert_eq!(progress.route(), Route::Walking);
    }

    #[test]
    #[should_panic(expected = "pins the route")]
    fn an_unpinned_route_is_a_bug_not_a_default() {
        let _ = Progress::empty().route();
    }
}
