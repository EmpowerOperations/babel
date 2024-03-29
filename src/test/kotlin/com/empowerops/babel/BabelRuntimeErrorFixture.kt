package com.empowerops.babel

import kotlinx.collections.immutable.persistentMapOf
import org.assertj.core.api.Assertions
import org.assertj.core.api.Assertions.assertThatThrownBy
import org.testng.annotations.Test

class BabelRuntimeErrorFixture {

    val compiler = BabelCompiler

    @Test
    fun `when running an expression that is index out of bounds should properly generate error`(){
        //act
        val expr = compile("sum(1, 3, i -> var[i] + var[x2] + i) + var[x2]")

        //act
        val ex = assertThatThrownBy { expr.evaluate(persistentMapOf("x1" to 3.0, "x2" to 4.0)) }
                .hasMessage("""
                        |Error in 'var[x2]': attempted to access 'var[4]' (the 4th parameter) when only 2 exist.
                        |sum(1, 3, i -> var[i] + var[x2] + i) + var[x2]
                        |                            ~~ evaluates to 4.0
                        |local-variables{i=1.0}
                        |parameters{x1=3.0, x2=4.0}
                        """.trimMargin()
                )
    }

    @Test
    fun `when attempting to run expression without statically known identifiers should fail immediately `(){
        //setup
        val expr = compile("x1 + x2")

        //act & assert
        assertThatThrownBy { expr.evaluate(persistentMapOf("x1" to 3.0)) }
                .isInstanceOf(IllegalArgumentException::class.java)
                .hasMessage("missing value(s) for x2")
    }

    @Test fun `when attempting to run multiline expression should properly format`(){
        //setup
        val expr = compile(
                """sum (
                  |  0,
                  |  20/x1,
                  |  i -> i + 2
                  |)
                  """.trimMargin()
        )

        //act
        val assertThatEvaluation = assertThatThrownBy { expr.evaluate(mapOf("x1" to 0.0)) }

        //assert
        assertThatEvaluation.hasMessage(
                """Error in 'sum(0,20/x1,i->...)': Illegal upper bound value.
                  |sum (
                  |  0,
                  |  20/x1,
                  |  ~~~~~ evaluates to Infinity
                  |  i -> i + 2
                  |)
                  |local-variables{}
                  |parameters{x1=0.0}
                  """.trimMargin()
        )
    }

    @Test
    fun `when lowerbound of sum is NaN should generate nice error message`(){

        //setup
        val expr = compile("sum(0/x1, 20, i -> i + 2)")

        //act
        val exception = assertThrown<RuntimeBabelException> { expr.evaluate(mapOf("x1" to 0.0)) }

        //assert
        Assertions.assertThat(exception.message).isEqualTo("""
                |Error in 'sum(0/x1,20,i->...)': Illegal lower bound value.
                |sum(0/x1, 20, i -> i + 2)
                |    ~~~~ evaluates to NaN
                |local-variables{}
                |parameters{x1=0.0}
                """.trimMargin()
        )
    }

    private inline fun <reified X> assertThrown(noinline function: () -> Any): X where X: Exception {
        val result = try { function() }
        catch(ex: Throwable){
            if(ex is X) return ex
            else throw ex
        }
        TODO("function $function should've thrown ${X::class.simpleName} but instead it returned $result")
    }

    @Test fun `when running with an un-ordered hashmap as globals should eagerly throw`(){
        //setup
        val expr = compile("x1 + x2")

        //act
        val assertThatEvaluation = assertThatThrownBy { expr.evaluate(hashMapOf("x1" to 1.0, "x2" to 2.0)) }

        //assert
        assertThatEvaluation
                .isInstanceOf(IllegalArgumentException::class.java)
                .hasMessageContaining("babel requires ordered global variables")
    }

    private fun compile(expr: String): BabelExpression {
        val result = compiler.compile(expr)

        return when(result){
            is BabelExpression -> result
            is CompilationFailure -> throw RuntimeException("unexpected expression problem: $result")
        }
    }

}