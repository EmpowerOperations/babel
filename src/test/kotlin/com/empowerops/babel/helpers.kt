package com.empowerops.babel

import kotlinx.collections.immutable.toImmutableMap
import org.assertj.core.api.Assertions


fun CompilerProvider.runExprTest(
        expr: String,
        expectedResult: Double,
        vararg inputs: Pair<String, Number>,
        containsDynamicLookup: Boolean = false,
        isBooleanExpression: Boolean = false,
        staticallyReferencedSymbols: Set<String>? = null
){
    //setup
    val inputs = inputs.toMap().mapValues { it.value.toDouble() }.toImmutableMap()
    val staticallyReferencedSymbols = staticallyReferencedSymbols ?: inputs.map { it.key }.toSet()

    //act
    val compiledExpression = compiler.compile(expr).successOrThrow()
    val firstResult = compiledExpression.evaluate(inputs)
    val secondResult = compiledExpression.evaluate(inputs)

    //assert
    Assertions.assertThat(firstResult).isEqualTo(expectedResult)
    Assertions.assertThat(secondResult).describedAs("the result from a second evaluation").isEqualTo(expectedResult)

    Assertions.assertThat(compiledExpression).isEqualTo(BabelExpression(expr, containsDynamicLookup, isBooleanExpression, staticallyReferencedSymbols))
}

fun BabelCompilationResult.successOrThrow() = when(this){
    is BabelExpression -> this
    is CompilationFailure -> {
        val problems = problems.joinToString("\n") { it.makeStaticMessage() }
        throw RuntimeException("unexpected compiler failure:\n$problems")
    }
}