package com.empowerops.babel

import com.empowerops.babel.BabelParser.ScalarExprContext
import org.antlr.v4.runtime.CommonToken
import org.antlr.v4.runtime.ParserRuleContext
import org.antlr.v4.runtime.RuleContext
import org.antlr.v4.runtime.Token
import org.antlr.v4.runtime.tree.*


fun ParserRuleContext.callsBinaryOp(): Boolean
        = childCount == 3 && getChild(0) is ScalarExprContext
        && (getChild(2) is ScalarExprContext)

fun ScalarExprContext.callsUnaryOpOrFunction(): Boolean
        = (childCount == 2 && getChild(1) is ScalarExprContext)
        || unaryFunction() != null

fun ScalarExprContext.callsVariadicFunction() = variadicFunction() != null

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

class ValueToken(val value: Number, text: String): CommonToken(BabelLexer.INTEGER, value.toString()){
    init { this.text = text }
}
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