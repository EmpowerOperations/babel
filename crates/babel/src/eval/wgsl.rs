//! The tape as WGSL: the GPU sieve's kernel, one function per constraint.
//!
//! The third backend over the tape, beside the tile and lane executors, and
//! the same shape as the SMT-LIB emitter: a small function that walks the
//! instructions and writes text, which a driver then compiles. It exists
//! because the kernel comes from a *user's expression at run time*; every
//! compile-Rust-to-GPU project wants the function known at build time, and
//! every driver already ships a JIT for shader source.
//!
//! # What the text promises, and what it does not
//!
//! **The GPU is a sieve, never a judge.** The function computes the residual
//! in `f32` and the harness keeps every candidate whose residual is within
//! [`SIEVE_SLACK`] of feasible; the CPU then re-judges the survivors in `f64`
//! against the real tape. So the text here may *miss* a feasible point — a
//! false negative costs hit rate — but nothing it keeps is ever delivered on
//! its say-so. That is the whole reason it can be loose about things the CPU
//! tape is strict about:
//!
//! - A non-finite intermediate is not detected as such. Shader compilers
//!   assume no NaNs (this laptop's driver evaluates `NaN <= 0` as *true*), so
//!   the emitter does not rely on propagation: every operator with a domain
//!   — `sqrt`, `ln`, `log`, `acos`, `asin`, division, `%` — is guarded by a
//!   comparison on its finite input, which fast-math respects, and a
//!   candidate outside the domain returns [`FAULT`] exactly as the CPU tape
//!   faults. Overflow to infinity is left to arithmetic: `inf` compares
//!   sanely, and an `inf - inf` NaN is a false positive the CPU re-check
//!   removes. `Check` emits nothing.
//! - A constant outside `f32` range becomes `±f32::MAX`. The CPU tape faults
//!   on the overflow such a constant produces; the sieve drops the candidate
//!   for a different reason and the outcome is the same.
//! - `pow` with a negative base is NaN on the GPU even for an integer
//!   exponent, where the CPU's `powf` is not. The front end expands constant
//!   integer exponents into multiplications before either sees them, so this
//!   only reaches a run-time exponent, where the CPU is NaN too.
//! - `max`/`min` do not carry Java's signed-zero rule. Irrelevant to a sign.
//!
//! **A subscript out of range is a fault**, and a fault is a candidate the
//! constraint does not hold for: the function returns [`FAULT`], a residual
//! nothing keeps.

// Built and tested with or without the `gpu` feature — naga validates the text
// on any machine — but only the sieve calls it.
#![cfg_attr(
    not(feature = "gpu"),
    allow(
        dead_code,
        reason = "the emitter's only production caller is the GPU sieve"
    )
)]

use std::fmt::Write as _;

use crate::ast::{BinaryOp, CompareOp, UnaryOp};

use super::tape::{Accumulate, IRTape, Instruction};

/// How far past feasible, relative to the magnitudes compared, the sieve still
/// keeps a candidate.
///
/// `f32` carries 24 bits, so one rounding is about `6e-8` relative; a tape of
/// a few dozen operations and a transcendental or two on the special function
/// units accumulates a few hundred of those at most. A hundredth of a percent
/// is generous — the survivors are re-judged exactly, so generosity costs a
/// little CPU and nothing else, where meanness costs hits. The test that
/// pins it draws a hundred thousand candidates per problem and requires that
/// nothing feasible was dropped and that survivors stay within a small
/// multiple of the feasible count.
pub(crate) const SIEVE_SLACK: f32 = 1e-4;

/// The residual a function returns for a candidate the CPU would fault on.
/// The largest finite `f32`, so it survives `max` with anything finite and
/// fails `<= slack` with everything. Not infinity: WGSL has no literal for it.
pub(crate) const FAULT: f32 = f32::MAX;

/// The helpers every emitted function calls. Emit once per shader, before any
/// function.
pub(crate) fn prelude() -> String {
    format!(
        "const BABEL_SLACK: f32 = {slack:?};\n\
         const BABEL_FAULT: f32 = {fault:?};\n\
         fn babel_slack(a: f32, b: f32) -> f32 {{\n    return BABEL_SLACK * (abs(a) + abs(b));\n}}\n",
        slack = SIEVE_SLACK,
        fault = FAULT,
    )
}

