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

**`eval` lowers as hard as it can**, in the name of speed. Today that is no
effort at all — it keeps the tree and walks it — and the type is opaque so a
flattened tape can replace the walk without an API change.

**`cvg` keeps the tree open**, because its whole job is reading structure: which
constraints a solver can be asked about, which variables another determines,
which comparison can be inverted into a bound.

The rule that keeps them honest: **neither backend's lowering is visible to the
other**. `rewrite_booleans` is the case to watch — the `<= 0` residual convention
is the *evaluator's*, and it currently runs in the shared pipeline where it
destroys the equality structure `cvg` needs. Moving it is the next change, not
this one.

## The front end

Six passes. `parse` in [`lib.rs`](lib.rs) is the whole pipeline and reads top to
bottom.

```
              fold_constants  invert_monotone  rewrite_booleans  unroll_aggregates  expand_powers
 source ─►  ─────────────►  ─────────────►  ─────────────►  ─────────────►  ─────────────►  Expression
      translate (fallible)                                       (fallible)
```

The output is an `Ast`, not an evaluable thing: turning one into something that
runs is `eval::compile`'s job, and one file over is `cvg`, which never lowers it
at all.

| phase | entry point | what it does |
|---|---|---|
| parse & lower | `frontend::translate` | ANTLR parse tree to `ast::Program`; resolves names to `GlobalId`/`LocalSlot`, records `is_constraint` |
| fold constants | `rewrite::fold_constants` | every subtree made only of literals becomes one `Kind::Literal` |
| invert monotone | `rewrite::invert_monotone` | `f(u) op c` becomes `u op' c'` for the strictly monotone `f` |
| booleans to arithmetic | `rewrite::rewrite_booleans` | eliminates `Kind::Compare` and `Kind::NearEq` |
| unroll aggregates | `rewrite::unroll_aggregates` | `Kind::Aggregate` over literal bounds becomes `Kind::Fold` |
| expand powers | `rewrite::expand_powers` | `x ^ n` for a constant whole `n` becomes repeated multiplication |

The order is not arbitrary. Folding runs first because it makes *"is this
constant?"* stop being a question anywhere else — afterwards a statically known
value **is** a `Kind::Literal`, which is why inversion, unrolling and power
expansion can all pattern-match instead of carrying evaluators of their own.
Inversion has to see `Kind::Compare`, so it goes before the boolean rewrite.
Folding has to *not* see the strictness epsilon that rewrite inserts, so it goes
before as well. Power expansion goes last, because a loop index is a literal
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
| runtime | `eval::eval_expr` → `ProblemKind::NonFiniteValue` | the rest: `ln(x)` at `x = 0`, overflow, a non-finite input |

The runtime check is on **every node**, not only the operations that can produce
a non-finite value, so the error names the innermost subexpression that went
wrong rather than the whole constraint. It also catches a non-finite *input* at
its `Kind::Global`, and an unwritten frame slot — `evaluate` fills the frame with
NaN as a sentinel, so reading one is a slot-allocation bug rather than a value.

Infinities are included deliberately, and `rewrite::monotone` is why: while
`ln(0)` was allowed to evaluate to `-inf`, zero satisfied *any* upper bound, and
the inversion pass had to carry a domain floor of `u >= 0` where the mathematics
asks for `u > 0`. Refusing the infinity is what lets the guards be the textbook
ones. `sqrt` keeps its inclusive floor, and the asymmetry is now principled:
`sqrt(0)` is a finite answer, `ln(0)` is not an answer.

**Eager here, coarse elsewhere.** This is the tree-walker's contract, not the
language's. The flattened tape and the SIMD/CUDA kernels should check the output
buffer — a reduction, never a branch per lane — and re-run an offending row
through this evaluator to get the span. Do not "fix" a kernel to match the
per-node check; the difference is the point.

The policy is one `is_finite` call at the foot of `eval_expr`. If a real use for
a saturating infinity turns up, that is where it changes.

## The one type

`ast::Kind` is a single enum spanning every phase, including the variants only
the front end produces and only `rewrite_booleans` consumes. That is deliberate:
it keeps every pass a composable `Program -> Program`, which is what makes the
rewriter pluggable. A separate post-rewrite type would make every pass change
types, and each new pass would need converting on both sides.

The cost is that `Kind::Compare` and `Kind::NearEq` are unreachable downstream
and the evaluator says so with `unreachable!`.

## The boolean convention

Babel has no boolean values at run time. A comparison lowers to arithmetic whose
*sign* carries the truth value: **`<= 0` is true**. So a violated constraint
reports how badly it was violated rather than merely that it was, which is the
canonical `g(x) <= 0` form an optimizer wants.

Strictness rides on a nudge: `a < b` becomes `(a - b) + ε` with ε being
`f64::MIN_POSITIVE`, which vanishes into rounding at any real magnitude and
survives only when the difference is exactly zero. `rewrite::residual` is the
single place that knows this, and `cvg::emit` is the single place that has to
undo it, since real arithmetic does not round.

Conjunction has no variant of its own, because it does not need one: two
residuals hold together exactly when their `max` is `<= 0`. `NearEq` lowers that
way, and so does the domain guard `invert_monotone` attaches to an upper bound.

## Who consumes the result

- [`eval/`](eval) — `compile(&Ast, &Schema)`, then
  `CompiledExpression::eval(MatRef)`. **One column per sample, one row per schema
  variable**, which is the shape `cvg` produces, so a generated batch is directly
  an input matrix with no transpose. There is no scalar entry point in the public
  API: the crate-internal `eval_row` exists because the walker is sequential by
  nature, and it is the same walk with a different loop around it, not a second
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
| [`eval/`](eval) | `compile` and the batch evaluator |
| [`diagnostics.rs`](diagnostics.rs) | `ProblemKind`, spans, and rendering |
| [`generated.rs`](generated.rs) | ANTLR output, not hand-edited |
| [`cvg/`](cvg) | constrained random vector generation — sampling, walking, SMT |
