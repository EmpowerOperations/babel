//! Source text to [`Ast`](crate::Ast).
//!
//! Everything here is meaning-preserving. [`parse`] lexes, parses and lowers to
//! [`crate::ast`], then the rewrites in [`rewrite`] canonicalise the tree
//! *without changing what it computes* — folding constants, inverting monotone
//! comparisons, unrolling aggregates over literal bounds, expanding whole
//! powers into multiplication.
//!
//! That is the line this module draws. A pass that makes the tree easier to
//! analyse belongs here; a pass that lowers it toward one consumer's needs
//! belongs to that consumer. `src/README.md` has the table of where each pass
//! falls and why the order between them is forced.

pub(crate) mod generated;
pub(crate) mod parse;
pub(crate) mod rewrite;

pub(crate) use parse::{parses_as_variable, translate};
