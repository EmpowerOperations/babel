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
        .grammars([
            grammars.join("BabelLexer.g4"),
            grammars.join("BabelParser.g4"),
        ])
        .library_directory(&out)
        .out_dir(&out)
        // The front end reaches typed contexts through `FromRuleNode` and walks
        // them with plain recursion, so the generated visitor is dead weight.
        .generate_visitor(false)
        .generate()
        .unwrap_or_else(|e| panic!("ANTLR code generation failed: {e}"));

    // `include!` reports a missing file readably enough on its own, but a
    // silently *empty* generation is worth catching here, where a panic surfaces
    // as a build-script error with this text attached.
    for name in ["babel_lexer.rs", "babel_parser.rs"] {
        let generated = out.join(name);
        let size = fs::metadata(&generated).map_or_else(
            |e| {
                panic!(
                    "ANTLR reported success but {} is missing ({e}). Check src/main/antlr/*.g4.",
                    generated.display()
                )
            },
            |meta| meta.len(),
        );
        assert!(
            size > 1024,
            "ANTLR generated {} but it is only {size} bytes - too small to be a working lexer or parser. Check src/main/antlr/*.g4.",
            generated.display()
        );
    }
}
