# Babel — todo

The Kotlin → Rust port is done: every construct in the grammar translates and evaluates, and the
suite is 91/91. What follows is what is left, roughly in the order it wants doing.

## Next

- [ ] **sojourn-CVG.** Emit an SMT-LIB2 document from the AST rather than transcoding against the
      Z3 API directly. Aggregates over constant bounds already unroll into a single n-ary
      `Kind::Fold`, which maps onto `(+ a b c …)` without a flattening pass — the quantifier that
      pushed solvers into "unknown" is gone for the bounded case.
      Worth picking the theory before the solver: babel has `sin`/`cos`/`log`/`sqrt`, so that is
      QF_NRA with transcendentals — dReal's domain — rather than QF_FP, which is where Z3
      bit-blasts and falls over.

- [ ] **The i64/f64 API redesign.** Everything is `f64` today; indices are coerced at the
      boundaries by `to_index`, strictly. That works because every bound in the corpus is a
      literal, but a *continuously-valued* design variable used as a bound now fails on almost
      every sample where the JVM version's rounding silently coped.
      The fix is to let the declaring component say a variable is an integer: break the
      "all globals are f64" contract and replace it with "globals are f64 or i64". Touches
      `Schema`, `Bound`, `evaluate` and the row representation — a public API change, so it wants
      its own increment, and it is much cheaper before Artemis binds against the current shape
      than after. Follow-on: the lambda parameter becomes a genuine `i64` in scope rather than
      being converted back to `f64` for the body, and `2*i-1` becomes exact integer arithmetic.

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
