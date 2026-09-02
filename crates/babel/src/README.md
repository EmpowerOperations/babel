# One AST, two backends

Babel has two consumers with genuinely different needs, and the crate is laid out
to say so. In the middle is an `Ast`; on either side is a backend that takes it
somewhere.

```
                                   +-- eval --> CompiledExpression   runs a batch
   source -->  frontend  -->  Ast -+
                                   +-- cvg  --> FeasibleSamples      searches for points
```

**`frontend` lowers as little as it can.** Everything it does is
meaning-preserving: it produces the canonical form of what the author wrote and
nothing more. A pass that makes the tree easier to *analyse* belongs there.

**`eval` lowers as hard as it can**, in the name of speed. It flattens the tree
to a three-address tape (`eval/tape.rs`), packs the temporaries into registers,
and runs it a tile of 256 samples at a time with each instruction one loop
across the lanes; a tape with a run-time loop runs a row at a time instead. The
type is opaque, so what runs the tape can change without an API change.

**`cvg` keeps the tree open**, because its whole job is reading structure: which
constraints a solver can be asked about, which variables another determines,
which comparison can be inverted into a bound.

The rule that keeps them honest: **neither backend's lowering is visible to the
other**. The case that proved it was `rewrite_booleans`, which used to sit in
this pipeline flattening every comparison into an anonymous residual — the
evaluator's convention, applied on `cvg`'s behalf, destroying the structure
`cvg` exists to read. It is `eval`'s now.

## The front end

Five passes. `parse` in [`lib.rs`](lib.rs) is the whole pipeline and reads top to
bottom.

```
              fold_constants   invert_monotone   unroll_aggregates   expand_powers
 source ─►  ──────────────►  ──────────────►  ──────────────►  ──────────────►  Ast
      translate (fallible)                        (fallible)
```

The output is an `Ast`, not an evaluable thing: turning one into something that
runs is `eval::compile`'s job, and one file over is `cvg`, which never lowers it
at all.

| phase | entry point | what it does |
|---|---|---|
| parse & lower | `frontend::translate` | ANTLR parse tree to `ast::Program`; resolves names to `GlobalId`/`LocalSlot`, records `is_constraint` |
| fold constants | `rewrite::fold_constants` | every subtree made only of literals becomes one `Kind::Literal` |
| invert monotone | `rewrite::invert_monotone` | `f(u) op c` becomes `u op' c'` for the strictly monotone `f` |
| unroll aggregates | `rewrite::unroll_aggregates` | `Kind::Aggregate` over literal bounds becomes `Kind::Fold` |
| expand powers | `rewrite::expand_powers` | `x ^ n` for a constant whole `n` becomes repeated multiplication |

The order is not arbitrary. Folding runs first because it makes *"is this
constant?"* stop being a question anywhere else — afterwards a statically known
value **is** a `Kind::Literal`, which is why inversion, unrolling and power
expansion can all pattern-match instead of carrying evaluators of their own.
Inversion has to see `Kind::Compare`, which it does, because nothing eliminates
one any more. Power expansion goes last, because a loop index is a literal
only once unrolling has substituted it: `sum(1, 3, i -> x^i)` reaches it as
`x^1`, `x^2`, `x^3`.

Two passes are fallible, and both refuse rather than defer:

- `fold_constants` rejects a constant subexpression that works out to NaN or an
  infinity — `sqrt(-1)`, `1/0`, and the literal `1.0e400`, which babel's grammar
  admits and `f64` cannot hold.
- `unroll_aggregates` rejects a statically known bound that is not a usable
  index, so `sum(1, 20/3, …)` is a compile error rather than a silent round.

## Nothing non-finite travels

One rule, enforced wherever it can be seen:

| phase | where | catches |
|---|---|---|
| compile | `rewrite::fold_constants` → `ProblemKind::NonFiniteConstant` | what is provable: `sqrt(-1)`, `1/0`, `1.0e400` |
| runtime | every checked instruction in `eval/tile.rs` and `eval/lane.rs` → `ProblemKind::NonFiniteValue` | the rest: `ln(x)` at `x = 0`, overflow, a non-finite input |

The runtime check is on **every instruction**, not only the operations that can
produce a non-finite value, so the error names the innermost subexpression that
went wrong rather than the whole constraint — instruction order is post-order,
so the first faulting instruction is the innermost node. It also catches a
non-finite *input* at its `Load`, and an unwritten local: the registers are
primed with NaN as a sentinel and a local the lowerer cannot prove assigned gets
an explicit `Check`, so reading one is a slot-allocation bug rather than a value.

