package com.empowerops.babel

import com.empowerops.babel.BabelLexer.*
import com.empowerops.babel.BabelParser.BooleanExprContext
import com.empowerops.babel.BabelParser.ScalarExprContext
import org.antlr.v4.runtime.*
import org.antlr.v4.runtime.misc.Interval
import org.antlr.v4.runtime.misc.Utils
import org.objectweb.asm.Opcodes
import java.util.*
import java.util.logging.Logger
import kotlin.math.roundToInt

typealias UnaryOp = (Double) -> Double
interface BinaryOp: (Double, Double) -> Double {
    val jbc: ByteCodeDescription
    override operator fun invoke(left: Double, right: Double): Double
}
sealed class ByteCodeDescription {
    data class InvokeStatic(val owner: String, val name: String): ByteCodeDescription()
    data class OnStackInstruction(val opCode: Int): ByteCodeDescription()
}

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
internal typealias Chunk = Array<HighLevelInstruction>

enum class VarScope { LOCAL_VAR, GLOBAL_PARAMETER }

sealed class HighLevelInstruction {
    internal class Custom(val operation: (stack: List<Double>, heap: Map<String, Number>, globals: Map<String, Double>) -> Unit): HighLevelInstruction()

    //control
    // ... -> ... ; metadata
    data class Label(val label: String): HighLevelInstruction()

    // ... left, right -> ... left, right; jumps if left > right
    data class JumpIfGreaterEqual(val label: String): HighLevelInstruction()

    //memory

    // ... valueToBePutInHeap -> S; heap contains keyed value
    data class StoreD(val key: String): HighLevelInstruction()
    data class StoreI(val key: String): HighLevelInstruction()

    // ... -> ... valueFromHeap
    data class LoadD(val key: String, val scope: VarScope): HighLevelInstruction()

    // ... idx -> ... valueFromHeapAtIdx
    data class LoadDIdx(val problemText: String, val rangeInText: IntRange): HighLevelInstruction()

    // ... -> ... value
    data class PushD(val value: Double): HighLevelInstruction()
    data class PushI(val value: Int): HighLevelInstruction()

    // ... erroneous -> ...
    object PopD: HighLevelInstruction()

    // ... a, b -> a, b, b
    // if offset is 0, if it is 1, then it will be a,b,a
    data class Duplicate(val offset: Int): HighLevelInstruction()

    object EnterScope: HighLevelInstruction()
    object ExitScope: HighLevelInstruction()

    //invoke
    // ... left, right -> ... result
    data class InvokeBinary(val op: BinaryOp): HighLevelInstruction()
    // ... input -> ... result
    data class InvokeUnary(val op: UnaryOp): HighLevelInstruction()
    // ... arg1, arg2, arg3 -> ... result
    data class InvokeVariadic(val argCount: Int, val op: VariadicOp): HighLevelInstruction()

    //manipulation

    // ... decimalValue -> ... integerValue
    // converts a double to an integer for use as an index
    data class IndexifyD(val problemText: String, val rangeInText: IntRange) : HighLevelInstruction()
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

    fun flatten(): List<HighLevelInstruction> = instructionChunks.flatMap { it.asIterable() }

    fun append(instruction: HighLevelInstruction){ instructionChunks.push(arrayOf(instruction)) }
    fun appendChunk(instructionChunk: Chunk){ instructionChunks.push(instructionChunk) }
    fun appendAsSingleChunk(instructions: List<HighLevelInstruction>){ instructionChunks.push(instructions.toTypedArray()) }
    fun appendAsSingleChunk(vararg instructions: HighLevelInstruction){ instructionChunks.push(instructions as Chunk) }

    fun popChunk(): Chunk = instructionChunks.pop()
}

internal class CodeGeneratingWalker(val sourceText: String) : BabelParserBaseListener() {

    val code = CodeBuilder()

    override fun exitProgram(ctx: BabelParser.ProgramContext) {
        val instructions = (ctx.statement() + ctx.returnStatement())
            .map { code.popChunk() }
            .asReversed()
            .flatMap { it.asIterable() }

        code.appendAsSingleChunk(instructions)
    }

