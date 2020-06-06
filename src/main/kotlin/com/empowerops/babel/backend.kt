package com.empowerops.babel

import com.empowerops.babel.BabelLexer.*
import com.empowerops.babel.BabelParser.BooleanExprContext
import com.empowerops.babel.BabelParser.ScalarExprContext
import kotlinx.collections.immutable.ImmutableMap
import kotlinx.collections.immutable.immutableMapOf
import kotlinx.collections.immutable.plus
import org.antlr.v4.runtime.*
import org.antlr.v4.runtime.misc.Interval
import org.antlr.v4.runtime.misc.Utils
import java.util.*
import java.util.logging.Logger

typealias UnaryOp = (Double) -> Double
typealias BinaryOp = (Double, Double) -> Double
typealias VariadicOp = (DoubleArray) -> Double

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

/**
 * a chunk is a group of instructions, typically that correspond to a node.
 */
internal typealias Chunk = Array<Instruction>
internal typealias Instruction = RuntimeMemory.() -> Unit

internal fun Instruction(op: RuntimeMemory.() -> Unit): Instruction = op

internal data class Label(val label: String): ((RuntimeMemory) -> Unit) {
    override fun invoke(memory: RuntimeMemory) {}
}
internal data class JumpIfGreaterEqual(val label: String): ((RuntimeMemory) -> Unit) {
    override fun invoke(memory: RuntimeMemory) {}
}

internal class RuntimeConfiguration {

    private var labelNo = 1;
    private var instructionChunks: Deque<Chunk> = LinkedList()

    fun enterScope() = Instruction { parentScopes.push(heap) }
    fun exitScope() = Instruction { heap = parentScopes.pop() }

    fun nextIntLabel(): Int = labelNo++

    fun flatten(): List<(RuntimeMemory) -> Unit> {
        return instructionChunks.flatMap { it.asIterable() }
    }

    private fun _append(instruction: Instruction) {
        instructionChunks.push(arrayOf(instruction))
    }

    fun append(instruction: Instruction){
        _append(instruction)
    }

    fun appendChunk(instructionChunk: Chunk){
        instructionChunks.push(instructionChunk)
    }
    fun appendAsSingleChunk(instructions: List<Instruction>){
        instructionChunks.push(instructions.toTypedArray())
    }
    fun appendAsSingleChunk(vararg instructions: RuntimeMemory.() -> Unit){
        instructionChunks.push(instructions as Chunk)
    }
    fun appendBinaryOperator(operation: BinaryOp){
        _append {
            val right = stack.popDouble()
            val left = stack.popDouble()
            val result = operation.invoke(left, right)
            stack.push(result)
        }
    }
    fun appendUnaryOperator(operation: UnaryOp){
        _append {
            val arg = stack.popDouble()
            val result = operation.invoke(arg)
            stack.push(result)
        }
    }
    fun appendVariadicInstruction(argCount: Int, operation: VariadicOp){
        _append {
            val input = stack.popDoubles(argCount)
            val result = operation.invoke(input)
            stack.push(result)
        }
    }

    fun popChunk(): Chunk = instructionChunks.pop()

}

internal data class RuntimeMemory(
        val globals: @Ordered Map<String, Double>,
        var heap: ImmutableMap<String, Number> = immutableMapOf(),
        val stack: LinkedList<Number> = LinkedList<Number>()
){
    val parentScopes: Deque<ImmutableMap<String, Number>> = LinkedList()
}

//TODO theres some easy optimization here, create a custom type thats baked by DoubleArray
//... though, you'd need some way of
fun Deque<Number>.popDouble() = pop().toDouble() //TODO: add stricter type-e-ness here.
fun Deque<Number>.popInt() = pop().toInt()
fun Deque<Number>.peekDouble() = peek() as Double
fun Deque<Number>.popDoubles(count: Int) = DoubleArray(count) { popDouble() }

internal class CodeGeneratingWalker(val sourceText: String) : BabelParserBaseListener() {

    val code = RuntimeConfiguration()

