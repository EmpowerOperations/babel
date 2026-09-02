# Babel — todo

The Kotlin → Rust port is done: every construct in the grammar translates and
evaluates, and babel's own suite is green. sojourn-CVG has moved in as
`crate::cvg`: contracts, ported fixtures, an adaptive sampler, a hit-and-run
walker, and distribution oracles with critical values behind them.

**Part one is the roadmap** — one line per item, done and outstanding.
**[Part two](#the-reasoning) is the reasoning** behind it: the measurements, the
things that went wrong, and the decisions that are not obvious from the code.

The through-line: **the measure that matters is `Document::untranslated`** — the
constraints the emitter cannot write down, which the pool must then attack blind.
Every wave is ordered so each item shrinks the input to the next.

---

## Done

### Wave 1 — the encodings that were always available

- [x] **`floor`, `ceil` and `%`.** `(to_real (to_int x))` is floor with the right
      sign; `ceil` is its reflection; `%` is `a - b*trunc(a/b)`, a *remainder*
      and not a modulo, because babel follows Java in taking the dividend's sign.
      Cost one widened logic line, `QF_NRA` → `QF_NIRA`, measured free.
      Closed `cvg_pools::modulo`.
- [x] **`BinaryOp::Mod` → `BinaryOp::Rem`.** The names were lying: `-7 % 3` is
      `-1`, a modulo would answer `2`.
- [x] **A `Script` of lines, not a string.** A missing `\n` between `(…)` forms
      is harmless; a missing one after a `;` comment eats the next command, and
      quietly. Made unrepresentable, as `Sexp` did for parentheses.
- [x] **`SmtLogic`**, via `ConstraintSolver::with_logic`, defaulting from
      `BABEL_SMT_LOGIC`, defaulting to `QF_NIRA`.
- [x] **Model values are rationals.** Z3 answers `2.5` with `(/ 5.0 2.0)`, so
      `as_rational` is the ordinary path. Its fallback was truncating: `approx`
      takes decimal *places*, so 1.01e-23 read back as `0.0`.
- [x] **`rewrite::fold_constants`.** Every constant subtree becomes a literal,
      using the evaluator's own `apply` so values cannot change.
- [x] **`ast::const_eval` deleted, `static_range` simplified.** The payoff of
      folding was not the folding — it was that "is this constant?" stopped
      being a question anywhere else.
- [x] **`rewrite::invert_monotone`.** Ten monotone functions. `2 < ln(x1)`
      reaches the solver as `x1 > e^2`, `20 > 2^x5` as `x5 < log2(20)`.

### Wave 2 — nothing non-finite travels, and powers are multiplication

- [x] **Non-finite is an error, at both phases.** `NonFiniteConstant` at compile
      time for what is provable, `NonFiniteValue` at runtime for the rest —
      checked on every node, so the error names the innermost subexpression.
- [x] **The domain guards became the textbook ones.** Refusing `-inf` is what
      let `ln`/`log10` move from `u >= 0` to `u > 0`. `sqrt` keeps its inclusive
      floor, and the asymmetry is now principled rather than incidental.
- [x] **`rewrite::expand_powers`.** `x ^ n` for constant whole `n` becomes
      repeated multiplication — for consistency first, speed second: the emitter
      already expanded them, so the solver reasoned about `(* x x x)` while the
      pool filtered with `powf`.
- [x] **`emit::power` deleted**, and a latent bug with it: it rendered a
      negative exponent as `(/ 1.0 …)` with no divisor guard.

      Measured against the wave-1 baseline, points/ms bound. **Seven runs, not
      one** — the first reading said +32% on `small (jvm)` and that does not
      replicate. Median and worst case both matter here, because the spread is
      about 30%:

      | expression | baseline | median of 7 | worst run | verdict |
      |---|---|---|---|---|
      | `x1 + x2` | 18584 | 19583 | -8% | drift |
      | `x1 + x2 > 20 - x3^2` | 9164 | 11686 (+28%) | +1% | real, magnitude uncertain |
      | `sin(x1)*cos(x2)+sqrt(abs(x3))` | 9360 | 10156 (+9%) | -3% | drift — no `^` in it |
      | deep arithmetic | 3546 | 3523 | -14% | drift |
      | `sum(1, 200, i -> var[i]^2 - 3.0)` | 77.6 | **125.5 (+62%)** | **+32%** | **real** |

      `transcendental` contains no `^` and still moved +9%, so roughly that much
      of every figure is machine drift rather than code. Subtracting it, only
      the 200-variable case is unambiguous — which fits: it does two hundred
      squarings per evaluation where `small` does one, and `powf(x, 2.0)` was
      never expensive enough for a single call to show.

### Restructure — one AST, two backends

- [x] **`frontend` / `eval` / `cvg`.** `gen` was considered and is a reserved
      keyword in edition 2024. The front end is everything meaning-preserving;
      the two backends lower it their own ways and neither sees the other's
      lowering.
- [x] **Renames.** `Expression` → `Ast`, `Bound<'a>` → an owned
      `CompiledExpression`, `ConstraintPool` → `FeasibleSamples`, `Solution` →
      `Satisfiability`, `Generator` → `Search`, `is_boolean_expression` →
      `is_constraint`, `compile` → `parse`.
- [x] **`compile` is a free function**, `eval::compile(&Ast, &Schema)`. A tree
      that knows how to compile itself is not a data type.
- [x] **Batch evaluation over faer.** `CompiledExpression::eval(MatRef) -> Col`,
      one column per sample, one row per schema variable — which is the shape
      `cvg` produces, so a batch goes back in with no transpose. The scalar
      `evaluate` is deleted.
- [x] **`ConstraintSystem`**, validated at construction, replacing the two
      parallel slices. That is what leaves `solve`'s `Result` about the search.
- [x] **`Satisfiability` is two arms**, not three. Z3's `unknown` no longer
      surfaces: one that yielded a point is `Satisfied`, one that yielded nothing
      is `Unsatisfiable { NotFound }`. `Infeasibility` keeps `Proved` and
      `NotFound` apart because they are different sentences to a user.
- [x] **`FeasibleSamples`** with `take` / `try_take` / `available` /
      `is_exhausted` / `close`, returning `Mat<f64>`.

---

## TODO

### Wave 3 — the hard part, scoped by what is left

- [x] **Measured the residue.** `emit::tests::residue_what_the_emitter_still_cannot_express`
      — a ratchet, so it fails when the set moves. **26 of 32 corpus shapes
      translate; 6 do not, and every one of them is `sin`.** The measurement
      derives the *why* rather than declaring it: collect the operators in every
      constraint, subtract those appearing in something translatable, and what is
      left cannot be expressible.
      It also turned up a row missing from the inversion table — `log(a, u)` for
      a constant base, which is `a^u`'s inverse and should have gone in beside
      it. Added; `3 > log(2, x)` now reaches the solver as `x < 8`.
- [x] **`log(base, u)` in the inversion table.** The two halves of one
      relationship: each is the other's inverse, both monotone in `u`, both
      reversing below a base of one.
- [ ] **Causalization — the matching third only.** The measurement says the
      full Modelica pipeline is over-specified for what is left. Of the six:

      | | constraints | needs |
      |---|---|---|
      | driven | `y == sin(x)`, `y > sin(theta)`, `y < sin(x*pi)`, `y > 1.1*sin(x*pi-0.5)` | bipartite matching |
      | periodic set | `sin(x1) <= 0` | interval decomposition on a bounded domain |
      | implicit | `x1 > sin(ln(cos(2.1^x1)))` | nothing; correctly reported |

      **There are no strongly-connected components anywhere in the residue** —
      so no BLT, no tearing, no Newton. Those exist to break algebraic loops and
      there are none. Build the matching and stop.
      **Caveat on all of this:** the corpus is sojourn-CVG's *test* fixtures, not
      customer expressions. If real formulations contain implicit trigonometry —
      `sin(x) == x/2` shapes — the SCC case returns and so does the tearing work.
      Worth asking Garry before committing to skip it.
- [ ] **A design of experiments over driven arguments.** Latin hypercube or
      Sobol over the argument expression's variables. A correctness issue, not a
      tuning one: pick one `x` and every point in the pool shares a `y`, which
      is a constant rather than a sample.
- [x] **Repair a near-miss point instead of discarding it.** Done in its minimal
      form: `cvg::repaired`, a bounded coordinate sweep run on a solver's seed.
      It landed because it had to. Emitting `a == b +/- t` as two bounds rather
      than `max(...) <= 0` changed which *edge* Z3 picks, and `cvg_pools::constants`
      went red — Z3 answers with the boundary exactly, because a boundary is the
      simplest solution there is, and `f64` then rounds it a hair outside. The
      previous green was luck: the old encoding happened to make Z3 pick the edge
      where the rounding went the other way.
      What is still owed is the gradient version. This steps coordinate-wise by
      ulps, which is right for a boundary witness and nothing else; it cannot
      rescue a point that is genuinely infeasible, and a test pins that it does
      not try.
- [ ] **The gradient version of repair.** When the solver
      nominates a point that babel's own `evaluate` then rejects, the pool bins
      it — throwing away the expensive half of the work over a difference the
      solver could not have seen, since it reasons in exact reals and we filter
      in `f64`. Perturb toward feasibility instead. The residual is signed and
      graded, so it says how badly and a finite-difference gradient says which
      way; isotropic jitter is the wrong tool in high dimensions, for the same
      concentration-of-measure reason the walker already has to work around.
      Watch the distribution: repairing a *seed* is free, repairing an *emitted*
      point skews the marginals, and `cvg_benchmarks` is what would catch it.

- [ ] **`take` should be `async`.** It blocks today, because the producer is a
      thread and the channel is `std::sync::mpsc` — a real park, not a spin, but
      not awaitable. Doing it honestly means an async-aware channel, which is a
      change to the worker rather than to this signature.
- [ ] **`impl Stream for FeasibleSamples`**, if and only if a call site shows it
      reading better than the inherent methods.
- [ ] **A typed `SolveError`.** `solve` still returns `anyhow::Result`, which is
      right for a binary and loose for a library.
- [ ] **A pipeline to hang these on.** Causalization splits one problem in two
      and the DOE turns one into `n`; both are `SolverProposal -> Vec<_>`, so a
      `flat_map` chain is the shape. Deliberately not designed yet — the trigger
      is the *second* fan-out pass.

### Parallel — the evaluator, not the solver

- [x] **Lower to a tape.** Done 2026-09-02 as step 1 of `i-am-the-brute-squad.md`:
      a three-address tape with register allocation, a tiled executor for
      straight-line tapes and a per-lane one for tapes with run-time loops.
      The tree-walker did *not* stay as the oracle, despite the plan below: the
      tape was held to it on a few thousand random rows, then it was deleted.
      The spec is the corpus, `runtime_errors.rs` and `special_values.rs`.
      Measurements in the brute-squad plan.
- [ ] **A BLAS evaluator over `faer` matrices**, taking a `MatRef` and never
      handing out a `MatMut`: no mutation of a matrix not allocated in the same
      lexical scope. Downstream of the tape, since it needs the flat form.
- [ ] **A fast sine for the evaluator.** Remez/minimax coefficients,
      Cody–Waite reduction. Objective functions keep the exact path, and nothing
      here is ever emitted to a solver.

### Standing

- [ ] **Equality constraints.** One syntax, `a == b +/- t`, covering at least six
      structurally different things — pinned, driven, driven-after-rearrangement,
      multi-valued, under-determined, implicit. The tolerance belongs to the
      *strategy*, not the constraint: the solver wants the surface, the walker
      wants the band. But not simply "drop it" — drop it on a fully determined
      system and the feasible set is one point, and *keeping* it does not make a
      high-dimensional band samplable either: `(2t)^200` is out of reach even at
      `t = 0.45`. Both roads lead to an SMT beachhead with the walker building
      out from it. Taxonomy, consumers, traps and the tests expected to move are
      below, under [equality constraints](#equality-constraints).
- [ ] **Restricting `a ^ b` to an integer `b`.** Needs Garry. Less urgent than
      it was — `expand_powers` covers constant exponents and `invert_monotone`
      rescues `2^x5` before any restriction would see it.
- [ ] **Relevance-filtered parameters in a runtime error.** Planned as wave 2's
      optional tail and **not done**. `RuntimeProblem::parameters` carries the
      whole row; narrowing it to the variables the failing subexpression
      actually reads means walking the program for the node matching
      `fault.span` and collecting its `Kind::Global` ids — error path only, free
      on the happy path.
- [ ] **Locals in a runtime error.** `RuntimeProblem::locals` is always empty;
      populating it needs a slot-to-name table the AST discards. Matters less
      now that the non-finite check is eager and the span is exact.
- [ ] **Capability metadata per backend.** The cheap half — `emit` taking a
      capability set rather than an implicit one — when a second backend exists.
- [ ] **`cvg_benchmarks::p118` is red.** Polytope mixing, KS 0.114 against 0.096.
- [ ] **The JVM tree does not compile** on this branch, and the gap has widened.
      `db9add8` commented out four `locals [...]` declarations `rewriters.kt`
      needs; the grammar has since gained `scalarBlock` / `scalarReturnStatement`
      and pointed `lambdaExpr` at the first, which the Kotlin front end knows
      nothing about. Restoring it now means teaching `rewriters.kt` the new rules
      as well. Deleting the tree is looking like the honest answer — but capture
      the `jvm-11-map` ledger rows' provenance first, since they cannot be
      reproduced without it.
- [ ] **The benchmark cannot resolve small changes.** Run-to-run spread is about
      15% on this machine; anything under that is not a reading. Also, no
      benchmark expression contains a constant subexpression, so folding is
      invisible to it.


---

# The reasoning

Why the items above are what they are. Everything here is a record of
something measured or something that went wrong, kept because neither is
recoverable from the code.

## The waves

### The restructure: one AST, two backends

Not a wave — the thing the waves kept running into. `cvg` needs to *read*
structure and `eval` needs to *destroy* it, and both were being served by one
pipeline that made the second's choices for the first. So the crate now says out
loud what it always was: a front end producing an `Ast`, and two backends
consuming it.

**`gen` is a reserved keyword in edition 2024**, verified rather than assumed —
`mod gen` does not compile. Hence `frontend` / `eval` / `cvg`.

**Evaluation is batch-only.** `CompiledExpression::eval(MatRef) -> Col<f64>`,
one column per sample, one row per schema variable. That is the shape `cvg`
already produces, so a generated batch is directly an input matrix with no
transpose, and the scalar `evaluate` is gone — it had no consumer that a batch
of one does not serve.

Except one, and finding it was the surprise. **The walker is a genuine
one-at-a-time consumer**: its shrinkage loop cannot propose a candidate until it
has judged the last, so it is sequential by nature. Wrapping each point in a
one-column matrix cost *five times the evaluation itself* — `p118` ran 32s
against 6s, and `top_corner_200d` blew through the 120s nextest timeout. The fix
is a crate-private `eval_row`, which is the body of `eval`'s own loop rather
than a second implementation. The public API stays batch-only; the premise that
nothing needs a row was simply wrong, and it was wrong inside this crate.

**`Satisfiability` is two arms.** Z3's `sat`/`unsat`/`unknown` no longer
surfaces: an `unknown` that still yielded a point is `Satisfied` like any other,
and one that yielded nothing is `Unsatisfiable { NotFound }`. The invariant that
makes two arms sufficient is that **`Satisfied` means at least one sample is
already in hand** — which required a real fix in the worker, because a solver
seed can fail the pool's own filter and leave nothing behind.

`Infeasibility` keeps `Proved` and `NotFound` apart rather than carrying a
`proved: bool`, because they are different sentences to whoever reads the
result: *"your constraints conflict, here are the three involved"* sends someone
to rewrite a formulation, *"we found nothing"* sends them to widen a tolerance.
A flag invites code that ignores it and says the first when it means the second.

Batching measured, on `BATOU`, points/ms:

| expression | batch of 1 | batch of 256 | gain |
|---|---|---|---|
| `x1 + x2` | 8508 | **49463** | 5.8x |
| `x1 + x2 > 20 - x3^2` | 6657 | 17700 | 2.7x |
| `sin*cos+sqrt*abs` | 6225 | 15021 | 2.4x |
| deep arithmetic | 3029 | 4325 | 1.4x |
| `sum(1, 200, i -> var[i]^2 - 3.0)` | 125 | 137 | 1.1x |

Exactly the expected shape: the win is largest where per-call overhead dominates
and nearly absent where the arithmetic does. The trivial case at 49463 is more
than double the best the old scalar path ever managed, and batch-of-one is
*worse* than the old path was — two buffers and a `Col` per call against one
`Vec`. That is the cost of the degenerate batch, and it is the number a tape
should attack.



## Repairing a point rather than discarding it

Geoff's, and it is the other end of a problem wave 1 already touched from one
side. When a solver nominates a point and babel's own `evaluate` then rejects
it, the pool bins the point — and the pool is right to distrust it, because the
solver reasons in **exact real arithmetic** while every filter downstream runs in
`f64`. A point sitting on a constraint boundary can be feasible in the model and
infeasible in the evaluation, by a difference the solver had no way to see. Wave
1's ulp-widening on inverted bounds was the same divergence approached from the
emitter's side.

Discarding is the expensive answer. A solver call is seconds; an evaluation is
under a microsecond. Throwing away the former over the latter, when the miss is
microscopic, is the wrong trade — the point should be **repaired**.

Two things make repair tractable here, and one makes it delicate.

*The residual is graded, not boolean.* That is the whole reason for the `<= 0`
convention: a violated constraint reports how badly it was violated. So a
near-miss already carries its own error signal, and a finite-difference gradient
over `d + 1` evaluations says which way to move — still nothing against the
solver call that produced the point.

*Feasible regions near a nominated point are locally simple.* The solver has
already done the global work. What is left is projection onto a set the point is
almost in.

*But "small" in high dimensions is not small.* Geoff's own caveat, and it is the
concentration-of-measure effect the walker already has to work around: in `d`
dimensions a random direction is nearly orthogonal to the gradient, so isotropic
jitter mostly moves sideways and the step you need grows with `d`. Repair has to
be gradient-directed, not a random walk — the same lesson that made the walker
add axis moves at 200 dimensions.

**The risk is distributional.** A repaired point is drawn from a different
distribution than an unrepaired one: repair pushes points onto the feasible
boundary, and a boundary is exactly where a uniform sample should be *rare*.
Repairing a **seed** is free, because seeds are already not uniform and the
walker's job is to forget where it started. Repairing an **emitted** point would
bias the marginals, and `cvg_benchmarks` is the instrument that would say so —
which makes this one of the few items on the list that arrives with its own
test already written.

## Where CVG stands

`solve` → `Solution` → `ConstraintPool::generate` is settled. Two live strategies, divided by what
each can promise:

- **`AdaptiveSampling` seeds.** It narrows its proposal box toward what it has found, which is what
  makes a narrow region reachable — and which also means it can exclude a part of the region it has
  not seen. Biased, effective, and its points never leave the pool.
- **`HitAndRun` emits.** Converges to the uniform distribution over the region, so what a caller
  receives is governed by the strategy with a guarantee rather than the one with a heuristic.

Which of the three runs is decided per pool by one probing round — see `Route`. Plain sampling is
asked first because where it works it is not a fallback but the *best* option: unbiased by
construction, no burn-in, and none of a Markov chain's trouble with regions in several pieces.

A third strategy sits behind those two: when sampling finds nothing, **Z3** is asked for a first
point, and only then can `Solution::Unsatisfiable` be produced.

**156 of 158 tests pass.**

| red | why |
|---|---|
| `cvg_pools::modulo` | SMT-LIB's `mod` is integer-only and babel's `%` follows Java's sign-of-dividend rule, so the emitter cannot express it. Reported as `Solution::Unknown`, never silently dropped. |
| `p118` | two runs disagree by KS 0.114 against a 0.096 threshold — see below. Unrelated to the solver. |

## What the oracles found

The point of the fairness work was to make "the distribution is fine" a checkable claim rather than
an assumption. It is now checkable, and it caught things. Three open findings:

- [x] **The walker's disconnected-region limit, measured and then routed around.**
      `parabolic_roots_narrowing` under the walker returned 2000 points worth **87 independent ones
      — 4.4% efficiency**: a chain cannot cross a gap it cannot step across, so each of the eight
      chains lived in one band for its whole life and the split between bands was binomial(8, ½),
      50% ± 18%. The band *proportions* were noise.
      Fixed by the easy path, which hands that problem to plain sampling and never starts a chain.
      The limit itself is unchanged and still applies wherever sampling cannot reach — it is one of
      the reasons to want a solver — but it is now measured rather than assumed, and routed around
      where routing around it is possible.

- [ ] **P118 and ToughSingleVar disagree marginally.** KS 0.0980 against a 0.0936 threshold, and
      0.0950 against 0.0908 — both about 4.5% over, consistently, across several configurations.
      Small but reproducible. Either a residual bias or a significance level slightly too tight for
      ~200 KS tests a suite; worth deciding which before loosening anything. P118's polytope is a
      long thin tube (the ±7 couplings chain the variables), which is the classic slow case for
      hit-and-run — **preconditioning** (rescaling by the covariance of found points, i.e. rounding
      the body) is the standard fix and would likely settle it.

- [ ] **Effective sample size is estimated, not exact.** `effective_sample_size` uses Sokal's
      automatic windowing over the autocorrelation function. It has to, because emission is
      round-robin across chains and so the correlation sits at the chain count rather than at lag
      one — the conventional "truncate at the first non-positive lag" rule stops at lag one and
      reports full independence for a sequence that has none. It did exactly that, and briefly made
      a correlated sample look like grounds for suspecting the walker. A per-chain estimate would be
      exact, but the test cannot see chain boundaries; exposing them is a public-API question.

## Not ported from the Kotlin

A pass over `sojourn-CVG` for anything missed. Most of it is genuinely skippable; two items are not.

- [x] **The easy path, from `sojourn.kt`.** Done. Turned `tough_single_var` and
      `parabolic_roots_narrowing` green outright, and `Strategy::UniformSampling` is now first in
      `DEFAULT_STRATEGIES` rather than test-only. Original note follows.

      **The easy path, from `sojourn.kt`.** `makeSampleAgent` runs one round of the *fair*
      (non-adaptive) sampler first, and if it lands more than `EASY_PATH_THRESHOLD_FACTOR` (0.1) of
      the target it stays on plain sampling forever and never engages the walker or the solver.
      Worth porting for three reasons: plain sampling is unbiased by construction, it is far
      cheaper than a burn-in, and it makes the disconnected-region problem above disappear on every
      region it can reach — including `parabolic_roots_narrowing`, whose 4.4% would become 100%.
      Note also that `RandomSamplingPool.create` — non-adaptive — was a *production* pool over
      there, not just an oracle. `Strategy::UniformSampling` should probably be in the default
      strategy list rather than test-only.

- [x] **Eight `Z3SolvingPoolFixture` cases never ported.** Done — all sixteen are now present,
      four of them green (`logarithms`, `modulo_with_a_symbolic_divisor`,
      `equality_with_a_loose_tolerance`, `sine_below_zero`). Porting them also turned up two
      *mis-ports* from V0: `ceiling_and_floor` had been written as a two-variable
      equality-with-tolerance when the fixture's case is four variables and two inequalities, and
      `absolute_value` as one equation over two variables when the fixture pins three variables to
      three magnitudes. The first had been sitting in the red column looking like the solver's
      problem; it passes. `a_simple_inequality` is ours and appears nowhere in the fixture, now
      labelled as such. Original note follows.

      **Eight `Z3SolvingPoolFixture` cases never ported.** `cvg_pools` has nine of the sixteen.
      Missing: `logarithms` (`2 < ln(x1)`), `mod` (`x1 % 3.0 >= 2`), `mod with symbolic divisor`
      (`3 > 10 % x1`), `constants` (`x1 == pi +/- 0.001`), `sgn`, `vars`
      (`1.5 == var[1] + var[2] +/- 0.001`), `equality` (`x1 == x2 +/- 0.1`), `sin`
      (`sin(x1) <= 0`).
      These are not solver tests — they are coverage of *babel features* under the pool, and
      several are inequality-shaped and would go green today: `2 < ln(x1)` admits 26% of its range,
      `sin(x1) <= 0` admits 50%, `x1 == x2 +/- 0.1` is a diagonal band. `vars` is the only exercise
      of `var[i]` under CVG anywhere. Cheap to add and the largest single gap in the port.

- [x] **`Solution::Unsatisfiable` is reachable.** It has a producer, and two tests: a
      contradictory pair returns it naming both constraints, and a satisfiable pair does not return
      it at all — because a verdict that never stays quiet means nothing.

- [ ] **`UNSAT` as a babel diagnostic, not just a sampling verdict.** Separate from everything
      above, and probably the higher-value half of wiring up a solver. A user who writes two
      constraints that cannot both hold currently gets silence — the pool searches, finds nothing,
      and reports having found nothing, which looks identical to a region that is merely hard. A
      solver's `check()` distinguishes "no point exists" from "we did not find one", and that is a
      *compile-time* answer: report it through `ProblemKind` alongside the syntax errors, where the
      user is already looking.
      **Most of the machinery now exists**: the emitter names its assertions, `Z3Backend` reads
      unsat cores back, and `Solution::Unsatisfiable` carries the conflicting constraints. What is
      missing is only the *entry point* — `compile()` sees one expression and no input variables, so
      reporting this through `ProblemKind` needs somewhere new for a caller to hand over a whole
      constraint set. Notably it needs no pool, no sampling and no strategy selection. It also
      applies to constraint sets that sample perfectly well, which is the case the
      "solver only when sampling is hard" framing misses entirely. Nothing in sojourn-CVG does
      this today; it is new work this repo is now positioned for.

Deliberately skipped, with reasons:

- **`EuclideanNormSanityFixture`** tests `findDispersion`, which was dropped for
  Kolmogorov-Smirnov. Its two cases do confirm the definition (mean Euclidean distance from the
  centroid: 2/3 for {0, 1, -1}, sqrt(2) for the unit square), which is what identified the JVM's
  unasserted `dispersion` numbers as mean absolute deviation. Nothing left to port.
- **`TransposeFixture`** tests a list-of-lists transpose helper. `Point` is positional, so there is
  nothing to transpose — though this may come back with the structure-of-arrays tape.
- **`IntegrationFixture`** is a single `TODO()` against a CLI `main` we do not have.
- **`IntegrationTests`** asserts `assertThat(points).isEqualTo(20_000)` on a `List` — comparing a
  list to an int, so it can never pass — and follows it with `assertThat(points.all { ... })`,
  which has no terminal assertion and therefore asserts nothing. The *intent* was throughput on
  P118; that belongs with the benchmark work, not here.
- **`LanguageFixture`** is JVM/Kotlin sanity (`1E20.toInt()`, `TreePVector`, operator precedence)
  plus decimal-to-rational parsing. Only the last matters, and it belongs with the SMT emitter.

Worth knowing while reading that code: **`sojourn.kt` does not compile.** There is a bare `fail;`
at line 286, mid-way through the pool-balancing logic, next to a comment reading "ok, running with
'x1 < 0.0001^(x2+1)' scares me" and a note that the improver offers much higher variance but the
adaptive sampler keeps getting picked. So the round-robin balancing — allocating each pool's budget
by measured throughput, and avoiding pools whose dispersion is poor — was work in progress that was
never finished or run. Treat it as a design sketch, not as behaviour to reproduce.

## Watch these

- **The rewrites are accumulating ad-hoc, and there is no pipeline to hang them on.** Wave 1 #1 was
  small enough to bolt straight onto `emit`. The next few are not. Monotone inversion rewrites a
  constraint, causalization *splits* one problem into a driven part and a solved part, and a DOE
  over a driven argument turns one problem into `n`. That last one is the tell: these are not
  transformations of a document, they are transformations of a **set of problems**, and doing them
  in sequence inside `emit` would mean control flow nobody can follow.
  The shape Geoff sketched — a `Vec<SolverProposal>`, each pass being
  `SolverProposal -> Vec<SolverProposal>`, the whole pipeline a `flat_map` chain — is the right
  instinct and worth writing down before it is forgotten. Causalization becomes a fan-out, the DOE
  becomes a fan-out, and inversion is the degenerate one-to-one case. It is combinatoric by nature,
  which is a reason to keep each pass cheap and to cap the breadth, not a reason to avoid the shape.
  **Deliberately not designing this yet.** There is one pass today and a design for one pass is a
  guess about the second. The trigger to build it is the *second* fan-out pass — that is when the
  ad-hoc version starts costing more than the abstraction, and step 7 ("measure the residue") is
  where we will know what the passes actually have to be.

- **Read-ahead is not free on expensive problems.** `BATCH_SIZE * CHANNEL_CAPACITY` points get
  produced on spec. Raising either costs real time on `top_corner_200d` and nothing anywhere else,
  so measure that test specifically before touching them — 64 points of read-ahead is about three
  seconds there.
- **A broken exhaustion signal is a hang, not a failure.** Which is why
  `a_pool_that_can_never_deliver_reports_exhausted_rather_than_blocking` was written before anything
  else in that increment, and why the nextest timeout sits behind it.
- **`Drop` must drain before it joins.** `Drop` runs before the struct's fields do, so the receiver
  is still alive while the pool waits on its worker — and a worker parked on a full channel stays
  parked until somebody reads. Draining first is what makes the join terminate rather than
  deadlock; `dropping_a_pool_mid_fill_does_not_deadlock` is the guard.

Green, but not robustly so — worth knowing before treating them as settled.

- **`parabolic_roots_ribbon` passes on about six seed points.** The band at x = 1 is 6.6e-6 wide in
  a box of 10, so a two-million-proposal probe lands roughly two or three points across both bands,
  and the walker's chains then start from whichever of those it drew. It went red before the easy
  path added a second probing round and green after — but nothing guarantees the next RNG seed puts
  a point in the far band. A solver is what makes this deterministic.
- **`signum` sits on the easy-path threshold.** Its feasible fraction is about a thousandth, and a
  hundred-point probe at 100x oversampling lands on the order of ten hits against a threshold of
  ten. It currently routes to sampling and passes. It could route either way.
- **A test restating a library constant went stale, silently.** `cvg_benchmarks` had its own
  `PRODUCTION` strategy list, so when `UniformSampling` joined the defaults the benchmarks spent a
  full run measuring a configuration the product had already left behind — and reported the old
  failures convincingly. `DEFAULT_STRATEGIES` is now `pub` (doc-hidden) and the tests import it.
  Worth remembering the shape of that mistake: the test was not wrong about what it measured, only
  about what it claimed to measure.

## The solver question, settled

Shopped for a replacement to Z3 and came back with Z3. Measured, on this machine:

| babel needs | Z3 | cvc5 |
|---|---|---|
| `sin` `cos` `tan` | parses, instant `unknown` | parses, **20s timeout** |
| `asin` `atan` `sinh` | parse, instant `unknown` | — |
| `ln` `log10` `log2` `exp` | **rejected — no such symbol** | **rejected — no such symbol** |
| `sqrt` `cbrt` | rejected — we encode as `y*y = x, y >= 0` | native, but the encoding already works |
| `^`, constant exponent | **works** — `(^ x 0.5)` on a pinned `x` gives 3.1622776601683795 | positive integers only |
| `^`, variable exponent | **`unknown`** — `2^x5 < 20`, and every log encoded through it | **rejected** |
| `pi` | a symbol, but never a model value — see below | — |
| `e` | **rejected — no such symbol** | — |

**Net capability gain of cvc5 over Z3 for the functions babel has: zero.** It adds `exp` (babel has
no `exp`) and native `sqrt` (already encoded), and loses real-exponent `^`. dReal has the best theory
by a distance but is Unix-only, x86_64-only on Linux, no arm64 anywhere, last release June 2021.

**Logs specifically, since the question came up.** Z3 has no logarithm under any spelling — `ln`,
`log`, `log2`, `log10` and `exp` are all parse errors, and so is the constant `e`. The obvious
workaround is inversion through `^`, writing `ln x = y` as `x = e^y`, and it does not work either:
open, it answers with the trivial origin `y = 0, x = 1`; narrowed to `9 < x < 11` it answers
`unknown`; *pinned* to `x = 10` it still answers `unknown`. **Z3 cannot compute `ln 10`.** The rule
underneath is that Z3's `^` is polynomial in practice — a variable in the base is fine, a variable
in the exponent is not — which is one fact covering `ln`, `log10`, `log(b, x)` and `2^x` at once.

Two traps found while measuring, worth not rediscovering:

- **`(^ -8.0 0.3333)` answers `sat` with `y = 0`.** Z3's `^` is underspecified for a negative base
  with a fractional exponent, so it is free to return anything and does. Sound by its own semantics,
  wrong by babel's `cbrt`. The auxiliary-variable encoding avoids this and should stay.
- **`sat` does not mean every variable has a value.** Ask about `pi`, or about `sin` on a pinned
  argument, and the model comes back missing the irrational entries, because `as_rational` fails and
  `approx(17).parse()` fails after it. Already handled — `escalate_for_seed` falls back to the lower
  bound and lets the pool's own filter judge the point — but the behaviour is deliberate, not
  incidental, and the fallback is why `Sat([])` never becomes a bogus seed.

**Nobody is coming to fix this.** SMT-COMP 2025 ran fifteen quantifier-free divisions and a dozen
quantified ones, and **not one of them involves a transcendental function**. The nonlinear real
division is `QF_NonLinearRealArith` — polynomials. No competition category means no competitive
pressure, which is the whole explanation for why a field this active has left `sin` where it is.
For what it is worth, the division babel does live in was won by **Z3-alpha** (2856 solved), a Z3
fork with learned strategy selection, ahead of **z3siri** (2803, also Z3-derived), **cvc5** (2766)
and **Yices2** (2746). The top two are Z3 wearing a hat. Yices2 is worth knowing about — there is
2026 work extending its MCSAT core to `sin` and `exp` that reportedly beats the field — but it is
a research prototype, not a release, and its licence is GPL-family rather than Z3's MIT.

Worth keeping, though: **cvc5 *can* be linked in-process on Windows/MSVC**, which the crate's
`#[cfg(not(unix))] panic!` appears to deny. That panic guards only the *static build-from-source*
path; the dynamic path has no platform gate. Proven end to end — MinGW-built DLL loaded by an MSVC
Rust binary through cvc5's C API, built, linked and ran. One upstream bug in the way:
`cvc5-sys` canonicalises the include dir, which on Windows yields a `\\?\`-prefixed
extended-length path, and libclang silently ignores such a path as an `-I` search directory — it
opens the main header and then cannot resolve that header's own includes. Five-line fix, worth
sending upstream. If a future cvc5 grows real trigonometry the door is open.

## Equality constraints

The biggest open question in the pool, and the one the driven-variable work runs
straight into. What follows is the shape of the problem, not a design.

### There is one syntax and at least six meanings

Babel admits exactly one equality form. `BabelParser.g4`:

```
booleanExpr : scalarExpr eq scalarExpr plusMinus literal
```

So `a == b +/- t`, and `t` **must be a literal** — there is no bare `a == b`,
and no computed tolerance. `rewrite_booleans` lowers all of it to
`max((b - t) - a, a - (b + t))`, one residual, and by the time anything
downstream looks, an equality is indistinguishable from any other constraint.

That is the problem. One syntax covers at least six structurally different
things, each of which wants different handling:

| | shape | corpus |
|---|---|---|
| **A** | **Pinned to a constant.** A variable equals a literal. No search at all: the dimension is gone. | `x1 == pi +/- 0.001`, `x2 == e +/- 0.001` |
| **B** | **Driven.** One variable alone on one side, appearing nowhere else in the constraint. Evaluate it, never solve for it. | `y == sin(x) +/- 1e-6`, `x1 == sqrt(x2) +/- 1e-4`, `x3 == cbrt(x4) +/- 1e-4`, `x1 == x2^3 +/- 1e-4` |
| **C** | **Driven after rearrangement.** The variable is on both sides but *linearly*, so gathering terms makes it B. | `x2 == x1 + 1/2*x2 - x3/x4 +/- 1e-5` — `x2` appears twice and `0.5*x2 = …` solves it |
| **D** | **Multi-valued.** The equation names a set with several branches. Nothing is driven; a solver may pick a branch and the walker must then stay on it. | `abs(x1) == 1 +/- 0.001` (two roots), `(x + 2) * (x - 1) == 0 +/- w` (two bands) |
| **E** | **Under-determined system.** More variables than equations, so *which* variables are driven is a choice — this is where matching earns its keep. | `1.5 == var[1] + var[2] +/- 0.001` together with `1.5 == var[2] - var[3] +/- 0.001`: two equations, three variables, one degree of freedom left |
| **F** | **Implicit.** The variable is inside and outside a function that cannot be inverted. No rearrangement exists; only iteration. | `sin(x) == x/2 +/- t` — not in the corpus, and Geoff's judgement is that it is rare enough to build against |

A and B are dimension reductions. C is A/B behind one rewrite. D is a branch
choice. E is a matching problem. **F is the only one that needs Newton**, which
is why the full Modelica pipeline is over-specified for what we have.

### The tolerance belongs to the strategy, not to the constraint

`+/-` exists because rejection sampling cannot land on a measure-zero set. But
each consumer wants something different from the same user constraint:

| consumer | wants | why |
|---|---|---|
| the SMT solver | **`a == b`, exactly** | it reasons in exact reals and can hit a measure-zero set. Handing it a band asks for a thicker answer than it needs, and the band is the part it is worst at |
| the walker | **the band** | it needs volume to move in. A surface has none |
| rejection sampling | **the band**, and a generous one | it will never hit anything thinner |
| the pool's final filter | **the band** | it re-checks in `f64`, where exact `==` is satisfied by essentially nothing |

So the solver should be asked for a point *on the surface*, and the walker
should explore the band *around* it. That decomposition is better than either
alone: the solver is good at finding the surface and bad at the band, and the
walker is the reverse.

**The expression we ask a solver to solve is therefore not the one the user
wrote.** That is the sentence this whole section turns on, and it is what makes
the fan-out pipeline the right shape rather than a nicety —
`SolverProposal -> Vec<SolverProposal>`, one user constraint becoming several
proposals, each aimed at a different consumer. Dropping the tolerance is one
such rewriting; causalizing a driven variable is another; splitting D into its
branches is a third.

### Two things that will bite

**A point exactly on the surface in real arithmetic is not on it in `f64`.**
The solver returns a witness for `a == b` and the pool re-checks in `f64`, where
it may miss by an ulp — the same divergence that made wave 1 widen inverted
bounds by one ulp, seen from the other side. It is also exactly the case
Geoff's *repair* item exists for: perturb the near-miss onto the surface rather
than discard it. **Dropping the tolerance for the solver and repairing its
output are one piece of work, not two.**

**A thin band is the walker's hardest case.** Hit-and-run in a slab has almost
every chord tiny, which is what `parabolic_roots_narrowing` and
`parabolic_roots_narrow` already measure. Making the solver's job easier by
handing it a surface makes the walker's job harder by handing it a slab, so the
two have to be assessed together.

### `TopCorner200D` as an equality, which is the case worth thinking about

`top_corner_200d` is 200 *inequalities* — `xi > 10.5` over `10..11`, a corner
that is `0.5^200` of the box, or `6e-61`. Write the same problem as 200
*equalities*, `xi == 10.5 +/- t`, and two things follow.

**Drop the tolerance and the feasible set is a single point.** 200 exact
equalities over 200 variables is fully determined. The solver finds the one
solution immediately — it is trivial for it — and then there is nothing to
sample, because "give me 500 distinct points" has no answer.

**Keep the tolerance and sampling still cannot find it.** The feasible set is a
product of 200 bands, so its volume is `(2t)^200`, and that decays exponentially
in the dimension count no matter how generous `t` is:

| `t` | band per dimension | fraction of the box | samples for one hit |
|---|---|---|---|
| 0.45 | 0.9 | 7.06e-10 | 1.4 billion |
| 0.1 | 0.2 | 1.6e-140 | — |
| 0.01 | 0.02 | underflows `f64` | — |

A band covering **ninety per cent of every dimension** is one hit in 1.4 billion,
and a realistic tolerance does not have a representable fraction. So the
equality twin is not the easy case: **it is reachable only through an SMT
beachhead with the walker building outward from it**, which is the same answer as
the inequality version and for the same reason.

That is the strongest argument for the surface-then-band split rather than a
complication of it. And the geometry is encouraging — the band is an
axis-aligned convex box, exactly what `top_corner_200d` already is, so a walker
that mixes in one should mix in the other. One thing to check rather than
assume: whether anything in `walking.rs` carries an *absolute* scale assumption,
since a `t` of `1e-4` makes the region four orders of magnitude thinner than
anything the walker has been run against.

It is also the missing benchmark. Nothing in the corpus is a high-dimensional
equality band, which is precisely the shape the surface-then-band split would
produce, and precisely where the walker's mixing is least understood.
`top_corner_200d` forced axis moves into the walker; its equality twin is the
natural place to find out what the next such surprise is.

### Which tests are expected to move

Not regressions — this is a semantic change and the churn is the point:

- **`cvg_pools::constants`** (A) — passes today only because `pi` and `e` are
  folded to literals and the band is samplable. Under A it should not be
  searched at all.
- **`cvg_pools::roots`, `power`, `signum`** (B) — driven; they should stop being
  solver questions entirely.
- **`cvg_pools::simple_arithmetic`** (C) — the rearrangement case, and
  the one most likely to expose a wrong matching.
- **`cvg_pools::absolute_value`** (D) — two branches; whatever picks a branch has
  to not bias which one.
- **`cvg_pools::dynamic_variable_lookup`** (E) — two equations over three variables, the
  smallest real matching problem in the corpus.
- **`cvg_benchmarks::parabolic_roots_*`** (D again, with distribution oracles
  attached) — these are the ones that will say whether the surface-then-band
  split preserves uniformity, and they are the reason not to guess.

- [ ] **Integer-only exponents, and one rewrite to go with them.** Restricting `a ^ b` so `b` is
      integer-typed buys three separate things:
      *Evaluator speed* — measured, a single `^2` costs about what `sin`+`cos`+`sqrt`+`abs` costs,
      because `powf` is a libm call (`x1 + x2 > 20 - x3^2` at 9163 pts/ms against 9360).
      *SMT coverage* — `Pow` leaves the `untranslated` list entirely, since every integer power
      expands to multiplication.
      *One uniform rewrite* rather than a special case per backend.
      Rewrite straight to `Kind::Fold` rather than to a `prod` aggregate: `Fold` is already the
      post-unroll n-ary form both the emitter and evaluator consume, and going via `prod` means
      emitting a node whose only purpose is to be rewritten again — a fixed point you would then
      have to prove terminates. Same pass that already unrolls aggregates.

- [ ] **Causalization, for the terms no solver will take.** The trick is to stop asking the solver
      about a transcendental at all: if `y == sin(x) +/- t` and `y` appears nowhere else awkward,
      then `y` is *determined* — choose `x`, evaluate, done. `sin(sin(x))` is fine too, being still a
      function of `x`. What breaks it is a term constraining its own argument, `sin(x) == x/2`,
      where `x` is inside and outside and inversion is unavoidable.
      This is **causalization** in the Modelica sense and the algorithms are mature: bipartite
      **matching** of equations to variables, **BLT decomposition** (Tarjan SCC) for a dependency
      order, and **tearing** to shrink the algebraic loops that remain. Acyclic blocks evaluate;
      strongly-connected blocks need Newton. The solver is then only wanted for the SCCs, and only
      when the question is UNSAT rather than "give me a point".

- [ ] **A piecewise sine belongs to the evaluator, not the emitter.** Table lookup with quadratic
      interpolation is `O(h^3)` error, SIMD-friendly, and much cheaper than libm — good for the
      tape. Choose coefficients by **Remez/minimax**, not Taylor: Taylor is optimal at a point,
      minimax across the interval. Bhaskara I's 7th-century rational approximation is the classic
      no-polynomial reference and is already good to ~0.0016.
      **Do not emit it to a solver.** A hundred pieces is a hundred-way `ite` split, and the modulo
      range reduction drags an integer variable in, pushing QF_NRA to QF_NIRA. Solvers tolerate
      degree far better than disjunction, so this would be worse than the Taylor series the JVM
      tried.

- [ ] **Capability metadata per backend, eventually.** Which rewrites to apply depends on what the
      target can accept, and today that is hardcoded as "refuse the transcendentals". The cheap half
      is worth doing whenever the second backend appears: give `emit` a capability set rather than
      an implicit one, so the refusal becomes data. The expensive half — runtime-pluggable solvers
      with discoverable feature flags — should wait for a second backend to actually exist, since
      a plugin system with one plugin is a guess about the second.

## The plan: getting constraints in front of the solver

The measure that matters is `Document::untranslated` — the constraints the emitter cannot write
down, which the pool must then attack blind. Sorting the CVG corpus by *why* a constraint is
untranslatable is what ordered this list, and the answer was a surprise: **most of it is not a
theory problem.** Four of babel's functions are refused today for reasons that were true when the
comment was written and are not true now.

Ordered by cost-to-value, cheapest first. Each step shrinks the input to the step after it.

- [x] **1 — Write the encodings we already knew how to write. Done.**
      `floor`, `ceil` and `%` were refused with the note that they "want `to_int`, which leaves
      QF_NRA for QF_NIRA". True, and not a reason. All three are now in `prelude()` as
      `babel_floor`, `babel_ceil` and `babel_mod`, with a `babel_trunc` that exists only because
      `%` needs it.
      `(to_real (to_int x))` is floor with the right sign behaviour — `to_int` of -2.7 is -3, floor
      and not truncation — which is what lets `ceil` be `(- (to_real (to_int (- x))))`. Babel's `%`
      follows Java in taking the sign of the *dividend*, so it is `a - b*trunc(a/b)`; floored modulo
      would answer 2 to `-7 % 3` where babel answers -1. Both halves of that are pinned in
      `the_prelude_helpers_mean_what_they_say`, in the form of claims a wrong encoding would fail.
      `%` also pushes the divisor guard that `/` pushes, for the reason `/` does: `a % 0` is NaN and
      the pool bins NaN, but SMT-LIB leaves `/0` underspecified, so a solver may otherwise satisfy
      a constraint *through* the zero and hand back a point that is thrown away.

      **The one cost is the logic line.** `to_int` and `to_real` live in SMT-LIB's `Reals_Ints`
      theory, which `QF_NRA` does not include, and Z3 enforces this — measured, a `QF_NRA` document
      containing `to_int` is rejected outright. So `emit` now writes `QF_NIRA`. That is free: the
      same prelude and the same polynomial constraint solve identically under either name, and a
      clean re-run of the benchmark suite is unchanged (`top_corner_200d` 31.4s against 30.8s
      before, `parabolic_roots_narrow` 7.5s against 8.1s — a 20% spread seen on the first run was
      machine contention, and a test that never calls a solver moved with the rest, which is what
      gave it away).

      `cvg_pools::modulo` is green. The suite is 163/164, with only `cvg_benchmarks::p118` left.

      Four things came out of review, all of them worth more than the feature was:
      *`%` is a remainder, not a modulo*, and `BinaryOp::Mod` is now `BinaryOp::Rem` all the way
      through. The names had been lying: a remainder takes the dividend's sign (`-7 % 3` is -1), a
      modulo takes the divisor's (2). The grammar's `MOD` token and the `%` spelling stay, since
      both are the JVM's.
      *The document is a list of lines, not a string.* `Script` owns the separator and comments are
      their own variant. A missing `
` between two `(…)` forms is harmless — SMT-LIB does not care
      — but a missing one after a `;` **eats the next command**, and quietly: the parse guard only
      fires when *every* assertion is lost, so a document that loses one still looks fine and comes
      back `sat` for a question nobody asked. Same argument as `Sexp` and parentheses: make it
      unrepresentable rather than tested for.
      *The logic is configurable*, via `ConstraintSolver::with_logic`, defaulting from
      `BABEL_SMT_LOGIC`, defaulting to `QF_NIRA`. Explicit beats environment beats built-in, and
      `from_variable` is split out so the precedence is testable without mutating a process-global.
      *Model values are rationals, always.* Z3 answers `2.5` with `(/ 5.0 2.0)`, so `as_rational` is
      the ordinary path rather than a fast case. Where it fails — either half overrunning `i64`, or
      an algebraic irrational like `(root-obj (+ (^ x 2) (- 2)) 2)` — the fallback was wrong:
      `approx`'s argument is decimal *places*, not significant figures, so at `approx_f64`'s 17 a
      value of 1.01e-23 read back as a confident `0.0`. Now 330, which covers every magnitude an
      `f64` can hold.

      **Not** constant folding, which was listed here on a wrong premise. `pi` and `e` never reach
      the emitter as symbols: the lexer has `PI` and `EULERS_E` tokens, `literal` admits them, and
      `front_end::translate_literal` returns `std::f64::consts::PI` at parse time. That is why
      `cvg_pools::constants` is green today. What is left for a folding pass is constant-argument
      *calls* — `sin(2.3)` — which is its own item.

- [x] **2 — Constant folding, and two deletions it paid for. Done.**
      `rewrite::fold_constants` collapses every subtree made only of literals, bottom-up, using the
      evaluator's own `UnaryOp::apply` / `BinaryOp::apply` so a value cannot change — `corpus.rs`
      pins several constant expressions by value and is the guard on that.
      The payoff was not the folding. It was that **"is this constant?" stopped being a question
      anywhere else.** `ast::const_eval` — a recursive evaluator that existed to serve exactly one
      caller — is gone, and `static_range` is now "are both bounds `Kind::Literal`?", because after
      folding a statically known bound *is* one. Bottom-up folding needs no recursion of its own:
      by the time a node is reached its children are literals already.
      Placement is load-bearing in both directions. Before `rewrite_booleans`, because folding a
      wholly-constant comparison afterwards would collapse `3 < 3` onto the strictness epsilon and
      put a three-hundred-digit denormal in the document. Before `invert_monotone`, because
      inversion wants a literal to invert against. That leaves unrolled aggregate terms unfolded —
      an evaluator-throughput opportunity for the tape work, not a coverage one.
      Non-finite constants are **refused**, not folded: `ProblemKind::NonFiniteConstant` at the
      offending span. Leaves as well as folded results, since babel's `FLOAT` admits `1.0e400` and
      refusing `1.0e400 * 1.0` while accepting `1.0e400` would be a rule with a hole in it. One
      consequence worth knowing: `sum(0/0, …)` now reports the division rather than "an illegal
      lower bound", so `IllegalAggregateBound` at compile time is reached only by a *finite* bound
      that is not an index — `sum(1, 20/3, …)`, which now has its own test.
      Note `pi` and `e` were never part of this. They are lexer tokens that
      `front_end::translate_literal` turns into `std::f64::consts::PI` at parse time.
      **`just bench` cannot see this change and never could** — not one of the five benchmark
      expressions contains a constant subexpression, since every literal in them sits next to a
      variable. Numbers are unchanged within the run-to-run spread, which is currently about 15%
      on this machine (`trivial` came back 14270 and 16792 on consecutive runs), so treat any
      single reading below that threshold as noise. If folding is ever to be measured, the suite
      needs a case written for it.

- [x] **3 — Monotone inversion. Done, and it is the cheap half of causalization.**
      `rewrite::invert_monotone` turns `f(u) op c` into `u op' c'` for ten strictly monotone
      functions, computing the bound here in `f64`. `2 < ln(x1)` reaches Z3 as `x1 > e^2` and
      `20 > 2^x5` as `x5 < log2(20)` — both linear, and the solver is never asked about a logarithm
      it does not have. `u` is a whole subtree, so `ln(x1 + x2) > 2` inverts too.
      Three things had to be right, and two of them only showed up under test:

      *Range is a correctness requirement, not an optimisation.* `atan(x) > 2` is unsatisfiable —
      2 is outside `atan`'s range — and inverting through `tan` gives `x > -2.18`, which is almost
      always true. A constant outside the range is left alone. The `bound.is_finite()` check
      afterwards catches the open ends (`atanh(1.0)`) and plain overflow (`exp(710)`).

      *`ln(0)` is negative infinity, not NaN.* So zero satisfies **any** upper bound, and the
      domain guard on `ln`/`log10` has to be `u >= 0` rather than the textbook `u > 0`. Written the
      textbook way first; the round-trip test caught it at exactly one sampled point. A guard
      narrower than the constraint it replaced is the direction that turns a sound `unsat` into a
      wrong one, so this mattered. **Match the evaluator, not the mathematics.**

      *`and` is `max`.* An upper bound does not carry the domain with it — a lower one does, since
      `f`-inverse lands in the domain by definition — so `ln(x) < 2` emits
      `max(x - e^2 + eps, -x + eps) <= 0`. No `Kind::And` was needed; two residuals hold together
      exactly when the larger is `<= 0`, which is how `NearEq` has always lowered. `residual` is
      now factored out of `rewrite_expr` and shared between the two.

      The bound is nudged one ulp outward — `next_down` for a lower, `next_up` for an upper — so
      the region asserted is never *narrower* than the one described. `fl(e^2)` is not `e^2`, and
      on the narrow side `unsat` stops implying anything. The test asserts that one-sidedness
      directly rather than asserting agreement, which is what the design actually claims.

      Unplanned benefit: this makes the wave-2 restriction affordable. Restricting `a ^ b` to an
      integer `b` would make `2^x5` a compile error, and inversion rewrites it away first.

- [ ] **4 — Causalization**, scoped by what 1–3 leave behind rather than by ambition. Details in the
      section above. The corpus residue after three steps is small and instructive: `y == sin(x)`
      and `y > sin(theta)` are feed-forward and yield to matching alone; `sin(x1) <= 0` is a
      periodic set, decomposable into intervals on a bounded domain but not by inversion; and
      `x1 > sin(ln(cos(2.1^x1)))` is implicit and will remain the thing nothing helps with. Build it
      when the residue is measured, not before — the shape of the leftovers should choose the
      algorithm.

- [ ] **5 — A design of experiments over driven arguments.** Once causalization says "choose `x`,
      then `y = sin(x)` follows", *how* `x` gets chosen is a real question and one point is the wrong
      answer. The driven variable is a deterministic function of its argument, so the distribution
      of `y` is entirely decided by the distribution of `x` — pick one `x` and every point in the
      pool shares a `y`, which is not a sample, it is a constant. What is wanted is a set of
      arguments spanning the feasible range, which is a space-filling design: Latin hypercube or
      Sobol over however many variables the argument expression contains, usually one.
      Worth flagging that this is not the sampler's job as currently written. The walker moves in
      the free variables and the driven ones are evaluated afterwards, so the design has to be over
      the *arguments*, and its quality shows up in `cvg_benchmarks` as the marginal of `y`.
      Good news: the oracles already there will measure it without modification.

- [ ] **6 — A fast sine, for the evaluator only.** Unchanged from the section above, with the
      accuracy question answered: **yes, fp32-ULP is comfortably achievable, and rather better.**
      SLEEF ships 1-ULP and 3.5-ULP variants of `sin` at `f64` using Cody-Waite argument reduction
      to `[-pi/2, pi/2]` and a nine-term polynomial; at `f32` four or five terms reach +/-1 ULP. The
      Intel hardware-table memory is real but points somewhere unhelpful: Tang and Story's IA-64
      work is *table-driven reduction followed by a polynomial*, around 0.6 ULP, and modern SIMD
      libms have mostly dropped the table because a multiply is cheaper than a cache miss. The x87
      `FSIN` instruction is the cautionary half of that story — microcoded, and it reduces against a
      66-bit pi, so near multiples of pi it is not approximating `sin x` at all. Intel documented
      its worst-case error as 1 ULP for years; the true figure is about 1.3 quintillion.
      So the tradeoff is not "fast or accurate". It is reduction quality against argument magnitude,
      and for constraint arguments in any sane range a short minimax polynomial is both faster than
      libm and accurate to the last bit or two. Objective functions can keep the exact path
      regardless; nothing here asks them to give up precision.

## Wave 2 — nothing non-finite travels, and powers are multiplication

- [x] **Non-finite is an error now, at both phases.** One rule:
      `ProblemKind::NonFiniteConstant` from `fold_constants` for what is provable
      at compile time, `ProblemKind::NonFiniteValue` from `eval_expr` for the rest.
      The runtime check is on **every node** rather than only the operations that can
      produce a non-finite value. The producing set is nearly everything once infinities
      are in play — `0/0`, `inf - inf`, `0 * inf`, `sqrt` of a negative — so checking
      selectively would save little, and checking uniformly catches two things it would
      miss: a non-finite **input**, which now fails at its own `Kind::Global` instead of
      travelling to wherever it first matters, and an **unwritten frame slot**, since
      `evaluate` fills the frame with NaN as a sentinel and reading one is a
      slot-allocation bug.
      **Infinities are in scope, and that was the whole point.** A NaN-only rule would
      have left the trap that prompted this: `ln(0)` is `-inf`, not NaN. See below.
      *Eager per lane, per instruction.* The tape fuses the finite test into every
      instruction's loop and records each lane's first fault, so it is as exact as
      the walker was and costs nothing on the happy path. Only a GPU sieve gets to
      be coarse — check the output buffer, re-run an offending column through the
      CPU tape for the span — and that split is in `src/README.md` so nobody
      "fixes" a kernel to match. The policy itself is the `is_finite` test on every
      checked instruction in `eval/tile.rs` and `eval/lane.rs`.
      Blast radius was smaller than expected. **The pool needed no change at all** —
      `.ok().is_some_and(|residual| residual <= 0.0)` always treated an `Err` and a NaN
      alike. Both `runtime_errors` bound tests moved from `IllegalAggregateBound` to
      `NonFiniteValue`, which points at `20/x1` rather than at "the upper bound";
      `IllegalAggregateBound` now fires only for a *finite* non-index, and
      `a_finite_bound_that_is_not_an_index_still_reports_as_one` exists so the variant
      does not quietly die.

- [x] **The domain guards became the textbook ones.** This is what including infinities
      bought. `rewrite::monotone` gave `ln` and `log10` a floor of `u >= 0` because
      `ln(0)` was `-inf` and so zero satisfied any upper bound — a workaround for the
      evaluator, not a fact about logarithms. Refusing the infinity made it `u > 0`.
      `sqrt` keeps the inclusive floor, and the asymmetry is now principled: `sqrt(0)`
      is `0.0`, a finite answer, where `ln(0)` is not an answer at all.
      The round-trip test needed tightening rather than relaxing. It skipped a sample
      when either side failed to evaluate, which was right when failure meant NaN; the
      un-inverted form now errors at exactly the points the guard exists to exclude, so
      silently skipping them would have gutted it. It asserts instead that **when the
      un-inverted form errors, the inverted form rejects.**

- [x] **`rewrite::expand_powers`.** `x ^ n` for a constant whole `n`, `|n| <= 64`,
      becomes a `Kind::Fold` of multiplications — `n == 0` a literal `1.0`, a negative
      `n` a reciprocal. Runs *after* unrolling, which is the only reason a loop index can
      be an exponent at all: `sum(1, 3, i -> x^i)` arrives as `x^1, x^2, x^3` only once
      `substitute` has put the literals there.
      **The first argument is consistency, not speed.** `emit::power` already expanded
      constant integer exponents, so the solver reasoned about `(* x x x)` while the pool
      filtered with `powf` — two functions that disagree in the last place. Doing it once,
      in the AST, leaves both reading the same expression.
      The trade is that results move in the last ulp for `n >= 3`: `2.3^3` is
      `12.166999999999998` through `powf` and `12.166999999999996` as `x*x*x`, about one
      case in five. `n == 2` agrees everywhere, libm having special-cased squaring, and
      **`corpus.rs` did not move** — every `^` in it is `^2`, the exact `3 ^ 4`, or
      `sin(21)^3`, which agrees.
      `emit::power` is deleted, `BinaryOp::Pow` is unconditionally untranslated, and a
      latent bug went with it: `power` rendered a negative exponent as `(/ 1.0 (* x x))`
      **without a divisor guard**, so a solver could satisfy `x^-2 < 1` through `x = 0`.
      As an ordinary `Kind::Binary { Div }` it picks the guard up automatically, and
      `a_negative_power_pins_its_base_away_from_zero` pins it.

- [x] **The benchmark measured one thing, and taught a lesson about itself.**
      The first `just bench` after wave 2 read +32% on `small (jvm)` and +35% on
      the 200-variable sum, and that was written up as fact. It was one run.
      Seven runs say otherwise:

      | expression | baseline | median of 7 | worst run | verdict |
      |---|---|---|---|---|
      | `x1 + x2` | 18584 | 19583 | -8% | drift |
      | `x1 + x2 > 20 - x3^2` | 9164 | 11686 (+28%) | +1% | real, magnitude uncertain |
      | `sin(x1)*cos(x2)+sqrt(abs(x3))` | 9360 | 10156 (+9%) | -3% | drift — no `^` in it |
      | deep arithmetic | 3546 | 3523 | -14% | drift |
      | `sum(1, 200, i -> var[i]^2 - 3.0)` | 77.6 | **125.5 (+62%)** | **+32%** | **real** |

      **`transcendental` contains no `^` and still moved +9%**, which is the
      tell: that much of every figure is machine drift, and runs later in a
      sequence come out faster across the board. Subtract it and only the
      200-variable case is unambiguous — every single run beat the baseline by
      at least 32%, and the median by 62%.

      Why that case and not `small (jvm)`, when both use `^2`: two hundred
      squarings per evaluation against one. `powf(x, 2.0)` is cheap enough that
      a single call disappears into the noise, and two hundred do not.

      The real spread is **about 30%, not the 15% previously recorded**, with a
      systematic warm-up drift on top. A single reading of this suite is not
      evidence. That is now in `performance-records/README.md`, where someone
      reading a row will see it.

- [ ] **Not done: relevance-filtered parameters.** Planned as this wave's optional tail
      and skipped. `RuntimeProblem::parameters` carries the whole row where it could
      carry only the variables the failing subexpression reads — walk the program for the
      node matching `fault.span`, collect its `Kind::Global` ids, map them through
      `global_positions`. Error path only, so free on the happy path. `locals` is the
      harder half and needs a slot-to-name table the AST discards.


## Next

- [x] **SMT-LIB2 emitter.** Done, in `cvg/emit.rs`: a pure
      `(&[InputVariable], &[Expression]) -> Document`, twelve tests, no solver involved. `Fold` maps
      straight onto `(+ a b c …)` as hoped. Two things worth knowing came out of writing it:
      *Strictness does not survive the trip.* The boolean rewrite encodes `<` as `+ f64::MIN_POSITIVE`,
      which works because f64 rounding swallows it at any real magnitude. Real arithmetic does not
      round, so emitting it literally puts a 310-digit denormal in the document *and* means the
      wrong thing. The emitter recognises the marker and asserts `(< r 0.0)` instead.
      *`Global` and `var[i]` index different lists.* `Global` indexes the expression's own symbols;
      `var[i]` indexes the schema. Using one for the other mis-resolves silently, and did, until
      the unrolled-aggregate test caught it.
      What it cannot express — `sqrt`, `cbrt`, `mod`, `floor`, `ceil`, the transcendentals, a
      non-integer exponent — is **reported** through `Document::untranslated`, never dropped. Five
      of the seven `cvg_pools` reds are already expressible; `roots` needs `sqrt`/`cbrt` and
      `modulo` needs `mod`, and both are dialect questions rather than theory ones.

- [x] **A backend: Z3, `bundled`, unconditional.** Done. Builds on Windows in about four minutes
      cold and links statically, so there is nothing to ship to a customer machine. The three
      hazards the probe warned about all landed as predicted, and all three are handled:
      *`Solver::from_string` cannot report a syntax error* — it returns `()`, and a malformed
      document leaves an empty solver that answers `sat` instantly. `Z3Backend::solve` refuses to
      believe any verdict unless `get_assertions()` came back non-empty. This is the load-bearing
      line in the backend; without it an emitter regression reads as success.
      *Algebraic irrationals have no rational form*, and `sqrt` is exactly where Z3 produces one.
      `Real::approx_f64()` is the fallback, used only when `as_rational()` declines.
      *`define-fun` helpers appear in the model* with arity > 0, and `apply(&[])` on one panics
      inside the binding rather than erroring. Skipped by arity.
      Unsat cores work through `from_string`, which was not a given — a contradictory pair comes
      back naming *both* constraints. `Solution::Unsatisfiable::blamed` changed from
      `Option<Expression>` to `Vec<Expression>` to say so honestly: a contradiction is a
      relationship, and `x > 8` is perfectly satisfiable until `x < 2` turns up.

- [x] **The emitter cannot produce an unbalanced document.** Terms are built as `Sexp` in
      `cvg/sexp.rs` and rendered by its `Display`, so parentheses come from structure rather than
      from format strings. That class of bug is not caught, it is unrepresentable — which matters
      because `Solver::from_string` reports a syntax error by silently accepting nothing and then
      answering `sat`. A stray paren used to read as "solved it".
      `sexp!` and `define_fun!` sit on top so the static fragments are written as lisp rather than
      as ninety lines of nested constructors; the prelude is six. They expand to `Sexp` constructor
      calls rather than to strings, so the shorthand does not bypass the guarantee — and balance
      ends up enforced twice, since Rust's tokenizer rejects an unbalanced macro argument before
      any of this code runs.
      Two Rust-isms shaped the syntax, recorded in the module docs: `define-fun` tokenizes as
      `define`, `-`, `fun`, so the keyword lives in the macro *name* rather than its argument; and
      `|x|` is a quoted symbol in SMT-LIB but two bitwise-ors in Rust, so quoted symbols come only
      from `Sexp::symbol` — fine, since they only ever hold run-time names. Raw `stringify!` was
      the alternative and is worse: it eats the space in `(ite(< x 0.0) ...)` and yields a string
      rather than a term.

- [x] **The prelude's *meaning* is pinned, not just its syntax.** It is a hand-written table and
      nothing else covered it: swapping `babel_min`'s `<=` for `>=` still parses, still solves, and
      every other test still passes, because no corpus case drives `min` through a solver.
      `the_prelude_helpers_mean_what_they_say` puts seventeen closed claims to Z3 —
      `(= (babel_min 2.0 5.0) 2.0)` must be sat, `(= (babel_max 2.0 5.0) 2.0)` must be unsat, and
      `babel_sgn(0)` must be `0` because babel follows Java there while Rust's `f64::signum` does
      not. Confirmed it fails by actually making that swap.

- [x] **Z3 validates every document the emitter can build.** A test walks 27 expression shapes —
      every translatable unary and binary, folds, blocks, `var[i]`, powers including negative and
      zero exponents, nested roots, a Unicode name — emits each and requires Z3 to accept it. Worth
      more than a parenthesis counter, since Z3 checks sorts, arities and scoping too.
      It also compares assertions *written* against assertions Z3 *took*, and pins the fact that
      makes one guard sufficient: **a parse error loses the entire document, not just the tail.**
      If Z3 ever became more forgiving, that test fails and the guard would need to start counting.

- [x] **The two emitter fixes.** Divisor guards — every `(/ a b)` now also asserts `b != 0`, without
      which Z3 satisfied `simple_arithmetic` by setting everything to zero and reading `0/0` as
      whatever suited. And auxiliary variables, so `sqrt` becomes `y >= 0 and y*y = x` and `cbrt`
      becomes `y*y*y = x` — which also gets the domain right for free, since a negative `sqrt` is
      then unsatisfiable, matching babel's NaN. That unlocked `roots`.
      Assertions are now named `(! ... :named c0)` so a core points back at a constraint, and
      auxiliary names carry their constraint index because two constraints both declaring `aux0` is
      a redeclaration that kills the whole document.

      **Z3 works, and a probe against a real emitted document turned up three things.**
      `z3 = { features = ["bundled"] }` builds on Windows and links statically — 4m38s cold, no DLL
      to ship. (An earlier attempt failed with CMake `FileTracker FTK1011`, which was MAX_PATH from
      a deep scratch directory, not Z3.) Feeding it `emit`'s actual output for `simple_arithmetic`:

      - [ ] **The emitter must guard divisors.** Z3 returned `x1=x2=x3=x4=0` as a model — satisfying
            the constraint via `x3/x4` = `0/0`, which SMT-LIB leaves *underspecified* so the solver
            may pick any value for it. Babel evaluates that point to NaN and rejects it, so the
            solver's answer would be silently discarded and the search would spin. Adding
            `(assert (not (= |x4| 0.0)))` produced a genuine point: `x4 = 1/2`, everything else zero.
            Every `(/ a b)` needs a side condition, which means the translator has to contribute
            assertions as well as a term.
      - [ ] **`Solver::from_string` swallows parse errors.** A deliberately malformed document
            returned **Sat** with an empty model rather than an error — the signature returns `()`.
            So an emitter bug would present as "solved it instantly". The backend has to verify the
            declared constants actually came back, or check Z3's error code, before believing a
            verdict.
      - **The auxiliary-variable encoding works.** `sqrt` as a fresh `aux0` with `aux0 >= 0` and
            `aux0*aux0 = x` solved fine, so `roots` is reachable once the translator can emit
            declarations alongside terms — the same machinery the divisor guards need.

      **UNSAT-as-diagnostic already works.** A contradictory pair (`x > 8` and `x < 2`) came back
      `Unsat` from the same path, with no pool and no model parsing involved.

      **cvc5 is ruled out on Windows.** `cvc5-sys` 0.4.0's build script is explicit:
      `#[cfg(not(unix))] #[cfg(feature = "static")] fn ensure_cvc5_built_and_install() { panic!(
      "This rust binding for cvc5 is only supported on Unix systems!") }`. The non-`static` path
      does not save it either — it probes for headers by invoking the C compiler with `-E -M -xc -`,
      which are GCC/Clang flags MSVC does not accept, and it wants a cvc5 already installed with
      `CVC5_LIB_DIR` pointing at it. So the in-process story becomes a build-time native-library
      requirement for anyone compiling babel, which is worse than the deployment problem it was
      chosen to avoid. It remains the best fit on Linux and macOS.

      Worth picking the theory before the solver: babel has `sin`/`cos`/`log`/`sqrt`, so that is
      QF_NRA with transcendentals — dReal's domain — rather than QF_FP, which is where Z3
      bit-blasts and falls over. `smt::DRealBackend` is stubbed with that reasoning recorded; the
      catch is that dReal has no Rust crate, so it is subprocess-only, which is the thing you did
      not want to ship to a customer machine. Z3 via `z3` + `bundled` links in-process and parses
      SMT-LIB2 through `Solver::from_string`, so text emission and in-process execution are
      orthogonal choices.
      Decided on the literal question: shortest round-trip decimals, not exact rationals. Exact
      literals would close one gap and leave the larger one open, since SMT reasons in real
      arithmetic while babel evaluates in `f64` and *every operation* rounds — `(* x x x)` need not
      equal `x.powf(3.0)` in the last place. The model is a real-arithmetic idealisation on purpose,
      and solver output is filtered through babel's own `evaluate` regardless. Swapping in exact
      dyadic rationals is a one-function change if that ever proves wrong.
      Considered and rejected: reading the literal's *source text* back through its span. Tempting,
      since SMT-LIB reals are exact decimals and a user's `0.1` maps in losslessly — but babel
      evaluates the `f64`, and a solver's point is filtered through babel's own `evaluate` before
      anyone sees it, so source text would have SMT modelling a program babel does not run. It is
      also a no-op in practice: for every literal in the corpus the shortest round-trip repr is
      character-identical to what was typed. And it covers neither input bounds (which arrive from
      the caller as `f64` with no source at all) nor `pi`/`e` (whose source text is not a numeral).
      The one place the argument reverses is the UNSAT diagnostic, where "what the user meant" is
      arguably the right semantics.
      Considered and rejected: reading the literal's *source text* back through its span. Tempting,
      since SMT-LIB reals are exact decimals and a user's `0.1` maps in losslessly — but babel
      evaluates the `f64`, and a solver's point is filtered through babel's own `evaluate` before
      anyone sees it, so source text would have SMT modelling a program babel does not run. It is
      also a no-op in practice: for every literal in the corpus the shortest round-trip repr is
      character-identical to what was typed. And it covers neither input bounds (which arrive from
      the caller as `f64` with no source at all) nor `pi`/`e` (whose source text is not a numeral).
      The one place the argument reverses is the UNSAT diagnostic, where "what the user meant" is
      arguably the right semantics.

- [x] **One entry point.** `solve`, `solve_with_rng` and `solve_with` are collapsed into
      `ConstraintSolver`, which holds the three injected dependencies — rng, known-feasible points,
      strategy list — as fields with defaults. Construction is infallible, because nothing it holds
      can be invalid alone; the one thing that can be (a constraint naming a variable the box does
      not declare) needs the problem and is still checked in `solve`. `Strategy`,
      `with_strategies` and `with_rng` stay `#[doc(hidden)]`: which strategy runs is the module's
      decision, and `Route` makes most of it at runtime from a probe rather than from
      configuration.
      Tests drive it with `#[pollster::test]` — a dev-dependency, so nothing propagates to
      consumers. Chosen over `#[tokio::test]` because the body is synchronous and a reactor buys
      nothing, and because the crate's own tests then stand as proof that no runtime is required.
      One line to swap if that turns out to be the wrong call.

