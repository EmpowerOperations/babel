package com.empowerops.babel

import kotlinx.collections.immutable.ImmutableMap
import kotlinx.collections.immutable.immutableMapOf
import java.util.*
import java.util.logging.Logger
import kotlin.collections.HashMap
import kotlinx.collections.immutable.plus
import java.lang.RuntimeException
import kotlin.NoSuchElementException
import kotlin.collections.LinkedHashMap

sealed class BabelCompilationResult {
    abstract val expressionLiteral: String
}

data class BabelExpression(
        override val expressionLiteral: String,
        val containsDynamicLookup: Boolean,
        val isBooleanExpression: Boolean,
        val staticallyReferencedSymbols: Set<String>
): BabelCompilationResult() {

    private lateinit var runtime: RunnableCode

    internal constructor(
            expressionLiteral: String,
            containsDynamicLookup: Boolean,
            isBooleanExpression: Boolean,
            staticallyReferencedSymbols: Set<String>,
            runtime: RunnableCode
    ): this(expressionLiteral, containsDynamicLookup, isBooleanExpression, staticallyReferencedSymbols){
        this.runtime = runtime
    }

    @Throws(RuntimeBabelException::class)
    fun evaluate(globalVars: @Ordered Map<String, Double>): Double {

        require(globalVars.isEmpty() || globalVars.javaClass != HashMap::class.java) {
            "babel requires ordered global variables. " +
                    "Consider using a java.util.LinkedHashMap (via mapOf() in kotlin, or constructor in java)" +
                    "kotlinx.collections.immutable.ImmutableOrderedMap (via immutableMapOf() in kotlin)"
        }

        require(globalVars.keys.containsAll(staticallyReferencedSymbols)) {
            "missing value(s) for ${(staticallyReferencedSymbols - globalVars.keys).joinToString()}"
        }

        Log.fine { "babel('$expressionLiteral').evaluate('${globalVars.toMap()}')" }

        val result = when(val runtime = runtime){
            is Emulate -> Emulator.run(expressionLiteral, runtime.instructions, globalVars)
            is SyntheticJavaClass -> runtime.instance.evaluate(globalVars, globalVars.values.toDoubleArray())
        }

        return result
    }

    companion object {
        private val Log = Logger.getLogger(BabelExpression::class.java.canonicalName)
    }
}

internal sealed class RunnableCode
internal data class Emulate(val instructions: List<HighLevelInstruction>): RunnableCode()
internal data class SyntheticJavaClass(val instance: ByteCodeBabelRuntime): RunnableCode()

abstract class ByteCodeBabelRuntime {
    abstract fun evaluate(globals: Map<String, Double>, vars: DoubleArray): Double
}


