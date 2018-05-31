package com.empowerops.babel

import org.assertj.core.api.Assertions
import org.testng.annotations.Test

class VariableNameFixture {

    val compiler = BabelCompiler()

    @Test fun `name x1`() = runNameTest("x1")
    @Test fun `name 3`() = runNameTest("3", legal = false)

    @Test fun `name DOLLARx1`(): Unit = runNameTest("\$x1", legal = false)
    @Test fun `name ATx`(): Unit = runNameTest("@x", legal = false)
    @Test fun `name BACKSLASHx`(): Unit = runNameTest("\\x", legal = false)
    @Test fun `name xDOLLAR`() = runNameTest("x\$", legal = false)

    private fun runNameTest(name: String, legal: Boolean = true){
        //act
        val nameIsLegal = compiler.isLegalVariableName(name)

        //assert
        Assertions.assertThat(nameIsLegal)
                .describedAs("'$name' is ${if(legal)"a legal" else "an illegal"} variable name")
                .isEqualTo(legal)
    }
}