- [x] **`solve` is `async` and now behaves like it.** The search runs on a worker thread; `solve`
      awaits a `futures_channel::oneshot` carrying the opening verdict, so the future genuinely
      completes off-thread and a caller-side `timeout` has something to race. Cancellation is an
      `AtomicBool` the worker reads between batches — the cooperative token that was always the
      actual requirement, since no signature can cancel CPU-bound work that does not agree to stop.

      The split that made it work: **one-shot and ongoing are different asynchronies.** "Crack the
      first point" has a completion and belongs to a `Future`; "keep filling between requests" has
      none and belongs to a worker. Trying to make one `Future` carry both is why it previously fit
      neither.
      Today's `ConstraintPool` became `Generator`, private to the worker and never shared, so there
      is nothing to lock. The public `ConstraintPool` is a handle: a `Receiver`, a buffer, and a
      stop flag. `std::sync::mpsc::sync_channel` supplies the rest for free — a bounded channel
      *is* the high-water mark, and `TryRecvError`'s `Empty`/`Disconnected` *is* the
      starved-versus-exhausted distinction that decides whether a caller waits or gives up.
      `generate` blocks until it has the count or the pool is exhausted, which cannot hang because
      exhaustion is detectable, and `status` covers the rest. `found()` and `try_generate` were
      both deleted: the first had no callers, and the second was speculative — letting every call
      block is simpler and the blocking is bounded anyway. `Status::Failed` carries the panic
      message rather than sitting beside a separate `failure()` accessor.

