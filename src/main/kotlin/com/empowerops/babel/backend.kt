package com.empowerops.babel

import com.empowerops.babel.BabelLexer.*
import com.empowerops.babel.BabelParser.BooleanExprContext
import com.empowerops.babel.BabelParser.ScalarExprContext
import com.empowerops.babel.RuntimeBabelBuilder.RuntimeConfiguration
import kotlinx.collections.immutable.ImmutableMap
import kotlinx.collections.immutable.immutableMapOf
import kotlinx.collections.immutable.plus
import org.antlr.v4.runtime.*
import org.antlr.v4.runtime.tree.*
import org.intellij.lang.annotations.MagicConstant
import java.io.Closeable
import java.util.*
import java.util.logging.Logger
import kotlin.collections.ArrayList

typealias UnaryOp = (Double) -> Double
typealias BinaryOp = (Double, Double) -> Double

internal class SyntaxErrorReportingWalker : BabelParserBaseListener() {

    var problems: Set<BabelExpressionProblem> = emptySet()
        private set

    override fun visitErrorNode(node: ErrorNode) {
        problems += BabelExpressionProblem("syntax error", node.symbol.line, node.symbol.charPositionInLine)
    }
}

/**
 * Created by Justin Casol on 2/2/2015.
 */
internal class SymbolTableBuildingWalker : BabelParserBaseListener() {

    var containsDynamicVarLookup: Boolean = false
        private set

    var staticallyReferencedVariables: Set<String> = emptySet()
        private set

    var localVars: Set<String> = emptySet()
        private set

    override fun exitAssignment(ctx: BabelParser.AssignmentContext) {
        localVars += ctx.name().text
    }

    override fun enterLambdaExpr(ctx: BabelParser.LambdaExprContext) {
        localVars += ctx.name().text
    }

    override fun exitLambdaExpr(ctx: BabelParser.LambdaExprContext) {
        localVars -= ctx.name().text
    }

    override fun exitVariable(ctx: BabelParser.VariableContext) {
        if(ctx.text !in localVars){
            staticallyReferencedVariables += ctx.text
        }
    }

    override fun exitVar(ctx: BabelParser.VarContext) {
        containsDynamicVarLookup = containsDynamicVarLookup || (ctx.parent is BabelParser.ScalarExprContext)
    }
}

internal class RuntimeBabelBuilder {

    val configuration = RuntimeConfiguration()

    class RuntimeConfiguration {

        var jobs: Deque<RuntimeMemory.() -> Unit> = LinkedList()

        fun append(runtimePart: (@CompileTimeFence RuntimeMemory).() -> Unit) {
            jobs.push(runtimePart)
        }

        inline fun appendBinaryInstruction(crossinline operation: BinaryOp){
            append {
                val right = stack.pop()
                val left = stack.pop()
                val result = operation.invoke(left, right)
                stack.push(result)
            }
        }
        inline fun appendUnaryInstruction(crossinline operation: UnaryOp){
            append {
                val arg = stack.pop()
                val result = operation.invoke(arg)
                stack.push(result)
            }
        }

        fun popOperation(): RuntimeMemory.() -> Unit = jobs.pop()

        operator fun invoke(globalVars: @Ordered Map<String, Double>): Double {
            require(jobs.count() == 1) { "stack-machine code generation failure" }

            val scope = RuntimeMemory(globalVars)
            jobs.peek().invoke(scope)

            val result = scope.stack.pop().toDouble()

            require(scope.stack.isEmpty()) { "execution incomplete, stack: ${scope.stack}" }

            return result
        }
    }

    fun build(additionalConfiguration: (@CompileTimeFence RuntimeConfiguration).() -> Unit): Unit {
        additionalConfiguration(configuration)
    }
}

@DslMarker
@Target(AnnotationTarget.TYPE)
private annotation class CompileTimeFence

