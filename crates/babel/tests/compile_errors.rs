//! Compile-time diagnostics, ported from `BabelCompilerErrorFixture.kt`.
//!
//! Two deliberate departures from the Kotlin fixture:
//!
//! * **Structured fields only.** The Kotlin tests asserted on rendered
//!   caret-annotated strings and on `abbreviatedProblemText`. Neither is ported;
//!   rendering is a presentation concern.
//! * **Consistent columns.** Kotlin's `characterNo` does not follow a single
//!   rule — `x1 + x2 +` reports range `8..8` with `characterNo` 9, while
//!   `1+(x > 3) + 2` reports range `5..5` with `characterNo` 5. Here `column`
//!   is always the zero-based column of `span.start`.
//!
//! Kotlin's `rangeInText` was an inclusive `IntRange`; [`Span`] is half-open, so
//! Kotlin's `12..13` becomes `Span::new(12, 14)`.
//!
//! **Provenance of the position values below:** cases marked `// observed` use
//! positions produced by the real ANTLR Rust runtime during the Step 0 spike.
//! Cases marked `// from kotlin` are translated from the JVM implementation and
//! should be re-verified against actual runtime output as V0.1 lands — the two
//! runtimes anchor end-of-input errors differently.

use babel::CompilationFailure;
use babel::diagnostics::{BoundKind, ProblemKind, Span};

fn compile_to_failure(expr: &str) -> CompilationFailure {
    match babel::compile(expr) {
        Ok(_) => panic!("expected {expr:?} to fail compilation, but it succeeded"),
        Err(failure) => failure,
    }
}

/// Asserts a single problem with the given classification and location.
fn assert_single(expr: &str, kind: ProblemKind, span: Span, line: u32, column: u32) {
    let failure = compile_to_failure(expr);
    assert_eq!(
        failure.problems.len(),
        1,
        "expected exactly one problem for {expr:?}, got {:#?}",
        failure.problems
    );
    let p = &failure.problems[0];
    assert_eq!(p.kind, kind, "kind for {expr:?}");
    assert_eq!(p.span, span, "span for {expr:?}");
    assert_eq!(p.line, line, "line for {expr:?}");
    assert_eq!(p.column, column, "column for {expr:?}");
}

#[test]
fn empty_expression_fails_eagerly() {
    let failure = compile_to_failure("");
    assert_eq!(failure.problems.len(), 1);
    assert_eq!(failure.problems[0].kind, ProblemKind::EmptyExpression);
}

#[test]
fn dangling_operator() {
    // observed: line 1, column 9, span 9..9 (anchored at EOF, not at the '+')
    assert_single("x1 + x2 +", ProblemKind::SyntaxError, Span::new(9, 9), 1, 9);
}

#[test]
fn illegal_character() {
    // observed: token recognition error, span covers the single character
    assert_single("1 + @x1", ProblemKind::SyntaxError, Span::new(4, 5), 1, 4);
}

#[test]
fn nan_lower_bound_is_caught_at_compile_time() {
    // from kotlin: range 4..6 inclusive over "0/0"
    let failure = compile_to_failure("sum(0/0, 20, i -> i + 2)");
    assert_eq!(failure.problems.len(), 1, "{:#?}", failure.problems);
    let p = &failure.problems[0];
    assert_eq!(p.kind, ProblemKind::IllegalAggregateBound(BoundKind::Lower));
    assert_eq!(p.span, Span::new(4, 7));
    assert_eq!(p.detail, "evaluates to NaN");
}

#[test]
fn equality_without_bound() {
    // from kotlin: range 6..6, i.e. end of input
    assert_single("x1 = x2", ProblemKind::SyntaxError, Span::new(6, 7), 1, 6);
}

#[test]
fn equality_with_non_literal_bound() {
    // from kotlin: range 12..13 inclusive over "x3"
    assert_single(
        "x1 = x2 +/- x3",
        ProblemKind::SyntaxError,
        Span::new(12, 14),
        1,
        12,
    );
}

#[test]
fn nested_boolean_clause_is_rejected() {
    // from kotlin: range 5..5 over the '>'
    assert_single(
        "1+(x > 3) + 2",
        ProblemKind::SyntaxError,
        Span::new(5, 6),
        1,
        5,
    );
}

#[test]
fn chained_equality_without_bound() {
    // from kotlin: range 23..23, end of input
    assert_single(
        "P1+P2+P3+P4+P5+P6+P7==30",
        ProblemKind::SyntaxError,
        Span::new(23, 24),
        1,
        23,
    );
}
