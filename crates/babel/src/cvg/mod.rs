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
//! [`FeasibleSamples::generate`] is not.
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
//! cannot be started in. That is the solver's job: when the first batch comes
//! back empty *and* [`Strategy::Solver`] is in the list, Z3 is asked for a
//! seed, and only it can return [`Infeasibility::Proved`]. Without it in the
//! list an empty first batch is simply [`Infeasibility::NotFound`].

mod emit;
mod sampling;
mod sexp;
mod smt;
mod walking;

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::JoinHandle;

use anyhow::{Result, anyhow};
use futures_channel::oneshot;
use rand::SeedableRng;
use rand::rngs::Xoshiro256PlusPlus;

use faer::Mat;

use crate::{Ast, CompiledExpression, Schema};
pub use emit::SmtLogic;
#[doc(hidden)]
pub use sampling::fill_box;
use sampling::{Adaptation, RandomSampler};
use walking::HitAndRunWalker;

#[cfg(test)]
#[path = "judging_tests.rs"]
mod judging_tests;

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

/// One constraint, in a form a caller can read.
///
/// Not the [`Ast`]. A verdict is something a user reads — in a log line, in a UI
/// telling them their formulation conflicts — and handing back a syntax tree
/// makes them render it themselves. The index is there for anyone who wants to
/// find the original in the list they supplied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstraintRef {
    /// Position in the list given to [`ConstraintSystem::new`].
    pub index: usize,
    /// The constraint as it was written.
    pub source: String,
}

impl std::fmt::Display for ConstraintRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.source)
    }
}

/// What a search concluded.
///
/// Two arms, not three. Z3's `sat`/`unsat`/`unknown` never reaches here: an
/// `unknown` that still yielded a point is [`Satisfied`](Satisfiability::Satisfied)
/// like any other, and one that yielded nothing is
/// [`Unsatisfiable`](Satisfiability::Unsatisfiable) with
/// [`Infeasibility::NotFound`] saying so. The trichotomy is a property of one
/// strategy and a caller can do nothing with it.
///
/// **`Satisfied` means at least one sample is already in hand.** That invariant
/// is what makes the two arms sufficient — there is no "probably fine, ask
/// later" state to represent.
#[derive(Debug)]
pub enum Satisfiability {
    Satisfied { samples: FeasibleSamples },
    Unsatisfiable { because: Infeasibility },
}

/// Why no sample was produced — and whether that is a proof or a shrug.
///
/// Kept as two variants rather than a `proved: bool` because they are different
/// sentences to whoever reads the result. *"Your constraints conflict, here are
/// the three involved"* sends someone to rewrite a formulation. *"We found
/// nothing"* sends them to widen a tolerance or wait longer. A flag invites
/// code that ignores it and says the first when it means the second.
#[derive(Debug)]
pub enum Infeasibility {
    /// A solver proved no point exists, and these are the constraints its proof
    /// used.
    ///
    /// A list rather than one culprit: a contradiction is a *relationship*.
    /// `x > 8` is perfectly satisfiable right up until `x < 2` appears, and
    /// naming either alone would be picking arbitrarily. Comes from the unsat
    /// core, so it is the constraints actually used rather than every one
    /// present.
    Proved { blamed: Vec<ConstraintRef> },
    /// Sampling found nothing and no solver could prove anything. **This is not
    /// a claim that the region is empty.**
    ///
    /// `unexpressed` names the constraints no solver could be asked about, which
    /// is usually the reason: a region defined by something outside the theory
    /// can only be found by luck.
    NotFound { unexpressed: Vec<ConstraintRef> },
}

/// A variable box and the constraints over it, proven to fit together.
///
/// The type exists because these two travelled as parallel slices that nothing
/// validated jointly, and because the properties that matter — how many degrees
/// of freedom are left, which variables another determines — are properties of
/// the *set*, not of any member. "System" is the word for constraints considered
/// together, as in a system of equations.
///
/// Construction is where a constraint naming an undeclared variable is caught,
/// which is what leaves [`solve`](ConstraintSystem::solve)'s `Result` about the
/// search and nothing else.
#[derive(Debug, Clone)]
pub struct ConstraintSystem {
    variables: Vec<InputVariable>,
    constraints: Vec<Ast>,
    schema: Schema,
}

