//! The batched executor: tiling, fault attribution, and agreement with the
//! per-lane executor.

use faer::Mat;

use super::super::tape::Shape;
use super::super::tape_for;
use super::{RegisterFile, TILE, run_tile, run_tile_on};
use crate::{EvalError, Schema};

fn runtime(result: Result<faer::Col<f64>, EvalError>) -> Box<crate::RuntimeProblem> {
    match result {
        Err(EvalError::Runtime(problem)) => problem,
        other => panic!("expected a runtime problem, got {other:?}"),
    }
}

/// Sebastiano Vigna's SplitMix64, so the random rows below are the same on
/// every machine and every `rand` version.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    #[expect(clippy::cast_precision_loss, reason = "53 bits fit a mantissa")]
    fn uniform(&mut self, low: f64, high: f64) -> f64 {
        low + (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64 * (high - low)
    }
}

/// One expression per instruction shape, over `x1..x4`.
const SOURCES: &[&str] = &[
    "x1 + x2",
    "x1 * x1 - x2 / x3",
    "x1 + x2 > 20 - x3^2",
    "sin(x1) * cos(x2) + sqrt(abs(x3))",
    "(((((x1 + x2) * x3 - x4) / (x1 + 1) + x2) * x3 - x4) / (x1 + 1) + x2) * x3 - x4",
    "max(x1, x2) - min(x3, x4)",
    "x1 == x2 +/- 0.5",
    "ln(x1) < 2",
    "x1 % x2",
    "x1 ^ x3",
    "sgn(x1) + floor(x2) + ceil(x3)",
    "sum(1, 4, i -> var[i] * i)",
    "var x = x1 * 2;\nvar y = x + x2;\ny * y - x",
    "prod(1, 3, i -> x1 + i)",
];

/// Permanent: the two executors are two loops over one instruction set, and
/// this is what says they stayed that way.
#[test]
fn the_lane_and_tile_executors_agree_bit_for_bit_on_random_inputs() {
    let mut rng = SplitMix64(0x7113_1A4E_0000_0001);
    let names = ["x1", "x2", "x3", "x4"];
    for source in SOURCES {
        let ast = crate::parse(source).expect("sources compile");
        let expression = crate::compile(&ast, &Schema::new(names)).expect("binds");
        assert_eq!(expression.tape().shape, Shape::StraightLine, "{source}");
        let rows: Vec<Vec<f64>> = (0..64)
            .map(|_| -> Vec<f64> { (0..names.len()).map(|_| rng.uniform(0.5, 10.0)).collect() })
            .filter(|row| expression.eval_row(row).is_ok())
            .collect();
        if rows.is_empty() {
            continue;
        }
        let batch = Mat::from_fn(names.len(), rows.len(), |r, c| rows[c][r]);
        let tiled = expression
            .eval(batch.as_ref())
            .expect("every row passed per lane");
        for (column, row) in rows.iter().enumerate() {
            let lane = expression.eval_row(row).unwrap();
            assert_eq!(
                tiled[column].to_bits(),
                lane.to_bits(),
                "{source:?} on {row:?}"
            );
        }
    }
}

#[test]
fn a_batched_fault_names_the_lowest_column_and_the_innermost_node() {
    let ast = crate::parse("ln(x1) + x2").unwrap();
    let expression = crate::compile(&ast, &Schema::new(["x1", "x2"])).unwrap();
    // Column 3 faults at `ln`; column 5 faults at the addition's input.
    let mut batch = Mat::from_fn(2, 8, |_, _| 1.0);
    batch[(0, 3)] = 0.0;
    batch[(1, 5)] = f64::NAN;
    let problem = runtime(expression.eval(batch.as_ref()));
    assert_eq!(problem.sample, Some(3));
    assert_eq!(problem.problem.span, crate::Span::new(0, 6));
    assert_eq!(
        problem.problem.kind,
        crate::ProblemKind::NonFiniteValue {
            value: f64::NEG_INFINITY
        }
    );
}

