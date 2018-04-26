package com.empowerops.babel

import org.antlr.v4.runtime.ANTLRErrorListener
import org.antlr.v4.runtime.ANTLRInputStream
import org.antlr.v4.runtime.CommonTokenStream
import org.antlr.v4.runtime.tree.ParseTree
import java.util.logging.Logger
import javax.inject.Inject

class BabelCompiler @Inject constructor(){

    fun isLegalVariableName(variableName: String): Boolean {
        val result = compile(variableName) { it.variable_only() }
        return result is BabelExpression
    }

    fun compile(functionLiteral: String, vararg walkers: BabelParserListener): BabelCompilationResult =
            compile(functionLiteral, *walkers) { it.expression() }

    /**
     * Compiles the supplied string into an executable babel expression,
     * or generates a list of errors if no such executable could be created.
     */
    private fun compile(
            sourceText: String,
            vararg walkers: BabelParserListener,
            parseCall: (BabelParser) -> ParseTree
    ): BabelCompilationResult {

        var problems = emptySet<BabelExpressionProblem>()

        if (sourceText.isEmpty()) {
            problems += BabelExpressionProblem("expression is empty", 1, 1)
        }
        else {
            val errorListener = ErrorCollectingListener()
            val parser = setupTokenizerAndParser(sourceText, errorListener)

            Log.fine { "compiling expression '$sourceText'" }
            val currentRoot = parseCall(parser)
            Log.fine { "done parsing expression, got '" + currentRoot.toStringTree(parser) + "'" }

            problems += errorListener.errors

            val syntaxErrorFinder = SyntaxErrorReportingWalker().apply { walk(currentRoot) }
            problems += syntaxErrorFinder.problems

            if (problems.any()) { return CompilationFailure(sourceText, problems) }

            //could throw a user error, anything we need to do to handle it?
            walkers.forEach { it.walk(currentRoot) }

            val booleanRewritingWalker = BooleanRewritingWalker().apply { walk(currentRoot) }
            val symbolTableBuildingWalker = SymbolTableBuildingWalker().apply { walk(currentRoot) }

            StaticEvaluatorRewritingWalker().apply { walk(currentRoot) }
            
            val codeGenerator = CodeGeneratingWalker(sourceText).apply { walk(currentRoot) }

            problems += SyntaxErrorReportingWalker().apply { walk(currentRoot) }.problems

            if(problems.any()) { return CompilationFailure(sourceText, problems) }

            require(problems.isEmpty())

            return BabelExpression(
                    sourceText,
                    containsDynamicLookup = symbolTableBuildingWalker.containsDynamicVarLookup,
                    isBooleanExpression = booleanRewritingWalker.isBooleanExpression,
                    staticallyReferencedSymbols = symbolTableBuildingWalker.staticallyReferencedVariables,
                    runtime = codeGenerator.instructions.configuration
            )
        }

        return CompilationFailure(sourceText, problems)
    }

    private fun setupTokenizerAndParser(functionLiteral: String, errorListner: ANTLRErrorListener): BabelParser {
        val antlrStringStream = ANTLRInputStream(functionLiteral)
        val lexer = BabelLexer(antlrStringStream)
        lexer.removeErrorListeners()
        val tokens = CommonTokenStream(lexer)
        val babelParser = BabelParser(tokens).apply {
            removeErrorListeners()
            addErrorListener(errorListner)
        }
        return babelParser
    }

    fun convertToLegalVariableName(variableName: String) : String {
        val builder = StringBuilder()
        variableName.mapIndexed { index: Int, c: Char -> if (index == 0) verifyFirstChar(c) else verifyOtherChar(c) }.forEach { builder.append(it) }
        return builder.toString()
    }

    private fun verifyFirstChar(c: Char) : Char {
        return if (isLegalVariableName(c+"x")) c else '_'
    }

    private fun verifyOtherChar(c: Char) : Char {
        return if (isLegalVariableName("x"+c)) c else '_'
    }

    companion object {
        private val Log = Logger.getLogger(BabelCompiler::class.java.canonicalName)
    }
}