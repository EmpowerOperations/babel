//! Lexes, parses, and lowers source text into [`crate::ast`].
//!
//! **This is the only module allowed to name `antlr4_runtime`.** ANTLR exists
//! purely as a compile step; once [`lower`] returns, everything downstream deals
//! in babel's own owned types. The borrow checker helps enforce this — the parse
//! tree is arena-owned and every typed context borrows from it, so an AST
//! holding ANTLR types could not outlive the parse.
//!
//! The grammar has no labelled alternatives, so `scalarExpr` yields a single
//! `ScalarExprContext` and lowering has to work out which alternative matched.
//! The generated code emits a typed accessor per possible child, so that is a
//! question of which accessor is `Some` — see [`Lowerer::lower_scalar_expr`].

use std::sync::{Arc, Mutex};

use antlr4_runtime::Recognizer;
use antlr4_runtime::errors::{ErrorListener, SyntaxErrorEvent};

use crate::ast::{BinaryOp, Block, GlobalIdx, Kind, Node, Program, UnaryOp};
use crate::diagnostics::{Problem, ProblemKind, Span};
use crate::generated::lexer::BabelLexer;
use crate::generated::parser::{
    self, BabelParser, BabelVisitor, BinaryFunctionContext, LiteralContext, ScalarEvaluableContext,
    ScalarExprContext, UnaryFunctionContext,
};

/// Everything compilation learns from walking the parse tree.
pub(crate) struct Lowered {
    pub program: Program,
    /// Distinct statically-referenced names in first-reference order.
    pub symbols: Vec<String>,
    pub contains_dynamic_lookup: bool,
    pub is_boolean_expression: bool,
}

/// Collects syntax errors from both the lexer and the parser.
///
/// V0.1 only needs to know whether any occurred. Line and column are recorded
/// because they arrive for free and already come through character-based;
/// classification beyond [`ProblemKind::SyntaxError`] is V0.2 work.
#[derive(Clone)]
struct ErrorSink {
    source: Arc<str>,
    problems: Arc<Mutex<Vec<Problem>>>,
}

