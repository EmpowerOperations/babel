# babel — the waves

The roadmap, one line per item. Reasoning, measurements and the things that went
wrong live in [todos.md](todos.md); this is the index into it.

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
- [ ] **Repair a near-miss point instead of discarding it.** When the solver
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

- [ ] **Lower to a tape.** A flat list of opcodes and arguments, so a whole grid
      of values can be run with one `foreach` rather than one tree walk per row.
      This is also the fix for `Expression::evaluate` rebuilding a `Schema` per
      call, and it is the thing the throughput benchmark exists to measure
      before and after. `eval.rs` stays as the oracle — same language, same
      libm, no FFI — which is exactly why it was written to be kept.
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
      in [todos.md](todos.md#equality-constraints).
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
- [ ] **The JVM tree does not compile** on this branch. Restore the grammar or
      delete the tree.
- [ ] **The benchmark cannot resolve small changes.** Run-to-run spread is about
      15% on this machine; anything under that is not a reading. Also, no
      benchmark expression contains a constant subexpression, so folding is
      invisible to it.
