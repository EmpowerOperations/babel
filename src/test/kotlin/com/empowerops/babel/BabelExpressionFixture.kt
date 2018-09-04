package com.empowerops.babel

import kotlinx.collections.immutable.toImmutableMap
import org.antlr.v4.runtime.misc.Pair as APair
import org.assertj.core.api.Assertions.*
import org.testng.annotations.Test
import java.lang.Math.*

class BabelExpressionFixture {

    val compiler = BabelCompiler()
    val Epsilon = java.lang.Double.MIN_NORMAL

    //arithmatic
    @Test fun `3 + 4`() = runExprTest("3 + 4", 3.0 + 4.0)
    @Test fun `3 - 4`() = runExprTest("3 - 4", 3.0 - 4.0)
    @Test fun `3 * 4`() = runExprTest("3 * 4", 3.0 * 4.0)
    @Test fun `3 div 4`() = runExprTest("3 / 4", 3.0 / 4.0)
    @Test fun `3 ^ 4`() = runExprTest("3 ^ 4", pow(3.0, 4.0))
    @Test fun `4 % 3`() = runExprTest("4 % 3", 4.0 % 3.0)
    @Test fun `-4 % 3`() = runExprTest("-4 % 3", -4.0 % 3.0)

    //sum and prod
    @Test fun `prod(1, 5, i to 2*i)`() = runExprTest(
            "prod(1, 5, i -> 2*i)",
            (1..5).map { 2.0*it }.fold(1.0){ accum, it -> accum * it }
    )
    @Test fun `identity sum 1 to 5`() = runExprTest(
            "sum(1, 5, i -> i)",
            1.0 + 2.0 + 3.0 + 4.0 + 5.0
    )
    @Test fun `identity prod 1 to 4`() = runExprTest(
            "prod(1, 4, i -> i)",
            1.0 * 2.0 * 3.0 * 4.0
    )
    @Test fun `sum(2, 2, i to var(i-1)`() = runExprTest(
            "sum(2, 2, i -> var[i-1])",
            (2..2).fold(0.0) { accum, it ->/*var[2-1] == var[1] == first-var == */ 2.0 },
            "x1" to 2.0, "x2" to 3.0, "x3" to 4.0,
            containsDynamicLookup = true,
            staticallyReferencedSymbols = emptySet()
    )
    @Test fun `sum(1, 2, i to x1) with extra variable`() = runExprTest(
            "sum(1, 2, i -> x1)",
            1.0 + 1.0,
            "x1" to 1.0, "x2" to 5.0,
            staticallyReferencedSymbols = setOf("x1")
    )
    @Test fun `sum(-5, -2, i to i)`() = runExprTest(
            "sum(-5, -2, i -> i)",
            (-5..-2).sumByDouble { it.toDouble() }
    )

    //lang.Math
    @Test fun `2 * pi`() = runExprTest("2 * pi", 2 * Math.PI)
    @Test fun `ln(20)`() = runExprTest("ln(20)", log(20.0))
    @Test fun `sin(21)^3`() = runExprTest("sin(21)^3", pow(sin(21.0), 3.0))
    @Test fun `abs(-4)`() = runExprTest("abs(-4)", abs(- 4.0))
    @Test fun `ceil(2_7)`() = runExprTest("ceil(2.7)", ceil(2.7))
    @Test fun `floor(2_7)`() = runExprTest("floor(2.7)", floor(2.7))
    @Test fun `log(2, 16)`() = runExprTest("log(2,16)", 4.0) //2^4 == 16
    @Test fun `sgn(-1)`() = runExprTest("sgn(-1)", signum(-1.0))

    //unary minus ambiguity
    @Test fun `"-3 - -3`() = runExprTest("-3 - -3", -3.0 - -3.0)
    @Test fun `"-3--3`() = runExprTest("-3--3", -3.0 - -3.0)

