package com.empowerops.babel

import com.empowerops.babel.BabelLexer.*
import com.empowerops.babel.BabelParser.BooleanExprContext
import com.empowerops.babel.BabelParser.ScalarExprContext
import org.antlr.v4.runtime.*
import org.antlr.v4.runtime.misc.Interval
import org.antlr.v4.runtime.misc.Utils
import java.lang.IllegalStateException
import java.util.*
import java.util.logging.Logger
import kotlin.math.roundToInt

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


internal fun Instruction(op: (stack: List<Double>, heap: Map<String, Number>, globals: Map<String, Double>) -> Unit) = Instruction.Custom(op)

/**
 * a chunk is a group of instructions, typically that correspond to a node.
 */
internal typealias Chunk = Array<Instruction>

class Globals {
    var str: String = ""
    var range: IntRange = 0..0
    var doubleValue: Double = 0.0
    var intValue: Int = 0

    var binaryOp: BinaryOp = { a, b -> throw IllegalStateException() }
    var UnaryOp: UnaryOp = { a -> throw IllegalStateException() }
    var VariadicOp: VariadicOp = { args -> throw IllegalStateException() }
}

sealed class Instruction {
    internal class Custom(val operation: (stack: List<Double>, heap: Map<String, Number>, globals: Map<String, Double>) -> Unit): Instruction()

    //control
    // ... -> ... ; metadata
    data class Label(val label: String): Instruction()

    // ... left, right -> ... left, right; jumps if left > right
    data class JumpIfGreaterEqual(val label: String): Instruction()

    //memory

    // ... valueToBePutInHeap -> S; heap contains keyed value
    data class StoreD(val key: String): Instruction()
    data class StoreI(val key: String): Instruction()

    // ... -> ... valueFromHeap
    data class LoadD(val key: String): Instruction()

    // ... idx -> ... valueFromHeapAtIdx
    data class LoadDIdx(val problemText: String, val rangeInText: IntRange): Instruction()

    // ... -> ... value
    data class PushD(val value: Double): Instruction()
    data class PushI(val value: Int): Instruction()

    // ... erroneous -> ...
    object PopD: Instruction()

    // ... a, b -> a, b, b
    // if offset is 0, if it is 1, then it will be a,b,a
    data class Duplicate(val offset: Int): Instruction()

    object EnterScope: Instruction()
    object ExitScope: Instruction()

    //invoke
    // ... left, right -> ... result
    data class InvokeBinary(val op: BinaryOp): Instruction()
    // ... input -> ... result
    data class InvokeUnary(val op: UnaryOp): Instruction()
    // ... arg1, arg2, arg3 -> ... result
    data class InvokeVariadic(val argCount: Int, val op: VariadicOp): Instruction()
    object AddD: Instruction()
    object AddI: Instruction()
    object SubtractD: Instruction()
    object MultiplyD: Instruction()
    object DivideD: Instruction()

    //manipulation

    // ... decimalValue -> ... integerValue
    // converts a double to an integer for use as an index
    data class IndexifyD(val problemText: String, val rangeInText: IntRange) : Instruction()
}

// contains a kind of working-set of code pages (sets of instructions)
// the main benefit to using code-gen is that you dont need any hierarchy in the end product,
// but while building the end product, we need to keep "groups" of related instructions together,
// so they can be appended-to and re-ordered by the code generating walker.
// this is a helper facility for that goal
internal class CodeBuilder {

    private var labelNo = 1;
    private var instructionChunks: Deque<Chunk> = LinkedList()

    fun nextIntLabel(): Int = labelNo++

    fun flatten(): List<Instruction> = instructionChunks.flatMap { it.asIterable() }

    fun append(instruction: Instruction){ instructionChunks.push(arrayOf(instruction)) }
    fun appendChunk(instructionChunk: Chunk){ instructionChunks.push(instructionChunk) }
    fun appendAsSingleChunk(instructions: List<Instruction>){ instructionChunks.push(instructions.toTypedArray()) }
    fun appendAsSingleChunk(vararg instructions: Instruction){ instructionChunks.push(instructions as Chunk) }

    fun popChunk(): Chunk = instructionChunks.pop()
}

internal class CodeGeneratingWalker(val sourceText: String) : BabelParserBaseListener() {

    val code = CodeBuilder()

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

