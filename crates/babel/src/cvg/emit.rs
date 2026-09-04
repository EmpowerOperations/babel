//! Rendering constraints as an SMT-LIB2 document.
//!
//! A pure `&[Ast] -> String`, with no solver attached and none required
//! to test it. That separation is the whole reason for emitting text rather than
//! building a solver's AST: the JVM version transcoded straight into Z3 objects,
//! so the only way to see what it had asked was to ask Z3.
//!
//! # What gets asserted
//!
//! Every constraint asserts as a boolean term — `(assert (> |x| 4.0))` — named
//! `cN` so an unsat core points back at the constraint the author wrote. Side
//! conditions the term depends on, described below, are asserted alongside it.
//!
//! # A comparison is emitted as a comparison
//!
//! It did not used to be. `rewrite_booleans` ran in the shared pipeline and
//! flattened every constraint into a residual before this module saw it, so
//! `x > 4` arrived as `4 - x + EPSILON` — the strictness carried by a nudge that
//! only works *because of `f64` rounding*. Real arithmetic does not round, so
//! emitting it literally put a three-hundred-digit denormal in the document and
//! still meant the wrong thing; this module had to recognise the marker and undo
//! it.
//!
//! `Kind::Compare` and `Kind::NearEq` survive compilation now and each backend
//! lowers them itself. `x > 4` is `(> |x| 4.0)`, and `a == b +/- t` is two
//! bounds `and`-ed rather than `(<= (babel_max …) 0.0)` — an `ite` where a
//! conjunction was meant. Both are easier for a solver and for a reader.
//!
//! # Side conditions
//!
//! Some operations need more than a term. `(/ a b)` needs `b` pinned away from
//! zero, because SMT-LIB leaves division by zero *underspecified* — the solver
//! is free to choose whatever value for it satisfies the constraint, and it
//! does: the first document this emitter handed Z3 came back solved with every
//! variable zero, satisfying `x2 == x1 + x2/2 - x3/x4` through `0/0`. Babel
//! evaluates that point to NaN and rejects it, so the solver's answer would have
//! been binned and the search would have spun. With the guard, the same document
//! yields a real point.
//!
//! `sqrt` and `cbrt` need an auxiliary variable: there is no `sqrt` in QF_NRA,
//! but `y >= 0 and y*y = x` says the same thing, and a solver handles it
//! comfortably.
//!
//! Both need the translator to contribute *commands* alongside its term, which
//! is why the walk carries an accumulator rather than being a pure
//! `Expr -> Sexp`.
//!
//! # Parentheses
//!
//! Nothing here writes a parenthesis. Terms are built as [`Sexp`] and rendered
//! by its `Display`, so an unbalanced document is not a bug to be caught but a
//! state that cannot be represented — which matters more than it sounds, since
//! `Solver::from_string` reports a syntax error by silently accepting nothing
//! and then answering `sat`. See [`crate::cvg::sexp`] for why this is a type
//! rather than the `lisp!` macro it looks like it should be.
//!
//! # What cannot be emitted, and why that is reported rather than dropped
//!
//! This targets `QF_NIRA`: the field operations, comparison, `ite`, `let`, and
//! `to_int`/`to_real`. Anything outside that — the transcendentals, `log`, a
//! power with a non-integer exponent — is *reported* through
//! [`Document::untranslated`].
//!
//! The JVM version dropped such constraints silently, and its own fixture pinned
//! the consequence: `sin of value offset by multiples of pi` asserted that the
//! returned points included ones which **did not satisfy the constraints**. A
//! solver answering a question you quietly did not ask is worse than a solver
//! that says it cannot help.
//!
//! The list is shorter than it was, because some of those gaps were dialect
//! rather than theory. `floor`, `ceil` and `%` used to be on it and are not: the
//! first two are `to_int` and the third is `a - b*trunc(a/b)`, and the only
//! price is the wider logic. What is left is genuinely missing rather than
//! merely unwritten — Z3 has no logarithm under any spelling, and its `sin`
//! parses and then answers `unknown`. dReal has all of them as primitives, at
//! the cost of a subprocess on a customer's machine.
//!
//! # Fidelity
//!
//! Literals are emitted as shortest round-trip decimals, which is *not* exact:
//! SMT-LIB reals are exact decimals, so `0.1` there means one tenth, while the
//! `f64` spelled `0.1` is 0.1000000000000000055511151231257827.
//!
//! Emitting exact dyadic rationals instead would close that particular gap and
//! is a one-function change — but it would not make the model exact, because the
//! far larger gap is that SMT reasons in *real arithmetic* while babel evaluates
//! in `f64`. Every operation rounds; `x^3` as `(* x x x)` need not equal
//! `x.powf(3.0)` in the last place. The model is a real-arithmetic idealisation
//! of the computation, deliberately — and a solver's points are filtered through
//! babel's own `evaluate` before anybody sees them. Exact literals would be false
//! precision about everything else.

use crate::ast::{AggregateKind, BinaryOp, Block, CompareOp, Expr, Kind, UnaryOp};
use crate::cvg::InputVariable;
use crate::cvg::sexp::{Sexp, define_fun, sexp};
use crate::{Ast, ast};

/// Which SMT-LIB logic a document declares.
///
/// Defaults to `QF_NIRA`, which is what the prelude needs — see the comment on
/// the `set-logic` line in [`emit`]. Overridable because the right answer is a
/// property of the backend and of what the constraints happen to use, and
/// neither is fixed: a document with no `to_int` in it would be honest as
/// `QF_NRA`, and a future backend may want `ALL` or a dialect of its own.
///
/// Precedence, most specific first: [`ConstraintSolver::with_logic`] beats the
/// `BABEL_SMT_LOGIC` environment variable, which beats `QF_NIRA`. The
/// environment sets the *default* rather than winning outright, so a test that
/// pins the logic still passes on a machine where the variable is set.
///
/// [`ConstraintSolver::with_logic`]: super::ConstraintSolver::with_logic
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtLogic(String);

impl SmtLogic {
    /// The environment variable consulted by [`SmtLogic::default`].
    pub const VARIABLE: &'static str = "BABEL_SMT_LOGIC";

    /// A logic by name. Unvalidated on purpose — the list of logics a solver
    /// accepts is the solver's business, and a name it rejects surfaces
    /// immediately as a parse failure rather than quietly.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

impl SmtLogic {
    /// The default, given whatever the environment said.
    ///
    /// Split out from [`Default`] so it can be tested: mutating a real
    /// environment variable is process-global, and under plain `cargo test`
    /// that races every other test in the binary.
    fn from_variable(value: Option<&str>) -> Self {
        value
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map_or_else(|| Self("QF_NIRA".to_owned()), Self::named)
    }
}

impl Default for SmtLogic {
    fn default() -> Self {
        Self::from_variable(std::env::var(Self::VARIABLE).ok().as_deref())
    }
}

impl std::fmt::Display for SmtLogic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A document under construction: a sequence of lines, not a string.
///
/// [`Sexp`] made an unbalanced document unrepresentable. This closes the other
/// half of the same hole. Building the text with `push_str` means every caller
/// is responsible for its own `\n`, and forgetting one is *usually* harmless —
/// SMT-LIB does not care about whitespace between `(…)` forms, so
/// `(check-sat)(get-model)` parses exactly like the spaced version.
///
/// The exception is what makes this worth a type. A `;` comment runs to the end
/// of the line, so a comment missing its newline **eats the command after it**,
/// and it does so quietly: the parse guard in [`super::smt`] only fires when
/// *every* assertion is lost, and a document that loses one is still a document
/// full of assertions. It would come back `sat` for a question nobody asked.
///
/// So a line is a line by construction. Commands and comments are different
/// variants, the separator belongs to the renderer, and `comment` collapses its
/// own whitespace — a babel source string may legally contain newlines, and one
/// reaching the output verbatim would break out of its comment and be read as
/// commands.
#[derive(Debug, Default)]
struct Script(Vec<Line>);

#[derive(Debug)]
enum Line {
    Command(Sexp),
    Comment(String),
}

impl Script {
    fn command(&mut self, command: Sexp) -> &mut Self {
        self.0.push(Line::Command(command));
        self
    }

