//! Structured compile-time and run-time diagnostics.
//!
//! The Kotlin implementation rendered problems into caret-annotated strings and
//! asserted on that text. This port keeps only the structured fields; rendering
//! is a presentation concern and belongs to whoever displays the problem.

use std::fmt;
use std::ops::Range;

/// Half-open range over the source text, measured in Unicode scalar values.
///
/// Character offsets rather than UTF-8 byte offsets: Babel accepts Unicode
/// identifiers, and consumers place carets by character, not by byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    #[must_use]
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// Converts a UTF-8 byte range — the form the ANTLR runtime reports in
    /// [`SyntaxErrorEvent::span`] — into character offsets.
    ///
    /// [`SyntaxErrorEvent::span`]: antlr4_runtime::errors::SyntaxErrorEvent
    #[must_use]
    pub fn from_utf8_range(_source: &str, _bytes: Range<usize>) -> Self {
        todo!("V0: byte-offset to char-offset conversion")
    }
}

/// Which aggregate bound was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundKind {
    Lower,
    Upper,
}

/// The classification of a problem.
///
/// An enum rather than the Kotlin `summary: String`, so tests assert on the
/// classification instead of on prose that is free to change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProblemKind {
    /// The supplied source text was empty.
    EmptyExpression,
    /// The parser or lexer rejected the input.
    SyntaxError,
    /// A boolean-valued expression appeared where a scalar was required.
    BooleanInScalarPosition,
    /// A `sum`/`prod` bound was NaN, infinite, or otherwise unusable.
    IllegalAggregateBound(BoundKind),
    /// `var[i]` addressed a parameter that does not exist.
    IndexOutOfBounds { requested: i64, available: usize },
}

/// A single compile-time or run-time problem, located in the source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub kind: ProblemKind,
    pub span: Span,
    /// One-based line number.
    pub line: u32,
    /// Zero-based column within `line`, in characters.
    pub column: u32,
    /// Short description of the offending value, e.g. `"evaluates to NaN"`.
    pub detail: String,
}

/// Compilation produced no evaluable expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilationFailure {
    pub source: String,
    pub problems: Vec<Problem>,
}

impl fmt::Display for CompilationFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "failed to compile {:?}: {} problem(s)",
            self.source,
            self.problems.len()
        )
    }
}

impl std::error::Error for CompilationFailure {}

/// An expression could not be bound to a [`Schema`](crate::Schema).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindError {
    /// Symbols the expression references that the schema does not supply.
    pub missing: Vec<String>,
}

impl fmt::Display for BindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "missing value(s) for {}", self.missing.join(", "))
    }
}

impl std::error::Error for BindError {}

/// Evaluation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    /// The expression referenced state the schema does not provide.
    Bind(BindError),
    /// A problem arose while evaluating, located in the source text.
    Runtime(Problem),
    /// The row handed to `evaluate` did not match the bound schema's width.
    RowWidthMismatch { expected: usize, actual: usize },
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bind(e) => write!(f, "{e}"),
            Self::Runtime(p) => write!(f, "{:?} at line {}:{}", p.kind, p.line, p.column),
            Self::RowWidthMismatch { expected, actual } => {
                write!(f, "expected a row of {expected} value(s), got {actual}")
            }
        }
    }
}

impl std::error::Error for EvalError {}

impl From<BindError> for EvalError {
    fn from(e: BindError) -> Self {
        Self::Bind(e)
    }
}
