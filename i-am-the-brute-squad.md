# I am the brute squad

*"Sample harder."* A plan for the regime where the feasible region is a
1e-6 to 1e-9 fraction of the box, the SMT solver cannot help — usually because
the constraint contains a transcendental — and the honest answer is to bring
more hardware to the party until a first feasible point lands. Once one has
landed, the existing land-and-expand machinery (adaptive seeding, hit-and-run)
takes over and this work is done.

Companion to [`todo.md`](todo.md), which owns the solver and equality-constraint
work. **Nothing here touches the Z3 or AST-rewrite side.** That work is in
flight on the other machine.

---

## The regimes

| feasible fraction | what works | who is responsible |
|---|---|---|
| > 1e-2 | plain rejection sampling, then the walker | the pool as it is today |
| 1e-2 .. 1e-6 | adaptive sampling seeds, the walker emits | the pool as it is today |
| **1e-6 .. 1e-9** | **wide-batch sampling on every core and any GPU present** | **this plan** |
| < 1e-9 | a solver, or a smarter search. No amount of silicon helps. | `todo.md` |

Time to first hit is geometric: expected proposals is one over the hit rate,
and the tail is heavy — a quarter of runs need 1.4x the expectation, one in
twenty needs 3x.

| hit rate | expected proposals | at 1M checks/s | at 40M checks/s |
|---|---|---|---|
| 1e-4 | 10 thousand | instant | instant |
| 1e-6 | 1 million | 1 s | 25 ms |
| 1e-8 | 100 million | 100 s | 2.5 s |
| 1e-10 | 10 billion | 3 hours | 4 min |
| 6e-61 (`top_corner_200d`) | never | never | never |

So the window hardware buys is about three orders of magnitude. That is still
where users' awkward-but-not-impossible constraints live, and it is the window
in which Z3's `unknown` on `sin` currently leaves them with nothing.

**Targets.** North of 1,000,000 constraint checks per second on a realistic
constraint, on the CPU alone. The number to beat from LGO is 40k, and the
tree-walker already does ~37M/s on `x1 + x2` at batch 256 on BATOU — so the
CPU target is a *pipeline* problem, not an evaluator problem. The GPU is
headroom for heavy constraints, not the thing that gets us over the line.

**Where this sits in the pool.** Today: probe with the fair sampler; ≥10% hit
rate routes to plain sampling, otherwise adaptive seeding plus the walker; Z3
is consulted only when the first batch is empty. The brute squad is the tier
between "first batch empty" and "ask Z3": blast a wide batch for a time budget,
and only then escalate. Eventually Z3 and the sampler should run concurrently
under one deadline — Z3 is single-threaded, the machine has sixteen threads,
whichever lands a point first wins — but the deadline and cancellation story
belongs with the solver work.

---

## The steps

### 0. Red tests first — done 2026-09-02

`tests/brute_squad.rs`, run with `just brute`. What landed, and what was decided
on the way:

- **The solver became `Strategy::Solver`**, last in `DEFAULT_STRATEGIES`. A
  strategy list alone could not keep Z3 out — the worker escalated whenever the
  first batch was empty — and a builder flag would have been a second knob that
  drifts from the list tests already pin. The list is now the escalation
  ladder, in order, and step 4's tier is one more variant before `Solver`.
- **The budget is enforced by polling `solve()` by hand** with a no-op waker and
  dropping the future at the deadline. Its body never yields and the work is
  already on a worker thread, so nothing in the library had to learn about
  deadlines. It leaks that worker until its current batch returns — milliseconds
  today, up to a budget once a tier loops — so a case stops at two misses rather
  than running a third attempt into a degraded machine. Step 4's deadline
  removes the leak.
- **Two of three attempts, and the B/E ≥ 3 rule.** With `m = exp(-B/E)` a green
  rung fails with probability `3m² - 2m³`: 4.9% at B/E = 2, 0.7% at 3. A tier
  turns a rung green only when its expected first-hit time is at most a third
  of the budget.
- **Ignored in debug, release only.** `just test` runs a p = 1e-2 smoke, the
  ladder pin and the checks/s count check; the twelve budgeted rungs need
  `just brute`, which is `--no-fail-fast` because red rungs are the point.
- **The trimmed grid**: 1e-4/1e-6/1e-8 for all three families, 1e-10 for the
  corner and the sine corner, 1e-12 for the corner only.

First run, on the laptop (DESKTOP-KH0S3BP, Ryzen 7 PRO 4750U), the whole file
in 8 s:

| rung | result | how |
|---|---|---|
| 1e-4, all three families | green | found in ~18 ms; seed 1 misses the corner and sine corner, seeds 2 and 3 hit. Deterministic. |
| 1e-6 and below, all nine | **red** | `gave up after ~4 ms` — the pool proposes ~13,200 candidates and stops |

