//! The torture problems, ported from `Benchmarks.kt` / `BenchmarksTests.kt`.
//!
//! Where `cvg_pools` is a smoke test, these are the hard ones.
//! `TopCorner200D`'s feasible region is 0.5^200 of its box — about one part in
//! 10^60 — so no amount of rejection sampling will land in it. P118 is
//! Hock-Schittkowski 118: fifteen variables under twenty-nine coupled linear
//! inequalities, and just as hopeless to sample blind.
//!
//! Every problem asserts three things: all points feasible, the requested count
//! delivered, and no duplicates. The JVM harness asserted only the weaker
//! "non-empty", and passed `allowEmptyResults` on the problems that returned
//! nothing at all — which turned two total failures and two 99%-shortfalls into
//! green ticks. That flag is deliberately not ported.
//!
//! # Fairness
//!
//! The JVM `ConstraintSet` carried an expected `centroid` and `dispersion`, and
//! the assertions against them were commented out — with a `TODO` on
//! `TopCorner200D` reading "this value doesnt seem correct at all". Distribution
//! quality, the whole point of a constrained vector generator, was never
//! actually under test.
//!
//! Brought back here, computed rather than guessed. `dispersion` was not wrong,
//! incidentally — 0.25 is the mean absolute deviation of a uniform on [0,1] — it
//! simply is not a statistic with a critical value attached, so there was no
//! threshold to compare it against. Everything below has one.
//!
//! The leverage is that **rejection sampling over the declared box is uniform
//! over the feasible region by construction** — that is what rejection sampling
//! *is* — so it does not need measuring against a standard, it can serve as one.
//! Note "over the declared box": a sampler that narrowed its proposals toward
//! what it had found would exclude a part of the region it had not seen, which
//! is why the oracle is [`Strategy::UniformSampling`] specifically, and why the
//! adaptive variant that once existed was never the oracle.
//!
//! That gives four oracles, each valid somewhere different — see [`Oracle`].

use babel::Ast;
use faer::Mat;

use babel::cvg::{
    ConstraintSolver, ConstraintSystem, InputVariable, Point, Satisfiability, Strategy,
};
use rand::RngExt;
use rand::SeedableRng;
use rand::rngs::Xoshiro256PlusPlus;

const SEED: u64 = 0x50_50_1E_5E_ED;

/// A second, unrelated seed, for the runs that must agree without being able to
/// agree by construction.
const RIVAL_SEED: u64 = 0x0D_D5_0F_1E_5E;

/// What production uses — taken from the library rather than restated here.
/// Restating it is how these benchmarks spent a run measuring a strategy list
/// the product had already moved on from.
use babel::cvg::DEFAULT_STRATEGIES as PRODUCTION;

/// A validated [`ConstraintSystem`], panicking on a fixture that does not bind.
///
/// Fixtures are written by hand and their variables always match their
/// constraints; a mismatch is a typo in the test, not a case under test.
fn system(variables: Vec<InputVariable>, constraints: Vec<Ast>) -> ConstraintSystem {
    ConstraintSystem::new(variables, constraints)
        .expect("a fixture's constraints should bind to its own box")
}

/// A sample matrix back as one `Vec<f64>` per point.
///
/// The pool speaks in matrices because that is what the evaluator eats, but
/// almost every assertion here is about *a point* — its coordinates, its
/// residual, its position in a distribution. Converting once at the boundary
/// keeps those assertions saying what they mean instead of indexing `(row,
/// column)` pairs.
fn columns(samples: &Mat<f64>) -> Vec<Vec<f64>> {
    (0..samples.ncols())
        .map(|column| {
            (0..samples.nrows())
                .map(|row| samples[(row, column)])
                .collect()
        })
        .collect()
}

/// The unbiased reference: no walker, and no adaptation to skew the proposals.
const REFERENCE: &[Strategy] = &[Strategy::UniformSampling];

/// Cap on how many points a distribution comparison uses.
///
/// Two-sample KS gains little past a couple of thousand and every comparison
/// costs a whole extra solve, so the 20,000-point problems compare a slice.
const DISTRIBUTION_SAMPLE: usize = 2_000;