    /// A `;` comment. Interior whitespace — newlines included — collapses to
    /// single spaces, which is what keeps the text from escaping the comment.
    fn comment(&mut self, text: &str) -> &mut Self {
        self.0.push(Line::Comment(
            text.split_whitespace().collect::<Vec<_>>().join(" "),
        ));
        self
    }
}

impl std::fmt::Display for Script {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for line in &self.0 {
            match line {
                Line::Command(command) => writeln!(f, "{command}")?,
                Line::Comment(text) => writeln!(f, "; {text}")?,
            }
        }
        Ok(())
    }
}

/// Why a constraint could not be handed to a solver.
///
/// The pool's response to any of these is the same — fall back to sampling —
/// and that is the problem this type exists for. A caller who watches their
/// generator crawl on a narrow region deserves to know it crawled because
/// nothing could be asked of the solver, and which part of their expression was
/// responsible. Silence here was the JVM implementation's worst habit.
///
/// Reported through `tracing` at `INFO` rather than returned, because there is
/// nothing the caller can *do* differently — sampling is already the fallback —
/// and an error would imply otherwise.
///
/// **It names the first construct the walk could not render, not the only one.**
/// The walk descends before it refuses, so the reason is the innermost failure:
/// `x1 > sin(ln(cos(2.1^x1)))` is reported against its `2.1^x1`, which is true
/// and is not the whole story. Innermost is the better default — it names an
/// actual construct rather than whatever contained it — but a reader should not
/// read one reason as an exhaustive account of a deeply nested expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// A scalar expression, which has no `<= 0` reading to assert.
    NotABooleanExpression,
    /// `sin`, `cos`, `tan` and the rest. Z3 parses them and then answers
    /// `unknown`; `rewrite::invert_monotone` could not rewrite this one away
    /// because the function is not monotone, or because the variable appears on
    /// both sides of the comparison.
    Transcendental(&'static str),
    /// A logarithm `rewrite::invert_monotone` could not turn into a bound —
    /// a variable base, or one nested where there is no comparison to invert.
    Logarithm,
    /// An exponent that is neither a constant whole number nor invertible.
    RealExponent,
    /// `var[i]` with a computed subscript: a load from a row the solver has no
    /// model of.
    ComputedSubscript,
    /// A literal that is not a finite `Real`. Unreachable while
    /// `rewrite::fold_constants` refuses non-finite constants, and kept so the
    /// emitter does not depend on that from a distance.
    NonFiniteLiteral,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotABooleanExpression => {
                f.write_str("it is a scalar expression, not a constraint")
            }
            Self::Transcendental(function) => write!(
                f,
                "`{function}` is outside the solver's theory, and could not be \
                 rewritten away — it is not monotone, or its variable appears on \
                 both sides of the comparison"
            ),
            Self::Logarithm => f.write_str(
                "the solver has no logarithm, and this one could not be inverted \
                 into a bound — its base varies, or it is nested where there is \
                 no comparison to invert against",
            ),
            Self::RealExponent => {
                f.write_str("the exponent is neither a constant whole number nor invertible")
            }
            Self::ComputedSubscript => {
                f.write_str("`var[i]` with a computed subscript is a load the solver cannot model")
            }
            Self::NonFiniteLiteral => {
                f.write_str("it contains a literal that is not a finite real")
            }
        }
    }
}

/// An SMT-LIB2 document, and an honest account of what it left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Document {
    pub(crate) text: String,
    /// Indices into the constraints given, for those that could not be
    /// expressed. A solver's verdict says nothing about these, so a caller that
    /// ignores this list will over-trust the answer — which is why it is not an
    /// `Option` or a log line.
    pub(crate) untranslated: Vec<usize>,
}

/// Helpers for the operations SMT-LIB lacks for `Real`.
///
/// Defined once rather than inlined because each is an `ite` that would
/// otherwise duplicate its argument two or three times, and nesting a few of
/// those turns a small expression into an enormous one.
///
/// Built rather than written out, so the prelude gets the same balance
/// guarantee as everything else. Each entry is
/// `(name, parameters, body-of-x-and-maybe-y)`.
fn prelude() -> Vec<Sexp> {
    vec![
        define_fun!(babel_abs (x Real) -> Real: (ite (< x 0.0) (- x) x)),
        define_fun!(babel_sqr (x Real) -> Real: (* x x)),
        define_fun!(babel_cube (x Real) -> Real: (* x x x)),
        define_fun!(babel_max (a Real) (b Real) -> Real: (ite (>= a b) a b)),
        define_fun!(babel_min (a Real) (b Real) -> Real: (ite (<= a b) a b)),
        // Babel's `sgn` follows Java: zero maps to zero. Rust's `f64::signum`
        // gives 1.0 there, so this has to encode babel's version and not the
        // host's, and `the_prelude_helpers_mean_what_they_say` pins it.
        define_fun!(babel_sgn (x Real) -> Real:
            (ite (< x 0.0) (- 1.0) (ite (> x 0.0) 1.0 0.0))),
        // `to_int` is floor and not truncation, negatives included: `to_int`
        // of -2.7 is -3. That is what lets `babel_ceil` work by reflection,
        // and it is why `%` below cannot be written over `babel_floor`.
        define_fun!(babel_floor (x Real) -> Real: (to_real (to_int x))),
        define_fun!(babel_ceil (x Real) -> Real: (- (to_real (to_int (- x))))),
        // `babel_trunc` is not reachable from babel source; it exists because
        // `%` needs it. Babel's `%` is a *remainder* and not a modulo, and the
        // difference is the sign: a remainder takes the dividend's, so `-7 % 3`
        // is -1, where a modulo takes the divisor's and answers 2. Java's `%`
        // and Rust's agree on the remainder, which is why `BinaryOp::Rem::apply`
        // can be a bare `a % b` and why this cannot be written over
        // `babel_floor`.
        define_fun!(babel_trunc (x Real) -> Real:
            (ite (>= x 0.0) (babel_floor x) (babel_ceil x))),
        define_fun!(babel_rem (a Real) (b Real) -> Real:
            (- a (* b (babel_trunc (/ a b))))),
    ]
}

pub(crate) fn emit(inputs: &[InputVariable], constraints: &[Ast], logic: &SmtLogic) -> Document {
    let mut script = Script::default();

    // Before `set-logic`, which is where SMT-LIB wants options. Asking for cores
    // up front costs nothing on a satisfiable query and is the only way to learn
    // *which* constraints conflict on an unsatisfiable one.
    script.command(Sexp::call(
        "set-option",
        [Sexp::atom(":produce-unsat-cores"), Sexp::atom("true")],
    ));
    // `QF_NIRA` by default rather than `QF_NRA`, because the prelude reaches for
    // `to_int` and `to_real`, which SMT-LIB puts in the `Reals_Ints` theory.
    // Measured: under `QF_NRA` Z3 rejects such a document outright — which the
    // parse guard in `Z3Backend::solve` would at least report as an error rather
    // than a wrong answer, but rejecting every document is not a plan. Widening
    // costs nothing on the polynomial cases: the same prelude and the same
    // constraint solve identically under either name.
    script.command(Sexp::call("set-logic", [Sexp::atom(logic.to_string())]));
    for definition in prelude() {
        script.command(definition);
    }

    for input in inputs {
        script.command(Sexp::call(
            "declare-const",
            [Sexp::symbol(&input.name), Sexp::atom("Real")],
        ));
    }
    for input in inputs {
        // Skipped rather than encoded as an unsatisfiable bound: a non-finite
        // range is the caller's problem to notice, not something to assert.
        if let (Some(low), Some(high)) = (real(input.lower_bound), real(input.upper_bound)) {
            let name = Sexp::symbol(&input.name);
            script.command(Sexp::call(
                "assert",
                [Sexp::call(
                    "and",
                    [
                        Sexp::call(">=", [name.clone(), low]),
                        Sexp::call("<=", [name, high]),
                    ],
                )],
            ));
        }
    }

    let mut untranslated = Vec::new();
    for (index, constraint) in constraints.iter().enumerate() {
        script.comment(constraint.source());

        match translate(constraint, index, inputs) {
            Ok(assertion) => {
                for condition in assertion.conditions {
                    script.command(condition);
                }
                // Named so that `(get-unsat-core)` can point back at the
                // constraint rather than at an anonymous term.
                script.command(Sexp::call(
                    "assert",
                    [Sexp::call(
                        "!",
                        [
                            assertion.claim,
                            Sexp::atom(":named"),
                            Sexp::atom(core_name(index)),
                        ],
                    )],
                ));
            }
            Err(reason) => {
                untranslated.push(index);
                script.comment(&format!("  NOT TRANSLATED - {reason}"));
                // The pool's answer to this is to fall back on sampling, and it
                // does so without saying anything. On a narrow region that is
                // the difference between a fast answer and a slow one, so the
                // caller is told which constraint and why — see `Refusal`.
                tracing::info!(
                    constraint = %constraint.source(),
                    %reason,
                    "no solver can be asked about this constraint; sampling must find it unaided"
                );
            }
        }
    }

    script.command(Sexp::call("check-sat", []));
    script.command(Sexp::call("get-model", []));

    Document {
        text: script.to_string(),
        untranslated,
    }
}

