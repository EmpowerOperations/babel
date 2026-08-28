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

use crate::ast::{
    AggregateKind, Assignment, BinaryOp, Block, CompareOp, Expr, GlobalId, Kind, LocalSlot,
    Program, UnaryOp,
};
use crate::diagnostics::{Problem, ProblemKind, Span};
use crate::generated::lexer::BabelLexer;
use crate::generated::parser::{
    self, BabelParser, BooleanExprContext, LiteralContext, ScalarEvaluableContext,
    ScalarExprContext, StatementBlockContext,
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
fn aggregate_kind(ctx: &ScalarExprContext<'_>) -> Option<AggregateKind> {
    if ctx.sum().is_some() {
        Some(AggregateKind::Sum)
    } else if ctx.prod().is_some() {
        Some(AggregateKind::Prod)
    } else {
        None
    }
}

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

/// Everything the translation accumulates, threaded explicitly rather than held
/// on the translator so it stays visible in every signature that touches it.
#[derive(Default)]
struct TranslationState {
    /// Distinct global names in first-reference order. A name only lands here
    /// if it did not resolve to a local first.
    globals: Vec<String>,
    /// A stack of lexical scopes, innermost on top. One entry for the root
    /// block today; lambda bodies will push their own.
    scope_stack: Vec<Scope>,
    /// Monotonic — slots are never reused, so a single flat frame serves the
    /// whole tree and a nested block cannot alias an enclosing binding.
    next_slot: u32,
    /// Whether any `var[i]` was translated. Really a property of the finished
    /// AST — "does a `DynamicIndex` appear" — but deriving it would want a
    /// traversal helper whose only other caller today is a test.
    contains_dynamic_lookup: bool,
}

/// The bindings introduced by one lexical scope.
///
/// The representation is deliberately behind the struct. A scope holds a lambda
/// parameter and perhaps a couple of `var` bindings, and at that size a linear
/// scan beats hashing and allocates nothing extra — but nothing outside here
/// depends on that, so it can become a map if scopes ever grow.
#[derive(Default)]
struct Scope {
    slot_by_name: Vec<(String, LocalSlot)>,
}

impl TranslationState {
    fn push_scope(&mut self) {
        self.scope_stack.push(Scope::default());
    }

    fn pop_scope(&mut self) {
        self.scope_stack.pop();
    }

    /// Binds `name` in the innermost scope and hands back its slot.
    ///
    /// Re-declaring a name shadows the earlier binding rather than erroring —
    /// well-defined, and the same rule Rust's `let` uses. The JVM
    /// implementation also allowed it, but only because its runtime map
    /// overwrote silently; that is not the reason it is allowed here. Making it
    /// an error is a defensible alternative and would need its own diagnostic.
    fn declare(&mut self, name: &str) -> LocalSlot {
        let slot = LocalSlot::from_index(self.next_slot);
        self.next_slot += 1;
        self.scope_stack
            .last_mut()
            .expect("a scope is pushed before anything is declared")
            .slot_by_name
            .push((name.to_owned(), slot));
        slot
    }

    /// Walks the stack from the top down, and each scope from its newest
    /// binding back. That ordering is the whole of shadowing.
    fn resolve_local(&self, name: &str) -> Option<LocalSlot> {
        self.scope_stack
            .iter()
            .rev()
            .find_map(|scope| {
                scope
                    .slot_by_name
                    .iter()
                    .rev()
                    .find(|(bound, _)| bound == name)
            })
            .map(|(_, slot)| *slot)
    }

    /// Resolves a global, registering it on first reference so ordering is
    /// stable.
    fn get_or_make_id(&mut self, name: &str) -> GlobalId {
        let position = self.globals.iter().position(|known| known == name);
        let position = match position {
            Some(position) => position,
            None => {
                self.globals.push(name.to_owned());
                self.globals.len() - 1
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
        let mut state = TranslationState::default();
        let block = ctx.statement_block().map_err(|_| {
            vec![unsupported(
                "an empty expression",
                self.source,
                Span::new(0, 0),
            )]
        })?;

        // Whether the *root* result is a comparison. The grammar also admits a
        // boolean in a lambda body, where it lowers to arithmetic like any other
        // sub-expression and says nothing about the expression as a whole — the
        // JVM implementation set this flag for a boolean anywhere, and so
        // reported `sum(1, 3, i -> i > 2)` as a boolean expression.
        let is_boolean_expression = block
            .return_statement()
            .is_ok_and(|ret| ret.boolean_expr().is_some());

        let body = self.translate_block(&block, &mut state)?;

        Ok(AbstractSyntaxTree {
            program: Program {
                body,
                frame_size: state.next_slot,
            },
            symbols: state.globals,
            contains_dynamic_lookup: state.contains_dynamic_lookup,
            is_boolean_expression,
        })
    }

    /// Translates `statementBlock`, which is both the root of an expression and
    /// the body of every lambda.
    ///
    /// Owns the scope it introduces: bindings made here are invisible once it
    /// returns.
    fn translate_block(
        &self,
        ctx: &StatementBlockContext<'_>,
        state: &mut TranslationState,
    ) -> Result<Block, Vec<Problem>> {
        state.push_scope();

        // `(statement ';')* returnStatement ';'?`
        let assignments = self.translate_assignments(ctx, state)?;

        let ret = ctx.return_statement().map_err(|_| {
            vec![unsupported(
                "an empty expression",
                self.source,
                Span::new(0, 0),
            )]
        })?;

        let result = if let Some(boolean) = ret.boolean_expr() {
            self.translate_boolean_expr(&boolean, state)?
        } else {
            let scalar = ret.scalar_expr().ok_or_else(|| {
                vec![unsupported(
                    "an empty expression",
                    self.source,
                    Span::new(0, 0),
                )]
            })?;
            self.translate_scalar_expr(&scalar, state)?
        };

        state.pop_scope();

        Ok(Block {
            assignments,
            result,
        })
    }

    /// Translates `(statement ';')*`, binding each name after its own value has
    /// been translated.
    ///
    /// That order is the whole of sequential scoping: in `var x = x`, the
    /// right-hand `x` resolves against the enclosing scope because `x` is not
    /// bound until the assignment completes.
    fn translate_assignments(
        &self,
        block: &StatementBlockContext<'_>,
        state: &mut TranslationState,
    ) -> Result<Vec<Assignment>, Vec<Problem>> {
        let mut assignments = Vec::new();

        for statement in block.statement_children() {
            let span = Span::new(0, 0);
            let assignment = statement
                .assignment()
                .map_err(|_| vec![unsupported("this statement", self.source, span)])?;

            let name = assignment
                .name()
                .map_err(|_| vec![unsupported("this assignment", self.source, span)])?
                .variable_token()
                .map_or(String::new(), |token| {
                    token.symbol().text_or_empty().to_owned()
                });

            let value_ctx = assignment
                .scalar_expr()
                .map_err(|_| vec![unsupported("this assignment", self.source, span)])?;
            let value = self.translate_scalar_expr(&value_ctx, state)?;

            assignments.push(Assignment {
                slot: state.declare(&name),
                value,
                span,
            });
        }

        Ok(assignments)
    }

    // These survive into the AST as [`Kind::Compare`] and [`Kind::NearEq`] and
    // are eliminated by [`crate::rewrite::rewrite_booleans`]; the evaluator
    // never sees them.
    fn translate_boolean_expr(
        &self,
        ctx: &BooleanExprContext<'_>,
        state: &mut TranslationState,
    ) -> Result<Expr, Vec<Problem>> {
        // TODO(spans): `span_of` is still stubbed; see lower_scalar_expr.
        let span = Span::new(0, 0);

        // `'(' booleanExpr ')'` — grouping adds no node.
        if let Some(inner) = ctx.boolean_expr() {
            return self.translate_boolean_expr(&inner, state);
        }

        let children: Vec<_> = ctx.scalar_expr_children().collect();
        let lhs = self.operand(&children, 0, span, state)?;
        let rhs = self.operand(&children, 1, span, state)?;

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
        state: &mut TranslationState,
    ) -> Result<Box<Expr>, Vec<Problem>> {
        let child = children
            .get(index)
            .ok_or_else(|| vec![unsupported("this expression", self.source, span)])?;
        Ok(Box::new(self.translate_scalar_expr(child, state)?))
    }

    fn translate_scalar_expr(
        &self,
        ctx: &ScalarExprContext<'_>,
        state: &mut TranslationState,
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
            let kind = state
                .resolve_local(&name)
                .map_or_else(|| Kind::Global(state.get_or_make_id(&name)), Kind::Local);
            return Ok(Expr::new(kind, span));
        }

        // `var '[' scalarExpr ']'` — a one-based index into the whole row, in
        // schema declaration order.
        //
        // No parent check is needed. The JVM implementation's `exitVar` had a
        // `when (ctx.parent)` to tell this from the `var x = …` of an
        // assignment, because a listener sees every `VarContext`; here
        // `assignment` is its own rule and owns its own `var` child.
        if ctx.var().is_some() {
            let subscript = self.operand(&children, 0, span, state)?;
            state.contains_dynamic_lookup = true;
            return Ok(Expr::new(Kind::DynamicIndex(subscript), span));
        }
        // `(sum | prod) '(' scalarExpr ',' scalarExpr ',' lambdaExpr ')'`
        if let Some(kind) = aggregate_kind(ctx) {
            let lambda = ctx
                .lambda_expr()
                .ok_or_else(|| vec![unsupported("this aggregate", self.source, span)])?;

            // Bounds translate in the enclosing scope: they cannot see the
            // parameter, which is not bound until the body.
            let lower = self.operand(&children, 0, span, state)?;
            let upper = self.operand(&children, 1, span, state)?;

            let name = lambda
                .name()
                .map_err(|_| vec![unsupported("this lambda", self.source, span)])?
                .variable_token()
                .map_or(String::new(), |token| {
                    token.symbol().text_or_empty().to_owned()
                });
            let body_ctx = lambda
                .statement_block()
                .map_err(|_| vec![unsupported("this lambda", self.source, span)])?;

            // One scope for the parameter, and `translate_block` pushes another
            // for the body — so a `var i = …` inside shadows the parameter
            // rather than colliding with it.
            state.push_scope();
            let param = state.declare(&name);
            let body = self.translate_block(&body_ctx, state);
            state.pop_scope();

            return Ok(Expr::new(
                Kind::Aggregate {
                    kind,
                    lower,
                    upper,
                    param,
                    body: Box::new(body?),
                },
                span,
            ));
        }

        // A bare lambda outside an aggregate is unreachable from the grammar.
        if ctx.lambda_expr().is_some() {
            return Err(vec![unsupported("lambdas", self.source, span)]);
        }

        if let Some(function) = ctx.binary_function() {
            let op = BinaryOp::from_function_keyword(function.text().as_ref())
                .ok_or_else(|| vec![unsupported("this binary function", self.source, span)])?;
            let lhs = self.operand(&children, 0, span, state)?;
            let rhs = self.operand(&children, 1, span, state)?;
            return Ok(Expr::new(Kind::Binary { op, lhs, rhs }, span));
        }

        if let Some(function) = ctx.unary_function() {
            let op = UnaryOp::from_keyword(function.text().as_ref())
                .ok_or_else(|| vec![unsupported("this unary function", self.source, span)])?;
            let arg = self.operand(&children, 0, span, state)?;
            return Ok(Expr::new(Kind::Unary { op, arg }, span));
        }

        // `negate : '-'` and `minus : '-'` are distinct rules, so unary minus
        // and binary subtraction never collide.
        if ctx.negate().is_some() {
            let arg = self.operand(&children, 0, span, state)?;
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
            let lhs = self.operand(&children, 0, span, state)?;
            let rhs = self.operand(&children, 1, span, state)?;
            return Ok(Expr::new(Kind::Binary { op, lhs, rhs }, span));
        }

        // `'(' scalarExpr ')'` — grouping adds no node, it just returns the
        // child. Precedence is already encoded in the tree's shape.
        if children.len() == 1 {
            return self.translate_scalar_expr(&children[0], state);
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

#[cfg(test)]
mod tests {
    /// A name bound by an assignment must not be reported as a global. The
    /// corpus covers this only indirectly, through `statically_referenced_symbols`.
    #[test]
    fn a_local_is_not_a_global() {
        let expression = crate::compile(
            "var x = x1;
 x + x1",
        )
        .expect("should compile");
        let globals: Vec<&str> = expression
            .statically_referenced_symbols()
            .into_iter()
            .collect();
        assert_eq!(globals, vec!["x1"], "`x` is local and must not be a global");
    }

    /// Innermost-outward resolution: where a local and a global share a name,
    /// the local wins. Nothing in the corpus exercises this.
    #[test]
    fn a_local_shadows_a_global_of_the_same_name() {
        let expression = crate::compile(
            "var x1 = 7;
 x1",
        )
        .expect("should compile");
        assert!(
            expression.statically_referenced_symbols().is_empty(),
            "the trailing x1 resolves to the local, so nothing is referenced globally"
        );
        assert_eq!(expression.evaluate(&[]).expect("should evaluate"), 7.0);
    }

    /// Sequential scoping: a name is bound only *after* its own value is
    /// translated, so the right-hand side sees the enclosing scope.
    #[test]
    fn an_assignment_does_not_bind_its_own_right_hand_side() {
        let expression = crate::compile(
            "var x1 = x1 + 1;
 x1",
        )
        .expect("should compile");
        let globals: Vec<&str> = expression
            .statically_referenced_symbols()
            .into_iter()
            .collect();
        assert_eq!(globals, vec!["x1"], "the right-hand x1 is the global");
        assert_eq!(
            expression
                .evaluate(&[("x1", 10.0)])
                .expect("should evaluate"),
            11.0
        );
    }
}
