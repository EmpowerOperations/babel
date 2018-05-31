package com.empowerops.babel

import org.testng.annotations.Test

class BabelStatementFixture: CompilerProvider {

    override val compiler = BabelCompiler()

    @Test fun `var first is 12 STOP first * x1`() = runExprTest(
            """var first = 12.0;
              |first * x1
              """.trimMargin(),
            24.0,
            "x1" to 2.0
    )

    @Test fun `sum 0 to 3 of var value is ith var STOP value * 2`() = runExprTest(
            """
                |
                |sum(0, 3, i ->
                |  var value = var[i];
                |  value * 2
                |)
                """.trimMargin(),
            3.0,
            "x1" to 1.0,
            "x2" to 2.0,
            "x3" to 3.0,
            containsDynamicLookup = true
    )
}