- [ ] **`Solution` wants an accessor.** Getting the pool out means writing
      `Solution::Satisfied(pool) | Solution::Unknown { pool, .. } => pool` at every call site. The
      Kotlin had a `Worthwhile` supertype over exactly those two cases; a `fn pool(&mut self)` or
      `fn into_pool(self)` is the Rust equivalent and removes the repetition.

- [x] **`SmtBackend` no longer demands `Send + Sync`.** It carried those while it was a stub, on
      the assumption the pool would share a `Box<dyn SmtBackend>` across threads. It does not — the
      backend is built where it is used, on the worker thread, and no call site is dynamic. The
      bounds would also have ruled out a reasonable implementation, since Z3's context is
      thread-local and anything caching one could never be `Sync`.

- [ ] **Suite runtime is 31 seconds**, nearly all of it `top_corner_200d` at 26 — which was
      already 23 before the worker landed, because burn-in for a 200-dimensional chain is genuinely
      expensive. The extra three seconds are read-ahead: `BATCH_SIZE * CHANNEL_CAPACITY` points get
      produced whether or not anybody asks for them, and at 200 dimensions a point is about four
      hundred walker moves. Both constants are deliberately small for that reason.
      Plus about four minutes on a cold build for Z3's C++, which caches.
      `.config/nextest.toml` sets a 60s slow-timeout with `terminate-after`, so a stuck test reports
      instead of stalling CI. The threshold has to clear the slowest *honest* test by a wide margin
      — an earlier 30s sat close enough to flag `top_corner_200d` as slow, which just teaches people
      to ignore the warning. Each emitted point costs about
      one feasibility evaluation per shrink, times thinning, and the distribution oracles each cost
      a whole extra solve. `TopCorner200D` alone is ~10s. Tolerable now; worth watching.

