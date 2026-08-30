//! How fast does babel evaluate?
//!
//! Every performance decision in this port has so far been made blind, and the
//! tape / SIMD work coming next is exactly the kind that needs a number before
//! and after. This is that number.
//!
//! Run it with `just bench`. Under a plain `cargo nextest run` it does a token
//! amount of work and prints nothing useful — see "Two ways to get a lie",
//! below.
//!
//! # Two paths, because they are very different
//!
//! * [`Bound::evaluate`] takes a row of `f64` — bind once, then evaluate. What
//!   Artemis will use.
//! * [`Expression::evaluate`] takes name-value pairs and rebuilds a `Schema`,
//!   binds, and allocates a row **on every call**. Convenient, and not for use
//!   in a loop.
//!
//! Reporting both separates "the evaluator is fast" from "the API is fast". The
//! gap between them is the cost of the convenience, and it grows with the number
//! of variables, because that is what the per-call `Schema` is built out of.
//!
//! # Two ways to get a lie
//!
//! **Measuring a debug build.** Rust debug is an order of magnitude slower than
//! release and the difference is not a constant factor. The workload scales off
//! `cfg!(debug_assertions)` so this still runs in CI for nothing, and the header
//! states the profile so a pasted number cannot be misread.
//!
//! **Measuring nothing at all.** A release build deletes a loop whose result is
//! unused. Every result is accumulated and the accumulator is passed through
//! `black_box`, and `every_case_costs_what_it_should` checks the shape of the
//! results afterwards — if three transcendentals run as fast as one addition,
//! the loop is not happening.

use std::hint::black_box;
use std::time::{Duration, Instant};

use babel::{Expression, Schema};
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

/// How long to run each case before reporting. Small enough in debug that a
/// normal test run barely notices.
const TARGET: Duration = if cfg!(debug_assertions) {
    Duration::from_millis(20)
} else {
    Duration::from_millis(400)
};

/// Best of this many, because scheduling noise only ever makes things slower.
const REPETITIONS: usize = 3;

/// How many distinct input rows to cycle through.
///
/// More than one so the optimiser cannot hoist the evaluation, and few enough to
/// stay in cache — this is measuring the evaluator, not the memory subsystem.
const ROWS: usize = 256;

/// Evaluations between clock reads. Amortises `Instant::now`, which is not free
/// and would otherwise dominate the cheapest case.
const CHUNK: usize = 64;

struct Case {
    name: &'static str,
    source: &'static str,
    variables: usize,
}

/// Increasing cost. Two are lifted verbatim from the JVM's `PerformanceFixture`
/// so the comparison is direct rather than approximate.
fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "trivial",
            source: "x1 + x2",
            variables: 2,
        },
        Case {
            // From the JVM fixture.
            name: "small (jvm)",
            source: "x1 + x2 > 20 - x3^2",
            variables: 3,
        },
        Case {
            name: "transcendental",
            source: "sin(x1) * cos(x2) + sqrt(abs(x3))",
            variables: 3,
        },
        Case {
            name: "deep arithmetic",
            source: "(((((x1 + x2) * x3 - x4) / (x1 + 1) + x2) * x3 - x4) / (x1 + 1) + x2) \
                     * x3 - x4 + ((x1 * x2) - (x3 / (x4 + 1)))^2",
            variables: 4,
        },
        Case {
            // From the JVM fixture. Constant bounds, so `unroll_aggregates`
            // flattens it to a single 200-term fold before the evaluator ever
            // sees it — worth knowing before comparing this row to the JVM's.
            name: "200-var sum (jvm)",
            source: "sum(1, 200, i -> var[i]^2 - 3.0)",
            variables: 200,
        },
    ]
}

/// Runs `evaluate` until `TARGET` has elapsed, returning evaluations per
/// millisecond.
///
/// Calibrated by time rather than by a fixed iteration count, because the cases
/// span two orders of magnitude and any count that suits one would be absurd for
/// another.
fn throughput(mut evaluate: impl FnMut(usize) -> f64) -> f64 {
    let mut best = 0.0f64;

    for _ in 0..REPETITIONS {
        let mut sink = 0.0f64;
        let mut count = 0usize;
        let start = Instant::now();

        loop {
            for _ in 0..CHUNK {
                sink += evaluate(count);
                count += 1;
            }
            if start.elapsed() >= TARGET {
                break;
            }
        }

        let elapsed = start.elapsed();
        // Without this the whole loop is dead code in a release build.
        black_box(sink);

        #[expect(
            clippy::cast_precision_loss,
            reason = "evaluation counts are far below the f64 integer limit"
        )]
        let rate = count as f64 / elapsed.as_secs_f64() / 1000.0;
        best = best.max(rate);
    }
    best
}

