//! Handing constraints to an SMT solver.
//!
//! The document itself is [`super::emit`]'s job; this is what sends it and reads
//! the answer back.
//!
//! # Why a document rather than a solver's API
//!
//! The JVM version built Z3 AST objects directly and paid for it: a hand-rolled
//! Taylor expansion of `sin` out to 1/11!, definitional axioms for `sqrt`,
//! `cbrt`, `log`, `ln`, `floor` and `ceil`, four axioms for `mod`, and a
//! mechanism that silently *dropped* any constraint making the solver answer
//! UNKNOWN. Roughly a third of that file was fighting the API rather than
//! solving constraints — and because it transcoded straight into Z3 objects, the
//! only way to see what it had asked was to ask Z3.
//!
//! Text is readable, diffable, testable with no solver attached, and portable
//! across solvers. It also makes the backend a small thing rather than the whole
//! integration.
//!
//! # The one hazard worth knowing
//!
//! **`Solver::from_string` returns `()`.** It cannot report a syntax error, and
//! a malformed document leaves an empty solver that then answers `sat` instantly
//! with an empty model — so an emitter bug reads as "solved it". Every verdict
//! here is therefore gated on the assertions actually having arrived. A parse
//! error loses the *whole* document rather than the tail after it, which is what
//! makes that single check sufficient; there is a test pinning exactly that, so
//! it will say so if Z3 ever becomes more forgiving.

#![allow(dead_code)] // `DRealBackend` is a recorded option, not a live one.

use anyhow::{Result, bail};

use super::{ConstraintPool, Point, emit};

/// What a solver concluded about a document.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Outcome {
    /// Satisfiable, with a model — variable name to value.
    Sat(Vec<(String, f64)>),
    /// Provably no solution, and which constraints were involved.
    ///
    /// Several indices rather than one culprit, because a contradiction is a
    /// *relationship*: `x > 8` is perfectly satisfiable until `x < 2` turns up.
    /// Naming one of them would be picking arbitrarily.
    Unsat { blamed: Vec<usize> },
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

    fn solve(&self, document: &str) -> Result<Outcome> {
        let solver = z3::Solver::new();
        solver.from_string(document);

        // `from_string` returns `()`. It *cannot* report a syntax error, and a
        // malformed document leaves an empty solver which then answers `sat`
        // instantly with an empty model — so an emitter bug would read as
        // "solved it". Checking the assertions actually arrived is the only
        // defence available, and it is worth more than it looks.
        if solver.get_assertions().is_empty() {
            bail!(
                "Z3 parsed no assertions from a {}-byte document. \
                 `Solver::from_string` reports a syntax error by silently \
                 accepting nothing, so treat this as one.",
                document.len()
            );
        }

        match solver.check() {
            z3::SatResult::Unsat => Ok(Outcome::Unsat {
                blamed: solver
                    .get_unsat_core()
                    .iter()
                    .filter_map(|term| emit::core_index(&term.to_string()))
                    .collect(),
            }),
            z3::SatResult::Unknown => Ok(Outcome::Unknown),
            z3::SatResult::Sat => {
                let Some(model) = solver.get_model() else {
                    bail!("Z3 answered sat but produced no model");
                };

                let mut values = Vec::new();
                for declaration in model.iter() {
                    // The `define-fun` prelude helpers are in the model too and
                    // they take arguments; `apply(&[])` on one panics inside the
                    // binding rather than returning an error.
                    if declaration.arity() != 0 {
                        continue;
                    }
                    let Some(term) = model.get_const_interp(&declaration.apply(&[])) else {
                        continue;
                    };
                    let Some(real) = term.as_real() else {
                        continue;
                    };

                    // Prefer the exact rational. Nonlinear arithmetic can put an
                    // algebraic irrational in a model — `sqrt` is exactly where —
                    // and those have no rational form, so fall back to Z3's own
                    // decimal approximation rather than dropping the variable.
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "narrowing a model value to f64 is the point of this function"
                    )]
                    let value = match real.as_rational() {
                        Some((numerator, denominator)) if denominator != 0 => {
                            numerator as f64 / denominator as f64
                        }
                        _ => real.approx_f64(),
                    };
                    values.push((declaration.name(), value));
                }
                Ok(Outcome::Sat(values))
            }
        }
    }
}
/// What a solver had to say about a pool that sampling could not crack.
pub(crate) enum Verdict {
    /// A point worth walking out from.
    ///
    /// `unexpressed` is normally empty. When it is not, the point satisfies only
    /// the constraints the emitter could write down — still the best start
    /// available, and the pool filters it against *all* of them anyway, but the
    /// caller must not report the region as understood.
    Seed {
        point: Point,
        unexpressed: Vec<usize>,
    },
    /// No point exists, and these constraints are why. Only ever returned when
    /// the solver was shown the whole problem.
    Impossible { blamed: Vec<usize> },
    /// Nothing usable, carrying whatever the emitter could not express — which
    /// is usually the reason.
    Inconclusive { unexpressed: Vec<usize> },
}

