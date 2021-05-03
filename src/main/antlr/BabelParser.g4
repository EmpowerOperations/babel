parser grammar BabelParser;

options {
    tokenVocab=BabelLexer;
    superClass=BabelParserBase;
}

@header {
   import javax.annotation.Nullable;
   import javax.annotation.Nonnull;
}

scalar_evaluable
    : statementBlock EOF
    ;

statementBlock
    locals [ @Nonnull Availability availability = Availability.Runtime ]
    : (statement ';')* returnStatement ';'?
    ;

//used in validation of text fields supplied by the user
variable_only
    : variable EOF
    ;

statement
    : assignment
    ;

returnStatement
    : 'return'? booleanExpr
    | 'return'? scalarExpr
    ;

assignment
    : var name '=' scalarExpr
    ;

booleanExpr
//    locals [ @Nonnull Availability availability = Availability.Runtime ]
// TODO: eagerly evaluated booleans probably arent too useful without universal/existential operators
    : scalarExpr (lt | lteq | gt | gteq) scalarExpr
    | scalarExpr eq scalarExpr plusMinus literal
    | '(' booleanExpr ')'
    ;

scalarExpr
    locals [ @Nonnull Availability availability = Availability.Runtime ]
    : literal
    | variable
    | var '[' scalarExpr ']'
    | '(' scalarExpr ')'
    | (sum | prod) '(' scalarExpr ',' scalarExpr ',' lambdaExpr ')'
    | binaryFunction '(' scalarExpr ',' scalarExpr ')'
    | unaryFunction '(' scalarExpr ')'
    | negate scalarExpr
    | scalarExpr raise scalarExpr
    | scalarExpr (mult | div | mod) scalarExpr
    | scalarExpr (plus | minus) scalarExpr
    ;

lambdaExpr
    : name '->' statementBlock
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
    : 'max'
    | 'min'
    | 'log'
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

name
    locals [ @Nullable Double closedValue = null ]
    : VARIABLE
    ;

variable : VARIABLE;

literal
    locals [ Number value ]
    : '-'?
    ( INTEGER { _localctx.value = Integer.parseInt(_localctx.getText()); }
    | FLOAT { _localctx.value = Double.parseDouble(_localctx.getText()); }
    | PI { _localctx.value = Math.PI; }
    | EULERS_E { _localctx.value = Math.E; }
    )
    ;
