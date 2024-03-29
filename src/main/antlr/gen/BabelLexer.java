// Generated from C:/Users/geoff/Code/babel/src/main/antlr\BabelLexer.g4 by ANTLR 4.9.1
import org.antlr.v4.runtime.Lexer;
import org.antlr.v4.runtime.CharStream;
import org.antlr.v4.runtime.Token;
import org.antlr.v4.runtime.TokenStream;
import org.antlr.v4.runtime.*;
import org.antlr.v4.runtime.atn.*;
import org.antlr.v4.runtime.dfa.DFA;
import org.antlr.v4.runtime.misc.*;

@SuppressWarnings({"all", "warnings", "unchecked", "unused", "cast"})
public class BabelLexer extends Lexer {
	static { RuntimeMetaData.checkVersion("4.9.1", RuntimeMetaData.VERSION); }

	protected static final DFA[] _decisionToDFA;
	protected static final PredictionContextCache _sharedContextCache =
		new PredictionContextCache();
	public static final int
		INTEGER=1, FLOAT=2, DIGIT=3, COS=4, SIN=5, TAN=6, ATAN=7, ACOS=8, ASIN=9, 
		SINH=10, COSH=11, TANH=12, COT=13, LN=14, LOG=15, ABS=16, SQRT=17, CBRT=18, 
		SQR=19, CUBE=20, CIEL=21, FLOOR=22, MAX=23, MIN=24, SGN=25, PI=26, EULERS_E=27, 
		TRUE=28, FALSE=29, SUM=30, PROD=31, DYN_VAR=32, LAMBDA=33, LT=34, LTEQ=35, 
		GT=36, GTEQ=37, EQ=38, MULT=39, DIV=40, MOD=41, PLUS=42, MINUS=43, EXPONENT=44, 
		OPEN_PAREN=45, CLOSE_PAREN=46, OPEN_BRACKET=47, CLOSE_BRACKET=48, COMMA=49, 
		ASSIGN=50, RETURN=51, PLUS_MINUS=52, EOL=53, VARIABLE=54, LINEBREAKS=55;
	public static String[] channelNames = {
		"DEFAULT_TOKEN_CHANNEL", "HIDDEN"
	};

	public static String[] modeNames = {
		"DEFAULT_MODE"
	};

	private static String[] makeRuleNames() {
		return new String[] {
			"INTEGER", "FLOAT", "DIGIT", "COS", "SIN", "TAN", "ATAN", "ACOS", "ASIN", 
			"SINH", "COSH", "TANH", "COT", "LN", "LOG", "ABS", "SQRT", "CBRT", "SQR", 
			"CUBE", "CIEL", "FLOOR", "MAX", "MIN", "SGN", "PI", "EULERS_E", "TRUE", 
			"FALSE", "SUM", "PROD", "DYN_VAR", "LAMBDA", "LT", "LTEQ", "GT", "GTEQ", 
			"EQ", "MULT", "DIV", "MOD", "PLUS", "MINUS", "EXPONENT", "OPEN_PAREN", 
			"CLOSE_PAREN", "OPEN_BRACKET", "CLOSE_BRACKET", "COMMA", "ASSIGN", "RETURN", 
			"PLUS_MINUS", "EOL", "VARIABLE", "VARIABLE_START", "VARIABLE_PART", "LINEBREAKS"
		};
	}
	public static final String[] ruleNames = makeRuleNames();

