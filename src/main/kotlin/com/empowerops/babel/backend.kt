package com.empowerops.babel

import com.empowerops.babel.BabelLexer.*
import com.empowerops.babel.BabelParser.BooleanExprContext
import com.empowerops.babel.BabelParser.ScalarExprContext
import com.empowerops.babel.RuntimeBabelBuilder.RuntimeConfiguration
import kotlinx.collections.immutable.ImmutableMap
import kotlinx.collections.immutable.immutableMapOf
import kotlinx.collections.immutable.plus
import org.antlr.v4.runtime.*
import org.antlr.v4.runtime.misc.Interval
import org.antlr.v4.runtime.misc.Utils
import org.antlr.v4.runtime.tree.ErrorNode
import java.util.*
import java.util.logging.Logger

typealias UnaryOp = (Double) -> Double
typealias BinaryOp = (Double, Double) -> Double

internal class TypeErrorReportingWalker(val sourceText: String) : BabelParserBaseListener() {

    enum class ExpressionType { Boolean, Scalar }
    data class TypedExpression(val expressionType: ExpressionType, val ctx: BabelParser.ScalarExprContext)

    fun BabelParser.ScalarExprContext.typedAs(type: ExpressionType) = TypedExpression(type, this)
    fun BabelParser.ScalarExprContext.asScalarExpression() = TypedExpression(ExpressionType.Scalar, this)

    val exprType: Deque<TypedExpression> = LinkedList()

    var problems: Set<ExpressionProblem> = emptySet()
        private set

    override fun exitScalarExpr(ctx: BabelParser.ScalarExprContext) {

        when {
            ctx.childCount == 0 || ctx is ErrorNode -> {
                // the most common case for this is when you having a dangling expression (eg 'x1 + x2 +')
                // but it can happen in many cercomstances. In playing around I've found that
                // a little bit of pollution here by adding an extra argument
                // to the stack doesn't cause any new failure modes
                // and allows analysis to continue on existing failure modes nicely.
                // it does mean we cannot have a post-condition that this analysis is "clean"
                // (eg a require(exprType.isEmpty()) after analysis)
                exprType.push(ctx.asScalarExpression())
                CodeGeneratingWalker.Log.fine { "pushed scalar type for imaginery node" }
            }
            ctx.callsLiteralOrVariable() -> exprType.push(ctx.asScalarExpression())
            ctx.callsDynamicVariableAccess() -> exprType.push(ctx.asScalarExpression())
            ctx.callsInlineExpression() -> {
                val child = exprType.pop()
                exprType.push(ctx.typedAs(child.expressionType))
            }
            ctx.callsAggregation() -> {
                ensureIsScalar(exprType.pop())
                ensureIsScalar(exprType.pop())
                exprType.push(ctx.asScalarExpression())
            }
            ctx.callsBinaryFunction() -> {
                ensureIsScalar(exprType.pop())
                ensureIsScalar(exprType.pop())
                exprType.push(ctx.asScalarExpression())
            }
            ctx.callsUnaryOpOrFunction() -> {
                ensureIsScalar(exprType.pop())
                exprType.push(ctx.asScalarExpression())
            }
            ctx.callsBinaryOp() -> {

                ensureIsScalar(exprType.pop())
                ensureIsScalar(exprType.pop())

                when(ctx.getChild(1)){
                    is BabelParser.GtContext, is BabelParser.GteqContext,
                    is BabelParser.LtContext, is BabelParser.LteqContext -> {
                        exprType.push(ctx.typedAs(ExpressionType.Boolean))
                    }
                    else -> exprType.push(ctx.asScalarExpression())
                }
            }
            ctx.wrapsLambda() -> {
                ensureIsScalar(exprType.pop())

                exprType.push(ctx.asScalarExpression())
            }
            else -> TODO("unknown expr ${ctx.text}")
        }
    }

