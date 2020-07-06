package com.empowerops.babel

import net.bytebuddy.ByteBuddy
import net.bytebuddy.implementation.*
import net.bytebuddy.implementation.bytecode.ByteCodeAppender
import org.assertj.core.api.Assertions.assertThat
import org.objectweb.asm.Label
import org.objectweb.asm.Opcodes
import org.testng.annotations.Test
import java.io.File

class RawAsmTests {

    @Test fun `when evaluting in kotlin`(){
        // x1 + x2 > 20 - x3^2
        val x1 = 1.0
        val x2 = 2.0
        val x3 = 3.0

        //act
        val result = (20 - Math.pow(x3, 2.0)) - (x1 + x2)

        //assert
        assertThat(result).isEqualTo(8.0)
    }

    @Test fun asdf(){
        var builder = ByteBuddy()
                .subclass(BabelRuntimeExpression::class.java)
                .name("com.empowerops.babel.BabelRuntimeExpression\$Generated")

        builder = builder.defineMethod("evaluate", Double::class.java, Opcodes.ACC_PUBLIC or Opcodes.ACC_FINAL)
                .withParameters(java.util.Map::class.java)
                .intercept(Implementation.Simple(ByteCodeAppender { mv, context, method ->

                    //    L0
                    val L0 = Label()
                    mv.visitLabel(L0)

                    mv.visitLdcInsn(20.0)
                    // ... 20.0

                    mv.visitVarInsn(Opcodes.ALOAD, 1)
                    mv.visitLdcInsn("x3")
                    mv.visitMethodInsn(Opcodes.INVOKEINTERFACE, "java/util/Map", "get", "(Ljava/lang/Object;)Ljava/lang/Object;", true)
                    mv.visitTypeInsn(Opcodes.CHECKCAST, "java/lang/Double")
                    mv.visitMethodInsn(Opcodes.INVOKEVIRTUAL, "java/lang/Double", "doubleValue", "()D", false)
                    // ... 20.0, x3

                    mv.visitLdcInsn(2.0)
                    // ... 20.0, x3, 2.0

                    //    INVOKESTATIC java/lang/Math.pow (DD)D
                    mv.visitMethodInsn(Opcodes.INVOKESTATIC, "java/lang/Math", "pow", "(DD)D", false)
                    // ... 20.0, x3^2

                    //    DSUB
                    mv.visitInsn(Opcodes.DSUB)
                    // ... (20-x3^2)

                    // x1
                    mv.visitVarInsn(Opcodes.ALOAD, 1)
                    mv.visitLdcInsn("x1")
                    mv.visitMethodInsn(Opcodes.INVOKEINTERFACE, "java/util/Map", "get", "(Ljava/lang/Object;)Ljava/lang/Object;", true)
                    mv.visitTypeInsn(Opcodes.CHECKCAST, "java/lang/Double")
                    mv.visitMethodInsn(Opcodes.INVOKEVIRTUAL, "java/lang/Double", "doubleValue", "()D", false)
                    // ... (20-x3^2), x1

                    // x2
                    mv.visitVarInsn(Opcodes.ALOAD, 1)
                    mv.visitLdcInsn("x2")
                    mv.visitMethodInsn(Opcodes.INVOKEINTERFACE, "java/util/Map", "get", "(Ljava/lang/Object;)Ljava/lang/Object;", true)
                    mv.visitTypeInsn(Opcodes.CHECKCAST, "java/lang/Double")
                    mv.visitMethodInsn(Opcodes.INVOKEVIRTUAL, "java/lang/Double", "doubleValue", "()D", false)
                    // ... (20-x3^2), x1, x2

                    //    DADD
                    mv.visitInsn(Opcodes.DADD)
                    // ... (20-x3^2), (x1+x2)

                    //    DSUB
                    mv.visitInsn(Opcodes.DSUB)
                    // ... (20-x3^2) - (x1+x2)

                    //    DRETURN
                    mv.visitInsn(Opcodes.DRETURN)

                    //   L1
                    val L1 = Label()
                    mv.visitLabel(L1)

                    //    LOCALVARIABLE this Lcom/empowerops/babel/RawAsmTests; L0 L1 0
                    mv.visitLocalVariable("this", "Lcom/empowerops/babel/BabelRuntimeExpression\$Generated;", null, L0, L1, 0)
                    mv.visitLocalVariable("globalVars", "Ljava/util/Map;", null, L0, L1, 1)

                    //    MAXSTACK = 7
                    //    MAXLOCALS = 2
                    ByteCodeAppender.Size(7, 2)
                }))


        val made = builder.make().apply {
            saveIn(File("C:/Users/Geoff/Desktop"))
        }

        val x = made.load(javaClass.classLoader).loaded.getDeclaredConstructor().newInstance()

        val result = x.evaluate(mapOf("x1" to 1.0, "x2" to 2.0, "x3" to 3.0))

        assertThat(result).isEqualTo(8.0)
    }

    abstract class BabelRuntimeExpression {
        abstract fun evaluate(globals: Map<String, Double>): Double

        fail ; //TOOD: this is going to work!
        // 1. extract private (internal?) methods here for helpers,
        //    eg for the heap lookup
//                    val value = heap[instruction.key]
//                            ?: globalVars[instruction.key]
//                            ?: throw NoSuchElementException(instruction.key)
        //    this is wayyy easier to do in a private fun than in your own code generator.
        // 2. you need to look into invoke virtual
        // 3. any way you can keep custom?
        // 4. also benchmark it!
    }
}