//could make this entirely immutable by
internal data class RuntimeMemory(
        val globals: @Ordered Map<String, Double>,
        var heap: ImmutableMap<String, Double> = immutableMapOf(),
        val stack: Deque<Double> = LinkedList()
){
    val parentScopes: Deque<ImmutableMap<String, Double>> = LinkedList()

    fun usingNestedScope(action: () -> Unit){
        parentScopes.push(heap)
        try {
            action()
        }
        finally {
            heap = parentScopes.pop()
        }
    }
}

internal class CodeGeneratingWalker(val sourceText: String) : BabelParserBaseListener() {

    val instructions = RuntimeBabelBuilder()
    private val operations = RuntimeNumerics()

    override fun exitExpression(ctx: BabelParser.ExpressionContext) = instructions.build {
        val ops = (ctx.statement() + ctx.returnStatement()).map { popOperation() }.asReversed()

        append {
            for(op in ops){
                op()
            }
        }
    }

    override fun exitAssignment(ctx: BabelParser.AssignmentContext) = instructions.build {
        val varName = ctx.name().text
        val valueGenerator = popOperation()

        append {
            valueGenerator()
            val value = stack.pop()
            heap += varName to value
        }
    }

    override fun exitBooleanExpr(ctx: BooleanExprContext) = instructions.build {
        when {
            ctx.childCount == 1 && ctx.scalarExpr() != null -> {}
            ctx.callsBinaryOp() -> handleBinaryOp()
            else -> TODO("unknown scalarExpr ${ctx.text}")
        }
    }

    override fun exitScalarExpr(ctx: ScalarExprContext) = instructions.build {

        when {
            ctx.childCount == 0 -> {
                //typically when an error occurs in parse tree generation -> noop
                Log.fine { "skipped code gen for $ctx as it erroneously has no children" }
            }
            ctx.callsLiteralOrVariable() -> {
                //noop -- handled by child directly
            }
            ctx.callsDynamicVariableAccess() -> {
                val indexingExpr = popOperation()
                val indexToValueConverter = popOperation()
                val rangeInText = ctx.scalarExpr(0).textLocation
                val problemText = ctx.text

                append {
                    indexingExpr()
                    val indexValue = stack.peek()
                    try {
                        indexToValueConverter()
                    }
                    catch(ex: IndexOutOfBoundsException){
                        throw makeRuntimeException(problemText, rangeInText, ex.message ?: "Invalid index", indexValue)
                    }
                }
            }
            ctx.callsInlineExpression() -> {
                //noop -- brackets are purely a directive for order-of-ops, no code changes needed.
            }
            ctx.callsAggregation() -> {

                val lowerBoundRangeInText = ctx.scalarExpr(0).textLocation
                val upperBoundRangeInText = ctx.scalarExpr(1).textLocation
                val problemText = ctx.children.joinToString("") { when(it){
                    is BabelParser.LambdaExprContext -> "${it.text.substringBefore("->")}->..."
                    else -> it.text
                }}

                val lambda = popOperation()
                val upperBoundExpr = popOperation()
                val lowerBoundExpr = popOperation()
                val aggregator = popOperation()
                val seedProvider = popOperation()

                append {

                    fun runInExceptionHandler(textLocation: IntRange, expr: RuntimeMemory.() -> Unit): Int {
                        expr()
                        val indexCandidate = stack.pop()
                        return indexCandidate.roundToIndex()
                                ?: throw makeRuntimeException(problemText, textLocation, "NaN bound value", indexCandidate)
                    }

                    val upperBound = runInExceptionHandler(upperBoundRangeInText, upperBoundExpr)
                    val lowerBound = runInExceptionHandler(lowerBoundRangeInText, lowerBoundExpr)

                    seedProvider()

                    for (index in lowerBound .. upperBound) {
                        stack.push(index.toDouble())
                        lambda()
                        aggregator()
                    }
                }
            }
            ctx.lambdaExpr()?.value != null -> {
                //closed lambda expression, added by a rewriter

                //noop; everything was handled by enter/exit lambda
            }
            ctx.callsBinaryFunction() -> {
                val right = popOperation()
                val left = popOperation()
                val function = popOperation()

                append {
                    left()
                    right()
                    function()
                }
            }
            ctx.callsUnaryOpOrFunction() -> {
                val arg = popOperation()
                val function = popOperation()

                append {
                    arg()
                    function()
                }
            }
            ctx.callsBinaryOp() -> handleBinaryOp()
            
            else -> TODO("unknown scalarExpr ${ctx.text}")
        }

    }

