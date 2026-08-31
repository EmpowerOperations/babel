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
//! * [`CompiledExpression::evaluate`] takes a row of `f64` — bind once, then evaluate. What
//!   Artemis will use.
//! * [`Ast::evaluate`] takes name-value pairs and rebuilds a `Schema`,
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

use babel::{Ast, Schema};
use faer::Mat;
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

/// How many distinct batches to cycle through.
///
/// More than one so the optimiser cannot hoist the evaluation, and few enough to
/// stay in cache — this is measuring the evaluator, not the memory subsystem.
const ROTATION: usize = 16;

/// Evaluations between clock reads. Amortises `Instant::now`, which is not free
/// and would otherwise dominate the cheapest case.
const CHUNK: usize = 64;

struct Case {
    name: &'static str,
    /// The ledger file this case records into, without extension.
    ///
    /// Given explicitly rather than derived from `name`, because slugifying
    /// `"small (jvm)"` is a transformation nobody should have to reverse in
    /// their head when they are looking for a file.
    slug: &'static str,
    source: &'static str,
    variables: usize,
}

/// Increasing cost. Two are lifted verbatim from the JVM's `PerformanceFixture`
/// so the comparison is direct rather than approximate.
fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "trivial",
            slug: "add-two-vars",
            source: "x1 + x2",
            variables: 2,
        },
        Case {
            // From the JVM fixture.
            name: "small (jvm)",
            slug: "compare-with-square",
            source: "x1 + x2 > 20 - x3^2",
            variables: 3,
        },
        Case {
            name: "transcendental",
            slug: "transcendental-mix",
            source: "sin(x1) * cos(x2) + sqrt(abs(x3))",
            variables: 3,
        },
        Case {
            name: "deep arithmetic",
            slug: "deep-arithmetic",
            source: "(((((x1 + x2) * x3 - x4) / (x1 + 1) + x2) * x3 - x4) / (x1 + 1) + x2) \
                     * x3 - x4 + ((x1 * x2) - (x3 / (x4 + 1)))^2",
            variables: 4,
        },
        Case {
            // From the JVM fixture. Constant bounds, so `unroll_aggregates`
            // flattens it to a single 200-term fold before the evaluator ever
            // sees it — worth knowing before comparing this row to the JVM's.
            name: "200-var sum (jvm)",
            slug: "sum-200-squares",
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

/// Batch widths measured, and why two of them.
///
/// `1` is the degenerate batch — everything the API used to do per call, still
/// done per call. `256` is a realistic width. The gap between them is what
/// batching actually buys, and it is the number worth watching as the tape
/// arrives: a tree walk amortises the two buffer allocations and nothing else,
/// where a tape should amortise the traversal itself.
const WIDTHS: [usize; 2] = [1, 256];

struct Measurement {
    name: &'static str,
    slug: &'static str,
    variables: usize,
    /// Points per millisecond at each of [`WIDTHS`].
    rates: [f64; 2],
}

fn measure(case: &Case) -> Measurement {
    let names: Vec<String> = (1..=case.variables).map(|i| format!("x{i}")).collect();
    let schema = Schema::new(names);
    let expression: Ast =
        babel::parse(case.source).unwrap_or_else(|e| panic!("{} did not compile: {e}", case.name));
    let compiled = babel::compile(&expression, &schema)
        .unwrap_or_else(|e| panic!("{} did not compile against its schema: {e:?}", case.name));

    // Batches are generated once. A benchmark that allocates per iteration is
    // measuring the allocator, which is the mistake the JVM fixture made.
    let mut rng = StdRng::seed_from_u64(0x8E_4C_47_9A_11);
    let batches: Vec<Vec<Mat<f64>>> = WIDTHS
        .iter()
        .map(|&width| {
            (0..ROTATION)
                .map(|_| Mat::from_fn(case.variables, width, |_, _| rng.random_range(0.1..10.0)))
                .collect()
        })
        .collect();

    let mut rates = [0.0; WIDTHS.len()];
    for (slot, (width, prepared)) in rates.iter_mut().zip(WIDTHS.iter().zip(&batches)) {
        // `throughput` counts *calls*; a call here is `width` points, so the
        // rate it returns has to be scaled or the wide batch looks slow.
        #[expect(
            clippy::cast_precision_loss,
            reason = "batch widths are far below the f64 integer limit"
        )]
        let scale = *width as f64;
        *slot = scale
            * throughput(|index| {
                let batch = black_box(&prepared[index % ROTATION]);
                compiled
                    .eval(batch.as_ref())
                    .map_or(f64::NAN, |residuals| residuals[0])
            });
    }

    Measurement {
        name: case.name,
        slug: case.slug,
        variables: case.variables,
        rates,
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
    println!("{:-<70}", "");
    println!(
        "{:<20} {:>5} {:>14} {:>14} {:>8}",
        "expression", "vars", "batch of 1", "batch of 256", "gain"
    );
    for m in measurements {
        println!(
            "{:<20} {:>5} {:>14.1} {:>14.1} {:>7.1}x",
            m.name,
            m.variables,
            m.rates[0],
            m.rates[1],
            m.rates[1] / m.rates[0]
        );
    }
    println!("{:-<70}", "");
    println!("points per millisecond through `CompiledExpression::eval(MatRef)`,");
    println!("at two batch widths. One column per sample, batches built up front,");
    println!("so this measures evaluation and not allocation.");
    println!();
}

