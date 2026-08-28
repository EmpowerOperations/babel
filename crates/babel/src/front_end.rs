//! Lexes and parses text into [`crate::ast`].
//!
//! **This is the only module allowed to name `antlr4_runtime`.** ANTLR exists
//! purely as a compile step; once [`translate`] returns, everything downstream deals
//! in babel's own owned types. The borrow checker helps enforce this — the parse
//! tree is arena-owned and every typed context borrows from it, so an AST
//! holding ANTLR types could not outlive the parse.
//!
//! The grammar has no labelled alternatives, so `scalarExpr` yields a single
//! `ScalarExprContext` and semantics has to work out which alternative matched.
//! The generated code emits a typed accessor per possible child, so that is a
//! question of which accessor is `Some` — see [`SemanticTranslator::translate_scalar_expr`].

use std::sync::{Arc, Mutex};

use antlr4_runtime::errors::{ErrorListener, SyntaxErrorEvent};
use antlr4_runtime::{CommonTokenStream, FromRuleNode, InputStream, ParsedFile, Recognizer};

use crate::ast::{BinaryOp, Block, CompareOp, Expr, GlobalId, Kind, Program, UnaryOp};
use crate::diagnostics::{Problem, ProblemKind, Span};
use crate::generated::lexer::BabelLexer;
use crate::generated::parser::{
    self, BabelParser, BooleanExprContext, LiteralContext, ScalarEvaluableContext,
    ScalarExprContext,
};

/// Everything compilation learns from walking the parse tree.
pub(crate) struct AbstractSyntaxTree {
    pub program: Program,
    /// Distinct statically-referenced names in first-reference order.
    pub symbols: Vec<String>,
    pub contains_dynamic_lookup: bool,
    pub is_boolean_expression: bool,
}

/// Collects syntax errors from both the lexer and the parser.
///
/// Every diagnostic is forwarded as reported — no filtering, no coalescing, no
/// rewording. A parser recovering from one mistake may emit several; choosing
/// between them is the caller's business.
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

        // The lexer reports no offending token; the parser always does. That is
        // the only classification available without reading the message, since
        // the runtime formats `MismatchedInput { expected, found }` into prose
        // before listeners see it.
        let kind = ProblemKind::Syntax {
            message: event.message.to_owned(),
            from_lexer: event.offending.is_none(),
        };

        self.problems
            .lock()
            .expect("error sink poisoned")
            .push(Problem::new(kind, &self.source, span));
    }
}

/// Anything babel parses but this build cannot yet translate.
///
/// These are emphatically not syntax errors — `sum(1,3,i->i)` is valid babel.
fn unsupported(feature: &str, source: &str, span: Span) -> Problem {
    // translate cannot localize these yet — `span_of` is still stubbed, so an
    // empty span here means "no idea where". Blaming the whole expression is
    // honest; pointing at character zero is not. This narrows on its own once
    // real spans land.
    let span = if span.is_empty() {
        Span::new(0, u32::try_from(source.chars().count()).unwrap_or(u32::MAX))
    } else {
        span
    };

    Problem::new(
        ProblemKind::Unsupported {
            feature: feature.to_owned(),
        },
        source,
        span,
    )
}

/// Parses `source` from the `scalar_evaluable` entry rule and translates the result to an AST.
///
/// # Errors
/// Returns every syntax error found, or a single "not supported yet" problem for
/// constructs outside V0.1's scope.
pub(crate) fn translate(source: &str) -> Result<AbstractSyntaxTree, Vec<Problem>> {
    let sink = ErrorSink::new(source);

    let parsed: ParsedFile = parser::parse_with_parser_constructor(
        source,
        |input| make_lexer(input, sink.clone()),
        |tokens| make_parser(tokens, sink.clone()),
        BabelParser::scalar_evaluable,
    )
    .map_err(|e| {
        vec![Problem::new(
            ProblemKind::Syntax {
                message: e.to_string(),
                from_lexer: false,
            },
            source,
            Span::new(0, 0),
        )]
    })?
    .into_parsed_file();

    let errors = sink.take();
    if !errors.is_empty() {
        return Err(errors);
    }

    let root = parsed
        .tree()
        .as_rule()
        .and_then(ScalarEvaluableContext::from_rule_node)
        .ok_or_else(|| vec![unsupported("this expression", source, Span::new(0, 0))])?;

    let translator = SemanticTranslator { source };

    translator.translate_program(&root)
}

/// Whether `name` parses cleanly as a lone variable.
pub(crate) fn parses_as_variable(name: &str) -> bool {
    let sink = ErrorSink::new(name);

    let parsed = parser::parse_with_parser_constructor(
        name,
        |input| make_lexer(input, sink.clone()),
        |tokens| make_parser(tokens, sink.clone()),
        BabelParser::variable_only,
    );

    parsed.is_ok() && sink.take().is_empty()
}

