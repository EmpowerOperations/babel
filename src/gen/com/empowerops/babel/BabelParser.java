// Generated from C:\Users\Geoff\Code\OASIS/Problem-Definition//pre-src/BabelParser.g4 by ANTLR 4.5.3
package com.empowerops.babel;
import org.antlr.v4.runtime.atn.*;
import org.antlr.v4.runtime.dfa.DFA;
import org.antlr.v4.runtime.*;
import org.antlr.v4.runtime.misc.*;
import org.antlr.v4.runtime.tree.*;
import java.util.List;
import java.util.Iterator;
import java.util.ArrayList;

@SuppressWarnings({"all", "warnings", "unchecked", "unused", "cast"})
public class BabelParser extends Parser {
	static { RuntimeMetaData.checkVersion("4.5.3", RuntimeMetaData.VERSION); }

	protected static final DFA[] _decisionToDFA;
	protected static final PredictionContextCache _sharedContextCache =
		new PredictionContextCache();
	public static final int
		INTEGER=1, FLOAT=2, DIGIT=3, COS=4, SIN=5, TAN=6, ATAN=7, ACOS=8, ASIN=9, 
		SINH=10, COSH=11, TANH=12, COT=13, LN=14, LOG=15, ABS=16, SQRT=17, CBRT=18, 
		SQR=19, CUBE=20, CIEL=21, FLOOR=22, MAX=23, MIN=24, CONSTANT=25, SUM=26, 
		PROD=27, DYN_VAR=28, VARIABLE=29, LAMBDA=30, LT=31, LTEQ=32, GT=33, GTEQ=34, 
		EQ=35, MULT=36, DIV=37, MOD=38, PLUS=39, MINUS=40, EXPONENT=41, OPEN_PAREN=42, 
		CLOSE_PAREN=43, OPEN_BRACKET=44, CLOSE_BRACKET=45, COMMA=46, LINEBREAKS=47;
	public static final int
		RULE_expression = 0, RULE_variable_only = 1, RULE_expr = 2, RULE_superscript = 3, 
		RULE_lambdaExpr = 4, RULE_plus = 5, RULE_minus = 6, RULE_negate = 7, RULE_mult = 8, 
		RULE_div = 9, RULE_mod = 10, RULE_raise = 11, RULE_sum = 12, RULE_prod = 13, 
		RULE_lt = 14, RULE_lteq = 15, RULE_gt = 16, RULE_gteq = 17, RULE_eq = 18, 
		RULE_dynamicReference = 19, RULE_binaryFunction = 20, RULE_unaryFunction = 21, 
		RULE_name = 22, RULE_variable = 23, RULE_literal = 24;
	public static final String[] ruleNames = {
		"expression", "variable_only", "expr", "superscript", "lambdaExpr", "plus", 
		"minus", "negate", "mult", "div", "mod", "raise", "sum", "prod", "lt", 
		"lteq", "gt", "gteq", "eq", "dynamicReference", "binaryFunction", "unaryFunction", 
		"name", "variable", "literal"
	};

	private static final String[] _LITERAL_NAMES = {
		null, null, null, null, "'cos'", "'sin'", "'tan'", "'atan'", "'acos'", 
		"'asin'", "'sinh'", "'cosh'", "'tanh'", "'cot'", "'ln'", "'log'", "'abs'", 
		"'sqrt'", "'cbrt'", "'sqr'", "'cube'", "'ceil'", "'floor'", "'max'", "'min'", 
		null, "'sum'", "'prod'", "'var'", null, "'->'", "'<'", "'<='", "'>'", 
		"'>='", "'=='", "'*'", "'/'", "'%'", "'+'", "'-'", "'^'", "'('", "')'", 
		"'['", "']'", "','"
	};
	private static final String[] _SYMBOLIC_NAMES = {
		null, "INTEGER", "FLOAT", "DIGIT", "COS", "SIN", "TAN", "ATAN", "ACOS", 
		"ASIN", "SINH", "COSH", "TANH", "COT", "LN", "LOG", "ABS", "SQRT", "CBRT", 
		"SQR", "CUBE", "CIEL", "FLOOR", "MAX", "MIN", "CONSTANT", "SUM", "PROD", 
		"DYN_VAR", "VARIABLE", "LAMBDA", "LT", "LTEQ", "GT", "GTEQ", "EQ", "MULT", 
		"DIV", "MOD", "PLUS", "MINUS", "EXPONENT", "OPEN_PAREN", "CLOSE_PAREN", 
		"OPEN_BRACKET", "CLOSE_BRACKET", "COMMA", "LINEBREAKS"
	};
	public static final Vocabulary VOCABULARY = new VocabularyImpl(_LITERAL_NAMES, _SYMBOLIC_NAMES);

