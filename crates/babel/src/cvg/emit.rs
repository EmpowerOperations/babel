//! Rendering constraints as an SMT-LIB2 document.
//!
//! A pure `&[Expression] -> String`, with no solver attached and none required
//! to test it. That separation is the whole reason for emitting text rather than
//! building a solver's AST: the JVM version transcoded straight into Z3 objects,
//! so the only way to see what it had asked was to ask Z3.
//!
//! # What gets asserted
//!
//! Babel's boolean rewrite has already run by the time an [`Expression`] exists,
//! so a constraint is no longer a comparison — it is a scalar residual whose
//! *sign* carries the truth value, satisfied when `<= 0`. Every constraint emits
//! as `(assert (<= <residual> 0.0))`, and the emitter never sees a comparison
//! operator at all.
//!
//! # Strictness does not survive the trip
//!
//! The rewrite encodes `a < b` as `a - b + EPSILON`, with `EPSILON` being
//! `f64::MIN_POSITIVE`. That works *because of `f64` rounding*: the nudge
//! vanishes at any meaningful magnitude and survives only when the difference is
//! exactly zero, which is the one place strict and non-strict differ.
//!
//! Real arithmetic does not round, so the trick does not translate — and
//! emitting it literally puts a three-hundred-digit denormal in the middle of an
//! otherwise readable document. So the emitter recognises the marker and asserts
//! `(< residual 0.0)` instead. This is the same kind of knowledge the emitter
//! already needs about the rewrite's output: it has to know that `<= 0` means
//! true, and this is how the rewrite spells `< 0`.
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
//! This targets plain `QF_NRA`: the field operations, comparison, `ite` and
//! `let`. Anything outside that — `mod`, `floor`, the transcendentals, a power
//! with a non-integer exponent — is *reported* through
//! [`Document::untranslated`].
//!
//! The JVM version dropped such constraints silently, and its own fixture pinned
//! the consequence: `sin of value offset by multiples of pi` asserted that the
//! returned points included ones which **did not satisfy the constraints**. A
//! solver answering a question you quietly did not ask is worse than a solver
//! that says it cannot help.
//!
//! Several of those gaps close once a backend is chosen, because they are
//! dialect rather than theory: dReal has `sin`, `cos`, `exp`, `log` and `sqrt`
//! as primitives, and Z3 accepts `^`. Neither is standard SMT-LIB, which is why
//! nothing here emits them yet.
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

use crate::ast::{AggregateKind, BinaryOp, Block, Expr, Kind, UnaryOp};
use crate::cvg::InputVariable;
use crate::cvg::sexp::{Sexp, define_fun, sexp};
use crate::{Expression, ast, rewrite};

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
    ]
}

pub(crate) fn emit(inputs: &[InputVariable], constraints: &[Expression]) -> Document {
    let mut text = String::new();
    // Before `set-logic`, which is where SMT-LIB wants options. Asking for cores
    // up front costs nothing on a satisfiable query and is the only way to learn
    // *which* constraints conflict on an unsatisfiable one.
    text.push_str("(set-option :produce-unsat-cores true)\n");
    text.push_str("(set-logic QF_NRA)\n");
    for definition in prelude() {
        text.push_str(&format!("{definition}\n"));
    }

    for input in inputs {
        let declaration = Sexp::call(
            "declare-const",
            [Sexp::symbol(&input.name), Sexp::atom("Real")],
        );
        text.push_str(&format!("{declaration}\n"));
    }
    for input in inputs {
        // Skipped rather than encoded as an unsatisfiable bound: a non-finite
        // range is the caller's problem to notice, not something to assert.
        if let (Some(low), Some(high)) = (real(input.lower_bound), real(input.upper_bound)) {
            let name = Sexp::symbol(&input.name);
            let bounded = Sexp::call(
                "assert",
                [Sexp::call(
                    "and",
                    [
                        Sexp::call(">=", [name.clone(), low]),
                        Sexp::call("<=", [name, high]),
                    ],
                )],
            );
            text.push_str(&format!("{bounded}\n"));
        }
    }

    let mut untranslated = Vec::new();
    for (index, constraint) in constraints.iter().enumerate() {
        text.push_str(&format!("; {}\n", comment(constraint.source())));

        match translate(constraint, index, inputs) {
            Some(assertion) => {
                for condition in &assertion.conditions {
                    text.push_str(&format!("{condition}\n"));
                }
                // Named so that `(get-unsat-core)` can point back at the
                // constraint rather than at an anonymous term.
                let named = Sexp::call(
                    "assert",
                    [Sexp::call(
                        "!",
                        [
                            Sexp::call(assertion.relation, [assertion.residual, Sexp::atom("0.0")]),
                            Sexp::atom(":named"),
                            Sexp::atom(core_name(index)),
                        ],
                    )],
                );
                text.push_str(&format!("{named}\n"));
            }
            None => {
                untranslated.push(index);
                text.push_str(";   NOT TRANSLATED - outside QF_NRA, left unasserted\n");
            }
        }
    }

    text.push_str("(check-sat)\n(get-model)\n");
    Document { text, untranslated }
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
    relation: &'static str,
    residual: Sexp,
}