/// How many random directions the joint comparison projects onto.
const PROJECTIONS: usize = 16;

/// Overall false-rejection budget, split across every test a comparison runs.
const SIGNIFICANCE: f64 = 0.01;

/// The least independence a sample may have and still be worth its size.
///
/// A Markov chain's output is correlated, so `n` points are worth fewer than `n`
/// independent ones. Some shortfall is inherent; an order of magnitude is a
/// quality problem, and one the caller should be told about rather than have
/// silently absorbed into a wider confidence interval.
const MINIMUM_EFFICIENCY: f64 = 0.1;

/// What can be claimed about how the points are spread.
enum Oracle {
    /// The feasible region is a box, so every coordinate's marginal must be
    /// uniform over a known interval. The strongest available, because it is
    /// absolute: it needs no reference sample, and so still applies where nothing
    /// can otherwise reach the region.
    UniformMarginals(Vec<(f64, f64)>),
    /// Indistinguishable from unbiased rejection sampling on the same problem.
    /// Applies wherever the reference can reach the region — most of the easy
    /// problems and none of the hard ones.
    MatchesReferenceSampler,
    /// Two runs from unrelated seeds agree with each other. Weaker, since it
    /// cannot see a bias both runs share, but it needs neither a closed form nor
    /// a reachable reference, which makes it the only thing available on P118.
    /// It catches what a Markov chain is most likely to get wrong: one that has
    /// not mixed still remembers where it started, and two runs started
    /// differently disagree.
    RunsAgree,
    /// The region is disconnected along the first coordinate, and every listed
    /// component must receive points. A strategy reporting only one component is
    /// not covering the region however uniform it looks inside that one.
    DisjointBands(Vec<(f64, f64)>),
}

struct Problem {
    name: &'static str,
    inputs: Vec<InputVariable>,
    constraints: Vec<Ast>,
    target_sample_size: usize,
    seeds: Vec<Point>,
    oracles: Vec<Oracle>,
}

fn variables(specs: &[(&str, f64, f64)]) -> Vec<InputVariable> {
    specs
        .iter()
        .map(|(name, low, high)| InputVariable::new(*name, *low, *high))
        .collect()
}

fn compile_all<S: AsRef<str>>(sources: &[S]) -> Vec<Ast> {
    sources
        .iter()
        .map(|source| {
            let source = source.as_ref();
            babel::parse(source)
                .unwrap_or_else(|e| panic!("constraint {source:?} did not compile: {e}"))
        })
        .collect()
}

async fn generate(
    problem: &Problem,
    seed: u64,
    strategies: &[Strategy],
    count: usize,
) -> Vec<Point> {
    let solution = ConstraintSolver::new()
        .with_rng(Xoshiro256PlusPlus::seed_from_u64(seed))
        .with_known_feasible(problem.seeds.clone())
        .with_strategies(strategies.to_vec())
        .solve(system(problem.inputs.clone(), problem.constraints.clone()))
        .await
        .unwrap_or_else(|e| panic!("{}: solving failed: {e}", problem.name));

    let mut pool = match solution {
        Satisfiability::Satisfied { samples } => samples,
        Satisfiability::Unsatisfiable { because } => {
            panic!(
                "{}: reported unsatisfiable, blaming {because:?}",
                problem.name
            )
        }
    };
    columns(&pool.take(count))
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// The Kolmogorov-Smirnov statistic against a uniform distribution over
/// `low..=high`: the largest gap between the empirical distribution and that one.
fn ks_against_uniform(samples: &mut [f64], low: f64, high: f64) -> f64 {
    samples.sort_by(f64::total_cmp);
    let n = samples.len() as f64;
    let span = high - low;

    samples
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let expected = ((value - low) / span).clamp(0.0, 1.0);
            // Both sides of the step, because the empirical distribution jumps at
            // each sample and the largest gap can be on either.
            let below = index as f64 / n;
            let above = (index as f64 + 1.0) / n;
            (expected - below).abs().max((above - expected).abs())
        })
        .fold(0.0, f64::max)
}

