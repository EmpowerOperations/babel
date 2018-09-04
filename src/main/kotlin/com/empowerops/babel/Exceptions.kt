package com.empowerops.babel

import kotlinx.collections.immutable.ImmutableMap

data class ExpressionProblem(
        val sourceText: String,
        val abbreviatedProblemText: String,
        val rangeInText: IntRange,
        val lineNo: Int, //one based
        val characterNo: Int, //zero based
        val summary: String,
        val problemValueDescription: String
){
    companion object {

        val EmptyExpression = ExpressionProblem(
                sourceText = "",
                abbreviatedProblemText = "",
                rangeInText = 0..0,
                lineNo = 1,
                characterNo = 1,
                summary = "Expression is empty",
                problemValueDescription = ""
        )
    }

    val message: String by lazy { when(this){
        EmptyExpression -> summary
        else -> makeStaticMessage()
    }}
}

data class RuntimeProblemSource(
        val sourceText: String,
        val abbreviatedProblemText: String,
        val rangeInText: IntRange,
        val lineNo: Int,
        val characterNo: Int,
        val summary: String,
        val problemValueDescription: String,
        val heap: ImmutableMap<String, Double>,
        val globals: Map<String, Double>,
        val references: @Ordered List<String>
)

class RuntimeBabelException(
        val runtimeProblemSource: RuntimeProblemSource
): RuntimeException(runtimeProblemSource.makeExceptionMessage())

internal fun RuntimeProblemSource.makeExceptionMessage(): String {

    val markedUpSource = markUp(characterNo, rangeInText.span, problemValueDescription, sourceText, lineNo)

    return """Error in '$abbreviatedProblemText': $summary.
      |$markedUpSource
      |local-variables$heap
      |parameters$globals
      |available reference$references
      """.trimMargin()
}
internal fun ExpressionProblem.makeStaticMessage(): String {

    val markedUpSource = markUp(characterNo, rangeInText.span, problemValueDescription, sourceText, lineNo)

    return """Error in '$abbreviatedProblemText': $summary.
      |$markedUpSource
      """.trimMargin()
}

private fun markUp(characterNo: Int, problemTextLength: Int, problemValueDescription: String, sourceText: String, lineNo: Int): String {
    val errorLine = " ".repeat(characterNo) + "~".repeat(problemTextLength) + " $problemValueDescription"
    val markedUpSource = sourceText.lines().toMutableList()
            .apply { add(lineNo, errorLine) }
            .joinToString("\n")
    return markedUpSource
}