/// One constraint as a relation and a residual, or `None` if it needs something
/// QF_NRA does not have.
fn translate(constraint: &Expression, index: usize, inputs: &[InputVariable]) -> Option<Assertion> {
    // A scalar expression has no `<= 0` reading, so asserting one would invent a
    // constraint the user did not write.
    if !constraint.is_boolean_expression {
        return None;
    }
    let mut names = Names {
        symbols: &constraint.symbols,
        inputs,
        constraint: index,
        conditions: Vec::new(),
        auxiliaries: 0,
    };

    // A trailing `+ EPSILON` is how the rewrite spells "strictly". Peel it off
    // and put the strictness in the relation instead.
    let body = &constraint.program.body;
    if body.assignments.is_empty()
        && let Kind::Binary {
            op: BinaryOp::Add,
            lhs,
            rhs,
        } = &body.result.kind
        && let Kind::Literal(value) = rhs.kind
        && value == rewrite::EPSILON
    {
        let residual = names.expression(lhs)?;
        return Some(Assertion {
            conditions: names.conditions,
            relation: "<",
            residual,
        });
    }

    let residual = names.block(body)?;
    Some(Assertion {
        conditions: names.conditions,
        relation: "<=",
        residual,
    })
}

impl Names<'_> {
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
            Kind::Literal(value) => real(*value),
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
                _ => None,
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

            // Bounds that were not constant, so the loop never unrolled. A
            // quantifier would leave QF_NRA.
            Kind::Aggregate { .. } => None,

            // The boolean rewrite runs during compilation, so no `Expression`
            // can still be holding one.
            Kind::Compare { .. } | Kind::NearEq { .. } => {
                unreachable!(
                    "comparisons are rewritten into arithmetic before an Expression exists"
                )
            }
        }
    }

    fn binary(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr) -> Option<Sexp> {
        // `Pow` inspects its right operand rather than rendering it, so it is
        // handled before either side is translated.
        if op == BinaryOp::Pow {
            return self.power(lhs, rhs);
        }

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

            // SMT-LIB's `mod` is integer-only, and babel's `%` follows Java in
            // taking the sign of the dividend. `log(base, x)` is
            // `ln x / ln base`, and there is no `ln`.
            BinaryOp::Mod | BinaryOp::LogB => return None,
            BinaryOp::Pow => unreachable!("handled above"),
        })
    }

    /// `x^n` for a constant whole-number `n`, as repeated multiplication.
    ///
    /// Only constant integer exponents: a real exponent is `exp(n * ln x)`, and
    /// there is no `exp`. The base is bound with `let` so that `x^40` does not
    /// duplicate the whole subtree forty times, and the binding is lexically
    /// scoped, so a nested power reusing the name shadows rather than collides.
    fn power(&mut self, base: &Expr, exponent: &Expr) -> Option<Sexp> {
        /// Beyond this, repeated multiplication is the wrong encoding anyway.
        const LIMIT: i64 = 64;

        let Kind::Literal(value) = exponent.kind else {
            return None;
        };
        let times = ast::to_index(value)?;
        if times.abs() > LIMIT {
            return None;
        }

        let rendered = self.expression(base)?;
        if times == 0 {
            return Some(Sexp::atom("1.0"));
        }

        let base_name = Sexp::atom("pow_base");
        let count = usize::try_from(times.abs()).ok()?;
        let product = if count == 1 {
            base_name.clone()
        } else {
            Sexp::call("*", std::iter::repeat_n(base_name.clone(), count))
        };
        let body = if times < 0 {
            Sexp::call("/", [Sexp::atom("1.0"), product])
        } else {
            product
        };
        Some(Sexp::call(
            "let",
            [Sexp::list([Sexp::list([base_name, rendered])]), body],
        ))
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

            // `floor` and `ceil` want `to_int`, which leaves QF_NRA for
            // QF_NIRA. The transcendentals are dReal primitives and Z3
            // non-starters. Neither is a silent drop.
            UnaryOp::Ceil
            | UnaryOp::Floor
            | UnaryOp::Ln
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
            | UnaryOp::Cot => return None,
        })
    }
}

