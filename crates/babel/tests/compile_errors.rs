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
use babel::diagnostics::{BoundKind, Problem, ProblemKind, Span};

fn compile_to_failure(expr: &str) -> CompilationFailure {
    match babel::parse(expr) {
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
fn a_statically_nan_bound_is_caught_at_compile_time() {
    // `0/0` is refused by constant folding before unrolling ever looks at the
    // bound, so the diagnostic points at the division rather than at "the lower
    // bound" — the more useful of the two.
    assert_reports(
        "sum(0/0, 20, i -> i + 2)",
        "a non-finite constant",
        |p| matches!(&p.kind, ProblemKind::NonFiniteConstant { value } if value.is_nan()),
    );
}

#[test]
fn a_fractional_bound_is_caught_at_compile_time() {
    // The other half of the same story, and why `IllegalAggregateBound` is
    // still reachable at compile time: `20/3` folds to a perfectly finite
    // 6.666…, which folding is happy with and `to_index` is not. The JVM
    // rounded it to 7 and said nothing.
    assert_reports("sum(1, 20/3, i -> i + 2)", "an illegal upper bound", |p| {
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

// --------------------------------------------- booleans are root-only

/// A comparison in a lambda body is a *parse* error, not a semantic one.
///
/// It used to parse, and then quietly sum constraint residuals as though they
/// were arithmetic: `sum(1, 3, i -> i > 2)` evaluated to `0.0` and
/// `prod(1, 3, i -> var a = i; a < 2)` to `-2.2e-308` — the strictness epsilon,
/// multiplied into a product. The JVM implementation went further and reported
/// the whole thing as a boolean expression.
///
/// `lambdaExpr` takes a `scalarBlock` now, which has no route to `booleanExpr`,
/// so the parser refuses it before meaning is ever assigned.
///
/// **The spans are the point.** ANTLR's wording is its own — "no viable
/// alternative at input …", which is jargon — but the caret lands exactly on
/// the offending operator, which is where a reader looks. If the wording ever
/// needs improving, the fix is an error alternative in the grammar rather than
/// a semantic check here.
#[test]
fn a_comparison_in_a_lambda_body_does_not_parse() {
    for (source, span, column) in [
        ("sum(1, 3, i -> i > 2)", Span::new(17, 18), 17),
        ("sum(1, 3, i -> i == 2 +/- 0.5)", Span::new(17, 19), 17),
        ("prod(1, 3, i -> var a = i; a < 2)", Span::new(29, 30), 29),
        ("sum(1, 3, i -> return i > 2)", Span::new(24, 25), 24),
    ] {
        assert_syntax_at(source, span, column, false);
    }
}

/// The other half, and the failure mode that matters more: a grammar change
/// that rejects too much. A lambda body is still a block, so statements and a
/// scalar result both have to survive.
#[test]
fn a_scalar_lambda_body_still_parses() {
    for source in [
        "sum(1, 3, i -> i)",
        "sum(1, 3, i -> var a = i + 1; a * 2)",
        "prod(1, 3, i -> return i * i)",
        "sum(1, 200, i -> var[i]^2 - 3.0)",
    ] {
        babel::parse(source)
            .unwrap_or_else(|e| panic!("{source:?} should parse: {:#?}", e.problems));
    }
}

// ------------------------------------------------------------- aggregates
//
// `sum` and `prod` are big-sigma and big-pi over a fixed index set, unrolled at
// compile time. A bound that is not a constant, or is a constant that is not an
// index, or a span wider than the unroll cap, is refused here rather than being
// a loop the evaluator would have to run one sample at a time.

#[test]
fn a_bound_that_depends_on_a_variable_does_not_compile() {
    for (source, bound, span) in [
        ("sum(1, x1, i -> i)", BoundKind::Upper, Span::new(7, 9)),
        (
            "sum(x1 + 0, x1 + 5, i -> var[i])",
            BoundKind::Lower,
            Span::new(4, 10),
        ),
        (
            "sum (\n  0,\n  20/x1,\n  i -> i + 2\n)",
            BoundKind::Upper,
            Span::new(13, 18),
        ),
        (
            "prod(1, ceil(sqrt(target)), i -> i)",
            BoundKind::Upper,
            Span::new(8, 26),
        ),
    ] {
        assert_reports(source, "a non-constant aggregate bound", |p| {
            p.kind == ProblemKind::AggregateBoundNotConstant { bound } && p.span == span
        });
    }
}

#[test]
fn a_constant_bound_that_is_not_an_index_does_not_compile() {
    for (source, bound, value) in [
        ("sum(1, 2.5, i -> i)", BoundKind::Upper, 2.5),
        ("sum(1.0e300, 20, i -> i)", BoundKind::Lower, 1e300),
    ] {
        assert_reports(source, "an illegal aggregate bound", |p| {
            p.kind == ProblemKind::IllegalAggregateBound { bound, value }
        });
    }
}

#[test]
fn an_aggregate_wider_than_the_unroll_cap_does_not_compile() {
    assert_reports("sum(1, 2000, i -> i)", "an aggregate past the cap", |p| {
        p.kind
            == ProblemKind::AggregateTooWide {
                terms: 2000,
                limit: 1024,
            }
    });
}