And the checks/s ledger, first row, tree-walker as it stands:

| family | eval-only | pipeline | RNG cost |
|---|---|---|---|
| corner | 12.8M/s | 11.8M/s | 8% |
| ball | 11.6M/s | 10.3M/s | 11% |
| sine corner | 8.2M/s | 7.5M/s | 9% |

Two things this says before any tier is built. The evaluator is already twelve
times past the one-million target on one laptop core, so **the 1e-6 rung is red
purely because the pool gives up, not because checking is slow** — step 4's
loop alone should turn it green, and probably 1e-8 with all cores. And the RNG
is about a tenth of the pipeline, so fusing it into the batch path is worth
having but is not where the order of magnitude lives.

Original plan follows.

Two fixtures, both red until the rest lands, both passing a strategy list with
**no solver in it** — Z3 answers `x1 > 0.999999` instantly and would make the
whole fixture a lie. `DEFAULT_STRATEGIES` is already `pub`, so no environment
variable or config switch is needed.

- **Throughput.** Constraint checks per second on a fixed set of constraints,
  amortised over several batches. Seconds, not minutes. Records into
  `performance-records/` alongside the evaluator ledgers, same format, same
  caveats: release only, ~30% noise floor, compare medians in one sitting.
- **Time to first hit**, over a family of constraints with a tunable feasible
  fraction: 1e-4, 1e-6, 1e-8, and a **permanently red 1e-10** so we know what
  we are dealing with. Every case has a time budget and asserts a first
  feasible point arrives inside it. A test that never finishes hits the 60 s
  nextest timeout and burns two minutes per run; a budgeted one fails fast
  and reports how far it got.

The family should include at least one transcendental so the case Z3 refuses
is the case being measured.

### 1. The IR tape and a naive evaluator

"Obvious code to an obvious baseline." Lower `Ast` → `Tape`, then interpret it.

**Three-address form, not a stack.** A stack machine is a register machine
with a fixed implicit allocation, and the implicitness is what hurts: every
target wants names.

- Emission to WGSL is one `let t7 = t3 * t4;` per instruction.
- Batched CPU execution runs each instruction as one loop over the batch —
  `regs[dst][..] = op(regs[a][..], regs[b][..])`. The dispatch cost is
  amortised over the batch width, which is the entire win over the tree walk.
- Register reuse falls out of a trivial liveness pass because the code is
  straight-line. Fewer live registers means a smaller working set:
  `registers × batch_width × 8 bytes`, which is what picks the tile size.

Sketch:

```rust
struct Reg(u16);

enum Insn {
    Load   { dst: Reg, input: u16 },                    // schema row → register
    Unary  { dst: Reg, op: UnaryOp, a: Reg },
    Binary { dst: Reg, op: BinaryOp, a: Reg, b: Reg },
    // Compare / NearEq / And lower to Sub, Max and a Const epsilon.
    // Fold lowers to a chain of Binary.
}

struct Tape {
    consts: Vec<f64>,   // preloaded as broadcast registers 0..k
    insns: Vec<Insn>,
    registers: u16,
    result: Reg,
}
```

Constants live out of the instruction stream so every instruction packs into
eight bytes.

**What does not lower**, and falls back to the tree-walker: an aggregate whose
bounds are only known at run time, and `var[expr]` with a non-literal
subscript. Both are rare in constraints. A ratchet test lists what falls back,
in the style of `emit::tests::residue_what_the_emitter_still_cannot_express`.

**Two things folded in here rather than later:**

- The sampler fills a matrix directly instead of allocating a `Vec` per
  candidate. Otherwise the tape has nothing to eat.
- The tape uses plain `f64::max`/`f64::min`, not the NaN-propagating wrapper.
  A NaN anywhere makes the output NaN, the non-finite check catches it, and
  that column re-runs through the tree-walker for exact semantics and a span.
  This keeps the inner loops branch-free, and it is the contract
  `src/README.md` already wrote for a future kernel: *eager here, coarse
  elsewhere*.

**Test:** differential. Every corpus expression that lowers agrees **exactly**
with the tree-walker over a few hundred random columns. The tree-walker is the
oracle and stays.

### 2. CPU vectorisation — mostly free, with two catches

Faer will not vectorise these loops; its SIMD lives inside its own kernels
and elementwise ops are not among them. But if step 1's inner loops are zipped
slice iterators with no early exit, LLVM auto-vectorises `+ - * /`, `sqrt`,
`abs`, `max`, `min`, `floor`, `ceil` on its own. Transcendentals and `%` stay
scalar libm calls, which is fine for a sieve.