Infinities are included deliberately, and `rewrite::monotone` is why: while
`ln(0)` was allowed to evaluate to `-inf`, zero satisfied *any* upper bound, and
the inversion pass had to carry a domain floor of `u >= 0` where the mathematics
asks for `u > 0`. Refusing the infinity is what lets the guards be the textbook
ones. `sqrt` keeps its inclusive floor, and the asymmetry is now principled:
`sqrt(0)` is a finite answer, `ln(0)` is not an answer.

**Eager per lane, per instruction.** This is the CPU evaluator's contract, not
the language's. The batched executor fuses the finite test into each
instruction's loop as an or-reduction, so the happy path pays nothing, and it
records each lane's first fault rather than stopping; the lowest faulted column
is reported at the end with the innermost span, exactly as the tree-walker it
replaced did. A future GPU sieve is the one place this is allowed to be
*coarse*: check the output buffer, re-run an offending column through this
evaluator for the span, and do not "fix" the kernel to match.

The policy is the `is_finite` test on every checked instruction in
`eval/tile.rs` and `eval/lane.rs`. If a real use for a saturating infinity turns
up, that is where it changes.

## The one type

`ast::Kind` is a single enum spanning every phase. That is deliberate: it keeps
every pass a composable `Program -> Program`, which is what makes the rewriter
pluggable. A separate post-rewrite type would make every pass change types, and
each new pass would need converting on both sides.

`Kind::Compare`, `Kind::NearEq` and `Kind::And` reach both backends intact, and
each lowers them its own way.

## The boolean convention belongs to `eval`

Babel has no boolean values at run time, so **the evaluator** turns a comparison
into arithmetic whose *sign* carries the truth value: `<= 0` is true. A violated
constraint then reports how badly it was violated rather than merely that it
was, which is the canonical `g(x) <= 0` form an optimizer wants.

Strictness rides on a nudge: `a < b` evaluates as `(a - b) + ε` with ε being
`f64::MIN_POSITIVE`, which vanishes into rounding at any real magnitude and
survives only when the difference is exactly zero — precisely where strict and
non-strict differ.

**This is one backend's convention, not the language's.** `cvg::emit` shares
none of it: a comparison is emitted as `(> x 5.0)`, an equality as two bounds
`and`-ed together. It used to receive `(< (- 5.0 x) 0.0)` and have to *detect* a
three-hundred-digit denormal to recover the strictness, and an equality arrived
as `(<= (babel_max …) 0.0)` — an `ite` where a conjunction was meant. Both are
gone with the pass that caused them.

`Kind::And` exists for the same reason. `invert_monotone` needs a conjunction
for its domain guard — `ln(x) < 2` means `x < e²` **and** `x > 0` — and used to
build `max(residual, residual) <= 0` by hand, which is the residual convention
leaking into the front end. A variant it can emit without knowing costs one arm
per backend.

## What the evaluator is held to

`tests/corpus.rs` (every construct at an ordinary input, exact unless it routes
through libm), `tests/runtime_errors.rs` (where a fault lands) and
`tests/special_values.rs` (signed zeros to the bit, operations that go
non-finite, every fault kind planted in a batch). Hand-written expectations
only. The tree-walking evaluator that preceded the tape was used once as a
differential oracle over a few thousand random rows and then deleted; a
recorded-output file was considered and rejected, because once the walker is
gone such a file is only the tape agreeing with itself.

## Who consumes the result

- [`eval/`](eval) — `compile(&Ast, &Schema)`, then
  `CompiledExpression::eval(MatRef)`. **One column per sample, one row per schema
  variable**, which is the shape `cvg` produces, so a generated batch is directly
  an input matrix with no transpose. There is no scalar entry point in the public
  API: the crate-internal `eval_row` exists because the walker is sequential by
  nature, and it is the same tape through the per-lane executor, not a second
  implementation.
- [`cvg/emit.rs`](cvg/emit.rs) — renders constraints as SMT-LIB2 for a solver.
  What it cannot express it *reports* through `Document::untranslated` rather
  than dropping, which is most of why the two passes above exist: every
  constraint they rewrite is one the solver can then see.

## Files

| | |
|---|---|
| [`ast.rs`](ast.rs) | `Program`, `Block`, `Expr`, `Kind`, and the operator semantics in `UnaryOp::apply` / `BinaryOp::apply` |
| [`frontend/`](frontend) | text to `Ast`: `parse.rs`, the `rewrite.rs` passes, the ANTLR output |
| [`eval/`](eval) | `compile`, the tape (`tape.rs`, `lower.rs`, `regalloc.rs`) and its two executors (`tile.rs`, `lane.rs`) |
| [`diagnostics.rs`](diagnostics.rs) | `ProblemKind`, spans, and rendering |
| [`generated.rs`](generated.rs) | ANTLR output, not hand-edited |
| [`cvg/`](cvg) | constrained random vector generation — sampling, walking, SMT |
