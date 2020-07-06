package com.empowerops.babel

class MyExpression {

    val x1: Double

    constructor(globalVars: @Ordered java.util.Map<String, Double>){
        x1 = globalVars.get("x1")
    }
    fun expression(): Double {
        return 20.0
    }

}