/// Asks a solver for a first point, once rejection sampling has failed to find
/// one.
///
/// Sampling coming up empty does not prove a region is empty; only a solver can
/// say that, which is why this is the one path able to produce
/// [`super::Solution::Unsatisfiable`].
///
/// # Errors
/// Transport and process failures. A solver *concluding* something — including
/// that it cannot decide — is a [`Verdict`], not an error.
pub(crate) fn escalate_for_seed(pool: &ConstraintPool) -> Result<Verdict> {
    let document = emit::emit(&pool.inputs, &pool.constraints);
    let unexpressed = document.untranslated;

    Ok(match Z3Backend.solve(&document.text)? {
        Outcome::Unsat { blamed } if unexpressed.is_empty() => Verdict::Impossible { blamed },

        // "Nothing satisfies the constraints we wrote down" is a much weaker
        // claim than "nothing satisfies the constraints" when some were left
        // out, and the difference is exactly the one worth not eliding.
        Outcome::Unsat { .. } | Outcome::Unknown => Verdict::Inconclusive { unexpressed },

        Outcome::Sat(values) => Verdict::Seed {
            // The model names variables; a `Point` is positional. Anything the
            // solver did not pin — an auxiliary, or a variable left free —
            // simply is not in the model, so fall back to the lower bound and
            // let the pool's filter judge the result.
            point: pool
                .inputs
                .iter()
                .map(|input| {
                    values
                        .iter()
                        .find(|(name, _)| *name == input.name)
                        .map_or(input.lower_bound, |(_, value)| *value)
                })
                .collect(),
            unexpressed,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cvg::InputVariable;

    /// How many assertions a solver actually took from a document.
    ///
    /// Compared against how many were written. Z3 stops parsing at the first
    /// error, so a malformed *tail* leaves the earlier assertions in place and
    /// the non-empty guard in `solve` never fires. A short count is the only way
    /// that shows up.
    fn assertions_taken(document: &str) -> usize {
        let solver = z3::Solver::new();
        solver.from_string(document);
        solver.get_assertions().len()
    }

    /// Every shape the emitter can produce, run past Z3 to see if it parses.
    ///
    /// This is the closest thing to a linter available without writing one, and
    /// it is worth more than a parenthesis counter: Z3 checks sorts, arities and
    /// scoping too, so a `Real` where an `Int` belongs or an auxiliary used
    /// before its declaration fails here rather than silently.
    ///
    /// It only works because [`Z3Backend::solve`] refuses a document that
    /// produced no assertions. `Solver::from_string` reports a syntax error by
    /// accepting nothing and then answering `sat`, so without that guard this
    /// test would pass on garbage.
    #[test]
    fn every_document_the_emitter_can_build_parses() {
        let box_of = |names: &[&str]| -> Vec<InputVariable> {
            names
                .iter()
                .map(|name| InputVariable::new(*name, -8.0, 8.0))
                .collect()
        };

        let cases: Vec<(Vec<InputVariable>, &str)> = vec![
            // literals, including the shapes `real` has to special-case
            (box_of(&["x"]), "x > 4"),
            (box_of(&["x"]), "x >= -1.5"),
            (box_of(&["x"]), "x < 0.00001"),
            (box_of(&["x"]), "x > pi"),
            (box_of(&["x"]), "x > e"),
            // every unary that translates
            (box_of(&["x"]), "abs(x) > 1"),
            (box_of(&["x"]), "sqr(x) > 1"),
            (box_of(&["x"]), "cube(x) > 1"),
            (box_of(&["x"]), "sgn(x) > 0"),
            (box_of(&["x"]), "sqrt(x) > 1"),
            (box_of(&["x"]), "cbrt(x) > 1"),
            (box_of(&["x"]), "-x > 1"),
            // every binary that translates
            (box_of(&["x", "y"]), "x + y > 1"),
            (box_of(&["x", "y"]), "x - y > 1"),
            (box_of(&["x", "y"]), "x * y > 1"),
            (box_of(&["x", "y"]), "x / y > 1"),
            (box_of(&["x", "y"]), "max(x, y) > 1"),
            (box_of(&["x", "y"]), "min(x, y) > 1"),
            (box_of(&["x"]), "x^3 > 1"),
            (box_of(&["x"]), "x^-2 > 1"),
            (box_of(&["x"]), "x^0 > 1"),
            // structure: folds, blocks, subscripts, equality-with-tolerance
            (box_of(&["a", "b", "c"]), "sum(1, 3, i -> var[i]) > 2"),
            (box_of(&["a", "b", "c"]), "prod(1, 3, i -> var[i]) > 2"),
            (box_of(&["x"]), "var a = x * 2; var b = a + 1; b > 3"),
            (box_of(&["x", "y"]), "x == y +/- 0.001"),
            // nesting deep enough that a stray parenthesis would show
            (box_of(&["x", "y"]), "abs(sqrt(abs(x)) - cbrt(y / 2)) < 1"),
            // a name the lexer allows and SMT-LIB needs quoting for
            (box_of(&["λ"]), "λ > 0.5"),
        ];

        for (inputs, source) in cases {
            let constraint = crate::compile(source).expect("test constraint should compile");
            let document = emit::emit(&inputs, std::slice::from_ref(&constraint));
            assert!(
                document.untranslated.is_empty(),
                "{source:?} is not meant to be beyond the emitter"
            );

            Z3Backend.solve(&document.text).unwrap_or_else(|e| {
                panic!(
                    "Z3 rejected the document for {source:?}: {e}\n{}",
                    document.text
                )
            });

            // Z3 stopping partway is not an error, just a shorter document.
            let written = document.text.matches("(assert ").count();
            assert_eq!(
                assertions_taken(&document.text),
                written,
                "Z3 took fewer assertions than were written for {source:?}, so it stopped parsing partway:\n{}",
                document.text
            );
        }
    }

    #[test]
    fn the_parse_guard_actually_catches_a_bad_document() {
        // The guard above is the whole reason the previous test means anything,
        // so it needs its own proof that it fires.
        let broken = "(set-logic QF_NRA)
(declare-const x Real)
(assert (this is not smtlib))
";
        assert!(
            Z3Backend.solve(broken).is_err(),
            "a malformed document was accepted"
        );
    }

    #[test]
    fn a_parse_error_anywhere_loses_the_whole_document() {
        // This decides whether the non-empty guard in `solve` is *sufficient*.
        // The worry was a malformed tail: earlier assertions parse, the solver
        // is not empty, and the guard waves it through with the last constraint
        // silently missing. Z3 turns out not to work that way — it keeps nothing
        // at all — so the guard covers a bad tail as well as a bad head.
        //
        // Pinned rather than deleted: if Z3 ever became more forgiving, this is
        // the test that fails, and the guard would have to start counting.
        let truncated = concat!(
            "(set-logic QF_NRA)\n",
            "(declare-const x Real)\n",
            "(assert (>= x 0.0))\n",
            "(assert (<= x ((((("
        );
        assert_eq!(
            assertions_taken(truncated),
            0,
            "Z3 kept some assertions from a document with a syntax error in it"
        );
        assert!(
            Z3Backend.solve(truncated).is_err(),
            "the guard let a partially-parsed document through"
        );
    }
}