/// Renders `tape` as one WGSL function `name`, over a pointer to an array of
/// `inputs` f32 values, returning the f32 residual.
///
/// The register file is a local array indexed by constants, which every
/// shader compiler keeps in registers. One statement per instruction; the
/// text is meant to be read by the driver and, when something is wrong, by a
/// person.
pub(crate) fn emit_function(tape: &IRTape, name: &str, inputs: usize) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "fn {name}(x: ptr<function, array<f32, {inputs}>>) -> f32 {{"
    );
    let _ = writeln!(out, "    var r: array<f32, {}>;", tape.registers.max(1));
    for (index, constant) in tape.consts.iter().enumerate() {
        let _ = writeln!(out, "    r[{index}] = {};", literal(*constant));
    }

    for insn in &tape.insns {
        match *insn {
            Instruction::Load { dst, input } => {
                let _ = writeln!(out, "    r[{}] = (*x)[{input}];", dst.index());
            }
            Instruction::Copy { dst, src } => {
                let _ = writeln!(out, "    r[{}] = r[{}];", dst.index(), src.index());
            }
            Instruction::Unary { dst, op, a } => {
                let a = reg(a.index());
                if let Some(outside) = unary_domain(op, &a) {
                    let _ = writeln!(
                        out,
                        "    if {outside} {{
        return BABEL_FAULT;
    }}"
                    );
                }
                let _ = writeln!(out, "    r[{}] = {};", dst.index(), unary(op, &a));
            }
            Instruction::Binary { dst, op, a, b } => {
                let (a, b) = (reg(a.index()), reg(b.index()));
                if let Some(outside) = binary_domain(op, &a, &b) {
                    let _ = writeln!(
                        out,
                        "    if {outside} {{
        return BABEL_FAULT;
    }}"
                    );
                }
                let _ = writeln!(out, "    r[{}] = {};", dst.index(), binary(op, &a, &b));
            }
            Instruction::Compare { dst, op, a, b } => {
                let (a, b) = (reg(a.index()), reg(b.index()));
                // The `<= 0` residual, widened by the slack. `Lt`/`Gt` add
                // `f64::MIN_POSITIVE` on the CPU, which is zero in f32 and
                // dwarfed by the slack anyway.
                let residual = match op {
                    CompareOp::Lte | CompareOp::Lt => format!("({a} - {b})"),
                    CompareOp::Gte | CompareOp::Gt => format!("({b} - {a})"),
                };
                let _ = writeln!(
                    out,
                    "    r[{}] = {residual} - babel_slack({a}, {b});",
                    dst.index()
                );
            }
            Instruction::NearEq {
                dst,
                a,
                b,
                tolerance,
            } => {
                let (a, b, t) = (reg(a.index()), reg(b.index()), reg(tolerance.index()));
                let _ = writeln!(
                    out,
                    "    r[{}] = max(({b} - {t}) - {a}, {a} - ({b} + {t})) - babel_slack({a}, {b}) - babel_slack({t}, 0.0);",
                    dst.index()
                );
            }
            Instruction::Combine { dst, how, a, b, .. } => {
                let (a, b) = (reg(a.index()), reg(b.index()));
                let combined = match how {
                    Accumulate::Sum => format!("{a} + {b}"),
                    Accumulate::Prod => format!("{a} * {b}"),
                    Accumulate::Worst => format!("max({a}, {b})"),
                };
                let _ = writeln!(out, "    r[{}] = {combined};", dst.index());
            }
            // A non-finite value poisons the residual and never passes the
            // sieve; there is nothing to check.
            Instruction::Check { .. } => {}
            Instruction::Gather { dst, index, .. } => {
                // One-based, integral, in range — the same three conditions
                // `resolve_index` applies on the CPU, and the same verdict for
                // a miss: the constraint does not hold.
                let g = reg(index.index());
                let _ = writeln!(
                    out,
                    "    {{\n        let gi = i32({g}) - 1;\n        if gi < 0 || gi >= {inputs} || f32(gi + 1) != {g} {{\n            return BABEL_FAULT;\n        }}\n        r[{}] = (*x)[u32(gi)];\n    }}",
                    dst.index()
                );
            }
        }
    }

    let _ = writeln!(out, "    return r[{}];\n}}", tape.result.index());
    out
}

