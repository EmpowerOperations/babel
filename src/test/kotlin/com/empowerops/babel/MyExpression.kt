package com.empowerops.babel

import java.util.*

class MyExpression {

    fun expression(globalVars: @Ordered java.util.Map<String, Double>): Double {
        return (20.0 - Math.pow(globalVars.get("x3"), 2.0)) - (globalVars.get("x1") + globalVars.get("x2"))
    }


    fun expression2(globalVars: @Ordered java.util.Map<String, Double>): Double {
        val x = 22.3
        return x + 1.1;
    }


    fun expression3(globalVars: @Ordered java.util.Map<String, Double>): Double {
        var index = 0
        var accum = 32.0
        while(index + 1 < 3){
            accum = accum + 3.0
            index = index + 1
        }
        return accum
    }

    fun expression4(globalVars: @Ordered java.util.Map<String, Double>): Double {
        val r = doubleArrayOf(1.0, 2.0, 3.0)
        return 4.5
    }
}