/// `atan` would absorb the infinity; the walker faulted on the product and so
/// must the tile, which is why every instruction is checked.
#[test]
fn an_absorbed_infinity_is_still_a_fault() {
    let ast = crate::parse("atan(x1 * x1)").unwrap();
    let expression = crate::compile(&ast, &Schema::new(["x1"])).unwrap();
    let mut batch = Mat::from_fn(1, TILE, |_, _| 1.0);
    batch[(0, 100)] = 1e200;
    let problem = runtime(expression.eval(batch.as_ref()));
    assert_eq!(problem.sample, Some(100));
    assert_eq!(problem.problem.span, crate::Span::new(5, 12));
}

#[test]
fn a_faulted_lane_does_not_disturb_its_neighbours() {
    let tape = tape_for("sqrt(x1) * 2", &["x1"]);
    let batch = Mat::from_fn(1, 4, |_, c| [4.0, -1.0, 9.0, 16.0][c]);
    let mut file = RegisterFile::new(&tape, 4);
    let mut faults = vec![None; 4];
    let fault = run_tile(&tape, batch.as_ref(), 0, 4, &mut file, &mut faults);
    assert_eq!(fault.map(|(lane, _)| lane), Some(1));
    let out = file.reg(tape.result, 4);
    assert_eq!([out[0], out[2], out[3]], [4.0, 6.0, 8.0]);
    assert!(out[1].is_nan());
}

#[test]
fn a_wide_batch_is_tiled_and_reassembled_in_order() {
    let ast = crate::parse("x1 * 2").unwrap();
    let expression = crate::compile(&ast, &Schema::new(["x1"])).unwrap();
    let columns = 2 * TILE + 7;
    #[expect(clippy::cast_precision_loss, reason = "small counts")]
    let batch = Mat::from_fn(1, columns, |_, c| c as f64);
    let residuals = expression.eval(batch.as_ref()).unwrap();
    for c in 0..columns {
        assert_eq!(residuals[c], batch[(0, c)] * 2.0);
    }
}

#[test]
fn the_sample_index_survives_tiling() {
    let ast = crate::parse("ln(x1)").unwrap();
    let expression = crate::compile(&ast, &Schema::new(["x1"])).unwrap();
    let mut batch = Mat::from_fn(1, 2 * TILE, |_, _| 1.0);
    batch[(0, TILE + 3)] = 0.0;
    assert_eq!(
        runtime(expression.eval(batch.as_ref())).sample,
        Some(TILE + 3)
    );
}

#[test]
fn an_empty_batch_yields_an_empty_column() {
    let ast = crate::parse("x1 * 2").unwrap();
    let expression = crate::compile(&ast, &Schema::new(["x1"])).unwrap();
    let batch = Mat::<f64>::zeros(1, 0);
    assert_eq!(expression.eval(batch.as_ref()).unwrap().nrows(), 0);
}

/// The vectorised `maxnum` and the scalar one may not agree on which zero
/// wins; the two executors must.
#[test]
fn max_and_min_agree_on_signed_zeros_between_executors() {
    for source in ["max(x1, x2)", "min(x1, x2)"] {
        let ast = crate::parse(source).unwrap();
        let expression = crate::compile(&ast, &Schema::new(["x1", "x2"])).unwrap();
        let rows = [[-0.0, 0.0], [0.0, -0.0], [0.0, 0.0], [-0.0, -0.0]];
        let batch = Mat::from_fn(2, rows.len(), |r, c| rows[c][r]);
        let tiled = expression.eval(batch.as_ref()).unwrap();
        for (c, row) in rows.iter().enumerate() {
            let lane = expression.eval_row(row).unwrap();
            assert_eq!(tiled[c].to_bits(), lane.to_bits(), "{source} on {row:?}");
        }
    }
}

