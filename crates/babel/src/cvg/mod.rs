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
//! The strategies divide along that line:
//!
//! * **Uniform rejection sampling** — the brute squad — *probes*, and on a
//!   region it reaches often enough it simply delivers: unbiased by
//!   construction, no burn-in, no chain. Where the probe lands nothing and the
//!   solver could not settle it, the same sampler keeps proposing on every
//!   core, for a proposal budget, until one batch lands: a region a millionth
//!   or a hundred-millionth of its box is a matter of milliseconds to seconds,
//!   and the seed it finds is what the walker starts from.
//! * **Hit-and-run** *emits* everywhere the probe did not settle it. It
//!   converges to the uniform distribution over the region, so what a caller
//!   receives is governed by the strategy with a guarantee. It cannot start
//!   without a feasible point, and a seed comes from the probe's own hits,
//!   from the solver, or from brute force.
//!
//! Neither can reach a region of measure zero — an equality constraint with a
//! tolerance tight enough is a ribbon that sampling will not land on and a walk
//! cannot be started in. That is the solver's job, and it goes *before* brute
//! force: when the probe comes back empty *and* [`Strategy::Solver`] is in
//! the list, Z3 is asked for a seed. It settles a ribbon or a contradiction
//! in milliseconds where brute force would spend its whole budget, and only
//! it can return [`Infeasibility::Proved`]. What it answers `unknown` on —
//! anything transcendental — is exactly what brute force then spends the
//! budget on. Without a solver in the list the probe hands straight to brute
//! force, and an empty search is simply [`Infeasibility::NotFound`].

mod emit;
mod problem;
mod progress;
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

use crate::{Ast, Schema};
pub use emit::SmtLogic;
use problem::{Problem, repaired};
use progress::{Progress, Route, Trial};
use sampling::RandomSampler;
#[doc(hidden)]
pub use sampling::fill_box;
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
    /// Rejection sampling over the declared box, never narrowed. Uniform over
    /// the feasible region by construction: the probe that decides the route,
    /// the thing that delivers where the probe succeeds, and the fairness
    /// oracle the tests measure against.
    ///
    /// Where the probe lands nothing, and the solver — if configured — could
    /// not settle it either, it is the brute squad: the same proposals, wider,
    /// on every core, for [`ConstraintSolver::with_proposal_budget`]
    /// candidates, to land the seed the walker needs. What it lands is a
    /// function of the seed and the budget, never of the thread count.
    BruteSquad,
    /// Hit-and-run: walk the chord of the region through the current point.
    /// Converges to the uniform distribution, but needs a feasible point to
    /// start from and crosses between disconnected pieces only by luck.
    HitAndRun,
    /// Ask the SMT solver for a first point when the probe found none. The
    /// only strategy that can *prove* a region empty. Asked *before* brute
    /// force, not after: a contradiction or an equality ribbon is settled in
    /// milliseconds where brute force would spend its whole budget, and what
    /// the solver answers `unknown` on — anything transcendental — is handed
    /// to brute force with the constraints it could not express.
    ///
    /// The one a test leaves out when it must measure sampling alone: Z3
    /// answers `x1 > 0.999999` instantly, which would make a time-to-first-hit
    /// fixture a measurement of Z3. Without it the probe hands straight to
    /// brute force.
    Solver,
}

/// What production uses: try plain sampling, and reach for the rest only if it
/// does not work. See [`Route`].
///
/// The strategies are partitioned by role in [`Ladder::new`] rather than by
/// position, so the order here is cosmetic. The actual order of escalation is
/// fixed by [`open`]: probe, then solver, then brute force, then the walker
/// from whatever seed those produced.
///
/// Public so that tests measuring "what a caller gets" cannot drift from it. A
/// copy of this list living in the test suite is a copy that goes stale, and did.
#[doc(hidden)]
pub const DEFAULT_STRATEGIES: &[Strategy] =
    &[Strategy::BruteSquad, Strategy::HitAndRun, Strategy::Solver];

/// Candidates the brute-force search proposes before giving up, unless
/// [`ConstraintSolver::with_proposal_budget`] says otherwise.
///
/// A billion: a few seconds across a laptop's sixteen threads and a quarter
/// of a minute on one, which reaches a region a hundred-millionth of its box
/// with ten expected hits and gives up on a ten-billionth in a time a caller
/// can wait out. Spent only on what the solver could not decide. A count
/// rather than a duration so that the same seed finds the same point on
/// every machine.
pub const DEFAULT_PROPOSAL_BUDGET: u64 = 1_000_000_000;

