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

    @Test fun `when using return should work normally`() = runExprTest(
            "sum(1, 1, x -> var value = 1.0; return 2.0)",
            2.0
    )

    @Test fun `sum 0 to 3 of var value is ith var STOP value * 2`() = runExprTest(
            """sum(1, 3, i ->
              |  var value = var[i];
              |  value * 2
              |)
              """.trimMargin(),
            (1.0*2) + (2.0*2) + (3.0*2),
            "x1" to 1.0,
            "x2" to 2.0,
            "x3" to 3.0,
            containsDynamicLookup = true,
            staticallyReferencedSymbols = emptySet()
    )

    @Test fun `when performing nested expressions should not clobber value`() = runExprTest(
            """sum(1, 1, i ->
              |  var value = 1.0;
              |  var another = prod(1, 1, i ->
              |    var value = 2.0;
              |    value;
              |  );
              |  return 1.0 - value;
              |)
              """.trimMargin(),
            0.0,
            "x1" to 3.0,
            staticallyReferencedSymbols = emptySet()
    )
}