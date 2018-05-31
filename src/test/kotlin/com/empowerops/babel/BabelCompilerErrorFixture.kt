package com.empowerops.babel

import org.assertj.core.api.Assertions.*
import org.testng.annotations.Test

class BabelCompilerErrorFixture {

    val compiler = BabelCompiler()

    @Test
    fun `when attempting to compile empty string should fail eagerly`(){
        //act
        val result = compileToFailure("")

        //assert
        assertThat(result.problems).isEqualTo(setOf(ExpressionProblem.EmptyExpression))
    }
                                                
    @Test fun `when serializing illegal expression should get back descriptive error`(){
        //act
        val failure = compileToFailure("x1 + x2 +")

        //assert
        assertThat(failure.problems).isEqualTo(setOf(
                ExpressionProblem(
                        "x1 + x2 +",
                        "end of expression",
                        8..8,
                        1,
                        9,
                        "syntax error",
                        "unexpected symbol"
                )
        ))
        assertThat(failure.problems.single().message).isEqualTo("" +
                "Error in 'end of expression': syntax error.\n" +
                "x1 + x2 +\n" +
                "         ~ unexpected symbol"
        )
    }


    @Test
    fun `when lowerbound of sum is NaN should generate nice error message`(){

        //act
        val failure = compileToFailure("sum(0/0, 20, i -> i + 2)")

        //assert
        assertThat(failure.problems).isEqualTo(setOf(ExpressionProblem(
                sourceText = "sum(0/0, 20, i -> i + 2)",
                abbreviatedProblemText = "sum(0/0,20,i->...)",
                rangeInText = 4 .. 6,
                lineNo = 1,
                characterNo = 4,
                summary = "Illegal lower bound value",
                problemValueDescription = "evaluates to NaN"
        )))
        assertThat(failure.problems.single().makeStaticMessage()).isEqualTo(
                """Error in 'sum(0/0,20,i->...)': Illegal lower bound value.
                  |sum(0/0, 20, i -> i + 2)
                  |    ~~~ evaluates to NaN
                  """.trimMargin()
        )
    }

    @Test fun `when using equals without bound should fail`(){
        val failure = compileToFailure("x1 = x2")

        assertThat(failure.problems).isNotEmpty()
    }
    @Test fun `when using equals with complex bound should fail`(){
        val failure = compileToFailure("x1 = x2 +/- x3")

        assertThat(failure.problems).isNotEmpty()
    }


    @Test fun `when attempting to compile expression with nested boolean clause should be rejected`(){
        val result = compileToFailure("1+(x > 3) + 2")

        assertThat(result.problems).isEqualTo(setOf(ExpressionProblem(
                sourceText = "1+(x > 3) + 2",
                abbreviatedProblemText = ">",
                rangeInText = 5..5,
                lineNo = 1,
                characterNo = 5,
                summary = "syntax error",
                problemValueDescription = "unexpected symbol"
        )))
        assertThat(result.problems.single().makeStaticMessage()).isEqualTo("""
                |Error in '>': syntax error.
                |1+(x > 3) + 2
                |     ~ unexpected symbol
                """.trimMargin()
        )
    }

    @Test fun `when compiling expression with garbage should get nice message`(){
        val result = compileToFailure("1 + @x1")

        assertThat(result.problems).isEqualTo(setOf(ExpressionProblem(
                sourceText = "1 + @x1",
                abbreviatedProblemText = "@",
                rangeInText = 4..4,
                lineNo = 1,
                characterNo = 4,
                summary = "syntax error",
                problemValueDescription = "illegal character"
        )))
        assertThat(result.problems.single().makeStaticMessage()).isEqualTo("""
                |Error in '@': syntax error.
                |1 + @x1
                |    ~ illegal character
                """.trimMargin()
        )
    }

    @Test fun `when embedding boolean expression in sum should get nice message`(){
        TODO()
    }

    private fun compileToFailure(expr: String): CompilationFailure = compiler.compile(expr) as CompilationFailure

}
