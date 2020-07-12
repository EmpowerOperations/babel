parser grammar BabelParser;

options { tokenVocab=BabelLexer; }

@header {
   import javax.annotation.Nullable;
   import javax.annotation.Nonnull;
}

program
    : (assignment ';')* returnStatement ';'?
    EOF;

syntheticblock
    : (assignment ';')* returnStatement ';'?
    ;

//used in validation of text fields supplied by the user
variable_only
    : variable EOF
    ;

returnStatement
    : RETURN? (booleanExpr | scalarExpr)
    ;

assignment
    : var name '=' scalarExpr
    ;

booleanExpr
    locals [ @Nullable ExprConstness availability = null ]
    : scalarExpr (lt | lteq | gt | gteq) scalarExpr
    | scalarExpr eq scalarExpr plusMinus literal
    | '(' booleanExpr ')'
    ;

scalarExpr
    locals [ @Nullable ExprConstness availability = null ]
    : (literal | variable)
    | var '[' scalarExpr ']'
    | '(' scalarExpr ')'
    | (sum | prod) '(' scalarExpr ',' scalarExpr ',' lambdaExpr ')'
    | binaryFunction '(' scalarExpr ',' scalarExpr ')'
    | unaryFunction '(' scalarExpr ')'
    | variadicFunction { int argCount = 0; } '(' scalarExpr { argCount++; } (',' scalarExpr { argCount++; })* ')'
        { _localctx.variadicFunction().argCount = argCount; }
    | negate scalarExpr
    | scalarExpr raise scalarExpr
    | scalarExpr (mult | div | mod) scalarExpr
    | scalarExpr (plus | minus) scalarExpr
    ;

lambdaExpr
    locals [ @Nullable ExprConstness availability = null ]
    : name '->' (assignment ';')* returnStatement ';'?
    ;

plus : '+';
minus : '-';
plusMinus : '+/-';
negate : '-'; //note it is legal to have to productions consuming the same token
mult : '*';
div : '/';
mod : '%';
raise : '^';
sum : 'sum';
prod : 'prod';
lt : '<';
lteq : '<=';
gt : '>';
gteq : '>=';
eq : '=' | '==' ;

var
    : 'var'
    ;

binaryFunction
    : 'log'
    ;

unaryFunction
    : 'cos' | 'sin' | 'tan'
    | 'atan' | 'acos' | 'asin'
    | 'sinh' | 'cosh' | 'tanh'
    | 'cot'
    //override Javas default log & log10 with ln & log respectively
    | 'ln' | 'log'
    | 'abs'
    | 'sqrt' | 'cbrt'
    | 'sqr' | 'cube'
    | 'ceil' | 'floor'
    | 'sgn'
    ;

variadicFunction
    locals [ int argCount = -1 ]
    : 'max'
    | 'min'
    ;

name
    locals [ VariableParseTreeLinkage linkage = BabelParserTranslations.nextVariableUID() ]
    : VARIABLE;

variable
    locals [ VariableParseTreeLinkage linkage = VariableParseTreeLinkage.NOT_LINKED.INSTANCE ]
    : VARIABLE { _localctx.linkage = BabelParserTranslations.findDeclarationSite(_localctx); };

literal : INTEGER | FLOAT | CONSTANT ;
