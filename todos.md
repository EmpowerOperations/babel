# Babel — todo

Loose threads from the Kotlin → Rust port, roughly in the order they want doing.
Numbers refer to the test suite in `crates/babel/tests`.

## Before parity

- [ ] **`var[i]` dynamic access** — the last unimplemented construct. 10 tests.
- [ ] **Real spans.** `span_of` in `front_end.rs` is a stub returning `Span::new(0, 0)`, so every
      AST node claims to be at character zero. Blocks `infinite_upper_bound`, which asserts
      `line_idx == 2`.
      Cheaper than first thought: `RuleNodeView::start()`/`stop()` give `TokenView`s for a whole
      context, and `Token::start()`/`stop()` are already measured in Unicode scalar values, so
      there is no byte conversion and the translator needs no source text. `stop()` is inclusive
      and `Span` is half-open.
- [ ] **Constant folding / static evaluation.** Blocks
      `nan_lower_bound_is_caught_at_compile_time`, the last red test. Also enables the
      compile-time `sum`/`prod` unrolling the JVM version did.
      *Careful:* unrolling manufactures deep trees — `sum(1, N, …)` becomes an N-deep chain — and
      the rewriter is recursive. Wants a depth cap. Hand-written babel is nowhere near the limit;
      unrolling is the only thing that will get there.

## The redesign — integers

- [ ] **Change the API to use `i64` and `f64`; propagate `i64` through scopes and lambda
      expressions.** Today everything is `f64` and indices are coerced at the boundaries by
      `to_index`, which is strict. That works only because every bound in the test corpus is a
      literal. A *continuously-valued* design variable used as a bound fails on almost every
      sample, where the JVM version's rounding silently "worked".
      The fix is to let the declaring component say a variable is an integer: break the
      "all globals are f64" contract and replace it with "globals are f64 or i64". Touches
      `Schema`, `Bound`, `evaluate`, and the row representation — a public API change, so it wants
      its own increment and its own conversation with Artemis.
      Follow-on: the lambda parameter becomes a genuine `i64` in scope rather than being converted
      back to `f64` for the body, and index arithmetic (`2*i-1`) becomes exact integer arithmetic
      rather than `f64` that happens to be exact below 2^53.

## Coverage

- [ ] **Get code coverage running.** `cargo-llvm-cov` is the mainstream choice: LLVM source-based
      instrumentation, integrates with nextest (`cargo llvm-cov nextest`), and emits lcov, HTML,
      and cobertura. lcov is the interchange format Coveralls, Codecov and IDE importers all read,
      so one command feeds both CI and the editor.

      Three things to know before chasing a number:

      **Exclude the generated parser first.** `babel_parser.rs` is ~2,760 lines of generated code
      in `OUT_DIR`. Unless it is filtered out (`--ignore-filename-regex`) it will dominate the
      report and the percentage will be meaningless.

      **100% statement coverage is currently impossible**, and not because of missing tests. The
      `unreachable!()` arms in `eval.rs` exist precisely because translation rules those variants
      out — they cannot be reached by any input, by construction. Either accept the shortfall,
      restructure so the impossible states are not representable, or mark them
      (`#[coverage(off)]`, nightly).

      **You are right that path coverage is a problem.** Rust's tooling gives good line and region
      coverage; branch coverage is the weak spot and has been partial for a long time. Path
      coverage is not really on offer. Region coverage is the closest useful thing — it counts
      distinct executed regions rather than whole lines, so it does catch a `?` that never fired.

- [ ] Wire it into `.github/workflows/rust.yml` and publish to Coveralls, once the number means
      something.

## Tooling and infrastructure

- [ ] **`just tag <version>`** — verify the working tree is clean and that `Cargo.toml`'s version
      matches the tag before tagging and pushing. Artemis pins babel by git tag, so a tag whose
      version disagrees resolves fine and wastes an afternoon. `digital-twin`'s `sanity-check` job
      does exactly this check and ports over nearly verbatim.
