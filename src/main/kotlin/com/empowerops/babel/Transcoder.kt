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
                .name(DotQualifiedName)

        val bytes = ByteCodeAppender { methodVisitor, implementationContext, instrumentedMethod ->

            val startLabel = Label()
            methodVisitor.visitLabel(startLabel)

            val endLabel = Label()

            var scopeLevel = 1;
            val variableTable = arrayListOf<VariableLifecycle>(
                    VariableLifecycle("this", 0, startLabel, descriptor = "L$SlashQualifiedName;", id = "this",  isParam = true),
                    VariableLifecycle("globals", 0, startLabel, descriptor = "Ljava/util/Map;", id = "globals", isParam = true),
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
                    is HighLevelInstruction.JumpIfGreaterEqual -> {
                        visitInsn(Opcodes.DCMPL)
                        visitLdcInsn(0)
                        visitJumpInsn(Opcodes.IF_ICMPGE, labelsByName.getValue(instruction.label))
                    }
                    is HighLevelInstruction.StoreD -> {
                        val newLabel = Label()
                        val newLifeCycle = VariableLifecycle(instruction.key, scopeLevel, newLabel, DoubleDescriptor)
                        val newVariableId = variableTable.lastIndex + 1
                        variableTable += newLifeCycle
                        visitVarInsn(Opcodes.DSTORE, newVariableId)
                        visitLabel(newLabel)
                    }
                    is HighLevelInstruction.StoreI -> {
                        val newLabel = Label()
                        val newLifeCycle = VariableLifecycle(instruction.key, scopeLevel, newLabel, IntDescriptor)
                        val newVariableId = variableTable.lastIndex + 1
                        variableTable += newLifeCycle
                        visitVarInsn(Opcodes.ISTORE, newVariableId)
                        visitLabel(newLabel)
                    }
                    is HighLevelInstruction.LoadD -> {
                        when(instruction.scope){
                            VarScope.LOCAL_VAR -> {
                                val variable = variableTable.first { it.isAvailable() && it.declaredName == instruction.key }
                                val index = variableTable.indexOf(variable)

                                val loadCode = if(variable.descriptor == IntDescriptor) Opcodes.ILOAD
                                        else if (variable.descriptor == DoubleDescriptor) Opcodes.DLOAD
                                        else TODO("not sure how to load $variable")

                                visitVarInsn(loadCode, index)
                                if(variable.descriptor == IntDescriptor){
                                    visitInsn(Opcodes.I2D)
                                }
                                Unit
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
                        visitVarInsn(Opcodes.ALOAD, VarsIndex)
                        visitInsn(Opcodes.SWAP)
                        visitInsn(Opcodes.DALOAD)
                    }
                    is HighLevelInstruction.PushD -> visitLdcInsn(instruction.value)
                    is HighLevelInstruction.PushI -> visitLdcInsn(instruction.value)
                    HighLevelInstruction.PopD -> visitInsn(Opcodes.POP2)
                    is HighLevelInstruction.Duplicate -> visitInsn(Opcodes.DUP)
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
                            is ByteCodeDescription.Opcodes -> jbc.opCodes.forEach(methodVisitor::visitInsn)
                        }
                    }
                    is HighLevelInstruction.InvokeUnary -> {
                        when(val jbc = instruction.op.jbc){
                            is ByteCodeDescription.InvokeStatic -> {
                                visitMethodInsn(Opcodes.INVOKESTATIC, jbc.owner , jbc.name, "(D)D", false)
                            }
                            is ByteCodeDescription.Opcodes -> jbc.opCodes.forEach(methodVisitor::visitInsn)
                        }
                    }
                    is HighLevelInstruction.InvokeVariadic -> {
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
                    is HighLevelInstruction.IndexifyD -> {
                        visitLdcInsn(-1.0 + 0.5) // -1 to adjust for indexes starting from 1, +0.5 to correct rounding.
                        visitInsn(Opcodes.DADD)
                        visitInsn(Opcodes.D2I)
                    }
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
                .withParameters(Map::class.java, DoubleArray::class.java)
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