/// The two name lists an expression resolves against, which are not the same
/// list.
///
/// `Global` holds an index into the expression's own statically-referenced
/// symbols, because the AST is built before any schema exists. `var[i]` holds a
/// one-based index into the *schema* — declaration order over every input,
/// including ones this expression never names. Using either for the other
/// resolves to the wrong variable silently, so both are carried explicitly.
struct Names<'a> {
    symbols: &'a [String],
    inputs: &'a [InputVariable],
    /// Which constraint is being translated. Only used to keep auxiliary names
    /// distinct across a document — two constraints each declaring `aux0` is a
    /// redeclaration, and the solver rejects the whole thing.
    constraint: usize,
    /// Complete SMT-LIB commands the term depends on: divisor guards, and the
    /// declarations and defining assertions of auxiliary variables. Written out
    /// immediately before the assertion that uses the term.
    conditions: Vec<Sexp>,
    auxiliaries: usize,
    /// Why the walk gave up, set at whichever site returned `None` first.
    ///
    /// An accumulator field rather than a return type because the walk is
    /// already `Option`-shaped and threading a reason through every `?` would
    /// cost more than it explains. `refuse` below is the only writer.
    refused: Option<Refusal>,
}

/// The name an assertion is tagged with, so an unsat core can be read back as
/// constraint indices.
fn core_name(index: usize) -> String {
    format!("c{index}")
}

/// Parses [`core_name`] back. Lives next to it so the two cannot drift.
pub(crate) fn core_index(name: &str) -> Option<usize> {
    name.strip_prefix('c')?.parse().ok()
}

/// One constraint, ready to assert.
struct Assertion {
    conditions: Vec<Sexp>,
    /// The constraint as a boolean term, ready to assert.
    ///
    /// It used to be a *residual* plus a relation to compare it against zero,
    /// because that is all the front end left behind. Now that `Kind::Compare`
    /// survives compilation this is `(> x 5.0)` rather than
    /// `(< (- 5.0 x) 0.0)` — the thing the author wrote.
    claim: Sexp,
}

/// One constraint as a boolean term and its side conditions, or the reason it
/// could not be written down.
fn translate(
    constraint: &Ast,
    index: usize,
    inputs: &[InputVariable],
) -> Result<Assertion, Refusal> {
    // A scalar expression has no `<= 0` reading, so asserting one would invent a
    // constraint the user did not write.
    if !constraint.is_constraint {
        return Err(Refusal::NotABooleanExpression);
    }
    let mut names = Names {
        symbols: &constraint.symbols,
        inputs,
        constraint: index,
        conditions: Vec::new(),
        auxiliaries: 0,
        refused: None,
    };

    // The walk is `Option`-shaped; `Names::refused` carries the reason it
    // stopped. This pairs the two back up at the one place that needs both.
    macro_rules! rendered {
        ($walk:expr) => {
            match $walk {
                Some(rendered) => rendered,
                None => {
                    return Err(names
                        .refused
                        .expect("a walk that returned None recorded why"));
                }
            }
        };
    }

    let claim = rendered!(names.claim(&constraint.program.body));
    Ok(Assertion {
        conditions: names.conditions,
        claim,
    })
}