    override fun exitExpression(ctx: BabelParser.ExpressionContext) {
        val instructions = (ctx.statement() + ctx.returnStatement())
            .map { code.popChunk() }
            .asReversed()
            .flatMap { it.asIterable() }

        code.appendAsSingleChunk(instructions)
    }

    override fun exitAssignment(ctx: BabelParser.AssignmentContext) {
        val varName = ctx.name().text
        val valueGenerator = code.popChunk()

        val newChunk = valueGenerator + Instruction {
            val value = stack.popDouble()
            heap += varName to value
        }
        code.appendChunk(newChunk)
    }

    override fun exitBooleanExpr(ctx: BooleanExprContext) {
        when {
            ctx.childCount == 1 && ctx.scalarExpr() != null -> {
                //was rewritten, noop
            }
            ctx.callsBinaryOp() -> {
                val right = code.popChunk()
                val op = code.popChunk()
                val left = code.popChunk()

                code.appendAsSingleChunk(*left, *right, *op)
            }
            else -> TODO("unknown scalarExpr ${ctx.text}")
        }
    }

    override fun exitScalarExpr(ctx: ScalarExprContext) {

        when {
            ctx.childCount == 0 -> {
                //typically when an error occurs in parse tree generation -> noop
                Log.fine { "skipped code gen for $ctx as it erroneously has no children" }
            }
            ctx.callsLiteralOrVariable() -> {
                //noop -- handled by child directly
            }
            ctx.callsDynamicVariableAccess() -> {
                val indexingExpr = code.popChunk()
                val indexToValueConverter = code.popChunk()

                val newChunk = indexingExpr + indexToValueConverter
                code.appendChunk(newChunk)
            }
            ctx.callsInlineExpression() -> {
                //noop -- brackets are purely a directive for order-of-ops, no code changes needed.
            }
            ctx.callsAggregation() -> {

                val lowerBoundRangeInText = ctx.scalarExpr(0).textLocation
                val upperBoundRangeInText = ctx.scalarExpr(1).textLocation
                val problemText = ctx.makeAbbreviatedProblemText()

                val lambda = code.popChunk()
                val upperBoundExpr = code.popChunk()
                val lowerBoundExpr = code.popChunk()
                val aggregator = code.popChunk()
                val seedProvider = code.popChunk()

                fun convertToIndex(textLocation: IntRange, expr: Chunk): Chunk = expr + Instruction {
                    val indexCandidate = stack.popDouble() //TODO this should simply be an integer
                    val result = indexCandidate.roundToIndex()
                            ?: throw makeRuntimeException(problemText, textLocation, "Illegal bound value", indexCandidate)

                    stack.push(result)
                }

                val upperBound = convertToIndex(upperBoundRangeInText, upperBoundExpr)
                val lowerBound = convertToIndex(lowerBoundRangeInText, lowerBoundExpr)

                val instructions = ArrayList<Instruction>()

                val accum = "%accum-${code.nextIntLabel()}"
                val loopHeader = "%loop-${code.nextIntLabel()}"

                instructions += upperBound                                              // ub
                instructions += lowerBound                                              // ub, idx
                instructions += seedProvider                                            // ub, idx, accum
                instructions += { heap += accum to stack.popDouble() }                  // ub, idx
                instructions += Label(loopHeader)                                       // ub, idx
                instructions += { stack.push(heap[accum]) }                             // ub, idx, accum
                instructions += {
                    val x = 4;
                }
                instructions += lambda                                                  // ub, idx, accum, iterationResult
                instructions += {
                    val y = lambda
                    val x = 4;
                }
                instructions += aggregator                                              // ub, idx, accumN
                instructions += { heap += accum to stack.popDouble() }                  // ub, idx
                instructions += { stack.push(stack.popInt() + 1) }                   // ub, idxN
                instructions += {
                    val x = 4;
                }
                instructions += JumpIfGreaterEqual(loopHeader)

                instructions += {
                    stack.popInt(/*idx*/);                                              // ub
                    stack.popInt(/*ub*/);                                               // [empty]
                    stack.push(heap[accum])                                             // accum
                }

                instructions += {
                    val x = 4;
                }

                code.appendAsSingleChunk(instructions)
            }
            ctx.lambdaExpr()?.value != null -> {
                //closed lambda expression, added by a rewriter

                //noop; everything was handled by enter/exit lambda
            }
            ctx.callsBinaryFunction() -> {
                val right = code.popChunk()
                val left = code.popChunk()
                val function = code.popChunk()

                code.appendAsSingleChunk(*left, *right, *function)
            }
            ctx.callsUnaryOpOrFunction() -> {
                val arg = code.popChunk()
                val function = code.popChunk()

                code.appendAsSingleChunk(*arg, *function)
            }
            ctx.callsBinaryOp() -> {
                val right = code.popChunk()
                val operation = code.popChunk()
                val left = code.popChunk()

                code.appendAsSingleChunk(*left, *right, *operation)
            }
            ctx.callsVariadicFunction() -> {
                val args = (0 until ctx.variadicFunction().argCount)
                    .map { code.popChunk() }
                    .asReversed()
                    .flatMap { it.asIterable() }

                val accumulatorFunction = code.popChunk()

                code.appendAsSingleChunk(*args.toTypedArray(), *accumulatorFunction)
            }
            
            else -> TODO("unknown scalarExpr ${ctx.text}")
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
                globals
        ))
    }

    override fun exitLambdaExpr(ctx: BabelParser.LambdaExprContext) {
        val lambdaParamName = ctx.name().text
        val childExpression = code.popChunk()

        code.appendAsSingleChunk(                               // ub, idx, accum <- from loop
            code.enterScope(),
            Instruction {
                heap += lambdaParamName to (ctx.value ?: stack[1]).toInt()
            },
            *childExpression,
            code.exitScope()
        )
    }

    override fun exitMod(ctx: BabelParser.ModContext) { code.appendBinaryOperator(BinaryOps.Modulo) }
    override fun exitPlus(ctx: BabelParser.PlusContext) { code.appendBinaryOperator(BinaryOps.Sum) }
    override fun exitMinus(ctx: BabelParser.MinusContext) { code.appendBinaryOperator(BinaryOps.Subtract) }
    override fun exitMult(ctx: BabelParser.MultContext) { code.appendBinaryOperator(BinaryOps.Multiply) }
    override fun exitDiv(ctx: BabelParser.DivContext) { code.appendBinaryOperator(BinaryOps.Divide) }
    override fun exitRaise(ctx: BabelParser.RaiseContext) { code.appendBinaryOperator(BinaryOps.Exponentiation) }

    override fun exitBinaryFunction(ctx: BabelParser.BinaryFunctionContext) {
        val function = RuntimeNumerics.findBinaryFunctionForType(ctx.start)
        code.appendBinaryOperator(function)
    }

    override fun exitUnaryFunction(ctx: BabelParser.UnaryFunctionContext) {
        val function = RuntimeNumerics.findUnaryFunction(ctx.start)
        code.appendUnaryOperator(function)
    }

    override fun exitVariadicFunction(ctx: BabelParser.VariadicFunctionContext) {
        val function = RuntimeNumerics.findVariadicFunctionForType(ctx.start)
        code.appendVariadicInstruction(ctx.argCount, function)
    }

    override fun exitVar(ctx: BabelParser.VarContext) {
        when(ctx.parent) {
            is BabelParser.ScalarExprContext, is BabelParser.BooleanExprContext -> {

                val rangeInText = (ctx.parent as ScalarExprContext).scalarExpr(0).textLocation
                val problemText = ctx.parent.text

                code.append {
                    val i1ndex = stack.popInt()
                    val globals = globals.values.toList()

                    if (i1ndex-1 !in globals.indices) {
                        val message = "attempted to access 'var[$i1ndex]' " +
                                "(the ${i1ndex.withOrdinalSuffix()} parameter) " +
                                "when only ${globals.size} exist"
                        throw makeRuntimeException(problemText, rangeInText, message, i1ndex.toDouble())
                    }
                    val value = globals[i1ndex-1]

                    stack.push(value)
                }
            }
            is BabelParser.AssignmentContext -> {
                //noop
            }
            else -> TODO("unknown use of var in ${ctx.text}")
        }
    }

    override fun exitNegate(ctx: BabelParser.NegateContext) {
        code.appendUnaryOperator(UnaryOps.Inversion)
    }

    override fun exitSum(ctx: BabelParser.SumContext) {
        code.append { stack.push(0.0 ) }
        code.appendBinaryOperator(BinaryOps.Sum)
    }

    override fun exitProd(ctx: BabelParser.ProdContext) {
        code.append { stack.push(1.0) }
        code.appendBinaryOperator(BinaryOps.Multiply)
    }

    override fun exitVariable(ctx: BabelParser.VariableContext) {
        val variable = ctx.text
        code.append {
            val value = heap[variable] ?: globals.getValue(variable)
            stack.push(value)
        }
    }

    override fun exitLiteral(ctx: BabelParser.LiteralContext) {
        code.append(LiteralClosure(ctx.value))
    }

    companion object {
        val Log = Logger.getLogger(CodeGeneratingWalker::class.java.canonicalName)
    }
}