	private static String[] makeLiteralNames() {
		return new String[] {
			null, null, null, null, "'cos'", "'sin'", "'tan'", "'atan'", "'acos'", 
			"'asin'", "'sinh'", "'cosh'", "'tanh'", "'cot'", "'ln'", "'log'", "'abs'", 
			"'sqrt'", "'cbrt'", "'sqr'", "'cube'", "'ceil'", "'floor'", "'max'", 
			"'min'", "'sgn'", "'pi'", "'e'", "'true'", "'false'", "'sum'", "'prod'", 
			"'var'", "'->'", "'<'", "'<='", "'>'", "'>='", "'=='", "'*'", "'/'", 
			"'%'", "'+'", "'-'", "'^'", "'('", "')'", "'['", "']'", "','", "'='", 
			"'return'", "'+/-'", "';'"
		};
	}
	private static final String[] _LITERAL_NAMES = makeLiteralNames();
	private static String[] makeSymbolicNames() {
		return new String[] {
			null, "INTEGER", "FLOAT", "DIGIT", "COS", "SIN", "TAN", "ATAN", "ACOS", 
			"ASIN", "SINH", "COSH", "TANH", "COT", "LN", "LOG", "ABS", "SQRT", "CBRT", 
			"SQR", "CUBE", "CIEL", "FLOOR", "MAX", "MIN", "SGN", "PI", "EULERS_E", 
			"TRUE", "FALSE", "SUM", "PROD", "DYN_VAR", "LAMBDA", "LT", "LTEQ", "GT", 
			"GTEQ", "EQ", "MULT", "DIV", "MOD", "PLUS", "MINUS", "EXPONENT", "OPEN_PAREN", 
			"CLOSE_PAREN", "OPEN_BRACKET", "CLOSE_BRACKET", "COMMA", "ASSIGN", "RETURN", 
			"PLUS_MINUS", "EOL", "VARIABLE", "LINEBREAKS"
		};
	}
	private static final String[] _SYMBOLIC_NAMES = makeSymbolicNames();
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


	public BabelLexer(CharStream input) {
		super(input);
		_interp = new LexerATNSimulator(this,_ATN,_decisionToDFA,_sharedContextCache);
	}

	@Override
	public String getGrammarFileName() { return "BabelLexer.g4"; }

	@Override
	public String[] getRuleNames() { return ruleNames; }

	@Override
	public String getSerializedATN() { return _serializedATN; }

	@Override
	public String[] getChannelNames() { return channelNames; }

	@Override
	public String[] getModeNames() { return modeNames; }

	@Override
	public ATN getATN() { return _ATN; }

