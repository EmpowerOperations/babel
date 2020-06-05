// Generated from C:/Users/geoff/Code/babel/src/main/antlr\BabelLexer.g4 by ANTLR 4.8
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
	static { RuntimeMetaData.checkVersion("4.8", RuntimeMetaData.VERSION); }

	protected static final DFA[] _decisionToDFA;
	protected static final PredictionContextCache _sharedContextCache =
		new PredictionContextCache();
	public static final int
		INTEGER=1, FLOAT=2, DIGIT=3, COS=4, SIN=5, TAN=6, ATAN=7, ACOS=8, ASIN=9, 
		SINH=10, COSH=11, TANH=12, COT=13, LN=14, LOG=15, ABS=16, SQRT=17, CBRT=18, 
		SQR=19, CUBE=20, CIEL=21, FLOOR=22, MAX=23, MIN=24, SGN=25, CONSTANT=26, 
		SUM=27, PROD=28, DYN_VAR=29, VARIABLE=30, LAMBDA=31, LT=32, LTEQ=33, GT=34, 
		GTEQ=35, EQ=36, MULT=37, DIV=38, MOD=39, PLUS=40, MINUS=41, EXPONENT=42, 
		OPEN_PAREN=43, CLOSE_PAREN=44, OPEN_BRACKET=45, CLOSE_BRACKET=46, COMMA=47, 
		ASSIGN=48, RETURN=49, PLUS_MINUS=50, EOL=51, LINEBREAKS=52;
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
			"CUBE", "CIEL", "FLOOR", "MAX", "MIN", "SGN", "CONSTANT", "SUM", "PROD", 
			"DYN_VAR", "VARIABLE", "LAMBDA", "LT", "LTEQ", "GT", "GTEQ", "EQ", "MULT", 
			"DIV", "MOD", "PLUS", "MINUS", "EXPONENT", "OPEN_PAREN", "CLOSE_PAREN", 
			"OPEN_BRACKET", "CLOSE_BRACKET", "COMMA", "ASSIGN", "RETURN", "PLUS_MINUS", 
			"EOL", "VARIABLE_START", "VARIABLE_PART", "LINEBREAKS"
		};
	}
	public static final String[] ruleNames = makeRuleNames();

	private static String[] makeLiteralNames() {
		return new String[] {
			null, null, null, null, "'cos'", "'sin'", "'tan'", "'atan'", "'acos'", 
			"'asin'", "'sinh'", "'cosh'", "'tanh'", "'cot'", "'ln'", "'log'", "'abs'", 
			"'sqrt'", "'cbrt'", "'sqr'", "'cube'", "'ceil'", "'floor'", "'max'", 
			"'min'", "'sgn'", null, "'sum'", "'prod'", "'var'", null, "'->'", "'<'", 
			"'<='", "'>'", "'>='", "'=='", "'*'", "'/'", "'%'", "'+'", "'-'", "'^'", 
			"'('", "')'", "'['", "']'", "','", "'='", "'return'", "'+/-'", "';'"
		};
	}
	private static final String[] _LITERAL_NAMES = makeLiteralNames();
	private static String[] makeSymbolicNames() {
		return new String[] {
			null, "INTEGER", "FLOAT", "DIGIT", "COS", "SIN", "TAN", "ATAN", "ACOS", 
			"ASIN", "SINH", "COSH", "TANH", "COT", "LN", "LOG", "ABS", "SQRT", "CBRT", 
			"SQR", "CUBE", "CIEL", "FLOOR", "MAX", "MIN", "SGN", "CONSTANT", "SUM", 
			"PROD", "DYN_VAR", "VARIABLE", "LAMBDA", "LT", "LTEQ", "GT", "GTEQ", 
			"EQ", "MULT", "DIV", "MOD", "PLUS", "MINUS", "EXPONENT", "OPEN_PAREN", 
			"CLOSE_PAREN", "OPEN_BRACKET", "CLOSE_BRACKET", "COMMA", "ASSIGN", "RETURN", 
			"PLUS_MINUS", "EOL", "LINEBREAKS"
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
		"\3\u608b\ua72a\u8133\ub9ed\u417c\u3be7\u7786\u5964\2\66\u014c\b\1\4\2"+
		"\t\2\4\3\t\3\4\4\t\4\4\5\t\5\4\6\t\6\4\7\t\7\4\b\t\b\4\t\t\t\4\n\t\n\4"+
		"\13\t\13\4\f\t\f\4\r\t\r\4\16\t\16\4\17\t\17\4\20\t\20\4\21\t\21\4\22"+
		"\t\22\4\23\t\23\4\24\t\24\4\25\t\25\4\26\t\26\4\27\t\27\4\30\t\30\4\31"+
		"\t\31\4\32\t\32\4\33\t\33\4\34\t\34\4\35\t\35\4\36\t\36\4\37\t\37\4 \t"+
		" \4!\t!\4\"\t\"\4#\t#\4$\t$\4%\t%\4&\t&\4\'\t\'\4(\t(\4)\t)\4*\t*\4+\t"+
		"+\4,\t,\4-\t-\4.\t.\4/\t/\4\60\t\60\4\61\t\61\4\62\t\62\4\63\t\63\4\64"+
		"\t\64\4\65\t\65\4\66\t\66\4\67\t\67\3\2\6\2q\n\2\r\2\16\2r\3\3\7\3v\n"+
		"\3\f\3\16\3y\13\3\3\3\3\3\6\3}\n\3\r\3\16\3~\3\3\3\3\5\3\u0083\n\3\3\3"+
		"\6\3\u0086\n\3\r\3\16\3\u0087\5\3\u008a\n\3\3\4\3\4\3\5\3\5\3\5\3\5\3"+
		"\6\3\6\3\6\3\6\3\7\3\7\3\7\3\7\3\b\3\b\3\b\3\b\3\b\3\t\3\t\3\t\3\t\3\t"+
		"\3\n\3\n\3\n\3\n\3\n\3\13\3\13\3\13\3\13\3\13\3\f\3\f\3\f\3\f\3\f\3\r"+
		"\3\r\3\r\3\r\3\r\3\16\3\16\3\16\3\16\3\17\3\17\3\17\3\20\3\20\3\20\3\20"+
		"\3\21\3\21\3\21\3\21\3\22\3\22\3\22\3\22\3\22\3\23\3\23\3\23\3\23\3\23"+
		"\3\24\3\24\3\24\3\24\3\25\3\25\3\25\3\25\3\25\3\26\3\26\3\26\3\26\3\26"+
		"\3\27\3\27\3\27\3\27\3\27\3\27\3\30\3\30\3\30\3\30\3\31\3\31\3\31\3\31"+
		"\3\32\3\32\3\32\3\32\3\33\3\33\3\33\5\33\u00f4\n\33\3\34\3\34\3\34\3\34"+
		"\3\35\3\35\3\35\3\35\3\35\3\36\3\36\3\36\3\36\3\37\3\37\7\37\u0105\n\37"+
		"\f\37\16\37\u0108\13\37\3 \3 \3 \3!\3!\3\"\3\"\3\"\3#\3#\3$\3$\3$\3%\3"+
		"%\3%\3&\3&\3\'\3\'\3(\3(\3)\3)\3*\3*\3+\3+\3,\3,\3-\3-\3.\3.\3/\3/\3\60"+
		"\3\60\3\61\3\61\3\62\3\62\3\62\3\62\3\62\3\62\3\62\3\63\3\63\3\63\3\63"+
		"\3\64\3\64\3\65\3\65\3\65\3\65\5\65\u0143\n\65\3\66\3\66\5\66\u0147\n"+
		"\66\3\67\3\67\3\67\3\67\2\28\3\3\5\4\7\5\t\6\13\7\r\b\17\t\21\n\23\13"+
		"\25\f\27\r\31\16\33\17\35\20\37\21!\22#\23%\24\'\25)\26+\27-\30/\31\61"+
		"\32\63\33\65\34\67\359\36;\37= ?!A\"C#E$G%I&K\'M(O)Q*S+U,W-Y.[/]\60_\61"+
		"a\62c\63e\64g\65i\2k\2m\66\3\2\t\4\2GGgg\5\2C\\aac|\4\2\2\u0101\ud802"+
		"\udc01\3\2\ud802\udc01\3\2\udc02\ue001\3\2\62;\5\2\f\f\17\17\"\"\2\u0154"+
		"\2\3\3\2\2\2\2\5\3\2\2\2\2\7\3\2\2\2\2\t\3\2\2\2\2\13\3\2\2\2\2\r\3\2"+
		"\2\2\2\17\3\2\2\2\2\21\3\2\2\2\2\23\3\2\2\2\2\25\3\2\2\2\2\27\3\2\2\2"+
		"\2\31\3\2\2\2\2\33\3\2\2\2\2\35\3\2\2\2\2\37\3\2\2\2\2!\3\2\2\2\2#\3\2"+
		"\2\2\2%\3\2\2\2\2\'\3\2\2\2\2)\3\2\2\2\2+\3\2\2\2\2-\3\2\2\2\2/\3\2\2"+
		"\2\2\61\3\2\2\2\2\63\3\2\2\2\2\65\3\2\2\2\2\67\3\2\2\2\29\3\2\2\2\2;\3"+
		"\2\2\2\2=\3\2\2\2\2?\3\2\2\2\2A\3\2\2\2\2C\3\2\2\2\2E\3\2\2\2\2G\3\2\2"+
		"\2\2I\3\2\2\2\2K\3\2\2\2\2M\3\2\2\2\2O\3\2\2\2\2Q\3\2\2\2\2S\3\2\2\2\2"+
		"U\3\2\2\2\2W\3\2\2\2\2Y\3\2\2\2\2[\3\2\2\2\2]\3\2\2\2\2_\3\2\2\2\2a\3"+
		"\2\2\2\2c\3\2\2\2\2e\3\2\2\2\2g\3\2\2\2\2m\3\2\2\2\3p\3\2\2\2\5w\3\2\2"+
		"\2\7\u008b\3\2\2\2\t\u008d\3\2\2\2\13\u0091\3\2\2\2\r\u0095\3\2\2\2\17"+
		"\u0099\3\2\2\2\21\u009e\3\2\2\2\23\u00a3\3\2\2\2\25\u00a8\3\2\2\2\27\u00ad"+
		"\3\2\2\2\31\u00b2\3\2\2\2\33\u00b7\3\2\2\2\35\u00bb\3\2\2\2\37\u00be\3"+
		"\2\2\2!\u00c2\3\2\2\2#\u00c6\3\2\2\2%\u00cb\3\2\2\2\'\u00d0\3\2\2\2)\u00d4"+
		"\3\2\2\2+\u00d9\3\2\2\2-\u00de\3\2\2\2/\u00e4\3\2\2\2\61\u00e8\3\2\2\2"+
		"\63\u00ec\3\2\2\2\65\u00f3\3\2\2\2\67\u00f5\3\2\2\29\u00f9\3\2\2\2;\u00fe"+
		"\3\2\2\2=\u0102\3\2\2\2?\u0109\3\2\2\2A\u010c\3\2\2\2C\u010e\3\2\2\2E"+
		"\u0111\3\2\2\2G\u0113\3\2\2\2I\u0116\3\2\2\2K\u0119\3\2\2\2M\u011b\3\2"+
		"\2\2O\u011d\3\2\2\2Q\u011f\3\2\2\2S\u0121\3\2\2\2U\u0123\3\2\2\2W\u0125"+
		"\3\2\2\2Y\u0127\3\2\2\2[\u0129\3\2\2\2]\u012b\3\2\2\2_\u012d\3\2\2\2a"+
		"\u012f\3\2\2\2c\u0131\3\2\2\2e\u0138\3\2\2\2g\u013c\3\2\2\2i\u0142\3\2"+
		"\2\2k\u0146\3\2\2\2m\u0148\3\2\2\2oq\5\7\4\2po\3\2\2\2qr\3\2\2\2rp\3\2"+
		"\2\2rs\3\2\2\2s\4\3\2\2\2tv\5\7\4\2ut\3\2\2\2vy\3\2\2\2wu\3\2\2\2wx\3"+
		"\2\2\2xz\3\2\2\2yw\3\2\2\2z|\7\60\2\2{}\5\7\4\2|{\3\2\2\2}~\3\2\2\2~|"+
		"\3\2\2\2~\177\3\2\2\2\177\u0089\3\2\2\2\u0080\u0082\t\2\2\2\u0081\u0083"+
		"\7/\2\2\u0082\u0081\3\2\2\2\u0082\u0083\3\2\2\2\u0083\u0085\3\2\2\2\u0084"+
		"\u0086\5\7\4\2\u0085\u0084\3\2\2\2\u0086\u0087\3\2\2\2\u0087\u0085\3\2"+
		"\2\2\u0087\u0088\3\2\2\2\u0088\u008a\3\2\2\2\u0089\u0080\3\2\2\2\u0089"+
		"\u008a\3\2\2\2\u008a\6\3\2\2\2\u008b\u008c\4\62;\2\u008c\b\3\2\2\2\u008d"+
		"\u008e\7e\2\2\u008e\u008f\7q\2\2\u008f\u0090\7u\2\2\u0090\n\3\2\2\2\u0091"+
		"\u0092\7u\2\2\u0092\u0093\7k\2\2\u0093\u0094\7p\2\2\u0094\f\3\2\2\2\u0095"+
		"\u0096\7v\2\2\u0096\u0097\7c\2\2\u0097\u0098\7p\2\2\u0098\16\3\2\2\2\u0099"+
		"\u009a\7c\2\2\u009a\u009b\7v\2\2\u009b\u009c\7c\2\2\u009c\u009d\7p\2\2"+
		"\u009d\20\3\2\2\2\u009e\u009f\7c\2\2\u009f\u00a0\7e\2\2\u00a0\u00a1\7"+
		"q\2\2\u00a1\u00a2\7u\2\2\u00a2\22\3\2\2\2\u00a3\u00a4\7c\2\2\u00a4\u00a5"+
		"\7u\2\2\u00a5\u00a6\7k\2\2\u00a6\u00a7\7p\2\2\u00a7\24\3\2\2\2\u00a8\u00a9"+
		"\7u\2\2\u00a9\u00aa\7k\2\2\u00aa\u00ab\7p\2\2\u00ab\u00ac\7j\2\2\u00ac"+
		"\26\3\2\2\2\u00ad\u00ae\7e\2\2\u00ae\u00af\7q\2\2\u00af\u00b0\7u\2\2\u00b0"+
		"\u00b1\7j\2\2\u00b1\30\3\2\2\2\u00b2\u00b3\7v\2\2\u00b3\u00b4\7c\2\2\u00b4"+
		"\u00b5\7p\2\2\u00b5\u00b6\7j\2\2\u00b6\32\3\2\2\2\u00b7\u00b8\7e\2\2\u00b8"+
		"\u00b9\7q\2\2\u00b9\u00ba\7v\2\2\u00ba\34\3\2\2\2\u00bb\u00bc\7n\2\2\u00bc"+
		"\u00bd\7p\2\2\u00bd\36\3\2\2\2\u00be\u00bf\7n\2\2\u00bf\u00c0\7q\2\2\u00c0"+
		"\u00c1\7i\2\2\u00c1 \3\2\2\2\u00c2\u00c3\7c\2\2\u00c3\u00c4\7d\2\2\u00c4"+
		"\u00c5\7u\2\2\u00c5\"\3\2\2\2\u00c6\u00c7\7u\2\2\u00c7\u00c8\7s\2\2\u00c8"+
		"\u00c9\7t\2\2\u00c9\u00ca\7v\2\2\u00ca$\3\2\2\2\u00cb\u00cc\7e\2\2\u00cc"+
		"\u00cd\7d\2\2\u00cd\u00ce\7t\2\2\u00ce\u00cf\7v\2\2\u00cf&\3\2\2\2\u00d0"+
		"\u00d1\7u\2\2\u00d1\u00d2\7s\2\2\u00d2\u00d3\7t\2\2\u00d3(\3\2\2\2\u00d4"+
		"\u00d5\7e\2\2\u00d5\u00d6\7w\2\2\u00d6\u00d7\7d\2\2\u00d7\u00d8\7g\2\2"+
		"\u00d8*\3\2\2\2\u00d9\u00da\7e\2\2\u00da\u00db\7g\2\2\u00db\u00dc\7k\2"+
		"\2\u00dc\u00dd\7n\2\2\u00dd,\3\2\2\2\u00de\u00df\7h\2\2\u00df\u00e0\7"+
		"n\2\2\u00e0\u00e1\7q\2\2\u00e1\u00e2\7q\2\2\u00e2\u00e3\7t\2\2\u00e3."+
		"\3\2\2\2\u00e4\u00e5\7o\2\2\u00e5\u00e6\7c\2\2\u00e6\u00e7\7z\2\2\u00e7"+
		"\60\3\2\2\2\u00e8\u00e9\7o\2\2\u00e9\u00ea\7k\2\2\u00ea\u00eb\7p\2\2\u00eb"+
		"\62\3\2\2\2\u00ec\u00ed\7u\2\2\u00ed\u00ee\7i\2\2\u00ee\u00ef\7p\2\2\u00ef"+
		"\64\3\2\2\2\u00f0\u00f1\7r\2\2\u00f1\u00f4\7k\2\2\u00f2\u00f4\7g\2\2\u00f3"+
		"\u00f0\3\2\2\2\u00f3\u00f2\3\2\2\2\u00f4\66\3\2\2\2\u00f5\u00f6\7u\2\2"+
		"\u00f6\u00f7\7w\2\2\u00f7\u00f8\7o\2\2\u00f88\3\2\2\2\u00f9\u00fa\7r\2"+
		"\2\u00fa\u00fb\7t\2\2\u00fb\u00fc\7q\2\2\u00fc\u00fd\7f\2\2\u00fd:\3\2"+
		"\2\2\u00fe\u00ff\7x\2\2\u00ff\u0100\7c\2\2\u0100\u0101\7t\2\2\u0101<\3"+
		"\2\2\2\u0102\u0106\5i\65\2\u0103\u0105\5k\66\2\u0104\u0103\3\2\2\2\u0105"+
		"\u0108\3\2\2\2\u0106\u0104\3\2\2\2\u0106\u0107\3\2\2\2\u0107>\3\2\2\2"+
		"\u0108\u0106\3\2\2\2\u0109\u010a\7/\2\2\u010a\u010b\7@\2\2\u010b@\3\2"+
		"\2\2\u010c\u010d\7>\2\2\u010dB\3\2\2\2\u010e\u010f\7>\2\2\u010f\u0110"+
		"\7?\2\2\u0110D\3\2\2\2\u0111\u0112\7@\2\2\u0112F\3\2\2\2\u0113\u0114\7"+
		"@\2\2\u0114\u0115\7?\2\2\u0115H\3\2\2\2\u0116\u0117\7?\2\2\u0117\u0118"+
		"\7?\2\2\u0118J\3\2\2\2\u0119\u011a\7,\2\2\u011aL\3\2\2\2\u011b\u011c\7"+
		"\61\2\2\u011cN\3\2\2\2\u011d\u011e\7\'\2\2\u011eP\3\2\2\2\u011f\u0120"+
		"\7-\2\2\u0120R\3\2\2\2\u0121\u0122\7/\2\2\u0122T\3\2\2\2\u0123\u0124\7"+
		"`\2\2\u0124V\3\2\2\2\u0125\u0126\7*\2\2\u0126X\3\2\2\2\u0127\u0128\7+"+
		"\2\2\u0128Z\3\2\2\2\u0129\u012a\7]\2\2\u012a\\\3\2\2\2\u012b\u012c\7_"+
		"\2\2\u012c^\3\2\2\2\u012d\u012e\7.\2\2\u012e`\3\2\2\2\u012f\u0130\7?\2"+
		"\2\u0130b\3\2\2\2\u0131\u0132\7t\2\2\u0132\u0133\7g\2\2\u0133\u0134\7"+
		"v\2\2\u0134\u0135\7w\2\2\u0135\u0136\7t\2\2\u0136\u0137\7p\2\2\u0137d"+
		"\3\2\2\2\u0138\u0139\7-\2\2\u0139\u013a\7\61\2\2\u013a\u013b\7/\2\2\u013b"+
		"f\3\2\2\2\u013c\u013d\7=\2\2\u013dh\3\2\2\2\u013e\u0143\t\3\2\2\u013f"+
		"\u0143\n\4\2\2\u0140\u0141\t\5\2\2\u0141\u0143\t\6\2\2\u0142\u013e\3\2"+
		"\2\2\u0142\u013f\3\2\2\2\u0142\u0140\3\2\2\2\u0143j\3\2\2\2\u0144\u0147"+
		"\t\7\2\2\u0145\u0147\5i\65\2\u0146\u0144\3\2\2\2\u0146\u0145\3\2\2\2\u0147"+
		"l\3\2\2\2\u0148\u0149\t\b\2\2\u0149\u014a\3\2\2\2\u014a\u014b\b\67\2\2"+
		"\u014bn\3\2\2\2\r\2rw~\u0082\u0087\u0089\u00f3\u0106\u0142\u0146\3\b\2"+
		"\2";
	public static final ATN _ATN =
		new ATNDeserializer().deserialize(_serializedATN.toCharArray());
	static {
		_decisionToDFA = new DFA[_ATN.getNumberOfDecisions()];
		for (int i = 0; i < _ATN.getNumberOfDecisions(); i++) {
			_decisionToDFA[i] = new DFA(_ATN.getDecisionState(i), i);
		}
	}
}