impl Names<'_> {
    /// Records why the walk is stopping and stops it.
    ///
    /// Mutating and returning `None` is the whole of its job, which is why it
    /// is called at the point of refusal rather than hidden inside something
    /// else. The first reason wins: it is the innermost, and therefore the one
    /// naming the actual construct rather than whatever contained it.
    fn refuse(&mut self, reason: Refusal) -> Option<Sexp> {
        self.refused.get_or_insert(reason);
        None
    }

    /// A constraint's block as a boolean term.
    ///
    /// Mirrors [`block`](Self::block), which renders the scalar half. The two
    /// are separate because the grammar keeps them separate: a boolean is the
    /// root of a constraint and never an operand, so exactly one node in the
    /// tree needs this treatment and every node below it needs the other.
    fn claim(&mut self, body: &Block) -> Option<Sexp> {
        let mut rendered = self.boolean(&body.result)?;
        for assignment in body.assignments.iter().rev() {
            let value = self.expression(&assignment.value)?;
            let binding = Sexp::list([Sexp::atom(local(assignment.slot.index())), value]);
            rendered = Sexp::call("let", [Sexp::list([binding]), rendered]);
        }
        Some(rendered)
    }

    /// One boolean node, rendered as the comparison the author wrote.
    ///
    /// Everything here used to arrive as arithmetic: `x > 5` as
    /// `(< (- 5.0 x) 0.0)` with a denormal standing in for strictness, and
    /// `a == b +/- t` as `(<= (babel_max …) 0.0)` — an `ite` where a
    /// conjunction was meant. A solver is markedly better at the direct form,
    /// and a reader is too.
    fn boolean(&mut self, expr: &Expr) -> Option<Sexp> {
        match &expr.kind {
            Kind::Compare { op, lhs, rhs } => {
                let left = self.expression(lhs)?;
                let right = self.expression(rhs)?;
                let relation = match op {
                    CompareOp::Lt => "<",
                    CompareOp::Lte => "<=",
                    CompareOp::Gt => ">",
                    CompareOp::Gte => ">=",
                };
                Some(Sexp::call(relation, [left, right]))
            }

            // `|a - b| <= t`, as two bounds rather than one `max`. SMT-LIB has
            // no absolute value, and a conjunction of linear bounds is the
            // easiest shape a solver can be given.
            Kind::NearEq {
                lhs,
                rhs,
                tolerance,
            } => {
                let left = self.expression(lhs)?;
                let right = self.expression(rhs)?;
                let bound = match real(*tolerance) {
                    Some(rendered) => rendered,
                    None => return self.refuse(Refusal::NonFiniteLiteral),
                };
                let difference = Sexp::call("-", [left, right]);
                Some(Sexp::call(
                    "and",
                    [
                        Sexp::call("<=", [difference.clone(), bound.clone()]),
                        Sexp::call(">=", [difference, Sexp::call("-", [bound])]),
                    ],
                ))
            }

            Kind::And { terms } => {
                let mut rendered = Vec::with_capacity(terms.len());
                for term in terms {
                    rendered.push(self.boolean(term)?);
                }
                Some(Sexp::call("and", rendered))
            }

            // The grammar puts a boolean at the root of a constraint and
            // nowhere else, so anything else here is a scalar where a truth
            // value was wanted.
            _ => self.refuse(Refusal::NotABooleanExpression),
        }
    }

    fn block(&mut self, body: &Block) -> Option<Sexp> {
        let mut rendered = self.expression(&body.result)?;
        // Innermost first, so that earlier assignments end up in outer `let`s
        // and stay visible to the later ones.
        for assignment in body.assignments.iter().rev() {
            let value = self.expression(&assignment.value)?;
            let binding = Sexp::list([Sexp::atom(local(assignment.slot.index())), value]);
            rendered = Sexp::call("let", [Sexp::list([binding]), rendered]);
        }
        Some(rendered)
    }

    fn expression(&mut self, expr: &Expr) -> Option<Sexp> {
        match &expr.kind {
            Kind::Literal(value) => match real(*value) {
                Some(rendered) => Some(rendered),
                None => self.refuse(Refusal::NonFiniteLiteral),
            },
            Kind::Global(id) => Some(Sexp::symbol(self.symbols.get(id.index())?)),
            Kind::Local(slot) => Some(Sexp::atom(local(slot.index()))),

            // A literal subscript names a variable, so it resolves here — and it
            // resolves against the schema, not against this expression's own
            // symbols. A computed one cannot: `var[i]` is a load from a row the
            // solver has no model of.
            Kind::DynamicIndex(index) => match index.kind {
                Kind::Literal(value) => {
                    let one_based = ast::to_index(value)?;
                    let position = usize::try_from(one_based.checked_sub(1)?).ok()?;
                    Some(Sexp::symbol(&self.inputs.get(position)?.name))
                }
                _ => self.refuse(Refusal::ComputedSubscript),
            },

            Kind::Unary { op, arg } => {
                let rendered = self.expression(arg)?;
                self.unary(*op, &rendered)
            }
            Kind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs),

            Kind::Fold { kind, terms } => {
                let mut rendered = Vec::with_capacity(terms.len());
                for term in terms {
                    rendered.push(self.expression(term)?);
                }
                let operator = match kind {
                    AggregateKind::Sum => "+",
                    AggregateKind::Prod => "*",
                };
                Some(match rendered.len() {
                    // SMT-LIB's `+` and `*` want at least two arguments.
                    0 => real(kind.identity())?,
                    1 => rendered.into_iter().next()?,
                    _ => Sexp::call(operator, rendered),
                })
            }

            Kind::Block(inner) => self.block(inner),

            // The front end unrolls every aggregate or refuses the expression;
            // one reaching a backend is a bug in the passes, not a formulation.
            Kind::Aggregate { .. } => {
                unreachable!("`unroll_aggregates` leaves no aggregate for a backend to see")
            }

            // A boolean is the root of a constraint and never an operand, so
            // the scalar walk cannot meet one. `lambdaExpr` takes a
            // `scalarBlock`, which makes that a fact about the grammar rather
            // than a hope about what users write.
            Kind::Compare { .. } | Kind::NearEq { .. } | Kind::And { .. } => {
                unreachable!("the grammar keeps booleans out of scalar position")
            }
        }
    }

    fn binary(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr) -> Option<Sexp> {
        let left = self.expression(lhs)?;
        let right = self.expression(rhs)?;
        Some(match op {
            BinaryOp::Add => Sexp::call("+", [left, right]),
            BinaryOp::Sub => Sexp::call("-", [left, right]),
            BinaryOp::Mul => Sexp::call("*", [left, right]),
            BinaryOp::Div => {
                // Without this the solver may satisfy the constraint *through*
                // the division, because SMT-LIB does not say what `x/0` is.
                self.conditions.push(Sexp::call(
                    "assert",
                    [Sexp::call(
                        "not",
                        [Sexp::call("=", [right.clone(), Sexp::atom("0.0")])],
                    )],
                ));
                Sexp::call("/", [left, right])
            }
            BinaryOp::Max => Sexp::call("babel_max", [left, right]),
            BinaryOp::Min => Sexp::call("babel_min", [left, right]),

            // SMT-LIB's own `mod` is integer-only, so this goes through
            // `babel_rem` — `a - b*trunc(a/b)`, which keeps Java's sign rule.
            // The guard is the one division needs and for the same reason:
            // `a % 0` is NaN in babel and the pool bins NaN residuals, but
            // SMT-LIB leaves `/0` underspecified, so without it a solver may
            // satisfy the constraint *through* a zero divisor and hand back a
            // point that is then thrown away.
            BinaryOp::Rem => {
                self.conditions.push(Sexp::call(
                    "assert",
                    [Sexp::call(
                        "not",
                        [Sexp::call("=", [right.clone(), Sexp::atom("0.0")])],
                    )],
                ));
                Sexp::call("babel_rem", [left, right])
            }

            // Both are real-exponent problems by the time they reach here: the
            // constant cases were dealt with earlier in the pipeline.
            //
            // `rewrite::expand_powers` turns every constant whole exponent into
            // multiplication, so a `^` still spelled as one has a real or a
            // variable exponent — `exp(n * ln x)`, and there is no `exp`.
            // `rewrite::invert_monotone` turns `log(a, u) op c` for a constant
            // base into a bound on `u`, so a `log` still spelled as one has a
            // variable base, or sits inside another function where there was no
            // comparison to invert against.
            //
            // What is left needs a logarithm, and Z3 has none under any
            // spelling. Neither does cvc5.
            BinaryOp::LogB => return self.refuse(Refusal::Logarithm),
            BinaryOp::Pow => return self.refuse(Refusal::RealExponent),
        })
    }

    /// A fresh auxiliary variable, declared and returned by name.
    fn fresh_auxiliary(&mut self) -> Sexp {
        let name = Sexp::atom(format!("aux_{}_{}", self.constraint, self.auxiliaries));
        self.auxiliaries += 1;
        self.conditions.push(Sexp::call(
            "declare-const",
            [name.clone(), Sexp::atom("Real")],
        ));
        name
    }

    fn unary(&mut self, op: UnaryOp, arg: &Sexp) -> Option<Sexp> {
        Some(match op {
            UnaryOp::Negate => Sexp::call("-", [arg.clone()]),
            UnaryOp::Abs => Sexp::call("babel_abs", [arg.clone()]),
            UnaryOp::Sqr => Sexp::call("babel_sqr", [arg.clone()]),
            UnaryOp::Cube => Sexp::call("babel_cube", [arg.clone()]),
            UnaryOp::Sgn => Sexp::call("babel_sgn", [arg.clone()]),

            // QF_NRA has no `sqrt`, but `y >= 0 and y*y = x` says the same. It
            // also gets the domain right for free: for a negative `x` there is
            // no such `y`, which is exactly babel's NaN.
            UnaryOp::Sqrt => {
                let name = self.fresh_auxiliary();
                self.conditions.push(Sexp::call(
                    "assert",
                    [Sexp::call(">=", [name.clone(), Sexp::atom("0.0")])],
                ));
                self.conditions.push(Sexp::call(
                    "assert",
                    [Sexp::call(
                        "=",
                        [Sexp::call("*", [name.clone(), name.clone()]), arg.clone()],
                    )],
                ));
                name
            }
            // No sign constraint: the cubic is one-to-one over the reals, so it
            // pins a negative root as readily as a positive one.
            UnaryOp::Cbrt => {
                let name = self.fresh_auxiliary();
                self.conditions.push(Sexp::call(
                    "assert",
                    [Sexp::call(
                        "=",
                        [
                            Sexp::call("*", [name.clone(), name.clone(), name.clone()]),
                            arg.clone(),
                        ],
                    )],
                ));
                name
            }

            UnaryOp::Floor => Sexp::call("babel_floor", [arg.clone()]),
            UnaryOp::Ceil => Sexp::call("babel_ceil", [arg.clone()]),

            // The transcendentals are dReal primitives and Z3 non-starters:
            // `sin` and friends parse and then answer `unknown` on anything
            // narrow enough to be worth asking, and `ln` is not a symbol at
            // all. Reported rather than dropped — see `Document::untranslated`.
            UnaryOp::Ln
            | UnaryOp::Log10
            | UnaryOp::Sin
            | UnaryOp::Cos
            | UnaryOp::Tan
            | UnaryOp::Asin
            | UnaryOp::Acos
            | UnaryOp::Atan
            | UnaryOp::Sinh
            | UnaryOp::Cosh
            | UnaryOp::Tanh
            | UnaryOp::Cot => return self.refuse(Refusal::Transcendental(name_of(op))),
        })
    }
}

/// A transcendental's name as the author wrote it, for the refusal message.
///
/// Only the ones that can be refused: everything else is rendered rather than
/// named, so a name would be dead weight.
fn name_of(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Ln => "ln",
        UnaryOp::Log10 => "log",
        UnaryOp::Sin => "sin",
        UnaryOp::Cos => "cos",
        UnaryOp::Tan => "tan",
        UnaryOp::Asin => "asin",
        UnaryOp::Acos => "acos",
        UnaryOp::Atan => "atan",
        UnaryOp::Sinh => "sinh",
        UnaryOp::Cosh => "cosh",
        UnaryOp::Tanh => "tanh",
        UnaryOp::Cot => "cot",
        _ => "an unsupported function",
    }
}

