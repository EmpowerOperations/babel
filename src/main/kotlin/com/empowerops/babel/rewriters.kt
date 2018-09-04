package com.empowerops.babel

import com.empowerops.babel.StaticEvaluatorRewritingWalker.Availability.Runtime
import com.empowerops.babel.StaticEvaluatorRewritingWalker.Availability.Static
import org.antlr.v4.runtime.CommonToken
import org.antlr.v4.runtime.ParserRuleContext
import org.antlr.v4.runtime.RuleContext
import org.antlr.v4.runtime.Token
import org.antlr.v4.runtime.tree.ParseTree
import org.antlr.v4.runtime.tree.TerminalNode
import org.antlr.v4.runtime.tree.TerminalNodeImpl
import org.intellij.lang.annotations.MagicConstant
import java.io.Closeable
import java.util.*

/**
 * Rewrites boolean expressions into scalar expressions as per constraint formulation.
 *
 * This class changes a boolean expression into an expression that returns a scalar value
 * according to the simple scheme:
 * ```
 * negative value -> true
 * zero           -> true
 * positive value -> false
 * ```
 *
 * This is how many seminal works on optimization traditionally treat constraints, and it has
 * the added advantage of keeping meta-data around about failed constraints, such that we
 * can determine which values fail constraints worse than others.
 * ```
 * right > left
 * ```
 * will be written as
 * ```
 * left - right < 0
 * ```
 * The same apply to
 * ```
 * left < right -> left - right < 0
 * ```
 * ```
 * left <= right -> left - right <= 0
 * ```
 * ```
 * left >= right -> right - left <= 0
 * ```
 *
 * Truth table:
 * As described above, to fit our true false scheme into the truth table we want we will need:
 *
 * ```
 * left     right    left <= right    left - right      with our scheme
 * 1        0           false           1            (positive) -> false
 * 1        1           true           0             (zero)     -> true
 * 0        1           true            -1           (negative) -> true
 * ```
 * and
 * ```
 * left     right    left < right    left - right + Epsilon         with our scheme
 * 1        0           false           1 + Epsilon == 1        (positive) -> false
 * 1        1           false           0 + Epsilon == Epsilon  (positive) -> false
 * 0        1           true            -1 + Epsilon== -1       (negative) -> true
 * ```
 *
 * Here are some note about why we should add an epsilon when do inequality comparison:
 * TLDR;
 *  rewrite as left - right < 0 truth table will be ok for `left - right <= 0` as seem form the table
 *  however it is not fit the `left - right < 0` truth table since if left - right will be equals to 0 and
 *  we considering 0 and positive of remain is true, so we add an epsilon because:
 *    1, with epsilon added we are effective comparing the same thing
 *    2, epsilon will be greater than zero so we can get the correct truth table
 *
 * As seem form the first table, our <= case fit perfectly into the table
 * However, when we looking at the non equality comparison, we run into a bit of problem and we need
 * ```
 * left     right    left < right    left - right      with our scheme
 * 1        0           false           1            (positive) -> false
 * 1        1           false           0            (zero)     -> true
 * 0        1           true            -1           (negative) -> true
 * ```
 * To solve this problem we add an Epsilon to off set the zero
 * This is true when we considering equality in computer science:
 * ```
 * A == B -> Math.Abs(A - B) <= Epsilon
 * ```
 * in such case A + Epsilon are essentially equals to A because
 * ```
 * A + Epsilon == A
 *      -> Math.Abs(A + Epsilon - A) <= Esilon
 *      -> Math.Abs(Epsilon) <= Esilon
 *      -> true
 * ```
 * This will stay true for case
 * ```
 * left + Epslion == left && left < right
 *    -> left + Epsilon < right
 *    -> left - right + Epsilon < 0
 * ```
 * Another thing we need to know Epsilon is the min positive number so Epsilon > 0
 *
 * with Epsilon added, we can get the correct table as shown here:

 * and we use
 * ```
 * left - right + Epsilon < 0
 * ```
 * to representing
 * ```
 * left < right
 * ```
 */
internal class BooleanRewritingWalker : BabelParserBaseListener() {

    var isBooleanExpression = false
        private set