fn make_parser(
    tokens: CommonTokenStream<BabelLexer<InputStream>>,
    err_sink: ErrorSink,
) -> BabelParser<BabelLexer<InputStream>> {
    let mut parser = BabelParser::new(tokens);
    parser.remove_error_listeners();
    parser.add_error_listener(err_sink);
    parser
}

fn make_lexer(input: InputStream, err_sink: ErrorSink) -> BabelLexer<InputStream> {
    let mut lexer = BabelLexer::new(input);
    lexer.remove_error_listeners();
    lexer.add_error_listener(err_sink);
    lexer
}

/// Translates a parse tree into an AST.
///
/// Deliberately holds no accumulator state: everything the translation produces
/// comes back through the return value, and the symbol table is threaded as an
/// explicit parameter. That keeps every method `&self` and stops the outputs
/// leaking out through a side channel.
struct SemanticTranslator<'a> {
    source: &'a str,
}

/// Hands out the [`GlobalId`]s the AST stores, remembering the distinct names
/// in first-reference order.
#[derive(Default)]
struct SymbolTable {
    names: Vec<String>,
}

impl SymbolTable {
    /// Resolves a name, registering it on first reference so ordering is stable.
    fn get_or_make_id(&mut self, name: &str) -> GlobalId {
        let position = self.names.iter().position(|known| known == name);
        let position = match position {
            Some(position) => position,
            None => {
                self.names.push(name.to_owned());
                self.names.len() - 1
            }
        };
        GlobalId::from_index(u32::try_from(position).unwrap_or(u32::MAX))
    }
}

