# How a babel expression is built

Source text in, an `Expression` out, through five passes. `compile` in
[`lib.rs`](lib.rs) is the whole pipeline and reads top to bottom.

```
                  fold_constants   invert_monotone   rewrite_booleans   unroll_aggregates
  source  ──►  ──────────────►  ──────────────►  ──────────────►  ──────────────►  Expression
        translate  (fallible)                                          (fallible)
```

| phase | entry point | what it does |
|---|---|---|
| parse & lower | `front_end::translate` | ANTLR parse tree to `ast::Program`; resolves names to `GlobalId`/`LocalSlot`, records `is_boolean_expression` |
| fold constants | `rewrite::fold_constants` | every subtree made only of literals becomes one `Kind::Literal` |
| invert monotone | `rewrite::invert_monotone` | `f(u) op c` becomes `u op' c'` for the strictly monotone `f` |
| booleans to arithmetic | `rewrite::rewrite_booleans` | eliminates `Kind::Compare` and `Kind::NearEq` |
| unroll aggregates | `rewrite::unroll_aggregates` | `Kind::Aggregate` over literal bounds becomes `Kind::Fold` |

The order is not arbitrary. Folding runs first because it makes *"is this
constant?"* stop being a question anywhere else — afterwards a statically known
value **is** a `Kind::Literal`, which is why inversion and unrolling can both
pattern-match instead of carrying evaluators of their own. Inversion has to see
`Kind::Compare`, so it goes before the boolean rewrite. Folding has to *not* see
the strictness epsilon that rewrite inserts, so it goes before as well.

Two passes are fallible, and both refuse rather than defer:

- `fold_constants` rejects a constant subexpression that works out to NaN or an
  infinity — `sqrt(-1)`, `1/0`, and the literal `1.0e400`, which babel's grammar
  admits and `f64` cannot hold.
- `unroll_aggregates` rejects a statically known bound that is not a usable
  index, so `sum(1, 20/3, …)` is a compile error rather than a silent round.

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

- [`eval.rs`](eval.rs) — `Bound::evaluate(&[f64])`, the hot path.
- [`cvg/emit.rs`](cvg/emit.rs) — renders constraints as SMT-LIB2 for a solver.
  What it cannot express it *reports* through `Document::untranslated` rather
  than dropping, which is most of why the two passes above exist: every
  constraint they rewrite is one the solver can then see.

## Files

| | |
|---|---|
| [`ast.rs`](ast.rs) | `Program`, `Block`, `Expr`, `Kind`, and the operator semantics in `UnaryOp::apply` / `BinaryOp::apply` |
| [`front_end.rs`](front_end.rs) | parse tree to AST |
| [`rewrite.rs`](rewrite.rs) | all four rewrites |
| [`eval.rs`](eval.rs) | the interpreter |
| [`diagnostics.rs`](diagnostics.rs) | `ProblemKind`, spans, and rendering |
| [`generated.rs`](generated.rs) | ANTLR output, not hand-edited |
| [`cvg/`](cvg) | constrained random vector generation — sampling, walking, SMT |