	public static final String _serializedATN =
		"\3\u608b\ua72a\u8133\ub9ed\u417c\u3be7\u7786\u5964\29\u015d\b\1\4\2\t"+
		"\2\4\3\t\3\4\4\t\4\4\5\t\5\4\6\t\6\4\7\t\7\4\b\t\b\4\t\t\t\4\n\t\n\4\13"+
		"\t\13\4\f\t\f\4\r\t\r\4\16\t\16\4\17\t\17\4\20\t\20\4\21\t\21\4\22\t\22"+
		"\4\23\t\23\4\24\t\24\4\25\t\25\4\26\t\26\4\27\t\27\4\30\t\30\4\31\t\31"+
		"\4\32\t\32\4\33\t\33\4\34\t\34\4\35\t\35\4\36\t\36\4\37\t\37\4 \t \4!"+
		"\t!\4\"\t\"\4#\t#\4$\t$\4%\t%\4&\t&\4\'\t\'\4(\t(\4)\t)\4*\t*\4+\t+\4"+
		",\t,\4-\t-\4.\t.\4/\t/\4\60\t\60\4\61\t\61\4\62\t\62\4\63\t\63\4\64\t"+
		"\64\4\65\t\65\4\66\t\66\4\67\t\67\48\t8\49\t9\4:\t:\3\2\6\2w\n\2\r\2\16"+
		"\2x\3\3\7\3|\n\3\f\3\16\3\177\13\3\3\3\3\3\6\3\u0083\n\3\r\3\16\3\u0084"+
		"\3\3\3\3\5\3\u0089\n\3\3\3\6\3\u008c\n\3\r\3\16\3\u008d\5\3\u0090\n\3"+
		"\3\4\3\4\3\5\3\5\3\5\3\5\3\6\3\6\3\6\3\6\3\7\3\7\3\7\3\7\3\b\3\b\3\b\3"+
		"\b\3\b\3\t\3\t\3\t\3\t\3\t\3\n\3\n\3\n\3\n\3\n\3\13\3\13\3\13\3\13\3\13"+
		"\3\f\3\f\3\f\3\f\3\f\3\r\3\r\3\r\3\r\3\r\3\16\3\16\3\16\3\16\3\17\3\17"+
		"\3\17\3\20\3\20\3\20\3\20\3\21\3\21\3\21\3\21\3\22\3\22\3\22\3\22\3\22"+
		"\3\23\3\23\3\23\3\23\3\23\3\24\3\24\3\24\3\24\3\25\3\25\3\25\3\25\3\25"+
		"\3\26\3\26\3\26\3\26\3\26\3\27\3\27\3\27\3\27\3\27\3\27\3\30\3\30\3\30"+
		"\3\30\3\31\3\31\3\31\3\31\3\32\3\32\3\32\3\32\3\33\3\33\3\33\3\34\3\34"+
		"\3\35\3\35\3\35\3\35\3\35\3\36\3\36\3\36\3\36\3\36\3\36\3\37\3\37\3\37"+
		"\3\37\3 \3 \3 \3 \3 \3!\3!\3!\3!\3\"\3\"\3\"\3#\3#\3$\3$\3$\3%\3%\3&\3"+
		"&\3&\3\'\3\'\3\'\3(\3(\3)\3)\3*\3*\3+\3+\3,\3,\3-\3-\3.\3.\3/\3/\3\60"+
		"\3\60\3\61\3\61\3\62\3\62\3\63\3\63\3\64\3\64\3\64\3\64\3\64\3\64\3\64"+
		"\3\65\3\65\3\65\3\65\3\66\3\66\3\67\3\67\7\67\u014b\n\67\f\67\16\67\u014e"+
		"\13\67\38\38\38\38\58\u0154\n8\39\39\59\u0158\n9\3:\3:\3:\3:\2\2;\3\3"+
		"\5\4\7\5\t\6\13\7\r\b\17\t\21\n\23\13\25\f\27\r\31\16\33\17\35\20\37\21"+
		"!\22#\23%\24\'\25)\26+\27-\30/\31\61\32\63\33\65\34\67\359\36;\37= ?!"+
		"A\"C#E$G%I&K\'M(O)Q*S+U,W-Y.[/]\60_\61a\62c\63e\64g\65i\66k\67m8o\2q\2"+
		"s9\3\2\t\4\2GGgg\5\2C\\aac|\4\2\2\u0101\ud802\udc01\3\2\ud802\udc01\3"+
		"\2\udc02\ue001\3\2\62;\5\2\f\f\17\17\"\"\2\u0164\2\3\3\2\2\2\2\5\3\2\2"+
		"\2\2\7\3\2\2\2\2\t\3\2\2\2\2\13\3\2\2\2\2\r\3\2\2\2\2\17\3\2\2\2\2\21"+
		"\3\2\2\2\2\23\3\2\2\2\2\25\3\2\2\2\2\27\3\2\2\2\2\31\3\2\2\2\2\33\3\2"+
		"\2\2\2\35\3\2\2\2\2\37\3\2\2\2\2!\3\2\2\2\2#\3\2\2\2\2%\3\2\2\2\2\'\3"+
		"\2\2\2\2)\3\2\2\2\2+\3\2\2\2\2-\3\2\2\2\2/\3\2\2\2\2\61\3\2\2\2\2\63\3"+
		"\2\2\2\2\65\3\2\2\2\2\67\3\2\2\2\29\3\2\2\2\2;\3\2\2\2\2=\3\2\2\2\2?\3"+
		"\2\2\2\2A\3\2\2\2\2C\3\2\2\2\2E\3\2\2\2\2G\3\2\2\2\2I\3\2\2\2\2K\3\2\2"+
		"\2\2M\3\2\2\2\2O\3\2\2\2\2Q\3\2\2\2\2S\3\2\2\2\2U\3\2\2\2\2W\3\2\2\2\2"+
		"Y\3\2\2\2\2[\3\2\2\2\2]\3\2\2\2\2_\3\2\2\2\2a\3\2\2\2\2c\3\2\2\2\2e\3"+
		"\2\2\2\2g\3\2\2\2\2i\3\2\2\2\2k\3\2\2\2\2m\3\2\2\2\2s\3\2\2\2\3v\3\2\2"+
		"\2\5}\3\2\2\2\7\u0091\3\2\2\2\t\u0093\3\2\2\2\13\u0097\3\2\2\2\r\u009b"+
		"\3\2\2\2\17\u009f\3\2\2\2\21\u00a4\3\2\2\2\23\u00a9\3\2\2\2\25\u00ae\3"+
		"\2\2\2\27\u00b3\3\2\2\2\31\u00b8\3\2\2\2\33\u00bd\3\2\2\2\35\u00c1\3\2"+
		"\2\2\37\u00c4\3\2\2\2!\u00c8\3\2\2\2#\u00cc\3\2\2\2%\u00d1\3\2\2\2\'\u00d6"+
		"\3\2\2\2)\u00da\3\2\2\2+\u00df\3\2\2\2-\u00e4\3\2\2\2/\u00ea\3\2\2\2\61"+
		"\u00ee\3\2\2\2\63\u00f2\3\2\2\2\65\u00f6\3\2\2\2\67\u00f9\3\2\2\29\u00fb"+
		"\3\2\2\2;\u0100\3\2\2\2=\u0106\3\2\2\2?\u010a\3\2\2\2A\u010f\3\2\2\2C"+
		"\u0113\3\2\2\2E\u0116\3\2\2\2G\u0118\3\2\2\2I\u011b\3\2\2\2K\u011d\3\2"+
		"\2\2M\u0120\3\2\2\2O\u0123\3\2\2\2Q\u0125\3\2\2\2S\u0127\3\2\2\2U\u0129"+
		"\3\2\2\2W\u012b\3\2\2\2Y\u012d\3\2\2\2[\u012f\3\2\2\2]\u0131\3\2\2\2_"+
		"\u0133\3\2\2\2a\u0135\3\2\2\2c\u0137\3\2\2\2e\u0139\3\2\2\2g\u013b\3\2"+
		"\2\2i\u0142\3\2\2\2k\u0146\3\2\2\2m\u0148\3\2\2\2o\u0153\3\2\2\2q\u0157"+
		"\3\2\2\2s\u0159\3\2\2\2uw\5\7\4\2vu\3\2\2\2wx\3\2\2\2xv\3\2\2\2xy\3\2"+
		"\2\2y\4\3\2\2\2z|\5\7\4\2{z\3\2\2\2|\177\3\2\2\2}{\3\2\2\2}~\3\2\2\2~"+
		"\u0080\3\2\2\2\177}\3\2\2\2\u0080\u0082\7\60\2\2\u0081\u0083\5\7\4\2\u0082"+
		"\u0081\3\2\2\2\u0083\u0084\3\2\2\2\u0084\u0082\3\2\2\2\u0084\u0085\3\2"+
		"\2\2\u0085\u008f\3\2\2\2\u0086\u0088\t\2\2\2\u0087\u0089\7/\2\2\u0088"+
		"\u0087\3\2\2\2\u0088\u0089\3\2\2\2\u0089\u008b\3\2\2\2\u008a\u008c\5\7"+
		"\4\2\u008b\u008a\3\2\2\2\u008c\u008d\3\2\2\2\u008d\u008b\3\2\2\2\u008d"+
		"\u008e\3\2\2\2\u008e\u0090\3\2\2\2\u008f\u0086\3\2\2\2\u008f\u0090\3\2"+
		"\2\2\u0090\6\3\2\2\2\u0091\u0092\4\62;\2\u0092\b\3\2\2\2\u0093\u0094\7"+
		"e\2\2\u0094\u0095\7q\2\2\u0095\u0096\7u\2\2\u0096\n\3\2\2\2\u0097\u0098"+
		"\7u\2\2\u0098\u0099\7k\2\2\u0099\u009a\7p\2\2\u009a\f\3\2\2\2\u009b\u009c"+
		"\7v\2\2\u009c\u009d\7c\2\2\u009d\u009e\7p\2\2\u009e\16\3\2\2\2\u009f\u00a0"+
		"\7c\2\2\u00a0\u00a1\7v\2\2\u00a1\u00a2\7c\2\2\u00a2\u00a3\7p\2\2\u00a3"+
		"\20\3\2\2\2\u00a4\u00a5\7c\2\2\u00a5\u00a6\7e\2\2\u00a6\u00a7\7q\2\2\u00a7"+
		"\u00a8\7u\2\2\u00a8\22\3\2\2\2\u00a9\u00aa\7c\2\2\u00aa\u00ab\7u\2\2\u00ab"+
		"\u00ac\7k\2\2\u00ac\u00ad\7p\2\2\u00ad\24\3\2\2\2\u00ae\u00af\7u\2\2\u00af"+
		"\u00b0\7k\2\2\u00b0\u00b1\7p\2\2\u00b1\u00b2\7j\2\2\u00b2\26\3\2\2\2\u00b3"+
		"\u00b4\7e\2\2\u00b4\u00b5\7q\2\2\u00b5\u00b6\7u\2\2\u00b6\u00b7\7j\2\2"+
		"\u00b7\30\3\2\2\2\u00b8\u00b9\7v\2\2\u00b9\u00ba\7c\2\2\u00ba\u00bb\7"+
		"p\2\2\u00bb\u00bc\7j\2\2\u00bc\32\3\2\2\2\u00bd\u00be\7e\2\2\u00be\u00bf"+
		"\7q\2\2\u00bf\u00c0\7v\2\2\u00c0\34\3\2\2\2\u00c1\u00c2\7n\2\2\u00c2\u00c3"+
		"\7p\2\2\u00c3\36\3\2\2\2\u00c4\u00c5\7n\2\2\u00c5\u00c6\7q\2\2\u00c6\u00c7"+
		"\7i\2\2\u00c7 \3\2\2\2\u00c8\u00c9\7c\2\2\u00c9\u00ca\7d\2\2\u00ca\u00cb"+
		"\7u\2\2\u00cb\"\3\2\2\2\u00cc\u00cd\7u\2\2\u00cd\u00ce\7s\2\2\u00ce\u00cf"+
		"\7t\2\2\u00cf\u00d0\7v\2\2\u00d0$\3\2\2\2\u00d1\u00d2\7e\2\2\u00d2\u00d3"+
		"\7d\2\2\u00d3\u00d4\7t\2\2\u00d4\u00d5\7v\2\2\u00d5&\3\2\2\2\u00d6\u00d7"+
		"\7u\2\2\u00d7\u00d8\7s\2\2\u00d8\u00d9\7t\2\2\u00d9(\3\2\2\2\u00da\u00db"+
		"\7e\2\2\u00db\u00dc\7w\2\2\u00dc\u00dd\7d\2\2\u00dd\u00de\7g\2\2\u00de"+
		"*\3\2\2\2\u00df\u00e0\7e\2\2\u00e0\u00e1\7g\2\2\u00e1\u00e2\7k\2\2\u00e2"+
		"\u00e3\7n\2\2\u00e3,\3\2\2\2\u00e4\u00e5\7h\2\2\u00e5\u00e6\7n\2\2\u00e6"+
		"\u00e7\7q\2\2\u00e7\u00e8\7q\2\2\u00e8\u00e9\7t\2\2\u00e9.\3\2\2\2\u00ea"+
		"\u00eb\7o\2\2\u00eb\u00ec\7c\2\2\u00ec\u00ed\7z\2\2\u00ed\60\3\2\2\2\u00ee"+
		"\u00ef\7o\2\2\u00ef\u00f0\7k\2\2\u00f0\u00f1\7p\2\2\u00f1\62\3\2\2\2\u00f2"+
		"\u00f3\7u\2\2\u00f3\u00f4\7i\2\2\u00f4\u00f5\7p\2\2\u00f5\64\3\2\2\2\u00f6"+
		"\u00f7\7r\2\2\u00f7\u00f8\7k\2\2\u00f8\66\3\2\2\2\u00f9\u00fa\7g\2\2\u00fa"+
		"8\3\2\2\2\u00fb\u00fc\7v\2\2\u00fc\u00fd\7t\2\2\u00fd\u00fe\7w\2\2\u00fe"+
		"\u00ff\7g\2\2\u00ff:\3\2\2\2\u0100\u0101\7h\2\2\u0101\u0102\7c\2\2\u0102"+
		"\u0103\7n\2\2\u0103\u0104\7u\2\2\u0104\u0105\7g\2\2\u0105<\3\2\2\2\u0106"+
		"\u0107\7u\2\2\u0107\u0108\7w\2\2\u0108\u0109\7o\2\2\u0109>\3\2\2\2\u010a"+
		"\u010b\7r\2\2\u010b\u010c\7t\2\2\u010c\u010d\7q\2\2\u010d\u010e\7f\2\2"+
		"\u010e@\3\2\2\2\u010f\u0110\7x\2\2\u0110\u0111\7c\2\2\u0111\u0112\7t\2"+
		"\2\u0112B\3\2\2\2\u0113\u0114\7/\2\2\u0114\u0115\7@\2\2\u0115D\3\2\2\2"+
		"\u0116\u0117\7>\2\2\u0117F\3\2\2\2\u0118\u0119\7>\2\2\u0119\u011a\7?\2"+
		"\2\u011aH\3\2\2\2\u011b\u011c\7@\2\2\u011cJ\3\2\2\2\u011d\u011e\7@\2\2"+
		"\u011e\u011f\7?\2\2\u011fL\3\2\2\2\u0120\u0121\7?\2\2\u0121\u0122\7?\2"+
		"\2\u0122N\3\2\2\2\u0123\u0124\7,\2\2\u0124P\3\2\2\2\u0125\u0126\7\61\2"+
		"\2\u0126R\3\2\2\2\u0127\u0128\7\'\2\2\u0128T\3\2\2\2\u0129\u012a\7-\2"+
		"\2\u012aV\3\2\2\2\u012b\u012c\7/\2\2\u012cX\3\2\2\2\u012d\u012e\7`\2\2"+
		"\u012eZ\3\2\2\2\u012f\u0130\7*\2\2\u0130\\\3\2\2\2\u0131\u0132\7+\2\2"+
		"\u0132^\3\2\2\2\u0133\u0134\7]\2\2\u0134`\3\2\2\2\u0135\u0136\7_\2\2\u0136"+
		"b\3\2\2\2\u0137\u0138\7.\2\2\u0138d\3\2\2\2\u0139\u013a\7?\2\2\u013af"+
		"\3\2\2\2\u013b\u013c\7t\2\2\u013c\u013d\7g\2\2\u013d\u013e\7v\2\2\u013e"+
		"\u013f\7w\2\2\u013f\u0140\7t\2\2\u0140\u0141\7p\2\2\u0141h\3\2\2\2\u0142"+
		"\u0143\7-\2\2\u0143\u0144\7\61\2\2\u0144\u0145\7/\2\2\u0145j\3\2\2\2\u0146"+
		"\u0147\7=\2\2\u0147l\3\2\2\2\u0148\u014c\5o8\2\u0149\u014b\5q9\2\u014a"+
		"\u0149\3\2\2\2\u014b\u014e\3\2\2\2\u014c\u014a\3\2\2\2\u014c\u014d\3\2"+
		"\2\2\u014dn\3\2\2\2\u014e\u014c\3\2\2\2\u014f\u0154\t\3\2\2\u0150\u0154"+
		"\n\4\2\2\u0151\u0152\t\5\2\2\u0152\u0154\t\6\2\2\u0153\u014f\3\2\2\2\u0153"+
		"\u0150\3\2\2\2\u0153\u0151\3\2\2\2\u0154p\3\2\2\2\u0155\u0158\t\7\2\2"+
		"\u0156\u0158\5o8\2\u0157\u0155\3\2\2\2\u0157\u0156\3\2\2\2\u0158r\3\2"+
		"\2\2\u0159\u015a\t\b\2\2\u015a\u015b\3\2\2\2\u015b\u015c\b:\2\2\u015c"+
		"t\3\2\2\2\f\2x}\u0084\u0088\u008d\u008f\u014c\u0153\u0157\3\b\2\2";
	public static final ATN _ATN =
		new ATNDeserializer().deserialize(_serializedATN.toCharArray());
	static {
		_decisionToDFA = new DFA[_ATN.getNumberOfDecisions()];
		for (int i = 0; i < _ATN.getNumberOfDecisions(); i++) {
			_decisionToDFA[i] = new DFA(_ATN.getDecisionState(i), i);
		}
	}
}