    //variables
    @Test fun `x1 + x2`() = runExprTest("x1 + x2",2.0 + 3.0, "x1" to 2.0, "x2" to 3.0)
    @Test fun `_name`() = runExprTest("_name",4.0, "_name" to 4.0)
    @Test fun `x_1`() = runExprTest("x_1", 4.0, "x_1" to 4.0)
    @Test fun `π`() = runExprTest("π", Math.PI, "π" to Math.PI)
    @Test fun `大_da_dai_meaning_big`() = runExprTest("大_da_dai_meaning_big",1e250, "大_da_dai_meaning_big" to 1e250)
    @Test fun `☕`() = runExprTest("☕", 42.0, "☕" to 42.0)
    @Test fun `测试`() = runExprTest("测试", 42.0, "测试" to 42.0)

    //indexer
    @Test fun `var(1)`() = runExprTest(
            "var[1]",
            0.0,
            "x1" to 0.0,
            containsDynamicLookup = true,
            staticallyReferencedSymbols = emptySet()
    )
    @Test fun `var(2)`() = runExprTest(
            "var[2]",
            2.0,
            "input-sds" to 1.0, "input-SDA" to 2.0, "input-SDJA" to 3.0,
            containsDynamicLookup = true,
            staticallyReferencedSymbols = emptySet()
    )
    @Test fun `var(3)`() = runExprTest(
            "var[3]",
            3.0,
            "input-sds" to 1.0, "input-SDA" to 2.0, "input-SDJA" to 3.0,
            containsDynamicLookup = true,
            staticallyReferencedSymbols = emptySet()
    )
    @Test fun `x + var(2) + z`() = runExprTest(
            "x + var[2] + z",
            1.0 + 1.1 + 1.01,
            "x" to 1.0, "y" to 1.1, "z" to 1.01,
            containsDynamicLookup = true,
            staticallyReferencedSymbols = setOf("x", "z")
    )

    //boolean --remember positive = false, negative = true
    @Test fun `6 gt 6`() = runExprTest("6 > 6", Epsilon, isBooleanExpression = true)
    @Test fun `4 lt 6`() = runExprTest("4 < 6", 4.0 - 6.0, isBooleanExpression = true)

    //epsilon
    @Test fun `1_0e200 lt 1_0e200`() = runExprTest("1.0e200 < 1.0e200", Epsilon, isBooleanExpression = true)
    @Test fun `1_0e200 gt 1_0e200`() = runExprTest("1.0e200 > 1.0e200", Epsilon, isBooleanExpression = true)
    @Test fun `1_0e200 lt 1_0e199`() = runExprTest("1.0e200 < 1.0e199", +9.0e199, isBooleanExpression = true)
    @Test fun `1_0e200 lteq 1_0e200`() = runExprTest("1.0e200 <= 1.0e200", 0.0, isBooleanExpression = true)
    @Test fun `1_0e200 gteq 1_0e200`() = runExprTest("1.0e200 >= 1.0e200", 0.0, isBooleanExpression = true)


    //nesting
    @Test fun `sum(3, 6, i to sum(3, 3, j to j + i))`() = runExprTest("sum(3, 6, i -> sum(3, 3, j -> j + i))",30.0)
    @Test fun `prod(3, 6, i to prod(3, 3, j to j + i))`() = runExprTest("prod(3, 6, i -> prod(3, 3, j -> j + i))",3024.0)

    //name-hiding
    @Test fun `sum(1, 3, x1 to x1) + x1`() = runExprTest(
            "sum(1, 3, x1 -> x1) + x1",
            (1..3).sumByDouble { it.toDouble() } + 1000.0,
            "x1" to 1000.0
    )
    @Test fun `sum(1, 2, i to i) + sum(3, 4, i to i)`() = runExprTest(
            "sum(1, 2, i -> i) + sum(3, 4, i -> i)",
            (1..2).sumByDouble { it.toDouble() } + (3..4).sumByDouble { it.toDouble() }
    )
    @Test fun `sum(1, 3, x to x) + sum(1,3, x to x)`() = runExprTest(
            "sum(1, 3, x -> x) + sum(1,3, x -> x)",
            (1..3).sumByDouble { it.toDouble() } + (1..3).sumByDouble { it.toDouble() }
    )
    @Test fun `prod(1, 2, i to i + sum(1000, 1000, i to i))`() = runExprTest(
            "prod(1, 2, i -> i + sum(1000, 1000, i -> i))",
            1001.0 * 1002.0
    )

