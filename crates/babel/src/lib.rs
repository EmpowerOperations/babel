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

pub mod ast;
pub mod diagnostics;

mod eval;
mod generated;
mod lower;

pub use diagnostics::{BindError, CompilationFailure, EvalError, Problem, ProblemKind, Span};

use std::collections::BTreeSet;

/// Compiles source text into an evaluable expression.
///
/// # Errors
/// Returns [`CompilationFailure`] with every problem found; compilation does
/// not stop at the first one.
pub fn compile(_source: &str) -> Result<Expression, CompilationFailure> {
    todo!("V0")
}

/// Whether `name` is a legal Babel variable name.
///
/// Babel accepts Unicode identifiers, so `π`, `测试` and `☕` are all legal.
#[must_use]
pub fn is_legal_variable_name(_name: &str) -> bool {
    todo!("V0")
}

/// A compiled expression, ready to be bound to a [`Schema`].
#[derive(Debug, Clone, PartialEq)]
pub struct Expression {
    source: String,
    program: ast::Program,
    /// Distinct statically-referenced names in first-reference order.
    /// [`ast::GlobalIdx`] indexes into *this*, not into the schema — the AST is
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

    /// Whether the expression uses `var[i]` dynamic lookup. Such expressions
    /// read the whole row by position, so they cannot declare their symbol
    /// dependencies statically.
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
    /// [`ast::GlobalIdx`].
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
    pub fn bind<'e>(&'e self, _schema: &Schema) -> Result<Bound<'e>, BindError> {
        todo!("V0")
    }

    /// Binds and evaluates in one step. Convenient for tests and one-offs;
    /// prefer [`Expression::bind`] when evaluating many rows.
    ///
    /// # Errors
    /// Returns [`EvalError`] if binding or evaluation fails.
    pub fn evaluate(&self, _inputs: &[(&str, f64)]) -> Result<f64, EvalError> {
        todo!("V0")
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
        Self { names: names.into_iter().map(Into::into).collect() }
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
pub struct Bound<'e> {
    expression: &'e Expression,
    /// `ast::GlobalIdx` -> position in the row.
    global_positions: Vec<u32>,
    /// Expected row width, i.e. the schema's length.
    width: usize,
}

impl<'e> Bound<'e> {
    #[must_use]
    pub const fn expression(&self) -> &'e Expression {
        self.expression
    }

    /// Evaluates against one row of values, ordered per the bound schema.
    ///
    /// # Errors
    /// Returns [`EvalError`] if the row width is wrong or evaluation fails.
    pub fn evaluate(&self, _row: &[f64]) -> Result<f64, EvalError> {
        todo!("V0")
    }
}