## Diagnostics

- [ ] **Split `Display` on the `{}` / `{:#}` boundary.** `Problem` always renders the full
      caret block, which is wrong for a log line. Plain `{}` should be the one-line summary and
      `{:#}` the block with source and caret.
      Note this does not recover Kotlin's `abbreviatedProblemText`, which elided lambda bodies
      (`sum(0/0,20,i->...)`) using the parse tree. `Display` has only source and span, so it
      renders `source[span]`. Whitespace collapsing is reachable; lambda elision is not.
- [ ] **`RuntimeProblem.locals` ships empty.** Kotlin printed `local-variables{x=3.0}` — covering
      `var x = …` bindings as well as lambda parameters, since both lived in the same runtime heap.
      Filling it needs a slot-to-name table the AST deliberately discards. `parameters` is
      populated; this is the remaining half.

## Coverage

- [ ] **Get code coverage running.** `cargo-llvm-cov` is the mainstream choice: LLVM source-based
      instrumentation, integrates with nextest (`cargo llvm-cov nextest`), and emits lcov, HTML and
      cobertura. lcov is what Coveralls, Codecov and IDE importers all read, so one command feeds
      both CI and the editor.

      Three things to know before chasing a number:

      **Exclude the generated parser first.** `babel_parser.rs` is ~2,760 lines in `OUT_DIR`.
      Unless it is filtered out (`--ignore-filename-regex`) it dominates the report and the
      percentage means nothing.

      **100% statement coverage is impossible**, and not for want of tests. The `unreachable!()`
      arms exist precisely because translation rules those states out — they cannot be reached by
      any input, by construction. Either accept the shortfall, restructure so the impossible states
      are not representable, or mark them (`#[coverage(off)]`, nightly).

      **Path coverage is not really on offer.** Rust gives good line and region coverage; branch
      coverage has been the weak spot for years. Region coverage is the closest useful thing — it
      counts distinct executed regions rather than whole lines, so it does catch a `?` that never
      fired.

