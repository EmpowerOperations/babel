package com.empowerops.babel

import org.antlr.v4.runtime.ANTLRErrorListener
import org.antlr.v4.runtime.ANTLRInputStream
import org.antlr.v4.runtime.CharStreams
import org.antlr.v4.runtime.CommonTokenStream
import org.antlr.v4.runtime.tree.ParseTree
import java.util.logging.Level
import java.util.logging.Logger
import javax.inject.Inject

object BabelCompiler {

    private val SuppressExceptions = java.lang.Boolean.getBoolean("com.empowerops.babel.BabelCompiler.SuppressExceptions")

    fun isLegalVariableName(variableName: String): Boolean {
        val result = compile(variableName) { it.variable_only() }
        return result is BabelExpression
    }

    fun compile(functionLiteral: String): BabelCompilationResult =
        compile(functionLiteral) { it.scalar_evaluable() }


    // TODO this doesnt work with serialization
    fun compile(functionLiteral: String, vararg walkers: BabelParserListener): BabelCompilationResult =
            compile(functionLiteral, *walkers) { it.scalar_evaluable() }

    /**
     * Compiles the supplied string into an executable babel expression,
     * or generates a list of errors if no such executable could be created.
     */
    internal fun compile(
            sourceText: String,
            vararg walkers: BabelParserListener,
            parseCall: (BabelParser) -> ParseTree
    ): BabelCompilationResult {

        var problems = emptySet<ExpressionProblem>()

        if (sourceText.isEmpty()) {
            problems += ExpressionProblem.EmptyExpression
        }
        else try {
            val errorListener = SyntaxErrorCollectingListener(sourceText)
            val parser = setupTokenizerAndParser(sourceText, errorListener)

            Log.fine { "compiling expression '$sourceText'" }
            val currentRoot = parseCall(parser)
            Log.fine { "done parsing expression, got '" + currentRoot.toStringTree(parser) + "'" }

            problems += errorListener.errors

            problems += TypeErrorReportingWalker(sourceText).apply { walk(currentRoot) }.problems
            if (problems.any()) { return CompilationFailure(sourceText, problems) }

            //could throw a user error, anything we need to do to handle it?
            walkers.forEach { it.walk(currentRoot) }

            val booleanRewritingWalker = BooleanRewritingWalker().apply { walk(currentRoot) }
            val symbolTableBuildingWalker = SymbolTableBuildingWalker().apply { walk(currentRoot) }

            problems += StaticEvaluatorRewritingWalker(sourceText).apply { walk(currentRoot) }.problems
            if (problems.any()) { return CompilationFailure(sourceText, problems) }

            val codeGenerator = CodeGeneratingWalker(sourceText).apply { walk(currentRoot) }

            if(problems.any()) { return CompilationFailure(sourceText, problems) }

            require(problems.isEmpty())

            return BabelExpression(
                    sourceText,
                    containsDynamicLookup = symbolTableBuildingWalker.containsDynamicVarLookup,
                    isBooleanExpression = booleanRewritingWalker.isBooleanExpression,
                    staticallyReferencedSymbols = symbolTableBuildingWalker.staticallyReferencedVariables
            ).also {
                it.runtime = codeGenerator.instructions.configuration
            }
        }
        catch(ex: RuntimeBabelException){
            problems += ex.runtimeProblemSource.run {
                ExpressionProblem(sourceText, abbreviatedProblemText, rangeInText, lineNo, characterNo, summary, problemValueDescription)
            }
        }
        catch(ex: RuntimeException) {
            if( ! SuppressExceptions) throw ex;
            Log.log(Level.SEVERE, "crash in babel compiler", ex)
            problems += ExpressionProblem(
                    "Internal error compiling", sourceText, 0..sourceText.lastIndex,
                    1, 1, ex.toString(), "unknown"
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
}

private val Log = Logger.getLogger(BabelCompiler::class.java.canonicalName)