        code.appendAsSingleChunk(*valueGenerator, Instruction.StoreD(varName))
    }

    override fun exitBooleanExpr(ctx: BooleanExprContext) {
        when {
            ctx.childCount == 1 && ctx.scalarExpr() != null -> {
                //was rewritten, noop
            }
            ctx.callsBinaryOp() -> {
                Log.warning("called code-gen for binary boolean expression (when it should've been rewritten?)")
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

                val lbRange = ctx.scalarExpr(0).textLocation
                val ubRange = ctx.scalarExpr(1).textLocation
                val problemText = ctx.makeAbbreviatedProblemText()

                val lambda = code.popChunk()
                val upperBoundExpr = code.popChunk()
                val lowerBoundExpr = code.popChunk()
                val aggregator = code.popChunk()
                val seedProvider = code.popChunk()

                val instructions = ArrayList<Instruction>()

                val accum = "%accum-${code.nextIntLabel()}"
                val loopHeader = "%loop-${code.nextIntLabel()}"

                instructions += upperBoundExpr                                  // ub-double
                instructions += Instruction.IndexifyD(problemText, ubRange)     // ub
                instructions += lowerBoundExpr                                  // ub, lb-double
                instructions += Instruction.IndexifyD(problemText, lbRange)     // ub, lb (idx)
                instructions += seedProvider                                    // ub, idx, accum
                instructions += Instruction.StoreD(accum)                       // ub, idx
                instructions += Instruction.Label(loopHeader)                   // ub, idx
                instructions += Instruction.LoadD(accum)                        // ub, idx, accum
                instructions += lambda                                          // ub, idx, accum, iterationResult
                instructions += aggregator                                      // ub, idx, accum+
                instructions += Instruction.StoreD(accum)                       // ub, idx
                instructions += Instruction.PushI(1)                            // ub, idx, 1
                instructions += Instruction.AddI                                // ub, idx+
                instructions += Instruction.JumpIfGreaterEqual(loopHeader)      // ub, idx+
                instructions += Instruction.PopD                                // ub
                instructions += Instruction.PopD                                //
                instructions += Instruction.LoadD(accum)                        // accum

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

    override fun exitLambdaExpr(ctx: BabelParser.LambdaExprContext) {
        val lambdaParamName = ctx.name().text
        val childExpression = code.popChunk()

        code.appendAsSingleChunk(                                       // ub, idx, accum <- from loop
            Instruction.EnterScope,                                     // ub, idx, accum
            if(ctx.value != null) Instruction.PushI(ctx.value!!)
                else Instruction.Duplicate(offset = 1),                 // ub, idx, accum, idx
            Instruction.StoreI(lambdaParamName),                        // ub, idx, accum; idx is now 'i' in heap
            *childExpression,                                           // ub, idx, accum
            Instruction.ExitScope                                       // ub, idx, accum
        )
    }

    override fun exitMod(ctx: BabelParser.ModContext) {
        code.append(Instruction.InvokeBinary(BinaryOps.Modulo))
    }

    override fun exitPlus(ctx: BabelParser.PlusContext) { code.append(Instruction.AddD) }
    override fun exitMinus(ctx: BabelParser.MinusContext) { code.append(Instruction.SubtractD) }
    override fun exitMult(ctx: BabelParser.MultContext) { code.append(Instruction.MultiplyD) }
    override fun exitDiv(ctx: BabelParser.DivContext) { code.append(Instruction.DivideD) }

    override fun exitRaise(ctx: BabelParser.RaiseContext) {
        code.append(Instruction.InvokeBinary(BinaryOps.Exponentiation))
    }

    override fun exitBinaryFunction(ctx: BabelParser.BinaryFunctionContext) {
        val function = RuntimeNumerics.findBinaryFunctionForType(ctx.start)
        code.append(Instruction.InvokeBinary(function))
    }

    override fun exitUnaryFunction(ctx: BabelParser.UnaryFunctionContext) {
        val function = RuntimeNumerics.findUnaryFunction(ctx.start)
        code.append(Instruction.InvokeUnary(function))
    }

    override fun exitVariadicFunction(ctx: BabelParser.VariadicFunctionContext) {
        val function = RuntimeNumerics.findVariadicFunctionForType(ctx.start)
        code.append(Instruction.InvokeVariadic(ctx.argCount, function))
    }

    override fun exitVar(ctx: BabelParser.VarContext) {
        when(ctx.parent) {
            is BabelParser.ScalarExprContext, is BabelParser.BooleanExprContext -> {

                val rangeInText = (ctx.parent as ScalarExprContext).scalarExpr(0).textLocation
                val problemText = ctx.parent.text

                code.appendAsSingleChunk(Instruction.LoadDIdx(problemText, rangeInText))
            }
            is BabelParser.AssignmentContext -> {
                //noop
            }
            else -> TODO("unknown use of var in ${ctx.text}")
        }
    }

    override fun exitNegate(ctx: BabelParser.NegateContext) {
        code.append(Instruction.InvokeUnary(UnaryOps.Inversion))
    }

    override fun exitSum(ctx: BabelParser.SumContext) {
        code.append(Instruction.PushD(0.0))
        code.append(Instruction.InvokeBinary(BinaryOps.Sum))
    }

    override fun exitProd(ctx: BabelParser.ProdContext) {
        code.append(Instruction.PushD(1.0))
        code.append(Instruction.InvokeBinary(BinaryOps.Multiply))
    }

    override fun exitVariable(ctx: BabelParser.VariableContext) {
        code.append(Instruction.LoadD(ctx.text))
    }

    override fun exitLiteral(ctx: BabelParser.LiteralContext) {
        code.append(Instruction.PushD(ctx.value))
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
        = if (this.isFinite()) roundToInt() else null

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

val IntRange.span: Int get() = last - first + 1
val ClosedRange<Double>.span: Double get() = Math.abs(endInclusive - start)