/// The two-sample statistic: the largest gap between two empirical
/// distributions. Needs a closed form for neither, which is what makes it usable
/// on regions that have none.
fn ks_two_sample(left: &mut [f64], right: &mut [f64]) -> f64 {
    left.sort_by(f64::total_cmp);
    right.sort_by(f64::total_cmp);
    let (n, m) = (left.len() as f64, right.len() as f64);

    let (mut i, mut j, mut largest) = (0usize, 0usize, 0.0f64);
    while i < left.len() && j < right.len() {
        // Step past every tie at once, so the comparison happens between complete
        // steps rather than partway up one.
        let value = left[i].min(right[j]);
        while i < left.len() && left[i] <= value {
            i += 1;
        }
        while j < right.len() && right[j] <= value {
            j += 1;
        }
        largest = largest.max((i as f64 / n - j as f64 / m).abs());
    }
    largest
}

/// The largest one-sample statistic expected by chance at significance `alpha`.
///
/// `c(alpha) = sqrt(-ln(alpha / 2) / 2)`, which gives the familiar 1.358 at the
/// 5% level. `alpha` should already carry any Bonferroni correction — testing two
/// hundred coordinates at 5% each would reject ten of them by luck alone.
///
/// `n` should be the *effective* sample size — see [`effective_sample_size`].
fn ks_critical_value(n: f64, alpha: f64) -> f64 {
    (-(alpha / 2.0).ln() / 2.0).sqrt() / n.sqrt()
}

/// The same, for the two-sample statistic.
fn ks_two_sample_critical(n: f64, m: f64, alpha: f64) -> f64 {
    (-(alpha / 2.0).ln() / 2.0).sqrt() * ((n + m) / (n * m)).sqrt()
}

/// How many independent observations a correlated sequence is worth.
///
/// Every test here is a Kolmogorov-Smirnov test, and KS assumes independent
/// samples. The walker is a Markov chain, so its output is not: `n` correlated
/// points carry less information than `n` independent ones, and using `n` in the
/// critical value makes the threshold too tight and the test reject good output.
///
/// `n_eff = n / tau`, with `tau = 1 + 2 * sum(rho_k)` the integrated
/// autocorrelation time.
///
/// The subtlety is where to stop summing, and the usual rule — truncate at the
/// first non-positive `rho_k` — is wrong here. Emission is round-robin across
/// chains, so successive points come from *different* chains: the first several
/// lags are uncorrelated and the real correlation sits out at the chain count.
/// Truncating at the first flat lag stops at lag one and reports full
/// independence for a sequence that has none. (It did exactly that, and made a
/// correlated sample look like grounds for suspecting the walker.)
///
/// Sokal's automatic windowing avoids having to know the chain count: grow the
/// window until it is at least `WINDOW_FACTOR` times the estimate computed from
/// it. On independent data that settles at the first few lags and returns `n`;
/// on data correlated at any lag it keeps going until the window covers it.
///
/// Must be given the sequence in emission order, before anything sorts it.
fn effective_sample_size(values: &[f64]) -> f64 {
    /// Sokal's constant. Five is the conventional choice: large enough that the
    /// window covers the correlation, small enough that summing noise past it
    /// does not inflate the estimate.
    const WINDOW_FACTOR: f64 = 5.0;

    let n = values.len();
    if n < 8 {
        return n as f64;
    }
    let count = n as f64;
    let mean = values.iter().sum::<f64>() / count;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count;
    if variance <= 0.0 {
        // Every value identical. Worth one observation, whatever `n` says.
        return 1.0;
    }

    let mut running = 1.0;
    for lag in 1..n / 4 {
        let covariance = values[..n - lag]
            .iter()
            .zip(&values[lag..])
            .map(|(a, b)| (a - mean) * (b - mean))
            .sum::<f64>()
            / count;
        running += 2.0 * covariance / variance;

        // The window is now long enough to have seen whatever it is going to.
        if (lag as f64) >= WINDOW_FACTOR * running.max(1.0) {
            break;
        }
    }
    (count / running.max(1.0)).clamp(1.0, count)
}