	/**
	 * @deprecated Use {@link #VOCABULARY} instead.
	 */
	@Deprecated
	public static final String[] tokenNames;
	static {
		tokenNames = new String[_SYMBOLIC_NAMES.length];
		for (int i = 0; i < tokenNames.length; i++) {
			tokenNames[i] = VOCABULARY.getLiteralName(i);
			if (tokenNames[i] == null) {
				tokenNames[i] = VOCABULARY.getSymbolicName(i);
			}

			if (tokenNames[i] == null) {
				tokenNames[i] = "<INVALID>";
			}
		}
	}

	@Override
	@Deprecated
	public String[] getTokenNames() {
		return tokenNames;
	}

	@Override

	public Vocabulary getVocabulary() {
		return VOCABULARY;
	}

	@Override
	public String getGrammarFileName() { return "BabelParser.g4"; }

	@Override
	public String[] getRuleNames() { return ruleNames; }

	@Override
	public String getSerializedATN() { return _serializedATN; }

	@Override
	public ATN getATN() { return _ATN; }

	public BabelParser(TokenStream input) {
		super(input);
		_interp = new ParserATNSimulator(this,_ATN,_decisionToDFA,_sharedContextCache);
	}
	public static class ExpressionContext extends ParserRuleContext {
		public ExprContext expr() {
			return getRuleContext(ExprContext.class,0);
		}
		public TerminalNode EOF() { return getToken(BabelParser.EOF, 0); }
		public ExpressionContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_expression; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterExpression(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitExpression(this);
		}
	}