/// A local slot's name inside a `let`.
fn local(slot: usize) -> String {
    format!("l{slot}")
}

/// An `f64` as an SMT-LIB `Real` literal.
///
/// Three things this has to get right that `format!("{value}")` does not: a
/// `Real` literal must carry a decimal point (`1` is an `Int`, and SMT-LIB will
/// not mix the sorts); a negative number is the *operator* `-` applied to a
/// literal, which is why this returns an [`Sexp`] and not an atom; and exponent
/// notation is not a `Real` literal at all, so `1e300` has to be written out.
///
/// `None` for infinities and NaN, which have no `Real` to be.
fn real(value: f64) -> Option<Sexp> {
    if !value.is_finite() {
        return None;
    }
    let digits = Sexp::atom(decimal(value.abs())?);
    Some(if value.is_sign_negative() {
        Sexp::call("-", [digits])
    } else {
        digits
    })
}

fn decimal(magnitude: f64) -> Option<String> {
    // `{:?}` gives the shortest representation that round-trips and always
    // includes a decimal point — but it uses exponent notation at the extremes.
    let shortest = format!("{magnitude:?}");
    if !shortest.contains(['e', 'E']) {
        return Some(shortest);
    }

    // Widen a fixed-point rendering until it round-trips. Bounded by the widest
    // an `f64` can need, which is the smallest subnormal at about 1e-324.
    let padded = [20usize, 40, 80, 160, 340, 700, 1_100]
        .into_iter()
        .map(|places| format!("{magnitude:.places$}"))
        .find(|rendered| rendered.parse::<f64>() == Ok(magnitude))?;

    // Drop the zeros the fixed width padded on, so `1e-5` reads as `0.00001`
    // rather than `0.00001000000000000000`. The decimal point stops the trim
    // from eating an integer's own zeros, and one digit is put back so the
    // literal stays a `Real` rather than becoming an `Int`.
    let trimmed = padded.trim_end_matches('0');
    let candidate = if trimmed.ends_with('.') {
        format!("{trimmed}0")
    } else {
        trimmed.to_owned()
    };
    Some(if candidate.parse::<f64>() == Ok(magnitude) {
        candidate
    } else {
        padded
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `real` yields a term now; these tests are about how it renders.
    fn render(value: f64) -> Option<String> {
        real(value).map(|term| term.to_string())
    }

    fn document(variables: &[(&str, f64, f64)], sources: &[&str]) -> Document {
        let inputs: Vec<InputVariable> = variables
            .iter()
            .map(|(name, low, high)| InputVariable::new(*name, *low, *high))
            .collect();
        let constraints: Vec<Ast> = sources
            .iter()
            .map(|source| crate::parse(source).expect("test constraint should compile"))
            .collect();
        emit(&inputs, &constraints, &SmtLogic::default())
    }

    /// The prelude is a hand-written table and nothing else pins what is *in*
    /// it. Swap `babel_min`'s `<=` for `>=` and the document still parses, Z3
    /// still solves it, and every other test in the crate still passes — because
    /// no case in the corpus exercises `min` through a solver. These are the
    /// assertions that would not survive it.
    ///
    /// Each claim is closed (no free variables), so Z3 answering `sat` means the
    /// claim is true and `unsat` means it is false.
    #[test]
    fn the_prelude_helpers_mean_what_they_say() {
        use crate::cvg::smt::{Outcome, SmtBackend, Z3Backend};

        let claims = [
            ("(= (babel_abs (- 3.0)) 3.0)", true),
            ("(= (babel_abs 3.0) 3.0)", true),
            ("(= (babel_abs 0.0) 0.0)", true),
            ("(= (babel_abs (- 3.0)) (- 3.0))", false),
            ("(= (babel_sqr (- 3.0)) 9.0)", true),
            ("(= (babel_cube (- 2.0)) (- 8.0))", true),
            ("(= (babel_cube 2.0) 8.0)", true),
            // The pair most worth pinning: they differ only in one character,
            // and swapping them is invisible everywhere else.
            ("(= (babel_max 2.0 5.0) 5.0)", true),
            ("(= (babel_max 5.0 2.0) 5.0)", true),
            ("(= (babel_min 2.0 5.0) 2.0)", true),
            ("(= (babel_min 5.0 2.0) 2.0)", true),
            ("(= (babel_max 2.0 5.0) 2.0)", false),
            ("(= (babel_min 2.0 5.0) 5.0)", false),
            // Babel's `sgn` follows Java: zero maps to zero. Rust's
            // `f64::signum` returns 1.0 there, so this is a real divergence and
            // the emitter has to encode babel's version, not the host's.
            ("(= (babel_sgn (- 4.0)) (- 1.0))", true),
            ("(= (babel_sgn 4.0) 1.0)", true),
            ("(= (babel_sgn 0.0) 0.0)", true),
            ("(= (babel_sgn 0.0) 1.0)", false),
            // `to_int` is floor, so the negative cases are where a plausible
            // wrong encoding shows up. Truncation would give -2.0 here.
            ("(= (babel_floor 2.7) 2.0)", true),
            ("(= (babel_floor (- 2.7)) (- 3.0))", true),
            ("(= (babel_floor (- 2.7)) (- 2.0))", false),
            ("(= (babel_floor 3.0) 3.0)", true),
            ("(= (babel_ceil 2.3) 3.0)", true),
            ("(= (babel_ceil (- 2.3)) (- 2.0))", true),
            ("(= (babel_ceil (- 2.3)) (- 3.0))", false),
            ("(= (babel_ceil 3.0) 3.0)", true),
            // The other half of the same risk. Babel's `%` is Java's, so the
            // sign follows the dividend: floored modulo would answer 2.0 to
            // the third of these and 0.5 to the fifth.
            ("(= (babel_rem 10.0 4.5) 1.0)", true),
            ("(= (babel_rem 7.0 3.0) 1.0)", true),
            ("(= (babel_rem (- 7.0) 3.0) (- 1.0))", true),
            ("(= (babel_rem (- 7.0) 3.0) 2.0)", false),
            ("(= (babel_rem 7.0 (- 3.0)) 1.0)", true),
            ("(= (babel_rem 7.5 2.5) 0.0)", true),
        ];

        let preamble: String = prelude().iter().map(|line| format!("{line}\n")).collect();
        for (claim, should_hold) in claims {
            let document = format!("(set-logic QF_NIRA)\n{preamble}(assert {claim})\n");
            let outcome = Z3Backend
                .solve(&document, 0)
                .unwrap_or_else(|e| panic!("Z3 rejected {claim}: {e}"));

            let held = matches!(outcome, Outcome::Sat(_));
            assert_eq!(
                held, should_hold,
                "{claim} should have been {should_hold}, Z3 said {held}"
            );
        }
    }

    #[test]
    fn a_whole_document() {
        let rendered = document(&[("x", 0.0, 10.0)], &["x > 4"]);
        // Joined, not newline-terminated, so the golden below keeps its own.
        let preamble: String = prelude()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(
                "
",
            );

        assert_eq!(rendered.untranslated, Vec::<usize>::new());
        assert_eq!(
            rendered.text,
            format!(
                "(set-option :produce-unsat-cores true)\n\
                 (set-logic QF_NIRA)\n{preamble}\n\
                 (declare-const |x| Real)\n\
                 (assert (and (>= |x| 0.0) (<= |x| 10.0)))\n\
                 ; x > 4\n\
                 (assert (! (> |x| 4.0) :named c0))\n\
                 (check-sat)\n(get-model)\n"
            )
        );
    }

    #[test]
    fn a_comparison_is_emitted_as_a_comparison() {
        // It used to arrive as the residual `(< (- 4.0 |x|) 0.0)`, because the
        // front end had already flattened it on the evaluator's behalf. The
        // solver now gets what the author wrote.
        let rendered = document(&[("x", 0.0, 10.0)], &["x > 4"]);
        assert!(
            rendered.text.contains("(> |x| 4.0)"),
            "expected a plain comparison:
{}",
            rendered.text
        );
    }

    #[test]
    fn strictness_is_native_and_no_denormal_exists_to_leak() {
        // `eval` marks `<` by adding `f64::MIN_POSITIVE`, relying on rounding
        // that real arithmetic does not do. This used to be *the emitter's*
        // problem: recognise that marker and undo it, or put ~310 digits in the
        // document and still mean the wrong thing. The epsilon never reaches
        // here now, because strictness is the relation.
        let strict = document(&[("x", 0.0, 10.0)], &["x > 4"]);
        let loose = document(&[("x", 0.0, 10.0)], &["x >= 4"]);

        assert!(strict.text.contains("(assert (! (> |x| 4.0) :named c0))"));
        assert!(loose.text.contains("(assert (! (>= |x| 4.0) :named c0))"));
        assert!(
            !strict.text.contains("0.0000000000"),
            "a denormal reached the document: {}",
            strict.text
        );
    }

    #[test]
    fn literals_carry_a_point_and_negatives_are_applications() {
        assert_eq!(render(1.0).as_deref(), Some("1.0"));
        assert_eq!(render(0.0001).as_deref(), Some("0.0001"));
        // Not `-1.5`: in SMT-LIB the minus is an operator, not part of a literal.
        assert_eq!(render(-1.5).as_deref(), Some("(- 1.5)"));
        assert_eq!(real(f64::NAN), None);
        assert_eq!(real(f64::INFINITY), None);
    }

    #[test]
    fn extreme_magnitudes_avoid_exponent_notation() {
        // `1e300` is not an SMT-LIB `Real` literal, so it has to be written out.
        for value in [1e300, 1e-300, f64::MIN_POSITIVE, f64::MAX] {
            let rendered = render(value).expect("finite values render");
            assert!(
                !rendered.contains('e') && !rendered.contains('E'),
                "{value} rendered with an exponent: {rendered}"
            );
            assert_eq!(
                rendered.parse::<f64>(),
                Ok(value),
                "{value} did not round-trip"
            );
        }
    }

    #[test]
    fn an_unrolled_aggregate_becomes_one_n_ary_application() {
        // Why `Kind::Fold` is n-ary: it maps onto SMT-LIB's `(+ a b c)` with no
        // flattening pass in between. Also the case that pins `var[i]` resolving
        // against the schema — this expression names no symbols statically, so
        // resolving the subscript against its own symbol list would find nothing.
        let rendered = document(
            &[("a", 0.0, 1.0), ("b", 0.0, 1.0), ("c", 0.0, 1.0)],
            &["sum(1, 3, i -> var[i]) > 2"],
        );
        assert_eq!(rendered.untranslated, Vec::<usize>::new());
        assert!(
            rendered.text.contains("(+ |a| |b| |c|)"),
            "expected one n-ary sum, got:\n{}",
            rendered.text
        );
    }

    #[test]
    fn assignments_become_nested_lets() {
        let rendered = document(
            &[("x", 0.0, 10.0)],
            &["var a = x * 2; var b = a + 1; b > 3"],
        );
        assert_eq!(rendered.untranslated, Vec::<usize>::new());
        // Earlier bindings outermost, so the later ones can see them.
        let text = &rendered.text;
        let outer = text.find("(let ((l0").expect("first binding");
        let inner = text.find("(let ((l1").expect("second binding");
        assert!(outer < inner, "bindings nested the wrong way:\n{text}");
    }

    #[test]
    fn a_whole_power_arrives_already_multiplied() {
        // The emitter no longer knows what an integer exponent is;
        // `rewrite::expand_powers` turned this into a `Kind::Fold` before the
        // document was built. Asserted here rather than only in `rewrite`,
        // because this is the property the *solver* depends on.
        let rendered = document(&[("x", 0.0, 10.0)], &["x^3 > 2"]);
        assert_eq!(rendered.untranslated, Vec::<usize>::new());
        assert!(
            rendered.text.contains("(* |x| |x| |x|)"),
            "expected repeated multiplication, got:{}",
            rendered.text
        );
    }

    #[test]
    fn a_negative_power_pins_its_base_away_from_zero() {
        // The bug that went out with `emit::power`. It rendered `x^-2` as
        // `(/ 1.0 (* x x))` with no guard, so a solver could satisfy the
        // constraint through `x = 0` — SMT-LIB leaves `/0` underspecified.
        // Expanding in the AST makes it an ordinary `Kind::Binary { Div }`,
        // which picks up the guard every division gets.
        let rendered = document(&[("x", 0.0, 10.0)], &["x^-2 < 1"]);
        assert_eq!(rendered.untranslated, Vec::<usize>::new());
        assert!(
            rendered.text.contains("(assert (not (= (* |x| |x|) 0.0)))"),
            "no divisor guard on a negative power:{}",
            rendered.text
        );
    }

    #[test]
    fn what_cannot_be_expressed_is_reported_not_dropped() {
        // The JVM version dropped these and returned points that did not satisfy
        // them. Each must appear in `untranslated`, and none may produce an
        // assertion.
        for source in [
            // A logarithm whose *base* varies. `log(a, u)` inverts when the
            // base is constant, which is not this.
            "3 > log(x, 2)",
            "sin(x) <= 0",
            "x^x > 2",
            // The variable appears inside a transcendental and outside it.
            // Nothing short of causalization touches this.
            "x > sin(ln(cos(2.1^x)))",
        ] {
            let rendered = document(&[("x", 0.1, 10.0)], &[source]);
            assert_eq!(
                rendered.untranslated,
                vec![0],
                "{source:?} should have been reported as untranslated"
            );
            // Only a translated constraint is named, so this catches a stray
            // assertion without tripping over the side conditions.
            assert!(
                !rendered.text.contains(":named"),
                "{source:?} produced an assertion anyway:\n{}",
                rendered.text
            );
        }
    }

    #[test]
    fn translatable_and_untranslatable_constraints_coexist() {
        // A document is still worth emitting when only part of it can be
        // expressed. It just has to say which part.
        let rendered = document(&[("x", 0.1, 10.0)], &["x > 4", "sin(x) <= 0", "x < 9"]);
        assert_eq!(rendered.untranslated, vec![1]);
        // Named `c0` and `c2`; the gap at `c1` is what keeps an unsat core
        // pointing at the right constraint.
        assert!(rendered.text.contains(":named c0)"));
        assert!(!rendered.text.contains(":named c1)"));
        assert!(rendered.text.contains(":named c2)"));
    }

    #[test]
    fn a_division_pins_its_divisor_away_from_zero() {
        // The bug this exists for. Without the guard, Z3 satisfied
        // `x2 == x1 + x2/2 - x3/x4` by setting every variable to zero and
        // reading `0/0` as whatever it liked; babel evaluates that to NaN and
        // throws the point away.
        let rendered = document(&[("a", 0.0, 1.0), ("b", 0.0, 1.0)], &["a / b > 2"]);
        assert_eq!(rendered.untranslated, Vec::<usize>::new());
        assert!(
            rendered.text.contains("(assert (not (= |b| 0.0)))"),
            "no divisor guard:\n{}",
            rendered.text
        );
    }

    #[test]
    fn a_modulo_pins_its_divisor_away_from_zero_too() {
        // Same hazard, same guard. `babel_rem` divides internally, so a
        // symbolic divisor is one a solver could otherwise drive to zero and
        // satisfy the constraint through — and `cvg_pools` has precisely that
        // case, in `modulo_with_a_symbolic_divisor`.
        let rendered = document(&[("a", 0.0, 10.0), ("b", 0.0, 10.0)], &["3 > a % b"]);
        assert_eq!(rendered.untranslated, Vec::<usize>::new());
        assert!(
            rendered.text.contains("(assert (not (= |b| 0.0)))"),
            "no divisor guard on `%`:\n{}",
            rendered.text
        );
        assert!(
            rendered.text.contains("babel_rem"),
            "`%` did not reach the helper:\n{}",
            rendered.text
        );
    }

    #[test]
    fn every_division_gets_its_own_guard() {
        let rendered = document(
            &[("a", 0.0, 1.0), ("b", 0.0, 1.0), ("c", 0.0, 1.0)],
            &["a / b + a / c > 2"],
        );
        assert!(rendered.text.contains("(assert (not (= |b| 0.0)))"));
        assert!(rendered.text.contains("(assert (not (= |c| 0.0)))"));
    }

    #[test]
    fn a_root_becomes_an_auxiliary_variable() {
        // QF_NRA has no `sqrt`, so `y >= 0 and y*y = x` stands in. The sign
        // constraint is also what makes `sqrt` of a negative unsatisfiable,
        // which is the right answer — babel gives NaN there.
        let rendered = document(&[("x", 0.0, 10.0), ("y", 0.0, 10.0)], &["y > sqrt(x)"]);
        assert_eq!(rendered.untranslated, Vec::<usize>::new());
        assert!(rendered.text.contains("(declare-const aux_0_0 Real)"));
        assert!(rendered.text.contains("(assert (>= aux_0_0 0.0))"));
        assert!(
            rendered
                .text
                .contains("(assert (= (* aux_0_0 aux_0_0) |x|))")
        );

        // A cube root is one-to-one over the reals, so it gets no sign
        // constraint — pinning it positive would lose the negative branch.
        let cubed = document(&[("x", -10.0, 10.0), ("y", -10.0, 10.0)], &["y > cbrt(x)"]);
        assert!(
            cubed
                .text
                .contains("(assert (= (* aux_0_0 aux_0_0 aux_0_0) |x|))")
        );
        assert!(!cubed.text.contains("(assert (>= aux_0_0 0.0))"));
    }

    #[test]
    fn auxiliary_names_do_not_collide_across_constraints() {
        // Two constraints each declaring `aux0` is a redeclaration, and the
        // solver rejects the entire document rather than just that line.
        let rendered = document(
            &[("x", 0.0, 10.0), ("y", 0.0, 10.0)],
            &["y > sqrt(x)", "x > sqrt(y)"],
        );
        assert_eq!(rendered.untranslated, Vec::<usize>::new());
        assert!(rendered.text.contains("(declare-const aux_0_0 Real)"));
        assert!(rendered.text.contains("(declare-const aux_1_0 Real)"));
        assert_eq!(rendered.text.matches("declare-const aux").count(), 2);
    }

    #[test]
    fn side_conditions_precede_the_assertion_that_needs_them() {
        // A declaration after its use is a parse error, and `Solver::from_string`
        // reports one by silently solving nothing.
        let rendered = document(&[("x", 0.0, 10.0), ("y", 0.0, 10.0)], &["y > sqrt(x)"]);
        let declaration = rendered
            .text
            .find("declare-const aux_0_0")
            .expect("declared");
        let usage = rendered.text.find(":named c0").expect("asserted");
        assert!(declaration < usage, "declared too late:\n{}", rendered.text);
    }

    #[test]
    fn the_logic_is_the_callers_to_choose() {
        let rendered = document(&[("x", 0.0, 10.0)], &["x > 4"]);
        assert!(
            rendered.text.contains("(set-logic QF_NIRA)"),
            "the default logic is not what it claims:
{}",
            rendered.text
        );

        let inputs = [InputVariable {
            name: "x".to_owned(),
            lower_bound: 0.0,
            upper_bound: 10.0,
        }];
        let constraints = [crate::parse("x > 4").expect("compiles")];
        let overridden = emit(&inputs, &constraints, &SmtLogic::named("QF_NRA"));
        assert!(overridden.text.contains("(set-logic QF_NRA)"));
        assert!(!overridden.text.contains("QF_NIRA"));
    }

    #[test]
    fn the_environment_sets_the_default_and_nothing_more() {
        // Precedence, in the only form that can be checked without mutating a
        // process-global: absent or blank falls back, anything else is taken
        // verbatim. That `with_logic` beats this is structural — it replaces
        // the field the default produced.
        assert_eq!(SmtLogic::from_variable(None), SmtLogic::named("QF_NIRA"));
        assert_eq!(
            SmtLogic::from_variable(Some("")),
            SmtLogic::named("QF_NIRA")
        );
        assert_eq!(
            SmtLogic::from_variable(Some("   ")),
            SmtLogic::named("QF_NIRA")
        );
        assert_eq!(SmtLogic::from_variable(Some("ALL")), SmtLogic::named("ALL"));
        assert_eq!(
            SmtLogic::from_variable(Some(" QF_NRA ")),
            SmtLogic::named("QF_NRA")
        );
    }

    #[test]
    fn a_comment_cannot_swallow_the_command_after_it() {
        // The one case where a missing newline is a bug rather than a
        // formatting quibble: `;` runs to end of line. Two ways it could go
        // wrong — the renderer forgetting the separator, and a source string
        // that carries its own newlines out of the comment and into the
        // command stream. Babel statements are newline-legal, so the second is
        // reachable from user input.
        let source = "var a = 4;
  return x
 > a";
        let inputs = [InputVariable {
            name: "x".to_owned(),
            lower_bound: 0.0,
            upper_bound: 10.0,
        }];
        let constraints = [crate::parse(source).expect("compiles")];
        let rendered = emit(&inputs, &constraints, &SmtLogic::default());

        for line in rendered.text.lines() {
            assert!(
                !(line.starts_with(';') && line.contains('(')),
                "a command ended up inside a comment:
{line}"
            );
        }
        assert_eq!(
            rendered.text.lines().filter(|l| l.starts_with(';')).count(),
            1,
            "the source broke across more than one comment line:
{}",
            rendered.text
        );
        assert!(
            rendered.text.contains(":named"),
            "the assertion after the comment did not survive:
{}",
            rendered.text
        );
    }

    /// Each way of being untranslatable reports itself as the right one.
    ///
    /// The reason reaches a caller only through a `tracing` line, which nothing
    /// asserts on, so without this the messages could drift into nonsense and
    /// every test would stay green.
    #[test]
    fn a_refusal_says_which_construct_defeated_it() {
        let inputs = [
            InputVariable {
                name: "x".to_owned(),
                lower_bound: 0.1,
                upper_bound: 10.0,
            },
            InputVariable {
                name: "n".to_owned(),
                lower_bound: 1.0,
                upper_bound: 4.0,
            },
        ];

        for (source, expected) in [
            ("sin(x) <= 0", Refusal::Transcendental("sin")),
            ("cos(x) <= 0", Refusal::Transcendental("cos")),
            ("tanh(x) < 4", Refusal::Transcendental("tanh")),
            ("3 > log(x, 2)", Refusal::Logarithm),
            ("x ^ x > 2", Refusal::RealExponent),
            ("x > 2 ^ n", Refusal::RealExponent),
            ("var[n] > 2", Refusal::ComputedSubscript),
            ("x + 1", Refusal::NotABooleanExpression),
        ] {
            let expression = crate::parse(source).expect("should compile");
            let Err(reason) = translate(&expression, 0, &inputs) else {
                panic!("{source:?} was expected to be untranslatable");
            };
            assert_eq!(reason, expected, "{source:?}");
        }
    }

    /// What the emitter still cannot express, and which operators are implicated.
    ///
    /// Run it for the report: `cargo test --lib residue -- --nocapture`.
    ///
    /// This is the measurement wave 3 opens with, kept as a test so it cannot go
    /// stale. The assertion at the bottom is a **ratchet**: the untranslatable
    /// set is pinned, so teaching the emitter something new fails here and makes
    /// somebody update the record deliberately.
    ///
    /// The *why* is derived rather than declared. Nothing here knows which
    /// operators `translate` refuses — it collects the operators in every
    /// constraint, then subtracts the ones appearing in something translatable.
    /// What is left cannot be expressible, and no list needs maintaining twice.
    #[test]
    fn residue_what_the_emitter_still_cannot_express() {
        // Every distinct constraint shape in the CVG corpus, from `cvg_pools.rs`
        // and `cvg_benchmarks.rs`. The thirty P118 rows are one shape and appear
        // once; placeholders are given concrete values.
        const CORPUS: &[&str] = &[
            // -- plain arithmetic and comparison
            "x > 8",
            "x < 2",
            "x1 < x2",
            "x1 + x2 > x3",
            "0 > -x4+x1-7",
            "x > 10.5",
            "x > 0.0",
            // -- equalities with a tolerance
            "x1 == x2 +/- 0.1",
            "x2 == x1 + 1/2*x2 - x3 / x4 +/- 0.00001",
            "1.5 == var[1] + var[2] +/- 0.001",
            "(x + 2) * (x - 1) == 0 +/- 1.0",
            // -- constants
            "x1 == pi +/- 0.001",
            "x2 == e +/- 0.001",
            // -- powers
            "x1 == x2^3 +/- 0.0001",
            "20 > 2^x5",
            // -- roots, absolute value, sign
            "x1 == sqrt(x2) +/- 0.0001",
            "x3 == cbrt(x4) +/- 0.0001",
            "abs(x1) == 1 +/- 0.001",
            "x2 == sgn(x1) +/- 0.001",
            // -- integer-ish
            "x1 > floor(x2)",
            "x3 > ceil(x4) + floor(x4)",
            "x1 % 3.0 >= 2",
            "3 > 10 % x1",
            "x3 == x4 % 4.5 +/- 0.0001",
            // -- logarithms
            "2 < ln(x1)",
            "3 > log(2, x)",
            // -- trigonometry
            "sin(x1) <= 0",
            "y == sin(x) +/- 0.000001",
            "y > sin(theta)",
            "y < sin(x*pi)",
            "y > 1.1*sin(x*pi-0.5)",
            "x1 > sin(ln(cos(2.1^x1)))",
        ];

        let names: Vec<String> = ["x", "y", "theta"]
            .iter()
            .map(|s| (*s).to_owned())
            .chain((1..=15).map(|i| format!("x{i}")))
            .collect();
        let inputs: Vec<InputVariable> = names
            .iter()
            .map(|name| InputVariable {
                name: name.clone(),
                lower_bound: 0.1,
                upper_bound: 10.0,
            })
            .collect();

        let mut translated: Vec<(&str, Vec<String>)> = Vec::new();
        let mut refused: Vec<(&str, Vec<String>, Refusal)> = Vec::new();

        for source in CORPUS {
            let expression = crate::parse(source)
                .unwrap_or_else(|e| panic!("{source:?} did not compile: {:#?}", e.problems));
            let rendered = emit(
                &inputs,
                std::slice::from_ref(&expression),
                &SmtLogic::default(),
            );
            let mut operators = Vec::new();
            collect_operators(&expression.program.body, &mut operators);
            operators.sort_unstable();
            operators.dedup();

            if rendered.untranslated.is_empty() {
                translated.push((source, operators));
            } else {
                // Asked again for the reason, which `Document` reports through
                // `tracing` rather than carrying. Two independent accounts of
                // the same refusal: the emitter's own, and the operator set
                // derived below. They should agree.
                let Err(reason) = translate(&expression, 0, &inputs) else {
                    panic!("{source:?} is in `untranslated` yet translates");
                };
                refused.push((source, operators, reason));
            }
        }

        // Anything appearing in something translatable is not the reason
        // anything else was refused.
        let expressible: std::collections::BTreeSet<&str> = translated
            .iter()
            .flat_map(|(_, ops)| ops.iter().map(String::as_str))
            .collect();

        println!();
        println!(
            "what the emitter cannot express, out of {} corpus shapes",
            CORPUS.len()
        );
        println!("{:-<78}", "");
        for (source, operators, reason) in &refused {
            let blamed: Vec<&str> = operators
                .iter()
                .map(String::as_str)
                .filter(|op| !expressible.contains(op))
                .collect();
            // `Debug` here rather than `Display`: the prose belongs in a log
            // line, where there is one of them, not in a column.
            println!("{source:<32} {:<14} {reason:?}", blamed.join(" "));
        }
        println!("{:-<78}", "");
        println!(
            "{} of {} translate; {} do not.",
            translated.len(),
            CORPUS.len(),
            refused.len()
        );

        let mut culprits: Vec<&str> = refused
            .iter()
            .flat_map(|(_, ops, _)| ops.iter().map(String::as_str))
            .filter(|op| !expressible.contains(op))
            .collect();
        culprits.sort_unstable();
        culprits.dedup();
        println!("operators implicated: {}", culprits.join(" "));
        println!();

        let outstanding: Vec<&str> = refused.iter().map(|(source, ..)| *source).collect();
        assert_eq!(
            outstanding,
            vec![
                "sin(x1) <= 0",
                "y == sin(x) +/- 0.000001",
                "y > sin(theta)",
                "y < sin(x*pi)",
                "y > 1.1*sin(x*pi-0.5)",
                "x1 > sin(ln(cos(2.1^x1)))",
            ],
            "the untranslatable set moved — update this list and `todo.md` together"
        );
    }

    /// Every operator appearing anywhere in a block, by name.
    fn collect_operators(block: &Block, into: &mut Vec<String>) {
        for assignment in &block.assignments {
            collect_from(&assignment.value, into);
        }
        collect_from(&block.result, into);
    }

    fn collect_from(node: &Expr, into: &mut Vec<String>) {
        match &node.kind {
            Kind::Unary { op, arg } => {
                into.push(format!("{op:?}"));
                collect_from(arg, into);
            }
            Kind::Binary { op, lhs, rhs } => {
                into.push(format!("{op:?}"));
                collect_from(lhs, into);
                collect_from(rhs, into);
            }
            Kind::DynamicIndex(index) => {
                into.push("DynamicIndex".to_owned());
                collect_from(index, into);
            }
            Kind::And { terms } => {
                into.push("And".to_owned());
                for term in terms {
                    collect_from(term, into);
                }
            }
            Kind::Fold { terms, .. } => {
                into.push("Fold".to_owned());
                for term in terms {
                    collect_from(term, into);
                }
            }
            Kind::Block(block) => collect_operators(block, into),
            Kind::Aggregate { .. } => {
                unreachable!("`unroll_aggregates` leaves no aggregate for a backend to see")
            }
            Kind::Compare { op, lhs, rhs } => {
                into.push(format!("{op:?}"));
                collect_from(lhs, into);
                collect_from(rhs, into);
            }
            Kind::NearEq { lhs, rhs, .. } => {
                into.push("NearEq".to_owned());
                collect_from(lhs, into);
                collect_from(rhs, into);
            }
            Kind::Literal(_) | Kind::Global(_) | Kind::Local(_) => {}
        }
    }

    #[test]
    fn core_names_round_trip() {
        for index in [0usize, 1, 7, 29] {
            assert_eq!(core_index(&core_name(index)), Some(index));
        }
        assert_eq!(core_index("not-a-core-name"), None);
    }

    #[test]
    fn unicode_names_survive_as_quoted_symbols() {
        let rendered = document(&[("λ", 0.0, 1.0)], &["λ > 0.5"]);
        assert!(rendered.text.contains("(declare-const |λ| Real)"));
    }

    #[test]
    fn the_reds_this_is_meant_to_unlock() {
        // The `cvg_pools` cases that no amount of sampling or walking will
        // reach. All of them are expressible now: `%`, `floor` and `ceil` were
        // the last holdouts and they are encodings rather than theory.
        for source in [
            // Inverted rather than encoded: `ln(x1) > 2` becomes `x1 > e^2`,
            // `2^x5 < 20` becomes `x5 < log2(20)`, and `log(2, x1) < 3` becomes
            // `x1 < 8`. Z3 is never asked about a logarithm — as well, since it
            // has none.
            "2 < ln(x1)",
            "20 > 2^x5",
            "3 > log(2, x1)",
            "x1 % 3.0 >= 2",
            "x3 == x4 % 4.5 +/- 0.0001",
            "x1 > floor(x2)",
            "x3 > ceil(x4) + floor(x4)",
            "x2 == x1 + 1/2*x2 - x3 / x4 +/- 0.00001",
            "x1 == x2^3 +/- 0.0001",
            "abs(x1) == 1 +/- 0.001",
            "x1 == pi +/- 0.001",
            "1.5 == var[1] + var[2] +/- 0.001",
            "x1 == sqrt(x2) +/- 0.0001",
            "x3 == cbrt(x4) +/- 0.0001",
        ] {
            let rendered = document(
                &[
                    ("x1", 0.0, 10.0),
                    ("x2", 0.0, 10.0),
                    ("x3", 0.0, 10.0),
                    ("x4", 0.0, 10.0),
                ],
                &[source],
            );
            assert_eq!(
                rendered.untranslated,
                Vec::<usize>::new(),
                "{source:?} should be expressible in QF_NIRA, but:\n{}",
                rendered.text
            );
        }
    }
}