/// The independence check, stated on its own rather than folded into a
/// confidence interval.
///
/// Correcting the critical value keeps the KS tests honest, but on its own it
/// would let the walker degrade indefinitely: every extra bit of correlation
/// simply widens the threshold and nothing ever fails. This is the assertion
/// that notices.
fn assert_efficient(context: &str, values: &[f64]) -> f64 {
    let effective = effective_sample_size(values);
    let efficiency = effective / values.len() as f64;
    assert!(
        efficiency >= MINIMUM_EFFICIENCY,
        "{context}: {} points are worth only {effective:.0} independent ones          ({:.1}% efficiency) — the chain is not mixing",
        values.len(),
        efficiency * 100.0
    );
    effective
}

fn assert_indistinguishable(
    context: &str,
    mut sample: Vec<f64>,
    mut reference: Vec<f64>,
    alpha: f64,
) {
    let effective_sample = assert_efficient(context, &sample);
    let effective_reference = assert_efficient(context, &reference);
    let critical = ks_two_sample_critical(effective_sample, effective_reference, alpha);

    let statistic = ks_two_sample(&mut sample, &mut reference);
    assert!(
        statistic <= critical,
        "{context}: distributions differ (KS {statistic:.4} > {critical:.4},          effective n {effective_sample:.0} and {effective_reference:.0})"
    );
}