object Emulator {
    fun run(expressionLiteral: String, instructions: List<HighLevelInstruction>, globalVars: @Ordered Map<String, Double>): Double {
        val globals = globalVars.values.toList()
        var heap = immutableMapOf<String, Number>()
        val stack = Stack().apply { head = -1 }
        val parentScopes: Deque<ImmutableMap<String, Number>> = LinkedList()

        var programCounter = 0

        val labelMap = mutableMapOf<String, Int>()

        while (programCounter < instructions.size) {
            val instruction = instructions[programCounter]

            try {
                @Suppress("IMPLICIT_CAST_TO_ANY") when (instruction) {
                    is HighLevelInstruction.Custom -> instruction.operation(stack.toList(), heap, globalVars)
                    is HighLevelInstruction.JumpIfGreaterEqual -> {
                        if (stack[1].toInt() >= stack[0].toInt()) {
                            programCounter = labelMap[instruction.label] ?: TODO("no such label $instruction")
                        }

                        // note, we continue such that programCounter will be incremented below,
                        // but since a label doesnt need to be evaluated a second time, this is fine.

                        Unit
                    }
                    is HighLevelInstruction.Label -> {
                        labelMap[instruction.label] = programCounter
                    }
                    is HighLevelInstruction.StoreD -> {
                        heap += instruction.key to stack.popDouble()
                    }
                    is HighLevelInstruction.StoreI -> {
                        heap += instruction.key to stack.popInt()
                    }
                    is HighLevelInstruction.EnterScope -> {
                        parentScopes.push(heap)
                    }
                    is HighLevelInstruction.ExitScope -> {
                        heap = parentScopes.pop()
                    }

                    is HighLevelInstruction.LoadD -> {
                        val value = when (instruction.scope) {
                            VarScope.LOCAL_VAR -> heap[instruction.key]
                            VarScope.GLOBAL_PARAMETER -> globalVars[instruction.key]
                        }

                        stack.push(value ?: throw NoSuchElementException("no value for $instruction"))
                    }
                    is HighLevelInstruction.LoadDIdx -> {
                        val i1ndex = stack.popInt()

                        if (i1ndex - 1 !in globals.indices) {
                            val message = "attempted to access 'var[$i1ndex]' " +
                                    "(the ${i1ndex.withOrdinalSuffix()} parameter) " +
                                    "when only ${globals.size} exist"
                            throw makeRuntimeException(
                                    expressionLiteral, heap, globalVars, instruction.problemText,
                                    instruction.rangeInText, message, i1ndex.toDouble()
                            )
                        }
                        val value = globals[i1ndex - 1]

                        stack.push(value)
                    }
                    is HighLevelInstruction.PushD -> {
                        stack.push(instruction.value)
                    }
                    is HighLevelInstruction.PushI -> {
                        stack.push(instruction.value)
                    }
                    is HighLevelInstruction.PopD -> {
                        stack.popDouble()
                    }

                    is HighLevelInstruction.InvokeBinary -> popTwiceExecAndPush(stack, instruction.op)

                    is HighLevelInstruction.InvokeUnary -> {
                        val arg = stack.popDouble()
                        val result = instruction.op.invoke(arg)
                        stack.push(result)
                    }
                    is HighLevelInstruction.InvokeVariadic -> {
                        val input: DoubleArray = stack.popDoubles(instruction.argCount)
                        val result: Double = instruction.op.invoke(input)
                        stack.push(result)
                    }

                    //manipulation
                    is HighLevelInstruction.IndexifyD -> {
                        val indexCandidate = stack.popDouble() //TODO this should simply be an integer
                        val result = indexCandidate.roundToIndex()
                                ?: throw makeRuntimeException(
                                        expressionLiteral, heap, globalVars, instruction.problemText,
                                        instruction.rangeInText, "Illegal bound value", indexCandidate
                                )

                        stack.push(result)
                    }
                    is HighLevelInstruction.Duplicate -> {
                        val number = stack[instruction.offset]
                        stack.push(number)
                    }
                } as Any
            } catch (ex: Exception) {
                if (ex is RuntimeBabelException) throw ex
                else throw RuntimeException("babel runtime error, pc=$programCounter, instruction=$instruction, stack=$stack, heap=$heap", ex)
            }

            programCounter += 1
        }

        val result = stack.pop()

        require(stack.isEmpty()) { "execution incomplete, stack: $stack" }
        return result
    }

    private fun makeRuntimeException(
            expressionLiteral: String,
            heap: Map<String, Number>,
            globals: Map<String, Double>,
            problemText: String,
            rangeInText: IntRange,
            summary: String,
            problemValue: Number
    ): RuntimeBabelException {
        val textBeforeProblem = expressionLiteral.substring(0..rangeInText.start)

        return RuntimeBabelException(RuntimeProblemSource(
                expressionLiteral,
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

    private inline fun popTwiceExecAndPush(stack: Stack, action: (Double, Double) -> Double){
        val right = stack.popDouble()
        val left = stack.popDouble()
        val result = action(left, right)
        stack.push(result)
    }
}

@Suppress("NOTHING_TO_INLINE")
inline class Stack(val data: DoubleArray = DoubleArray(128)){

    inline fun push(newStackTop: Number): Unit { data[++head] = newStackTop.toDouble() }
    inline fun popDoubles(count: Int) = DoubleArray(count) { popDouble() }
    inline fun popDouble(): Double = data[head--]
    inline fun popInt(): Int = data[head--].toInt()
    inline fun pop(): Double = data[head--]
    inline fun isEmpty(): Boolean = head == -1

    inline operator fun get(index: Int): Double = data[head - index]
    inline operator fun set(index: Int, value: Double) { data[head - index] = value }

    fun toList(): List<Double> = data.slice(0 .. head).asReversed()

    var head: Int
        get() = data[data.lastIndex].toInt()
        set(value) { data[data.lastIndex] = value.toDouble() }

    override fun toString() = toList().toString()
}

data class CompilationFailure(
        override val expressionLiteral: String,
        val problems: Set<ExpressionProblem>
): BabelCompilationResult()

/**
 * Indicates that the specified type must preserve order, either insertion order or an explicit sorte order.
 *
 * [ArrayList], [ImmutableMap], [LinkedHashMap] are collections that maintain order
 * [HashSet] and [HashMap] do not.
 */
@Target(AnnotationTarget.TYPE)
annotation class Ordered

