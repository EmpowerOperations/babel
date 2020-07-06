package com.empowerops.babel

import kotlinx.collections.immutable.ImmutableMap
import kotlinx.collections.immutable.immutableMapOf
import java.util.*
import java.util.logging.Logger
import kotlin.collections.HashMap
import kotlinx.collections.immutable.plus
import java.lang.RuntimeException
import kotlin.NoSuchElementException

sealed class BabelCompilationResult {
    abstract val expressionLiteral: String
}

data class BabelExpression(
        override val expressionLiteral: String,
        val containsDynamicLookup: Boolean,
        val isBooleanExpression: Boolean,
        val staticallyReferencedSymbols: Set<String>
): BabelCompilationResult() {

    private lateinit var instructions: List<Instruction>

    internal constructor (
            expressionLiteral: String,
            containsDynamicLookup: Boolean,
            isBooleanExpression: Boolean,
            staticallyReferencedSymbols: Set<String>,
            runtime: CodeBuilder
    ): this(expressionLiteral, containsDynamicLookup, isBooleanExpression, staticallyReferencedSymbols){
        this.instructions = runtime.flatten()
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

        val globals = globalVars.values.toList()
        var heap = immutableMapOf<String, Number>()
        val stack = Stack().apply { head = -1 }
        val parentScopes: Deque<ImmutableMap<String, Number>> = LinkedList()

        var programCounter = 0

        val labelMap = mutableMapOf<String, Int>()

        while(programCounter < instructions.size){
            val instruction = instructions[programCounter]

            try {
                @Suppress("IMPLICIT_CAST_TO_ANY") when (instruction) {
                    is Instruction.Custom -> instruction.operation(stack.toList(), heap, globalVars)
                    is Instruction.JumpIfGreaterEqual -> {
                        if (stack[1].toInt() >= stack[0].toInt()) {
                            programCounter = labelMap[instruction.label] ?: TODO("no such label $instruction")
                        }

                        // note, we continue such that programCounter will be incremented below,
                        // but since a label doesnt need to be evaluated a second time, this is fine.

                        Unit
                    }
                    is Instruction.Label -> {
                        labelMap[instruction.label] = programCounter
                    }
                    is Instruction.StoreD -> {
                        heap += instruction.key to stack.popDouble()
                    }
                    is Instruction.StoreI -> {
                        heap += instruction.key to stack.popInt()
                    }
                    is Instruction.EnterScope -> {
                        parentScopes.push(heap)
                    }
                    is Instruction.ExitScope -> {
                        heap = parentScopes.pop()
                    }

                    is Instruction.LoadD -> {
                        val value = heap[instruction.key]
                            ?: globalVars[instruction.key]
                            ?: throw NoSuchElementException(instruction.key)

                        stack.push(value)
                    }
                    is Instruction.LoadDIdx -> {
                        val i1ndex = stack.popInt()

                        if (i1ndex - 1 !in globals.indices) {
                            val message = "attempted to access 'var[$i1ndex]' " +
                                    "(the ${i1ndex.withOrdinalSuffix()} parameter) " +
                                    "when only ${globals.size} exist"
                            throw makeRuntimeException(
                                heap, globalVars, instruction.problemText, instruction.rangeInText,
                                message, i1ndex.toDouble()
                            )
                        }
                        val value = globals[i1ndex - 1]

                        stack.push(value)
                    }
                    is Instruction.PushD -> {
                        stack.push(instruction.value)
                    }
                    is Instruction.PushI -> {
                        stack.push(instruction.value)
                    }
                    is Instruction.PopD -> {
                        stack.popDouble()
                    }

                    is Instruction.InvokeBinary -> popTwiceExecAndPush(stack, instruction.op)

                    is Instruction.InvokeUnary -> {
                        val arg = stack.popDouble()
                        val result = instruction.op.invoke(arg)
                        stack.push(result)
                    }
                    is Instruction.InvokeVariadic -> {
                        val input = stack.popDoubles(instruction.argCount)
                        val result = instruction.op.invoke(input)
                        stack.push(result)
                    }

                    is Instruction.AddI -> popTwiceExecAndPush(stack) { l, r -> l + r }
                    is Instruction.AddD -> popTwiceExecAndPush(stack) { l, r -> l + r }
                    is Instruction.SubtractD -> popTwiceExecAndPush(stack) { l, r -> l - r }
                    is Instruction.MultiplyD -> popTwiceExecAndPush(stack) { l, r -> l * r }
                    is Instruction.DivideD -> popTwiceExecAndPush(stack) { l, r -> l / r }

                    //manipulation
                    is Instruction.IndexifyD -> {
                        val indexCandidate = stack.popDouble() //TODO this should simply be an integer
                        val result = indexCandidate.roundToIndex()
                            ?: throw makeRuntimeException(
                                heap, globalVars, instruction.problemText, instruction.rangeInText,
                                "Illegal bound value", indexCandidate
                            )

                        stack.push(result)
                    }
                    is Instruction.Duplicate -> {
                        val number = stack[instruction.offset]
                        stack.push(number)
                    }
                } as Any
            }
            catch(ex: Exception){
                if(ex is RuntimeBabelException) throw ex
                        else throw RuntimeException("babel runtime error, pc=$programCounter, instruction=$instruction, stack=$stack, heap=$heap", ex)
            }

            programCounter += 1
        }

        val result = stack.pop()

        require(stack.isEmpty()) { "execution incomplete, stack: $stack" }

        return result
    }

    private inline fun popTwiceExecAndPush(stack: Stack, action: (Double, Double) -> Double){
        val right = stack.popDouble()
        val left = stack.popDouble()
        val result = action(left, right)
        stack.push(result)
    }

    companion object {
        private val Log = Logger.getLogger(BabelExpression::class.java.canonicalName)
    }

    private fun makeRuntimeException(
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