/// The scalar backend and whatever `pulp::Arch::new()` picked run the same
/// tape to the same bits, faults included. On CI's AVX2 runners this is the
/// vector kernels against their scalar tails; on a machine without AVX2 the
/// two coincide, which is fine.
#[test]
fn the_scalar_and_dispatched_backends_agree_bit_for_bit_on_random_inputs() {
    let mut rng = SplitMix64(0x5CA1_A2AB_1E00_0001);
    let names = ["x1", "x2", "x3", "x4"];
    for source in SOURCES {
        let tape = tape_for(source, &names);
        // Wide enough to cross a tile boundary, and with a few rows pushed
        // out of every operator's domain so that faults have to agree too.
        let columns = TILE + 37;
        let batch = Mat::from_fn(names.len(), columns, |_, c| {
            if c % 41 == 0 {
                -rng.uniform(0.5, 10.0)
            } else {
                rng.uniform(0.5, 10.0)
            }
        });
        let mut detected = RegisterFile::new(&tape, columns);
        let mut scalar = RegisterFile::new(&tape, columns);
        let mut detected_faults = vec![None; columns];
        let mut scalar_faults = vec![None; columns];
        let a = run_tile_on(
            pulp::Arch::new(),
            &tape,
            batch.as_ref(),
            0,
            columns,
            &mut detected,
            &mut detected_faults,
        );
        let b = run_tile_on(
            pulp::Arch::Scalar,
            &tape,
            batch.as_ref(),
            0,
            columns,
            &mut scalar,
            &mut scalar_faults,
        );
        // Rendered, because a fault can carry a NaN and NaN is not equal to
        // itself.
        assert_eq!(
            format!("{a:?}"),
            format!("{b:?}"),
            "{source:?}: first fault"
        );
        assert_eq!(
            format!("{detected_faults:?}"),
            format!("{scalar_faults:?}"),
            "{source:?}: fault table"
        );
        let (x, y) = (
            detected.reg(tape.result, columns),
            scalar.reg(tape.result, columns),
        );
        for c in 0..columns {
            if detected_faults[c].is_none() {
                assert_eq!(x[c].to_bits(), y[c].to_bits(), "{source:?}, column {c}");
            }
        }
    }
}

// --------------------------------------------------------------- lenient

#[test]
fn a_lenient_batch_poisons_faulted_columns_and_keeps_the_rest() {
    let ast = crate::parse("ln(x1) + x2").unwrap();
    let expression = crate::compile(&ast, &Schema::new(["x1", "x2"])).unwrap();
    let mut batch = Mat::from_fn(2, 8, |r, c| {
        if r == 0 {
            [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0][c]
        } else {
            1.0
        }
    });
    batch[(0, 3)] = 0.0;
    batch[(1, 5)] = f64::NAN;
    let lenient = expression.eval_lenient(batch.as_ref()).unwrap();
    for c in 0..8 {
        if c == 3 || c == 5 {
            assert!(lenient[c].is_nan(), "column {c} should be poisoned");
        } else {
            let alone = Mat::from_fn(2, 1, |r, _| batch[(r, c)]);
            let strict = expression.eval(alone.as_ref()).unwrap()[0];
            assert_eq!(lenient[c].to_bits(), strict.to_bits(), "column {c}");
        }
    }
}

/// Decided from the fault table, not the result register: the product faulted
/// even though `atan` would have made the value finite again.
#[test]
fn a_lenient_batch_poisons_an_absorbed_infinity() {
    let ast = crate::parse("atan(x1 * x1)").unwrap();
    let expression = crate::compile(&ast, &Schema::new(["x1"])).unwrap();
    let mut batch = Mat::from_fn(1, TILE + 5, |_, _| 1.0);
    batch[(0, TILE + 2)] = 1e200;
    let lenient = expression.eval_lenient(batch.as_ref()).unwrap();
    assert!(lenient[TILE + 2].is_nan());
    assert!(lenient[TILE + 1].is_finite());
}

#[test]
fn a_lenient_loop_tape_poisons_per_column() {
    let ast = crate::parse("sum(1, x1, i -> i)").unwrap();
    let expression = crate::compile(&ast, &Schema::new(["x1"])).unwrap();
    let batch = Mat::from_fn(1, 3, |_, c| [2.0, 1.5, 3.0][c]);
    let lenient = expression.eval_lenient(batch.as_ref()).unwrap();
    assert_eq!(lenient[0], 3.0);
    assert!(lenient[1].is_nan());
    assert_eq!(lenient[2], 6.0);
}

#[test]
fn a_lenient_batch_still_rejects_a_width_mismatch() {
    let ast = crate::parse("x1 + x2").unwrap();
    let expression = crate::compile(&ast, &Schema::new(["x1", "x2"])).unwrap();
    let wrong = Mat::from_fn(3, 4, |_, _| 1.0);
    assert!(matches!(
        expression.eval_lenient(wrong.as_ref()),
        Err(EvalError::RowWidthMismatch { .. })
    ));
}
