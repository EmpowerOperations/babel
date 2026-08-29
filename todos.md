# Babel — todo

The Kotlin → Rust port is done: every construct in the grammar translates and evaluates, and
babel's own suite is 92/92. sojourn-CVG has moved in as `crate::cvg`: contracts, ported
fixtures, an adaptive sampler, a hit-and-run walker, and distribution oracles with critical values
behind them. What follows is what is left, roughly in the order it wants doing.

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

The SMT backend is still `unimplemented!()`.

**114 of 122 tests pass.** Seven of the eight reds are measure-zero equality cases waiting on the
solver; the eighth is P118's distribution agreement.

| red | why |
|---|---|
| 7 × `cvg_pools` equality-with-tolerance | `x1 == sqrt(x2) +/- 0.0001` and friends: measure-zero ribbons that nothing samples and no seed reaches. The solver's job. |
| `p118` | two runs disagree by KS 0.114 against a 0.096 threshold — see below |

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

- [ ] **`Solution::Unsatisfiable` is unreachable.** Nothing constructs it. In the Kotlin it came
      from `Z3SolvingPool.check()` returning `UNSATISFIABLE` before any sampling started — the
      solver is the only thing that can earn the claim. The variant is right; it simply has no
      producer until the backend lands.

- [ ] **`UNSAT` as a babel diagnostic, not just a sampling verdict.** Separate from everything
      above, and probably the higher-value half of wiring up a solver. A user who writes two
      constraints that cannot both hold currently gets silence — the pool searches, finds nothing,
      and reports having found nothing, which looks identical to a region that is merely hard. A
      solver's `check()` distinguishes "no point exists" from "we did not find one", and that is a
      *compile-time* answer: report it through `ProblemKind` alongside the syntax errors, where the
      user is already looking.
      Notably this needs no pool, no sampling and no strategy selection — just `check()` on the
      emitted SMT-LIB2 — so it can land before or independently of the sampling backend. It also
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

## Next

- [ ] **SMT-LIB2 emitter.** A pure `Program -> String` function, testable with no solver attached,
      and where `Kind::Fold` earns its keep against `(+ a b c …)`. Aggregates over constant bounds
      already unroll, so the quantifier that pushed solvers into "unknown" is gone for the bounded
      case. This is what closes the five `cvg_pools` reds and the ribbon: they all need a first
      feasible point that no amount of sampling or walking will produce.
      Worth picking the theory before the solver: babel has `sin`/`cos`/`log`/`sqrt`, so that is
      QF_NRA with transcendentals — dReal's domain — rather than QF_FP, which is where Z3
      bit-blasts and falls over. `smt::DRealBackend` is stubbed with that reasoning recorded; the
      catch is that dReal has no Rust crate, so it is subprocess-only, which is the thing you did
      not want to ship to a customer machine. Z3 via `z3` + `bundled` links in-process and parses
      SMT-LIB2 through `Solver::from_string`, so text emission and in-process execution are
      orthogonal choices.
      Carry over from `LanguageFixture`: SMT-LIB `Real` literals are exact decimals, so emitting
      `0.1` asserts exactly 1/10 while the `f64` in hand is not. Kotlin converted decimals to
      rationals (`"0.1111"` → `1111/10000`) for this reason. Decide deliberately.

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

- [ ] **`solve` is `async` but does not yet behave like it.** Still true after the consolidation,
      and worth fixing before Artemis binds:
      *The cost is in the wrong function.* The expensive call is `ConstraintPool::generate`, which
      is synchronous and where the walker's burn-in happens — seconds, at 200 dimensions. A caller
      who awaits `solve` and then calls `generate` has awaited the cheap half.
      *An inline future cannot be timed out.* The body never yields, so a `timeout` wrapped around
      it will not fire — the one thing the async shape was justified with. Nor does a thread hop
      fix it on its own: cancelling CPU-bound work needs a cooperative check the work itself
      honours, an `AtomicBool` or a progress callback, which works the same in a sync signature.
      The signature does not need to change for either fix. What is needed is the body moved onto a
      thread behind a oneshot (which needs `PointSource` to be `Send`), and a cancellation token
      the search actually reads. The doc comment on `solve` says all of this so a reader does not
      mistake the `async` for a working timeout.

- [ ] **`Solution` wants an accessor.** Getting the pool out means writing
      `Solution::Satisfied(pool) | Solution::Unknown { pool, .. } => pool` at every call site. The
      Kotlin had a `Worthwhile` supertype over exactly those two cases; a `fn pool(&mut self)` or
      `fn into_pool(self)` is the Rust equivalent and removes the repetition.

- [ ] **Suite runtime is 37 seconds**, nearly all of it the walker. Each emitted point costs about
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

- [ ] **There is no benchmark.** `PerformanceFixture` was never ported, so nothing has been
      measured against the JVM. Not urgent — the expectation is that avoiding Java's memory
      semantics wins even before any vectorisation — but every performance decision from here is
      being made blind. Note the JVM fixture built its input `Map` *inside* the timed loop, so its
      ~10k evals/sec is not a usable baseline.
- [ ] **Flatten the AST to a structure-of-arrays tape**, batch loop innermost, and *measure before
      reaching for SIMD*. This is where the real speedup lives: replacing one-by-one evaluation
      with BLAS or GPU dispatch. Keep the tree-walk evaluator permanently as the tape's
      differential oracle — same language, same libm, no FFI, and it isolates exactly the layer
      where the risky optimisation lives.
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
