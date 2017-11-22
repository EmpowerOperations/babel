parser grammar BabelParser;

options { tokenVocab=BabelLexer; }

@header {
   import javax.annotation.Nullable;
}

expression
    : (statement ';')* returnStatement ';'?
    EOF;

//used in validation of text fields supplied by the user
variable_only
    : variable
    EOF;

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
    : scalarExpr (lt | lteq | gt | gteq) scalarExpr
    | '(' booleanExpr ')'
    ;

scalarExpr
    : (literal | variable)
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
    locals [ @Nullable Double value = null ]
    : name '->' scalarExpr
    ;

plus : '+';
minus : '-';
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

name : VARIABLE;
variable : VARIABLE;

literal : INTEGER | FLOAT | CONSTANT ;