/// A local slot's name inside a `let`.
fn local(slot: usize) -> String {
    format!("l{slot}")
}

/// Source text flattened onto one line, for a comment.
fn comment(source: &str) -> String {
    source.split_whitespace().collect::<Vec<_>>().join(" ")
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
        let constraints: Vec<Expression> = sources
            .iter()
            .map(|source| crate::compile(source).expect("test constraint should compile"))
            .collect();
        emit(&inputs, &constraints)
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
        ];

        let preamble: String = prelude().iter().map(|line| format!("{line}\n")).collect();
        for (claim, should_hold) in claims {
            let document = format!("(set-logic QF_NRA)\n{preamble}(assert {claim})\n");
            let outcome = Z3Backend
                .solve(&document)
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
                 (set-logic QF_NRA)\n{preamble}\n\
                 (declare-const |x| Real)\n\
                 (assert (and (>= |x| 0.0) (<= |x| 10.0)))\n\
                 ; x > 4\n\
                 (assert (! (< (- 4.0 |x|) 0.0) :named c0))\n\
                 (check-sat)\n(get-model)\n"
            )
        );
    }

    #[test]
    fn a_comparison_is_never_emitted_as_a_comparison() {
        // `x > 4` does not become `(> x 4)`. The boolean rewrite already turned
        // it into the residual `4 - x`, true when non-positive, and that is the
        // only form the emitter ever sees.
        let rendered = document(&[("x", 0.0, 10.0)], &["x > 4"]);
        assert!(rendered.text.contains("(< (- 4.0 |x|) 0.0)"));
    }

    #[test]
    fn strictness_becomes_the_relation_not_a_denormal() {
        // The rewrite marks `<` by adding `f64::MIN_POSITIVE`, which relies on
        // rounding that real arithmetic does not do. Emitting it literally would
        // put ~310 digits in the document and still mean the wrong thing.
        let strict = document(&[("x", 0.0, 10.0)], &["x > 4"]);
        let loose = document(&[("x", 0.0, 10.0)], &["x >= 4"]);

        assert!(
            strict
                .text
                .contains("(assert (! (< (- 4.0 |x|) 0.0) :named c0))")
        );
        assert!(
            loose
                .text
                .contains("(assert (! (<= (- 4.0 |x|) 0.0) :named c0))")
        );
        assert!(
            !strict.text.contains("0.0000000000"),
            "the denormal leaked into the document:\n{}",
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
    fn a_power_binds_its_base_once() {
        let rendered = document(&[("x", 0.0, 10.0)], &["x^3 > 2"]);
        assert!(
            rendered.text.contains("(* pow_base pow_base pow_base)"),
            "expected repeated multiplication, got:\n{}",
            rendered.text
        );
    }

    #[test]
    fn what_cannot_be_expressed_is_reported_not_dropped() {
        // The JVM version dropped these and returned points that did not satisfy
        // them. Each must appear in `untranslated`, and none may produce an
        // assertion.
        for source in [
            "x % 3.0 >= 2",
            "x > floor(x)",
            "2 < ln(x)",
            "sin(x) <= 0",
            "x^x > 2",
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
        // The `cvg_pools` cases that no amount of sampling or walking will reach.
        // Six of the seven are expressible; only `modulo` is not, and SMT-LIB's
        // `mod` being integer-only is why.
        for source in [
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
                "{source:?} should be expressible in QF_NRA, but:\n{}",
                rendered.text
            );
        }
    }
}
