set shell := ["pwsh", "-NoProfile", "-Command"]

# The Rust crate is the only thing that builds on this branch; the Kotlin
# implementation is being replaced and its Gradle build is intentionally broken.
# When a second crate arrives (an FFI cdylib for Artemis, say), promote this to
# a root workspace and drop this line.
set working-directory := 'crates/babel'

# What running bare `just` does.
default: build

# Also regenerates the ANTLR lexer and parser: build.rs reruns antlr4-rust-gen
# over ../../src/main/antlr/*.g4 whenever a grammar changes.
[doc("Compile the crate and every test target")]
build:
    cargo build --all-targets

# Expected to be RED for the duration of the port — every test fails on todo!()
# until the feature it covers lands.
#
# nextest rather than `cargo test` so a panic or stack overflow in one test
# doesn't take the rest of the binary with it; the AST is recursive and the
# corpus nests aggregates several deep.
[doc("Run the test suite (red by design until the port lands)")]
test *ARGS:
    cargo nextest run --no-fail-fast {{ARGS}}

# Unlike `test`, this must stay GREEN throughout: a test that fails to compile
# isn't a red test, it's an incomplete API.
[doc("Compile the tests without running them - the gate that must stay green")]
test-compile:
    cargo test --no-run --all-targets

[doc("List every test case by name, for cross-checking against the Kotlin fixtures")]
test-list:
    cargo nextest list

[doc("Apply rustfmt")]
fmt:
    cargo fmt --all

[doc("Check formatting and run clippy with warnings denied")]
lint:
    cargo fmt --all --check
    cargo clippy --all-targets --all-features -- -D warnings

[doc("Remove build artifacts")]
clean:
    cargo clean

[doc("Everything CI runs, in CI's order - red until the port is done")]
ci: lint build test-compile test
