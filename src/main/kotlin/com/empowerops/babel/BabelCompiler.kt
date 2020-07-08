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
            compile(functionLiteral, *walkers) { it.program() }

    /**
     * Compiles the supplied string into an executable babel expression,
     * or generates a list of errors if no such executable could be created.
     */
    private fun compile(
            sourceText: String,
            vararg walkers: BabelParserListener,
            parseCall: (BabelParser) -> ParseTree
    ): BabelCompilationResult {

        var problems = emptySet<ExpressionProblem>()

        if (sourceText.isEmpty()) {
            problems += ExpressionProblem.EmptyExpression
        }
        else {
            val errorListener = SyntaxErrorCollectingListener(sourceText)
            val parser = setupTokenizerAndParser(sourceText, errorListener)

            Log.fine { "compiling expression '$sourceText'" }
            val currentRoot = parseCall(parser)
            Log.fine { "done parsing expression, got '" + currentRoot.toStringTree(parser) + "'" }

            problems += errorListener.errors

            //could throw a user error, anything we need to do to handle it?
            walkers.forEach { it.walk(currentRoot) }

            val booleanRewritingWalker = BooleanRewritingWalker().apply { walk(currentRoot) }
            val symbolTableBuildingWalker = SymbolTableBuildingWalker().apply { walk(currentRoot) }

            problems += StaticEvaluatorRewritingWalker(sourceText).apply { walk(currentRoot) }.problems
            if (problems.any()) { return CompilationFailure(sourceText, problems) }

            val codeGenerator = CodeGeneratingWalker(sourceText).apply { walk(currentRoot) }

            if(problems.any()) { return CompilationFailure(sourceText, problems) }

            require(problems.isEmpty())

            val highLevelInstructions = codeGenerator.code.flatten()

            val runtime = when {
                System.getProperty(COMPILE_BYTE_CODE_PROPERTY_NAME)?.toLowerCase() == "true" -> {
                    SyntheticJavaClass(Transcoder.transcodeToByteCode(highLevelInstructions))
                }
                else -> {
                    Emulate(highLevelInstructions)
                }
            }

            return BabelExpression(
                    sourceText,
                    containsDynamicLookup = symbolTableBuildingWalker.containsDynamicVarLookup,
                    isBooleanExpression = booleanRewritingWalker.isBooleanExpression,
                    staticallyReferencedSymbols = symbolTableBuildingWalker.staticallyReferencedVariables,
                    runtime = runtime
            )
        }

        return CompilationFailure(sourceText, problems)
    }

    private fun setupTokenizerAndParser(functionLiteral: String, errorListner: ANTLRErrorListener): BabelParser {

        // TODO: direct use of CharStreams.fromString(funcitonLiteral) yields different behaviour...?
        // whats the migration path?
        val antlrStringStream = ANTLRInputStream(functionLiteral)

        val lexer = BabelLexer(antlrStringStream).apply {
            removeErrorListeners()
            addErrorListener(errorListner)
        }
        val tokens = CommonTokenStream(lexer)
        val babelParser = BabelParser(tokens).apply {
            removeErrorListeners()
            addErrorListener(errorListner)
        }
        return babelParser
    }

    companion object {
        private val Log = Logger.getLogger(BabelCompiler::class.java.canonicalName)
        const val COMPILE_BYTE_CODE_PROPERTY_NAME = "com.empowerops.babel.CompileToByteCode"
    }
}