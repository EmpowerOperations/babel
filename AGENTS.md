# Babel — notes for agents

Babel is a small expression language for optimizer formulations: `x1 + x2 * cos(x3)^2`
is a transform, `x1 < x2 + x3` is a constraint. It began as Kotlin/ANTLR on the JVM
(EmpowerOps' optimizer used it) and is being ported to Rust on this branch, with the
old university project sojourn-CVG (constrained random vector generation, Z3-backed)
folded in as `crate::cvg`.

Read these before changing anything, in this order:

1. [`crates/babel/src/README.md`](crates/babel/src/README.md) — the architecture:
   one `Ast`, a meaning-preserving front end, two backends (`eval`, `cvg`).
2. [`todo.md`](todo.md) — the roadmap *and* the reasoning: measurements, dead ends,
   and the decisions that are not recoverable from the code. Part two is long on
   purpose. Add to it when you learn something the code cannot say.
3. [`crates/babel/performance-records/README.md`](crates/babel/performance-records/README.md)
   — how to read and write a throughput number honestly.
4. [`i-am-the-brute-squad.md`](i-am-the-brute-squad.md) — the plan for wide-batch
   sampling (IR tape, CPU vectorisation, wgpu). Owns the "sample harder" tier;
   `todo.md` owns the solver and equality-constraint side.

## Layout

| path | what | status |
|---|---|---|
| `crates/babel/` | the Rust crate. Only thing that builds. | live |
| `src/main/antlr/*.g4` | the grammar. **Single source of truth**, shared by both implementations; `build.rs` regenerates the Rust lexer/parser from it. | live |
| `src/main/kotlin`, `src/test/kotlin`, `build.gradle.kts` | the JVM implementation | **intentionally broken**; kept as the port's reference. Do not fix the Gradle build. The Kotlin test fixtures are the spec the Rust tests were ported from — `corpus.rs` ← `BabelExpressionFixture.kt`, `cvg_pools.rs` ← `Z3SolvingPoolFixture.kt`, etc. |
| `sojourn-CVG/` | git submodule of the original CVG project | reference only; `sojourn.kt` does not compile (bare `fail;` at line 286) |
| `Justfile`, `.github/workflows/rust.yml` | CI is exactly `just ci` | live |

`crates/babel/src/frontend/generated.rs` is ANTLR output; never hand-edit it.

## Build and test

Everything runs from `crates/babel/` (the Justfile `cd`s there and uses `pwsh`).

```
just build          cargo build --all-targets   (also regenerates the parser)
just test-compile   cargo test --no-run         MUST stay green
just test           cargo nextest run --no-fail-fast
just lint           fmt --check + clippy -D warnings
just bench          release-mode throughput, writes performance-records/*.csv
just brute          time-to-first-hit rungs + checks/s, release, machine otherwise idle
```

- Use **nextest**, not `cargo test`: the AST is recursive and a stack overflow in one
  test must not take the binary with it. `.config/nextest.toml` sets a 60 s
  slow-timeout; `top_corner_200d` legitimately takes ~25 s.
- The `z3` crate is built with `bundled`, so a cold build compiles Z3 from source and
  needs CMake plus a C++ toolchain (MSVC on Windows). Slow the first time, cached after.
- `antlr-rust-codegen` pulls in RustPython; the lockfile currently wants a recent
  stable rustc. If `cargo build` complains about `requires rustc 1.9x`, update the
  toolchain rather than downgrading dependencies.
- The environment variable `BABEL_SMT_LOGIC` overrides the SMT-LIB logic (default
  `QF_NIRA`).

## How to work here

**TDD.** The port was driven test-first and the tests are the spec. A new behaviour
starts as a failing test in `crates/babel/tests/` (integration, public API) or a
`#[cfg(test)]` module beside the code (unit). Red tests are acceptable on a
feature branch; tests that fail to *compile* are not — that is an incomplete API.

**Assertions are exact by default.** Only cases that route through libm carry a
tolerance. Do not add blanket tolerances to make something pass.

**The tape is the only evaluator, and the tests are its spec.** `eval/` lowers
the AST to a three-address tape and runs it tiled or per lane. It was held to
the tree-walker it replaced on a few thousand random and adversarial rows, then
the walker was deleted. The spec is `tests/corpus.rs`, `tests/runtime_errors.rs`
and `tests/special_values.rs`: plain tests with hand-written expectations. Add
cases there; never a recorded-output file. The CPU tape checks for non-finite
values on every instruction; only a future GPU sieve is allowed to be coarse,
and it must re-run an offending column through the tape for the span rather
than be "fixed" to match (see src/README.md).

**Neither backend's lowering is visible to the other.** The front end produces the
canonical form of what the author wrote and nothing more. If a pass makes the tree
easier to *analyse*, it belongs in `frontend::rewrite`; if it makes it faster to
*run*, it belongs in `eval`; if it makes it *emittable* to a solver, in `cvg::emit`.
The `<= 0 is true` residual convention is `eval`'s, not the language's.

**SIMD is explicit.** The tile executor's kernels live in `eval/simd.rs`, built
on `pulp` with the instruction set picked at run time. Every operator is either
a named vector kernel or a named `*_scalar` one; do not rely on auto-vectorisation
anywhere. Never use pulp's `mul_add` (fused on every backend) or its `max`/`min`
(x86 semantics, not NaN-propagating). The crate has no `unsafe`; keep it that way.

**`sum` and `prod` bounds are constants.** Both are unrolled at compile time; a
bound that depends on a variable is a compile error, not a loop. That feature was
dropped deliberately (todo.md, "Dropped features") — do not reintroduce a
run-time aggregate without reading why.

**Nothing non-finite travels.** NaN/inf is a compile error where provable
(`ProblemKind::NonFiniteConstant`) and a runtime error otherwise
(`ProblemKind::NonFiniteValue`), reported against the innermost span.

**Measure before claiming a speedup.** Run-to-run noise on throughput is ~30%.
Compare medians of several runs, in one sitting, with an untouched case as a control,
against the parent commit. Benchmarks are release-only; a debug number is meaningless
and under upsert would overwrite a good row.

**Z3 is the solver, and its limits are known.** No logarithms, no `e`, `sin`/`cos`
parse but answer `unknown`, `^` with a variable exponent answers `unknown`, `^` with
a negative base and fractional exponent is unsound for babel's `cbrt`. The rewrite
passes (`invert_monotone`, `expand_powers`) exist to route around this; the metric
that matters is `Document::untranslated`. cvc5 and dReal were evaluated and rejected;
the table is in todo.md under "The solver question, settled". Do not re-shop for a
solver without a new fact.

**`Solver::from_string` returns `()`.** A malformed SMT-LIB document leaves an empty
solver that answers `sat` with an empty model. Every verdict in `cvg::smt` is gated
on the assertions having arrived; keep it that way.

## Style

- Doc comments explain *why* and record what was measured; the code says what.
  Match that register — the module headers are the model.
- Prefer a type that makes the mistake unrepresentable (`Sexp`, `Script`) over a
  check that reports it.
- Public API is batch-only: `CompiledExpression::eval(MatRef) -> Col<f64>`, one
  column per sample, one row per schema variable. `eval_row` is crate-private for
  the walker and is the same tape through the per-lane executor, not a second
  implementation.
- Non-obvious decisions go in `todo.md` part two, with the measurement that
  justified them.
