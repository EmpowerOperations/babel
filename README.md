# Babel

A small expression language for optimizer formulations. `x1 + x2 * cos(x3)^2` is a
transform; `x1 < x2 + x3` is a constraint. The crate parses expressions, evaluates them
in batches, and generates random points that satisfy a set of constraints (the `cvg`
module, a Z3-backed constrained random vector generator).

```rust
let ast = babel::parse("x1 + x2 > 20 - x3^2")?;
let compiled = babel::compile(&ast, &Schema::new(["x1", "x2", "x3"]))?;

// One column per sample, one row per schema variable.
let residuals = compiled.eval(samples.as_ref())?;
```

Babel is consumed as a cargo git dependency; it is not on crates.io.

## Building

Everything is a [`just`](https://github.com/casey/just) recipe, and CI runs exactly `just ci`.

```
just build          compile the crate and every test target
just test           run the test suite with nextest
just lint           rustfmt and clippy, warnings denied
```

The lexer and parser are generated from [`grammar/`](grammar) by `build.rs` at build time.
The GPU sieve is behind the `gpu` feature and off by default.

## Reading further

- [`src/README.md`](src/README.md): the architecture, one AST and two backends.
- [`AGENTS.md`](AGENTS.md): conventions, layout, and the reading order for the design notes.
- [`docs/todo.md`](docs/todo.md): the roadmap and the reasoning behind the decisions.

## License

Apache-2.0. See [LICENSE](LICENSE). The constrained random vector generator descends from
the Apache-2 [sojourn-CVG](https://github.com/Groostav/sojourn-CVG) project; its original
notes are under [`docs/sojourn/`](docs/sojourn).