- [ ] Wire it into `.github/workflows/rust.yml` and publish to Coveralls, once the number means
      something.

## Performance

- [x] **There is a benchmark now**, on both sides, and the numbers are in
      `crates/babel/tests/throughput_benchmarks.rs` (`just bench`, release) and
      `src/test/kotlin/.../ThroughputBenchmarks.kt`. Points per millisecond:

      | expression | vars | Rust bound | Rust naive | JVM map | bound vs JVM |
      |---|---|---|---|---|---|
      | `x1 + x2` | 2 | 18584 | 4680 | 9700 | 1.9x |
      | `x1 + x2 > 20 - x3^2` | 3 | 9164 | 2843 | 4198 | 2.2x |
      | `sin(x1)*cos(x2)+sqrt(abs(x3))` | 3 | 9360 | 2935 | 4388 | 2.1x |
      | deep arithmetic | 4 | 3546 | 1792 | 1244 | 2.9x |
      | `sum(1, 200, i -> var[i]^2 - 3.0)` | 200 | 77.6 | 46.7 | **4.5** | **17x** |

      Rust release, JVM 11 (the toolchain the Gradle build pins, despite `JAVA_HOME` being 21).
      *bound* is `Bound::evaluate(&[f64])`, bound once; *naive* is
      `Expression::evaluate(&[(&str, f64)])`; *map* is `BabelExpression.evaluate(Map)`. Every
      harness gets pre-built inputs, so none of them is timing allocation.

      The old `~10k evals/sec` figure was worse than recorded: `PerformanceFixture` builds its `Map`
      inside the timed region, calls `print(".")` inside it as well, and warms up for fifty
      iterations where tiered HotSpot wants ten thousand before C2 engages. Left in place as the
      record of what that number meant.

