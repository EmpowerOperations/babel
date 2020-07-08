package com.empowerops.babel

import net.bytebuddy.ByteBuddy
import net.bytebuddy.asm.AsmVisitorWrapper
import net.bytebuddy.implementation.Implementation
import net.bytebuddy.implementation.bytecode.ByteCodeAppender
import org.objectweb.asm.ClassWriter
import org.objectweb.asm.Label
import org.objectweb.asm.Opcodes
import java.io.File
import java.util.Map

object Transcoder {

    private const val Name = "com.empowerops.babel.BabelRuntime\$Generated"
    private const val GlobalsIndex = 1
    private const val VarsIndex = 2

    fun transcodeToByteCode(instructions: List<HighLevelInstruction>): ByteCodeBabelRuntime {

        var builder = ByteBuddy()
                .subclass(ByteCodeBabelRuntime::class.java)
                .visit(object: AsmVisitorWrapper by AsmVisitorWrapper.NoOp.INSTANCE {
                    override fun mergeWriter(flags: Int): Int = flags or ClassWriter.COMPUTE_FRAMES
                })
                .name(Name)

        val bytes = ByteCodeAppender { methodVisitor, implementationContext, instrumentedMethod ->

            val startLabel = Label()
            methodVisitor.visitLabel(startLabel)

            val endLabel = Label()

            var scopeLevel = 1;
            val variableTable = arrayListOf<VariableLifecycle>(
                    VariableLifecycle("this", 0, startLabel, descriptor = "L$Name;", id = "this",  isParam = true),
                    VariableLifecycle("globals", 0, startLabel, descriptor = "Ljava/util/Map;", id = "globalVars", isParam = true),
                    VariableLifecycle("vars", 0, startLabel, descriptor = "[D", id = "vars", isParam = true)
                    // if the function signature changes, the parameters must be included here.
            )
            val labelsByName = LinkedHashMap<String, Label>()

            for(instruction in instructions) methodVisitor.apply {
                val x: Any? = when(instruction){
                    is HighLevelInstruction.Custom -> TODO()
                    is HighLevelInstruction.Label -> {
                        val label = Label()
                        labelsByName[instruction.label] = label
                        methodVisitor.visitLabel(label)
                    }
                    is HighLevelInstruction.JumpIfGreaterEqual -> TODO()
                    is HighLevelInstruction.StoreD -> {
                        val newLabel = Label()
                        val newLifeCycle = VariableLifecycle(instruction.key, scopeLevel, newLabel, "D")
                        variableTable += newLifeCycle
                        visitVarInsn(Opcodes.DSTORE, variableTable.indexOf(newLifeCycle))
                        visitLabel(newLabel)
                    }
                    is HighLevelInstruction.StoreI -> {
                        val newLabel = Label()
                        val newLifeCycle = VariableLifecycle(instruction.key, scopeLevel, newLabel, "I")
                        variableTable += newLifeCycle
                        visitVarInsn(Opcodes.DSTORE, variableTable.indexOf(newLifeCycle))
                        visitLabel(newLabel)
                    }
                    is HighLevelInstruction.LoadD -> {
                        when(instruction.scope){
                            VarScope.LOCAL_VAR -> {
                                val index = variableTable.takeWhile { it.endLabel != null || it.declaredName != instruction.key }.size
                                visitVarInsn(Opcodes.DLOAD, index)
                            }
                            VarScope.GLOBAL_PARAMETER -> {
                                visitVarInsn(Opcodes.ALOAD, GlobalsIndex)
                                visitLdcInsn(instruction.key)
                                visitMethodInsn(Opcodes.INVOKEINTERFACE, "java/util/Map", "get", "(Ljava/lang/Object;)Ljava/lang/Object;", true)
                                visitTypeInsn(Opcodes.CHECKCAST, "java/lang/Double")
                                visitMethodInsn(Opcodes.INVOKEVIRTUAL, "java/lang/Double", "doubleValue", "()D", false)
                            }
                        }
                        // TODO: so in the original implementation this 'searches'
                        // local vars and global vars at runtime. Thats silly.
                    }
                    is HighLevelInstruction.LoadDIdx -> {
//                        visitVarInsn(Opcodes.ALOAD, VarsIndex)
//                        visitInsn(Opcodes.SWAP)
//                        visitInsn(Opcodes.DALOAD)
                        visitInsn(Opcodes.POP)
                        visitLdcInsn(42.0)
                    }
                    is HighLevelInstruction.PushD -> visitLdcInsn(instruction.value)
                    is HighLevelInstruction.PushI -> visitLdcInsn(instruction.value)
                    HighLevelInstruction.PopD -> TODO()
                    is HighLevelInstruction.Duplicate -> TODO()
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
                                visitMethodInsn(Opcodes.INVOKESTATIC, jbc.owner , jbc.name, "(DD)D", false)
                            }
                            is ByteCodeDescription.OnStackInstruction -> {
                                visitInsn(jbc.opCode)
                            }
                        }

                    }
                    is HighLevelInstruction.InvokeUnary -> TODO()
                    is HighLevelInstruction.InvokeVariadic -> TODO()
                    is HighLevelInstruction.IndexifyD -> TODO()
                }
            }

            methodVisitor.visitLabel(endLabel)
            methodVisitor.visitInsn(Opcodes.DRETURN)

            for(variable in variableTable){
                methodVisitor.visitLocalVariable(
                        variable.id,
                        variable.descriptor,
                        null,
                        variable.startLabel,
                        variable.endLabel ?: endLabel,
                        variableTable.indexOf(variable)
                )
            }

            ByteCodeAppender.Size(-1, -1)
        }

        builder = builder.defineMethod("evaluate", Double::class.java, Opcodes.ACC_PUBLIC or Opcodes.ACC_FINAL)
                .withParameters(Map::class.java)
                .intercept(Implementation.Simple(bytes))

        val make = builder.make().apply { saveIn(File("C:/Users/Geoff/Desktop")) }
        val loaded = make.load(javaClass.classLoader).loaded
        val instance = loaded.getDeclaredConstructor().newInstance()

        return instance
    }

    data class VariableLifecycle(
            val declaredName: String,
            val scopeLevel : Int,
            val startLabel: Label,
            val descriptor: String,
            var endLabel: Label? = null,
            val id: String = "$declaredName\$${serialID++}",
            val isParam: Boolean = false
    ){
        fun isAvailable(): Boolean = endLabel == null || isParam

        companion object {
            var serialID = 1;
        }
    }
}