internal class LiteralClosure(val value: Double): (RuntimeMemory) -> Unit {
    override fun invoke(memory: RuntimeMemory) {
        memory.stack.push(value)
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
        LOG -> BinaryOps.LogB

        else -> TODO("unknown binary operation ${token.text}")
    }

    fun findVariadicFunctionForType(token: Token): VariadicOp = when(token.type) {
        MAX -> VariadicOps.Max
        MIN -> VariadicOps.Min

        else -> TODO("unknown variadic operation ${token.text}")
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

    object Sum: BinaryOp { override fun invoke(a: Double, b: Double) = a + b }
    object Multiply: BinaryOp { override fun invoke(a: Double, b: Double) = a * b }

    object Subtract: BinaryOp { override fun invoke(a: Double, b: Double) = a - b }
    object Divide: BinaryOp { override fun invoke(a: Double, b: Double) = a / b }

    object Exponentiation : BinaryOp { override fun invoke(a: Double, b: Double) = Math.pow(a, b) }
    object Modulo : BinaryOp { override fun invoke(a: Double, b: Double) = a % b }
}

object VariadicOps {
    object Max: VariadicOp { override fun invoke(input: DoubleArray) = input.max()!! }
    object Min: VariadicOp { override fun invoke(input: DoubleArray) = input.min()!! }
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
            msg: String,
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
            else -> msg
        }

