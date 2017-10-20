package com.empowerops.babel

import org.assertj.core.api.Assertions
import org.testng.annotations.Test

class BabelCompilerErrorFixture {

    val compiler = BabelCompiler()

    @Test
    fun `when attempting to compile empty string should fail eagerly`(){
        //act
        val result = compileToFailure("")

        //assert
        Assertions.assertThat(result.problems).isEqualTo(setOf(
                BabelExpressionProblem("expression is empty", 1, 1)
        ))
    }

    @Test fun `when serializing illegal expression should get back descriptive error`(){
        //act
        val failure = compileToFailure("x1 + x2 +")

        //assert
        Assertions.assertThat(failure.problems).isEqualTo(setOf(
                BabelExpressionProblem(

                        "mismatched input '<EOF>' expecting {INTEGER, FLOAT, 'cos', 'sin', 'tan', 'atan', 'acos', " +
                                "'asin', 'sinh', 'cosh', 'tanh', 'cot', 'ln', 'log', 'abs', 'sqrt', 'cbrt', 'sqr', " +
                                "'cube', 'ceil', 'floor', 'max', 'min', 'sgn', CONSTANT, 'sum', 'prod', 'var', VARIABLE, " +
                                "'-', '('}",
                        1, 9
                )
        ))
    }

    private fun compileToFailure(expr: String): CompilationFailure = compiler.compile(expr) as CompilationFailure

}