/// How much work Z3 may do before giving up with `unknown`, unless
/// [`ConstraintSolver::with_solver_limit`] says otherwise.
///
/// In Z3's resource units, so that the same problem answers the same way on
/// every machine. Three million is about twenty-five seconds on this laptop
/// for a mixed integer-nonlinear instance (measured at eight seconds per
/// million, roughly linear up to there and not beyond), which is the "tough"
/// regime's wait; a contradiction or a ribbon is settled in milliseconds and
/// never approaches it. An `unknown` from the limit hands over to brute force
/// like any other.
pub const DEFAULT_SOLVER_LIMIT: u32 = 3_000_000;

/// The hit rate below which plain sampling is not trusted to deliver.
///
/// A rate, judged on the probe. Below it the delivery batches — a hundred
/// candidates per point asked for — come back empty often enough that
/// [`BARREN_BATCHES`] would call a live region exhausted: at one in a
/// thousand a batch for 32 points expects 3.2 hits and is empty four times in
/// a hundred, three in a row six times in a hundred thousand; at one in ten
/// thousand it is empty three times in four. The JVM's
/// `EASY_PATH_THRESHOLD_FACTOR` was a tenth of the points *asked for* at
/// hundredfold oversampling, which is the same rate.
const EASY_PATH_THRESHOLD: f64 = 0.001;

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
    budgets: Budgets,
}

/// How much each rung of the ladder may spend before handing over.
///
/// Every one a count rather than a clock, so that the same seed reaches the
/// same verdict on every machine; the thread count changes only how soon.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Budgets {
    /// See [`ConstraintSolver::with_proposal_budget`].
    pub(crate) proposals: u64,
    /// See [`ConstraintSolver::with_threads`].
    pub(crate) threads: usize,
    /// See [`ConstraintSolver::with_solver_limit`].
    pub(crate) solver_limit: u32,
}

impl Default for Budgets {
    fn default() -> Self {
        Self {
            proposals: DEFAULT_PROPOSAL_BUDGET,
            threads: std::thread::available_parallelism().map_or(1, std::num::NonZero::get),
            solver_limit: DEFAULT_SOLVER_LIMIT,
        }
    }
}