    override fun exitBooleanExpr(ctx: BabelParser.BooleanExprContext) {

        isBooleanExpression = true

        val operation = ctx.children?.firstOrNull { it.isOperation }
        when(operation){
            is BabelParser.LteqContext -> {
                val childScalar = insertScalar(ctx)
                rewriteLessEqual(childScalar)
            }
            is BabelParser.GteqContext -> {
                val childScalar = insertScalar(ctx)
                rewriteGreaterEqual(childScalar)
            }
            is BabelParser.LtContext -> {
                val childScalar = insertScalar(ctx)
                swapInequalityWithSubtraction(childScalar , "~<")
                addEpsilon(childScalar)
            }
            is BabelParser.GtContext -> {
                val childScalar = insertScalar(ctx)
                swapLiteralChildren(childScalar)
                swapInequalityWithSubtraction(childScalar, "~>")
                addEpsilon(childScalar)
            }
            is BabelParser.EqContext -> {
                val childScalar = insertScalar(ctx)
                val (left, right) = childScalar.scalarExpr()
                val offset = childScalar.literal()

                childScalar.children.clear()

                configure(childScalar) {
                    binaryFunction {
                        // given our interpretation of (-inf to 0] => true, (0 to +inf) => false
                        // "max" supplies us with a logical "and"
                        terminal(BabelLexer.MAX, "~==")
                    }
                    terminal(BabelLexer.OPEN_PAREN)
                    scalar {
                        append(left)
                        greaterEqual()
                        scalar {
                            append(right)
                            minus()
                            scalar {
                                append(offset)
                            }
                        }
                    }.let {
                        rewriteGreaterEqual(it)
                    }
                    terminal(BabelLexer.COMMA)
                    scalar {
                        append(left)
                        lessEqual()
                        scalar {
                            append(right)
                            plus()
                            scalar {
                                append(offset)
                            }
                        }
                    }.let {
                        rewriteLessEqual(it)
                    }
                    terminal(BabelLexer.CLOSE_PAREN)
                }
            }
            else -> {
                //no-op for error nodes or re-written trees.
            }
        }
    }

    private fun rewriteGreaterEqual(childScalar: BabelParser.ScalarExprContext) {
        swapLiteralChildren(childScalar)
        swapInequalityWithSubtraction(childScalar, "~>=")
    }

    private fun rewriteLessEqual(childScalar: BabelParser.ScalarExprContext) {
        swapInequalityWithSubtraction(childScalar, "~<=")
    }

    //signature implies purity, _but it absolutely is not pure_
    private fun insertScalar(ctx: BabelParser.BooleanExprContext): BabelParser.ScalarExprContext {

        val originalChildren = ArrayList(ctx.children)
        ctx.children.clear()

        configure(ctx) {
            scalar(originalChildren)
        }

        return ctx.children.single() as BabelParser.ScalarExprContext
    }

    private fun addEpsilon(ctx: BabelParser.ScalarExprContext) {

        val originalChildren = ArrayList(ctx.children)
        ctx.children.clear()

        configure(ctx) {
            scalar(originalChildren)
            plus()
            scalar {
                literal(value = Epsilon)
            }
        }
    }

    private fun swapInequalityWithSubtraction(ctx: BabelParser.ScalarExprContext, replacedElement: String) {

        val originalChildren = ArrayList(ctx.children)
        require(originalChildren.size == 3) //TODO: include an assertion that the middle child is an operation.
        // Requires a bit of polymorphism I wont have until i remove the `plus` `gteq`, etc productions.

        ctx.children.clear()

        configure(ctx) {
            append(originalChildren.first())
            minus(text = replacedElement)
            append(originalChildren.last())
        }
    }

    private fun swapLiteralChildren(ctx: BabelParser.ScalarExprContext) {
        val firstChild = ctx.children.first()

        ctx.apply {
            children[0] = children[2]
            children[2] = firstChild
        }
    }

    private val ParseTree.isOperation get() = this.childCount == 1
            && this.getChild(0) is TerminalNode

    companion object {
        @JvmField val Epsilon: Double = java.lang.Double.MIN_NORMAL
    }
}

//TBD: terrible name. what does LLVM/Clang calls these? Optimizers? what about kotlinc?
class StaticEvaluatorRewritingWalker(val sourceText: String) : BabelParserBaseListener() {

