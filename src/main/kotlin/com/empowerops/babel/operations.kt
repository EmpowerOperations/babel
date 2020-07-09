package com.empowerops.babel

import org.antlr.v4.runtime.Token
import org.objectweb.asm.Opcodes


interface UnaryOp: (Double) -> Double {
    val jbc: ByteCodeDescription
    override fun invoke(arg: Double): Double
}
interface BinaryOp: (Double, Double) -> Double {
    val jbc: ByteCodeDescription
    override operator fun invoke(left: Double, right: Double): Double
}
sealed class ByteCodeDescription {
    data class InvokeStatic(val owner: String, val name: String): ByteCodeDescription()
    data class Opcodes(val opCodes: List<Int>): ByteCodeDescription(){
        constructor(vararg opCodes: Int): this(opCodes.toList())
    }
}

typealias VariadicOp = (DoubleArray) -> Double

/**
 * Class to adapt simple algebra within java into an executable & babel-consumable form.
 *
 * Strategy here is to first consult a couple maps looking for the value or operation,
 * then dump the problem on Java.lang.Math to look up functions defined in the grammar but not
 * in the custom maps.
 */
internal object RuntimeNumerics {

    fun findValueForConstant(constant: Token): Number = when(constant.text){
        "e" -> Math.E
        "pi" -> Math.PI
        else -> TODO("unknown math constant ${constant.text}")
    }

    fun findUnaryFunction(function: Token): UnaryOp = when(function.type) {

        BabelLexer.COS -> UnaryOps.Cos
        BabelLexer.SIN -> UnaryOps.Sin
        BabelLexer.TAN -> UnaryOps.Tan
        BabelLexer.ATAN -> UnaryOps.Atan
        BabelLexer.ACOS -> UnaryOps.Acos
        BabelLexer.ASIN -> UnaryOps.Asin
        BabelLexer.SINH -> UnaryOps.Sinh
        BabelLexer.COSH -> UnaryOps.Cosh
        BabelLexer.TANH -> UnaryOps.Tanh
        BabelLexer.COT -> UnaryOps.Cot
        BabelLexer.LN -> UnaryOps.Ln
        BabelLexer.LOG -> UnaryOps.Log
        BabelLexer.ABS -> UnaryOps.Abs
        BabelLexer.SQRT -> UnaryOps.Sqrt
        BabelLexer.CBRT -> UnaryOps.Cbrt
        BabelLexer.SQR -> UnaryOps.Sqr
        BabelLexer.CUBE -> UnaryOps.Cube
        BabelLexer.CIEL -> UnaryOps.Ceil
        BabelLexer.FLOOR -> UnaryOps.Floor
        BabelLexer.SGN -> UnaryOps.Sgn

        else -> TODO("unknown unary operation ${function.text}")
    }

    fun findBinaryFunctionForType(token: Token): BinaryOp = when(token.type){
        BabelLexer.LOG -> BinaryOps.LogB

        else -> TODO("unknown binary operation ${token.text}")
    }

    fun findVariadicFunctionForType(token: Token): VariadicOp = when(token.type) {
        BabelLexer.MAX -> VariadicOps.Max
        BabelLexer.MIN -> VariadicOps.Min

        else -> TODO("unknown variadic operation ${token.text}")
    }
}

internal object UnaryOps {

    // note that the funny code here is primarily for debugability,
    // we don't use objects for any eager-ness or etc reason, a `val cos = Math::cos`
    // would more-or-less work just fine,
    // but it would be more difficult to debug.
    // as written, under the debugger, things should look pretty straight-forward.

