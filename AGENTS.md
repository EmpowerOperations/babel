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

**The pool's ladder is probe, solver, brute force.** `cvg`'s uniform sampler
(`Strategy::BruteSquad`) probes with one brute-force batch — tens of
microseconds — and delivers where that lands often enough. Where it lands
nothing, Z3 is asked first, under a resource limit (`with_solver_limit`, in
Z3's own units so the answer is machine-independent) — it settles a ribbon or a
contradiction in milliseconds and answers `unknown` on anything transcendental
or on anything past the limit — and only what Z3 could not decide gets brute force: the same
sampler on every core for a proposal budget (`with_proposal_budget`, default a
billion). What brute force finds is a function of the seed and the budget, never
of the thread count — keep it that way (the batch is the unit of randomness).
`Strategy` is a test-only configuration, not a user-facing one; the fairness
oracles in `tests/cvg_benchmarks.rs` measure against the same sampler. Pool
tests run with `common::PROPOSAL_BUDGET`, a million under debug, because the
default takes minutes on an unoptimised tape. The pool's state is a value:
`cvg::progress::Progress`, threaded through `serve` → `open` → `keep_filling`
and folded with `absorb`/`extend`, never a field. `Problem` is immutable and
compiled once; `Ladder` holds only the strategies' streams and knobs. Keep it
that way — the only `&mut` in the search is an RNG or a walker's chain.

**An equality is read before it is searched.** `cvg::classify` reads
`a == b +/- t` and answers what can be concluded: `Pinned`, `Driven`, `Implicit`
or `Opaque`. A `Driven` variable is one the walker *computes* rather than
searches, which is what lets it move along a measure-zero surface instead of
jittering beside it — `classify::plan` turns a system into the schema positions
the walker moves and the ones it computes, in evaluation order, and `Problem::retract`
applies it. Three rules hold the whole thing up:

- **Driving is a Gibbs draw, not an evaluation.** `y == f(x) +/- t` admits the
  whole band, so `retract` draws uniformly from `f(free) ± t`. Assigning
  `y = f(free)` collapses the band to its centre line and throws away a
  dimension — on two hundred pinned variables it returned the same point two
  hundred times. Drawing from a conditional slice is the move that leaves the
  uniform distribution invariant; evaluating to the centre is not.
- **A drive is a proposal, never a rewrite.** Feasibility is re-checked against
  every constraint afterwards, and `retract` skips a coordinate whose definition
  will not evaluate. So a wrong or over-narrow isolation costs rejected moves and
  never a wrong point — which is why the isolation table can be aggressive.
  Refusing to drive is always safe; no constraint is ever dropped.
- **A variable named on both sides makes the equality *implicit* in it**, and
  `ConstraintSystem::new` refuses it, naming the rearrangement (`a == b + a/2` is
  `a/2 - b == 0`). Not "cyclic" — a cycle is a mutual dependency *between*
  equations, which `plan` meets and handles by driving neither. Two narrower
  rules were tried and discarded; both are written up in todo.md.

`classify::isolate` peels arithmetic off a variable that occurs **exactly once**
(*linear* in it, in the term-rewriting sense), so `x1 + x2 == 3` drives `x1`.
Seven rules, one per operator, each with its own test — the two where operands do
not commute (`a - u == c`, `a / u == c`) are where a swap is silently wrong. No
inverses for `^`, `%`, `max`, `min` or the unary functions: those need a
*symbolic* inverse table, and `sin` would drive onto one branch of infinitely
many. The known hole is that driving assumes the feasible set is a **graph** over
the free coordinates; `x1 * x2 == 0` is a cross and one arm is unreachable. That
is red on purpose.

`var[i]` is resolved at `ConstraintSystem::new` — the first moment a schema
exists, since `parse` has none and `Kind::Global` indexes the expression's own
symbols while `var[i]` indexes the schema. After that
`Ast::contains_dynamic_lookup` means "a subscript nothing could resolve" rather
than "a subscript", and nothing downstream special-cases one.

**Another language is never built with a string builder.** WGSL and SMT-LIB
both go through askama templates under `crates/babel/templates/`, compiled at
build time against views in `eval/wgsl.rs`, `cvg/sieve.rs` and `cvg/emit.rs`.
The semantics — which helper, which guard, what is refused — stay in Rust; the
syntax lives in files that read as the language they produce, with one macro
arm per operator, and the operator types the templates match over are the
subsets the target language can spell, so a missing arm is a compile error. A
`format!` that writes a brace, a parenthesis, an operator or a keyword of
another language is the smell to refuse. Template output is validated (naga,
Z3's parser), checked for the substrings that matter and for balance, and never
recorded to a file.

**The GPU is a sieve and never a judge.** Behind the opt-in `gpu` feature
(`just brute`, `just bench` and `just test-gpu` turn it on), brute force runs
on whatever wgpu adapter is present: the tape is rendered as
WGSL through the templates, candidates are drawn and judged on the device in `f32`
with a slack, and *every survivor is re-judged exactly on the CPU*. A false
negative costs hit rate; a false positive costs a CPU check; neither changes an
answer. Shader compilers assume no NaNs, so the emitter guards every operator's
domain with a comparison rather than relying on NaN propagation — keep it that
way. Shader text is validated with naga and compared against the CPU, never
recorded to a file. The GPU path is deterministic per device, not across
machines; `with_gpu(false)` is the reproducible path, and `BABEL_GPU` picks the
adapter (`off`, an index, or a name) and logs the list when set. Every wait on the device
has a timeout, and the device is held only while a brute-force search is
using it — a `Weak` in the module, an `Arc` in each live sieve — never for the
life of the process. The default build has no wgpu in it and must stay that
way; `test-compile` builds with every feature so the GPU code cannot rot.

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
- Prefer a type that makes the mistake unrepresentable (`Progress`, the `Slot`
  binding table, `SmtUnary`) over a
  check that reports it.
- Public API is batch-only: `CompiledExpression::eval(MatRef) -> Col<f64>`, one
  column per sample, one row per schema variable. `eval_row` is crate-private for
  the walker and is the same tape through the per-lane executor, not a second
  implementation.
- Non-obvious decisions go in `todo.md` part two, with the measurement that
  justified them.