    fun ensureIsScalar(expr: TypedExpression): Unit = when(expr.expressionType){
        ExpressionType.Scalar -> {}
        ExpressionType.Boolean -> {
            val problemText = expr.ctx.text
            val rangeInText = expr.ctx.textLocation

            problems += ExpressionProblem(
                    sourceText,
                    problemText,
                    rangeInText,
                    expr.ctx.start.line,
                    expr.ctx.start.charPositionInLine + 1,
                    "Attempted to embed boolean expression in scalar expression",
                    "evaluates to true/false"
            )
        }
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

        fun popOperation(): RuntimeMemory.() -> Unit = jobs.pop()

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

        operator fun invoke(globalVars: Map<String, Double>, references: @Ordered List<String>): Double {
            require(jobs.count() == 1) { "stack-machine code generation failure" }

            val scope = RuntimeMemory(globalVars, references)
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
        val globals: Map<String, Double>,
        val references: @Ordered List<String>,
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
                val problemText = ctx.makeAbbreviatedProblemText()

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
                                ?: throw makeRuntimeException(problemText, textLocation, "Illegal bound value", indexCandidate)
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
            summary: String,
            problemValue: Double
    ): RuntimeBabelException {
        val textBeforeProblem = sourceText.substring(0..rangeInText.start)
        return RuntimeBabelException(RuntimeProblemSource(
                sourceText,
                problemText,
                rangeInText,
                textBeforeProblem.count { it == '\n' }+1,
                textBeforeProblem.substringAfterLast("\n").count() - 1,
                summary,
                "evaluates to $problemValue",
                heap,
                globals,
                references
        ))
    }

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
        val function = RuntimeNumerics.findBinaryFunctionForType(ctx.start)
        appendBinaryInstruction(function)
    }

    override fun exitUnaryFunction(ctx: BabelParser.UnaryFunctionContext) = instructions.build {
        val function = RuntimeNumerics.findUnaryFunction(ctx.start)
        appendUnaryInstruction(function)
    }

