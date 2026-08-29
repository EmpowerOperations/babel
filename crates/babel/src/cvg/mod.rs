//! Constrained random vector generation.
//!
//! Given a box of input variables and a set of babel constraints, produce
//! points that satisfy all of them and that cover the feasible region
//! reasonably evenly.
//!
//! Lives inside babel rather than alongside it so that [`crate::ast`] can stay
//! private — the SMT-LIB2 emitter is an internal function over the AST, not a
//! published consumer of it.
//!
//! # Strategy
//!
//! Finding the *first* feasible point is the hard part, and for a tight region
//! it needs a solver. Once there, cheap strategies cover the space quickly. That
//! is why [`solve`] is the expensive, awaitable call and
//! [`ConstraintPool::generate`] is not.
//!
//! The two live strategies divide along that line, and their output is treated
//! differently because their guarantees differ:
//!
//! * **Adaptive rejection sampling** *seeds*. It narrows its proposal box toward
//!   whatever it has found, which is what makes a narrow region reachable at all
//!   — and which also means it can exclude a part of the region it has not seen
//!   yet. Biased, effective, and its points never leave the pool.
//! * **Hit-and-run** *emits*. It converges to the uniform distribution over the
//!   region, so what a caller receives is governed by the strategy with a
//!   guarantee rather than the one with a heuristic. It cannot start without a
//!   feasible point, which is what the seeder is for.
//!
//! Neither can reach a region of measure zero — an equality constraint with a
//! tolerance tight enough is a ribbon that sampling will not land on and a walk
//! cannot be started in. That is the solver's job, and it is not written yet.

mod sampling;
mod smt;
mod walking;

use anyhow::Result;
use rand::SeedableRng;
use rand::rngs::StdRng;

use crate::{Bound, Expression, Schema};
use sampling::{Adaptation, RandomSampler};
use walking::HitAndRunWalker;

/// A point in the input space, ordered by [`Schema`] position.
///
/// Positional rather than a name-to-value map: the JVM implementation allocated
/// a hash map per candidate inside a loop that oversamples a hundred to one, and
/// the schema already carries the names. It is also the shape a column-major
/// matrix wants, for when evaluation goes batched.
pub type Point = Vec<f64>;

/// One input variable and the range it may take.
#[derive(Debug, Clone, PartialEq)]
pub struct InputVariable {
    pub name: String,
    pub lower_bound: f64,
    pub upper_bound: f64,
}

impl InputVariable {
    #[must_use]
    pub fn new(name: impl Into<String>, lower_bound: f64, upper_bound: f64) -> Self {
        Self {
            name: name.into(),
            lower_bound,
            upper_bound,
        }
    }

    #[must_use]
    pub fn contains(&self, value: f64) -> bool {
        (self.lower_bound..=self.upper_bound).contains(&value)
    }
}

/// What [`solve`] concluded.
///
/// Three-way rather than a `Result` because "no such point exists" is an
/// *answer*, and because a solver that could not reason about one constraint
/// still leaves a usable pool — collapsing that into success would let a caller
/// silently treat a partial understanding as a total one.
#[derive(Debug)]
pub enum Solution {
    /// Points were found. Past tense deliberately: rejection sampling cannot
    /// *prove* a region is non-empty, only report having landed in it.
    /// [`Solution::Unsatisfiable`] stays modal by contrast — only a solver
    /// earns the right to claim no point exists.
    Satisfied(ConstraintPool),
    /// A usable pool, but some constraint could not be reasoned about. Points
    /// are filtered rather than proven, and coverage may be poor.
    Unknown {
        pool: ConstraintPool,
        unsolved: Vec<Expression>,
    },
    /// No point satisfies the constraints.
    Unsatisfiable { blamed: Option<Expression> },
}

