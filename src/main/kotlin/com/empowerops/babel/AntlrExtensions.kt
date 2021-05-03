package com.empowerops.babel

import com.empowerops.babel.BabelParser.ScalarExprContext
import org.antlr.v4.runtime.CommonToken
import org.antlr.v4.runtime.ParserRuleContext
import org.antlr.v4.runtime.RuleContext
import org.antlr.v4.runtime.Token
import org.antlr.v4.runtime.tree.*
import java.lang.StringBuilder
import java.lang.reflect.Modifier


fun ParserRuleContext.callsBinaryOp(): Boolean
        = childCount == 3 && getChild(0) is ScalarExprContext
        && (getChild(2) is ScalarExprContext)

fun ScalarExprContext.callsUnaryOpOrFunction(): Boolean
        = (childCount == 2 && getChild(1) is ScalarExprContext)
        || unaryFunction() != null

fun ScalarExprContext.callsBinaryFunction() = binaryFunction() != null

fun ScalarExprContext.callsAggregation() = sum() != null || prod() != null

fun ScalarExprContext.callsInlineExpression(): Boolean
        = childCount == 3 && getChild(1) is ScalarExprContext
        && (getChild(0) as? TerminalNode)?.symbol?.type == BabelParser.OPEN_PAREN

fun ScalarExprContext.callsLiteralOrVariable() = childCount == 1 && literal() != null || variable() != null
fun ScalarExprContext.callsDynamicVariableAccess() = `var`() != null

fun ScalarExprContext.wrapsLambda() = childCount == 1 && getChild(0) is BabelParser.LambdaExprContext

fun ParseTreeListener.walk(treeToWalk: ParseTree): Unit
        = ParseTreeWalker.DEFAULT.walk(this, treeToWalk)

fun ParserRuleContext.adopt(child: ParseTree) {
    if(children == null) children = ArrayList<ParseTree>()
    children.add(child)
    if(child is TerminalNodeImpl) child.parent = this
    if(child is ParserRuleContext) child.parent = this
}

var ParserRuleContext.terminal: Token?
    get() {
        return (children.singleOrNull() as? TerminalNode?)?.symbol
    }
    set(value) {
        children = mutableListOf<ParseTree>(TerminalNodeImpl(value))
        start = value
        stop = value
    }

class FloatToken(text: String): CommonToken(BabelLexer.FLOAT, text)
val ParserRuleContext.textLocation: IntRange get() = start.startIndex .. stop.stopIndex
val Token.textLocation: IntRange get() = startIndex .. stopIndex

internal fun ScalarExprContext.makeAbbreviatedProblemText(): String {
    return children.joinToString("") {
        when(it){
            is BabelParser.LambdaExprContext -> "${it.text.substringBefore("->")}->..."
            else -> it.text
        }
    }
}


fun ParseTree.renderToSimpleString(): String {
    val stringBuilder = StringBuilder()
    renderToString(stringBuilder, this, 0)
    return stringBuilder.toString()
}

private fun renderToString(stringBuilder: StringBuilder, parseTree: ParseTree, indentLevel: Int) {
    val prefix = "  ".repeat(indentLevel)
    stringBuilder.append(prefix)

    val nodeKlass = parseTree::class
    if(parseTree !is TerminalNode){
        stringBuilder.append(nodeKlass.simpleName)
    }

    val fields = nodeKlass.java.fields
        .filter { ! Modifier.isStatic(it.modifiers) }
        .filter { it.name !in SkippedFields }

    if(fields.any()){
        stringBuilder.append(" ")
        for(field in fields){
            val value = field.get(parseTree)
            stringBuilder.append(field.name).append('=').append(value)
            stringBuilder.append(", ")
        }
        stringBuilder.delete(stringBuilder.length - ", ".length, stringBuilder.length)
    }

    if(parseTree is TerminalNode) {
        stringBuilder.append('\'').append(parseTree.text).append('\'').appendLine()
    }
    else if(parseTree.children.all { it is TerminalNode }) {
        stringBuilder.append(" { ")
        for(child in parseTree.children){
            stringBuilder.append('\'').append(child.text).append('\'').append(' ')
        }
        stringBuilder.append("}").appendLine()
    }
    else if(parseTree.childCount != 0) {
        stringBuilder.append(" {").appendLine()
        for(index in 0 until parseTree.childCount){
            renderToString(stringBuilder, parseTree.getChild(index), indentLevel+1)
        }
        stringBuilder.append(prefix).append("}").appendLine()
    }
    else {
        stringBuilder.appendLine()
    }
}

private val SkippedFields = arrayOf("children", "parent", "start", "stop", "exception", "invokingState", "symbol")

private val ParseTree.children: List<ParseTree> get() = object: AbstractList<ParseTree>(){
    override val size: Int get() = childCount
    override fun get(index: Int) = getChild(index)
}