    override fun exitAssignment(ctx: BabelParser.AssignmentContext) {
        val varName = ctx.name().text
        val valueGenerator = code.popChunk()

        code.appendAsSingleChunk(*valueGenerator, HighLevelInstruction.StoreD(varName))
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

                val instructions = ArrayList<HighLevelInstruction>()

                val accum = "%accum-${code.nextIntLabel()}"
                val loopHeader = "%loop-${code.nextIntLabel()}"

                instructions += upperBoundExpr                                           // ub-double
                instructions += HighLevelInstruction.IndexifyD(problemText, ubRange)     // ub
                instructions += lowerBoundExpr                                           // ub, lb-double
                instructions += HighLevelInstruction.IndexifyD(problemText, lbRange)     // ub, lb (idx)
                instructions += seedProvider                                             // ub, idx, accum
                instructions += HighLevelInstruction.StoreD(accum)                       // ub, idx
                instructions += HighLevelInstruction.Label(loopHeader)                   // ub, idx
                instructions += HighLevelInstruction.LoadD(accum, VarScope.LOCAL_VAR)    // ub, idx, accum
                instructions += lambda                                                   // ub, idx, accum, iterationResult
                instructions += aggregator                                               // ub, idx, accum+
                instructions += HighLevelInstruction.StoreD(accum)                       // ub, idx
                instructions += HighLevelInstruction.PushI(1)                      // ub, idx, 1
                instructions += HighLevelInstruction.InvokeBinary(BinaryOps.Add)         // ub, idx+
                instructions += HighLevelInstruction.JumpIfGreaterEqual(loopHeader)      // ub, idx+
                instructions += HighLevelInstruction.PopD                                // ub
                instructions += HighLevelInstruction.PopD                                //
                instructions += HighLevelInstruction.LoadD(accum, VarScope.LOCAL_VAR)    // accum

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
            HighLevelInstruction.EnterScope,                                     // ub, idx, accum
            if(ctx.value != null) HighLevelInstruction.PushI(ctx.value!!)
                else HighLevelInstruction.Duplicate(offset = 1),                 // ub, idx, accum, idx
            HighLevelInstruction.StoreI(lambdaParamName),                        // ub, idx, accum; idx is now 'i' in heap
            *childExpression,                                           // ub, idx, accum
            HighLevelInstruction.ExitScope                                       // ub, idx, accum
        )
    }

    override fun exitMod(ctx: BabelParser.ModContext) {
        code.append(HighLevelInstruction.InvokeBinary(BinaryOps.Modulo))
    }

    override fun exitPlus(ctx: BabelParser.PlusContext) { code.append(HighLevelInstruction.InvokeBinary(BinaryOps.Add)) }
    override fun exitMinus(ctx: BabelParser.MinusContext) { code.append(HighLevelInstruction.InvokeBinary(BinaryOps.Subtract)) }
    override fun exitMult(ctx: BabelParser.MultContext) { code.append(HighLevelInstruction.InvokeBinary(BinaryOps.Multiply)) }
    override fun exitDiv(ctx: BabelParser.DivContext) { code.append(HighLevelInstruction.InvokeBinary(BinaryOps.Divide)) }

    override fun exitRaise(ctx: BabelParser.RaiseContext) {
        code.append(HighLevelInstruction.InvokeBinary(BinaryOps.Exponentiation))
    }

    override fun exitBinaryFunction(ctx: BabelParser.BinaryFunctionContext) {
        val function = RuntimeNumerics.findBinaryFunctionForType(ctx.start)
        code.append(HighLevelInstruction.InvokeBinary(function))
    }

    override fun exitUnaryFunction(ctx: BabelParser.UnaryFunctionContext) {
        val function = RuntimeNumerics.findUnaryFunction(ctx.start)
        code.append(HighLevelInstruction.InvokeUnary(function))
    }

    override fun exitVariadicFunction(ctx: BabelParser.VariadicFunctionContext) {
        val function = RuntimeNumerics.findVariadicFunctionForType(ctx.start)
        code.append(HighLevelInstruction.InvokeVariadic(ctx.argCount, function))
    }

    override fun exitVar(ctx: BabelParser.VarContext) {
        when(ctx.parent) {
            is BabelParser.ScalarExprContext, is BabelParser.BooleanExprContext -> {

                val rangeInText = (ctx.parent as ScalarExprContext).scalarExpr(0).textLocation
                val problemText = ctx.parent.text

                code.appendAsSingleChunk(HighLevelInstruction.LoadDIdx(problemText, rangeInText))
            }
            is BabelParser.AssignmentContext -> {
                //noop
            }
            else -> TODO("unknown use of var in ${ctx.text}")
        }
    }

    override fun exitNegate(ctx: BabelParser.NegateContext) {
        code.append(HighLevelInstruction.InvokeUnary(UnaryOps.Inversion))
    }

    override fun exitSum(ctx: BabelParser.SumContext) {
        code.append(HighLevelInstruction.PushD(0.0))
        code.append(HighLevelInstruction.InvokeBinary(BinaryOps.Add))
    }

    override fun exitProd(ctx: BabelParser.ProdContext) {
        code.append(HighLevelInstruction.PushD(1.0))
        code.append(HighLevelInstruction.InvokeBinary(BinaryOps.Multiply))
    }

    override fun exitVariable(ctx: BabelParser.VariableContext) {

        val availableLocals = ctx.findDeclaredVariablesFromParents()
        val scope = if(ctx.text in availableLocals) VarScope.LOCAL_VAR else VarScope.GLOBAL_PARAMETER
        code.append(HighLevelInstruction.LoadD(ctx.text, scope))
    }

    override fun exitLiteral(ctx: BabelParser.LiteralContext) {
        code.append(HighLevelInstruction.PushD(ctx.value))
    }

    companion object {
        val Log = Logger.getLogger(CodeGeneratingWalker::class.java.canonicalName)
    }

    // in college this function took me days or weeks to get right
    // i knocked this up in 20 minutes.
    // also, its time complexity isnt aweful,
    // its ~log(n) since its primarily concerned with the path from the reference to the root,
    // though its also linear on the number of statements.
    fun RuleContext.findDeclaredVariablesFromParents(): Set<String> {
        val lineage = generateSequence(this) { it.parent }

        val availableVars = mutableSetOf<String>()

        for((child, node) in lineage.windowed(2)){
            when(node){
                is BabelParser.ProgramContext -> {
                    val statementsBefore = node.statement().takeWhile { it != child }
                    val priorAssignments = statementsBefore.map { it.assignment() }
                    val priorDeclaredVars = priorAssignments.map { it.name().text }

                    availableVars += priorDeclaredVars
                }
                is BabelParser.LambdaExprContext -> {
                    val lambdaVarName = node.name().text
                    availableVars += lambdaVarName
                }
            }
        }

        return availableVars
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
    object LogB: BinaryOp {
        override val jbc: ByteCodeDescription get() = TODO()
        override fun invoke(left: Double, right: Double) = Math.log(right) / Math.log(left)
    }
    object Add: BinaryOp {
        override val jbc = ByteCodeDescription.OnStackInstruction(Opcodes.DADD)
        override fun invoke(left: Double, right: Double) = left + right
    }
    object Subtract: BinaryOp {
        override val jbc = ByteCodeDescription.OnStackInstruction(Opcodes.DSUB)
        override fun invoke(left: Double, right: Double) = left - right
    }
    object Multiply: BinaryOp {
        override val jbc = ByteCodeDescription.OnStackInstruction(Opcodes.DMUL)
        override fun invoke(left: Double, right: Double) = left * right
    }
    object Divide: BinaryOp {
        override val jbc = ByteCodeDescription.OnStackInstruction(Opcodes.DDIV)
        override fun invoke(left: Double, right: Double) = left / right
    }

    object Exponentiation : BinaryOp {
        override val jbc = ByteCodeDescription.InvokeStatic("java/lang/Math", "pow")
        override fun invoke(left: Double, right: Double) = Math.pow(left, right)
    }

    object Modulo : BinaryOp {
        override val jbc: ByteCodeDescription get() = ByteCodeDescription.OnStackInstruction(Opcodes.DREM)
        override fun invoke(left: Double, right: Double) = left % right
    }
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