- [ ] **`Expression::evaluate` is slower than the JVM's, and it should not be.** 4680 against 9700
      points/ms on `x1 + x2`; the JVM wins the small cases outright and only loses once expressions
      get dear enough to hide the difference. The cause is not the evaluator, it is that the
      convenience wrapper builds a whole `Schema` per call — `Schema::new` clones a `String` for
      every name, so the 200-variable case allocates two hundred strings *per evaluation*, where
      the JVM merely hashes into a map it already has.
      Fixable without touching the evaluator: resolve symbols against the supplied pairs directly
      rather than constructing a `Schema` and binding. Worth doing, because this is the method
      whose name makes it the one a newcomer reaches for.

- [ ] **The JVM tree does not compile on this branch**, so the Kotlin benchmark cannot be run in
      place. Commit `db9add8` ("POrting to rust") commented out four `locals [...]` declarations in
      `BabelParser.g4` — `availability`, `closedValue`, `value` — which `rewriters.kt` still
      depends on, giving nine unresolved references. Presumably the Rust ANTLR codegen would not
      accept them.
      The numbers above came from a throwaway `git worktree` at `db9add8^`, the last commit where
      it built; the worktree has been removed. `ThroughputBenchmarks.kt` is committed to the real
      tree and will run the moment the grammar is restored — or it can be deleted along with the
      rest of the JVM tree, which is already on this list.
