package com.empowerops.babel

import com.empowerops.babel.BabelParser.BooleanExprContext
import com.empowerops.babel.BabelParser.ScalarExprContext
import org.antlr.v4.runtime.*
import org.antlr.v4.runtime.misc.Interval
import org.antlr.v4.runtime.misc.Utils
import java.util.*
import java.util.logging.Logger
import kotlin.math.roundToInt


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

sealed class HighLevelInstruction {
    internal class Custom(val operation: (stack: List<Double>, heap: Map<String, Number>, globals: Map<String, Double>) -> Unit): HighLevelInstruction()

    //control
    // ... -> ... ; metadata
    data class Label(val label: String): HighLevelInstruction()

    // ... left, right -> ...; jumps if left > right
    data class JumpIfGreater(val label: String): HighLevelInstruction()
    data class Jump(val label: String): HighLevelInstruction()

    //memory

    // ... valueToBePutInHeap -> S; heap contains keyed value
    data class StoreD(val reference: VariableReference): HighLevelInstruction()
    data class StoreI(val reference: VariableReference): HighLevelInstruction()

    // ... -> ... valueFromHeap
    data class LoadD(val reference: VariableReference): HighLevelInstruction()
    data class LoadI(val reference: VariableReference): HighLevelInstruction()

    // ... idx -> ... valueFromHeapAtIdx
    data class LoadDIdx(val problemText: String, val rangeInText: IntRange): HighLevelInstruction()

    // ... -> ... value
    data class PushD(val value: Double): HighLevelInstruction()
    data class PushI(val value: Int): HighLevelInstruction()

    // ... erroneous -> ...
    object PopD: HighLevelInstruction()
    object PopI: HighLevelInstruction()

    // ... a, b -> a, b, b
    object DuplicateI: HighLevelInstruction()

    // ... a, b -> a, b, b
    object DuplicateD: HighLevelInstruction()

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

    // ... decmial-1.0-based -> ... index-0-based
    // converts a double to an integer for use as an index
    data class IndexifyDouble(val problemText: String, val rangeInText: IntRange) : HighLevelInstruction()
    // ... index-0-based -> ... decimal-1.0-based
    object DoublifyIndex : HighLevelInstruction()

    object AddI: HighLevelInstruction()
    object AddD: HighLevelInstruction()
}

sealed class ExprConstness {
    object AHEAD_OF_TIME: ExprConstness()
    object RUNTIME : ExprConstness()
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
        val instructions = (ctx.assignment() + ctx.returnStatement())
            .map { code.popChunk() }
            .asReversed()
            .flatMap { it.asIterable() }