/// Which strategies a pool may use.
///
/// Hidden, and hidden deliberately: which strategy runs is the module's
/// decision, and [`Route`] makes most of it at runtime from a probe rather than
/// from configuration. This exists so that tests can pin one strategy and
/// measure it alone, because a pool that mixes them cannot say which one
/// produced a bad distribution.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Rejection sampling that narrows its proposal box toward what it has
    /// found. Effective on narrow regions, and *not* uniform over the feasible
    /// region — the narrowed box can exclude a part not yet seen.
    AdaptiveSampling,
    /// Rejection sampling over the declared box, never narrowed. Uniform over
    /// the feasible region by construction, and useless once that region is a
    /// small enough fraction of the box. Both the probe that decides the route
    /// and the fairness oracle the tests measure against.
    UniformSampling,
    /// Hit-and-run: walk the chord of the region through the current point.
    /// Converges to the uniform distribution, but needs a feasible point to
    /// start from and crosses between disconnected pieces only by luck.
    HitAndRun,
}

/// What production uses: try plain sampling, and reach for the rest only if it
/// does not work. See [`Route`].
///
/// Public so that tests measuring "what a caller gets" cannot drift from it. A
/// copy of this list living in the test suite is a copy that goes stale, and did.
#[doc(hidden)]
pub const DEFAULT_STRATEGIES: &[Strategy] = &[
    Strategy::UniformSampling,
    Strategy::AdaptiveSampling,
    Strategy::HitAndRun,
];

/// The share of a probe that plain sampling must land to be trusted with the
/// job. Ported from the JVM's `EASY_PATH_THRESHOLD_FACTOR`.
const EASY_PATH_THRESHOLD: f64 = 0.1;

/// The route is decided on at least this many points, however few the first
/// call asks for. A one-point probe decides nothing.
const MINIMUM_PROBE: usize = 100;

/// How the pool has decided to answer.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    /// Not settled yet. The next call probes.
    Undecided,
    /// Plain sampling reaches the region often enough. Nothing else runs.
    Sampling,
    /// It does not. Seed with adaptation, deliver with the walker.
    Walking,
}

/// Everything a solve needs beyond the problem itself.
///
/// The randomness, any points the caller already believes in, and which
/// strategies to use — all dependencies, all with defaults. They live here
/// rather than as parameters because there used to be three entry points
/// (`solve`, `solve_with_rng`, `solve_with`) that differed only in how many of
/// these they let you reach, and two of the three existed purely so the tests
/// could get past the first.
///
/// Construction cannot fail: nothing held here can be invalid on its own. What
/// *can* be invalid — a constraint naming a variable the box does not declare —
/// needs the problem, and so is checked in [`ConstraintSolver::solve`].
///
/// ```no_run
/// # use babel::cvg::{ConstraintSolver, InputVariable, Solution};
/// # async fn example() -> anyhow::Result<()> {
/// let inputs = vec![InputVariable::new("x", -1.0, 1.0)];
/// let constraints = vec![babel::compile("x > 0")?];
///
/// if let Solution::Satisfied(mut pool) = ConstraintSolver::new()
///     .solve(inputs, constraints)
///     .await?
/// {
///     let points = pool.generate(1_000);
/// }
/// # Ok(())
/// # }
/// ```
/// Not `Clone`: `StdRng` is not, and rightly so — two solvers sharing a stream
/// would silently produce the same "random" points.
#[derive(Debug)]
pub struct ConstraintSolver {
    rng: StdRng,
    known_feasible: Vec<Point>,
    strategies: Vec<Strategy>,
}

impl Default for ConstraintSolver {
    fn default() -> Self {
        Self {
            rng: StdRng::from_rng(&mut rand::rng()),
            known_feasible: Vec::new(),
            strategies: DEFAULT_STRATEGIES.to_vec(),
        }
    }
}

impl ConstraintSolver {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Points the caller already believes are feasible.
    ///
    /// A hint, not an assertion: infeasible ones are discarded rather than
    /// trusted. Worth supplying — on a region too tight to sample, a seed is the
    /// difference between the walker working and having nothing to start from.
    #[must_use]
    pub fn with_known_feasible(mut self, points: Vec<Point>) -> Self {
        self.known_feasible = points;
        self
    }

