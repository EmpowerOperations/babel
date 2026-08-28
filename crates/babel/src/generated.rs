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

/// What the front end requires the generator to have produced.
///
/// Binding these as typed values means a change in the generated surface fails
/// *here*, next to a description of what was expected, rather than as a scatter
/// of "cannot find" errors across the front end. It compiles to nothing.
///
/// Version skew between runtime and generator is already caught separately: the
/// generated modules open with `__antlr4_rust_require_codegen_api!`.
const _: () = {
    // Never called — it exists to be type-checked.
    #[allow(dead_code, clippy::items_after_statements)]
    fn contract() {
        // Entry rules the front end parses from.
        let _: fn(
            &mut parser::BabelParser<lexer::BabelLexer<antlr4_runtime::InputStream>>,
        ) -> Result<antlr4_runtime::ParseTree, antlr4_runtime::AntlrError> =
            parser::BabelParser::scalar_evaluable;
        let _: fn(
            &mut parser::BabelParser<lexer::BabelLexer<antlr4_runtime::InputStream>>,
        ) -> Result<antlr4_runtime::ParseTree, antlr4_runtime::AntlrError> =
            parser::BabelParser::variable_only;

        // The typed root, reached via `FromRuleNode` rather than the visitor.
        fn root<'a>(
            node: antlr4_runtime::RuleNodeView<'a>,
        ) -> Option<parser::ScalarEvaluableContext<'a>> {
            antlr4_runtime::FromRuleNode::from_rule_node(node)
        }
        let _ = root;
    }
};