- [x] **Flatten the AST to a tape**, batch loop innermost, and *measure before reaching for
      SIMD*. Done; see step 1 of `i-am-the-brute-squad.md`. The tree-walk evaluator was not kept
      as the differential oracle after all: the tests are the spec, and the walker is gone.
- [ ] **Reverse-mode autodiff over the tape.** Roughly 100 lines once the tape exists, and probably
      worth more to the expensive-constraint / penalty-function work than raw throughput.
- [ ] **GPU** via wgpu or CubeCL, once the batch tape shows the shape is right.

## Semantic analysis

Babel is small but not so small that it has no semantics to check. These are *translation-time*
errors — knowable without running the expression — and they are the reason `SemanticTranslator`
keeps a fallible signature.

- [ ] **Statically illegal subscripts.** `var[0]` and `var[-1]` are wrong for *every* schema, since
      indices are one-based — no need to wait for a row. Same for a non-integral literal subscript.
      Aggregate bounds already get this treatment via constant folding; subscripts do not.
- [ ] Revisit whether anything else deserves rejecting rather than evaluating: division by a
      literal zero, a lambda whose parameter is unused, a bound range that is statically empty.
      Kotlin allowed all three, and the first is load-bearing — `0/0` producing NaN is how the
      illegal-bound check triggers.