/// Where the ledgers live, relative to `CARGO_MANIFEST_DIR` rather than the
/// working directory, so they land in the same place whether the runner was
/// invoked from the crate or the repo root.
const LEDGER_DIR: &str = "performance-records";

/// The header every ledger carries. `sep=;` is the hint that makes Excel open
/// the file without a wizard.
const LEDGER_HEADER: &str = "sep=;\nversion                 ;timestamp               ;host   ;vars ;bound       ;naive       ;map         ;\n";

/// Records this run, **one file per case**, replacing the previous run at the
/// same version rather than appending beside it.
///
/// One file per case rather than one file with a case column, matching the
/// other repos: a ledger is then a single measurement's history read top to
/// bottom, and the upsert rule is the simple one — if the last row carries this
/// version, replace it, otherwise append.
///
/// Upsert rather than append because a tuning session is a dozen runs of one
/// version, and keeping all of them buries the history the file exists to show.
/// Bumping the version is what turns the current row into a historical one.
///
/// **Release only.** The same test runs in debug at a twentieth of the
/// workload, and under upsert that would not merely add a bad row — it would
/// overwrite the good one. Delete the guard if the workloads ever match.
///
/// Failures are printed, never panicked on: a benchmark that cannot write its
/// log has still produced its numbers, and the numbers are on stdout.
fn record_in_ledgers(measurements: &[Measurement]) {
    if cfg!(debug_assertions) {
        return;
    }

    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(LEDGER_DIR);
    let version = env!("CARGO_PKG_VERSION");
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".to_owned());
    let timestamp = timestamp_utc();

    for m in measurements {
        let path = directory.join(format!("{}.csv", m.slug));

        // Column widths match the header, so the file reads as text and a
        // Column widths match the header, so the file reads as text and a
        // `git diff` lines up. `map` is left blank: it is the JVM's column.
        let row = format!(
            "{version:<24};{timestamp:<24};{host:<7};{:<5};{:<12.1};{:<12.1};{:<12};",
            m.variables, m.rates[0], m.rates[1], ""
        );

        // A missing ledger is created rather than treated as an error: adding a
        // case to `cases()` should not also require touching this directory.
        let existing = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(_) => LEDGER_HEADER.to_owned(),
        };

        let mut lines: Vec<&str> = existing.lines().collect();
        while lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.pop();
        }
        // The previous run at this version, if the last row is one.
        if lines
            .last()
            .and_then(|line| line.split(';').next())
            .is_some_and(|recorded| recorded.trim() == version)
        {
            lines.pop();
        }

        let mut updated: String = lines.iter().map(|line| format!("{line}\n")).collect();
        updated.push_str(&row);
        updated.push('\n');

        if let Err(e) = std::fs::write(&path, updated) {
            println!("could not write {}: {e}", path.display());
        }
    }

    println!(
        "recorded {version} into {} ledgers under {}",
        measurements.len(),
        directory.display()
    );
}

/// An RFC 3339 timestamp, to the second, in UTC.
///
/// Hand-rolled because the alternative is a `chrono` or `time` dependency in a
/// crate that has neither, for one line in a benchmark. Civil-from-days is
/// Howard Hinnant's algorithm, which is the standard one and gets the leap year
/// rules exactly right.
fn timestamp_utc() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());

    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    let (hour, minute, second) = (rest / 3600, (rest % 3600) / 60, rest % 60);

    // Shift the epoch to 0000-03-01 so leap days fall at the end of the cycle.
    let z = i64::try_from(days).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;

    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = era * 400 + year_of_era + i64::from(month <= 2);

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

#[test]
fn every_case_costs_what_it_should() {
    let measurements: Vec<Measurement> = cases().iter().map(measure).collect();
    report(&measurements);
    record_in_ledgers(&measurements);

    let rate = |name: &str| -> f64 {
        measurements
            .iter()
            .find(|m| m.name == name)
            .map(|m| m.rates[1])
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
        "the cheapest and dearest cases ran at comparable rates, so the work is probably being optimised away"
    );

    // Batching must not make things *slower*. Today it amortises two buffer
    // allocations across the batch; once a tape lands it should amortise the
    // traversal itself. Either way the wide batch should never lose to the
    // degenerate one, and if it does then the per-call setup is not where it is
    // believed to be.
    //
    // **Release only**, and the first version of this got that wrong. The debug
    // workload is a 20ms window, which is nowhere near long enough to compare
    // two rates: it passed alone and failed under full-suite load, which is
    // precisely the flaky threshold this file's own comments warn against. The
    // spread assertion above survives either profile because a hundred-fold gap
    // cannot be faked; a two-way comparison of similar numbers cannot.
    for m in measurements.iter().filter(|_| !cfg!(debug_assertions)) {
        assert!(
            m.rates[1] >= m.rates[0],
            "{}: a batch of 256 ran slower per point than a batch of 1, so batching is costing rather than amortising",
            m.name
        );
    }
}
