package com.empowerops.babel

class MyExpression {

    fun expression(globalVars: @Ordered java.util.Map<String, Double>): Double {
        return (20.0 - Math.pow(globalVars.get("x3"), 2.0)) - (globalVars.get("x1") + globalVars.get("x2"))
    }

}