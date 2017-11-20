parser grammar BabelParser;

options { tokenVocab=BabelLexer; }

expression
    : expr EOF;

//used in validation of text fields supplied by the user
variable_only
    : variable EOF;

expr
    : (literal | variable)
    | var '[' expr ']'
    | '(' expr ')'
    | (sum | prod) '(' expr ',' expr ',' lambdaExpr ')'
    | binaryFunction '(' expr ',' expr ')'
    | unaryFunction '(' expr ')'
    | negate expr
    | expr raise expr
    | expr (mult | div | mod) expr
    | expr (plus | minus) expr
    | expr (lt | lteq | gt | gteq) expr
    ;

lambdaExpr
    : name '->' expr
    ;

_inScope
    : literal lambdaExpr

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

literal : (INTEGER | FLOAT) | CONSTANT ;
