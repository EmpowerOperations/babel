//! Babel — a small constraint-expression language for optimizer formulations.
//!
//! ```ignore
//! let expr = babel::compile("x1 + x2 > 20 - x3^2")?;
//! let value = expr.evaluate(&[("x1", 1.0), ("x2", 2.0), ("x3", 3.0)])?;
//! ```
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
mod front_end;
mod generated;
mod rewrite;

pub use diagnostics::{
    BindError, CompilationFailure, EvalError, Problem, ProblemKind, RuntimeProblem, Span,
};

use std::collections::BTreeSet;

/// Compiles source text into an evaluable expression.
///
/// # Errors
/// Returns [`CompilationFailure`] with every problem found; compilation does
/// not stop at the first one.
pub fn compile(source: &str) -> Result<Expression, CompilationFailure> {
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

    let lowered = match front_end::translate(source) {
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
    let program = rewrite::fold_constants(lowered.program).map_err(render)?;

    // Then the monotone functions no solver will take are inverted away, while
    // comparisons still exist to be matched on.
    let program = rewrite::invert_monotone(program);

    // Comparisons become arithmetic next, so unrolling never has to clone one.
    let program = rewrite::rewrite_booleans(program);

    // Then aggregates over known bounds expand, which is also where a bound that
    // is not a usable index stops being a run-time surprise.
    let program = rewrite::unroll_aggregates(program).map_err(render)?;

    Ok(Expression {
        source: source.to_owned(),
        program,
        symbols: lowered.symbols,
        contains_dynamic_lookup: lowered.contains_dynamic_lookup,
        is_boolean_expression: lowered.is_boolean_expression,
    })
}

/// Whether `name` is a legal Babel variable name.
///
/// Babel accepts Unicode identifiers, so `π`, `测试` and `☕` are all legal.
#[must_use]
pub fn is_legal_variable_name(name: &str) -> bool {
    !name.is_empty() && front_end::parses_as_variable(name)
}

/// A compiled expression, ready to be bound to a [`Schema`].
#[derive(Debug, Clone, PartialEq)]
pub struct Expression {
    source: String,
    program: ast::Program,
    /// Distinct statically-referenced names in first-reference order.
    /// [`ast::GlobalId`] indexes into *this*, not into the schema — the AST is
    /// built before any schema exists, so [`Expression::bind`] is what maps
    /// these onto row positions.
    symbols: Vec<String>,
    contains_dynamic_lookup: bool,
    is_boolean_expression: bool,
}

impl Expression {
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
    /// [`statically_referenced_symbols`]: Expression::statically_referenced_symbols
    #[must_use]
    pub const fn contains_dynamic_lookup(&self) -> bool {
        self.contains_dynamic_lookup
    }

    /// Whether the source was a boolean expression, and therefore whether the
    /// result should be read as a constraint residual rather than a value.
    #[must_use]
    pub const fn is_boolean_expression(&self) -> bool {
        self.is_boolean_expression
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

    /// Resolves this expression's symbols against `schema`.
    ///
    /// This is where missing values are reported — once per schema, rather than
    /// on every evaluation as the JVM implementation did.
    ///
    /// # Errors
    /// Returns [`BindError`] if the schema omits a symbol the expression needs.
    pub fn bind<'a>(&'a self, schema: &'a Schema) -> Result<Bound<'a>, BindError> {
        let mut global_positions = Vec::with_capacity(self.symbols.len());
        let mut missing = Vec::new();

        for symbol in &self.symbols {
            match schema.names.iter().position(|name| name == symbol) {
                Some(position) => {
                    global_positions.push(u32::try_from(position).unwrap_or(u32::MAX));
                }
                None => missing.push(symbol.clone()),
            }
        }

        if !missing.is_empty() {
            return Err(BindError { missing });
        }

        Ok(Bound {
            expression: self,
            schema,
            global_positions,
        })
    }

    /// Binds and evaluates in one step. Convenient for tests and one-offs;
    /// prefer [`Expression::bind`] when evaluating many rows.
    ///
    /// # Errors
    /// Returns [`EvalError`] if binding or evaluation fails.
    pub fn evaluate(&self, inputs: &[(&str, f64)]) -> Result<f64, EvalError> {
        let schema = Schema::new(inputs.iter().map(|(name, _)| *name));
        let bound = self.bind(&schema)?;
        let row: Vec<f64> = inputs.iter().map(|(_, value)| *value).collect();
        bound.evaluate(&row)
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

/// An [`Expression`] with its symbols resolved against a [`Schema`].
///
/// This is the seam the batched evaluator will grow from: a flattened tape will
/// live here, and `evaluate_batch` becomes an additive change.
#[derive(Debug, Clone)]
pub struct Bound<'a> {
    expression: &'a Expression,
    /// The schema this is bound to. Held rather than partially copied: it
    /// gives the expected row width, and the names a runtime failure needs to
    /// report the values it was given.
    schema: &'a Schema,
    /// `ast::GlobalId` -> position in the row.
    global_positions: Vec<u32>,
}

impl<'a> Bound<'a> {
    #[must_use]
    pub const fn expression(&self) -> &'a Expression {
        self.expression
    }

    #[must_use]
    pub const fn schema(&self) -> &'a Schema {
        self.schema
    }

    /// Evaluates against one row of values, ordered per the bound schema.
    ///
    /// # Errors
    /// Returns [`EvalError`] if the row width is wrong or evaluation fails.
    pub fn evaluate(&self, row: &[f64]) -> Result<f64, EvalError> {
        if row.len() != self.schema.len() {
            return Err(EvalError::RowWidthMismatch {
                expected: self.schema.len(),
                actual: row.len(),
            });
        }

        let globals: Vec<f64> = self
            .global_positions
            .iter()
            .map(|&p| row[p as usize])
            .collect();

        eval::evaluate(&self.expression.program, &globals, row).map_err(|fault| {
            // The evaluator reports a kind and a location; rendering needs the
            // source, which it deliberately does not carry. Building the
            // `Problem` here keeps line and column derived from `span.start` in
            // the one place that does it for syntax errors too.
            EvalError::Runtime(Box::new(RuntimeProblem {
                problem: Problem::new(fault.kind, self.expression.source(), fault.span),
                // Needs a slot-to-name table the AST deliberately discards.
                locals: Vec::new(),
                parameters: self
                    .schema
                    .names()
                    .iter()
                    .cloned()
                    .zip(row.iter().copied())
                    .collect(),
            }))
        })
    }
}
