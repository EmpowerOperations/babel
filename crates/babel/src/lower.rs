//! Lowers the ANTLR parse tree into [`crate::ast`].
//!
//! This is where name resolution happens: each statically-referenced name is
//! assigned a [`GlobalIdx`](crate::ast::GlobalIdx), and each `var x = …`
//! binding or lambda parameter is assigned a
//! [`LocalSlot`](crate::ast::LocalSlot). Shadowing is therefore settled here,
//! structurally, and evaluation needs no scope chain.

#![allow(dead_code)] // V0 scaffolding; remove once `compile` is wired up.

use crate::ast::Program;
use crate::diagnostics::Problem;

/// Everything compilation learns from walking the parse tree.
pub(crate) struct Lowered {
    pub program: Program,
    /// Distinct statically-referenced names in first-reference order.
    pub symbols: Vec<String>,
    pub contains_dynamic_lookup: bool,
    pub is_boolean_expression: bool,
}

/// Walks a parsed `scalar_evaluable` tree and produces the AST.
///
/// # Errors
/// Returns every problem found rather than stopping at the first.
pub(crate) fn lower(
    _source: &str,
    _tree: &antlr4_runtime::ParseTree,
) -> Result<Lowered, Vec<Problem>> {
    todo!("V0")
}