    override fun exitVar(ctx: BabelParser.VarContext) = instructions.build {
        when(ctx.parent) {
            is BabelParser.ScalarExprContext, is BabelParser.BooleanExprContext -> append {
                val index = (stack.pop().roundToIndex() ?: throw IndexOutOfBoundsException("Attempted to use NaN as index")) - 1
                if (index !in 0 until references.size) {
                    throw IndexOutOfBoundsException(
                            "attempted to access 'var[${index + 1}]' " +
                                    "(the ${(index + 1).withOrdinalSuffix()} parameter) " +
                                    "when only ${references.size} exist"
                    )
                }
                val symbol = references[index]
                val value = globals[symbol]

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

        val value = ctx.value

        append {
            stack.push(value)
        }
    }

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
internal object RuntimeNumerics {

    fun findValueForConstant(constant: Token): Number = when(constant.text){
        "e" -> Math.E
        "pi" -> Math.PI
        else -> TODO("unknown math constant ${constant.text}")
    }

    fun findUnaryFunction(function: Token): UnaryOp = when(function.type) {

        COS -> UnaryOps.Cos
        SIN -> UnaryOps.Sin
        TAN -> UnaryOps.Tan
        ATAN -> UnaryOps.Atan
        ACOS -> UnaryOps.Acos
        ASIN -> UnaryOps.Asin
        SINH -> UnaryOps.Sinh
        COSH -> UnaryOps.Cosh
        TANH -> UnaryOps.Tanh
        COT -> UnaryOps.Cot
        LN -> UnaryOps.Ln
        LOG -> UnaryOps.Log
        ABS -> UnaryOps.Abs
        SQRT -> UnaryOps.Sqrt
        CBRT -> UnaryOps.Cbrt
        SQR -> UnaryOps.Sqr
        CUBE -> UnaryOps.Cube
        CIEL -> UnaryOps.Ceil
        FLOOR -> UnaryOps.Floor
        SGN -> UnaryOps.Sgn

        else -> TODO("unknown unary operation ${function.text}")
    }

    fun findBinaryFunctionForType(token: Token): BinaryOp = when(token.type){
        MAX -> BinaryOps.Max
        MIN -> BinaryOps.Min
        LOG -> BinaryOps.LogB

        else -> TODO("unknown binary operation ${token.text}")
    }
}

internal object UnaryOps {

    // note that the funny code here is primarily for debugability,
    // we don't use objects for any eager-ness or etc reason, a `val cos = Math::cos`
    // would more-or-less work just fine,
    // but it would be more difficult to debug.
    // as written, under the debugger, things should look pretty straight-forward.

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

internal class SyntaxErrorCollectingListener(val sourceText: String): BaseErrorListener(){

    var errors : Set<ExpressionProblem> = emptySet()
        private set

    @Suppress("NAME_SHADOWING") //I actually use this a lot, intentionally, its a great way to limit scope...?
    override fun syntaxError(
            recognizer: Recognizer<*, *>?,
            offendingSymbol: Any?,
            lineNumber: Int,
            rowNumber: Int,
            message: String,
            exception: RecognitionException?
    ) {
//        val token = offendingSymbol as Token
        val message = when(exception){
            is NoViableAltException -> {
                //no good,
                // for the input "1+(x > 3) + 2"
                // the default message is no viable alternative at input '1+(x>'
                // with the 'offending token' being '<'
                // note that the no viable alt indicates backtracking,
                // the below code returns some nonsensical allowed inputs, including 'INTEGER' and 'cos'.
                // consider that if we replaced the '<' with '345', it would read '1 + (x 123', which doesnt make sense.
//                val intervals = exception.expectedTokens.intervals
//                val names = intervals.asSequence().flatMap { (it.a .. it.b).asSequence() }.map { recognizer.vocabulary.getDisplayName(it) }.toList()

                "unexpected symbol"
                //note the default is 'no viable alternative at [backtracked-string]', which, given our error system, is too verbose.
            }
            else -> message
        }

        fun makeProblem(abbreviatedProblemText: String, rangeInText: IntRange, lineNo: Int, characterNo: Int, problemValueDescription: String) = ExpressionProblem(
                sourceText,
                abbreviatedProblemText,
                rangeInText,
                lineNo,
                characterNo,
                "syntax error",
                problemValueDescription
        )

        errors +=
                //lexer error, eg bad variable names
                when(offendingSymbol){
                    is Token -> {
                        val token = offendingSymbol
                        // EOF is expressed as a negative range on the last char (eg 9..8 in 'x1 + x2 +'),
                        // so we coerce it to 8..8 to highlight the last char
                        val rangeInText = token.startIndex.coerceAtMost(token.stopIndex) .. token.stopIndex

                        val tokenText = when(token.type){
                            Token.EOF -> "end of expression"
                            else -> token.text
                        }

                        makeProblem(tokenText, rangeInText, lineNumber, token.charPositionInLine, message)
                    }
                    null -> when(exception){
                        is LexerNoViableAltException -> {
                            //copied from the exceptions own toString()... its really nasty :(
                            var symbol = exception.inputStream.getText(Interval.of(exception.startIndex, exception.startIndex))
                            symbol = Utils.escapeWhitespace(symbol, false)

                            val charIndex = exception.inputStream.getText(Interval.of(0, exception.startIndex-1)).substringAfter('\n').count()

                            val descriptor = "character${if (symbol.length >= 2) "s" else ""}"
                            makeProblem(
                                    symbol,
                                    exception.startIndex .. exception.startIndex,
                                    lineNumber,
                                    charIndex,
                                    "illegal $descriptor"
                            )
                        }
                        else -> TODO()
                    }
                    else -> TODO()
                }

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
        = if (this.isFinite()) Math.round(this).toInt() else null

internal val BabelParser.LiteralContext.value: Double get() {

    fun Token.value(): Double = (this as? ValueToken)?.value ?: text.toDouble()

    val value: Double = when {
        CONSTANT() != null -> RuntimeNumerics.findValueForConstant(CONSTANT().symbol)
        INTEGER() != null -> INTEGER().symbol.value()
        FLOAT() != null -> FLOAT().symbol.value()

        else -> TODO("unknown literal type for ${text}")
    }.toDouble()

    return value
}

internal val IntRange.span: Int get() = last - first + 1
