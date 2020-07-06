package com.empowerops.babel

import net.bytebuddy.ByteBuddy
import net.bytebuddy.description.method.MethodDescription
import net.bytebuddy.description.type.TypeDescription
import net.bytebuddy.implementation.*
import net.bytebuddy.implementation.bind.annotation.FieldProxy
import net.bytebuddy.implementation.bytecode.ByteCodeAppender
import net.bytebuddy.implementation.bytecode.StackManipulation
import net.bytebuddy.implementation.bytecode.assign.Assigner
import net.bytebuddy.implementation.bytecode.member.FieldAccess
import net.bytebuddy.implementation.bytecode.member.MethodInvocation
import net.bytebuddy.implementation.bytecode.member.MethodReturn
import net.bytebuddy.implementation.bytecode.member.MethodVariableAccess
import net.bytebuddy.matcher.ElementMatchers
import org.assertj.core.api.Assertions.assertThat
import org.objectweb.asm.MethodVisitor
import org.objectweb.asm.Opcodes
import org.testng.annotations.Test
import java.io.File
import java.lang.reflect.Type

class RawAsmTests {

    @Test fun todo(){
//        val mn = MethodNode()
//        val instructions = mn.instructions
//        instructions.add(VarInsnNode())
        
    }

    val x1 = 3.0
    val x2 = 4.0
    val x3 = 2.0

    @Test fun expression(): Double {
        return (20.0 - Math.pow(x3, 2.0)) - (x1 + x2)
    }

    @Test fun asdf(){
        var builder = ByteBuddy()
                .subclass(BabelRuntimeExpression::class.java)
                .name("com.empowerops.babel.BabelRuntimeExpression\$Generated")

        builder = builder
                .defineField("x1", Double::class.java, Opcodes.ACC_PUBLIC)
                .defineField("x3", Double::class.java, Opcodes.ACC_PUBLIC)
                .defineField("x2", Double::class.java, Opcodes.ACC_PUBLIC)

        val bytes = ByteCodeAppender { methodVisitor, context, method ->
            //.intercept(Implementation.Simple(bytes)) //with code
            //    L0
            //    LDC 20.0
            //    ALOAD 0
            //    GETFIELD com/empowerops/babel/BabelRuntimeExpression$Generated.x3 : D
            //    LDC 2.0
            //    INVOKESTATIC java/lang/Math.pow (DD)D
            //    DSUB
            //    ALOAD 0
            //    GETFIELD com/empowerops/babel/BabelRuntimeExpression$Generated.x1 : D
            //    ALOAD 0
            //    GETFIELD com/empowerops/babel/BabelRuntimeExpression$Generated.x2 : D
            //    DADD
            //    DSUB
            //    DRETURN
            //   L1
            //    LOCALVARIABLE this Lcom/empowerops/babel/RawAsmTests; L0 L1 0
            //    MAXSTACK = 6
            //    MAXLOCALS = 1
            TODO()
        }

        builder = builder.defineConstructor()
                .withParameters(*emptyArray<Type>())
                .intercept(
                        MethodCall.invoke(Any::class.java.getConstructor())
                                .andThen(FieldAccessor.ofField("x1").setsValue(6.0))
                )

        builder = builder.defineMethod("evaluate", Double::class.java, Opcodes.ACC_PUBLIC or Opcodes.ACC_FINAL)
                .withParameters(*emptyArray<Type>())
                .intercept(FixedValue.value(4.0))


        val made = builder.make().apply {
            saveIn(File("C:/Users/Geoff/Desktop"))
        }

        val x = made.load(javaClass.classLoader).loaded.getDeclaredConstructor().newInstance()

        val result = x.evaluate()

        assertThat(result).isEqualTo(5.0)
    }

    interface BabelRuntimeExpression {
        fun evaluate(): Double
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