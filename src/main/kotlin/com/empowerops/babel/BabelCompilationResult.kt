package com.empowerops.babel

import kotlinx.collections.immutable.ImmutableMap
import java.io.ObjectStreamException
import java.io.Serializable
import java.lang.RuntimeException
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
): BabelCompilationResult(), Serializable {

    @Transient internal var runtime: RuntimeBabelBuilder.RuntimeConfiguration? = null

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

        return runtime!!.invoke(globalVars)
    }

    companion object {
        private val Log = Logger.getLogger(BabelExpression::class.java.canonicalName)
    }

    // very simple serialization strategy,
    private fun writeReplace(): Any? = SerializedExpression(expressionLiteral)
}

data class SerializedExpression(val expr: String): Serializable {
    private fun readResolve(): Any? = when(val compiledResult = BabelCompiler.compile(expr)){
        is BabelExpression -> compiledResult
        is CompilationFailure -> throw RuntimeException("failed to deserialize '$expr': $compiledResult")
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