    /// Pins the randomness, so a run is reproducible.
    #[doc(hidden)]
    #[must_use]
    pub fn with_rng(mut self, rng: StdRng) -> Self {
        self.rng = rng;
        self
    }

    /// Pins the strategy list.
    ///
    /// Hidden along with [`Strategy`] itself: which strategies run is the
    /// module's decision, not the caller's, and [`Route`] makes most of it at
    /// runtime anyway. Tests use this to measure one strategy at a time, because
    /// a pool that mixes them cannot say which produced a bad distribution.
    #[doc(hidden)]
    #[must_use]
    pub fn with_strategies(mut self, strategies: Vec<Strategy>) -> Self {
        self.strategies = strategies;
        self
    }

    /// Finds a feasible region and hands back something that can sample it.
    ///
    /// # Why this is `async`
    ///
    /// There is no bound on how long it takes. A solver can hit exponential
    /// blowup and effectively not finish, so a plain `fn` returning in 45ms or
    /// 45 minutes would be lying about its cost. A future says so in the type.
    ///
    /// No runtime is imposed. [`Future`](std::future::Future) is in `core`; drive
    /// this with tokio, smol, or a bare `block_on` — this crate's own tests use
    /// the last of those, which is the proof that nothing heavier is required.
    ///
    /// **The signal is currently ahead of the implementation.** The body runs
    /// inline and never yields, so a `timeout` wrapped around this call will not
    /// fire, and the expensive half is usually
    /// [`ConstraintPool::generate`] — which is synchronous — rather than this.
    /// Both are fixable and neither changes this signature: the body moves onto
    /// a thread behind a oneshot, and cancellation becomes a token the search
    /// checks. Recorded in `todos.md`; do not read the `async` here as a promise
    /// that a timeout works today.
    ///
    /// # Errors
    /// Anything that went wrong, as opposed to anything that was concluded. An
    /// unsatisfiable problem is a [`Solution`], not an error.
    pub async fn solve(
        self,
        inputs: Vec<InputVariable>,
        constraints: Vec<Expression>,
    ) -> Result<Solution> {
        let mut pool = ConstraintPool::new(inputs, constraints, self.rng, &self.strategies)?;
        pool.seed(self.known_feasible);

        if !pool.found.is_empty() {
            return Ok(Solution::Satisfied(pool));
        }

        // Seeding is what needs a solver when the region is tight. Rejection
        // sampling finding nothing does not prove there is nothing to find, so
        // we cannot answer `Unsatisfiable` here — only a solver can.
        let seeds = pool.generate(1);
        if seeds.is_empty() {
            smt::escalate_for_seed(&pool);
        }

        Ok(Solution::Satisfied(pool))
    }
}

/// A feasible region, and the strategies that sample it.
///
/// Owns its accumulated points rather than making the caller pass them back in
/// on every call, and filters its own output. The JVM interface did neither —
/// it took `existingPoints` as a parameter and warned in a comment that
/// *"the results may not actually be feasible! you must filter this list on the
/// callers side!"*.
pub struct ConstraintPool {
    schema: Schema,
    inputs: Vec<InputVariable>,
    constraints: Vec<Expression>,
    found: Vec<Point>,
    /// Unbiased rejection sampling over the declared box, if configured. Both
    /// the probe that decides the [`Route`] and, where that probe succeeds, the
    /// thing that delivers.
    fair: Option<Box<dyn PointSource>>,
    route: Route,
    /// Run first, and their output is *not* returned — it only populates
    /// `found`. Adaptive sampling lives here: it is the strategy that can reach
    /// a narrow region, and the one whose output is biased.
    seeders: Vec<Box<dyn PointSource>>,
    /// Produce what `generate` returns. Empty only when no strategy needs a
    /// seed, in which case the seeders are promoted and deliver directly.
    emitters: Vec<Box<dyn PointSource>>,
}