struct Measurement {
    name: &'static str,
    variables: usize,
    bound: f64,
    naive: f64,
}

fn measure(case: &Case) -> Measurement {
    let names: Vec<String> = (1..=case.variables).map(|i| format!("x{i}")).collect();
    let schema = Schema::new(names.clone());
    let expression: Expression = babel::compile(case.source)
        .unwrap_or_else(|e| panic!("{} did not compile: {e}", case.name));

    // Inputs are generated once. A benchmark that allocates per iteration is
    // measuring the allocator, which is the mistake the JVM fixture made.
    let mut rng = StdRng::seed_from_u64(0x8E_4C_47_9A_11);
    let rows: Vec<Vec<f64>> = (0..ROWS)
        .map(|_| {
            (0..case.variables)
                .map(|_| rng.random_range(0.1..10.0))
                .collect()
        })
        .collect();

    let bound_rate = {
        let bound = expression
            .bind(&schema)
            .unwrap_or_else(|e| panic!("{} did not bind: {e:?}", case.name));
        throughput(|index| {
            let row = black_box(&rows[index % ROWS]);
            bound.evaluate(row).unwrap_or(f64::NAN)
        })
    };

    // Pairs are built up front too, so this measures `evaluate` and not `Vec`
    // construction — the same courtesy the JVM harness gets.
    let pairs: Vec<Vec<(&str, f64)>> = rows
        .iter()
        .map(|row| {
            names
                .iter()
                .map(String::as_str)
                .zip(row.iter().copied())
                .collect()
        })
        .collect();
    let naive_rate = throughput(|index| {
        let bindings = black_box(&pairs[index % ROWS]);
        expression.evaluate(bindings).unwrap_or(f64::NAN)
    });

    Measurement {
        name: case.name,
        variables: case.variables,
        bound: bound_rate,
        naive: naive_rate,
    }
}

fn report(measurements: &[Measurement]) {
    let profile = if cfg!(debug_assertions) {
        "DEBUG — these numbers mean nothing, run `just bench`"
    } else {
        "release"
    };

    println!();
    println!("babel evaluation throughput, points/ms ({profile})");
    println!("{:-<64}", "");
    println!(
        "{:<20} {:>5} {:>12} {:>12} {:>8}",
        "expression", "vars", "bound", "naive", "ratio"
    );
    for m in measurements {
        println!(
            "{:<20} {:>5} {:>12.1} {:>12.1} {:>7.1}x",
            m.name,
            m.variables,
            m.bound,
            m.naive,
            m.bound / m.naive
        );
    }
    println!("{:-<64}", "");
    println!(
        "bound = `Bound::evaluate(&[f64])`, bound once up front.\n\
         naive = `Expression::evaluate(&[(&str, f64)])`, which rebuilds a Schema\n\
         and binds on every call. Both are given pre-built inputs."
    );
    println!();
}

#[test]
fn every_case_costs_what_it_should() {
    let measurements: Vec<Measurement> = cases().iter().map(measure).collect();
    report(&measurements);

    let rate = |name: &str| -> f64 {
        measurements
            .iter()
            .find(|m| m.name == name)
            .map(|m| m.bound)
            .expect("case should have been measured")
    };

    // These are shape checks, not performance targets — a threshold in
    // points/ms would fail on somebody else's laptop and teach everyone to
    // ignore it. What they catch is a harness measuring the wrong thing.

    // Two hundred squarings and a fold cannot cost what one addition costs. A
    // hundred-fold spread is not something an elided loop can fake, and unlike a
    // comparison between neighbouring cases it holds in either profile.
    assert!(
        rate("trivial") > rate("200-var sum (jvm)") * 10.0,
        "the cheapest and dearest cases ran at comparable rates, so the work is \
         probably being optimised away"
    );

    for m in &measurements {
        assert!(
            m.bound > m.naive,
            "{}: binding per call came out faster than binding once, which means \
             one of the two paths is not doing the work",
            m.name
        );
    }

    // The interesting invariant, and not the one first written here. The *ratio*
    // between the paths shrinks as expressions get dearer — 4.4x at two
    // variables against 1.8x at two hundred — because evaluation grows faster
    // than the fixed overhead does. What grows with variable count is the
    // overhead itself, measured as time per call, since names are what a
    // per-call `Schema` is built out of.
    let overhead = |m: &Measurement| 1.0 / m.naive - 1.0 / m.bound;
    let narrow = measurements.first().expect("at least one case");
    let wide = measurements.last().expect("at least one case");
    assert!(
        overhead(wide) > overhead(narrow) * 5.0,
        "rebuilding a Schema cost about as much for {} variables as for {} \
         ({:.5} against {:.5} ms per call), so it is probably not being rebuilt",
        wide.variables,
        narrow.variables,
        overhead(wide),
        overhead(narrow)
    );
}
