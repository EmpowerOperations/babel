package com.empowerops.babel

import com.empowerops.babel.BabelParser.*
import com.empowerops.babel.StaticEvaluatorRewritingWalker.Availability.Runtime
import com.empowerops.babel.StaticEvaluatorRewritingWalker.Availability.Static
import org.antlr.v4.runtime.ParserRuleContext
import org.antlr.v4.runtime.RuleContext
import org.antlr.v4.runtime.tree.TerminalNode
import org.antlr.v4.runtime.tree.TerminalNodeImpl
import java.util.*

//TBD: terrible name. what does LLVM/Clang calls these? Optimizers? what about kotlinc?
class StaticEvaluatorRewritingWalker() : BabelParserBaseListener() {
    
    enum class Availability { Static, Runtime }

    private val availability: Deque<Availability> = LinkedList()

    override fun exitScalarExpr(ctx: ScalarExprContext){
        val availabilityByExpr = buildAndUpdateAvailabilityIndex(ctx)

        when {

            ctx.sum() != null || ctx.prod() != null -> {
                tryRewriteChildren(ctx, availabilityByExpr)
                tryStaticallyUnrolling(ctx, availabilityByExpr)
            }

            else -> tryRewriteChildren(ctx, availabilityByExpr)
        }

        val newAvailability = when {
            availabilityByExpr.isEmpty() -> null
            ctx.`var`() != null -> Runtime
            availabilityByExpr.values.all { it == Static } -> Static
            availabilityByExpr.values.any { it == Runtime } -> Runtime
            else -> TODO()
        }
        if(newAvailability != null) { availability.push(newAvailability) }
    }

    override fun exitExpression(ctx: ExpressionContext) {
        tryRewriteChildren(ctx, buildAndUpdateAvailabilityIndex(ctx))
    }

    private fun tryStaticallyUnrolling(ctx: ScalarExprContext, availabilityByExpr: Map<ScalarExprContext, Availability>) {

        val lowerBoundExpr = ctx.scalarExpr(0)
        val upperBoundExpr = ctx.scalarExpr(1)
        val lambdaExpr = ctx.lambdaExpr()

        if(availabilityByExpr[lowerBoundExpr] == Static
                && availabilityByExpr[upperBoundExpr] == Static
                && availabilityByExpr[lambdaExpr.scalarExpr()] == Runtime){

            //assumes that the other re-write has already happened...
            // really not liking the mutability here.
            val (lower, upper) = listOf(lowerBoundExpr, upperBoundExpr).map { it.asStaticValue().roundToIndex()!! }
            val plusOrTimes: Rewriter.() -> Unit = when {
                ctx.sum() != null -> {{ plus() }}
                ctx.prod() != null -> {{ times() }}
                else -> TODO()
            }

            ctx.children.clear()

            (upper downTo lower).fold(ctx){ parent, currentIndex ->

                val thisLevelLHS = ScalarExprContext(parent, -1).apply { children = mutableListOf() }
                val thisLevelRHS = lambdaExpr.clone().apply {
                    value = currentIndex.toDouble()
//                    text = "((${name()} = $value) -> ${scalarExpr().text})"
                }

                configure(parent){
                    when(currentIndex - lower){
                        0 -> append(thisLevelRHS)
                        in 1 .. Int.MAX_VALUE -> {
                            append(thisLevelLHS)
                            plusOrTimes()
                            scalar {
                                append(thisLevelRHS)
                            }
                        }
                        else -> TODO()
                    }
                }

                thisLevelLHS
            }
        }
    }

    private fun tryRewriteChildren(ctx: ParserRuleContext, exprsByAvailability: Map<ScalarExprContext, Availability>) {

        when {
            Runtime in exprsByAvailability.values -> {
                for (evaluable in exprsByAvailability.filterValues { it == Static }.keys) {

                    val result = evaluate(evaluable)

                    evaluable.children = result.children
                }
            }
        }
    }

    private fun buildAndUpdateAvailabilityIndex(ctx: ParserRuleContext): Map<ScalarExprContext, Availability> {
        val childExprs = ctx.children
                .map { it as? ScalarExprContext ?: (it as? LambdaExprContext)?.scalarExpr() }
                .filterIsInstance<ScalarExprContext>()

        val exprsByAvailability = childExprs.asReversed().associate { it to availability.pop() }
        return exprsByAvailability
    }

    private fun evaluate(node: ScalarExprContext): ScalarExprContext {

        val compiler = CodeGeneratingWalker(node.text).apply { walk(node) }

        val expr = BabelExpression(
                node.text,
                containsDynamicLookup = true,
                isBooleanExpression = false,
                staticallyReferencedSymbols = emptySet(),
                runtime = compiler.instructions.configuration
        )

        val result = expr.evaluate(emptyMap())

        val terminal = TerminalNodeImpl(ValueToken(result, node.text))
        val literal = LiteralContext(terminal)
        return ExprContext(literal)
    }

    override fun exitVariable(ctx: VariableContext) { availability.push(Runtime) }
    override fun exitLiteral(ctx: LiteralContext) { availability.push(Static) }
}


private fun ExprContext(parent: ParserRuleContext? = null, children: List<RuleContext> = emptyList())
        = ScalarExprContext(parent, -1).apply { children.forEach(this::adopt) }

private fun ExprContext(literalContext: LiteralContext) = ExprContext().apply { adopt(literalContext) }
private fun LiteralContext(node: TerminalNode) = LiteralContext(null, -1).apply { adopt(node) }

private fun LambdaExprContext.clone() = LambdaExprContext(parent as ParserRuleContext, invokingState).apply {
    children = this@clone.children
}

private fun ScalarExprContext.asStaticValue(): Double
        = ((children.single() as LiteralContext).FLOAT().symbol as ValueToken).value