fn reg(index: usize) -> String {
    format!("r[{index}]")
}

/// An `f64` constant as an `f32` WGSL literal. Beyond `f32` range it becomes
/// `±f32::MAX`; see the module docs for why that is the same outcome.
fn literal(value: f64) -> String {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the sieve is f32 by design; precision loss is the documented trade"
    )]
    let narrowed = value as f32;
    let finite = if narrowed.is_infinite() {
        f32::MAX.copysign(narrowed)
    } else {
        narrowed
    };
    format!("{finite:?}")
}

/// The condition under which `op` leaves its domain and the CPU tape would
/// fault on the result — a comparison on the finite input, which a fast-math
/// compiler cannot fold away. `None` for operators that are total or only
/// overflow.
fn unary_domain(op: UnaryOp, a: &str) -> Option<String> {
    match op {
        UnaryOp::Sqrt => Some(format!("{a} < 0.0")),
        UnaryOp::Ln | UnaryOp::Log10 => Some(format!("{a} <= 0.0")),
        UnaryOp::Acos | UnaryOp::Asin => Some(format!("abs({a}) > 1.0")),
        // `1 / tan(0)` is infinite on both sides; infinity compares sanely.
        UnaryOp::Negate
        | UnaryOp::Cos
        | UnaryOp::Sin
        | UnaryOp::Tan
        | UnaryOp::Atan
        | UnaryOp::Cosh
        | UnaryOp::Sinh
        | UnaryOp::Tanh
        | UnaryOp::Cot
        | UnaryOp::Abs
        | UnaryOp::Cbrt
        | UnaryOp::Sqr
        | UnaryOp::Cube
        | UnaryOp::Ceil
        | UnaryOp::Floor
        | UnaryOp::Sgn => None,
    }
}

/// [`unary_domain`]'s binary twin. `pow` with a negative base and a
/// non-integral exponent is NaN on both sides; with an integral exponent the
/// GPU is NaN where the CPU is not, a false negative documented above.
fn binary_domain(op: BinaryOp, a: &str, b: &str) -> Option<String> {
    match op {
        BinaryOp::Div | BinaryOp::Rem => Some(format!("{b} == 0.0")),
        BinaryOp::Pow => Some(format!("{a} < 0.0 && {b} != floor({b})")),
        BinaryOp::LogB => Some(format!("{a} <= 0.0 || {b} <= 0.0 || {a} == 1.0")),
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Max | BinaryOp::Min => None,
    }
}

fn unary(op: UnaryOp, a: &str) -> String {
    match op {
        UnaryOp::Negate => format!("-({a})"),
        UnaryOp::Cos => format!("cos({a})"),
        UnaryOp::Sin => format!("sin({a})"),
        UnaryOp::Tan => format!("tan({a})"),
        UnaryOp::Acos => format!("acos({a})"),
        UnaryOp::Asin => format!("asin({a})"),
        UnaryOp::Atan => format!("atan({a})"),
        UnaryOp::Cosh => format!("cosh({a})"),
        UnaryOp::Sinh => format!("sinh({a})"),
        UnaryOp::Tanh => format!("tanh({a})"),
        UnaryOp::Cot => format!("1.0 / tan({a})"),
        UnaryOp::Ln => format!("log({a})"),
        UnaryOp::Log10 => format!("log({a}) / {:?}", std::f32::consts::LN_10),
        UnaryOp::Abs => format!("abs({a})"),
        UnaryOp::Sqrt => format!("sqrt({a})"),
        UnaryOp::Cbrt => format!("sign({a}) * pow(abs({a}), {:?})", 1.0f32 / 3.0),
        UnaryOp::Sqr => format!("{a} * {a}"),
        UnaryOp::Cube => format!("{a} * {a} * {a}"),
        UnaryOp::Ceil => format!("ceil({a})"),
        UnaryOp::Floor => format!("floor({a})"),
        UnaryOp::Sgn => format!("sign({a})"),
    }
}