    object Cos: UnaryOp {
        override val jbc = ByteCodeDescription.InvokeStatic("java/lang/Math", "cos")
        override fun invoke(arg: Double) = Math.cos(arg)
    }
    object Sin: UnaryOp {
        override val jbc = ByteCodeDescription.InvokeStatic("java/lang/Math", "sin")
        override fun invoke(arg: Double) = Math.sin(arg)
    }
    object Tan: UnaryOp {
        override val jbc = ByteCodeDescription.InvokeStatic("java/lang/Math", "tan")
        override fun invoke(arg: Double) = Math.tan(arg)
    }
    object Atan: UnaryOp {
        override val jbc = ByteCodeDescription.InvokeStatic("java/lang/Math", "atan")
        override fun invoke(arg: Double) = Math.atan(arg)
    }
    object Acos: UnaryOp {
        override val jbc = ByteCodeDescription.InvokeStatic("java/lang/Math", "acos")
        override fun invoke(arg: Double) = Math.acos(arg)
    }
    object Asin: UnaryOp {
        override val jbc = ByteCodeDescription.InvokeStatic("java/lang/Math", "asin")
        override fun invoke(arg: Double) = Math.asin(arg)
    }
    object Sinh: UnaryOp {
        override val jbc = ByteCodeDescription.InvokeStatic("java/lang/Math", "sinh")
        override fun invoke(arg: Double) = Math.sinh(arg)
    }
    object Cosh: UnaryOp {
        override val jbc = ByteCodeDescription.InvokeStatic("java/lang/Math", "cosh")
        override fun invoke(arg: Double) = Math.cosh(arg)
    }
    object Tanh: UnaryOp {
        override val jbc = ByteCodeDescription.InvokeStatic("java/lang/Math", "tanh")
        override fun invoke(arg: Double) = Math.tanh(arg)
    }
    object Cot: UnaryOp {
        override val jbc = ByteCodeDescription.InvokeStatic("java/lang/Math", "tan")
        override fun invoke(arg: Double) = 1 / Math.tan(arg)
    }
    //dont like that 'log' is the natural logarithm and 'log10' is log base 10 in java terms
    //so we'll switch it here
    object Ln: UnaryOp {
        override val jbc = ByteCodeDescription.InvokeStatic("java/lang/Math", "log")
        override fun invoke(arg: Double) = Math.log(arg)
    }
    object Log: UnaryOp {
        override val jbc = ByteCodeDescription.InvokeStatic("java/lang/Math", "log10")
        override fun invoke(arg: Double) = Math.log10(arg)
    }
    object Abs: UnaryOp {
        override val jbc = ByteCodeDescription.InvokeStatic("java/lang/Math", "abs")
        override fun invoke(arg: Double) = Math.abs(arg)
    }
    object Sqrt: UnaryOp {
        override val jbc = ByteCodeDescription.InvokeStatic("java/lang/Math", "sqrt")
        override fun invoke(arg: Double) = Math.sqrt(arg)
    }
    object Cbrt: UnaryOp {
        override val jbc = ByteCodeDescription.InvokeStatic("java/lang/Math", "cbrt")
        override fun invoke(arg: Double) = Math.cbrt(arg)
    }
    object Sqr: UnaryOp {
        override val jbc = ByteCodeDescription.Opcodes(Opcodes.DUP, Opcodes.DMUL)
        override fun invoke(arg: Double) = arg * arg
    }
    object Cube: UnaryOp {
        override val jbc = ByteCodeDescription.Opcodes(Opcodes.DUP, Opcodes.DUP, Opcodes.DMUL, Opcodes.DMUL)
        override fun invoke(arg: Double) = arg * arg * arg
    }
    object Ceil: UnaryOp {
        override val jbc = ByteCodeDescription.InvokeStatic("java/lang/Math", "ceil")
        override fun invoke(arg: Double) = Math.ceil(arg)
    }
    object Floor: UnaryOp {
        override val jbc = ByteCodeDescription.InvokeStatic("java/lang/Math", "floor")
        override fun invoke(arg: Double) = Math.floor(arg)
    }
    object Inversion: UnaryOp {
        override val jbc = ByteCodeDescription.Opcodes(Opcodes.DNEG)
        override fun invoke(arg: Double) = -arg
    }
    object Sgn: UnaryOp {
        override val jbc = ByteCodeDescription.InvokeStatic("java/lang/Math", "signum")
        override fun invoke(arg: Double) = Math.signum(arg)
    }
}

object BinaryOps {
    object LogB: BinaryOp {
        override val jbc: ByteCodeDescription get() = TODO()
        override fun invoke(left: Double, right: Double) = Math.log(right) / Math.log(left)
    }
    object Add: BinaryOp {
        override val jbc = ByteCodeDescription.Opcodes(Opcodes.DADD)
        override fun invoke(left: Double, right: Double) = left + right
    }
    object Subtract: BinaryOp {
        override val jbc = ByteCodeDescription.Opcodes(Opcodes.DSUB)
        override fun invoke(left: Double, right: Double) = left - right
    }
    object Multiply: BinaryOp {
        override val jbc = ByteCodeDescription.Opcodes(Opcodes.DMUL)
        override fun invoke(left: Double, right: Double) = left * right
    }
    object Divide: BinaryOp {
        override val jbc = ByteCodeDescription.Opcodes(Opcodes.DDIV)
        override fun invoke(left: Double, right: Double) = left / right
    }

    object Exponentiation : BinaryOp {
        override val jbc = ByteCodeDescription.InvokeStatic("java/lang/Math", "pow")
        override fun invoke(left: Double, right: Double) = Math.pow(left, right)
    }

    object Modulo : BinaryOp {
        override val jbc: ByteCodeDescription get() = ByteCodeDescription.Opcodes(Opcodes.DREM)
        override fun invoke(left: Double, right: Double) = left % right
    }
}

object VariadicOps {
    object Max: VariadicOp { override fun invoke(input: DoubleArray) = input.max()!! }
    object Min: VariadicOp { override fun invoke(input: DoubleArray) = input.min()!! }
}
