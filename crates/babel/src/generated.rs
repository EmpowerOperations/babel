//! ANTLR-generated lexer and parser.
//!
//! Produced by `build.rs` from `src/main/antlr/*.g4` into `OUT_DIR`. The two
//! grammars generate independent modules with no cross-references, so each is
//! included under its own namespace.

pub(crate) mod lexer {
    include!(concat!(env!("OUT_DIR"), "/antlr/babel_lexer.rs"));
}

pub(crate) mod parser {
    include!(concat!(env!("OUT_DIR"), "/antlr/babel_parser.rs"));
}
