package com.empowerops.babel

import com.thoughtworks.xstream.XStream
import org.assertj.core.api.Assertions.assertThat
import org.testng.annotations.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.ObjectInputStream
import java.io.ObjectOutputStream

class BabelSerializationFixture {

    @Test fun `using java serialization doesnt blow up`(){

        //setup
        val expr = BabelCompiler.compile("x1 + cos(x2)").successOrThrow()

        //act
        val outputBytes = ByteArrayOutputStream()
        val objectOutputStream = ObjectOutputStream(outputBytes)

        objectOutputStream.writeObject(expr)

        val inputBytes = ByteArrayInputStream(outputBytes.toByteArray())
        val inputObjectStream = ObjectInputStream(inputBytes)

        val result = inputObjectStream.readObject()

        //assert
        assertThat(result)
            .isEqualTo(expr) // should be value-equals...
            .isNotSameAs(expr) // but not the same reference
    }

    @Test fun `using third party serializer produces reasonable output and doesnt blow up`(){

        //setup
        val xstream = XStream()
        val expr = BabelCompiler.compile("x1 + cos(x2)").successOrThrow()

        //act
        val serialized = xstream.toXML(expr)
        val deserialized = xstream.fromXML(serialized) as BabelExpression

        //assert
        assertThat(serialized).isEqualTo("""
            <com.empowerops.babel.BabelExpression resolves-to="com.empowerops.babel.SerializedExpression">
              <expr>x1 + cos(x2)</expr>
            </com.empowerops.babel.BabelExpression>
        """.trimIndent())

        assertThat(serialized)
            .isEqualTo(deserialized)
            .isNotSameAs(deserialized)
    }
}