impl Default for ConstraintSolver {
    fn default() -> Self {
        Self {
            rng: Xoshiro256PlusPlus::from_rng(&mut rand::rng()),
            known_feasible: Vec::new(),
            strategies: DEFAULT_STRATEGIES.to_vec(),
            logic: SmtLogic::default(),
            budgets: Budgets::default(),
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

    /// How many candidates brute force may propose before giving up.
    ///
    /// A *proposal* is one random point in the declared box, judged against
    /// every constraint. When the opening probe lands nothing and the solver,
    /// if configured, comes back without a proof or a usable witness, the pool
    /// keeps proposing on every core until a batch lands or this many have
    /// been judged. The default
    /// is [`DEFAULT_PROPOSAL_BUDGET`]; the cost is some seventy million
    /// proposals a second per core on a simple constraint set. Zero skips
    /// brute force entirely. A count rather than a duration, so that the same
    /// seed finds the same point on every machine.
    #[must_use]
    pub const fn with_proposal_budget(mut self, proposals: u64) -> Self {
        self.budgets.proposals = proposals;
        self
    }

    /// How much work the SMT solver may do before it gives up with `unknown`.
    ///
    /// In Z3's own resource units — a count of the work it has done, not a
    /// clock — so that the same problem gets the same answer on every
    /// machine. The default is [`DEFAULT_SOLVER_LIMIT`]; zero is no limit at
    /// all, which is what a dropped [`solve`](Self::solve) future cannot
    /// interrupt. An `unknown` from the limit is handled like any other:
    /// brute force gets the budget.
    #[must_use]
    pub const fn with_solver_limit(mut self, limit: u32) -> Self {
        self.budgets.solver_limit = limit;
        self
    }

    /// Pins how many threads brute force fans out over.
    ///
    /// Hidden because it never changes what is found — a test uses it to
    /// prove exactly that. Defaults to the available parallelism.
    #[doc(hidden)]
    #[must_use]
    pub const fn with_threads(mut self, threads: usize) -> Self {
        self.budgets.threads = threads;
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
    /// The search runs on its own thread and this future waits on the opening
    /// verdict, so a `timeout` around it does fire. **Dropping the future is
    /// how to cancel:** a brute-force search notices between batches and
    /// stops, freeing every core it took. A solver call in progress is not
    /// interruptible, but it is bounded by [`with_solver_limit`](Self::with_solver_limit)
    /// and runs to that on the abandoned thread; and [`FeasibleSamples::take`]
    /// is synchronous by design. Recorded in the root `todo.md`.
    ///
    /// # Errors
    /// Anything that went wrong, as opposed to anything that was concluded. An
    /// unsatisfiable problem is a [`Satisfiability`], not an error.
    pub async fn solve(self, system: ConstraintSystem) -> Result<Satisfiability> {
        // Kept so the verdict's constraint indices can be turned back into
        // something a caller reads; the worker takes the originals.
        let blame_table = system.clone();
        let problem = Problem::new(system, self.logic);
        let schema = problem.schema().clone();
        let ladder = Ladder::new(&problem, self.rng, &self.strategies, self.budgets);

        let (send_batch, batches) = mpsc::sync_channel(CHANNEL_CAPACITY);
        let (send_opening, opening) = oneshot::channel();
        let stop = Arc::new(AtomicBool::new(false));

        let worker_stop = Arc::clone(&stop);
        let known_feasible = self.known_feasible;
        let worker = std::thread::spawn(move || {
            serve(
                &problem,
                ladder,
                known_feasible,
                send_opening,
                &send_batch,
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

/// The strategies, holding nothing but their streams and their knobs.
///
/// Lives entirely on the worker thread and is never shared. That is the whole
/// concurrency design — no locks, because there is nothing to lock. What the
/// caller holds is [`FeasibleSamples`], which is a handle to the worker and
/// owns none of this. What the search has *found* is not here either: that is
/// a [`Progress`] value the worker threads through its loop.
struct Ladder {
    /// Uniform rejection sampling over the declared box, if configured. The
    /// probe that decides the [`Route`], the thing that delivers where that
    /// probe succeeds, and the brute squad where it does not.
    sampler: Option<RandomSampler>,
    /// Delivers on the walking route, from whatever points are in hand.
    walker: Option<HitAndRunWalker>,
    /// The solver's resource limit, when [`Strategy::Solver`] is configured.
    /// Not a strategy object: the solver needs the whole problem to emit a
    /// document, and it runs once, on the opening, rather than per batch.
    solver: Option<u32>,
}

impl std::fmt::Debug for Ladder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ladder")
            .field("sampler", &self.sampler.is_some())
            .field("walker", &self.walker.is_some())
            .field("solver", &self.solver)
            .finish()
    }
}

impl Ladder {
    fn new(
        problem: &Problem,
        mut rng: Xoshiro256PlusPlus,
        strategies: &[Strategy],
        budgets: Budgets,
    ) -> Self {
        let mut ladder = Self {
            sampler: None,
            walker: None,
            solver: None,
        };
        // Each strategy gets its own stream, derived from the one passed in and
        // drawn in list order, so that adding or removing a strategy does not
        // reseed the ones after it.
        for strategy in strategies {
            let stream = Xoshiro256PlusPlus::from_rng(&mut rng);
            match strategy {
                Strategy::Solver => ladder.solver = Some(budgets.solver_limit),
                Strategy::BruteSquad => {
                    ladder.sampler = Some(RandomSampler::new(
                        problem.box_bounds(),
                        stream,
                        budgets.proposals,
                        budgets.threads,
                    ));
                }
                Strategy::HitAndRun => ladder.walker = Some(HitAndRunWalker::new(stream)),
            }
        }
        ladder
    }

    /// Which route the probe's trial settles.
    ///
    /// Plain sampling is the only thing configured when there is no walker, so
    /// there is no decision to make and nothing to fall back to.
    fn route_for(&self, probe: &Trial) -> Route {
        if self.walker.is_none() {
            return Route::Sampling;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "sample counts are far below the f64 integer limit"
        )]
        let enough = probe.points.len() as f64 >= EASY_PATH_THRESHOLD * probe.proposed as f64;
        if enough {
            Route::Sampling
        } else {
            Route::Walking
        }
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
/// Holds no search state — the [`Ladder`] and the [`Progress`] value live on
/// the worker thread and nowhere else. This is a receiving end, a buffer, and the means to stop the
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

/// The caller's way of saying "never mind".
///
/// Dropping the [`solve`](ConstraintSolver::solve) future drops the receiving
/// end of the opening channel, and the sending end can see that. Brute force
/// asks between batches; nothing else runs long enough to need to.
pub(crate) struct Cancellation<'a>(&'a oneshot::Sender<Result<Opening>>);

impl Cancellation<'_> {
    pub(crate) fn is_requested(&self) -> bool {
        self.0.is_canceled()
    }
}

/// The worker's thread body: open, report the verdict, keep filling.
fn serve(
    problem: &Problem,
    mut ladder: Ladder,
    known: Vec<Point>,
    opening: oneshot::Sender<Result<Opening>>,
    batches: &SyncSender<Vec<Point>>,
    stop: &AtomicBool,
) {
    // Hints are judged, not trusted, and count as points rather than trials.
    let progress = Progress::empty().extend(problem.keep_feasible(known));
    let cancel = Cancellation(&opening);

    let (verdict, progress) = match open(problem, &mut ladder, progress, &cancel) {
        Ok((verdict, progress)) => (verdict, progress),
        Err(error) => {
            drop(opening.send(Err(error)));
            return;
        }
    };
    if cancel.is_requested() {
        // The caller dropped the future mid-search. There is nobody to report
        // to, and brute force stopped for exactly that reason.
        return;
    }
    tracing::debug!(
        points = progress.points().len(),
        proposed = progress.proposed(),
        landed = progress.landed(),
        route = ?progress.route(),
        "opened"
    );

    let deliverable = matches!(verdict, Opening::Satisfied);
    if opening.send(Ok(verdict)).is_err() || !deliverable {
        // Either the caller gave up before we answered, or there is nothing to
        // deliver. Dropping `batches` on the way out is what tells the pool it
        // is exhausted rather than merely slow.
        return;
    }

    // The points in hand are the first batch: every one is feasible, and they
    // are as good as any that would follow.
    let first: Vec<Point> = progress.points().iter().take(BATCH_SIZE).cloned().collect();
    if batches.send(first).is_err() {
        return;
    }
    keep_filling(problem, &mut ladder, progress, batches, stop);
}

/// The opening: probe, then the solver, then brute force, in that order and
/// with no flags between them. Each rung runs only if the ones before it left
/// nothing in hand.
///
/// The probe is one brute-force batch, tens of microseconds, and settles most
/// problems outright. The solver goes next because it settles a contradiction
/// or an equality ribbon in milliseconds, where brute force would spend its
/// whole budget, and what it answers `unknown` on — anything transcendental,
/// anything past its resource limit — is exactly what brute force is for.
///
/// `Satisfied` means at least one feasible point is in hand, which is what
/// [`Satisfiability::Satisfied`] promises.
fn open(
    problem: &Problem,
    ladder: &mut Ladder,
    progress: Progress,
    cancel: &Cancellation<'_>,
) -> Result<(Opening, Progress)> {
    let mut progress = match &mut ladder.sampler {
        Some(sampler) => {
            let probe = sampler.probe(problem);
            let route = ladder.route_for(&probe);
            progress.absorb(probe).pin(route)
        }
        None => progress.pin(Route::Walking),
    };
    if !progress.is_empty() {
        return Ok((Opening::Satisfied, progress));
    }

    let unexpressed = match ladder.solver {
        // Every constraint is then "unexpressed" in the sense `NotFound` uses:
        // none was put to anything that could reason about it.
        None => (0..problem.constraints().len()).collect(),
        Some(limit) => match smt::escalate_for_seed(problem, limit)? {
            smt::Verdict::Impossible { blamed } => {
                return Ok((Opening::Impossible { blamed }, progress));
            }
            smt::Verdict::Inconclusive { unexpressed } => unexpressed,
            smt::Verdict::Seed { point, unexpressed } => {
                // The witness is exact in real arithmetic and need not be in
                // `f64`. Repairing beats discarding: the solver call that found
                // it is the expensive part, and the miss is in the last place.
                // A seed is not a sample either: it satisfies whatever could
                // be expressed, and it is judged against *everything*; if it
                // does not survive that, brute force still gets its turn.
                let witness = repaired(point, problem).into_iter().collect();
                progress = progress.extend(problem.keep_feasible(witness));
                unexpressed
            }
        },
    };

    if progress.is_empty()
        && let Some(sampler) = &mut ladder.sampler
    {
        let trial = sampler.brute_force(problem, cancel);
        tracing::info!(
            proposed = trial.proposed,
            landed = trial.points.len(),
            "brute force"
        );
        progress = progress.absorb(trial);
    }

    Ok(if progress.is_empty() {
        (Opening::Unproven { unexpressed }, progress)
    } else {
        (Opening::Satisfied, progress)
    })
}

/// The steady state: one batch per trip through the channel until the caller
/// stops asking or the region runs dry.
fn keep_filling(
    problem: &Problem,
    ladder: &mut Ladder,
    mut progress: Progress,
    batches: &SyncSender<Vec<Point>>,
    stop: &AtomicBool,
) {
    let mut barren = 0;
    while !stop.load(Ordering::Relaxed) {
        let (batch, next) = next_batch(problem, ladder, progress, BATCH_SIZE);
        progress = next;
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

/// At most `count` feasible points, and the progress that now includes them.
///
/// Which strategy delivers is read off the route the probe pinned. Fewer than
/// asked for is normal — a strategy may simply not find that many in one pass.
/// Never more, and never an infeasible one.
fn next_batch(
    problem: &Problem,
    ladder: &mut Ladder,
    progress: Progress,
    count: usize,
) -> (Vec<Point>, Progress) {
    match (progress.route(), &mut ladder.sampler, &mut ladder.walker) {
        (Route::Sampling, Some(sampler), _) => {
            let trial = sampler.deliver(problem, count);
            (trial.points.clone(), progress.absorb(trial))
        }
        (Route::Walking, _, Some(walker)) => {
            let walked = walker.extend(problem, progress.points(), count);
            let points = problem.keep_feasible(walked);
            (points.clone(), progress.extend(points))
        }
        _ => (Vec::new(), progress),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A region one millionth of its box: the probe misses, brute force
    /// lands a seed, the walker delivers from it.
    fn one_in_a_million() -> ConstraintSystem {
        ConstraintSystem::new(
            vec![InputVariable::new("x1", 0.0, 1.0)],
            vec![crate::parse("x1 > 0.999999").expect("fixture should parse")],
        )
        .expect("the fixture binds")
    }

    const SEED: u64 = 0x50_50_1E_5E_ED;

    #[pollster::test]
    async fn brute_force_seeds_the_walker_when_the_probe_is_empty() {
        let verdict = ConstraintSolver::new()
            .with_rng(Xoshiro256PlusPlus::seed_from_u64(SEED))
            .with_strategies(vec![Strategy::BruteSquad, Strategy::HitAndRun])
            .with_threads(2)
            .solve(one_in_a_million())
            .await
            .expect("nothing should go wrong");

        let Satisfiability::Satisfied { mut samples } = verdict else {
            panic!("brute force should have found the region: {verdict:?}");
        };
        let delivered = samples.take(10);
        assert_eq!(delivered.ncols(), 10);
        for column in 0..10 {
            assert!(
                delivered[(0, column)] > 0.999_999,
                "{}",
                delivered[(0, column)]
            );
        }
    }

    /// The order of escalation: probe, solver, brute force. With no budget at
    /// all a region Z3 can express is still found, because Z3 goes first;
    /// a region Z3 answers `unknown` on — a transcendental — is found anyway,
    /// because what it cannot decide is handed to brute force.
    #[pollster::test]
    async fn the_solver_goes_first_and_brute_force_takes_what_it_cannot_decide() {
        let by_solver = ConstraintSolver::new()
            .with_rng(Xoshiro256PlusPlus::seed_from_u64(SEED))
            .with_proposal_budget(0)
            .solve(one_in_a_million())
            .await
            .expect("nothing should go wrong");
        assert!(
            matches!(by_solver, Satisfiability::Satisfied { .. }),
            "Z3 should have seeded the region with no brute force at all: {by_solver:?}"
        );

        // `sin` is increasing on `[0, 1]`, so this is `x1 > 0.99999` written
        // so that no solver can be asked about it: one in a hundred thousand,
        // ten expected hits in the budget below.
        let transcendental = ConstraintSystem::new(
            vec![InputVariable::new("x1", 0.0, 1.0)],
            vec![crate::parse("sin(x1) > sin(0.99999)").expect("fixture should parse")],
        )
        .expect("the fixture binds");
        let by_brute_force = ConstraintSolver::new()
            .with_rng(Xoshiro256PlusPlus::seed_from_u64(SEED))
            .with_proposal_budget(1_000_000)
            .with_threads(2)
            .solve(transcendental)
            .await
            .expect("nothing should go wrong");
        let Satisfiability::Satisfied { mut samples } = by_brute_force else {
            panic!("brute force should have taken over from Z3's `unknown`: {by_brute_force:?}");
        };
        let delivered = samples.take(5);
        assert_eq!(delivered.ncols(), 5);
        for column in 0..5 {
            assert!(
                delivered[(0, column)] > 0.99999,
                "{}",
                delivered[(0, column)]
            );
        }
    }

    /// The solver limit reaches Z3 through the builder: an instance Z3 would
    /// grind on comes back `NotFound` promptly, with no brute force to mask it.
    #[pollster::test]
    async fn the_solver_limit_bounds_the_opening() {
        let hard = ConstraintSystem::new(
            vec![
                InputVariable::new("x", 0.0, 100.0),
                InputVariable::new("y", 0.0, 100.0),
                InputVariable::new("z", 0.0, 100.0),
            ],
            [
                "floor(x) * floor(y) == floor(z) * 7 + 3 +/- 0.000000001",
                "x*y*z == 12345.678 +/- 0.000000001",
                "x^2 + y^2 == z^2 + 1 +/- 0.000000001",
            ]
            .iter()
            .map(|s| crate::parse(s).expect("fixture should parse"))
            .collect(),
        )
        .expect("the fixture binds");

        let started = std::time::Instant::now();
        let verdict = ConstraintSolver::new()
            .with_rng(Xoshiro256PlusPlus::seed_from_u64(SEED))
            .with_solver_limit(30_000)
            .with_proposal_budget(0)
            .solve(hard)
            .await
            .expect("nothing should go wrong");
        let took = started.elapsed();
        assert!(
            matches!(
                verdict,
                Satisfiability::Unsatisfiable {
                    because: Infeasibility::NotFound { .. }
                }
            ),
            "{verdict:?}"
        );
        assert!(took < std::time::Duration::from_secs(10), "{took:?}");
    }

    /// Pins that the loop is what changed: with no budget the pool behaves as
    /// it did before step 4 and gives up after the probe.
    #[pollster::test]
    async fn a_zero_budget_is_the_old_behaviour() {
        let verdict = ConstraintSolver::new()
            .with_rng(Xoshiro256PlusPlus::seed_from_u64(SEED))
            .with_strategies(vec![Strategy::BruteSquad, Strategy::HitAndRun])
            .with_proposal_budget(0)
            .solve(one_in_a_million())
            .await
            .expect("nothing should go wrong");

        assert!(
            matches!(
                verdict,
                Satisfiability::Unsatisfiable {
                    because: Infeasibility::NotFound { .. }
                }
            ),
            "{verdict:?}"
        );
    }

    /// The caller dropped the `solve` future — here, the receiving end of the
    /// opening channel — while brute force was grinding on an empty region
    /// with an effectively unlimited budget. The worker must notice and
    /// return, not spend the budget.
    #[test]
    fn a_dropped_future_stops_the_search() {
        let system = ConstraintSystem::new(
            vec![InputVariable::new("x1", 0.0, 1.0)],
            vec![crate::parse("x1 > 2").expect("fixture should parse")],
        )
        .expect("the fixture binds");
        let problem = Problem::new(system, SmtLogic::default());
        let ladder = Ladder::new(
            &problem,
            Xoshiro256PlusPlus::seed_from_u64(SEED),
            &[Strategy::BruteSquad, Strategy::HitAndRun],
            Budgets {
                proposals: u64::MAX,
                threads: 2,
                ..Budgets::default()
            },
        );

        let (send_opening, opening) = oneshot::channel();
        let (send_batch, _batches) = mpsc::sync_channel(CHANNEL_CAPACITY);
        let stop = AtomicBool::new(false);
        drop(opening);

        let started = std::time::Instant::now();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                serve(
                    &problem,
                    ladder,
                    Vec::new(),
                    send_opening,
                    &send_batch,
                    &stop,
                )
            });
        });
        let took = started.elapsed();
        assert!(
            took < std::time::Duration::from_secs(2),
            "the worker ran {took:?} after its caller was gone"
        );
    }
}
