//! S-expressions that cannot be unbalanced.
//!
//! The emitter used to build SMT-LIB by concatenating format strings —
//! `format!("({op} {left} {right})")` and so on. That works until it does not,
//! and the failure mode is genuinely nasty: `Solver::from_string` reports a
//! syntax error by *silently accepting nothing*, so a single missing paren gives
//! you an empty solver that answers `sat` instantly with an empty model. A typo
//! reads as "solved it".
//!
//! Here the parentheses are not written at all. They come from [`Sexp::List`]'s
//! `Display`, which means an unbalanced document is not a bug to be caught but a
//! state that cannot be represented. The whole class is gone rather than tested
//! for.
//!
//! Two variants and a `Display` is a low enough bar that this is a value type
//! rather than a framework — closer to `serde_json::Value` than to a DSL. It
//! also asks nothing of the reader that this crate does not already: lisp is an
//! AST with the parentheses left in, and anyone following [`crate::ast`] is
//! already reading node trees.
//!
//! Crates exist that would do this job — `smtlib` and `aws-smt-ir` are both
//! permissively licensed, and `smtlib-syntax` is closest of all in intent but is
//! GPL-3.0, which rules it out for a library that gets linked and shipped. None
//! of them touch the part that is actually hard. The s-expressions are the
//! trivial layer; the domain logic in [`super::emit`] — divisor guards,
//! auxiliary variables, peeling the strictness epsilon, tracking what could not
//! be expressed — is babel-specific and would survive any of those dependencies
//! unchanged.
//!
//! # Writing them
//!
//! Building an `Sexp` by hand is safe and *extremely* verbose, so [`sexp!`] and
//! [`define_fun!`] take the lisp as-written and expand to the same constructor
//! calls. They are macros rather than parsers because Rust's own tokenizer
//! refuses unbalanced delimiters — so `sexp!((* x x)` is a compile error before
//! any of this code runs, and balance is enforced twice over.
//!
//! What they cannot do is the interesting half. A macro sees source text, and
//! most of a document is built from the AST at run time from names and values
//! that do not exist until then. So the macros cover the static fragments and
//! the type covers everything.
//!
//! Two Rust-isms shape the syntax, and are worth knowing before extending them:
//!
//! * Rust tokenizes `define-fun` as three tokens — `define`, `-`, `fun` — so
//!   every hyphenated SMT-LIB keyword has to be spelled by the macro rather than
//!   written in its argument. That is why [`define_fun!`] exists as its own
//!   macro instead of `sexp!` handling it.
//! * `|x|` is a quoted symbol in SMT-LIB and a pair of bitwise-ors in Rust, so
//!   quoted symbols cannot be written in a macro at all. Only [`Sexp::symbol`]
//!   produces them, which is fine: they only ever hold names that arrive at run
//!   time.

use std::fmt;

/// An S-expression: either a token, or a parenthesised sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Sexp {
    /// Rendered verbatim. Numerals, symbols, keywords.
    Atom(String),
    /// Rendered space-separated inside parentheses.
    List(Vec<Sexp>),
}

impl Sexp {
    pub(crate) fn atom(text: impl Into<String>) -> Self {
        Self::Atom(text.into())
    }

    /// An SMT-LIB quoted symbol.
    ///
    /// Always quoted, because babel identifiers may be any Unicode — the corpus
    /// has emoji and Han characters in variable names. A quoted symbol accepts
    /// anything but `|` and `\`, neither of which babel's lexer admits.
    pub(crate) fn symbol(name: &str) -> Self {
        Self::Atom(format!("|{name}|"))
    }

    pub(crate) fn list(items: impl IntoIterator<Item = Self>) -> Self {
        Self::List(items.into_iter().collect())
    }

    /// `(head arg…)` — the shape almost everything in SMT-LIB takes.
    pub(crate) fn call(head: &str, arguments: impl IntoIterator<Item = Self>) -> Self {
        let mut items = vec![Self::atom(head)];
        items.extend(arguments);
        Self::List(items)
    }
}

impl fmt::Display for Sexp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Atom(text) => f.write_str(text),
            Self::List(items) => {
                f.write_str("(")?;
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        f.write_str(" ")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str(")")
            }
        }
    }
}