        errors += when {
            //lexer error, eg bad variable names
            offendingSymbol is Token -> {
                val token = offendingSymbol
                // EOF is expressed as a negative range on the last char (eg 9..8 in 'x1 + x2 +'),
                // so we coerce it to 8..8 to highlight the last char
                val rangeInText = token.startIndex.coerceAtMost(token.stopIndex) .. token.stopIndex

                val tokenText = when(token.type){
                    Token.EOF -> "end of expression"
                    else -> token.text
                }

                ExpressionProblem(
                    sourceText,
                    tokenText,
                    rangeInText,
                    lineNumber,
                    token.charPositionInLine,
                    "syntax error",
                    message
                )
            }
            exception is LexerNoViableAltException -> {
                //copied from the exceptions own toString()... its really nasty :(
                var symbol = exception.inputStream.getText(Interval.of(exception.startIndex, exception.startIndex))
                symbol = Utils.escapeWhitespace(symbol, false)

                val charIndex = exception.inputStream.getText(Interval.of(0, exception.startIndex-1)).substringAfter('\n').count()

                val descriptor = "character${if (symbol.length >= 2) "s" else ""}"

                ExpressionProblem(
                    sourceText,
                    symbol,
                    exception.startIndex .. exception.startIndex,
                    lineNumber,
                    charIndex,
                    "syntax error",
                    "illegal $descriptor"
                )
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
