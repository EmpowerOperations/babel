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
fn a_boolean_cannot_be_used_as_a_scalar() {
    // The grammar admits `booleanExpr` only at `returnStatement`, so there is
    // nowhere for the `* 3` to attach and these are syntax errors.
    //
    // Worth pinning: the JVM implementation carried a whole semantic check
    // (`TypeErrorReportingWalker`, and the `BooleanInScalarPosition` problem it
    // raised) for this case, because its rewriter would turn `x1 > 5` into
    // `5 - x1` in place and the surrounding arithmetic would then compile
    // happily. Rejecting at the grammar makes that unreachable, and this test is
    // what lets the check stay deleted.
    for expression in ["(x1 > 5) * 3", "(x1 > 5) * 3 < 0"] {
        assert_reports(expression, "a syntax error", |p| {
            matches!(&p.kind, ProblemKind::Syntax { .. })
        });
    }
}

#[test]
fn chained_equality_without_bound() {
    assert_syntax_at("P1+P2+P3+P4+P5+P6+P7==30", Span::new(24, 24), 24, false);
}

#[test]
fn nan_lower_bound_is_caught_at_compile_time() {
    // Needs constant folding to know 0/0 is statically NaN. Red until then.
    assert_reports("sum(0/0, 20, i -> i + 2)", "an illegal lower bound", |p| {
        matches!(&p.kind, ProblemKind::IllegalAggregateBound { .. })
    });
}

/// `{:#}` is the expanded form — source and caret, monospace assumed. The
/// alternate flag means "more elaborate" throughout the standard library
/// (`{:#?}`, `{:#x}`), so it means that here too.
#[test]
fn alternate_display_renders_a_caret_block() {
    let failure = compile_to_failure("x1 + x2 +");
    let rendered = format!("{:#}", failure.problems[0]);
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

/// Plain `{}` is the one-liner a caller puts in a log.
#[test]
fn plain_display_is_a_single_line() {
    let failure = compile_to_failure("x1 + x2 +");
    let rendered = failure.problems[0].to_string();

    assert_eq!(rendered.lines().count(), 1, "got {rendered:?}");
    assert!(
        rendered.starts_with("syntax error at 'end of expression'"),
        "got {rendered:?}"
    );
}