impl SemanticTranslator<'_> {
    /// Walks a parsed `scalar_evaluable` tree and builds the AST.
    fn translate_program(
        &self,
        ctx: &ScalarEvaluableContext<'_>,
    ) -> Result<AbstractSyntaxTree, Vec<Problem>> {
        let mut symbols = SymbolTable::default();
        let block = ctx.statement_block().map_err(|_| {
            vec![unsupported(
                "an empty expression",
                self.source,
                Span::new(0, 0),
            )]
        })?;

        // `(statement ';')* returnStatement ';'?` — V0.1 handles no statements,
        // so any assignment is out of scope.
        if block.statement_children().next().is_some() {
            return Err(vec![unsupported(
                "assignments",
                self.source,
                Span::new(0, 0),
            )]);
        }

        let ret = block.return_statement().map_err(|_| {
            vec![unsupported(
                "an empty expression",
                self.source,
                Span::new(0, 0),
            )]
        })?;

        // `returnStatement : 'return'? booleanExpr | 'return'? scalarExpr` —
        // the only place the grammar admits a boolean.
        let is_boolean_expression = ret.boolean_expr().is_some();

        let result = if let Some(boolean) = ret.boolean_expr() {
            self.translate_boolean_expr(&boolean, &mut symbols)?
        } else {
            let scalar = ret.scalar_expr().ok_or_else(|| {
                vec![unsupported(
                    "an empty expression",
                    self.source,
                    Span::new(0, 0),
                )]
            })?;
            self.translate_scalar_expr(&scalar, &mut symbols)?
        };

        Ok(AbstractSyntaxTree {
            program: Program {
                body: Block {
                    assignments: Vec::new(),
                    result,
                },
                // No locals until assignments and lambdas land.
                frame_size: 0,
            },
            symbols: symbols.names,
            // `var[i]` is still rejected during translation, so this stays false.
            contains_dynamic_lookup: false,
            is_boolean_expression,
        })
    }

    // These survive into the AST as [`Kind::Compare`] and [`Kind::NearEq`] and
    // are eliminated by [`crate::rewrite::rewrite_booleans`]; the evaluator
    // never sees them.
    fn translate_boolean_expr(
        &self,
        ctx: &BooleanExprContext<'_>,
        symbols: &mut SymbolTable,
    ) -> Result<Expr, Vec<Problem>> {
        // TODO(spans): `span_of` is still stubbed; see lower_scalar_expr.
        let span = Span::new(0, 0);

        // `'(' booleanExpr ')'` — grouping adds no node.
        if let Some(inner) = ctx.boolean_expr() {
            return self.translate_boolean_expr(&inner, symbols);
        }

        let children: Vec<_> = ctx.scalar_expr_children().collect();
        let lhs = self.operand(&children, 0, span, symbols)?;
        let rhs = self.operand(&children, 1, span, symbols)?;

        // `scalarExpr eq scalarExpr plusMinus literal` — the grammar requires a
        // literal tolerance, so it is always statically known.
        if ctx.eq().is_some() {
            let literal = ctx
                .literal()
                .ok_or_else(|| vec![unsupported("this equality", self.source, span)])?;
            let tolerance = translate_literal(&literal, self.source)?;
            return Ok(Expr::new(
                Kind::NearEq {
                    lhs,
                    rhs,
                    tolerance,
                },
                span,
            ));
        }

        let op = if ctx.lteq().is_some() {
            CompareOp::Lte
        } else if ctx.gteq().is_some() {
            CompareOp::Gte
        } else if ctx.lt().is_some() {
            CompareOp::Lt
        } else if ctx.gt().is_some() {
            CompareOp::Gt
        } else {
            return Err(vec![unsupported("this comparison", self.source, span)]);
        };

        Ok(Expr::new(Kind::Compare { op, lhs, rhs }, span))
    }

    /// Lowers the `index`-th sub-expression of a node.
    fn operand(
        &self,
        children: &[ScalarExprContext<'_>],
        index: usize,
        span: Span,
        symbols: &mut SymbolTable,
    ) -> Result<Box<Expr>, Vec<Problem>> {
        let child = children
            .get(index)
            .ok_or_else(|| vec![unsupported("this expression", self.source, span)])?;
        Ok(Box::new(self.translate_scalar_expr(child, symbols)?))
    }

    fn translate_scalar_expr(
        &self,
        ctx: &ScalarExprContext<'_>,
        symbols: &mut SymbolTable,
    ) -> Result<Expr, Vec<Problem>> {
        // TODO(spans): generated contexts expose `start()` as a
        // `__GeneratedTokenView`, which carries only text — no byte offsets — so
        // real spans need `direct_terminals()` walking or a token-store lookup.
        let span = Span::new(0, 0);
        let children: Vec<_> = ctx.scalar_expr_children().collect();

        // Ordering matters: `open_paren_token` is present for grouping *and* for
        // every function call and aggregate, so grouping is tested last.

        if let Some(literal) = ctx.literal() {
            return Ok(Expr::new(
                Kind::Literal(translate_literal(&literal, self.source)?),
                span,
            ));
        }

        if let Some(variable) = ctx.variable() {
            let name = variable
                .variable_token()
                .map_or(String::new(), |t| t.symbol().text_or_empty().to_owned());
            let idx = symbols.get_or_make_id(&name);
            return Ok(Expr::new(Kind::Global(idx), span));
        }

        if ctx.var().is_some() {
            return Err(vec![unsupported(
                "dynamic variable access (var[i])",
                self.source,
                span,
            )]);
        }
        if ctx.sum().is_some() || ctx.prod().is_some() {
            return Err(vec![unsupported("sum and prod", self.source, span)]);
        }
        if ctx.lambda_expr().is_some() {
            return Err(vec![unsupported("lambdas", self.source, span)]);
        }

        if let Some(function) = ctx.binary_function() {
            let op = BinaryOp::from_function_keyword(function.text().as_ref())
                .ok_or_else(|| vec![unsupported("this binary function", self.source, span)])?;
            let lhs = self.operand(&children, 0, span, symbols)?;
            let rhs = self.operand(&children, 1, span, symbols)?;
            return Ok(Expr::new(Kind::Binary { op, lhs, rhs }, span));
        }

        if let Some(function) = ctx.unary_function() {
            let op = UnaryOp::from_keyword(function.text().as_ref())
                .ok_or_else(|| vec![unsupported("this unary function", self.source, span)])?;
            let arg = self.operand(&children, 0, span, symbols)?;
            return Ok(Expr::new(Kind::Unary { op, arg }, span));
        }

        // `negate : '-'` and `minus : '-'` are distinct rules, so unary minus
        // and binary subtraction never collide.
        if ctx.negate().is_some() {
            let arg = self.operand(&children, 0, span, symbols)?;
            return Ok(Expr::new(
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
            let lhs = self.operand(&children, 0, span, symbols)?;
            let rhs = self.operand(&children, 1, span, symbols)?;
            return Ok(Expr::new(Kind::Binary { op, lhs, rhs }, span));
        }

        // `'(' scalarExpr ')'` — grouping adds no node, it just returns the
        // child. Precedence is already encoded in the tree's shape.
        if children.len() == 1 {
            return self.translate_scalar_expr(&children[0], symbols);
        }

        Err(vec![unsupported("this expression", self.source, span)])
    }
}

/// `literal : '-'? (INTEGER | FLOAT | PI | EULERS_E)`
///
/// Note `-3` can arrive here as a negative literal *or* as `negate` applied to
/// `3`, depending on which alternative ANTLR picks. Both are valid and produce
/// the same value; `-3 - -3` and `-3--3` in the corpus pin that.
fn translate_literal(ctx: &LiteralContext<'_>, source: &str) -> Result<f64, Vec<Problem>> {
    let magnitude: f64 = if let Some(token) = ctx.integer_token().or_else(|| ctx.float_token()) {
        token
            .symbol()
            .text_or_empty()
            .parse::<f64>()
            .map_err(|_| vec![unsupported("this numeric literal", source, Span::new(0, 0))])?
    } else if ctx.pi_token().is_some() {
        std::f64::consts::PI
    } else if ctx.eulers_e_token().is_some() {
        std::f64::consts::E
    } else {
        return Err(vec![unsupported("this literal", source, Span::new(0, 0))]);
    };

    Ok(if ctx.minus_token().is_some() {
        -magnitude
    } else {
        magnitude
    })
}