/// A system that does not hold together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemError {
    /// A constraint names a variable the box does not declare. It could never be
    /// satisfied, and saying so once beats saying it on every evaluation — which
    /// is what the JVM implementation did.
    Unbound {
        constraint: ConstraintRef,
        missing: Vec<String>,
    },
    /// A scalar expression where a constraint was wanted. It has no `<= 0`
    /// reading, so asserting one would invent a constraint nobody wrote.
    NotAConstraint { constraint: ConstraintRef },
}

impl std::fmt::Display for SystemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unbound {
                constraint,
                missing,
            } => write!(
                f,
                "constraint {constraint} references {} which is not an input variable",
                missing.join(", ")
            ),
            Self::NotAConstraint { constraint } => write!(
                f,
                "{constraint} is a scalar expression, not a constraint: it has no truth value"
            ),
        }
    }
}

impl std::error::Error for SystemError {}

impl ConstraintSystem {
    /// Checks that every constraint is one, and that each binds to the box.
    ///
    /// # Errors
    /// [`SystemError`] for the first constraint that does not fit. One rather
    /// than all: an unbound name is nearly always a typo, and a list of
    /// consequences is less use than the cause.
    pub fn new(variables: Vec<InputVariable>, constraints: Vec<Ast>) -> Result<Self, SystemError> {
        let schema = Schema::new(variables.iter().map(|input| input.name.clone()));

        for (index, constraint) in constraints.iter().enumerate() {
            let named = ConstraintRef {
                index,
                source: constraint.source().to_owned(),
            };
            if !constraint.is_constraint() {
                return Err(SystemError::NotAConstraint { constraint: named });
            }
            if let Err(unbound) = crate::compile(constraint, &schema) {
                return Err(SystemError::Unbound {
                    constraint: named,
                    missing: unbound.missing,
                });
            }
        }

        Ok(Self {
            variables,
            constraints,
            schema,
        })
    }

    #[must_use]
    pub fn variables(&self) -> &[InputVariable] {
        &self.variables
    }

    #[must_use]
    pub fn constraints(&self) -> &[Ast] {
        &self.constraints
    }

    #[must_use]
    pub const fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Searches for feasible samples with the default strategies.
    ///
    /// Sugar for [`ConstraintSolver::new().solve(system)`](ConstraintSolver::solve).
    /// Reach for the builder when the randomness or the strategy list has to be
    /// pinned, which is mostly tests.
    ///
    /// # Errors
    /// Anything that went *wrong*, as opposed to anything that was *concluded*.
    /// An unsatisfiable system is a [`Satisfiability`], not an error.
    pub async fn solve(self) -> Result<Satisfiability> {
        ConstraintSolver::new().solve(self).await
    }

    /// The constraint at `index`, as a caller reads it.
    fn named(&self, index: usize) -> ConstraintRef {
        ConstraintRef {
            index,
            source: self.constraints[index].source().to_owned(),
        }
    }
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
    /// Ask the SMT solver for a first point when sampling found none. The only
    /// strategy that can *prove* a region empty, and the last rung of the
    /// ladder because a solver call costs seconds where a proposal costs
    /// nanoseconds.
    ///
    /// The one a test leaves out when it must measure sampling alone: Z3
    /// answers `x1 > 0.999999` instantly, which would make a time-to-first-hit
    /// fixture a measurement of Z3.
    Solver,
}

/// What production uses: try plain sampling, and reach for the rest only if it
/// does not work. See [`Route`].
///
/// In escalation order. The samplers and the walker are partitioned by role in
/// [`Search::new`] rather than by position, so their order is cosmetic; the
/// solver is genuinely last, consulted only once everything before it has
/// come up empty.
///
/// Public so that tests measuring "what a caller gets" cannot drift from it. A
/// copy of this list living in the test suite is a copy that goes stale, and did.
#[doc(hidden)]
pub const DEFAULT_STRATEGIES: &[Strategy] = &[
    Strategy::UniformSampling,
    Strategy::AdaptiveSampling,
    Strategy::HitAndRun,
    Strategy::Solver,
];

/// The share of a probe that plain sampling must land to be trusted with the
/// job. Ported from the JVM's `EASY_PATH_THRESHOLD_FACTOR`.
const EASY_PATH_THRESHOLD: f64 = 0.1;