/// Lisp, written as lisp, expanded into [`Sexp`] constructor calls.
///
/// ```ignore
/// sexp!((ite (< x 0.0) (- x) x))
/// ```
///
/// Every token becomes an atom and every parenthesised group a list, so what
/// comes out is a real `Sexp` rather than a pre-rendered string — the type's
/// balance guarantee is not bypassed, it is reached by a shorter road. Rust's
/// tokenizer rejects unbalanced delimiters in macro arguments, so a missing
/// parenthesis here fails to compile.
///
/// Multi-character operators are single Rust tokens, so `<=` and `>=` survive
/// intact. A quoted symbol cannot be written here — see the module docs.
macro_rules! sexp {
    // A parenthesised group. Must come first: a group is itself a single token
    // tree, so the atom arm below would happily swallow one.
    ( ( $($items:tt)* ) ) => {
        $crate::cvg::sexp::Sexp::List(::std::vec![ $(sexp!($items)),* ])
    };
    ( $atom:tt ) => {
        $crate::cvg::sexp::Sexp::atom(::std::stringify!($atom))
    };
}

/// An SMT-LIB `define-fun`, written the way it reads.
///
/// ```ignore
/// define_fun!(babel_abs (x Real) -> Real: (ite (< x 0.0) (- x) x))
/// define_fun!(babel_max (a Real) (b Real) -> Real: (ite (>= a b) a b))
/// ```
///
/// Everything except the `-> Sort:` splitter is lisp exactly as it would be
/// written: `babel_abs (x Real)` is the head of the SMT-LIB form and the trailing
/// group is its body. The splitter is there because Rust tokenizes `define-fun`
/// as `define`, `-`, `fun`, so the keyword has to come from the macro's name
/// rather than its argument — and once the form is being broken up anyway, an
/// arrow reads better than juxtaposing `Real` against the body, which is the
/// shape that made C function-pointer typedefs unreadable.
///
/// Whitespace inside the body is irrelevant, because it is rebuilt from tokens
/// rather than copied: `(-x)` and `(- x)` both render `(- x)`.
macro_rules! define_fun {

    // note: i dont like macros. And i dont like macro creep. Adding more features here is _very_ tempting:
    //
    // # The trade, which is real
    //
    // This buys brevity — the prelude is six lines instead of ninety — and a body
    // that can be read straight against the SMT-LIB spec rather than decoded from
    // nested constructor calls. For a table that is hand-maintained and
    // semantically load-bearing, that matters.
    //
    // It costs **IDE support**, and the loss is not subtle. Inside the macro your
    // editor is parsing lisp as Rust tokens: there is no meaningful highlighting,
    // no rename, no go-to-definition, and error spans point at the expansion.
    // Plain [`Sexp::call`] gets all of that. If you find yourself fighting the
    // editor here rather than reading, that is this trade going the wrong way and
    // it is fine to reverse it — see below.
    //
    // Nor does it check anything beyond structure. `babel_abz` compiles. What
    // catches that is `every_document_the_emitter_can_build_parses`, which puts
    // each document to Z3, and `the_prelude_helpers_mean_what_they_say`, which puts
    // the *meaning* of each helper to Z3. Those tests are the real safety net; the
    // macro only guarantees the parentheses match.
    //
    // # Where this stops
    //
    // **Static fragments only. Anything carrying a runtime value goes through the
    // constructors.**
    //
    // The failure mode for a thing like this is not writing it, it is continuing to
    // write it. The specific temptation is interpolation — `sexp!((+ #left #right))`
    // — and it should be refused. That is the step where this stops being lisp
    // spelled in Rust tokens and becomes a template language with its own
    // evaluation rules, at which point every objection to embedding one language in
    // another applies to us in full and we own a parser nobody asked for.
    //
    // # If it stops paying
    //
    // In rough order of how much they cost to adopt:
    //
    // * **Plain [`Sexp::call`] constructors.** About thirty lines more for the
    //   prelude, full editor support, invents nothing. The obvious retreat.
    // * **A string-building `LispBuilder`.** Tempting and boring, but it hands back
    //   exactly the failure this module exists to prevent: an unbalanced document,
    //   which [`super::smt`] cannot even report, because `Solver::from_string`
    //   answers a syntax error with `sat`.
    // * **The `smtlib` or `aws-smt-ir` crates.** Both permissive. Both replace the
    //   forty lines above and none of the domain logic below.
    // * **Z3's typed AST**, building `z3::ast::Real` directly and skipping text.
    //   Answers every objection at once — checked by rustc, no parentheses, full
    //   tooling — at the price of welding the emitter to one solver and giving up a
    //   document you can read, diff and hand to something else.
    ($name:ident $( ( $parameter:ident $sort:ident ) )* -> $returns:ident : $($body:tt)+) => {
        $crate::cvg::sexp::Sexp::call(
            "define-fun",
            [
                $crate::cvg::sexp::Sexp::atom(::std::stringify!($name)),
                $crate::cvg::sexp::Sexp::list([
                    $($crate::cvg::sexp::Sexp::list([
                        $crate::cvg::sexp::Sexp::atom(::std::stringify!($parameter)),
                        $crate::cvg::sexp::Sexp::atom(::std::stringify!($sort)),
                    ])),*
                ]),
                $crate::cvg::sexp::Sexp::atom(::std::stringify!($returns)),
                sexp!($($body)+),
            ],
        )
    };
}