impl ErrorSink {
    fn new(source: &str) -> Self {
        Self {
            source: Arc::from(source),
            problems: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn take(&self) -> Vec<Problem> {
        std::mem::take(&mut *self.problems.lock().expect("error sink poisoned"))
    }
}

impl<R: Recognizer + ?Sized> ErrorListener<R> for ErrorSink {
    fn syntax_error(&mut self, _recognizer: &R, event: &SyntaxErrorEvent<'_>) {
        let span = event.span.clone().map_or(Span::new(0, 0), |bytes| {
            Span::from_utf8_range(&self.source, bytes)
        });

        self.problems
            .lock()
            .expect("error sink poisoned")
            .push(Problem {
                kind: ProblemKind::SyntaxError,
                span,
                line: u32::try_from(event.line).unwrap_or(0),
                column: u32::try_from(event.column).unwrap_or(0),
                detail: event.message.to_owned(),
            });
    }
}

/// Anything V0.1 parses but cannot yet lower.
fn unsupported(what: &str, span: Span) -> Problem {
    Problem {
        kind: ProblemKind::SyntaxError,
        span,
        line: 0,
        column: 0,
        detail: format!("{what} is not supported yet"),
    }
}

/// Parses `source` from the `scalar_evaluable` entry rule and lowers the result.
///
/// # Errors
/// Returns every syntax error found, or a single "not supported yet" problem for
/// constructs outside V0.1's scope.
pub(crate) fn lower(source: &str) -> Result<Lowered, Vec<Problem>> {
    let sink = ErrorSink::new(source);

    let parsed = parser::parse_with_parser_constructor(
        source,
        |input| {
            let mut lexer = BabelLexer::new(input);
            lexer.remove_error_listeners();
            lexer.add_error_listener(sink.clone());
            lexer
        },
        |tokens| {
            let mut parser = BabelParser::new(tokens);
            parser.remove_error_listeners();
            parser.add_error_listener(sink.clone());
            parser
        },
        BabelParser::scalar_evaluable,
    )
    .map_err(|e| {
        vec![Problem {
            kind: ProblemKind::SyntaxError,
            span: Span::new(0, 0),
            line: 0,
            column: 0,
            detail: e.to_string(),
        }]
    })?
    .into_parsed_file();

    let errors = sink.take();
    if !errors.is_empty() {
        return Err(errors);
    }

    let mut lowerer = Lowerer {
        symbols: Vec::new(),
    };
    let program = lowerer.visit(parsed.tree())?;

    Ok(Lowered {
        program,
        symbols: lowerer.symbols,
        // Both are false by construction in V0.1: `var[i]` and boolean
        // expressions are rejected during lowering.
        contains_dynamic_lookup: false,
        is_boolean_expression: false,
    })
}

/// Whether `name` parses cleanly as a lone variable.
pub(crate) fn parses_as_variable(name: &str) -> bool {
    let sink = ErrorSink::new(name);

    let parsed = parser::parse_with_parser_constructor(
        name,
        |input| {
            let mut lexer = BabelLexer::new(input);
            lexer.remove_error_listeners();
            lexer.add_error_listener(sink.clone());
            lexer
        },
        |tokens| {
            let mut parser = BabelParser::new(tokens);
            parser.remove_error_listeners();
            parser.add_error_listener(sink.clone());
            parser
        },
        BabelParser::variable_only,
    );

    parsed.is_ok() && sink.take().is_empty()
}

struct Lowerer {
    symbols: Vec<String>,
}

/// The generated visitor is used for exactly one hop: turning the untyped root
/// [`antlr4_runtime::Node`] into a typed [`ScalarEvaluableContext`]. Everything
/// below is plain recursion, which composes with `?` in a way the visitor's
/// single `Result` associated type does not.
impl BabelVisitor for Lowerer {
    type Result = Result<Program, Vec<Problem>>;

    fn default_result(&mut self) -> Self::Result {
        Err(vec![unsupported("this expression", Span::new(0, 0))])
    }

    fn visit_scalar_evaluable(&mut self, ctx: &ScalarEvaluableContext) -> Self::Result {
        let block = ctx
            .statement_block()
            .map_err(|_| vec![unsupported("an empty expression", Span::new(0, 0))])?;

        // `(statement ';')* returnStatement ';'?` — V0.1 handles no statements,
        // so any assignment is out of scope.
        if block.statement_children().next().is_some() {
            return Err(vec![unsupported("assignments", Span::new(0, 0))]);
        }

        let ret = block
            .return_statement()
            .map_err(|_| vec![unsupported("an empty expression", Span::new(0, 0))])?;

        if ret.boolean_expr().is_some() {
            return Err(vec![unsupported("boolean expressions", Span::new(0, 0))]);
        }

        let scalar = ret
            .scalar_expr()
            .ok_or_else(|| vec![unsupported("an empty expression", Span::new(0, 0))])?;

        let result = self.lower_scalar_expr(&scalar)?;

        Ok(Program {
            body: Block {
                assignments: Vec::new(),
                result,
            },
            // No locals until assignments and lambdas land.
            frame_size: 0,
        })
    }
}

impl Lowerer {
    /// Resolves a name to its index in [`Lowered::symbols`], appending on first
    /// reference so that ordering is stable.
    fn intern(&mut self, name: &str) -> GlobalIdx {
        let position = self
            .symbols
            .iter()
            .position(|s| s == name)
            .unwrap_or_else(|| {
                self.symbols.push(name.to_owned());
                self.symbols.len() - 1
            });
        GlobalIdx(u32::try_from(position).unwrap_or(u32::MAX))
    }

    fn lower_scalar_expr(&mut self, ctx: &ScalarExprContext<'_>) -> Result<Node, Vec<Problem>> {
        let span = span_of(ctx);
        let children: Vec<_> = ctx.scalar_expr_children().collect();

        let operand = |this: &mut Self, index: usize| -> Result<Box<Node>, Vec<Problem>> {
            let child = children
                .get(index)
                .ok_or_else(|| vec![unsupported("this expression", span)])?;
            Ok(Box::new(this.lower_scalar_expr(child)?))
        };

        // Ordering matters: `open_paren_token` is present for grouping *and* for
        // every function call and aggregate, so grouping is tested last.

        if let Some(literal) = ctx.literal() {
            return Ok(Node::new(Kind::Literal(lower_literal(&literal)?), span));
        }

        if let Some(variable) = ctx.variable() {
            let name = variable
                .variable_token()
                .map_or(String::new(), |t| t.symbol().text_or_empty().to_owned());
            return Ok(Node::new(Kind::Global(self.intern(&name)), span));
        }

        if ctx.var().is_some() {
            return Err(vec![unsupported("dynamic variable access (var[i])", span)]);
        }
        if ctx.sum().is_some() || ctx.prod().is_some() {
            return Err(vec![unsupported("sum and prod", span)]);
        }
        if ctx.lambda_expr().is_some() {
            return Err(vec![unsupported("lambdas", span)]);
        }

        if let Some(function) = ctx.binary_function() {
            let op = binary_function_op(&function)
                .ok_or_else(|| vec![unsupported("this binary function", span)])?;
            let lhs = operand(self, 0)?;
            let rhs = operand(self, 1)?;
            return Ok(Node::new(Kind::Binary { op, lhs, rhs }, span));
        }

        if let Some(function) = ctx.unary_function() {
            let op = unary_function_op(&function)
                .ok_or_else(|| vec![unsupported("this unary function", span)])?;
            let arg = operand(self, 0)?;
            return Ok(Node::new(Kind::Unary { op, arg }, span));
        }

        // `negate : '-'` and `minus : '-'` are distinct rules, so unary minus
        // and binary subtraction never collide.
        if ctx.negate().is_some() {
            let arg = operand(self, 0)?;
            return Ok(Node::new(
                Kind::Unary {
                    op: UnaryOp::Negate,
                    arg,
                },
                span,
            ));
        }

        let binary = if ctx.raise().is_some() {
            Some(BinaryOp::Pow)
        } else if ctx.mult().is_some() {
            Some(BinaryOp::Mul)
        } else if ctx.div().is_some() {
            Some(BinaryOp::Div)
        } else if ctx.r#mod().is_some() {
            Some(BinaryOp::Mod)
        } else if ctx.plus().is_some() {
            Some(BinaryOp::Add)
        } else if ctx.minus().is_some() {
            Some(BinaryOp::Sub)
        } else {
            None
        };

        if let Some(op) = binary {
            let lhs = operand(self, 0)?;
            let rhs = operand(self, 1)?;
            return Ok(Node::new(Kind::Binary { op, lhs, rhs }, span));
        }

        // `'(' scalarExpr ')'` — grouping adds no node, it just returns the
        // child. Precedence is already encoded in the tree's shape.
        if children.len() == 1 {
            return self.lower_scalar_expr(&children[0]);
        }

        Err(vec![unsupported("this expression", span)])
    }
}

/// `literal : '-'? (INTEGER | FLOAT | PI | EULERS_E)`
///
/// Note `-3` can arrive here as a negative literal *or* as `negate` applied to
/// `3`, depending on which alternative ANTLR picks. Both are valid and produce
/// the same value; `-3 - -3` and `-3--3` in the corpus pin that.
fn lower_literal(ctx: &LiteralContext<'_>) -> Result<f64, Vec<Problem>> {
    let magnitude = if let Some(token) = ctx.integer_token().or_else(|| ctx.float_token()) {
        token
            .symbol()
            .text_or_empty()
            .parse::<f64>()
            .map_err(|_| vec![unsupported("this numeric literal", Span::new(0, 0))])?
    } else if ctx.pi_token().is_some() {
        std::f64::consts::PI
    } else if ctx.eulers_e_token().is_some() {
        std::f64::consts::E
    } else {
        return Err(vec![unsupported("this literal", Span::new(0, 0))]);
    };

    Ok(if ctx.minus_token().is_some() {
        -magnitude
    } else {
        magnitude
    })
}

fn unary_function_op(ctx: &UnaryFunctionContext<'_>) -> Option<UnaryOp> {
    // The rule wraps exactly one keyword token, so matching its text is both
    // shorter and clearer than twenty `is_some()` probes.
    Some(match ctx.text().as_ref() {
        "cos" => UnaryOp::Cos,
        "sin" => UnaryOp::Sin,
        "tan" => UnaryOp::Tan,
        "acos" => UnaryOp::Acos,
        "asin" => UnaryOp::Asin,
        "atan" => UnaryOp::Atan,
        "cosh" => UnaryOp::Cosh,
        "sinh" => UnaryOp::Sinh,
        "tanh" => UnaryOp::Tanh,
        "cot" => UnaryOp::Cot,
        // Babel renames Java's log/log10 to ln/log respectively.
        "ln" => UnaryOp::Ln,
        "log" => UnaryOp::Log10,
        "abs" => UnaryOp::Abs,
        "sqrt" => UnaryOp::Sqrt,
        "cbrt" => UnaryOp::Cbrt,
        "sqr" => UnaryOp::Sqr,
        "cube" => UnaryOp::Cube,
        "ceil" => UnaryOp::Ceil,
        "floor" => UnaryOp::Floor,
        "sgn" => UnaryOp::Sgn,
        _ => return None,
    })
}

fn binary_function_op(ctx: &BinaryFunctionContext<'_>) -> Option<BinaryOp> {
    Some(match ctx.text().as_ref() {
        "max" => BinaryOp::Max,
        "min" => BinaryOp::Min,
        "log" => BinaryOp::LogB,
        _ => return None,
    })
}

/// Source location for a context.
///
/// TODO(V0.2): currently always empty. Generated contexts expose `start()` as a
/// `__GeneratedTokenView`, which carries only text — no byte offsets — so real
/// spans need either `direct_terminals()` walking or a token-store lookup.
/// Nothing in V0.1 asserts on an AST span, and syntax-error spans come from
/// `SyntaxErrorEvent` instead, so this is deferred rather than half-built.
fn span_of(_ctx: &ScalarExprContext<'_>) -> Span {
    Span::new(0, 0)
}