/// A direction uniformly distributed on the unit sphere, by Muller's method.
fn unit_vector(rng: &mut Xoshiro256PlusPlus, dimensions: usize) -> Vec<f64> {
    loop {
        let components: Vec<f64> = (0..dimensions)
            .map(|_| {
                let u1: f64 = 1.0 - rng.random_range(0.0..1.0);
                let u2: f64 = rng.random_range(0.0..1.0);
                (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
            })
            .collect();
        let norm = components.iter().map(|c| c * c).sum::<f64>().sqrt();
        if norm > 0.0 && norm.is_finite() {
            return components.iter().map(|c| c / norm).collect();
        }
    }
}

fn project(points: &[Point], direction: &[f64]) -> Vec<f64> {
    points
        .iter()
        .map(|point| point.iter().zip(direction).map(|(v, d)| v * d).sum())
        .collect()
}

fn distances_from(points: &[Point], centre: &[f64]) -> Vec<f64> {
    points
        .iter()
        .map(|point| {
            point
                .iter()
                .zip(centre)
                .map(|(v, c)| (v - c) * (v - c))
                .sum::<f64>()
                .sqrt()
        })
        .collect()
}

/// Three comparisons, because each catches something the others cannot.
///
/// *Marginals* are the obvious test and the weakest: every coordinate can be
/// perfectly uniform while the joint distribution is badly correlated.
///
/// *Random projections* close that gap, on the Cramér-Wold result that two
/// distributions agreeing on every one-dimensional projection are the same
/// distribution. Sixteen is not every projection, but it is enough to catch
/// structure lying across the coordinates rather than along them.
///
/// *Radial profile* catches what neither does, being the one nonlinear view. It
/// exists for the bug this walker was written to avoid: placing points uniformly
/// in radius rather than in volume leaves marginals and projections looking
/// plausible while the interior is over-full.
fn assert_same_distribution(context: &str, sample: &[Point], reference: &[Point]) {
    assert!(
        !sample.is_empty() && !reference.is_empty(),
        "{context}: nothing to compare"
    );
    let dimensions = sample[0].len();

    // One budget split across every test performed, so that adding a projection
    // cannot turn a passing comparison into a failing one.
    let alpha = SIGNIFICANCE / (dimensions + PROJECTIONS + 1) as f64;

    for index in 0..dimensions {
        assert_indistinguishable(
            &format!("{context}: coordinate {index}"),
            sample.iter().map(|point| point[index]).collect(),
            reference.iter().map(|point| point[index]).collect(),
            alpha,
        );
    }

    let mut rng = Xoshiro256PlusPlus::seed_from_u64(SEED);
    for projection in 0..PROJECTIONS {
        let direction = unit_vector(&mut rng, dimensions);
        assert_indistinguishable(
            &format!("{context}: projection {projection}"),
            project(sample, &direction),
            project(reference, &direction),
            alpha,
        );
    }

    let centre: Vec<f64> = (0..dimensions)
        .map(|index| {
            reference.iter().map(|point| point[index]).sum::<f64>() / reference.len() as f64
        })
        .collect();
    assert_indistinguishable(
        &format!("{context}: radial profile"),
        distances_from(sample, &centre),
        distances_from(reference, &centre),
        alpha,
    );
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

async fn assert_fair(problem: &Problem, points: &[Point]) {
    let wanted = problem
        .target_sample_size
        .min(DISTRIBUTION_SAMPLE)
        .min(points.len());
    let sample = &points[..wanted];

    for oracle in &problem.oracles {
        match oracle {
            Oracle::UniformMarginals(intervals) => {
                let alpha = SIGNIFICANCE / intervals.len() as f64;

                for (index, (low, high)) in intervals.iter().enumerate() {
                    let mut column: Vec<f64> = points.iter().map(|point| point[index]).collect();
                    let context = format!(
                        "{}: coordinate {}",
                        problem.name, problem.inputs[index].name
                    );
                    let effective = assert_efficient(&context, &column);
                    let critical = ks_critical_value(effective, alpha);

                    let statistic = ks_against_uniform(&mut column, *low, *high);
                    assert!(
                        statistic <= critical,
                        "{context} is not uniform over {low}..={high} \
                         (KS {statistic:.4} > {critical:.4} at alpha {alpha:.2e}, \
                         effective n {effective:.0} of {})",
                        points.len()
                    );
                }
            }

            Oracle::MatchesReferenceSampler => {
                let reference = generate(problem, SEED, REFERENCE, wanted).await;
                assert_same_distribution(
                    &format!("{} vs unbiased rejection sampling", problem.name),
                    sample,
                    &reference,
                );
            }

            Oracle::RunsAgree => {
                let rival = generate(problem, RIVAL_SEED, PRODUCTION, wanted).await;
                assert_same_distribution(
                    &format!("{} across two seeds", problem.name),
                    sample,
                    &rival,
                );
            }

            Oracle::DisjointBands(bands) => {
                for (low, high) in bands {
                    let hits = points
                        .iter()
                        .filter(|point| (*low..=*high).contains(&point[0]))
                        .count();
                    assert!(
                        hits > 0,
                        "{}: found nothing in the band {low}..={high}",
                        problem.name
                    );
                }
            }
        }
    }
}

async fn run(problem: Problem) {
    let points = generate(&problem, SEED, PRODUCTION, problem.target_sample_size).await;

    // 1. everything returned is feasible
    for point in &points {
        let bindings: Vec<(&str, f64)> = problem
            .inputs
            .iter()
            .map(|v| v.name.as_str())
            .zip(point.iter().copied())
            .collect();
        for constraint in &problem.constraints {
            let residual = babel::eval_one(constraint, &bindings)
                .unwrap_or_else(|e| panic!("{}: evaluation failed: {e}", problem.name));
            assert!(
                residual <= 1e-12,
                "{}: {point:?} fails {:?} (residual {residual})",
                problem.name,
                constraint.source()
            );
        }
    }

    // 2. the requested number of points came back
    assert_eq!(
        points.len(),
        problem.target_sample_size,
        "{}: generated {} of {} requested",
        problem.name,
        points.len(),
        problem.target_sample_size
    );

    // 3. no duplicates — a stalled chain emitting one point over and over would
    //    sail through the feasibility check otherwise, and stalling is the
    //    walker's characteristic failure
    let mut seen: Vec<&Point> = Vec::new();
    for point in &points {
        assert!(
            !seen.contains(&point),
            "{}: duplicate point {point:?}",
            problem.name
        );
        seen.push(point);
    }

    // 4. the points are spread the way the region says they should be
    assert_fair(&problem, &points).await;
}

// ---------------------------------------------------------------------------
// The problems
// ---------------------------------------------------------------------------

#[pollster::test]
async fn sanity_check() {
    // Half the box, and the half is a box itself — so the marginal must be
    // uniform on 0..=1, and the reference sampler reaches it easily. The one
    // problem where every oracle applies at once.
    run(Problem {
        name: "SanityCheck",
        inputs: variables(&[("x", -1.0, 1.0)]),
        constraints: compile_all(&["x > 0.0"]),
        target_sample_size: 1_000,
        seeds: vec![vec![0.5]],
        oracles: vec![
            Oracle::UniformMarginals(vec![(0.0, 1.0)]),
            Oracle::MatchesReferenceSampler,
            Oracle::RunsAgree,
        ],
    })
    .await;
}

#[pollster::test]
async fn braindead_inequalities() {
    // Five variables in the unit cube under three loose inequalities. The region
    // is a polytope, not a box, so the marginals are not uniform and there is no
    // absolute claim to make — but rejection sampling reaches it comfortably, so
    // it can be measured against something unbiased.
    run(Problem {
        name: "Braindead",
        inputs: variables(&[
            ("x1", 0.0, 1.0),
            ("x2", 0.0, 1.0),
            ("x3", 0.0, 1.0),
            ("x4", 0.0, 1.0),
            ("x5", 0.0, 1.0),
        ]),
        constraints: compile_all(&["x1 + x2 > x3", "x2 + x3 > x4", "x3 + x4 > x5"]),
        target_sample_size: 5_000,
        seeds: Vec::new(),
        oracles: vec![Oracle::MatchesReferenceSampler],
    })
    .await;
}

#[pollster::test]
async fn top_corner_200d() {
    // 200 variables in 10..11, each required above 10.5. The feasible region is
    // 0.5^200 of the box, so rejection sampling cannot reach it and there is no
    // reference to compare against — but the region *is* a box, so the absolute
    // oracle applies, and it is the stronger one anyway.
    //
    // This is the problem that forced axis moves into the walker: pure
    // hit-and-run mixes as O(d^2) here, and left every coordinate's spread 6%
    // short of uniform.
    let names: Vec<String> = (1..=200).map(|i| format!("x{i}")).collect();
    let constraints: Vec<String> = names.iter().map(|name| format!("{name} > 10.5")).collect();

    run(Problem {
        name: "TopCorner200D",
        inputs: names
            .iter()
            .map(|name| InputVariable::new(name.clone(), 10.0, 11.0))
            .collect(),
        constraints: compile_all(&constraints),
        target_sample_size: 200,
        seeds: vec![vec![10.75; 200]],
        oracles: vec![Oracle::UniformMarginals(vec![(10.5, 11.0); 200])],
    })
    .await;
}

#[pollster::test]
async fn tough_single_var() {
    // The crescent between two phase-shifted sine waves. Narrow, curved, and
    // nowhere near a box — but rejection sampling clears it, so it tests whether
    // the walker follows a curved region honestly rather than cutting corners.
    run(Problem {
        name: "ToughSingleVar",
        inputs: variables(&[("x", -3.0, 1.0), ("y", -1.0, 1.0)]),
        constraints: compile_all(&["y < sin(x*pi)", "y > 1.1*sin(x*pi-0.5)"]),
        target_sample_size: 1_000,
        seeds: Vec::new(),
        oracles: vec![Oracle::MatchesReferenceSampler],
    })
    .await;
}

#[pollster::test]
async fn p118() {
    // Hock-Schittkowski 118: fifteen variables, twenty-nine coupled linear
    // inequalities forming a narrow polytope. No closed form and no reachable
    // reference, so agreement between two independently seeded runs is the only
    // distribution claim available.
    //
    // Transcribed literally rather than generated. The bands do follow a pattern
    // (the second bound cycles 6, 7, 6) but a loop would hide a transcription
    // slip in the one test that most needs to be right.
    run(Problem {
        name: "P118",
        inputs: [
            21.0, 57.0, 16.0, 90.0, 120.0, 60.0, 90.0, 120.0, 60.0, 90.0, 120.0, 60.0, 90.0, 120.0,
            60.0,
        ]
        .iter()
        .enumerate()
        .map(|(index, upper)| InputVariable::new(format!("x{}", index + 1), 0.0, *upper))
        .collect(),
        constraints: compile_all(&[
            "0 > -x4+x1-7",
            "0 > x4-x1-6",
            "0 > -x5+x2-7",
            "0 > x5-x2-7",
            "0 > -x6+x3-7",
            "0 > x6-x3-6",
            "0 > -x7+x4-7",
            "0 > x7-x4-6",
            "0 > -x8+x5-7",
            "0 > x8-x5-7",
            "0 > -x9+x6-7",
            "0 > x9-x6-6",
            "0 > -x10+x7-7",
            "0 > x10-x7-6",
            "0 > -x11+x8-7",
            "0 > x11-x8-7",
            "0 > -x12+x9-7",
            "0 > x12-x9-6",
            "0 > -x13+x10-7",
            "0 > x13-x10-6",
            "0 > -x14+x11-7",
            "0 > x14-x11-7",
            "0 > -x15+x12-7",
            "0 > x15-x12-6",
            "0 > -x1-x2-x3+60",
            "0 > -x4-x5-x6+50",
            "0 > -x7-x8-x9+70",
            "0 > -x10-x11-x12+85",
            "0 > -x13-x14-x15+100",
        ]),
        target_sample_size: 1_000,
        seeds: vec![vec![
            1.0, 45.0, 15.0, 6.0, 39.0, 20.0, 11.0, 35.0, 25.0, 16.0, 40.0, 30.0, 20.0, 46.0, 35.0,
        ]],
        oracles: vec![Oracle::RunsAgree],
    })
    .await;
}

/// Equality with a shrinking tolerance — the series showing where rejection
/// sampling stops working, and the only problem here whose region is genuinely
/// **disjoint**: two bands, around x = -2 and x = 1.
///
/// That disjointness is the point. A chain reaches another component only if a
/// first draw happens to land in it, and that stops happening once the
/// components are small — which is what separates the walker from the sampler.
async fn parabolic_roots(offset: &str, width: f64, extra: Vec<Oracle>) {
    let mut oracles = vec![Oracle::DisjointBands(vec![
        (-2.0 - width, -2.0 + width),
        (1.0 - width, 1.0 + width),
    ])];
    oracles.extend(extra);

    run(Problem {
        name: "ParabolicRoots",
        inputs: variables(&[("x", -5.0, 5.0)]),
        constraints: compile_all(&[format!("(x + 2) * (x - 1) == 0 +/- {offset}")]),
        target_sample_size: 20_000,
        seeds: vec![vec![-2.0]],
        oracles,
    })
    .await;
}

#[pollster::test]
async fn parabolic_roots_wide() {
    parabolic_roots("1.0", 1.0, vec![Oracle::MatchesReferenceSampler]).await;
}

#[pollster::test]
async fn parabolic_roots_narrowing() {
    parabolic_roots("0.1", 0.1, vec![Oracle::MatchesReferenceSampler]).await;
}

#[pollster::test]
async fn parabolic_roots_narrow() {
    // Unbiased rejection sampling manages only a few hundred points here, too few
    // to compare against, so band coverage is what is left.
    parabolic_roots("0.001", 0.001, Vec::new()).await;
}

#[pollster::test]
async fn parabolic_roots_ribbon() {
    parabolic_roots("0.00001", 0.00001, Vec::new()).await;
}

// ---------------------------------------------------------------------------
// The oracles, tested against known-bad samples
// ---------------------------------------------------------------------------
//
// An oracle that cannot fail is worse than no oracle: it reports success on
// everything and reads, in the log, exactly like one that works. Each of these
// hands the machinery a distribution wrong in a specific way and requires it to
// notice — including, below, the precise bug the walker was written to avoid.

#[test]
fn the_uniformity_test_can_fail() {
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(7);
    let critical = ks_critical_value(1_000.0, 0.01);

    let mut uniform: Vec<f64> = (0..1_000).map(|_| rng.random_range(0.0..1.0)).collect();
    let accepted = ks_against_uniform(&mut uniform, 0.0, 1.0);
    assert!(
        accepted <= critical,
        "a genuinely uniform sample was rejected: {accepted} > {critical}"
    );

    // Squaring bunches the same sample toward zero. Its true distribution is
    // sqrt(y), which departs from uniform by 0.25 at its worst — five times the
    // critical value, so this is not a marginal call.
    let mut bunched: Vec<f64> = uniform.iter().map(|value| value * value).collect();
    let rejected = ks_against_uniform(&mut bunched, 0.0, 1.0);
    assert!(
        rejected > critical,
        "a visibly skewed sample was accepted: {rejected} <= {critical}"
    );
}

#[test]
fn the_two_sample_test_can_fail() {
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(11);
    let critical = ks_two_sample_critical(1_000.0, 1_000.0, 0.01);

    let mut draw =
        |scale: f64| -> Vec<f64> { (0..1_000).map(|_| rng.random_range(0.0..scale)).collect() };
    let (mut left, mut right, mut wider) = (draw(1.0), draw(1.0), draw(1.2));

    let same = ks_two_sample(&mut left, &mut right);
    assert!(
        same <= critical,
        "two samples from one distribution were separated: {same} > {critical}"
    );

    let different = ks_two_sample(&mut left, &mut wider);
    assert!(
        different > critical,
        "a 20% wider distribution went unnoticed: {different} <= {critical}"
    );
}

#[test]
fn projections_catch_what_marginals_miss() {
    // Uniform on a square, against uniform on a thin diagonal band inside it.
    // Both have very nearly uniform marginals in x and in y — the band tapers
    // only in the corners — so a per-coordinate test sees almost nothing. The
    // projection onto the anti-diagonal sees a distribution collapsed to a sliver.
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(13);
    let square: Vec<Point> = (0..2_000)
        .map(|_| vec![rng.random_range(-1.0..1.0), rng.random_range(-1.0..1.0)])
        .collect();
    let band: Vec<Point> = (0..2_000)
        .map(|_| {
            let x: f64 = rng.random_range(-1.0..1.0);
            let offset: f64 = rng.random_range(-0.05..0.05);
            vec![x, (x + offset).clamp(-1.0, 1.0)]
        })
        .collect();

    let critical =
        ks_two_sample_critical(square.len() as f64, band.len() as f64, SIGNIFICANCE / 19.0);
    for index in 0..2 {
        let statistic = ks_two_sample(
            &mut square.iter().map(|p| p[index]).collect::<Vec<_>>(),
            &mut band.iter().map(|p| p[index]).collect::<Vec<_>>(),
        );
        assert!(
            statistic <= critical,
            "the premise is wrong: coordinate {index} already differs ({statistic} > {critical})"
        );
    }

    let anti_diagonal = [
        std::f64::consts::FRAC_1_SQRT_2,
        -std::f64::consts::FRAC_1_SQRT_2,
    ];
    let statistic = ks_two_sample(
        &mut project(&square, &anti_diagonal),
        &mut project(&band, &anti_diagonal),
    );
    assert!(
        statistic > critical,
        "a diagonal band was indistinguishable from a square: {statistic} <= {critical}"
    );
}

#[test]
fn the_radial_test_catches_uniform_in_radius() {
    // Exactly the bug the walker was written to avoid. On a disc, uniform in area
    // needs r = sqrt(u); using r = u instead piles points toward the middle.
    // Reproduce both and require the radial comparison to separate them.
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(17);
    let mut disc = |radial: fn(f64) -> f64| -> Vec<Point> {
        (0..2_000)
            .map(|_| {
                let angle: f64 = rng.random_range(0.0..std::f64::consts::TAU);
                let radius = radial(rng.random_range(0.0..1.0));
                vec![radius * angle.cos(), radius * angle.sin()]
            })
            .collect()
    };
    let by_area = disc(f64::sqrt);
    let also_by_area = disc(f64::sqrt);
    let by_radius = disc(|u| u);

    let origin = [0.0, 0.0];
    let critical = ks_two_sample_critical(2_000.0, 2_000.0, SIGNIFICANCE / 19.0);

    let same = ks_two_sample(
        &mut distances_from(&by_area, &origin),
        &mut distances_from(&also_by_area, &origin),
    );
    assert!(
        same <= critical,
        "two correct discs were separated: {same} > {critical}"
    );

    let different = ks_two_sample(
        &mut distances_from(&by_area, &origin),
        &mut distances_from(&by_radius, &origin),
    );
    assert!(
        different > critical,
        "uniform-in-radius passed as uniform-in-area: {different} <= {critical}"
    );
}