/// How many points the route decision is made on.
///
/// Fixed, rather than derived from what a caller asked for or from
/// [`BATCH_SIZE`]. A tuning knob must not be able to move a correctness
/// decision, and this one is close to the line for at least one case in the
/// corpus: `signum` admits about a thousandth of its box, so a hundred-point
/// probe at hundred-fold oversampling lands roughly ten hits against a
/// threshold of ten. Pinning the number keeps that verdict reproducible.
const MINIMUM_PROBE: usize = 100;

/// How many points the worker produces per round trip through the channel.
///
/// Trades channel overhead against shutdown latency and memory: the stop flag
/// is only checked between batches, so this also bounds how long `drop` waits.
///
/// Small, because read-ahead is not free on the expensive problems. Total
/// look-ahead is this times [`CHANNEL_CAPACITY`], and every point of it is
/// produced whether or not anybody asks: at 200 dimensions a point costs some
/// four hundred walker moves, so 64 points of buffer is about three seconds of
/// work done on spec. Cheap problems never notice either number.
const BATCH_SIZE: usize = 32;

/// How many batches may sit unread before the worker blocks.
///
/// This *is* the high-water mark. A bounded channel parks the producer when it
/// is full and wakes it when the consumer drains — which is the whole of
/// "fill up in the background between requests", with no watermarks, condvars
/// or polling to write.
const CHANNEL_CAPACITY: usize = 2;

/// Consecutive empty batches before the worker concludes there is nothing left.
///
/// More than one because an empty batch is not proof: rejection sampling can
/// miss a whole round by luck on a region it usually reaches. More than a
/// handful would just burn cycles on a region that really is exhausted.
const BARREN_BATCHES: usize = 3;

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
/// # use babel::cvg::{ConstraintSystem, InputVariable, Satisfiability};
/// # async fn example() -> anyhow::Result<()> {
/// let system = ConstraintSystem::new(
///     vec![InputVariable::new("x", -1.0, 1.0)],
///     vec![babel::parse("x > 0")?],
/// )?;
///
/// if let Satisfiability::Satisfied { mut samples } = system.solve().await? {
///     // One column per sample, one row per variable — an input matrix as it
///     // stands, no transpose.
///     let batch = samples.take(1_000);
/// }
/// # Ok(())
/// # }
/// ```
/// Deliberately not `Clone`, even though its generator is: two solvers sharing
/// a stream would silently produce the same "random" points, and a `Clone`
/// here would make that a one-word mistake.
#[derive(Debug)]
pub struct ConstraintSolver {
    rng: Xoshiro256PlusPlus,
    known_feasible: Vec<Point>,
    strategies: Vec<Strategy>,
    logic: SmtLogic,
}

