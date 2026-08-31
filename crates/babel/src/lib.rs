//! Babel — a small constraint-expression language for optimizer formulations.
//!
//! ```ignore
//! let ast = babel::parse("x1 + x2 > 20 - x3^2")?;
//! let compiled = babel::compile(&ast, &Schema::new(["x1", "x2", "x3"]))?;
//!
//! // One column per sample, one row per schema variable.
//! let residuals = compiled.eval(samples.as_ref())?;
//! ```
//!
//! Two backends consume an [`Ast`]: this one, which runs it over a batch, and
//! [`cvg`], which reads its structure to search for points that satisfy it.
//! `src/README.md` has the picture.
//!
//! Boolean expressions evaluate to a scalar whose *sign* carries the truth
//! value: `<= 0` is true, `> 0` is false. That is the canonical `g(x) <= 0`
//! constraint form, so a violated constraint reports how badly it was violated.

// Crate-private while the shape is still settling; goes public when the
// pluggable rewriter needs it.
mod ast;
pub mod cvg;
pub mod diagnostics;

mod eval;
mod frontend;

pub use diagnostics::{
    BindError, CompilationFailure, EvalError, Problem, ProblemKind, RuntimeProblem, Span,
};
pub use eval::{CompiledExpression, compile, eval_one};

use std::collections::BTreeSet;

/// Compiles source text into an evaluable expression.
///
/// # Errors
/// Returns [`CompilationFailure`] with every problem found; compilation does
/// not stop at the first one.
pub fn parse(source: &str) -> Result<Ast, CompilationFailure> {
    if source.is_empty() {
        return Err(CompilationFailure {
            source: source.to_owned(),
            problems: vec![Problem::new(
                ProblemKind::EmptyExpression,
                source,
                Span::new(0, 0),
            )],
        });
    }

    let lowered = match frontend::translate(source) {
        Ok(lowered) => lowered,
        Err(problems) => {
            return Err(CompilationFailure {
                source: source.to_owned(),
                problems,
            });
        }
    };

    // Two of these passes report kind and span; rendering needs the source,
    // which lives here rather than in the rewriter.
    let render = |faults: Vec<diagnostics::Fault>| CompilationFailure {
        source: source.to_owned(),
        problems: faults
            .into_iter()
            .map(|fault| Problem::new(fault.kind, source, fault.span))
            .collect(),
    };

    // Constants collapse first, and everything after depends on it: a statically
    // known value is a `Kind::Literal` from here on, so no later pass needs an
    // evaluator of its own to recognise one. See `src/README.md`.
    let program = frontend::rewrite::fold_constants(lowered.program).map_err(render)?;

    // Then the monotone functions no solver will take are inverted away, while
    // comparisons still exist to be matched on.
    let program = frontend::rewrite::invert_monotone(program);

    // Then aggregates over known bounds expand, which is also where a bound that
    // is not a usable index stops being a run-time surprise.
    let program = frontend::rewrite::unroll_aggregates(program).map_err(render)?;

    // Last, so that a loop index substituted by unrolling is a literal by the
    // time an exponent is looked at.
    let program = frontend::rewrite::expand_powers(program);

    Ok(Ast {
        source: source.to_owned(),
        program,
        symbols: lowered.symbols,
        contains_dynamic_lookup: lowered.contains_dynamic_lookup,
        is_constraint: lowered.is_constraint,
    })
}

/// Whether `name` is a legal Babel variable name.
///
/// Babel accepts Unicode identifiers, so `π`, `测试` and `☕` are all legal.
#[must_use]
pub fn is_legal_variable_name(name: &str) -> bool {
    !name.is_empty() && frontend::parses_as_variable(name)
}

/// A compiled expression, ready to be bound to a [`Schema`].
#[derive(Debug, Clone, PartialEq)]
pub struct Ast {
    source: String,
    program: ast::Program,
    /// Distinct statically-referenced names in first-reference order.
    /// [`ast::GlobalId`] indexes into *this*, not into the schema — the AST is
    /// built before any schema exists, so [`Ast::bind`] is what maps
    /// these onto row positions.
    symbols: Vec<String>,
    contains_dynamic_lookup: bool,
    is_constraint: bool,
}

impl Ast {
    /// The source text this was compiled from.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Whether the expression uses `var[i]` dynamic lookup.
    ///
    /// A subscript is a one-based index into the whole [`Schema`] in
    /// declaration order, so such an expression can read a variable it never
    /// names and its [`statically_referenced_symbols`] are not the whole story.
    /// **A caller must not prune columns it believes are unreferenced while
    /// this is true.**
    ///
    /// [`statically_referenced_symbols`]: Ast::statically_referenced_symbols
    #[must_use]
    pub const fn contains_dynamic_lookup(&self) -> bool {
        self.contains_dynamic_lookup
    }

    /// Whether the source was a boolean expression, and therefore whether the
    /// result should be read as a constraint residual rather than a value.
    #[must_use]
    pub const fn is_constraint(&self) -> bool {
        self.is_constraint
    }

    /// Statically-referenced names in first-reference order, indexed by
    /// [`ast::GlobalId`].
    #[must_use]
    pub fn symbols(&self) -> &[String] {
        &self.symbols
    }

    /// The same names as an ordered set, for callers that only care about
    /// membership.
    #[must_use]
    pub fn statically_referenced_symbols(&self) -> BTreeSet<&str> {
        self.symbols.iter().map(String::as_str).collect()
    }
}

/// An ordered set of variable names.
///
/// Order is load-bearing: `var[i]` indexes into it, one-based. This is the
/// requirement that forced the JVM API to demand a `LinkedHashMap`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    names: Vec<String>,
}

impl Schema {
    /// Builds a schema from names in declaration order.
    #[must_use]
    pub fn new<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            names: names.into_iter().map(Into::into).collect(),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.names.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.names
    }
}