    private fun RuntimeConfiguration.handleBinaryOp() {
        val right = popOperation()
        val op = popOperation()
        val left = popOperation()

        append {
            left()
            right()
            op()
        }
    }

    private fun RuntimeMemory.makeRuntimeException(
            problemText: String,
            rangeInText: IntRange,
            cause: String,
            problemValue: Double
    ) = RuntimeBabelException(
            """Error in '$problemText': $cause.
              |$sourceText
              |${" ".repeat(rangeInText.first)}${"~".repeat(rangeInText.span)} evaluates to $problemValue
              |local-variables$heap
              |parameters$globals
              """.trimMargin()
    )

    override fun exitLambdaExpr(ctx: BabelParser.LambdaExprContext) = instructions.build {
        val lambdaParamName = ctx.name().text
        val childExpression = popOperation()
        append {
            usingNestedScope {
                heap += lambdaParamName to (ctx.value ?: stack.pop())
                childExpression()
            }
        }
    }

    override fun exitMod(ctx: BabelParser.ModContext) = instructions.build { appendBinaryInstruction(BinaryOps.Modulo) }
    override fun exitPlus(ctx: BabelParser.PlusContext) = instructions.build { appendBinaryInstruction(BinaryOps.Sum) }
    override fun exitMinus(ctx: BabelParser.MinusContext) = instructions.build { appendBinaryInstruction(BinaryOps.Subtract) }
    override fun exitMult(ctx: BabelParser.MultContext) = instructions.build { appendBinaryInstruction(BinaryOps.Multiply) }
    override fun exitDiv(ctx: BabelParser.DivContext) = instructions.build { appendBinaryInstruction(BinaryOps.Divide) }
    override fun exitRaise(ctx: BabelParser.RaiseContext) = instructions.build { appendBinaryInstruction(BinaryOps.Exponentiation) }

    override fun exitBinaryFunction(ctx: BabelParser.BinaryFunctionContext) = instructions.build {
        val function = operations.findBinaryFunctionForType(ctx.start)
        appendBinaryInstruction(function)
    }

    override fun exitUnaryFunction(ctx: BabelParser.UnaryFunctionContext) = instructions.build {
        val function = operations.findUnaryFunctionNamed(ctx.text)
        appendUnaryInstruction(function)
    }

    override fun exitVar(ctx: BabelParser.VarContext) = instructions.build {
        when(ctx.parent) {
            is BabelParser.ScalarExprContext, is BabelParser.BooleanExprContext -> append {
                val index = (stack.pop().roundToIndex() ?: throw IndexOutOfBoundsException("Attempted to use NaN as index")) - 1
                val globals = globals.values.toList()

                if (index !in 0 until globals.size) {
                    throw IndexOutOfBoundsException(
                            "attempted to access 'var[${index + 1}]' " +
                                    "(the ${(index + 1).withOrdinalSuffix()} parameter) " +
                                    "when only ${globals.size} exist"
                    )
                }
                val value = globals[index]

                stack.push(value)
            }
            is BabelParser.AssignmentContext -> {
                //noop
            }
            else -> TODO("unknown use of var in ${ctx.text}")
        }
    }

    override fun exitNegate(ctx: BabelParser.NegateContext) = instructions.build {
        appendUnaryInstruction(UnaryOps.Inversion)
    }