pub(crate) use {define_fun, sexp};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nesting_renders_with_matched_parentheses() {
        let term = Sexp::call(
            "assert",
            [Sexp::call(
                "<=",
                [
                    Sexp::call("+", [Sexp::symbol("x"), Sexp::atom("1.0")]),
                    Sexp::atom("0.0"),
                ],
            )],
        );
        assert_eq!(term.to_string(), "(assert (<= (+ |x| 1.0) 0.0))");
    }

    #[test]
    fn an_empty_list_is_still_a_list() {
        assert_eq!(Sexp::list([]).to_string(), "()");
        assert_eq!(Sexp::call("check-sat", []).to_string(), "(check-sat)");
    }

    #[test]
    fn every_rendering_is_balanced() {
        // The property the whole type exists for: there is no way to construct
        // an `Sexp` whose rendering has a stray parenthesis, because the
        // parentheses are not part of the data.
        let specimens = [
            Sexp::atom("1.0"),
            Sexp::symbol("x"),
            Sexp::list([]),
            Sexp::call("-", [Sexp::atom("1.0")]),
            Sexp::call(
                "let",
                [
                    Sexp::list([Sexp::list([Sexp::atom("l0"), Sexp::symbol("x")])]),
                    Sexp::call("*", [Sexp::atom("l0"), Sexp::atom("l0")]),
                ],
            ),
        ];

        for specimen in specimens {
            let rendered = specimen.to_string();
            let mut depth = 0i32;
            for character in rendered.chars() {
                match character {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                assert!(depth >= 0, "closed too early: {rendered}");
            }
            assert_eq!(depth, 0, "left open: {rendered}");
        }
    }

    #[test]
    fn the_macro_builds_what_the_constructors_would() {
        // The macros are a shorthand, not a second implementation: whatever they
        // expand to has to be the same value the long-hand produces.
        assert_eq!(
            sexp!((* x x x)),
            Sexp::call("*", [Sexp::atom("x"), Sexp::atom("x"), Sexp::atom("x")])
        );
        assert_eq!(
            sexp!((ite (< x 0.0) (- x) x)).to_string(),
            "(ite (< x 0.0) (- x) x)"
        );
        // Multi-character operators are one Rust token, so they survive whole.
        assert_eq!(sexp!((>= a b)).to_string(), "(>= a b)");
        assert_eq!(sexp!((<= a b)).to_string(), "(<= a b)");
    }

    #[test]
    fn define_fun_renders_a_whole_definition() {
        assert_eq!(
            define_fun!(babel_abs (x Real) -> Real: (ite (< x 0.0) (- x) x)).to_string(),
            "(define-fun babel_abs ((x Real)) Real (ite (< x 0.0) (- x) x))"
        );
        assert_eq!(
            define_fun!(babel_max (a Real) (b Real) -> Real: (ite (>= a b) a b)).to_string(),
            "(define-fun babel_max ((a Real) (b Real)) Real (ite (>= a b) a b))"
        );
        // Spacing in the source does not reach the output: the body is rebuilt
        // from tokens, so `(-x)` and `(- x)` are the same term.
        assert_eq!(
            define_fun!(negate (x Real) -> Real: (-x)).to_string(),
            "(define-fun negate ((x Real)) Real (- x))"
        );
    }

    #[test]
    fn a_quoted_symbol_survives_unicode() {
        assert_eq!(Sexp::symbol("λ").to_string(), "|λ|");
        assert_eq!(Sexp::symbol("x1").to_string(), "|x1|");
    }
}