	public final ExpressionContext expression() throws RecognitionException {
		ExpressionContext _localctx = new ExpressionContext(_ctx, getState());
		enterRule(_localctx, 0, RULE_expression);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(50);
			expr(0);
			setState(51);
			match(EOF);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class Variable_onlyContext extends ParserRuleContext {
		public VariableContext variable() {
			return getRuleContext(VariableContext.class,0);
		}
		public TerminalNode EOF() { return getToken(BabelParser.EOF, 0); }
		public Variable_onlyContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_variable_only; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterVariable_only(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitVariable_only(this);
		}
	}

	public final Variable_onlyContext variable_only() throws RecognitionException {
		Variable_onlyContext _localctx = new Variable_onlyContext(_ctx, getState());
		enterRule(_localctx, 2, RULE_variable_only);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(53);
			variable();
			setState(54);
			match(EOF);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class ExprContext extends ParserRuleContext {
		public LiteralContext literal() {
			return getRuleContext(LiteralContext.class,0);
		}
		public VariableContext variable() {
			return getRuleContext(VariableContext.class,0);
		}
		public DynamicReferenceContext dynamicReference() {
			return getRuleContext(DynamicReferenceContext.class,0);
		}
		public List<ExprContext> expr() {
			return getRuleContexts(ExprContext.class);
		}
		public ExprContext expr(int i) {
			return getRuleContext(ExprContext.class,i);
		}
		public LambdaExprContext lambdaExpr() {
			return getRuleContext(LambdaExprContext.class,0);
		}
		public SumContext sum() {
			return getRuleContext(SumContext.class,0);
		}
		public ProdContext prod() {
			return getRuleContext(ProdContext.class,0);
		}
		public BinaryFunctionContext binaryFunction() {
			return getRuleContext(BinaryFunctionContext.class,0);
		}
		public UnaryFunctionContext unaryFunction() {
			return getRuleContext(UnaryFunctionContext.class,0);
		}
		public NegateContext negate() {
			return getRuleContext(NegateContext.class,0);
		}
		public MultContext mult() {
			return getRuleContext(MultContext.class,0);
		}
		public DivContext div() {
			return getRuleContext(DivContext.class,0);
		}
		public ModContext mod() {
			return getRuleContext(ModContext.class,0);
		}
		public PlusContext plus() {
			return getRuleContext(PlusContext.class,0);
		}
		public MinusContext minus() {
			return getRuleContext(MinusContext.class,0);
		}
		public LtContext lt() {
			return getRuleContext(LtContext.class,0);
		}
		public LteqContext lteq() {
			return getRuleContext(LteqContext.class,0);
		}
		public GtContext gt() {
			return getRuleContext(GtContext.class,0);
		}
		public GteqContext gteq() {
			return getRuleContext(GteqContext.class,0);
		}
		public EqContext eq() {
			return getRuleContext(EqContext.class,0);
		}
		public RaiseContext raise() {
			return getRuleContext(RaiseContext.class,0);
		}
		public SuperscriptContext superscript() {
			return getRuleContext(SuperscriptContext.class,0);
		}
		public ExprContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_expr; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterExpr(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitExpr(this);
		}
	}

	public final ExprContext expr() throws RecognitionException {
		return expr(0);
	}

	private ExprContext expr(int _p) throws RecognitionException {
		ParserRuleContext _parentctx = _ctx;
		int _parentState = getState();
		ExprContext _localctx = new ExprContext(_ctx, _parentState);
		ExprContext _prevctx = _localctx;
		int _startState = 4;
		enterRecursionRule(_localctx, 4, RULE_expr, _p);
		try {
			int _alt;
			enterOuterAlt(_localctx, 1);
			{
			setState(97);
			_errHandler.sync(this);
			switch ( getInterpreter().adaptivePredict(_input,2,_ctx) ) {
			case 1:
				{
				setState(59);
				switch (_input.LA(1)) {
				case INTEGER:
				case FLOAT:
				case CONSTANT:
					{
					setState(57);
					literal();
					}
					break;
				case VARIABLE:
					{
					setState(58);
					variable();
					}
					break;
				default:
					throw new NoViableAltException(this);
				}
				}
				break;
			case 2:
				{
				setState(61);
				dynamicReference();
				setState(62);
				match(OPEN_BRACKET);
				setState(63);
				expr(0);
				setState(64);
				match(CLOSE_BRACKET);
				}
				break;
			case 3:
				{
				setState(66);
				match(OPEN_PAREN);
				setState(67);
				expr(0);
				setState(68);
				match(CLOSE_PAREN);
				}
				break;
			case 4:
				{
				setState(72);
				switch (_input.LA(1)) {
				case SUM:
					{
					setState(70);
					sum();
					}
					break;
				case PROD:
					{
					setState(71);
					prod();
					}
					break;
				default:
					throw new NoViableAltException(this);
				}
				setState(74);
				match(OPEN_PAREN);
				setState(75);
				expr(0);
				setState(76);
				match(COMMA);
				setState(77);
				expr(0);
				setState(78);
				match(COMMA);
				setState(79);
				lambdaExpr();
				setState(80);
				match(CLOSE_PAREN);
				}
				break;
			case 5:
				{
				setState(82);
				binaryFunction();
				setState(83);
				match(OPEN_PAREN);
				setState(84);
				expr(0);
				setState(85);
				match(COMMA);
				setState(86);
				expr(0);
				setState(87);
				match(CLOSE_PAREN);
				}
				break;
			case 6:
				{
				setState(89);
				unaryFunction();
				setState(90);
				match(OPEN_PAREN);
				setState(91);
				expr(0);
				setState(92);
				match(CLOSE_PAREN);
				}
				break;
			case 7:
				{
				setState(94);
				negate();
				setState(95);
				expr(5);
				}
				break;
			}
			_ctx.stop = _input.LT(-1);
			setState(130);
			_errHandler.sync(this);
			_alt = getInterpreter().adaptivePredict(_input,7,_ctx);
			while ( _alt!=2 && _alt!=org.antlr.v4.runtime.atn.ATN.INVALID_ALT_NUMBER ) {
				if ( _alt==1 ) {
					if ( _parseListeners!=null ) triggerExitRuleEvent();
					_prevctx = _localctx;
					{
					setState(128);
					_errHandler.sync(this);
					switch ( getInterpreter().adaptivePredict(_input,6,_ctx) ) {
					case 1:
						{
						_localctx = new ExprContext(_parentctx, _parentState);
						pushNewRecursionContext(_localctx, _startState, RULE_expr);
						setState(99);
						if (!(precpred(_ctx, 3))) throw new FailedPredicateException(this, "precpred(_ctx, 3)");
						setState(103);
						switch (_input.LA(1)) {
						case MULT:
							{
							setState(100);
							mult();
							}
							break;
						case DIV:
							{
							setState(101);
							div();
							}
							break;
						case MOD:
							{
							setState(102);
							mod();
							}
							break;
						default:
							throw new NoViableAltException(this);
						}
						setState(105);
						expr(4);
						}
						break;
					case 2:
						{
						_localctx = new ExprContext(_parentctx, _parentState);
						pushNewRecursionContext(_localctx, _startState, RULE_expr);
						setState(107);
						if (!(precpred(_ctx, 2))) throw new FailedPredicateException(this, "precpred(_ctx, 2)");
						setState(110);
						switch (_input.LA(1)) {
						case PLUS:
							{
							setState(108);
							plus();
							}
							break;
						case MINUS:
							{
							setState(109);
							minus();
							}
							break;
						default:
							throw new NoViableAltException(this);
						}
						setState(112);
						expr(3);
						}
						break;
					case 3:
						{
						_localctx = new ExprContext(_parentctx, _parentState);
						pushNewRecursionContext(_localctx, _startState, RULE_expr);
						setState(114);
						if (!(precpred(_ctx, 1))) throw new FailedPredicateException(this, "precpred(_ctx, 1)");
						setState(120);
						switch (_input.LA(1)) {
						case LT:
							{
							setState(115);
							lt();
							}
							break;
						case LTEQ:
							{
							setState(116);
							lteq();
							}
							break;
						case GT:
							{
							setState(117);
							gt();
							}
							break;
						case GTEQ:
							{
							setState(118);
							gteq();
							}
							break;
						case EQ:
							{
							setState(119);
							eq();
							}
							break;
						default:
							throw new NoViableAltException(this);
						}
						setState(122);
						expr(2);
						}
						break;
					case 4:
						{
						_localctx = new ExprContext(_parentctx, _parentState);
						pushNewRecursionContext(_localctx, _startState, RULE_expr);
						setState(124);
						if (!(precpred(_ctx, 4))) throw new FailedPredicateException(this, "precpred(_ctx, 4)");
						setState(125);
						raise();
						setState(126);
						superscript();
						}
						break;
					}
					} 
				}
				setState(132);
				_errHandler.sync(this);
				_alt = getInterpreter().adaptivePredict(_input,7,_ctx);
			}
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			unrollRecursionContexts(_parentctx);
		}
		return _localctx;
	}

	public static class SuperscriptContext extends ParserRuleContext {
		public ExprContext expr() {
			return getRuleContext(ExprContext.class,0);
		}
		public LiteralContext literal() {
			return getRuleContext(LiteralContext.class,0);
		}
		public VariableContext variable() {
			return getRuleContext(VariableContext.class,0);
		}
		public SuperscriptContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_superscript; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterSuperscript(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitSuperscript(this);
		}
	}

	public final SuperscriptContext superscript() throws RecognitionException {
		SuperscriptContext _localctx = new SuperscriptContext(_ctx, getState());
		enterRule(_localctx, 6, RULE_superscript);
		try {
			setState(139);
			switch (_input.LA(1)) {
			case OPEN_PAREN:
				enterOuterAlt(_localctx, 1);
				{
				{
				setState(133);
				match(OPEN_PAREN);
				setState(134);
				expr(0);
				setState(135);
				match(CLOSE_PAREN);
				}
				}
				break;
			case INTEGER:
			case FLOAT:
			case CONSTANT:
				enterOuterAlt(_localctx, 2);
				{
				setState(137);
				literal();
				}
				break;
			case VARIABLE:
				enterOuterAlt(_localctx, 3);
				{
				setState(138);
				variable();
				}
				break;
			default:
				throw new NoViableAltException(this);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class LambdaExprContext extends ParserRuleContext {
		public NameContext name() {
			return getRuleContext(NameContext.class,0);
		}
		public ExprContext expr() {
			return getRuleContext(ExprContext.class,0);
		}
		public LambdaExprContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_lambdaExpr; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterLambdaExpr(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitLambdaExpr(this);
		}
	}

	public final LambdaExprContext lambdaExpr() throws RecognitionException {
		LambdaExprContext _localctx = new LambdaExprContext(_ctx, getState());
		enterRule(_localctx, 8, RULE_lambdaExpr);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(141);
			name();
			setState(142);
			match(LAMBDA);
			setState(143);
			expr(0);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class PlusContext extends ParserRuleContext {
		public PlusContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_plus; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterPlus(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitPlus(this);
		}
	}

	public final PlusContext plus() throws RecognitionException {
		PlusContext _localctx = new PlusContext(_ctx, getState());
		enterRule(_localctx, 10, RULE_plus);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(145);
			match(PLUS);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class MinusContext extends ParserRuleContext {
		public MinusContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_minus; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterMinus(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitMinus(this);
		}
	}

	public final MinusContext minus() throws RecognitionException {
		MinusContext _localctx = new MinusContext(_ctx, getState());
		enterRule(_localctx, 12, RULE_minus);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(147);
			match(MINUS);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class NegateContext extends ParserRuleContext {
		public NegateContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_negate; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterNegate(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitNegate(this);
		}
	}

	public final NegateContext negate() throws RecognitionException {
		NegateContext _localctx = new NegateContext(_ctx, getState());
		enterRule(_localctx, 14, RULE_negate);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(149);
			match(MINUS);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class MultContext extends ParserRuleContext {
		public MultContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_mult; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterMult(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitMult(this);
		}
	}

	public final MultContext mult() throws RecognitionException {
		MultContext _localctx = new MultContext(_ctx, getState());
		enterRule(_localctx, 16, RULE_mult);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(151);
			match(MULT);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class DivContext extends ParserRuleContext {
		public DivContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_div; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterDiv(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitDiv(this);
		}
	}

	public final DivContext div() throws RecognitionException {
		DivContext _localctx = new DivContext(_ctx, getState());
		enterRule(_localctx, 18, RULE_div);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(153);
			match(DIV);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class ModContext extends ParserRuleContext {
		public ModContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_mod; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterMod(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitMod(this);
		}
	}

	public final ModContext mod() throws RecognitionException {
		ModContext _localctx = new ModContext(_ctx, getState());
		enterRule(_localctx, 20, RULE_mod);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(155);
			match(MOD);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class RaiseContext extends ParserRuleContext {
		public RaiseContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_raise; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterRaise(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitRaise(this);
		}
	}

	public final RaiseContext raise() throws RecognitionException {
		RaiseContext _localctx = new RaiseContext(_ctx, getState());
		enterRule(_localctx, 22, RULE_raise);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(157);
			match(EXPONENT);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class SumContext extends ParserRuleContext {
		public SumContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_sum; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterSum(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitSum(this);
		}
	}

	public final SumContext sum() throws RecognitionException {
		SumContext _localctx = new SumContext(_ctx, getState());
		enterRule(_localctx, 24, RULE_sum);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(159);
			match(SUM);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class ProdContext extends ParserRuleContext {
		public ProdContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_prod; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterProd(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitProd(this);
		}
	}

	public final ProdContext prod() throws RecognitionException {
		ProdContext _localctx = new ProdContext(_ctx, getState());
		enterRule(_localctx, 26, RULE_prod);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(161);
			match(PROD);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class LtContext extends ParserRuleContext {
		public LtContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_lt; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterLt(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitLt(this);
		}
	}

	public final LtContext lt() throws RecognitionException {
		LtContext _localctx = new LtContext(_ctx, getState());
		enterRule(_localctx, 28, RULE_lt);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(163);
			match(LT);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class LteqContext extends ParserRuleContext {
		public LteqContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_lteq; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterLteq(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitLteq(this);
		}
	}

	public final LteqContext lteq() throws RecognitionException {
		LteqContext _localctx = new LteqContext(_ctx, getState());
		enterRule(_localctx, 30, RULE_lteq);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(165);
			match(LTEQ);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class GtContext extends ParserRuleContext {
		public GtContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_gt; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterGt(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitGt(this);
		}
	}

	public final GtContext gt() throws RecognitionException {
		GtContext _localctx = new GtContext(_ctx, getState());
		enterRule(_localctx, 32, RULE_gt);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(167);
			match(GT);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class GteqContext extends ParserRuleContext {
		public GteqContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_gteq; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterGteq(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitGteq(this);
		}
	}

	public final GteqContext gteq() throws RecognitionException {
		GteqContext _localctx = new GteqContext(_ctx, getState());
		enterRule(_localctx, 34, RULE_gteq);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(169);
			match(GTEQ);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class EqContext extends ParserRuleContext {
		public EqContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_eq; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterEq(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitEq(this);
		}
	}

	public final EqContext eq() throws RecognitionException {
		EqContext _localctx = new EqContext(_ctx, getState());
		enterRule(_localctx, 36, RULE_eq);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(171);
			match(EQ);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class DynamicReferenceContext extends ParserRuleContext {
		public DynamicReferenceContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_dynamicReference; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterDynamicReference(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitDynamicReference(this);
		}
	}

	public final DynamicReferenceContext dynamicReference() throws RecognitionException {
		DynamicReferenceContext _localctx = new DynamicReferenceContext(_ctx, getState());
		enterRule(_localctx, 38, RULE_dynamicReference);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(173);
			match(DYN_VAR);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class BinaryFunctionContext extends ParserRuleContext {
		public BinaryFunctionContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_binaryFunction; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterBinaryFunction(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitBinaryFunction(this);
		}
	}

	public final BinaryFunctionContext binaryFunction() throws RecognitionException {
		BinaryFunctionContext _localctx = new BinaryFunctionContext(_ctx, getState());
		enterRule(_localctx, 40, RULE_binaryFunction);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(175);
			_la = _input.LA(1);
			if ( !((((_la) & ~0x3f) == 0 && ((1L << _la) & ((1L << LOG) | (1L << MAX) | (1L << MIN))) != 0)) ) {
			_errHandler.recoverInline(this);
			} else {
				consume();
			}
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class UnaryFunctionContext extends ParserRuleContext {
		public UnaryFunctionContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_unaryFunction; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterUnaryFunction(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitUnaryFunction(this);
		}
	}

	public final UnaryFunctionContext unaryFunction() throws RecognitionException {
		UnaryFunctionContext _localctx = new UnaryFunctionContext(_ctx, getState());
		enterRule(_localctx, 42, RULE_unaryFunction);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(177);
			_la = _input.LA(1);
			if ( !((((_la) & ~0x3f) == 0 && ((1L << _la) & ((1L << COS) | (1L << SIN) | (1L << TAN) | (1L << ATAN) | (1L << ACOS) | (1L << ASIN) | (1L << SINH) | (1L << COSH) | (1L << TANH) | (1L << COT) | (1L << LN) | (1L << LOG) | (1L << ABS) | (1L << SQRT) | (1L << CBRT) | (1L << SQR) | (1L << CUBE) | (1L << CIEL) | (1L << FLOOR))) != 0)) ) {
			_errHandler.recoverInline(this);
			} else {
				consume();
			}
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class NameContext extends ParserRuleContext {
		public TerminalNode VARIABLE() { return getToken(BabelParser.VARIABLE, 0); }
		public NameContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_name; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterName(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitName(this);
		}
	}

	public final NameContext name() throws RecognitionException {
		NameContext _localctx = new NameContext(_ctx, getState());
		enterRule(_localctx, 44, RULE_name);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(179);
			match(VARIABLE);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class VariableContext extends ParserRuleContext {
		public TerminalNode VARIABLE() { return getToken(BabelParser.VARIABLE, 0); }
		public VariableContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_variable; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterVariable(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitVariable(this);
		}
	}

	public final VariableContext variable() throws RecognitionException {
		VariableContext _localctx = new VariableContext(_ctx, getState());
		enterRule(_localctx, 46, RULE_variable);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(181);
			match(VARIABLE);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static class LiteralContext extends ParserRuleContext {
		public TerminalNode INTEGER() { return getToken(BabelParser.INTEGER, 0); }
		public TerminalNode FLOAT() { return getToken(BabelParser.FLOAT, 0); }
		public TerminalNode CONSTANT() { return getToken(BabelParser.CONSTANT, 0); }
		public LiteralContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_literal; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).enterLiteral(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof BabelParserListener ) ((BabelParserListener)listener).exitLiteral(this);
		}
	}

	public final LiteralContext literal() throws RecognitionException {
		LiteralContext _localctx = new LiteralContext(_ctx, getState());
		enterRule(_localctx, 48, RULE_literal);
		int _la;
		try {
			setState(185);
			switch (_input.LA(1)) {
			case INTEGER:
			case FLOAT:
				enterOuterAlt(_localctx, 1);
				{
				setState(183);
				_la = _input.LA(1);
				if ( !(_la==INTEGER || _la==FLOAT) ) {
				_errHandler.recoverInline(this);
				} else {
					consume();
				}
				}
				break;
			case CONSTANT:
				enterOuterAlt(_localctx, 2);
				{
				setState(184);
				match(CONSTANT);
				}
				break;
			default:
				throw new NoViableAltException(this);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public boolean sempred(RuleContext _localctx, int ruleIndex, int predIndex) {
		switch (ruleIndex) {
		case 2:
			return expr_sempred((ExprContext)_localctx, predIndex);
		}
		return true;
	}
	private boolean expr_sempred(ExprContext _localctx, int predIndex) {
		switch (predIndex) {
		case 0:
			return precpred(_ctx, 3);
		case 1:
			return precpred(_ctx, 2);
		case 2:
			return precpred(_ctx, 1);
		case 3:
			return precpred(_ctx, 4);
		}
		return true;
	}

	public static final String _serializedATN =
		"\3\u0430\ud6d1\u8206\uad2d\u4417\uaef1\u8d80\uaadd\3\61\u00be\4\2\t\2"+
		"\4\3\t\3\4\4\t\4\4\5\t\5\4\6\t\6\4\7\t\7\4\b\t\b\4\t\t\t\4\n\t\n\4\13"+
		"\t\13\4\f\t\f\4\r\t\r\4\16\t\16\4\17\t\17\4\20\t\20\4\21\t\21\4\22\t\22"+
		"\4\23\t\23\4\24\t\24\4\25\t\25\4\26\t\26\4\27\t\27\4\30\t\30\4\31\t\31"+
		"\4\32\t\32\3\2\3\2\3\2\3\3\3\3\3\3\3\4\3\4\3\4\5\4>\n\4\3\4\3\4\3\4\3"+
		"\4\3\4\3\4\3\4\3\4\3\4\3\4\3\4\5\4K\n\4\3\4\3\4\3\4\3\4\3\4\3\4\3\4\3"+
		"\4\3\4\3\4\3\4\3\4\3\4\3\4\3\4\3\4\3\4\3\4\3\4\3\4\3\4\3\4\3\4\5\4d\n"+
		"\4\3\4\3\4\3\4\3\4\5\4j\n\4\3\4\3\4\3\4\3\4\3\4\5\4q\n\4\3\4\3\4\3\4\3"+
		"\4\3\4\3\4\3\4\3\4\5\4{\n\4\3\4\3\4\3\4\3\4\3\4\3\4\7\4\u0083\n\4\f\4"+
		"\16\4\u0086\13\4\3\5\3\5\3\5\3\5\3\5\3\5\5\5\u008e\n\5\3\6\3\6\3\6\3\6"+
		"\3\7\3\7\3\b\3\b\3\t\3\t\3\n\3\n\3\13\3\13\3\f\3\f\3\r\3\r\3\16\3\16\3"+
		"\17\3\17\3\20\3\20\3\21\3\21\3\22\3\22\3\23\3\23\3\24\3\24\3\25\3\25\3"+
		"\26\3\26\3\27\3\27\3\30\3\30\3\31\3\31\3\32\3\32\5\32\u00bc\n\32\3\32"+
		"\2\3\6\33\2\4\6\b\n\f\16\20\22\24\26\30\32\34\36 \"$&(*,.\60\62\2\5\4"+
		"\2\21\21\31\32\3\2\6\30\3\2\3\4\u00ba\2\64\3\2\2\2\4\67\3\2\2\2\6c\3\2"+
		"\2\2\b\u008d\3\2\2\2\n\u008f\3\2\2\2\f\u0093\3\2\2\2\16\u0095\3\2\2\2"+
		"\20\u0097\3\2\2\2\22\u0099\3\2\2\2\24\u009b\3\2\2\2\26\u009d\3\2\2\2\30"+
		"\u009f\3\2\2\2\32\u00a1\3\2\2\2\34\u00a3\3\2\2\2\36\u00a5\3\2\2\2 \u00a7"+
		"\3\2\2\2\"\u00a9\3\2\2\2$\u00ab\3\2\2\2&\u00ad\3\2\2\2(\u00af\3\2\2\2"+
		"*\u00b1\3\2\2\2,\u00b3\3\2\2\2.\u00b5\3\2\2\2\60\u00b7\3\2\2\2\62\u00bb"+
		"\3\2\2\2\64\65\5\6\4\2\65\66\7\2\2\3\66\3\3\2\2\2\678\5\60\31\289\7\2"+
		"\2\39\5\3\2\2\2:=\b\4\1\2;>\5\62\32\2<>\5\60\31\2=;\3\2\2\2=<\3\2\2\2"+
		">d\3\2\2\2?@\5(\25\2@A\7.\2\2AB\5\6\4\2BC\7/\2\2Cd\3\2\2\2DE\7,\2\2EF"+
		"\5\6\4\2FG\7-\2\2Gd\3\2\2\2HK\5\32\16\2IK\5\34\17\2JH\3\2\2\2JI\3\2\2"+
		"\2KL\3\2\2\2LM\7,\2\2MN\5\6\4\2NO\7\60\2\2OP\5\6\4\2PQ\7\60\2\2QR\5\n"+
		"\6\2RS\7-\2\2Sd\3\2\2\2TU\5*\26\2UV\7,\2\2VW\5\6\4\2WX\7\60\2\2XY\5\6"+
		"\4\2YZ\7-\2\2Zd\3\2\2\2[\\\5,\27\2\\]\7,\2\2]^\5\6\4\2^_\7-\2\2_d\3\2"+
		"\2\2`a\5\20\t\2ab\5\6\4\7bd\3\2\2\2c:\3\2\2\2c?\3\2\2\2cD\3\2\2\2cJ\3"+
		"\2\2\2cT\3\2\2\2c[\3\2\2\2c`\3\2\2\2d\u0084\3\2\2\2ei\f\5\2\2fj\5\22\n"+
		"\2gj\5\24\13\2hj\5\26\f\2if\3\2\2\2ig\3\2\2\2ih\3\2\2\2jk\3\2\2\2kl\5"+
		"\6\4\6l\u0083\3\2\2\2mp\f\4\2\2nq\5\f\7\2oq\5\16\b\2pn\3\2\2\2po\3\2\2"+
		"\2qr\3\2\2\2rs\5\6\4\5s\u0083\3\2\2\2tz\f\3\2\2u{\5\36\20\2v{\5 \21\2"+
		"w{\5\"\22\2x{\5$\23\2y{\5&\24\2zu\3\2\2\2zv\3\2\2\2zw\3\2\2\2zx\3\2\2"+
		"\2zy\3\2\2\2{|\3\2\2\2|}\5\6\4\4}\u0083\3\2\2\2~\177\f\6\2\2\177\u0080"+
		"\5\30\r\2\u0080\u0081\5\b\5\2\u0081\u0083\3\2\2\2\u0082e\3\2\2\2\u0082"+
		"m\3\2\2\2\u0082t\3\2\2\2\u0082~\3\2\2\2\u0083\u0086\3\2\2\2\u0084\u0082"+
		"\3\2\2\2\u0084\u0085\3\2\2\2\u0085\7\3\2\2\2\u0086\u0084\3\2\2\2\u0087"+
		"\u0088\7,\2\2\u0088\u0089\5\6\4\2\u0089\u008a\7-\2\2\u008a\u008e\3\2\2"+
		"\2\u008b\u008e\5\62\32\2\u008c\u008e\5\60\31\2\u008d\u0087\3\2\2\2\u008d"+
		"\u008b\3\2\2\2\u008d\u008c\3\2\2\2\u008e\t\3\2\2\2\u008f\u0090\5.\30\2"+
		"\u0090\u0091\7 \2\2\u0091\u0092\5\6\4\2\u0092\13\3\2\2\2\u0093\u0094\7"+
		")\2\2\u0094\r\3\2\2\2\u0095\u0096\7*\2\2\u0096\17\3\2\2\2\u0097\u0098"+
		"\7*\2\2\u0098\21\3\2\2\2\u0099\u009a\7&\2\2\u009a\23\3\2\2\2\u009b\u009c"+
		"\7\'\2\2\u009c\25\3\2\2\2\u009d\u009e\7(\2\2\u009e\27\3\2\2\2\u009f\u00a0"+
		"\7+\2\2\u00a0\31\3\2\2\2\u00a1\u00a2\7\34\2\2\u00a2\33\3\2\2\2\u00a3\u00a4"+
		"\7\35\2\2\u00a4\35\3\2\2\2\u00a5\u00a6\7!\2\2\u00a6\37\3\2\2\2\u00a7\u00a8"+
		"\7\"\2\2\u00a8!\3\2\2\2\u00a9\u00aa\7#\2\2\u00aa#\3\2\2\2\u00ab\u00ac"+
		"\7$\2\2\u00ac%\3\2\2\2\u00ad\u00ae\7%\2\2\u00ae\'\3\2\2\2\u00af\u00b0"+
		"\7\36\2\2\u00b0)\3\2\2\2\u00b1\u00b2\t\2\2\2\u00b2+\3\2\2\2\u00b3\u00b4"+
		"\t\3\2\2\u00b4-\3\2\2\2\u00b5\u00b6\7\37\2\2\u00b6/\3\2\2\2\u00b7\u00b8"+
		"\7\37\2\2\u00b8\61\3\2\2\2\u00b9\u00bc\t\4\2\2\u00ba\u00bc\7\33\2\2\u00bb"+
		"\u00b9\3\2\2\2\u00bb\u00ba\3\2\2\2\u00bc\63\3\2\2\2\f=Jcipz\u0082\u0084"+
		"\u008d\u00bb";
	public static final ATN _ATN =
		new ATNDeserializer().deserialize(_serializedATN.toCharArray());
	static {
		_decisionToDFA = new DFA[_ATN.getNumberOfDecisions()];
		for (int i = 0; i < _ATN.getNumberOfDecisions(); i++) {
			_decisionToDFA[i] = new DFA(_ATN.getDecisionState(i), i);
		}
	}
}