fn binary(op: BinaryOp, a: &str, b: &str) -> String {
    match op {
        BinaryOp::Add => format!("{a} + {b}"),
        BinaryOp::Sub => format!("{a} - {b}"),
        BinaryOp::Mul => format!("{a} * {b}"),
        BinaryOp::Div => format!("{a} / {b}"),
        // WGSL's `%` on floats is the truncated remainder, as Rust's is.
        BinaryOp::Rem => format!("{a} % {b}"),
        BinaryOp::Pow => format!("pow({a}, {b})"),
        BinaryOp::Max => format!("max({a}, {b})"),
        BinaryOp::Min => format!("min({a}, {b})"),
        // `log(a, b)` is the log of `b` to base `a`: `BinaryOp::apply` is `b.ln() / a.ln()`.
        BinaryOp::LogB => format!("log({b}) / log({a})"),
    }
}

#[cfg(test)]
mod tests {
    //! No GPU here: the text is validated by naga, wgpu's own shader
    //! front end, which checks syntax, types, arities and scoping. Whether
    //! the function *computes* the right residual is the sieve's test, which
    //! needs an adapter.

    use super::{emit_function, prelude};
    use crate::Schema;

    /// Constraints that between them use every instruction and most
    /// operators: the three rung families, a fault, a gather, an equality,
    /// an aggregate, and the transcendentals the GPU is there for.
    const CORPUS: &[&str] = &[
        "x1 > 0.9995",
        "x1^2 + x2^2 + x3^2 < 0.0001",
        "sin(x1) > sin(0.9995)",
        "sqrt(x1 - 5) + x1 < 6",
        "var[ceil(x2 * 2)] > 0.5",
        "x1 == pi +/- 0.001",
        "sum(1, 3, i -> var[i] * var[i]) < 1.5",
        "ln(x1) < 2",
        "log(x2, 10) > -3",
        "cbrt(x3) < 0.9",
        "x1 % 0.3 < 0.1",
        "cot(x2) > 1",
        "tanh(x3) - sinh(x1) + cosh(x2) < 0.5",
        "max(x1, x2) - min(x2, x3) > 0.1",
        "abs(floor(x1) - ceil(x2)) < 2",
        "sgn(x3) > 0",
        "acos(x1) + asin(x2) + atan(x3) + tan(x1) + cos(x2) > 1",
        "x1 ^ x2 < 0.5",
        "-x1 < -0.5",
    ];

    fn schema() -> Schema {
        Schema::new(["x1", "x2", "x3"])
    }

    fn shader(sources: &[&str]) -> String {
        let schema = schema();
        let mut text = prelude();
        for (index, source) in sources.iter().enumerate() {
            let ast = crate::parse(source).unwrap_or_else(|e| panic!("{source:?}: {e}"));
            let compiled =
                crate::compile(&ast, &schema).unwrap_or_else(|e| panic!("{source:?}: {e}"));
            text.push_str(&emit_function(&compiled.tape, &format!("c{index}"), 3));
        }
        text
    }

    fn validate(text: &str) {
        let module = naga::front::wgsl::parse_str(text)
            .unwrap_or_else(|e| panic!("{}\n---\n{text}", e.emit_to_string(text)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:?}\n---\n{text}"));
    }

    #[test]
    fn every_corpus_constraint_emits_a_shader_naga_accepts() {
        validate(&shader(CORPUS));
    }

    #[test]
    fn each_constraint_validates_alone_too() {
        // So a failure names the constraint rather than the corpus.
        for source in CORPUS {
            validate(&shader(&[source]));
        }
    }

    #[test]
    fn a_constant_beyond_f32_becomes_the_largest_finite_and_still_validates() {
        let text = shader(&["x1 * 1.0e300 < 1"]);
        assert!(text.contains("3.4028235e38"), "{text}");
        assert!(!text.contains("inf"), "{text}");
        validate(&text);
    }

    #[test]
    fn a_comparison_carries_the_slack_and_a_gather_can_fault() {
        let text = shader(&["var[ceil(x2 * 2)] > 0.5"]);
        assert!(text.contains("babel_slack("), "{text}");
        assert!(text.contains("return BABEL_FAULT;"), "{text}");
    }
}
