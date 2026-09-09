//! Variable-name legality, ported from `VariableNameFixture.kt`.
//!
//! Extended beyond the Kotlin fixture with the Unicode identifiers the
//! expression corpus exercises, since `VARIABLE_START` had to be rewritten for
//! the Rust target — the JVM grammar expressed it in terms of UTF-16 surrogate
//! pairs, which Rust's `char` cannot represent.

fn assert_legal(name: &str) {
    assert!(
        babel::is_legal_variable_name(name),
        "{name:?} should be a legal variable name"
    );
}

fn assert_illegal(name: &str) {
    assert!(
        !babel::is_legal_variable_name(name),
        "{name:?} should be an illegal variable name"
    );
}

#[test]
fn ascii_names_are_legal() {
    for name in ["x1", "_name", "x_1"] {
        assert_legal(name);
    }
}

#[test]
fn unicode_names_are_legal() {
    for name in ["π", "测试", "☕", "大_da_dai_meaning_big"] {
        assert_legal(name);
    }
}

#[test]
fn bare_number_is_illegal() {
    assert_illegal("3");
}

#[test]
fn punctuation_is_illegal() {
    for name in ["$x1", "@x", r"\x", "x$"] {
        assert_illegal(name);
    }
}
