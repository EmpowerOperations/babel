//! Babel's abstract syntax tree.
//!
//! ANTLR 4 produces a *parse* tree and offers no way to rewrite it — the tree
//! rewriting that ANTLR 3 supported was deliberately removed, and Parr's
//! recommendation for v4 is to build your own model with a visitor. This module
//! is that model. Everything downstream of [`crate::lower`] works here, not on
//! ANTLR contexts.
//!
//! Two invariants shape the design:
//!
//! 1. **Symbols are resolved during lowering**, not at evaluation time. Names
//!    become [`GlobalIdx`] or [`LocalSlot`], so shadowing (`sum(1,3,x1 -> x1) + x1`)
//!    is settled structurally and evaluation needs no scope chain.
//! 2. **Every node carries a [`Span`]**, because diagnostics are reported against
//!    arbitrary sub-expressions at both compile time and run time.

use crate::diagnostics::{Problem, ProblemKind, Span};

/// Position of a variable in the bound [`Schema`](crate::Schema)'s declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GlobalIdx(pub u32);

/// Position of a value in the current evaluation frame — a `var x = …` binding
/// or a lambda parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalSlot(pub u32);

/// A complete compiled expression.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub body: Block,
    /// Number of local slots the deepest frame requires. Lets evaluation
    /// allocate one flat frame instead of growing a scope chain.
    pub frame_size: u32,
}

/// `(statement ';')* returnStatement ';'?`
///
/// The grammar guarantees a trailing result expression, so this is a struct
/// with a required `result` rather than a list of statements that might not
/// produce a value.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub assignments: Vec<Assignment>,
    pub result: Node,
}

/// `var <name> = <expr>`
#[derive(Debug, Clone, PartialEq)]
pub struct Assignment {
    pub slot: LocalSlot,
    pub value: Node,
    pub span: Span,
}

/// An expression node: a [`Kind`] plus its source location.
///
/// The split follows `rustc_ast::Expr { kind, span, .. }` — it keeps `match`
/// arms free of span noise while guaranteeing every node has one.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub kind: Kind,
    pub span: Span,
}

impl Node {
    #[must_use]
    pub fn new(kind: Kind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Literal(f64),

    /// A statically named variable, resolved against the schema.
    Global(GlobalIdx),
    /// A `var x = …` binding or a lambda parameter.
    Local(LocalSlot),
    /// `var[expr]` — a one-based index into schema declaration order, computed
    /// at run time. This is what makes [`crate::Schema`] ordered.
    DynamicIndex(Box<Node>),

    Unary {
        op: UnaryOp,
        arg: Box<Node>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Node>,
        rhs: Box<Node>,
    },

    /// `sum(lower, upper, param -> body)` / `prod(…)`.
    Aggregate {
        kind: AggregateKind,
        lower: Box<Node>,
        upper: Box<Node>,
        param: LocalSlot,
        body: Box<Block>,
    },

    /// A multi-statement lambda body used in expression position.
    Block(Box<Block>),

    // ---- eliminated by the boolean rewrite; never reach the evaluator ----
    //
    // Kept in the same enum rather than split into a second IR type so that
    // rewrites stay composable `Node -> Node` functions. That composability is
    // what makes the rewriter pluggable (the sojourn-CVG use case); a separate
    // post-rewrite type would make every rewrite change types.
    /// `a < b`, `a >= b`, … — rewritten into arithmetic whose sign encodes the
    /// truth value: `<= 0` is true, `> 0` is false.
    Compare {
        op: CompareOp,
        lhs: Box<Node>,
        rhs: Box<Node>,
    },
    /// `a == b +/- tolerance`.
    NearEq {
        lhs: Box<Node>,
        rhs: Box<Node>,
        tolerance: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,
    Cos,
    Sin,
    Tan,
    Acos,
    Asin,
    Atan,
    Cosh,
    Sinh,
    Tanh,
    Cot,
    /// Natural logarithm — Babel's `ln`.
    Ln,
    /// Base-10 logarithm — Babel's unary `log`, overriding Java's naming.
    Log10,
    Abs,
    Sqrt,
    Cbrt,
    Sqr,
    Cube,
    Ceil,
    Floor,
    Sgn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Max,
    Min,
    /// `log(base, x)` — Babel's binary `log`.
    LogB,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Lt,
    Lte,
    Gt,
    Gte,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregateKind {
    Sum,
    Prod,
}

impl AggregateKind {
    /// The fold's identity: `0` for `sum`, `1` for `prod`.
    #[must_use]
    pub const fn identity(self) -> f64 {
        match self {
            Self::Sum => 0.0,
            Self::Prod => 1.0,
        }
    }
}

/// The single place where an `f64` becomes an integer index.
///
/// Babel conflates integer and floating-point math: `sum`/`prod` bounds and
/// `var[i]` subscripts are all `f64`. That is a wart, and giving indices a real
/// integer type is deferred — but routing every conversion through one function
/// means there is exactly one seam to cut when that happens, rather than
/// truncation logic scattered across the evaluator.
///
/// # Errors
/// Returns a [`Problem`] when the value is NaN, infinite, or outside the range
/// an index can represent.
pub fn to_index(_value: f64, _span: Span, _kind: ProblemKind) -> Result<i64, Problem> {
    todo!("V0: f64 -> index conversion, the single integer-coercion seam")
}
