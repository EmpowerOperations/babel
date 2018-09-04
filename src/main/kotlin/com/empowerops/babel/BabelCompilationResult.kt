package com.empowerops.babel

import kotlinx.collections.immutable.ImmutableMap
import java.util.ArrayList
import java.util.HashSet
import java.util.LinkedHashMap
import java.util.logging.Logger

sealed class BabelCompilationResult {
    abstract val expressionLiteral: String
}

data class BabelExpression(
        override val expressionLiteral: String,
        val containsDynamicLookup: Boolean,
        val isBooleanExpression: Boolean,
        val staticallyReferencedSymbols: Set<String>
): BabelCompilationResult() {

    private lateinit var runtime: RuntimeBabelBuilder.RuntimeConfiguration

    internal constructor (
            expressionLiteral: String,
            containsDynamicLookup: Boolean,
            isBooleanExpression: Boolean,
            staticallyReferencedSymbols: Set<String>,
            runtime: RuntimeBabelBuilder.RuntimeConfiguration
    ): this(expressionLiteral, containsDynamicLookup, isBooleanExpression, staticallyReferencedSymbols){
        this.runtime = runtime
    }

    @Throws(RuntimeBabelException::class)
    fun evaluate(globalVars: Map<String, Double>, references: @Ordered List<String>): Double {

        require(globalVars.isEmpty() || globalVars.javaClass != HashMap::class.java) {
            "babel requires ordered global variables. " +
                    "Consider using a java.util.LinkedHashMap (via mapOf() in kotlin, or constructor in java)" +
                    "kotlinx.collections.immutable.ImmutableOrderedMap (via immutableMapOf() in kotlin)"
        }

        require(globalVars.keys.containsAll(staticallyReferencedSymbols)) {
            "missing value(s) for ${(staticallyReferencedSymbols - globalVars.keys).joinToString()}"
        }

        Log.fine { "babel('$expressionLiteral').evaluate('${globalVars.toMap()}')" }

        return runtime.invoke(globalVars, references)
    }

    companion object {
        private val Log = Logger.getLogger(BabelExpression::class.java.canonicalName)
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