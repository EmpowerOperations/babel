package com.empowerops.babel

data class BabelExpressionProblem(val description: String, val line: Int, val character: Int) {
    val sourcedDescription = "at line $line char $character: $description"
    override fun toString() = sourcedDescription
}

class RuntimeBabelException(message: String) : RuntimeException(message)