        code.appendAsSingleChunk(instructions)
    }

    override fun exitAssignment(ctx: BabelParser.AssignmentContext) {
        val varName = ctx.name().text
        val valueGenerator = code.popChunk()

        code.appendAsSingleChunk(*valueGenerator, HighLevelInstruction.StoreD(ctx.name().linkage))
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

                val loopHeader = "loopHeader%${code.nextIntLabel()}"
                val loopIdx = "loopIdx%${code.nextIntLabel()}"
                val loopUpperBound = "loopUB%${code.nextIntLabel()}"
                val accum = "loopAccum%${code.nextIntLabel()}"
                val loopExit = "loopExit%${code.nextIntLabel()}"

                val accumLink = BabelParserTranslations.newLinkage(accum)
                val idxLink = BabelParserTranslations.newLinkage(loopIdx)
                val ubLink = BabelParserTranslations.newLinkage(loopUpperBound)

                //setup accumulator
                instructions += seedProvider
                instructions += HighLevelInstruction.StoreD(accumLink)

                //setup lower bound var
                instructions += lowerBoundExpr
                instructions += HighLevelInstruction.IndexifyDouble(problemText, lbRange)
                instructions += HighLevelInstruction.StoreI(idxLink)

                //setup upper bound var
                instructions += upperBoundExpr
                instructions += HighLevelInstruction.IndexifyDouble(problemText, ubRange)
                instructions += HighLevelInstruction.StoreI(ubLink)

                //loop start
                instructions += HighLevelInstruction.Label(loopHeader)
                instructions += HighLevelInstruction.LoadI(idxLink)                 // ... idx
                instructions += HighLevelInstruction.LoadI(ubLink)                  // ... idx, ub
                instructions += HighLevelInstruction.JumpIfGreater(loopExit)        // ...

                instructions += HighLevelInstruction.LoadI(idxLink)                 // ... idx
                instructions += HighLevelInstruction.DoublifyIndex                  // ... i1ndex
                instructions += lambda                                              // ... iterationResult
                instructions += HighLevelInstruction.LoadD(accumLink)               // ... iterationResult, accum
                instructions += aggregator                                          // ... accum*
                instructions += HighLevelInstruction.StoreD(accumLink)              // ...

                instructions += HighLevelInstruction.LoadI(idxLink)                 // ... idx
                instructions += HighLevelInstruction.PushI(1)                       // ... idx, 1
                instructions += HighLevelInstruction.AddI                           // ... idx*
                instructions += HighLevelInstruction.StoreI(idxLink)                // ...

                instructions += HighLevelInstruction.Jump(loopHeader)               //

                //loop footer
                instructions += HighLevelInstruction.Label(loopExit)
                instructions += HighLevelInstruction.LoadD(accumLink)               // ... accum

                // heres some code generated for loops:
                //        var index = 0
                //        var accum = 32.0
                //        while(index + 1 < 3){
                //            accum = accum + 3.0
                //            index = index + 1
                //        }
                //        return accum
                //    }

                //   L0
                //    ALOAD 1
                //    LDC "globalVars"
                //    INVOKESTATIC kotlin/jvm/internal/Intrinsics.checkParameterIsNotNull (Ljava/lang/Object;Ljava/lang/String;)V
                //   L1
                //    LINENUMBER 19 L1
                //    ICONST_0
                //    ISTORE 2
                //   L2
                //    LINENUMBER 20 L2
                //    LDC 32.0
                //    DSTORE 3
                //   L3
                //    LINENUMBER 21 L3
                //   L4
                //    ILOAD 2                <--- conditional block, loads the body of expr in while(expr)
                //    ICONST_1
                //    IADD
                //    ICONST_3
                //    IF_ICMPGE L5
                //   L6
                //    LINENUMBER 22 L6      <--- begin loop body
                //    DLOAD 3
                //    LDC 3.0
                //    DADD
                //    DSTORE 3
                //   L7
                //    LINENUMBER 23 L7
                //    ILOAD 2
                //    ICONST_1
                //    IADD
                //    ISTORE 2
                //   L8
                //    LINENUMBER 21 L8
                //    GOTO L4                <--- loop bottom, unconditional jump to header block
                //   L5
                //    LINENUMBER 25 L5
                //    DLOAD 3
                //    DRETURN
                //   L9
                //    LOCALVARIABLE accum D L3 L9 3
                //    LOCALVARIABLE index I L2 L9 2
                //    LOCALVARIABLE this Lcom/empowerops/babel/MyExpression; L0 L9 0
                //    LOCALVARIABLE globalVars Ljava/util/Map; L0 L9 1
                //    MAXSTACK = 4
                //    MAXLOCALS = 5


                code.appendAsSingleChunk(instructions)
            }
            ctx.lambdaExpr()?.availability is ExprConstness.AHEAD_OF_TIME -> {
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
        val lambdaLinkage = ctx.name().linkage

        val childExpressions = (ctx.assignment() + ctx.returnStatement())
                .map { code.popChunk() }
                .asReversed()
                .flatMap { it.asIterable() }

        code.appendAsSingleChunk(                                       // ... i1ndex-decimal <-- from loop
            // from loop
            HighLevelInstruction.EnterScope,                            // ... i1ndex
            HighLevelInstruction.StoreD(lambdaLinkage),                 // ...
            *childExpressions.toTypedArray(),                           // ... lambdaResult
            HighLevelInstruction.ExitScope                              // ... lambdaResult
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

                code.appendAsSingleChunk(
                        HighLevelInstruction.IndexifyDouble(problemText, rangeInText),
                        HighLevelInstruction.LoadDIdx(problemText, rangeInText)
                )
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
        code.append(HighLevelInstruction.LoadD(ctx.linkage))
    }

    override fun exitLiteral(ctx: BabelParser.LiteralContext) {
        code.append(HighLevelInstruction.PushD(ctx.value))
    }

    companion object {
        val Log = Logger.getLogger(CodeGeneratingWalker::class.java.canonicalName)
    }
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

    fun Token.value(): Double = (this as? ValueToken)?.value?.toDouble() ?: text.toDouble()

    val value: Double = when {
        CONSTANT() != null -> RuntimeNumerics.findValueForConstant(CONSTANT().symbol)
        INTEGER() != null -> INTEGER().symbol.value()
        FLOAT() != null -> FLOAT().symbol.value()

        else -> TODO("unknown literal type for $text")
    }.toDouble()

    return value
}

val IntRange.span: Int get() = last - first + 1
val ClosedRange<Double>.span: Double get() = Math.abs(endInclusive - start)

object BabelParserTranslations {

    var lastID = 1;

    @JvmStatic fun findDeclarationSite(ctx: BabelParser.VariableContext): VariableReference {
        val locallyDeclared = ctx.findDeclaredVariablesFromParents().firstOrNull { it.text == ctx.text }
        return locallyDeclared?.linkage ?: VariableReference.GlobalVariable(ctx.text)
    }

    @JvmStatic fun newLinkage(name: String) = VariableReference.LinkedLocal(name, lastID++)
}

sealed class VariableReference {
    abstract val identifier: String
    data class GlobalVariable(val name: String): VariableReference() {
        override val identifier get() = name
    }
    data class LinkedLocal(val name: String, val uid: Int): VariableReference() {
        override val identifier: String get() = "$name%$uid"
    }
}

// in college this function took me days or weeks to get right
// i knocked this up in 20 minutes.
// also, its time complexity isnt aweful,
// its ~log(n) since its primarily concerned with the path from the reference to the root,
// though its also linear on the number of statements.
fun RuleContext.findDeclaredVariablesFromParents(): Set<BabelParser.NameContext> {
    val lineage = generateSequence(this) { it.parent }

    val availableVars = mutableSetOf<BabelParser.NameContext>()

    for((child, node) in lineage.windowed(2)){
        when(node){
            is BabelParser.ProgramContext -> {
                val priorAssignments = node.assignment().takeWhile { it != child }
                val priorDeclaredVars = priorAssignments.map { it.name() }

                availableVars += priorDeclaredVars
            }
            is BabelParser.SyntheticblockContext -> {
                val priorAssignments = node.assignment().takeWhile { it != child }
                val priorDeclaredVars = priorAssignments.map { it.name() }

                availableVars += priorDeclaredVars
            }
            is BabelParser.LambdaExprContext -> {
                val lambdaVarName = node.name()
                val priorAssignments = node.assignment().takeWhile { it != child }
                val priorDeclaredVars = priorAssignments.map { it.name() }

                availableVars += priorDeclaredVars + lambdaVarName
            }
        }
    }

    return availableVars
}