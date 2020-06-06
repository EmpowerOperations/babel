package com.empowerops.babel

import kotlinx.collections.immutable.ImmutableMap
import java.util.*
import java.util.logging.Logger
import kotlin.collections.HashMap
import kotlinx.collections.immutable.plus
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

        val scope = RuntimeMemory(globalVars)

        var programCounter = 0
        while(programCounter < instructions.size){
            val instruction = instructions[programCounter]

            @Suppress("IMPLICIT_CAST_TO_ANY") when(instruction){
                is Instruction.Custom -> instruction.operation(scope)
                is Instruction.JumpIfGreaterEqual -> {
                    if(scope.stack[1].toInt() >= scope.stack[0].toInt()){
                        programCounter = instructions.indexOf(Instruction.Label(instruction.label))
                                .takeUnless { it == -1 } ?: TODO("no such label $instruction")
                    }

                    Unit
                }
                is Instruction.Label -> Unit //noop
                is Instruction.StoreD -> { scope.heap += instruction.key to scope.stack.popDouble() }
                is Instruction.StoreI -> { scope.heap += instruction.key to scope.stack.popInt() }
                is Instruction.LoadD -> {
                    val value = scope.heap[instruction.key] ?: scope.globals[instruction.key]
                            ?: throw NoSuchElementException(instruction.key)
                    scope.stack.push(value)
                }
                is Instruction.LoadDIdx -> {
                    val i1ndex = scope.stack.popInt()
                    val globals = scope.globals.values.toList()

                    if (i1ndex-1 !in globals.indices) {
                        val message = "attempted to access 'var[$i1ndex]' " +
                                "(the ${i1ndex.withOrdinalSuffix()} parameter) " +
                                "when only ${globals.size} exist"
                        throw makeRuntimeException(scope, instruction.problemText, instruction.rangeInText, message, i1ndex.toDouble())
                    }
                    val value = globals[i1ndex-1]

                    scope.stack.push(value)
                }
                is Instruction.PushD -> { scope.stack.push(instruction.value) }
                is Instruction.PushI -> { scope.stack.push(instruction.value) }
                is Instruction.PopD -> { scope.stack.popDouble() }
                is Instruction.EnterScope -> { scope.parentScopes.push(scope.heap) }
                is Instruction.ExitScope -> { scope.heap = scope.parentScopes.pop() }


                is Instruction.InvokeBinary -> {
                    val right = scope.stack.popDouble()
                    val left = scope.stack.popDouble()
                    val result = instruction.op.invoke(left, right)
                    scope.stack.push(result)
                }
                is Instruction.InvokeUnary -> {
                    val arg = scope.stack.popDouble()
                    val result = instruction.op.invoke(arg)
                    scope.stack.push(result)
                }
                is Instruction.InvokeVariadic -> {
                    val input = scope.stack.popDoubles(instruction.argCount)
                    val result = instruction.op.invoke(input)
                    scope.stack.push(result)
                }
                is Instruction.AddD -> {
                    val right = scope.stack.popDouble()
                    val left = scope.stack.popDouble()
                    val result = left + right
                    scope.stack.push(result)
                }
                is Instruction.SubtractD -> {
                    val right = scope.stack.popDouble()
                    val left = scope.stack.popDouble()
                    val result = left - right
                    scope.stack.push(result)
                }
                is Instruction.MultiplyD -> {
                    val right = scope.stack.popDouble()
                    val left = scope.stack.popDouble()
                    val result = left * right
                    scope.stack.push(result)
                }
                is Instruction.DivideD -> {
                    val right = scope.stack.popDouble()
                    val left = scope.stack.popDouble()
                    val result = left / right
                    scope.stack.push(result)
                }

                //manipulation
                is Instruction.IndexifyD -> {
                    val indexCandidate = scope.stack.popDouble() //TODO this should simply be an integer
                    val result = indexCandidate.roundToIndex()
                            ?: throw makeRuntimeException(scope, instruction.problemText, instruction.rangeInText, "Illegal bound value", indexCandidate)

                    scope.stack.push(result)
                }
                is Instruction.Duplicate -> {
                    val number = scope.stack[instruction.offset]
                    scope.stack.push(number)
                }
            } as Any

            programCounter += 1
        }

        val result = scope.stack.pop().toDouble()

        require(scope.stack.isEmpty()) { "execution incomplete, stack: ${scope.stack}" }

        return result
    }

    companion object {
        private val Log = Logger.getLogger(BabelExpression::class.java.canonicalName)
    }

    private fun makeRuntimeException(
            scope: RuntimeMemory,
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
                scope.heap,
                scope.globals
        ))
    }
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