- [ ] **General constant folding.** Only aggregate bounds fold today. Kotlin folded more broadly.
      An optimisation, not semantics.

## Cleanups

- [ ] **Translation is currently infallible, and its `Result` is a lie.** Every error path in the
      translator was an `unsupported` problem; with those gone there is no reachable `Err`, and
      `SemanticTranslator` has no fields left — a unit struct with `&self` methods.
      **Deliberately left alone.** The semantic checks above make it genuinely fallible again and
      give the struct real state back (it needs the source to build problems). Revisit once they
      land: if they do not materialise, collapse the `Result` and turn the methods into free
      functions.
- [ ] **Canonicalise `+`/`*` into `Kind::Fold`.** Today `Fold` only arises from unrolling, so `a+b`
      has two possible shapes. Canonicalising would mean `BinaryOp` loses `Add` and `Mul` — a swap
      rather than an addition, so no consumer grows a case. Worth doing when SMT emission lands and
      the n-ary form is being consumed anyway.
- [ ] **Traversal helper (`preorder`/`postorder`).** Would let `contains_dynamic_lookup` be a
      read-only pass instead of an accumulator flag, and replace the hand-rolled recursion in
      `rewrite.rs`'s tests. Three callers now, so it has probably earned its place.
- [ ] **Upstream bug report to `antlr-rust-runtime`.** The parser builds
      `AntlrError::MismatchedInput { expected, found }` and formats it into a string before any
      listener sees it (`parser.rs:6819`), with the recovery path passing `error: None`
      (`parser.rs:5533`) while the fatal path passes `Some(error)`. So the expected-token set is
      only reachable as prose.

## Tooling

- [ ] **`just tag <version>`** — verify the tree is clean and `Cargo.toml`'s version matches the tag
      before tagging and pushing. Artemis pins babel by git tag, so a mismatched tag resolves fine
      and wastes an afternoon. `digital-twin`'s `sanity-check` job ports over nearly verbatim.
- [ ] **`cargo test --doc` in the justfile.** nextest does not run doctests. The `lib.rs` example is
      still ```` ```ignore ```` from when `compile()` was `todo!()`; it works now.
- [ ] **Justfile arg passthrough breaks on nextest filtersets.** `just test add` works;
      `just test 'test(/foo/)'` does not, because `{{ARGS}}` interpolates into a pwsh command line
      and pwsh tries to execute the filter expression.
- [ ] **Delete the Kotlin tree** and promote `crates/babel` to a root workspace when a second crate
      appears. `set working-directory` in the Justfile goes away at the same time.
- [ ] **Panama bindings** for the existing Java codebase. Mechanical, and the i64/f64 split will
      force changes on that side — but its model for variables is higher fidelity than "string", so
      it should bridge the gap without much trouble.

## Deliberately not doing

- **Serialization is a caller concern.** "Dump the source, recompile on load" is the whole strategy,
  and `Expression` already owns its source. Neither kotlinx.serialization nor XStream can tie into a
  Rust type anyway, and attaching serde to the AST would mean writing a `to_lisp`/`from_lisp` for no
  benefit.
- **No `Send + Sync` assertion.** It is almost certainly satisfied — everything in `Expression` is
  owned — but asserting it signals "somebody is doing something with threads here" when nobody is.
  Add it when rayon actually shows up.
- **Pluggable walkers** (`compile(literal, vararg walkers)`) are gone and staying gone; merging
  sojourn-CVG replaces the need.
- **`ProblemKind::BooleanInScalarPosition`** is deleted. It existed because the JVM rewriter would
  turn `x1 > 5` into `5 - x1` in place, letting `(x1 > 5) * 3` compile. The grammar admits
  `booleanExpr` only at `returnStatement`, so that is unreachable here — pinned by
  `a_boolean_cannot_be_used_as_a_scalar`.
