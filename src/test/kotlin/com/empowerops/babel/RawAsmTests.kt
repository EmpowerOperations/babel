package com.empowerops.babel

import net.bytebuddy.ByteBuddy
import net.bytebuddy.description.type.TypeDefinition
import net.bytebuddy.implementation.*
import net.bytebuddy.implementation.bytecode.ByteCodeAppender
import org.assertj.core.api.Assertions.assertThat
import org.objectweb.asm.Label
import org.objectweb.asm.Opcodes
import org.testng.annotations.Test
import java.io.File
import java.lang.reflect.Type

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

        builder = builder
                .defineField("x1", Double::class.java, Opcodes.ACC_PUBLIC)
                .defineField("x2", Double::class.java, Opcodes.ACC_PUBLIC)
                .defineField("x3", Double::class.java, Opcodes.ACC_PUBLIC)

        builder = builder
                .defineMethod("inject", JavaConstants.VoidClazz, Opcodes.ACC_PUBLIC)
                .withParameters(java.util.Map::class.java)
                .intercept(Implementation.Simple(ByteCodeAppender { mv, context, method ->

                    //    L0
                    val L0 = Label()
                    mv.visitLabel(L0)

                    //    ALOAD 0
                    mv.visitVarInsn(Opcodes.ALOAD, 0)

                    //    ALOAD 1
                    mv.visitVarInsn(Opcodes.ALOAD, 1)

                    //    LDC "x1"
                    mv.visitLdcInsn("x1")

                    //    INVOKEINTERFACE java/util/Map.get (Ljava/lang/Object;)Ljava/lang/Object; (itf)
                    mv.visitMethodInsn(Opcodes.INVOKEINTERFACE, "java/util/Map", "get", "(Ljava/lang/Object;)Ljava/lang/Object;", true)

                    //    CHECKCAST java/lang/Double
                    mv.visitTypeInsn(Opcodes.CHECKCAST, "java/lang/Number")

                    //    INVOKEVIRTUAL java/lang/Number.doubleValue ()D
                    mv.visitMethodInsn(Opcodes.INVOKEVIRTUAL, "java/lang/Number", "doubleValue", "()D", false)

                    //    PUTFIELD com/empowerops/babel/MyExpression.x1 : D
                    mv.visitFieldInsn(Opcodes.PUTFIELD, "com/empowerops/babel/BabelRuntimeExpression\$Generated", "x1", "D")

                    //    ... as above, for x2
                    mv.visitVarInsn(Opcodes.ALOAD, 0)
                    mv.visitVarInsn(Opcodes.ALOAD, 1)
                    mv.visitLdcInsn("x2")
                    mv.visitMethodInsn(Opcodes.INVOKEINTERFACE, "java/util/Map", "get", "(Ljava/lang/Object;)Ljava/lang/Object;", true)
                    mv.visitTypeInsn(Opcodes.CHECKCAST, "java/lang/Number")
                    mv.visitMethodInsn(Opcodes.INVOKEVIRTUAL, "java/lang/Number", "doubleValue", "()D", false)
                    mv.visitFieldInsn(Opcodes.PUTFIELD, "com/empowerops/babel/BabelRuntimeExpression\$Generated", "x2", "D")

                    // ... x3
                    mv.visitVarInsn(Opcodes.ALOAD, 0)
                    mv.visitVarInsn(Opcodes.ALOAD, 1)
                    mv.visitLdcInsn("x3")
                    mv.visitMethodInsn(Opcodes.INVOKEINTERFACE, "java/util/Map", "get", "(Ljava/lang/Object;)Ljava/lang/Object;", true)
                    mv.visitTypeInsn(Opcodes.CHECKCAST, "java/lang/Number")
                    mv.visitMethodInsn(Opcodes.INVOKEVIRTUAL, "java/lang/Number", "doubleValue", "()D", false)
                    mv.visitFieldInsn(Opcodes.PUTFIELD, "com/empowerops/babel/BabelRuntimeExpression\$Generated", "x3", "D")

                    //    L1
                    val L1 = Label()
                    mv.visitLabel(L1)

                    //     RETURN
                    mv.visitInsn(Opcodes.RETURN)

                    //    L2
                    val L2 = Label()
                    mv.visitLabel(L2)

                    //    LOCALVARIABLE this Lcom/empowerops/babel/MyExpression; L0 L6 0
                    mv.visitLocalVariable("this", "Lcom/empowerops/babel/BabelRuntimeExpression\$Generated;", null, L0, L2, 0)

                    //    LOCALVARIABLE globalVars Ljava/util/Map; L0 L6 1
                    mv.visitLocalVariable("globalVars", "Ljava/util/Map;", null, L0, L2, 1)

                    ByteCodeAppender.Size(4, 2)
                }))

        builder = builder.defineMethod("evaluate", Double::class.java, Opcodes.ACC_PUBLIC or Opcodes.ACC_FINAL)
                .withParameters(*emptyArray<Type>())
                .intercept(Implementation.Simple(ByteCodeAppender { mv, context, method ->
                    //    L0
                    val L0 = Label()
                    mv.visitLabel(L0)

                    //    LDC 20.0
                    mv.visitLdcInsn(20.0)

                    //    ALOAD 0
                    mv.visitVarInsn(Opcodes.ALOAD, 0)

                    //    GETFIELD com/empowerops/babel/BabelRuntimeExpression$Generated.x3 : D
                    mv.visitFieldInsn(Opcodes.GETFIELD, "com/empowerops/babel/BabelRuntimeExpression\$Generated", "x3", "D")

                    //    LDC 2.0
                    mv.visitLdcInsn(2.0)

                    //    INVOKESTATIC java/lang/Math.pow (DD)D
                    mv.visitMethodInsn(Opcodes.INVOKESTATIC, "java/lang/Math", "pow", "(DD)D", false)

                    //    DSUB
                    mv.visitInsn(Opcodes.DSUB)

                    //    ALOAD 0
                    mv.visitVarInsn(Opcodes.ALOAD, 0)

                    //    GETFIELD com/empowerops/babel/BabelRuntimeExpression$Generated.x1 : D
                    mv.visitFieldInsn(Opcodes.GETFIELD, "com/empowerops/babel/BabelRuntimeExpression\$Generated", "x1", "D")

                    //    ALOAD 0
                    mv.visitVarInsn(Opcodes.ALOAD, 0)

                    //    GETFIELD com/empowerops/babel/BabelRuntimeExpression$Generated.x2 : D
                    mv.visitFieldInsn(Opcodes.GETFIELD, "com/empowerops/babel/BabelRuntimeExpression\$Generated", "x2", "D")

                    //    DADD
                    mv.visitInsn(Opcodes.DADD)

                    //    DSUB
                    mv.visitInsn(Opcodes.DSUB)

                    //    DRETURN
                    mv.visitInsn(Opcodes.DRETURN)

                    //   L1
                    val L1 = Label()
                    mv.visitLabel(L1)

                    //    LOCALVARIABLE this Lcom/empowerops/babel/RawAsmTests; L0 L1 0
                    mv.visitLocalVariable("this", "Lcom/empowerops/babel/BabelRuntimeExpression\$Generated;", null, L0, L1, 0)

                    //    MAXSTACK = 6
                    //    MAXLOCALS = 1
                    ByteCodeAppender.Size(6, 1)
                }))


        val made = builder.make().apply {
            saveIn(File("C:/Users/Geoff/Desktop"))
        }

        val x = made.load(javaClass.classLoader).loaded.getDeclaredConstructor().newInstance()

        x.inject(mapOf("x1" to 1.0, "x2" to 2.0, "x3" to 3.0))
        val result = x.evaluate()

        assertThat(result).isEqualTo(8.0)
    }

    interface BabelRuntimeExpression {
        fun evaluate(): Double
        fun inject(globals: Map<String, Double>)
    }
}
//
//class Appender (val implementationTarget: Implementation.Target) : ByteCodeAppender {
//    private val typeDescription: TypeDescription = implementationTarget.instrumentedType
//
//    override fun apply(methodVisitor: MethodVisitor,
//                       implementationContext: Implementation.Context,
//                       instrumentedMethod: MethodDescription): ByteCodeAppender.Size {
//        val parameterType = instrumentedMethod.parameters[0].type
//        val setterMethod: MethodDescription = methodAccessorFactory.registerSetterFor(fieldDescription, MethodAccessorFactory.AccessType.DEFAULT)
//        val stackSize = StackManipulation.Compound(
//                StackManipulation.Compound(
//                        MethodVariableAccess.loadThis(),
//                        FieldAccess.forField(typeDescription.declaredFields.filter(ElementMatchers.named("instance")).only).read()
//                ),
//                MethodVariableAccess.of(parameterType).loadFrom(1),
//                assigner.assign(parameterType, setterMethod.parameters[0].type, Assigner.Typing.DYNAMIC),
//                MethodInvocation.invoke(setterMethod),
//                MethodReturn.VOID
//        ).apply(methodVisitor, implementationContext)
//        return ByteCodeAppender.Size(stackSize.maximalSize, instrumentedMethod.stackSize)
//    }
//}