- [ ] **`cargo test --doc` in the justfile.** nextest does not run doctests. The `lib.rs` example
      is still ```` ```ignore ```` from when `compile()` was `todo!()` — it works now and wants
      un-ignoring.
- [ ] **`.idea/` needs a gitignore line** (`crates/babel/.idea/` is untracked).
- [ ] **Justfile arg passthrough breaks on nextest filtersets.** `just test add` works;
      `just test 'test(/foo/)'` does not, because `{{ARGS}}` interpolates into a pwsh command line
      and pwsh tries to execute the filter expression.
- [ ] **Delete the Kotlin tree** once parity is committed, and promote `crates/babel` to a root
      workspace when a second crate appears (an FFI cdylib, say). `set working-directory` in the
      Justfile goes away at the same time.
- [ ] **Rebuild `PerformanceFixture` as a real benchmark** against `Bound::evaluate`. Note the JVM
      one built its input `Map` *inside* the timed loop, so its ~10k evals/sec is not a usable
      baseline — a good chunk of it was map allocation.

## Semantic analysis

Babel is small but not so small that it has no semantics to check. Everything below is a
*translation-time* error — knowable without running the expression — and none of it is ported yet.
Together they are the reason `SemanticTranslator` earns a fallible signature.

- [ ] **Boolean in scalar position.** `ProblemKind::BooleanInScalarPosition` is defined with no
      producer. Kotlin's `TypeErrorReportingWalker` walked the tree tracking whether each
      `scalarExpr` evaluated to a boolean and rejected embedding one where a scalar was required.
      Straight port.
- [ ] **Statically illegal aggregate bounds.** A literal or foldable bound that is NaN, infinite or
      non-integral is knowable at compile time. `sum(1, 1.5, i -> i)` currently fails at run time,
      once per evaluation, when it could fail once at compile time. Needs constant folding.
- [ ] **Statically illegal subscripts.** `var[0]` and `var[-1]` are wrong for *every* schema, since
      indices are one-based — no need to wait for a row to find out. Same for a non-integral
      literal subscript. Better than the JVM version, which only ever caught these at run time.
- [ ] Revisit whether anything else deserves rejecting rather than evaluating: division by a
      literal zero, a lambda whose parameter is unused, a bound range that is statically empty.
      Kotlin allowed all three, and at least the first is load-bearing — `0/0` producing NaN is how
      the illegal-bound test triggers.

## Cleanups

- [ ] **Translation is currently infallible, and its `Result` is a lie.** Every error path in
      `translate_program`, `translate_block`, `translate_assignments`, `translate_scalar_expr` and
      `translate_boolean_expr` was an `unsupported` problem; with those gone there is no reachable
      `Err`, and `SemanticTranslator` has no fields left — it is a unit struct with `&self`
      methods, a namespace wearing an object costume.
      **Deliberately left alone.** The semantic checks above make it genuinely fallible again and
      give the struct real state (it will need the source text back to build problems). Revisit
      once they land: if they do not materialise, collapse the `Result` and turn the methods into
      free functions.

- [ ] **`RuntimeProblem.locals` and `.parameters` ship empty.** The evaluator sees slots and a flat
      `&[f64]`; populating them needs the `Schema` at the error site and a slot-to-name table the
      AST deliberately discards. Nothing asserts them yet.
- [ ] **Traversal helper (`preorder`/`postorder`).** Would let `contains_dynamic_lookup` be a
      read-only pass over the finished AST instead of an accumulator flag, and would replace the
      hand-rolled recursion in `rewrite.rs`'s test. Two callers is thin — revisit at three.
- [ ] **Upstream bug report to `antlr-rust-runtime`.** The parser builds
      `AntlrError::MismatchedInput { expected, found }` and then formats it into a string before
      any listener sees it (`parser.rs:6819`), with the recovery path passing `error: None`
      (`parser.rs:5533`). The fatal path passes `Some(error)`. So the expected-token set is only
      reachable as prose. Asking the recovery path to forward the structured error would let
      diagnostics carry it.

## Longer horizon

- [ ] **Flatten the AST to a structure-of-arrays tape**, batch loop innermost, and *measure before
      reaching for SIMD*. Keep the tree-walk evaluator permanently as the tape's differential
      oracle — same language, same libm, no FFI, and it isolates exactly the layer where the risky
      optimization lives.
- [ ] **Reverse-mode autodiff over the tape.** Roughly 100 lines once the tape exists, and probably
      worth more to the expensive-constraint / penalty-function work than raw throughput.
- [ ] **GPU** via wgpu or CubeCL, once the batch tape shows the shape is right. Only pays when
      evaluating thousands of candidates against one expression.
- [ ] **C API + Panama jar** for whatever still lives on the JVM. Not on the critical path —
      Artemis is Rust and can consume the crate directly.
- [ ] **Merge sojourn-CVG**, and move it from transcoding directly against the Z3 API to emitting
      SMT-LIB2. Worth picking the theory before the solver: babel has `sin`/`cos`/`log`/`sqrt`, so
      that is QF_NRA with transcendentals — dReal's domain — rather than QF_FP, which is where Z3
      bit-blasts and falls over.
