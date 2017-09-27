package com.empowerops.babel

import org.assertj.core.api.Assertions
import org.testng.annotations.Test

class VariableNameFixture {

    val compiler = BabelCompiler()

    @Test fun `name x1`() = runNameTest("x1")
    @Test fun `name 3`() = runNameTest("3", legal = false)

    private fun runNameTest(name: String, legal: Boolean = true){
        //act
        val nameIsLegal = compiler.isLegalVariableName(name)

        //assert
        Assertions.assertThat(nameIsLegal)
                .describedAs("$name is a ${if(legal)"legal" else "illegal"} variable name")
                .isEqualTo(legal)
    }
}