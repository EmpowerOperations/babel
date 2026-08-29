//! Handing constraints to an SMT solver.
//!
//! Nothing here is implemented yet. It exists so the shape is committed and the
//! solver survey is recorded, because the choice is consequential: most of the
//! JVM implementation's complexity was working around one solver's limits, not
//! doing the actual job.
//!
//! # The plan
//!
//! Emit an **SMT-LIB2 document** rather than driving a solver's API. The JVM
//! version built Z3 AST objects directly and paid for it — a hand-rolled Taylor
//! expansion of `sin` out to 1/11!, definitional axioms for `sqrt`, `cbrt`,
//! `log`, `ln`, `floor` and `ceil`, four axioms for `mod`, and a mechanism to
//! silently *drop* any constraint that made the solver answer UNKNOWN. Roughly a
//! third of that file was fighting the API rather than solving constraints.
//!
//! A text document is readable, diffable, testable with no solver attached, and
//! portable across solvers. [`crate::ast::Kind::Fold`] already matches SMT-LIB's
//! n-ary `(+ a b c …)`, so an unrolled aggregate emits directly.
//!
//! One fidelity note for whoever writes the emitter: SMT-LIB `Real` literals are
//! *exact decimals*, so emitting `0.1` asserts exactly one tenth while the `f64`
//! being emitted is not. The JVM version converted through exact rationals —
//! `"0.1111"` to `1111/10000` — which is what `LanguageFixture` was testing.

#![allow(dead_code)] // Scaffolding: the shape is committed, the bodies are not.

use super::ConstraintPool;

/// What a solver concluded about a document.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Outcome {
    /// Satisfiable, with a model — variable name to value.
    Sat(Vec<(String, f64)>),
    /// Provably no solution.
    Unsat,
    /// The solver gave up: timeout, unsupported theory, incomplete procedure.
    Unknown,
}

/// Somewhere to send an SMT-LIB2 document.
///
/// A trait because the deployment story and the theory support pull in opposite
/// directions, and we should be able to measure rather than guess. See the
/// survey below.
pub(crate) trait SmtBackend: Send + Sync {
    fn name(&self) -> &'static str;

    /// Solve a complete SMT-LIB2 document.
    ///
    /// # Errors
    /// Transport and process failures. A solver *concluding* `unknown` is an
    /// [`Outcome`], not an error.
    fn solve(&self, document: &str) -> anyhow::Result<Outcome>;
}

/// **dReal** — the best theory fit, and the reason this trait exists.
///
/// A δ-complete solver for nonlinear real arithmetic built on interval
/// constraint propagation. It answers `delta-sat` with a witness box, or
/// `unsat`. Crucially **`sin`, `cos`, `exp`, `log` and `sqrt` are primitives**,
/// so every transcendental workaround in the JVM implementation simply
/// disappears — no Taylor series, no range reduction, no definitional axioms.
///
/// "Satisfiable within δ" rather than exactly is the trade, and for *generating
/// sample points* that is arguably what you want: the job is finding points, not
/// proving theorems. Points get filtered against the real constraints anyway.
///
/// The catch, and the reason this is a stub rather than a dependency: **there is
/// no Rust crate.** dReal is subprocess-only — write the document to stdin, read
/// `delta-sat`/`unsat` and the model back. Which also hands you an OS-level
/// answer to a wedged solver, since `kill(pid)` always works where interrupting
/// a native library inside your own process does not.
pub(crate) struct DRealBackend {
    /// Path to the `dreal` executable.
    pub(crate) executable: std::path::PathBuf,
    /// The δ it is allowed to be wrong by. dReal's own default is `0.001`.
    pub(crate) precision: f64,
}

impl SmtBackend for DRealBackend {
    fn name(&self) -> &'static str {
        "dreal"
    }

    fn solve(&self, _document: &str) -> anyhow::Result<Outcome> {
        unimplemented!(
            "dReal backend: spawn {} with --precision {}, write the document to \
             stdin, parse `delta-sat`/`unsat` and the witness box from stdout",
            self.executable.display(),
            self.precision
        )
    }
}

/// **Z3** — the incumbent, and the only one that can be linked in-process.
///
/// The `z3` crate's `bundled`/`vendored` features build Z3 from source via
/// `z3-src`, so it links statically and there is no executable to deploy. And
/// `Solver::from_string` parses SMT-LIB2 text, so leaving the *API* does not
/// mean leaving *Z3* — the document approach works either way.
///
/// The cost is that Z3's QF_NRA has no transcendentals, so `sin` and friends
/// need encoding again. That is the whole reason to shop around.
///
/// `cvc5` has in-process bindings too (`cvc5` / `cvc5-sys`) and better
/// transcendental support than Z3; worth measuring against both.
pub(crate) struct Z3Backend;

impl SmtBackend for Z3Backend {
    fn name(&self) -> &'static str {
        "z3"
    }

    fn solve(&self, _document: &str) -> anyhow::Result<Outcome> {
        unimplemented!("z3 backend: `Solver::from_string`, then `check` and `get_model`")
    }
}

/// Renders a pool's constraints as an SMT-LIB2 document.
pub(crate) fn emit(_pool: &ConstraintPool) -> String {
    unimplemented!("SMT-LIB2 emitter: declare-const per input, assert per constraint, check-sat")
}

/// Called when rejection sampling cannot find a first point.
///
/// Sampling failing does not prove the region is empty — only a solver can
/// answer that — so until one is wired up this is a hard stop rather than a
/// misleading `Unsatisfiable`.
pub(crate) fn escalate_for_seed(pool: &ConstraintPool) -> ! {
    unimplemented!(
        "no SMT backend yet: rejection sampling found no feasible point for {pool:?}, \
         and only a solver can tell whether one exists"
    )
}
