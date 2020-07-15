package com.empowerops.babel

import net.bytebuddy.ByteBuddy
import net.bytebuddy.asm.AsmVisitorWrapper
import net.bytebuddy.implementation.Implementation
import net.bytebuddy.implementation.bytecode.ByteCodeAppender
import org.objectweb.asm.ClassWriter
import org.objectweb.asm.Label
import org.objectweb.asm.Opcodes
import java.io.File
import java.util.concurrent.atomic.AtomicInteger

object Transcoder {

    private final val serial = AtomicInteger(0)

    private const val DotQualifiedName = "com.empowerops.babel.ByteCodeBabelRuntime\$Generated"
    private const val SlashQualifiedName = "com/empowerops/babel/ByteCodeBabelRuntime\$Generated"
    private const val DoubleDescriptor = "D"
    private const val IntDescriptor = "I"
    private const val GlobalsIndex = 1
    private const val VarsIndex = 2

    fun transcodeToByteCode(instructions: List<HighLevelInstruction>): ByteCodeBabelRuntime {

        var builder = ByteBuddy()
                .subclass(ByteCodeBabelRuntime::class.java)
                .visit(object: AsmVisitorWrapper by AsmVisitorWrapper.NoOp.INSTANCE {
                    override fun mergeWriter(flags: Int): Int = flags or ClassWriter.COMPUTE_FRAMES
                })
                .name("$DotQualifiedName\$${serial.getAndIncrement()}")

        val bytes = ByteCodeAppender { methodVisitor, implementationContext, instrumentedMethod ->

            val startLabel = Label()
            methodVisitor.visitLabel(startLabel)

            val endLabel = Label()

            var scopeLevel = 1;
            val variableTable = arrayListOf<VariableLifecycle>(
                    VariableLifecycle("this", 0, startLabel, descriptor = "L$SlashQualifiedName;", isParam = true),
                    VariableLifecycle("globals", 0, startLabel, descriptor = "Ljava/util/Map;", isParam = true),
                    VariableLifecycle("vars", 0, startLabel, descriptor = "[D", isParam = true)
                    // if the function signature changes, the parameters must be included here.
            )

            val labelsByName: Map<String, Label> = instructions
                .filterIsInstance<HighLevelInstruction.Label>()
                .associate { it.label to Label() }

            for(instruction in instructions) {
                val x: Any? = when(instruction){
                    is HighLevelInstruction.Custom -> TODO()
                    is HighLevelInstruction.Label -> {
                        val label = Label()
                        methodVisitor.visitLabel(label)
                    }
                    is HighLevelInstruction.Jump -> {
                        val targetLabel = labelsByName.getValue(instruction.label)
                        methodVisitor.visitJumpInsn(Opcodes.GOTO, targetLabel)
                    }
                    is HighLevelInstruction.JumpIfGreater -> {
                        val targetLabel = labelsByName.getValue(instruction.label)
                        methodVisitor.visitJumpInsn(Opcodes.IF_ICMPGT, targetLabel)
                    }
                    is HighLevelInstruction.StoreD -> {
                        val newLabel = Label()
                        val name = instruction.reference.identifier

                        val existingLifeCycle = variableTable.singleOrNull { it.uniqueName == name }
                        val lifeCycle = existingLifeCycle ?: VariableLifecycle(name, scopeLevel, newLabel, DoubleDescriptor)
                        val newVar = lifeCycle !in variableTable
                        if(newVar) variableTable += lifeCycle

                        val jvmId = variableTable.indexOf(lifeCycle).takeUnless { it == -1 }!!
                        with(methodVisitor) {
                            visitVarInsn(Opcodes.DSTORE, jvmId)
                            if(newVar) visitLabel(newLabel)
                        }
                    }
                    is HighLevelInstruction.StoreI -> {
                        val newLabel = Label()
                        val name = instruction.reference.identifier

                        val existingLifeCycle = variableTable.singleOrNull { it.uniqueName == name }
                        val lifeCycle = existingLifeCycle ?: VariableLifecycle(name, scopeLevel, newLabel, IntDescriptor)
                        val newVar =lifeCycle !in variableTable
                        if(newVar) variableTable += lifeCycle

                        val jvmId = variableTable.indexOf(lifeCycle).takeUnless { it == -1 }!!
                        with(methodVisitor) {
                            visitVarInsn(Opcodes.ISTORE, jvmId)
                            if(newVar) visitLabel(newLabel)
                        }
                    }
                    is HighLevelInstruction.LoadI -> {
                        when(val linkage = instruction.reference){
                            is VariableReference.LinkedLocal -> {
                                val name = instruction.reference.identifier
                                val variable = variableTable.first { it.isAvailable() && it.uniqueName == name }
                                val index = variableTable.indexOf(variable)

                                methodVisitor.visitVarInsn(Opcodes.ILOAD, index)
                            }
                            is VariableReference.GlobalVariable -> with(methodVisitor){
                                visitVarInsn(Opcodes.ALOAD, GlobalsIndex)
                                visitLdcInsn(instruction.reference.identifier)
                                visitMethodInsn(Opcodes.INVOKEINTERFACE, "java/util/Map", "get", "(Ljava/lang/Object;)Ljava/lang/Object;", true)
                                visitTypeInsn(Opcodes.CHECKCAST, "java/lang/Integer")
                                visitMethodInsn(Opcodes.INVOKEVIRTUAL, "java/lang/Integer", "intValue", "()D", false)
                            }
                        }
                    }
                    is HighLevelInstruction.LoadD -> {
                        when(val linkage = instruction.reference){
                            is VariableReference.LinkedLocal -> {
                                val name = instruction.reference.identifier
                                val variable = variableTable.first { it.isAvailable() && it.uniqueName == name }
                                val index = variableTable.indexOf(variable)

                                methodVisitor.visitVarInsn(Opcodes.DLOAD, index)
                            }
                            is VariableReference.GlobalVariable -> with(methodVisitor){
                                visitVarInsn(Opcodes.ALOAD, GlobalsIndex)
                                visitLdcInsn(instruction.reference.identifier)
                                visitMethodInsn(Opcodes.INVOKEINTERFACE, "java/util/Map", "get", "(Ljava/lang/Object;)Ljava/lang/Object;", true)
                                visitTypeInsn(Opcodes.CHECKCAST, "java/lang/Double")
                                visitMethodInsn(Opcodes.INVOKEVIRTUAL, "java/lang/Double", "doubleValue", "()D", false)
                            }
                        }
                    }
                    is HighLevelInstruction.LoadDIdx -> with(methodVisitor){
                        visitVarInsn(Opcodes.ALOAD, VarsIndex)
                        visitInsn(Opcodes.SWAP)
                        visitInsn(Opcodes.DALOAD)
                    }
                    is HighLevelInstruction.PushD -> methodVisitor.visitLdcInsn(instruction.value)
                    is HighLevelInstruction.PushI -> when(instruction.value){
                        0 -> methodVisitor.visitInsn(Opcodes.ICONST_0)
                        1 -> methodVisitor.visitInsn(Opcodes.ICONST_1)
                        else -> methodVisitor.visitLdcInsn(instruction.value)
                    }
                    HighLevelInstruction.PopD -> methodVisitor.visitInsn(Opcodes.POP2)
                    HighLevelInstruction.PopI -> methodVisitor.visitInsn(Opcodes.POP)
                    is HighLevelInstruction.DuplicateI -> methodVisitor.visitInsn(Opcodes.DUP)
                    HighLevelInstruction.DuplicateD -> methodVisitor.visitInsn(Opcodes.DUP2)
                    HighLevelInstruction.EnterScope -> {
                        // the way we implement this here, because of immutability + SSA,
                        // all vars are valid from their declaration to the next end-scope

                        scopeLevel += 1
                    }
                    HighLevelInstruction.ExitScope -> {
                        val endScopeLabel = Label()
                        for(lifeCycle in variableTable.filter { it.endLabel == null && it.scopeLevel == scopeLevel }){
                            lifeCycle.endLabel = endScopeLabel
                        }

                        scopeLevel -= 1
                        methodVisitor.visitLabel(endScopeLabel)
                    }
                    is HighLevelInstruction.InvokeBinary -> {
                        when(val jbc = instruction.op.jbc){
                            is ByteCodeDescription.InvokeStatic -> {
                                methodVisitor.visitMethodInsn(Opcodes.INVOKESTATIC, jbc.owner , jbc.name, "(DD)D", false)
                            }
                            is ByteCodeDescription.Opcodes -> jbc.opCodes.forEach(methodVisitor::visitInsn)
                        }
                    }
                    is HighLevelInstruction.InvokeUnary -> {
                        when(val jbc = instruction.op.jbc){
                            is ByteCodeDescription.InvokeStatic -> {
                                methodVisitor.visitMethodInsn(Opcodes.INVOKESTATIC, jbc.owner , jbc.name, "(D)D", false)
                            }
                            is ByteCodeDescription.Opcodes -> jbc.opCodes.forEach(methodVisitor::visitInsn)
                        }
                    }
                    is HighLevelInstruction.InvokeVariadic -> with(methodVisitor){
                        // problem: our high level machine has infinite stacks. The JVM does not.
                        // with this implementation something like max(arg0, arg1, ... arg100000)
                        // would require a lot of stack.

                        // called with stack =                                 ... arg0, arg1, ... argN
                        visitLdcInsn(instruction.argCount)                  // ... arg0, arg1, ... argN, count
                        visitIntInsn(Opcodes.NEWARRAY, Opcodes.T_DOUBLE)    // ... arg0, arg1, ... argN, arrayRef

                        for(index in instruction.argCount-1 downTo 0){      // ... arg0, arg1, ... argi-1, argi, arrayRef
                            visitLdcInsn(index)                             // ... arg0, arg1, ... argi-1, argi, arrayRef, index
                            visitMethodInsn(Opcodes.INVOKESTATIC, "com/empowerops/babel/ByteCodeBabelRuntime", "DASTORE_ALT", "(D[DI)[D", false)
                            visitInsn(Opcodes.NOP)                          // ... arg0, arg1, ... argi-1, arrayRef
                        }

                        // ..., arrayRef
                        when(val jbc = instruction.op.jbc){
                            is ByteCodeDescription.InvokeStatic -> {
                                visitMethodInsn(Opcodes.INVOKESTATIC, jbc.owner , jbc.name, "([D)D", false)
                            }
                            is ByteCodeDescription.Opcodes -> TODO()
                        }

                        Unit
                    }
                    is HighLevelInstruction.DoublifyIndex -> with(methodVisitor){
                        visitLdcInsn(1) // +1 to move from 0-based to 1-based
                        visitInsn(Opcodes.IADD)
                        visitInsn(Opcodes.I2D)
                    }
                    is HighLevelInstruction.IndexifyDouble -> with(methodVisitor){
                        visitLdcInsn(-1.0 + 0.5) // -1 to adjust for indexes starting from 1, +0.5 to correct rounding.
                        visitInsn(Opcodes.DADD)
                        visitInsn(Opcodes.D2I)
                    }

                    HighLevelInstruction.AddI -> methodVisitor.visitInsn(Opcodes.IADD)
                    HighLevelInstruction.AddD -> methodVisitor.visitInsn(Opcodes.DADD)
                }
            }

            methodVisitor.visitLabel(endLabel)
            methodVisitor.visitInsn(Opcodes.DRETURN)

            for(variable in variableTable){
                methodVisitor.visitLocalVariable(
                        variable.uniqueName,
                        variable.descriptor,
                        null,
                        variable.startLabel,
                        variable.endLabel ?: endLabel,
                        variableTable.indexOf(variable)
                )
            }

//            ByteCodeAppender.Size(20, 10)
            ByteCodeAppender.Size(-1, -1)
        }

        builder = builder.defineMethod("evaluate", Double::class.java, Opcodes.ACC_PUBLIC or Opcodes.ACC_FINAL)
                .withParameters(Map::class.java, DoubleArray::class.java)
                .intercept(Implementation.Simple(bytes))

        val make = builder.make().apply { saveIn(File("C:/Users/Geoff/Desktop")) }
        val loaded = make.load(javaClass.classLoader).loaded
        val instance = loaded.getDeclaredConstructor().newInstance()

        return instance
    }

    data class VariableLifecycle(
            val uniqueName: String,
            val scopeLevel : Int,
            val startLabel: Label,
            val descriptor: String,
            var endLabel: Label? = null,
            val isParam: Boolean = false
    ){
        fun isAvailable(): Boolean = endLabel == null || isParam
    }
}