The catches:

- **The default x86-64 target is SSE2**, two lanes. AVX2 needs runtime
  dispatch: the `multiversion` crate does it with one attribute and no SIMD
  API. Reach for `pulp` only if the assembly says auto-vectorisation failed.
- **Batch width is a cache decision**, not a convenience. Pick it so
  `registers × width × 8` fits L1 or L2.

Transcendentals on CPU, if they ever matter enough: range reduction plus a
minimax polynomial, which is FMA chains and vectorises. Not tables — a gather
is the weak SIMD instruction. Sieve tier only; the exact evaluator judges.

Steps 1 and 2 may collapse into one.

### 3. GPU via wgpu, kernels as generated WGSL

The dumbest path is the right one, because the kernel comes from a *user's
expression at run time*. Every "compile Rust to GPU" project (rust-gpu,
rust-cuda, CubeCL) compiles functions known at build time; that is not this
problem. Every GPU driver already ships a JIT for shader source. So: the tape
becomes WGSL text, one line per instruction, and wgpu hands it to whatever
adapter is present — Vulkan, DX12 or Metal, on AMD, NVIDIA, Intel or an iGPU.

- **This is the oneAPI dream in miniature.** The tape is the one
  implementation; the CPU interpreter and the WGSL emitter are two small
  backends over it, structurally the same as the SMT-LIB emitter.
- **f32.** WGSL has f32 natively; f64 is a non-standard naga extension and f16
  has too few digits to rank residuals on. The GPU is a *sieve*: keep anything
  whose f32 residual is below a small positive slack, then re-evaluate
  survivors exactly on the CPU. It never changes an answer, only throughput.
- **Transcendentals are NVIDIA's problem.** WGSL has `sin cos tan asin acos
  atan sinh cosh tanh log log2 exp pow sqrt abs floor ceil sign max min %`,
  all on the special function units. `ln` is `log`, `log10` is `log(x)/ln10`,
  `cbrt` is `sign(x) * pow(abs(x), 1/3)`. Throughput drops on a heavy
  constraint and that is fine; optimism is the complexity reducer.
- **Optimistic connect.** Request an adapter; if none, or the shader fails to
  compile, run the CPU tape. A few lines.
- Development on this laptop's Vega iGPU (wgpu over Vulkan); real numbers from
  BATOU's RX 7800 XT.

Repair proper — nudging a near-miss onto the surface — stays parked with the
equality work in `todo.md`. The sieve only needs the exact re-check.

### 4. Wire it into the pool

The middle route: first batch empty → brute squad for a time budget → then
Z3. This is what turns the step-0 fixtures green. Pool-side, not Z3-side.

Also worth its own verdict while in there: **found one point, cannot find a
second**. The degenerate feasible set — a fully determined equality system,
an ulp-width band, a repaired witness sitting exactly on a boundary. The
walker's chord through such a point has zero length, every proposal fails,
and today it looks like exhaustion.

---

## Shelved, with reasons

- **Interval constraint propagation** over the AST (forward interval
  evaluation, backward projection through each operator, contract the box
  before sampling). It nests fine and handles `sin` and `ln` where Z3 refuses,
  but it is dReal's core algorithm with dReal's failure modes — the dependency
  problem, multi-branch preimages — and it is solver-side work. `todo.md`
  material.
- **Table-interpolated transcendentals.** Slower than the SFU on GPU, a gather
  on CPU. Polynomials if anything.
- **fp16 on the GPU.** Three significant digits; a residual on `x > 10.5` over
  `10..11` carries no ranking information at that precision.
- **`+/- 0.0` and negative tolerances as compile errors.** Real, cheap, wanted
  — two users typed `x1 == f(x2) +/- 0.0` and got an ulp game. But it is AST
  side. Noted for `todo.md`.

## The GPU landscape, so it need not be re-shopped

Surveyed 2026-09-01. wgpu is mature and vendor-neutral; CubeCL JITs `#[cube]`
Rust to CUDA/HIP/WGSL/SPIR-V/CPU-SIMD but is built for compile-time kernels
and admits rough edges; rust-gpu moved to the Rust-GPU org and rust-cuda is
active, both nightly and ahead-of-time; Mojo now targets AMD RDNA3/4 and
NVIDIA with an open-source GPU toolchain but is a language, not a library;
SYCL/oneAPI (Khronos's post-OpenCL answer; DPC++ and AdaptiveCpp) is C++
single-source, ahead-of-time, adopted in HPC and on Intel, marginal elsewhere.
OpenCL is shipped by everyone and started by nobody. None of the clever ones
solve *runtime codegen from a user expression*; shader text does.
