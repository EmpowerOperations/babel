package com.empowerops.babel

import com.empowerops.babel.BabelParser.ExprContext
import org.antlr.v4.runtime.ParserRuleContext
import org.antlr.v4.runtime.RuleContext
import org.antlr.v4.runtime.tree.*


fun ExprContext.callsBinaryOp(): Boolean
        = childCount == 3 && getChild(0) is ExprContext
        && (getChild(2) is ExprContext)

fun ExprContext.callsUnaryOpOrFunction(): Boolean
        = (childCount == 2 && getChild(1) is ExprContext)
        || unaryFunction() != null

fun ExprContext.callsBinaryFunction() = binaryFunction() != null

fun ExprContext.callsAggregation() = sum() != null || prod() != null

fun ExprContext.callsInlineExpression(): Boolean
        = childCount == 3 && getChild(1) is ExprContext
        && (getChild(0) as? TerminalNode)?.symbol?.type == BabelParser.OPEN_PAREN

fun ExprContext.callsLiteralOrVariable() = childCount == 1 && literal() != null || variable() != null
fun ExprContext.callsDynamicVariableAccess() = `var`() != null


val ParserRuleContext.textLocation get() = start.startIndex .. stop.stopIndex

fun ParseTreeListener.walk(treeToWalk: ParseTree): Unit
        = ParseTreeWalker.DEFAULT.walk(this, treeToWalk)

fun ParserRuleContext.adopt(child: ParseTree) {
    if(children == null) children = ArrayList<ParseTree>()
    children.add(child)
    if(child is TerminalNodeImpl) child.parent = this
    if(child is ParserRuleContext) child.parent = this
}