    enum class Availability { Static, Runtime }

    private val availability: Deque<Availability> = LinkedList()

    var problems: Set<ExpressionProblem> = emptySet()
        private set

    override fun exitScalarExpr(ctx: BabelParser.ScalarExprContext){
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
            availabilityByExpr.values.any { it == Runtime } -> Availability.Runtime
            else -> TODO()
        }
        if(newAvailability != null) { availability.push(newAvailability) }
    }

    override fun exitExpression(ctx: BabelParser.ExpressionContext) {
        tryRewriteChildren(ctx, buildAndUpdateAvailabilityIndex(ctx))
    }

    private fun tryStaticallyUnrolling(ctx: BabelParser.ScalarExprContext, availabilityByExpr: Map<BabelParser.ScalarExprContext, Availability>) {

        val lowerBoundExpr = ctx.scalarExpr(0)
        val upperBoundExpr = ctx.scalarExpr(1)
        val lambdaExpr = ctx.lambdaExpr()

        if(availabilityByExpr[lowerBoundExpr] == Static
                && availabilityByExpr[upperBoundExpr] == Static
                && availabilityByExpr[lambdaExpr.scalarExpr()] == Runtime){

            //assumes that the other re-write has already happened...
            // really not liking the mutability here.
            val (lower, upper) = mapOf("lower" to lowerBoundExpr, "upper" to upperBoundExpr).mapValues { (boundType, boundExpr) ->
                
                val staticValue = boundExpr.asStaticValue()!!
                val result = staticValue.roundToIndex()

                if(result == null){
                    problems += ExpressionProblem(
                            sourceText,
                            ctx.makeAbbreviatedProblemText(),
                            boundExpr.textLocation,
                            boundExpr.start.line,
                            boundExpr.start.charPositionInLine,
                            "Illegal $boundType bound value",
                            "evaluates to $staticValue"
                    )
                }
                
                result
            }.values.toList()

            if(lower == null || upper == null) return

            val plusOrTimes: Rewriter.() -> Unit = when {
                ctx.sum() != null -> {{ plus() }}
                ctx.prod() != null -> {{ times() }}
                else -> TODO()
            }

            ctx.children.clear()

            (upper downTo lower).fold(ctx){ parent, currentIndex ->

                val thisLevelLHS = BabelParser.ScalarExprContext(parent, -1).apply { children = mutableListOf() }
                val thisLevelRHS = lambdaExpr.clone().apply {
                    value = currentIndex.toDouble()
//                    text = "((${name()} = $value) -> ${scalarExpr().text})"
                }

                configure(parent) {
                    when (currentIndex - lower) {
                        0 -> append(thisLevelRHS)
                        in 1..Int.MAX_VALUE -> {
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

    private fun tryRewriteChildren(ctx: ParserRuleContext, exprsByAvailability: Map<BabelParser.ScalarExprContext, Availability>) {

        when {
            Runtime in exprsByAvailability.values -> {
                for (evaluable in exprsByAvailability.filterValues { it == Static }.keys) {

                    if(evaluable.children.singleOrNull() is BabelParser.LiteralContext) {
                        //already as efficient as it can be!
                        continue
                    }

                    evaluateAndRewriteForLiteral(evaluable)
                }
            }
        }
    }

    private fun buildAndUpdateAvailabilityIndex(ctx: ParserRuleContext): Map<BabelParser.ScalarExprContext, Availability> {
        val childExprs = ctx.children
                .map { it as? BabelParser.ScalarExprContext ?: (it as? BabelParser.LambdaExprContext)?.scalarExpr() }
                .filterIsInstance<BabelParser.ScalarExprContext>()

        val exprsByAvailability = childExprs.asReversed().associate { it to availability.pop() }
        return exprsByAvailability
    }

    private fun evaluateAndRewriteForLiteral(ctx: BabelParser.ScalarExprContext) {

        val compiler = CodeGeneratingWalker(ctx.text).apply { walk(ctx) }

        val expr = BabelExpression(
                ctx.text,
                containsDynamicLookup = true,
                isBooleanExpression = false,
                staticallyReferencedSymbols = emptySet(),
                runtime = compiler.instructions.configuration
        )

        val result = expr.evaluate(emptyMap(), emptyList())

        val originalText = ctx.text
        val (startToken, stopToken) = ctx.start to ctx.stop
        ctx.children.clear()

        configure(ctx){
            literal(result, startToken, stopToken, originalText)
        }
    }

    override fun exitVariable(ctx: BabelParser.VariableContext) { availability.push(Runtime) }
    override fun exitLiteral(ctx: BabelParser.LiteralContext) { availability.push(Static) }
}

internal fun <T> configure(target: T, block: Rewriter.() -> Unit): T where T: ParserRuleContext {
    Rewriter(target).use { it.block() }
    return target
}


internal class Rewriter(val target: ParserRuleContext): Closeable {

    var start: Token? = null
    var stop: Token? = null

    init {
        if(target.children == null){ target.children = mutableListOf() }
    }

    override fun close() {
        target.start = start ?: target.children.firstOrNull()?.let { when(it) {
            is TerminalNode -> it.symbol
            is ParserRuleContext -> it.start
            else -> null
        }}

        target.stop = stop ?: target.children.lastOrNull()?.let { when(it) {
            is TerminalNode -> it.symbol
            is ParserRuleContext -> it.start
            else -> null
        }}
    }

    fun append(node: ParseTree){
        target.children.add(node)
        if(node is RuleContext) node.parent = target
    }

    fun prepend(node: ParseTree) {
        target.children.add(0, node)
        if(node is RuleContext) node.parent = target
    }

    fun times(text: String = "*") = append(BabelParser.MultContext(target, -1).apply {
        terminal = CommonToken(BabelLexer.MULT, text)
    })
    fun plus(text: String = "+") = append(BabelParser.PlusContext(target, -1).apply {
        terminal = CommonToken(BabelLexer.PLUS, text)
    })
    fun minus(text: String = "-") = append(BabelParser.MinusContext(target, -1).apply {
        terminal = CommonToken(BabelLexer.MINUS, text)
    })
    fun literal(value: Double, startToken: Token? = null, stopToken: Token? = null, text: String = value.toString()) {
        val node = BabelParser.LiteralContext(target, -1).apply {
            terminal = ValueToken(value, text).apply {
                line = startToken?.line ?: line
                charPositionInLine = startToken?.charPositionInLine ?: charPositionInLine
                channel = startToken?.channel ?: channel
                startIndex = startToken?.startIndex ?: -1
                stopIndex = stopToken?.stopIndex ?: startToken?.startIndex ?: -1
            }
        }
        append(node)
    }
    fun greaterEqual(text: String = ">=") = append(BabelParser.GteqContext(target, -1).apply {
        terminal = CommonToken(BabelLexer.GTEQ, text)
    })
    fun lessEqual(text: String = "<=") = append(BabelParser.LteqContext(target, -1).apply {
        terminal = CommonToken(BabelLexer.LTEQ, text)
    })

    fun scalar(initialChildren: List<ParseTree> = mutableListOf(), block: Rewriter.() -> Unit = {}): BabelParser.ScalarExprContext {
        val scalarCtx = BabelParser.ScalarExprContext(target, -1).apply { children = initialChildren }
        Rewriter(scalarCtx).use { it.block() }
        append(scalarCtx)
        return scalarCtx
    }

    fun binaryFunction(block: Rewriter.() -> Unit){
        val result = BabelParser.BinaryFunctionContext(target, -1)
        Rewriter(result).use { it.block() }
        append(result)
    }

    fun terminal(
            @MagicConstant(valuesFromClass = BabelLexer::class) tokenType: Int,
            text: String = BabelLexer.VOCABULARY.getLiteralName(tokenType).removePrefix("'").removeSuffix("'")
    ): Unit {
        val result = TerminalNodeImpl(CommonToken(tokenType, text))
        target.children.add(result)
    }
}


private fun BabelParser.LambdaExprContext.clone() = BabelParser.LambdaExprContext(parent as ParserRuleContext, invokingState).apply {
    children = this@clone.children
}

private fun BabelParser.ScalarExprContext.asStaticValue(): Double? {
    return (children.singleOrNull() as? BabelParser.LiteralContext)?.value
}