impl std::fmt::Debug for ConstraintPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConstraintPool")
            .field("inputs", &self.inputs.len())
            .field("constraints", &self.constraints.len())
            .field("found", &self.found.len())
            .field("route", &self.route)
            .field("fair", &self.fair.as_ref().map(|s| s.name()))
            .field(
                "seeders",
                &self.seeders.iter().map(|s| s.name()).collect::<Vec<_>>(),
            )
            .field(
                "emitters",
                &self.emitters.iter().map(|s| s.name()).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl ConstraintPool {
    fn new(
        inputs: Vec<InputVariable>,
        constraints: Vec<Expression>,
        mut rng: StdRng,
        strategies: &[Strategy],
    ) -> Result<Self> {
        let schema = Schema::new(inputs.iter().map(|input| input.name.clone()));

        // Fail here rather than per-evaluation: a constraint naming a variable
        // the box does not declare can never be satisfied, and saying so once is
        // kinder than saying it a thousand times.
        for constraint in &constraints {
            constraint.bind(&schema).map_err(|missing| {
                anyhow::anyhow!(
                    "constraint {:?} references {} which is not an input variable",
                    constraint.source(),
                    missing.missing.join(", ")
                )
            })?;
        }

        // Each strategy gets its own generator, derived from the one passed in,
        // so that adding a strategy does not change what the others produce.
        let mut fair: Option<Box<dyn PointSource>> = None;
        let mut seeders: Vec<Box<dyn PointSource>> = Vec::new();
        let mut emitters: Vec<Box<dyn PointSource>> = Vec::new();
        for strategy in strategies {
            let stream = StdRng::from_rng(&mut rng);
            match strategy {
                Strategy::UniformSampling => {
                    fair = Some(Box::new(RandomSampler::new(
                        inputs.clone(),
                        stream,
                        Adaptation::None,
                    )));
                }
                Strategy::AdaptiveSampling => seeders.push(Box::new(RandomSampler::new(
                    inputs.clone(),
                    stream,
                    Adaptation::Narrowing,
                ))),
                Strategy::HitAndRun => emitters.push(Box::new(HitAndRunWalker::new(stream))),
            }
        }

        // Nothing needs a seed, so there is nothing to hold back: the seeders
        // are the whole pool and deliver their own output.
        if emitters.is_empty() {
            std::mem::swap(&mut seeders, &mut emitters);
        }

        let route = match (&fair, emitters.is_empty()) {
            // Plain sampling is the only thing configured, so there is no
            // decision to make and nothing to fall back to.
            (Some(_), true) => Route::Sampling,
            (Some(_), false) => Route::Undecided,
            (None, _) => Route::Walking,
        };

        Ok(Self {
            schema,
            inputs,
            constraints,
            found: Vec::new(),
            fair,
            route,
            seeders,
            emitters,
        })
    }

    /// At most `count` feasible points.
    ///
    /// Fewer than asked for is normal — a strategy may simply not find that many
    /// in one pass. Never more, and never an infeasible one.
    pub fn generate(&mut self, count: usize) -> Vec<Point> {
        if count == 0 {
            return Vec::new();
        }
        // Built once per call rather than once per candidate: binding rebuilds a
        // `Schema` every time, which is fine for a test and wasteful at a
        // thousand candidates a batch.
        let context = SearchContext::new(&self.inputs, &self.constraints, &self.schema);

        // One round of plain sampling settles which way this pool works. Cheap,
        // and where it succeeds there is no reason to do anything cleverer.
        if let (Route::Undecided, Some(fair)) = (self.route, self.fair.as_mut()) {
            let probe = count.max(MINIMUM_PROBE);
            let landed: Vec<Point> = fair
                .generate(probe, &self.found, &context)
                .into_iter()
                .filter(|point| context.is_feasible(point))
                .collect();

            #[expect(
                clippy::cast_precision_loss,
                reason = "sample counts are far below the f64 integer limit"
            )]
            let enough = landed.len() as f64 >= EASY_PATH_THRESHOLD * probe as f64;
            self.route = if enough {
                Route::Sampling
            } else {
                Route::Walking
            };
            self.found.extend(landed.iter().cloned());

            // The probe already produced deliverable points, and they are as
            // good as any that would follow.
            if enough {
                let mut delivered = landed;
                delivered.truncate(count);
                return delivered;
            }
        }

        if let (Route::Sampling, Some(fair)) = (self.route, self.fair.as_mut()) {
            let delivered: Vec<Point> = fair
                .generate(count, &self.found, &context)
                .into_iter()
                .filter(|point| context.is_feasible(point))
                .take(count)
                .collect();
            self.found.extend(delivered.iter().cloned());
            return delivered;
        }

        // Seeders feed the pool without delivering. Their job is to reach the
        // region at all; the emitter's job is to sample it evenly, and mixing
        // the two would put biased points in the caller's hands.
        for seeder in &mut self.seeders {
            let proposed = seeder.generate(count, &self.found, &context);
            let feasible: Vec<Point> = proposed
                .into_iter()
                .filter(|point| context.is_feasible(point))
                .collect();
            self.found.extend(feasible);
        }

        let mut accepted: Vec<Point> = Vec::new();
        for emitter in &mut self.emitters {
            if accepted.len() >= count {
                break;
            }
            let wanted = count - accepted.len();
            let proposed = emitter.generate(wanted, &self.found, &context);
            accepted.extend(
                proposed
                    .into_iter()
                    .filter(|point| context.is_feasible(point))
                    .take(wanted),
            );
        }

        self.found.extend(accepted.iter().cloned());
        accepted
    }

    /// Adopts points a caller believes are feasible, discarding any that are
    /// not. Returns how many were kept.
    pub fn seed(&mut self, points: Vec<Point>) -> usize {
        let context = SearchContext::new(&self.inputs, &self.constraints, &self.schema);
        let before = self.found.len();
        self.found.extend(
            points
                .into_iter()
                .filter(|point| context.is_feasible(point)),
        );
        self.found.len() - before
    }

    /// The order a [`Point`]'s values are in.
    #[must_use]
    pub const fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Every feasible point found so far, across all calls.
    #[must_use]
    pub fn found(&self) -> &[Point] {
        &self.found
    }
}

