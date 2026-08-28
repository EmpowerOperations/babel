//! Compile-time diagnostics, ported from `BabelCompilerErrorFixture.kt`.
//!
//! Babel forwards ANTLR's syntax diagnostics rather than curating them, so for
//! these cases **ANTLR's own output is the oracle**. A parser recovering from
//! one mistake often emits several diagnostics, and the count is a recovery
//! detail rather than a contract — so each case asserts that *some* reported
//! problem matches, never that exactly one was reported.
//!
//! Departures from the Kotlin fixture:
//!
//! * Structured fields only. Kotlin asserted on rendered caret strings and on
//!   `abbreviatedProblemText`; neither is ported.
//! * `line_idx` and `column_idx` are zero-based and both derived from
//!   `span.start`. Kotlin's `lineNo` was one-based and its `characterNo` was
//!   computed inconsistently between call sites.
//! * Kotlin's `rangeInText` was an inclusive `IntRange`; [`Span`] is half-open.

use babel::CompilationFailure;
use babel::diagnostics::{Problem, ProblemKind, Span};

fn compile_to_failure(expr: &str) -> CompilationFailure {
    match babel::compile(expr) {
        Ok(_) => panic!("expected {expr:?} to fail compilation, but it succeeded"),
        Err(failure) => failure,
    }
}

/// Asserts that at least one reported problem satisfies `predicate`.
fn assert_reports(expr: &str, description: &str, predicate: impl Fn(&Problem) -> bool) {
    let failure = compile_to_failure(expr);
    assert!(
        failure.problems.iter().any(predicate),
        "expected {expr:?} to report {description}, got {:#?}",
        failure.problems
    );
}

/// The common shape: a syntax error at a known place.
fn assert_syntax_at(expr: &str, span: Span, column_idx: u32, from_lexer: bool) {
    assert_reports(
        expr,
        &format!("a syntax error at {span:?} (from_lexer={from_lexer})"),
        |p| {
            matches!(&p.kind, ProblemKind::Syntax { from_lexer: l, .. } if *l == from_lexer)
                && p.span == span
                && p.line_idx == 0
                && p.column_idx == column_idx
        },
    );
}

#[test]
fn empty_expression_fails_eagerly() {
    // The one case where an exact count really is the contract: babel rejects
    // this before ANTLR is ever involved.
    let failure = compile_to_failure("");
    assert_eq!(failure.problems.len(), 1);
    assert_eq!(failure.problems[0].kind, ProblemKind::EmptyExpression);
}

#[test]
fn dangling_operator() {
    assert_syntax_at("x1 + x2 +", Span::new(9, 9), 9, false);
}

#[test]
fn illegal_character() {
    assert_syntax_at("1 + @x1", Span::new(4, 5), 4, true);
}

#[test]
fn equality_without_bound() {
    assert_syntax_at("x1 = x2", Span::new(7, 7), 7, false);
}

#[test]
fn equality_with_non_literal_bound() {
    assert_syntax_at("x1 = x2 +/- x3", Span::new(12, 14), 12, false);
}

#[test]
fn nested_boolean_clause_is_rejected() {
    assert_syntax_at("1+(x > 3) + 2", Span::new(5, 6), 5, false);
}

#[test]
fn chained_equality_without_bound() {
    assert_syntax_at("P1+P2+P3+P4+P5+P6+P7==30", Span::new(24, 24), 24, false);
}

#[test]
fn unsupported_constructs_are_not_syntax_errors() {
    // `sum(1,3,i->i)` is perfectly good babel; this build just cannot lower it
    // yet. Reporting it as a syntax error would be a lie.
    assert_reports("sum(1, 3, i -> i)", "an unsupported-feature problem", |p| {
        matches!(&p.kind, ProblemKind::Unsupported { .. })
    });
}

#[test]
fn nan_lower_bound_is_caught_at_compile_time() {
    // Needs constant folding to know 0/0 is statically NaN. Red until then.
    assert_reports("sum(0/0, 20, i -> i + 2)", "an illegal lower bound", |p| {
        matches!(&p.kind, ProblemKind::IllegalAggregateBound { .. })
    });
}

/// Renders a known failure so `Display` is pinned by something other than
/// inspection.
#[test]
fn display_renders_a_caret_block() {
    let failure = compile_to_failure("x1 + x2 +");
    let rendered = failure.problems[0].to_string();
    let lines: Vec<&str> = rendered.lines().collect();

    assert_eq!(lines[0], "Error in 'end of expression': syntax error.");
    assert_eq!(lines[1], "x1 + x2 +");
    // Zero-width end-of-input span underlines the final character.
    assert!(
        lines[2].starts_with("        ~"),
        "expected the caret under the trailing '+', got {:?}",
        lines[2]
    );
}