    override fun exitSum(ctx: BabelParser.SumContext) = instructions.build {
        append { stack.push(0.0 ) }
        appendBinaryInstruction(BinaryOps.Sum)
    }

    override fun exitProd(ctx: BabelParser.ProdContext) = instructions.build {
        append { stack.push(1.0) }
        appendBinaryInstruction(BinaryOps.Multiply)
    }

    override fun exitVariable(ctx: BabelParser.VariableContext) = instructions.build {
        val variable = ctx.text
        append {
            val value = heap[variable] ?: globals.getValue(variable)
            stack.push(value)
        }
    }

    override fun exitLiteral(ctx: BabelParser.LiteralContext) = instructions.build {

        fun Token.findValue(): Double = (this as? ValueToken)?.value ?: text.toDouble()

        val value: Number = when {
            ctx.CONSTANT() != null -> operations.findValueForConstant(ctx.text)
            ctx.INTEGER() != null -> ctx.INTEGER().symbol.findValue()
            ctx.FLOAT() != null -> ctx.FLOAT().symbol.findValue()
            
            else -> TODO("unknown literal type for ${ctx.text}")
        }

        append {
            stack.push(value.toDouble())
        }
    }

    private val IntRange.span: Int get() = last - first + 1

    companion object {
        val Log = Logger.getLogger(CodeGeneratingWalker::class.java.canonicalName)
    }
}

/**
 * Class to adapt simple algebra within java into an executable & babel-consumable form.
 *
 * Strategy here is to first consult a couple maps looking for the value or operation,
 * then dump the problem on Java.lang.Math to look up functions defined in the grammar but not
 * in the custom maps.
 */
internal class RuntimeNumerics {

    // note that the funny code here is primarily for debugability,
    // we don't use objects for any eager-ness or etc reason, a `val cos = Math::cos`
    // would more-or-less work just fine,
    // but it would be more difficult to debug.
    // as written, under the debugger, things should look pretty straight-forward.

    fun findValueForConstant(constantName: String): Number = when(constantName){
        "e" -> Math.E
        "pi" -> Math.PI
        else -> TODO("unknown math constant $constantName")
    }

    fun findUnaryFunctionNamed(functionName: String): UnaryOp = when(functionName) {

        "cos" -> UnaryOps.Cos
        "sin" -> UnaryOps.Sin
        "tan" -> UnaryOps.Tan
        "atan" -> UnaryOps.Atan
        "acos" -> UnaryOps.Acos
        "asin" -> UnaryOps.Asin
        "sinh" -> UnaryOps.Sinh
        "cosh" -> UnaryOps.Cosh
        "tanh" -> UnaryOps.Tanh
        "cot" -> UnaryOps.Cot
        "ln" -> UnaryOps.Ln
        "log" -> UnaryOps.Log
        "abs" -> UnaryOps.Abs
        "sqrt" -> UnaryOps.Sqrt
        "cbrt" -> UnaryOps.Cbrt
        "sqr" -> UnaryOps.Sqr
        "cube" -> UnaryOps.Cube
        "ceil" -> UnaryOps.Ceil
        "floor" -> UnaryOps.Floor
        "sgn" -> UnaryOps.Sgn

        else -> TODO("unknown unary operation $functionName")
    }

    fun findBinaryFunctionForType(token: Token): BinaryOp = when(token.type){
        MAX -> BinaryOps.Max
        MIN -> BinaryOps.Min
        LOG -> BinaryOps.LogB

        else -> TODO("unknown binary operation ${token.text}")
    }
}