/// A strategy for proposing candidate points.
///
/// Proposals are filtered by [`ConstraintPool::generate`], so an implementation
/// may return infeasible candidates — it is not required to check.
pub(crate) trait PointSource {
    fn name(&self) -> &'static str;

    /// Propose up to `count` candidates, optionally informed by what has already
    /// been found. `existing` is empty on the first call, which is what makes
    /// seeding the hard case.
    fn generate(
        &mut self,
        count: usize,
        existing: &[Point],
        context: &SearchContext<'_>,
    ) -> Vec<Point>;
}

/// The box and the constraints, in the form a strategy needs them.
///
/// Passed to every [`PointSource`] because a strategy that searches — as opposed
/// to one that guesses — cannot work without asking whether a point is feasible.
/// Bisecting out to the edge of a region is nothing but that question, repeated.
pub(crate) struct SearchContext<'a> {
    inputs: &'a [InputVariable],
    bounds: Vec<Bound<'a>>,
}

impl<'a> SearchContext<'a> {
    fn new(inputs: &'a [InputVariable], constraints: &'a [Expression], schema: &'a Schema) -> Self {
        let bounds = constraints
            .iter()
            .map(|constraint| {
                constraint
                    .bind(schema)
                    .expect("`ConstraintPool::new` already proved every constraint binds")
            })
            .collect();
        Self { inputs, bounds }
    }

    pub(crate) const fn inputs(&self) -> &'a [InputVariable] {
        self.inputs
    }

    /// Whether a point is inside the box and satisfies every constraint.
    pub(crate) fn is_feasible(&self, point: &Point) -> bool {
        if point.len() != self.inputs.len() {
            return false;
        }
        if !self
            .inputs
            .iter()
            .zip(point)
            .all(|(input, value)| input.contains(*value))
        {
            return false;
        }

        self.bounds.iter().all(|bound| {
            bound
                .evaluate(point)
                .ok()
                // Babel's boolean rewrite yields a residual whose sign carries
                // the truth value: `<= 0` is satisfied. A NaN residual is not a
                // pass.
                .is_some_and(|residual| residual <= 0.0)
        })
    }
}
