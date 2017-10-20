package com.empowerops.babel

import com.empowerops.babel.BabelParser.ExprContext
import org.antlr.v4.runtime.ParserRuleContext
import org.antlr.v4.runtime.tree.TerminalNode


fun ExprContext.callsBinaryOp(): Boolean
        = childCount == 3 && getChild(0) is ExprContext
        && (getChild(2) is ExprContext || superscript() != null)

fun ExprContext.callsUnaryOpOrFunction(): Boolean
        = (childCount == 2 && getChild(1) is ExprContext)
        || unaryFunction() != null

fun ExprContext.callsBinaryFunction() = binaryFunction() != null

fun ExprContext.callsAggregation() = sum() != null || prod() != null

fun ExprContext.callsInlineExpression(): Boolean
        = childCount == 3 && getChild(1) is ExprContext
        && (getChild(0) as? TerminalNode)?.symbol?.type == BabelParser.OPEN_PAREN

fun ExprContext.callsLiteralOrVariable() = childCount == 1 && literal() != null || variable() != null
fun ExprContext.callsDynamicVariableAccess() = dynamicReference() != null


val ParserRuleContext.textLocation get() = start.startIndex .. stop.stopIndex
