package com.empowerops.babel

import net.bytebuddy.ByteBuddy
import net.bytebuddy.asm.AsmVisitorWrapper
import net.bytebuddy.implementation.*
import net.bytebuddy.implementation.bytecode.ByteCodeAppender
import org.assertj.core.api.Assertions.assertThat
import org.objectweb.asm.ClassWriter
import org.objectweb.asm.Label
import org.objectweb.asm.Opcodes
import org.testng.annotations.Test
import java.io.File
import java.util.*
import kotlin.system.measureTimeMillis

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

    @Test fun `when using byte buddy as codegen should run fast as hell`(){
        var builder = ByteBuddy()
                .subclass(MabelRuntimeExpression::class.java)
                .visit(object: AsmVisitorWrapper by AsmVisitorWrapper.NoOp.INSTANCE {
                    override fun mergeWriter(flags: Int): Int = flags or ClassWriter.COMPUTE_FRAMES
                })

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
//                    mv.visitLocalVariable("this", "Lcom/empowerops/babel/BabelRuntimeExpression\$Generated;", null, L0, L1, 0)
                    mv.visitLocalVariable("globalVars", "Ljava/util/Map;", null, L0, L1, 1)

                    ByteCodeAppender.Size(-1, -1 ) //ignored
                }))

        val made = builder.make().apply {
            saveIn(File("C:/Users/Geoff/Desktop"))
        }

        val expr = made.load(javaClass.classLoader).loaded.getDeclaredConstructor().newInstance()

        //sanity check
        val result = expr.evaluate(mapOf("x1" to 1.0, "x2" to 2.0, "x3" to 3.0))
        assertThat(result).isEqualTo(8.0)

        //performance
        benchmark(expr, listOf("x1", "x2", "x3"), listOf(0.0 .. 20.0, 0.0 .. 20.0, 0.0 .. 20.0), 50, 5_000_000)
    }

    abstract class MabelRuntimeExpression {
        abstract fun evaluate(globals: Map<String, Double>): Double

//        fail ; //TOOD: this is going to work!
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

    private fun benchmark(runtime: MabelRuntimeExpression, vars: List<String>, bounds: List<ClosedRange<Double>>, warmupCount: Int, evalCount: Int) {

        require(vars.size == bounds.size)
        println("running $evalCount iterations of $runtime...")

        //setup
        val random = Random()
        var failedIteration: Int = -1
        val inputGrid = try {
            (0 until evalCount + warmupCount).map {
                try {
                    DoubleArray(vars.size) { random.nextDouble() * bounds[it].span + bounds[it].start }
                }
                catch(err: OutOfMemoryError){
                    failedIteration = it
                    throw err
                }
            }
        }
        catch(ex: OutOfMemoryError){
            System.gc()
            System.err.println("failed on grid allocation $failedIteration")
            throw ex
        }
        //warmup
        for (index in 0 until warmupCount) {
            val row = inputGrid[index]
            val vector = vars.withIndex().associate { (index, sym) -> sym to row[index] }
            runtime.evaluate(vector)
        }

        //act
        val time = measureTimeMillis {
            for (index in warmupCount until (evalCount + warmupCount)) {
                val row = inputGrid[index]
                val vector = vars.withIndex().associate { (index, sym) -> sym to row[index] }
                runtime.evaluate(vector)
            }
        }

        //assert
        println("took ${time}ms for $evalCount evaluations")
    }

}