impl Default for ConstraintSolver {
    fn default() -> Self {
        Self {
            rng: Xoshiro256PlusPlus::from_rng(&mut rand::rng()),
            known_feasible: Vec::new(),
            strategies: DEFAULT_STRATEGIES.to_vec(),
            logic: SmtLogic::default(),
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
    pub fn with_rng(mut self, rng: Xoshiro256PlusPlus) -> Self {
        self.rng = rng;
        self
    }

    /// The SMT-LIB logic the emitted document declares.
    ///
    /// Rarely worth setting. It exists because the right logic is a property of
    /// the backend and of what the constraints use, and neither is fixed —
    /// see [`SmtLogic`] for the default and for the `BABEL_SMT_LOGIC` escape
    /// hatch this takes precedence over.
    #[must_use]
    pub fn with_logic(mut self, logic: SmtLogic) -> Self {
        self.logic = logic;
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
    /// [`FeasibleSamples::generate`] — which is synchronous — rather than this.
    /// Both are fixable and neither changes this signature: the body moves onto
    /// a thread behind a oneshot, and cancellation becomes a token the search
    /// checks. Recorded in the root `todo.md`; do not read the `async` here as a
    /// promise that a timeout works today.
    ///
    /// # Errors
    /// Anything that went wrong, as opposed to anything that was concluded. An
    /// unsatisfiable problem is a [`Satisfiability`], not an error.
    pub async fn solve(self, system: ConstraintSystem) -> Result<Satisfiability> {
        // Kept so the verdict's constraint indices can be turned back into
        // something a caller reads; the worker takes the originals.
        let blame_table = system.clone();
        let generator = Search::new(
            system.variables,
            system.constraints,
            self.rng,
            &self.strategies,
            self.logic,
        )?;
        let schema = generator.schema.clone();

        let (send_batch, batches) = mpsc::sync_channel(CHANNEL_CAPACITY);
        let (send_opening, opening) = oneshot::channel();
        let stop = Arc::new(AtomicBool::new(false));

        let worker_stop = Arc::clone(&stop);
        let known_feasible = self.known_feasible;
        let worker = std::thread::spawn(move || {
            run(
                generator,
                known_feasible,
                send_opening,
                send_batch,
                &worker_stop,
            );
        });

        let verdict = opening
            .await
            .map_err(|_| anyhow!("the search thread ended without reporting a verdict"))??;

        // Built even for an unsatisfiable problem, which does not keep it: its
        // `Drop` is what joins the worker.
        let pool = FeasibleSamples {
            schema,
            batches,
            buffer: VecDeque::new(),
            worker: Some(worker),
            stop,
            exhausted: false,
            failure: None,
        };

        let name_all = |indices: Vec<usize>| -> Vec<ConstraintRef> {
            indices.into_iter().map(|i| blame_table.named(i)).collect()
        };

        Ok(match verdict {
            // The pool is dropped on both unsatisfiable paths, and its `Drop` is
            // what joins the worker.
            Opening::Satisfied => Satisfiability::Satisfied { samples: pool },
            Opening::Impossible { blamed } => Satisfiability::Unsatisfiable {
                because: Infeasibility::Proved {
                    blamed: name_all(blamed),
                },
            },
            Opening::Unproven { unexpressed } => Satisfiability::Unsatisfiable {
                because: Infeasibility::NotFound {
                    unexpressed: name_all(unexpressed),
                },
            },
        })
    }
}

/// The search itself: a feasible region and the strategies that sample it.
///
/// Lives entirely on the worker thread and is never shared. That is the whole
/// concurrency design — no locks, because there is nothing to lock. What the
/// caller holds is [`FeasibleSamples`], which is a handle to this and owns none
/// of it.
///
/// Owns its accumulated points rather than making the caller pass them back in
/// on every call, and filters its own output. The JVM interface did neither —
/// it took `existingPoints` as a parameter and warned in a comment that
/// *"the results may not actually be feasible! you must filter this list on the
/// callers side!"*.
pub(crate) struct Search {
    schema: Schema,
    inputs: Vec<InputVariable>,
    constraints: Vec<Ast>,
    /// Carried rather than defaulted at the point of use, so that a document is
    /// emitted under the logic the caller chose and not under whatever the
    /// worker thread's environment happens to say.
    pub(crate) logic: SmtLogic,
    /// Whether [`Strategy::Solver`] was configured. Not a [`PointSource`]: the
    /// solver needs the whole search to emit a document, and it runs once, on
    /// the opening, rather than per batch.
    solver: bool,
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

impl std::fmt::Debug for Search {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Search")
            .field("inputs", &self.inputs.len())
            .field("constraints", &self.constraints.len())
            .field("found", &self.found.len())
            .field("route", &self.route)
            .field("solver", &self.solver)
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

impl Search {
    fn new(
        inputs: Vec<InputVariable>,
        constraints: Vec<Ast>,
        mut rng: Xoshiro256PlusPlus,
        strategies: &[Strategy],
        logic: SmtLogic,
    ) -> Result<Self> {
        let schema = Schema::new(inputs.iter().map(|input| input.name.clone()));

        // Fail here rather than per-evaluation: a constraint naming a variable
        // the box does not declare can never be satisfied, and saying so once is
        // kinder than saying it a thousand times.
        for constraint in &constraints {
            crate::compile(constraint, &schema).map_err(|missing| {
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
        let mut solver = false;
        for strategy in strategies {
            let stream = Xoshiro256PlusPlus::from_rng(&mut rng);
            match strategy {
                // Draws a stream like the others so that adding or removing it
                // does not reseed whatever comes after it in the list.
                Strategy::Solver => solver = true,
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
            logic,
            solver,
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
    fn produce(&mut self, count: usize) -> Vec<Point> {
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
            let probe = MINIMUM_PROBE;
            let candidates = fair.generate(probe, &self.found, &context);
            let landed = context.feasible_columns(candidates.as_ref());

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
            // Every proposed candidate is judged, where the one-at-a-time filter
            // used to stop at `count` hits. The stream is consumed identically,
            // so the points that come out are the same; only the judging of the
            // surplus is extra, and a batch is what makes judging cheap.
            let candidates = fair.generate(count, &self.found, &context);
            let mut delivered = context.feasible_columns(candidates.as_ref());
            delivered.truncate(count);
            self.found.extend(delivered.iter().cloned());
            return delivered;
        }

        // Seeders feed the pool without delivering. Their job is to reach the
        // region at all; the emitter's job is to sample it evenly, and mixing
        // the two would put biased points in the caller's hands.
        for seeder in &mut self.seeders {
            let candidates = seeder.generate(count, &self.found, &context);
            let feasible = context.feasible_columns(candidates.as_ref());
            self.found.extend(feasible);
        }

        let mut accepted: Vec<Point> = Vec::new();
        for emitter in &mut self.emitters {
            if accepted.len() >= count {
                break;
            }
            let wanted = count - accepted.len();
            let candidates = emitter.generate(wanted, &self.found, &context);
            let mut feasible = context.feasible_columns(candidates.as_ref());
            feasible.truncate(wanted);
            accepted.extend(feasible);
        }

        self.found.extend(accepted.iter().cloned());
        accepted
    }

    /// Adopts points a caller believes are feasible, discarding any that are
    /// not. Returns how many were kept.
    fn seed(&mut self, points: Vec<Point>) -> usize {
        let context = SearchContext::new(&self.inputs, &self.constraints, &self.schema);
        let before = self.found.len();
        self.found.extend(
            points
                .into_iter()
                .filter(|point| context.is_feasible(point)),
        );
        self.found.len() - before
    }
}

/// What a pool is doing, when it is not simply handing over points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Still producing, or at least still trying.
    Filling,
    /// The worker finished. There will be no more points, ever.
    Exhausted,
    /// The worker panicked, and this is what with.
    ///
    /// Separate from [`Status::Exhausted`] on purpose: "no more points exist"
    /// and "we broke" are different facts, and folding the second into the first
    /// would hide a defect behind a legitimate-looking state.
    Failed(String),
}

/// A feasible region being sampled on a background thread.
///
/// Holds no search state — that is [`Search`], which the worker owns
/// outright. This is a receiving end, a buffer, and the means to stop the
/// worker.
pub struct FeasibleSamples {
    schema: Schema,
    batches: Receiver<Vec<Point>>,
    buffer: VecDeque<Point>,
    worker: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    /// Set once the channel disconnects. The worker is gone and no amount of
    /// waiting will produce more.
    exhausted: bool,
    failure: Option<String>,
}

impl std::fmt::Debug for FeasibleSamples {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FeasibleSamples")
            .field("buffered", &self.buffer.len())
            .field("status", &self.status())
            .finish()
    }
}

impl FeasibleSamples {
    /// Up to `count` samples, waiting for them.
    ///
    /// **One column per sample, one row per schema variable** — the shape
    /// [`CompiledExpression::eval`](crate::CompiledExpression::eval) takes, so a
    /// batch goes straight back in with no transpose.
    ///
    /// Fewer than `count` means the search is exhausted and no amount of waiting
    /// will produce more. That is a real outcome, not an error: a region can
    /// yield forty points and then nothing, ever, and blocking forever on the
    /// forty-first is the hang this returns short to avoid.
    ///
    /// Blocking rather than `async`, for now. The producer is a thread and the
    /// channel is `std::sync::mpsc`, so waiting here is a real park rather than
    /// a spin; making this `async` honestly means an async-aware channel, which
    /// is a change to the worker and not to this signature. Use
    /// [`try_take`](Self::try_take) from a context that must not block.
    pub fn take(&mut self, count: usize) -> Mat<f64> {
        while self.buffer.len() < count && !self.exhausted {
            match self.batches.recv() {
                Ok(batch) => self.buffer.extend(batch),
                Err(_) => {
                    self.exhausted = true;
                    self.failure = self.worker.take().and_then(reap);
                }
            }
        }
        self.drain(count)
    }

    /// Up to `count` samples from what is already buffered. Never waits.
    ///
    /// Named `try_take` and not `poll`: `poll` is the async primitive, and a
    /// method by that name on a type callers `await` around would read as one.
    pub fn try_take(&mut self, count: usize) -> Mat<f64> {
        while self.buffer.len() < count {
            match self.batches.try_recv() {
                Ok(batch) => self.buffer.extend(batch),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.exhausted = true;
                    self.failure = self.worker.take().and_then(reap);
                    break;
                }
            }
        }
        self.drain(count)
    }

    /// How many samples can be had right now without waiting.
    #[must_use]
    pub fn available(&self) -> usize {
        self.buffer.len()
    }

    /// Whether the search has finished. No further sample will ever arrive.
    #[must_use]
    pub const fn is_exhausted(&self) -> bool {
        self.exhausted
    }

    /// Stop producing.
    ///
    /// [`Drop`] does this too; calling it early is for a caller who has enough
    /// and wants the worker's CPU back before the handle goes out of scope.
    pub fn close(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Takes `count` from the buffer as a column-per-sample matrix.
    fn drain(&mut self, count: usize) -> Mat<f64> {
        let taken = count.min(self.buffer.len());
        let rows = self.schema.len();
        // `from_fn` visits in the matrix's own order, so the points come out of
        // the buffer by index rather than by draining as it goes.
        let samples = Mat::from_fn(rows, taken, |row, column| self.buffer[column][row]);
        self.buffer.drain(..taken);
        samples
    }

    #[must_use]
    pub fn status(&self) -> Status {
        match (&self.failure, self.exhausted) {
            (Some(panic), _) => Status::Failed(panic.clone()),
            (None, true) => Status::Exhausted,
            (None, false) => Status::Filling,
        }
    }

    /// The order a [`Point`]'s values are in.
    #[must_use]
    pub const fn schema(&self) -> &Schema {
        &self.schema
    }
}

impl Drop for FeasibleSamples {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);

        // Draining is not tidiness, it is the difference between joining and
        // deadlocking. `drop` runs before the fields do, so the receiver is
        // still alive here — and a worker parked on a full channel stays parked
        // until somebody reads. Emptying it lets that last `send` return, at
        // which point the worker sees the stop flag and exits, the sender drops,
        // and `recv` finally errors out of this loop.
        while self.batches.recv().is_ok() {}

        if let Some(handle) = self.worker.take() {
            drop(handle.join());
        }
    }
}

/// Collects a finished worker, describing a panic if it left one.
///
/// Takes the handle by value so that the caller does the storing — the failure
/// travels back as a return value rather than being written to a field from in
/// here.
fn reap(handle: JoinHandle<()>) -> Option<String> {
    let payload = handle.join().err()?;
    Some(
        payload
            .downcast_ref::<&str>()
            .map(|text| (*text).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "worker panicked".to_owned()),
    )
}

/// What the worker concluded while looking for its first point.
///
/// Sent exactly once, and the thing [`ConstraintSolver::solve`] awaits. Carries
/// constraint *indices* rather than expressions so the worker never needs a copy
/// of them.
enum Opening {
    /// At least one sample is in hand. The invariant `Satisfiability::Satisfied`
    /// rests on, and the reason Z3's `unknown` need not surface: an `unknown`
    /// that still produced a point arrives here like any other success.
    Satisfied,
    /// A solver proved the region empty.
    Impossible { blamed: Vec<usize> },
    /// Nothing found and nothing proven, carrying whatever could not be
    /// expressed — which is usually why.
    Unproven { unexpressed: Vec<usize> },
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
fn repaired(mut point: Point, context: &SearchContext<'_>) -> Option<Point> {
    if context.is_feasible(&point) {
        return Some(point);
    }

    for sweep in 0..REPAIR_SWEEPS {
        let mut improved = false;

        for index in 0..point.len() {
            let before = context.worst_residual(&point)?;
            let original = point[index];

            // Growing the step across sweeps: an ulp first, because that is what
            // a boundary witness misses by, then wider in case the rounding
            // compounded through a longer expression.
            let step = ulps(original, 1 << (2 * sweep));

            for candidate in [original + step, original - step] {
                point[index] = candidate;
                let better = context
                    .worst_residual(&point)
                    .is_some_and(|after| after < before);
                if better {
                    improved = true;
                    break;
                }
                point[index] = original;
            }
        }

        if context.is_feasible(&point) {
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

/// The worker: find a first point, report the verdict, then keep filling.
fn run(
    mut generator: Search,
    known_feasible: Vec<Point>,
    opening: oneshot::Sender<Result<Opening>>,
    batches: SyncSender<Vec<Point>>,
    stop: &AtomicBool,
) {
    generator.seed(known_feasible);

    // The first batch doubles as the probe: producing anything at all settles
    // that the region is reachable, and the points are as good as any that would
    // follow.
    let mut first = generator.produce(BATCH_SIZE);
    let verdict = if !first.is_empty() {
        Ok(Opening::Satisfied)
    } else if !generator.solver {
        // Sampling found nothing and there is no solver in the ladder to ask.
        // Every constraint is then "unexpressed" in the sense `NotFound` uses:
        // none was put to anything that could reason about it.
        tracing::debug!("first batch empty and no solver configured; giving up without escalating");
        Ok(Opening::Unproven {
            unexpressed: (0..generator.constraints.len()).collect(),
        })
    } else {
        match smt::escalate_for_seed(&generator) {
            Ok(smt::Verdict::Impossible { blamed }) => Ok(Opening::Impossible { blamed }),
            Ok(smt::Verdict::Inconclusive { unexpressed }) => Ok(Opening::Unproven { unexpressed }),
            Ok(smt::Verdict::Seed { point, unexpressed }) => {
                // The witness is exact in real arithmetic and need not be in
                // `f64`. Repairing beats discarding: the solver call that found
                // it is the expensive part, and the miss is in the last place.
                let context = SearchContext::new(
                    &generator.inputs,
                    &generator.constraints,
                    &generator.schema,
                );
                let seed = repaired(point, &context);
                drop(context);
                generator.seed(seed.into_iter().collect());
                first = generator.produce(BATCH_SIZE);
                // A seed is not a sample: it satisfies whatever could be
                // expressed, and the pool filters against *everything*. If
                // nothing survives that, there is nothing in hand and saying
                // `Satisfied` would break the one invariant it carries.
                Ok(if first.is_empty() {
                    Opening::Unproven { unexpressed }
                } else {
                    Opening::Satisfied
                })
            }
            Err(error) => Err(error),
        }
    };

    let deliverable = matches!(verdict, Ok(Opening::Satisfied));
    if opening.send(verdict).is_err() || !deliverable {
        // Either the caller gave up before we answered, or there is nothing to
        // deliver. Dropping `batches` on the way out is what tells the pool it
        // is exhausted rather than merely slow.
        return;
    }

    if !first.is_empty() && batches.send(first).is_err() {
        return;
    }

    let mut barren = 0;
    while !stop.load(Ordering::Relaxed) {
        let batch = generator.produce(BATCH_SIZE);
        if batch.is_empty() {
            barren += 1;
            if barren >= BARREN_BATCHES {
                return;
            }
            continue;
        }
        barren = 0;
        if batches.send(batch).is_err() {
            return;
        }
    }
}

/// A strategy for proposing candidate points.
///
/// Proposals are filtered by [`FeasibleSamples::generate`], so an implementation
/// may return infeasible candidates — it is not required to check.
pub(crate) trait PointSource: Send {
    fn name(&self) -> &'static str;

    /// Propose candidates, optionally informed by what has already been found,
    /// as a matrix with one column per candidate and one row per variable —
    /// the shape the batched evaluator judges in one call. A source may return
    /// more than `count` (the samplers over-propose) or none
    /// (`Mat::zeros(rows, 0)`); the pool filters and truncates. `existing` is
    /// empty on the first call, which is what makes seeding the hard case.
    fn generate(
        &mut self,
        count: usize,
        existing: &[Point],
        context: &SearchContext<'_>,
    ) -> Mat<f64>;
}

/// Points as a matrix, one column each: the shape [`PointSource::generate`]
/// returns and [`FeasibleSamples`] hands out.
pub(crate) fn points_to_matrix(points: &[Point], rows: usize) -> Mat<f64> {
    Mat::from_fn(rows, points.len(), |row, column| points[column][row])
}

/// The box and the constraints, in the form a strategy needs them.
///
/// Passed to every [`PointSource`] because a strategy that searches — as opposed
/// to one that guesses — cannot work without asking whether a point is feasible.
/// Bisecting out to the edge of a region is nothing but that question, repeated.
pub(crate) struct SearchContext<'a> {
    inputs: &'a [InputVariable],
    bounds: Vec<CompiledExpression>,
}

impl<'a> SearchContext<'a> {
    fn new(inputs: &'a [InputVariable], constraints: &'a [Ast], schema: &'a Schema) -> Self {
        let bounds = constraints
            .iter()
            .map(|constraint| {
                crate::compile(constraint, schema)
                    .expect("`FeasibleSamples::new` already proved every constraint binds")
            })
            .collect();
        Self { inputs, bounds }
    }

    pub(crate) const fn inputs(&self) -> &'a [InputVariable] {
        self.inputs
    }

    /// How badly the worst constraint is violated, or `None` if the point is
    /// outside the box or cannot be evaluated.
    ///
    /// [`is_feasible`](Self::is_feasible) asks a yes-or-no question; this asks
    /// *how far*, which is what the `<= 0` convention makes available and what a
    /// repair needs in order to know which way to step.
    pub(crate) fn worst_residual(&self, point: &Point) -> Option<f64> {
        if point.len() != self.inputs.len() {
            return None;
        }
        if !self
            .inputs
            .iter()
            .zip(point)
            .all(|(input, value)| input.contains(*value))
        {
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
    /// that propose thousands of independent candidates at once. Judged with
    /// the lenient evaluator: a candidate whose evaluation faults — `ln` of a
    /// negative, a subscript out of range — is infeasible, not fatal, because
    /// NaN fails `<= 0`.
    pub(crate) fn feasible_columns(&self, candidates: faer::MatRef<'_, f64>) -> Vec<Point> {
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
            let residuals = bound.eval_lenient(candidates).expect(
                "`Search::new` proved every constraint binds, and candidates are shaped by the same box",
            );
            for (column, pass) in pass.iter_mut().enumerate() {
                *pass &= residuals[column] <= 0.0;
            }
        }

        (0..columns)
            .filter(|&column| pass[column])
            .map(|column| (0..rows).map(|row| candidates[(row, column)]).collect())
            .collect()
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context_for(source: &str) -> (Vec<InputVariable>, Vec<Ast>, Schema) {
        let inputs = vec![InputVariable::new("x1", 0.0, 10.0)];
        let constraints = vec![crate::parse(source).expect("fixture should compile")];
        let schema = Schema::new(inputs.iter().map(|input| input.name.clone()));
        (inputs, constraints, schema)
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
        let (inputs, constraints, schema) = context_for("x1 == pi +/- 0.001");
        let context = SearchContext::new(&inputs, &constraints, &schema);

        // The value Z3 actually returns, as a decimal parsed back into f64 —
        // not `PI - 0.001`, which Rust computes to a *different* f64 and which
        // happens to land inside. That difference is the entire bug.
        let edge: f64 = "3.140592653589793".parse().expect("a literal");
        assert!(
            !context.is_feasible(&vec![edge]),
            "this test is pointless unless the boundary really does miss"
        );
        let repaired_edge = repaired(vec![edge], &context).expect("a near-miss should be repaired");
        assert!(context.is_feasible(&repaired_edge));
        assert!(
            (repaired_edge[0] - edge).abs() < 1e-12,
            "repair moved the point {} away from the witness, which is not a nudge",
            (repaired_edge[0] - edge).abs()
        );

        assert!(
            repaired(vec![7.0], &context).is_none(),
            "a point nowhere near the band was 'repaired' into feasibility"
        );
    }

    /// `worst_residual` has to grade, not just judge — a repair steps downhill
    /// and there is no hill in a boolean.
    #[test]
    fn the_worst_residual_is_graded() {
        let (inputs, constraints, schema) = context_for("x1 > 4");
        let context = SearchContext::new(&inputs, &constraints, &schema);

        let near = context.worst_residual(&vec![3.9]).expect("inside the box");
        let far = context.worst_residual(&vec![1.0]).expect("inside the box");
        assert!(
            near < far,
            "{near} should be a smaller violation than {far}"
        );
        assert!(context.worst_residual(&vec![5.0]).is_some_and(|r| r <= 0.0));
        assert!(
            context.worst_residual(&vec![99.0]).is_none(),
            "outside the box is not a residual"
        );
    }
}