    //integration
    @Test fun `rosenbrock 10`() = runExprTest(
            "sum(2, 10, i -> 100*(var[i]-var[i-1]^2)^2 + (1-var[i-1])^2)",
            271194.0,
            "x1" to 2.0, "x2" to 3.0, "x3" to 4.0, "x4" to 5.0, "x5" to 6.0,
            "x6" to 6.0, "x7" to 2.0, "x8" to 3.0, "x9" to 4.0, "x10" to 5.0,
            containsDynamicLookup = true,
            staticallyReferencedSymbols = emptySet()
    )
    @Test fun `rastrigin 10`() = runExprTest(
            "10*10+sum(1, 10, i -> var[i]^2 - 10*cos(2*pi*var[i]))",
            180.0,
            "x1" to 2.0, "x2" to 3.0, "x3" to 4.0, "x4" to -5.0, "x5" to 6.0,
            "x6" to -6.0, "x7" to 2.0, "x8" to 3.0, "x9" to 4.0, "x10" to 5.0,
            containsDynamicLookup = true,
            staticallyReferencedSymbols = emptySet()
    )
    @Test fun `from the manual`() = runExprTest(
            "l1_height * l2_width >= 7.0E2",
            100.0,
            "l1_height" to 30, "l2_width" to 20,
            isBooleanExpression = true
    )

    @Test fun `sum with dynamic bounds`() = runExprTest( "sum(x1 + 0, x1 + 5, i -> var[i])",
            (3..3+5).sumByDouble { listOf(3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0)[it - 1] },
            "x1" to 3.0, "x2" to 4.0, "x3" to 5.0,
            "x4" to 6.0, "x5" to 7.0, "x6" to 8.0,
            "x7" to 9.0, "x8" to 10.0, "offByOne" to 20_000.0,
            containsDynamicLookup = true,
            staticallyReferencedSymbols = setOf("x1")
    )

    @Test fun `large sum with dynamic var access with no whitespace`() = runExprTest(
            "sum(1,50,i->((var[2*i-1]^2-var[2*i])^2+(var[2*i-1]-1)^2))",
            0.0, //not verified, just looking for compiler errors.
            *((1..100).map { "x$it" to 1.0 }.toTypedArray()),
            containsDynamicLookup = true,
            staticallyReferencedSymbols = emptySet()
    )

    @Test fun `simple multi statement`() = runExprTest(
            """var x = x1;
              | x + x1
              """.trimMargin(),
            1.0 + 1.0,
            "x1" to 1.0,
            containsDynamicLookup = false
    )

    @Test fun `simple equality constraint`() = runExprTest(
            "x1 == x2 +/- 0.15",
            //x1 >= x2 - 0.15 && x1 <= x2 + 0.15
            // upper bound is problem bound
            // ==> x1 <= x2 + 0.15
            // ==> x1 - (x2 + 0.15) <= 0
            // sub in values
             1.0 - (0.9 + 0.15),
            "x1" to 1.0, "x2" to 0.9,
            isBooleanExpression = true
    )

    fun runExprTest(expr: String,
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
        val firstResult = compiledExpression.evaluate(inputs, inputs.keys.toList())
        val secondResult = compiledExpression.evaluate(inputs, inputs.keys.toList())

        //assert
        assertThat(firstResult).isEqualTo(expectedResult)
        assertThat(secondResult).describedAs("the result from a second evaluation").isEqualTo(expectedResult)

        assertThat(compiledExpression).isEqualTo(BabelExpression(expr, containsDynamicLookup, isBooleanExpression, staticallyReferencedSymbols))
    }

    private fun BabelCompilationResult.successOrThrow() = when(this){
        is BabelExpression -> this
        is CompilationFailure -> throw RuntimeException("unexpected compiler failure:\n${problems.joinToString("\n")}")
    }
}

