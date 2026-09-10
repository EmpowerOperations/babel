set shell := ["pwsh", "-NoProfile", "-Command"]

# What running bare `just` does.
default: build

# Formats first. rustfmt is idempotent, needs only a parse, and never changes
# meaning, so it belongs in the loop that runs after every edit rather than in a
# step somebody has to remember. Clippy stays out: its fixes are refactors, and
# it costs a check pass on every build.
#
# Also regenerates the ANTLR lexer and parser: build.rs reruns antlr4-rust-gen
# over grammar/*.g4 whenever a grammar changes.
[doc("Format, then compile the crate and every test target")]
build: fmt
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
    cargo test --no-run --all-targets --all-features

[doc("List every test case by name, for cross-checking against the Kotlin fixtures")]
test-list:
    cargo nextest list

[doc("Apply rustfmt; `build` runs this first")]
fmt:
    cargo fmt --all

# CI cannot write the formatting back, so it checks for drift instead. Nothing
# else needs this; everywhere else `build` formats.
[doc("Fail on formatting drift - CI's substitute for the write-through in build")]
fmt-check:
    cargo fmt --all --check

# Check-only on purpose. `cargo clippy --fix` rewrites code by compiler
# suggestion; those are refactors to read in a diff, not something a build does
# to files another session may be editing.
[doc("Clippy with warnings denied")]
lint:
    cargo clippy --all-targets --all-features -- -D warnings

[doc("Remove build artifacts")]
clean:
    cargo clean

# `--features gpu`: the GPU sieve is opt-in for consumers, but the measurements
# want it, and record "no adapter" honestly on a machine without one.
[doc("Evaluation and constraint-check throughput, in release - a debug number is meaningless here")]
bench:
    cargo nextest run --release --no-capture --features gpu -E 'binary(throughput_benchmarks) | (binary(brute_squad) & test(checks_per_second))'

# Wall-clock budgeted, so they are ignored in debug and only mean anything with
# the machine otherwise idle. Red by design until the tier each rung names lands;
# see docs/brute-squad.md.
[doc("Time to first feasible point per hit-rate rung, plus checks/s, in release")]
brute:
    cargo nextest run --release --no-capture --no-fail-fast --features gpu --test brute_squad

[doc("The test suite with the GPU sieve compiled in; its tests skip themselves without an adapter")]
test-gpu *ARGS:
    cargo nextest run --no-fail-fast --features gpu {{ARGS}}

[doc("Everything CI runs, in CI's order - red until the port is done")]
ci: fmt-check lint build test-compile test
