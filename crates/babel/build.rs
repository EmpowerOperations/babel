//! Generates the Babel lexer and parser from `src/main/antlr/*.g4`.
//!
//! The grammars live at the repository root rather than inside this crate so
//! that they remain the single source of truth during the port.

use antlr_rust_codegen::Builder;
use std::{env, fs, path::PathBuf};

fn main() {
    let grammars = PathBuf::from("../../src/main/antlr");
    println!("cargo:rerun-if-changed={}", grammars.display());

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("antlr");
    fs::create_dir_all(&out).expect("create OUT_DIR/antlr");

    Builder::new()
        // Lexer first: the parser's `tokenVocab` needs its token file.
        .grammars([grammars.join("BabelLexer.g4"), grammars.join("BabelParser.g4")])
        .library_directory(&out)
        .out_dir(&out)
        .generate_visitor(true)
        .generate()
        .unwrap_or_else(|e| panic!("ANTLR code generation failed: {e}"));
}