// the syntax:
//    object cos: UnaryOp by Math::cos
// is a little more elegant, but
internal object UnaryOps {
    object Cos: UnaryOp { override fun invoke(p0: Double) = Math.cos(p0) }
    object Sin: UnaryOp { override fun invoke(p0: Double) = Math.sin(p0) }
    object Tan: UnaryOp { override fun invoke(p0: Double) = Math.tan(p0) }
    object Atan: UnaryOp { override fun invoke(p0: Double) = Math.atan(p0) }
    object Acos: UnaryOp { override fun invoke(p0: Double) = Math.acos(p0) }
    object Asin: UnaryOp { override fun invoke(p0: Double) = Math.asin(p0) }
    object Sinh: UnaryOp { override fun invoke(p0: Double) = Math.sinh(p0) }
    object Cosh: UnaryOp { override fun invoke(p0: Double) = Math.cosh(p0) }
    object Tanh: UnaryOp { override fun invoke(p0: Double) = Math.tanh(p0) }
    object Cot: UnaryOp { override fun invoke(p0: Double) = 1 / Math.tan(p0) }
    //dont like that 'log' is the natural logarithm and 'log10' is log base 10 in java terms
    //so we'll switch it here
    object Ln: UnaryOp { override fun invoke(p0: Double) = Math.log(p0) }
    object Log: UnaryOp { override fun invoke(p0: Double) = Math.log10(p0) }
    object Abs: UnaryOp { override fun invoke(p0: Double) = Math.abs(p0) }
    object Sqrt: UnaryOp { override fun invoke(p0: Double) = Math.sqrt(p0) }
    object Cbrt: UnaryOp { override fun invoke(p0: Double) = Math.cbrt(p0) }
    object Sqr: UnaryOp { override fun invoke(p0: Double) = p0 * p0 }
    object Cube: UnaryOp { override fun invoke(p0: Double) = p0 * p0 * p0 }
    object Ceil: UnaryOp { override fun invoke(p0: Double) = Math.ceil(p0) }
    object Floor: UnaryOp { override fun invoke(p0: Double) = Math.floor(p0) }
    object Inversion: UnaryOp { override fun invoke(p0: Double) = -p0 }
    object Sgn: UnaryOp { override fun invoke(p0: Double) = Math.signum(p0) }
}

object BinaryOps {
    object LogB: BinaryOp { override fun invoke(a: Double, b: Double) = Math.log(b) / Math.log(a) }
    object Max: BinaryOp { override fun invoke(a: Double, b: Double) = Math.max(a, b) }
    object Min: BinaryOp { override fun invoke(a: Double, b: Double) = Math.min(a, b) }

    object Sum: BinaryOp { override fun invoke(a: Double, b: Double) = a + b }
    object Multiply: BinaryOp { override fun invoke(a: Double, b: Double) = a * b }

    object Subtract: BinaryOp { override fun invoke(a: Double, b: Double) = a - b }
    object Divide: BinaryOp { override fun invoke(a: Double, b: Double) = a / b }

    object Exponentiation : BinaryOp { override fun invoke(a: Double, b: Double) = Math.pow(a, b) }
    object Modulo : BinaryOp { override fun invoke(a: Double, b: Double) = a % b }
}

internal class ErrorCollectingListener : BaseErrorListener(){

    var errors : Set<BabelExpressionProblem> = emptySet()
        private set

