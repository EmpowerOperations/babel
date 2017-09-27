package com.empowerops.babel

import org.testng.annotations.Test

typealias MyUnaryOp = (Double) -> Double
object DelegatedOp: MyUnaryOp by Math::cos

class DemoKT16752 {

    @Test(enabled = false)//demonstrates bug KT16752 => expects failure while that bug exists
    fun `when calling delegated unary operations should properly operate`(){

        val delegated = DelegatedOp

        delegated.invoke(Math.PI)
    }
}