    override fun syntaxError(
            recognizer: Recognizer<*, *>?,
            offendingSymbol: Any?,
            lineNumber: Int,
            rowNumber: Int,
            message: String,
            exception: RecognitionException?
    ) {
        val token = offendingSymbol as? Token
        errors += BabelExpressionProblem(message, token?.line ?: 1, token?.charPositionInLine ?: 1)
    }
}

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

    override fun exitBooleanExpr(ctx: BooleanExprContext) {

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

                configure(childScalar){
                    binaryFunction {
                        // given our interpretation of (-inf to 0] => true, (0 to +inf) => false
                        // "max" supplies us with a logical "and"
                        terminal(MAX, "~==")
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

                val x = 4;
            }
            else -> {
                //no-op for error nodes or re-written trees.
            }
        }
    }

    private fun rewriteGreaterEqual(childScalar: ScalarExprContext) {
        swapLiteralChildren(childScalar)
        swapInequalityWithSubtraction(childScalar, "~>=")
    }

    private fun rewriteLessEqual(childScalar: ScalarExprContext) {
        swapInequalityWithSubtraction(childScalar, "~<=")
    }

    //signature implies purity, _but it absolutely is not pure_
    private fun insertScalar(ctx: BooleanExprContext): ScalarExprContext {
        
        val originalChildren = ArrayList(ctx.children)
        ctx.children.clear()

        configure(ctx){
            scalar(originalChildren)
        }

        return ctx.children.single() as ScalarExprContext
    }

    private fun addEpsilon(ctx: ScalarExprContext) {

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
    
    private fun swapInequalityWithSubtraction(ctx: ScalarExprContext, replacedElement: String) {

        val originalChildren = ArrayList(ctx.children)
        require(originalChildren.size == 3) //TODO: include an assertion that the middle child is an operation.
        // Requires a bit of polymorphism I wont have until i remove the `plus` `gteq`, etc productions.
        
        ctx.children.clear()

        configure(ctx){
            append(originalChildren.first())
            minus(text = replacedElement)
            append(originalChildren.last())
        }
    }

    private fun swapLiteralChildren(ctx: ScalarExprContext) {
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

internal fun Int.withOrdinalSuffix(): String
        = this.toString() + when (this % 100) {
            11, 12, 13 -> "th"
            else -> when (this % 10) {
                1 -> "st"
                2 -> "nd"
                3 -> "rd"
                4, 5, 6, 7, 8, 9, 0 -> "th"
                else -> TODO()
            }
        }

internal fun Double.roundToIndex(): Int?
        = if ( ! this.isNaN()) Math.round(this).toInt() else null

internal class Rewriter(val target: ParserRuleContext): Closeable {

    init {
        if(target.children == null){ target.children = mutableListOf() }
    }

    override fun close() {
        target.start = target.children.firstOrNull()?.let { when(it) {
            is TerminalNode -> it.symbol
            is ParserRuleContext -> it.start
            else -> null
        }}

        target.stop = target.children.lastOrNull()?.let { when(it) {
            is TerminalNode -> it.symbol
            is ParserRuleContext -> it.start
            else -> null
        }}
    }

    fun append(node: ParseTree){ target.children.add(node) }
    fun prepend(node: ParseTree) { target.children.add(0, node) }

    fun times(text: String = "*") = append(BabelParser.MultContext(target, -1).apply {
        terminal = CommonToken(BabelLexer.MULT, text)
    })
    fun plus(text: String = "+") = append(BabelParser.PlusContext(target, -1).apply {
        terminal = CommonToken(BabelLexer.PLUS, text)
    })
    fun minus(text: String = "-") = append(BabelParser.MinusContext(target, -1).apply {
        terminal = CommonToken(BabelLexer.MINUS, text)
    })
    fun literal(value: Double) = append(BabelParser.LiteralContext(target, -1).apply {
        terminal = ValueToken(value)
    })
    fun greaterEqual(text: String = ">=") = append(BabelParser.GteqContext(target, -1).apply {
        terminal = CommonToken(BabelLexer.GTEQ, text)
    })
    fun lessEqual(text: String = "<=") = append(BabelParser.LteqContext(target, -1).apply {
        terminal = CommonToken(BabelLexer.LTEQ, text)
    })

    fun scalar(initialChildren: List<ParseTree> = mutableListOf(), block: Rewriter.() -> Unit = {}): ScalarExprContext {
        val scalarCtx = ScalarExprContext(target, -1).apply { children = initialChildren }
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

internal fun <T> configure(target: T, block: Rewriter.() -> Unit): T where T: ParserRuleContext {
    Rewriter(target).use { it.block() }
    return target
}

