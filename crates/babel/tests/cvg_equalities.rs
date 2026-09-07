//! The equality taxonomy, measured rather than assumed.
//!
//! Babel admits exactly one equality form — `a == b +/- t`, with `t` a literal —
//! and that one syntax covers at least six structurally different problems. The
//! taxonomy is written up in the root `todo.md` under *Equality constraints*;
//! these are the cases that decide whether it is real.
//!
//! **These are expected to be red, and the red is the deliverable.** The pool
//! today treats every equality identically: one residual, no idea that a
//! variable is pinned, driven, or one of two branches. The classifier that would
//! tell those apart is being designed against what these measure, so a case that
//! fails here is a specification for it, not a regression.
//!
//! # Why these and not the existing pool cases
//!
//! [`cvg_pools`](../cvg_pools.rs) already carries one case per taxonomy row and
//! every one of them is green. That is not evidence the shapes are handled — the
//! tolerances there are loose enough (`1e-3` to `1e-5`) that the band has volume
//! a walker can move in, so the pipeline never has to know what kind of equality
//! it is looking at. Tighten the tolerance and the distinction starts to matter.
//! Each case here is a pool case with the slack taken out, which is the cheapest
//! honest way to find the floor.
//!
//! **Rows A and B are green as of the classifier** ([`cvg::classify`]) — a
//! variable alone on one side of an equality is computed from the others rather
//! than searched for, which is what lets a walker move along a measure-zero
//! surface instead of jittering beside it. Rows C, D and E classify as `Opaque`
//! and are still red, which is the whole point of keeping them here.
//!
//! Case F — implicit, `sin(x) == x/2` — is deliberately absent. No use case has
//! turned up for `x == f(x)`, and Newton is a lot of machinery to carry for a
//! shape nobody writes. The classifier should recognise it as a self-dependency
//! and refuse it; see `todo.md`.
//!
//! # What is asserted
//!
//! Three claims, and the third is the one that bites. Delivering the requested
//! count and having every point feasible are what the pool fixtures already
//! check. **Coverage** is new: a solver hands back one witness, and a pool that
//! reports that witness five hundred times has satisfied both of the other two
//! claims while exploring nothing. So each case names the coordinates that are
//! genuinely free and the fraction of their range the sample must span.

use babel::Ast;
use babel::cvg::{ConstraintSolver, ConstraintSystem, InputVariable, Satisfiability, SystemError};
use faer::Mat;
use rand::SeedableRng;
use rand::rngs::Xoshiro256PlusPlus;

/// Pinned so a failure is reproducible, and the same value the other cvg suites
/// use so a point seen in one is the point seen in another.
const SEED: u64 = 0x50_50_1E_5E_ED;

/// A sample matrix back as one `Vec<f64>` per point.
fn columns(samples: &Mat<f64>) -> Vec<Vec<f64>> {
    (0..samples.ncols())
        .map(|column| {
            (0..samples.nrows())
                .map(|row| samples[(row, column)])
                .collect()
        })
        .collect()
}

/// Compiles constraint sources, so a compiler failure reads as one.
fn constraints(sources: &[&str]) -> Vec<Ast> {
    sources
        .iter()
        .map(|source| {
            babel::parse(source)
                .unwrap_or_else(|e| panic!("constraint {source:?} did not compile: {e}"))
        })
        .collect()
}

/// One taxonomy case.
struct Case<'a> {
    /// Taxonomy row and a word on the shape, for the failure message.
    what: &'a str,
    variables: &'a [(&'a str, f64, f64)],
    sources: &'a [&'a str],
    wanted: usize,
    /// Coordinates that are genuinely free under these constraints, each with
    /// the fraction of its declared range the sample is expected to span.
    ///
    /// Stated per case rather than inferred, because inferring it is the
    /// classifier's job and this is the test that classifier gets built against.
    coverage: &'a [(&'a str, f64)],
    /// How much of the feasible set the sample must actually *occupy*, as
    /// opposed to merely reach the ends of. `None` where the case does not make
    /// the claim.
    occupancy: Option<Occupancy<'a>>,
}

/// A grid over some of the free coordinates, and how many of its cells the
/// sample has to land in.
///
/// # Why extent is not enough
///
/// [`Case::coverage`] measures `max - min`, and a handful of points at the
/// extremes satisfies it completely. That is not hypothetical: the pool spends
/// up to a fixed budget of solver calls looking for parts of the box it has not
/// reached, and those seeds land at the *edges* of what is uncovered, by
/// construction. Sixteen well-placed points pass a span assertion on any number
/// of coordinates while occupying almost nothing between them.
///
/// Occupancy cannot be met that way. `n` seeds fill at most `n` cells however
/// cleverly they are placed, so a threshold above the seed budget can only be
/// reached by a search that **moves along the feasible set** — which is what
/// rows C and E are actually about, and what a span assertion could not tell
/// apart from a lucky scattering.
enum Occupancy<'a> {
    /// One grid over all the listed coordinates together, holding
    /// `divisions^over.len()` cells.
    ///
    /// The stronger claim, because a surface can be traversed in one coordinate
    /// while another stays pinned and only a joint grid sees that. **It is also
    /// the one that does not scale**: past about three coordinates the cell
    /// count runs away from the point count, every point lands in its own cell,
    /// and occupancy saturates at `n` for good and bad samples alike. Use it at
    /// one or two coordinates and reach for `Marginal` above that.
    Joint {
        over: &'a [&'a str],
        divisions: usize,
        least: usize,
    },
    /// A separate histogram per coordinate, every one of which must reach
    /// `least`. Cost is `points * over.len()`, so it is unchanged at two hundred
    /// coordinates where a joint grid is hopeless.
    ///
    /// Weaker in theory: a sample strung along a diagonal fills every marginal
    /// while occupying a line, which is the Latin hypercube's own blind spot
    /// seen from the other side. That is not a way for this to pass without the
    /// feature, though, because a diagonal is not a sample either the current
    /// pipeline or the intended one produces — what the current one produces is
    /// a handful of seeds, and `n` seeds fill at most `n` bins in *every*
    /// marginal at once.
    Marginal {
        over: &'a [&'a str],
        divisions: usize,
        least: usize,
    },
}

/// Asks for `wanted` points and reports, in one message, everything that was
/// wrong with what came back.
///
/// Reporting all three failures together rather than tripping on the first is
/// deliberate: these cases are expected to fail, and "delivered 1 of 500" and
/// "covered 0% of x2" are different diagnoses that a fail-fast assertion would
/// hide behind each other.
async fn assert_explores(case: Case<'_>) {
    let inputs: Vec<InputVariable> = case
        .variables
        .iter()
        .map(|(name, low, high)| InputVariable::new(*name, *low, *high))
        .collect();
    let compiled = constraints(case.sources);

    let system = ConstraintSystem::new(inputs.clone(), compiled.clone())
        .expect("a fixture's constraints should bind to its own box");

    let solution = ConstraintSolver::new()
        .with_rng(Xoshiro256PlusPlus::seed_from_u64(SEED))
        .solve(system)
        .await
        .unwrap_or_else(|e| panic!("{}: solving failed: {e}", case.what));

    let mut pool = match solution {
        Satisfiability::Satisfied { samples } => samples,
        Satisfiability::Unsatisfiable { because } => {
            panic!("{}: reported unsatisfiable, blaming {because:?}", case.what)
        }
    };

    let points = columns(&pool.take(case.wanted));
    let mut complaints: Vec<String> = Vec::new();

    // 1. The count. `take` returning short means the search is exhausted, which
    //    is a real outcome — and for a fully determined system it is the *only*
    //    outcome, which is one of the things these cases are here to show.
    if points.len() != case.wanted {
        complaints.push(format!(
            "delivered {} of {} requested (exhausted: {})",
            points.len(),
            case.wanted,
            pool.is_exhausted()
        ));
    }

    // 2. Feasibility, re-checked independently of the pool: it filters its own
    //    output, and a test that trusts the thing it is testing is not a test.
    let names: Vec<&str> = inputs.iter().map(|v| v.name.as_str()).collect();
    let mut infeasible = 0usize;
    let mut worst = f64::NEG_INFINITY;
    for point in &points {
        let bindings: Vec<(&str, f64)> = names.iter().copied().zip(point.iter().copied()).collect();
        for expression in &compiled {
            let residual = babel::eval_one(expression, &bindings).unwrap_or_else(|e| {
                panic!("{}: evaluating {:?}: {e}", case.what, expression.source())
            });
            // The pool fixtures' tolerance: a solver-produced point can sit a
            // hair outside where a sampled one never does. A non-finite residual
            // is counted too — it is not a satisfied constraint, and writing this
            // as a bare `>` would silently let NaN through.
            if !residual.is_finite() || residual > 1e-10 {
                infeasible += 1;
                worst = worst.max(residual);
            }
        }
    }
    if infeasible > 0 {
        complaints.push(format!(
            "{infeasible} constraint violations across {} points, worst residual {worst:e}",
            points.len()
        ));
    }

    // 3. Coverage. The claim a solver alone cannot satisfy: one witness repeated
    //    is feasible and countable and explores nothing.
    for (name, wanted_fraction) in case.coverage {
        let index = names
            .iter()
            .position(|n| n == name)
            .unwrap_or_else(|| panic!("{}: no variable named {name}", case.what));
        let (_, low, high) = case.variables[index];

        let values: Vec<f64> = points.iter().map(|point| point[index]).collect();
        let span = match (
            values.iter().copied().reduce(f64::min),
            values.iter().copied().reduce(f64::max),
        ) {
            (Some(min), Some(max)) => (max - min) / (high - low),
            _ => 0.0,
        };
        let distinct = {
            let mut sorted = values.clone();
            sorted.sort_by(f64::total_cmp);
            sorted.dedup();
            sorted.len()
        };

        if span < *wanted_fraction {
            complaints.push(format!(
                "{name} spans {:.4}% of its range, wanted {:.1}% \
                 ({distinct} distinct values in {} points)",
                span * 100.0,
                wanted_fraction * 100.0,
                points.len()
            ));
        }
    }

    // 4. Occupancy. Extent is satisfiable by a few points at the edges of the
    //    box; this is not, and the gap between them is the difference between
    //    having *found* the feasible set and being able to move along it.
    if let Some(grid) = &case.occupancy {
        let (over, divisions, least) = match grid {
            Occupancy::Joint {
                over,
                divisions,
                least,
            }
            | Occupancy::Marginal {
                over,
                divisions,
                least,
            } => (*over, *divisions, *least),
        };

        let axis_of = |name: &str| {
            names
                .iter()
                .position(|candidate| *candidate == name)
                .unwrap_or_else(|| panic!("{}: no variable named {name}", case.what))
        };
        // Which bin of `divisions` a point falls in on one coordinate, or `None`
        // outside the declared range, where it has no bin.
        let bin = |point: &[f64], axis: usize| {
            let (_, low, high) = case.variables[axis];
            let scaled = (point[axis] - low) / (high - low);
            if !(0.0..=1.0).contains(&scaled) {
                return None;
            }
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "scaled is guarded to 0.0..=1.0 on the line above"
            )]
            let index = (scaled * divisions as f64) as usize;
            Some(index.min(divisions - 1))
        };

        match grid {
            Occupancy::Joint { .. } => {
                let axes: Vec<usize> = over.iter().map(|name| axis_of(name)).collect();
                let occupied: std::collections::BTreeSet<Vec<usize>> = points
                    .iter()
                    .filter_map(|point| axes.iter().map(|axis| bin(point, *axis)).collect())
                    .collect();

                let cells = divisions.pow(
                    u32::try_from(over.len()).expect("a joint grid is over a handful of axes"),
                );
                if occupied.len() < least {
                    complaints.push(format!(
                        "occupies {} of {cells} cells over {over:?}, wanted {least}.{}",
                        occupied.len(),
                        " A search that only finds the feasible set fills about as many cells as it has seeds; one that moves along it fills far more",
                    ));
                }
            }

            Occupancy::Marginal { .. } => {
                // The *worst* coordinate, not the average: a search that fills
                // one marginal and freezes another has not traversed the set,
                // and an average would let the good one pay for the bad.
                let worst = over
                    .iter()
                    .map(|name| {
                        let axis = axis_of(name);
                        let filled: std::collections::BTreeSet<usize> =
                            points.iter().filter_map(|point| bin(point, axis)).collect();
                        (filled.len(), *name)
                    })
                    .min();

                if let Some((filled, name)) = worst
                    && filled < least
                {
                    complaints.push(format!(
                        "the emptiest of {} marginals is {name}, at {filled} of {divisions} bins, wanted {least}.{}",
                        over.len(),
                        " Seeds fill at most one bin each in every marginal at once; only moving along the set fills more",
                    ));
                }
            }
        }
    }

    assert!(
        complaints.is_empty(),
        "{}:\n  - {}",
        case.what,
        complaints.join("\n  - ")
    );
}

// ---------------------------------------------------------------------------
// A — pinned to a constant
// ---------------------------------------------------------------------------

/// One dimension pinned, one free. The pinned dimension should stop being
/// searched at all; the free one should still be explored.
///
/// `cvg_pools::constants` is the loose version of this and passes, because at
/// `+/- 0.001` the band is a thousandth of the box and the walker can move
/// inside it. At `1e-9` there is nothing to move in, so `x1` has to come from
/// somewhere other than sampling — and `x2`, which no constraint mentions, has
/// to keep being explored anyway. A pipeline that solves the whole point at once
/// gets `x1` right and `x2` frozen.
#[pollster::test]
async fn a_pinned_variable_does_not_freeze_the_free_one() {
    assert_explores(Case {
        what: "A (pinned): x1 == pi at 1e-9, x2 unconstrained",
        variables: &[("x1", 0.0, 10.0), ("x2", 0.0, 10.0)],
        sources: &["x1 == pi +/- 0.000000001"],
        wanted: 200,
        occupancy: None,
        coverage: &[("x2", 0.8)],
    })
    .await;
}

/// Every dimension pinned, which is as close to *fully determined* as babel can
/// currently express — and the gap between those two is the point.
///
/// This was written expecting a short `take`: the notes describe a fully
/// determined system as one where the feasible set is a single point and "give
/// me two hundred distinct points" has no answer. **It passes**, and the reason
/// is that a tolerance is not optional in this language. `+/- 1e-9` leaves a
/// `2e-9` square, which holds an enormous number of representable doubles, so
/// two hundred distinct feasible points genuinely exist and the pool finds them.
///
/// The determined case only appears once the tolerance is *dropped* for the
/// solver, which is the surface-then-band work and cannot be written here yet.
/// So this stands as the guard on the near-miss repair instead: at `1e-9` the
/// band is thin enough that a solver witness rounding a hair outside is a real
/// possibility, and this is what would catch a regression in `cvg::repaired`.
#[pollster::test]
async fn a_pinned_system_is_still_satisfiable_because_the_tolerance_is_not_optional() {
    assert_explores(Case {
        what: "A (pinned, both dimensions): the closest thing to fully determined",
        variables: &[("x1", 0.0, 10.0), ("x2", 0.0, 10.0)],
        sources: &["x1 == pi +/- 0.000000001", "x2 == e +/- 0.000000001"],
        wanted: 200,
        occupancy: None,
        coverage: &[],
    })
    .await;
}

// ---------------------------------------------------------------------------
// B — driven
// ---------------------------------------------------------------------------

/// `y` is determined by `x`: choose `x`, evaluate, done. No search in `y` at
/// all, and `x` free across its whole range.
///
/// `cvg_pools::a_constraint_nothing_can_reason_about_still_yields_points_and_says_so`
/// is the same constraint at `1e-6` over a box containing the origin, and it
/// passes for a reason its own doc comment admits is luck: `sin` is refused by
/// the emitter, so Z3 sees only the bounds, returns a point near the origin, and
/// `sin(0) = 0` puts that point on the curve by accident.
///
/// This moves the box to `2..3`, where the origin is not available and the luck
/// runs out, and tightens the tolerance by three orders. Driving `y` from `x`
/// makes it trivial; not driving it makes it impossible.
#[pollster::test]
async fn a_driven_variable_is_evaluated_not_searched() {
    assert_explores(Case {
        what: "B (driven): y == sin(x) at 1e-9, box away from the origin",
        variables: &[("x", 2.0, 3.0), ("y", -1.0, 1.0)],
        sources: &["y == sin(x) +/- 0.000000001"],
        wanted: 200,
        occupancy: None,
        coverage: &[("x", 0.8)],
    })
    .await;
}

/// Driven through a function the emitter *can* translate, so the solver is not
/// the thing standing in the way — the missing classification is.
///
/// `cvg_pools::roots` is this pair at `1e-4`. Z3 handles `sqrt` and `cbrt` as
/// polynomial constraints, so it will answer; the question is whether anything
/// then explores the curve rather than sitting on the one witness.
#[pollster::test]
async fn a_driven_variable_a_solver_can_reach_is_still_explored() {
    assert_explores(Case {
        what: "B (driven, translatable): x1 == sqrt(x2), x3 == cbrt(x4) at 1e-9",
        variables: &[
            ("x1", 0.0, 10.0),
            ("x2", 0.0, 10.0),
            ("x3", 0.0, 10.0),
            ("x4", 0.0, 10.0),
        ],
        sources: &[
            "x1 == sqrt(x2) +/- 0.000000001",
            "x3 == cbrt(x4) +/- 0.000000001",
        ],
        wanted: 200,
        occupancy: None,
        coverage: &[("x2", 0.5), ("x4", 0.5)],
    })
    .await;
}

// ---------------------------------------------------------------------------
// C — driven after rearrangement
// ---------------------------------------------------------------------------

/// `x2` appears on both sides, but linearly, so gathering terms turns this into
/// B: `0.5*x2 = x1 - x3/x4`.
///
/// `cvg_pools::simple_arithmetic` is this at `1e-5`. The rearrangement is the
/// whole content of row C, and nothing in the pipeline performs it today — so
/// what this measures is how much the loose tolerance was covering for.
#[pollster::test]
async fn a_variable_on_both_sides_linearly_is_still_driven() {
    assert_explores(Case {
        what: "C (rearrangeable): x2 == x1 + 1/2*x2 - x3/x4 at 1e-9",
        variables: &[
            ("x1", 0.0, 10.0),
            ("x2", 0.0, 10.0),
            ("x3", 0.0, 10.0),
            ("x4", 1.0, 10.0),
        ],
        sources: &["x2 == x1 + 1/2*x2 - x3 / x4 +/- 0.000000001"],
        wanted: 200,
        occupancy: None,
        coverage: &[("x1", 0.5), ("x3", 0.5)],
    })
    .await;
}

/// The same shape, asked a question a scattering of seeds cannot answer.
///
/// `a_variable_on_both_sides_linearly_is_still_driven` passes, and it passes
/// **without the rearrangement it is named for**: `classify::shape` still
/// reports this `Opaque`, and the points come from the pool's gap-covering
/// solver calls rather than from anything understanding that `0.5*x2 = x1 -
/// x3/x4`. A span assertion cannot tell those apart, because gap-covering places
/// its seeds at the edges of the box by construction — which is exactly where a
/// span is measured.
///
/// Occupancy can. Gathering the linear terms makes `x2` driven and leaves `x1`,
/// `x3` and `x4` free, so the feasible set is a three-dimensional sheet and a
/// walker moving in it fills the `(x1, x3)` plane densely.
///
/// Nearly every cell of the 8x8 grid is reachable: the constraint needs some
/// `x4` in `1..10` with `x1 - 5 <= x3/x4 <= x1`, and since `x3/x4` sweeps
/// `x3/10 ..= x3` there is one for all but the corner where both `x1` and `x3`
/// are near zero. So `least: 40` asks for under two thirds of what exists —
/// while still being well above the pool's seed budget, which is what makes it
/// discriminate.
#[pollster::test]
async fn a_rearrangeable_equality_is_traversed_not_merely_reached() {
    assert_explores(Case {
        what: "C (rearrangeable, strict): x2 == x1 + 1/2*x2 - x3/x4 at 1e-9",
        variables: &[
            ("x1", 0.0, 10.0),
            ("x2", 0.0, 10.0),
            ("x3", 0.0, 10.0),
            ("x4", 1.0, 10.0),
        ],
        sources: &["x2 == x1 + 1/2*x2 - x3 / x4 +/- 0.000000001"],
        wanted: 500,
        coverage: &[("x1", 0.5), ("x3", 0.5)],
        occupancy: Some(Occupancy::Joint {
            over: &["x1", "x3"],
            divisions: 8,
            least: 40,
        }),
    })
    .await;
}

/// Row C in twenty-four free dimensions, which is where the joint grid stops
/// working and the marginals have to carry it.
///
/// `y == y/2 + mean(x1..x24)` gathers to `y = 2 * mean(x1..x24)`: one driven
/// coordinate over a twenty-four dimensional sheet. The three-variable case is
/// the same shape with the dimension count too low to expose anything.
///
/// # Why the free marginals are uniform, which is what makes this assertable
///
/// The `xi` are otherwise unconstrained, and `y`'s declared box is `0..2` —
/// exactly the range `2 * mean(x1..x24)` can produce when every `xi` is in
/// `0..1`. **`y`'s bounds therefore never bind**, so the feasible set projects
/// onto the `xi` as the whole unit cube and each `xi` is uniform on `0..1` over
/// it. That is an absolute claim needing no reference sampler — the same reason
/// `top_corner_200d` can be asserted at all.
///
/// # Why marginal occupancy and not a joint grid
///
/// Cost, and an honest account of what neither one buys.
///
/// A joint grid over twenty-four coordinates at eight divisions would hold
/// `8^24` cells — `2^72`, which does not fit in a `usize`, so the arithmetic
/// alone rules it out. Per-coordinate histograms cost `points * coordinates` and
/// are unchanged at any dimension.
///
/// **What is not true is that the joint grid would be weaker here.** Both catch
/// the failure this fixture is aimed at, because a handful of seeds with stuck
/// chains fills about as many cells as it has seeds either way. And both are
/// blind to the same thing: a sample strung along a *curve* through the sheet
/// fills every marginal and lands in five hundred distinct joint cells, and
/// neither instrument can tell it from a genuine twenty-four dimensional fill.
///
/// That blind spot is tolerable rather than solved. A curve is not something
/// either the current pipeline or the intended one produces — gathering the
/// linear terms leaves the walker moving freely in all twenty-four free
/// coordinates, which is a full-dimensional walk. If an implementation ever does
/// traverse a sub-manifold, this fixture will pass and be wrong, and the
/// instrument for that case is a nearest-neighbour or random-projection check
/// that nothing here has needed yet.
///
/// Forty bins with thirty required is well clear of the pool's seed budget: a
/// search that only *finds* the sheet delivers a handful of solver seeds and
/// whatever neighbourhood each stuck chain can reach, filling at most one bin
/// per seed **in every marginal at once**. Filling thirty needs the walker to
/// move across the sheet, which is what gathering the linear terms buys.
#[pollster::test]
async fn a_rearrangeable_equality_is_traversed_in_many_dimensions() {
    const FREE: usize = 24;

    let names: Vec<String> = (1..=FREE).map(|index| format!("x{index}")).collect();
    let terms = names.join(" + ");
    let source = format!("y == y/2 + ({terms})/{FREE}.0 +/- 0.000000001");

    let mut variables: Vec<(&str, f64, f64)> = vec![("y", 0.0, 2.0)];
    variables.extend(names.iter().map(|name| (name.as_str(), 0.0, 1.0)));

    let watched: Vec<&str> = names.iter().map(String::as_str).collect();

    assert_explores(Case {
        what: "C (rearrangeable, 24 free dimensions): y == y/2 + mean(x1..x24) at 1e-9",
        variables: &variables,
        sources: &[&source],
        wanted: 500,
        // Extent is left to the occupancy claim, which subsumes it: a marginal
        // cannot fill thirty of forty bins without also spanning the range.
        coverage: &[],
        occupancy: Some(Occupancy::Marginal {
            over: &watched,
            divisions: 40,
            least: 30,
        }),
    })
    .await;
}

// ---------------------------------------------------------------------------
// D — multi-valued
// ---------------------------------------------------------------------------

/// Two roots, and both have to receive points.
///
/// `cvg_pools::absolute_value` asks only for feasibility, which one branch
/// satisfies completely. The failure mode a branch-blind pipeline has is
/// invisible to that assertion and obvious to this one: a solver picks a branch,
/// the walker seeds from it, and shrinkage keeps it there forever because the
/// components are disjoint and a chain cannot cross the gap.
///
/// Coverage is the wrong instrument for a two-point set, so this states it
/// directly: `x1` must span at least the distance between the branches.
#[pollster::test]
async fn both_branches_of_an_absolute_value_receive_points() {
    assert_explores(Case {
        what: "D (multi-valued): abs(x1) == 1 at 1e-9, branches at -1 and +1",
        variables: &[("x1", -2.0, 2.0)],
        sources: &["abs(x1) == 1 +/- 0.000000001"],
        wanted: 200,
        // The branches sit at -1 and +1 in a range of 4. Reaching both spans
        // 0.5 of it and reaching one spans ~0, so anything above ~0.1 is the
        // answer — the threshold is not 0.5 because that would additionally
        // require the extreme point of each band, which is not the claim.
        occupancy: None,
        coverage: &[("x1", 0.4)],
    })
    .await;
}

/// The same claim on the shape the benchmarks already measure distribution for,
/// so a fix can be checked against `cvg_benchmarks::parabolic_roots_*` for
/// uniformity rather than merely for reach.
#[pollster::test]
async fn both_bands_of_a_parabola_receive_points() {
    assert_explores(Case {
        what: "D (multi-valued): (x + 2) * (x - 1) == 0 at 1e-9, bands at -2 and 1",
        variables: &[("x", -5.0, 5.0)],
        sources: &["(x + 2) * (x - 1) == 0 +/- 0.000000001"],
        wanted: 200,
        // Bands at -2 and 1 in a range of 10, so reaching both spans 0.3 and
        // reaching one spans ~0. Below that, for the reason above.
        occupancy: None,
        coverage: &[("x", 0.25)],
    })
    .await;
}

// ---------------------------------------------------------------------------
// E — under-determined
// ---------------------------------------------------------------------------

/// Two equations, three variables, one degree of freedom left — and *which* two
/// variables get driven is a choice, which is where matching earns its keep.
///
/// `cvg_pools::dynamic_variable_lookup` is this at `1e-3`. At `1e-9` the
/// feasible set is a line segment in a three-dimensional box, so points exist in
/// quantity but only along one direction, and finding that direction is the
/// whole problem.
#[pollster::test]
async fn an_under_determined_system_explores_its_remaining_freedom() {
    assert_explores(Case {
        what: "E (under-determined): two equations, three variables, at 1e-9",
        variables: &[("x1", -1.0, 1.0), ("x2", -2.0, 2.0), ("x3", -2.0, 2.0)],
        sources: &[
            "1.5 == var[1] + var[2] +/- 0.000000001",
            "1.5 == var[2] - var[3] +/- 0.000000001",
        ],
        wanted: 200,
        // One degree of freedom, so the segment runs diagonally and every
        // coordinate moves along it. `x1` is the narrowest and bounds the rest.
        occupancy: None,
        coverage: &[("x1", 0.5)],
    })
    .await;
}

/// The same system, asked to fill its one degree of freedom rather than reach
/// the ends of it.
///
/// Two equations over three variables leave a line segment, and
/// `an_under_determined_system_explores_its_remaining_freedom` is satisfied by
/// two points at its ends. Matching would pick two variables to drive and leave
/// the third free, after which the segment is traversed by moving that one
/// coordinate — and the histogram over `x2` fills.
///
/// # Where the numbers come from
///
/// They are bounded by the geometry and not chosen for difficulty.
/// `x2 = 1.5 - x1` with `x1` in `-1..1` puts `x2` in `0.5..2.5`, and
/// `x3 = x2 - 1.5` with `x3` in `-2..2` puts it in `-0.5..3.5`; together
/// **`x2` can only reach `0.5..2`**, which is 37.5% of its declared `-2..2`. So
/// a span assertion above `0.375` is unsatisfiable however good the search is,
/// and `0.3` is the honest ask.
///
/// That also sets the grid. Over `-2..2` the reachable stretch is 30 of 80 bins,
/// so `least: 24` is four fifths of what exists — and, being well above the
/// pool's seed budget, is not reachable by scattering seeds. Forty bins would
/// have left only fifteen reachable, *below* the budget, and would have proved
/// nothing.
///
/// `x2` rather than `x1` because `x2` appears in both equations and is the
/// coordinate a wrong matching is most likely to freeze.
#[pollster::test]
async fn an_under_determined_system_fills_its_remaining_freedom() {
    assert_explores(Case {
        what: "E (under-determined, strict): two equations, three variables, at 1e-9",
        variables: &[("x1", -1.0, 1.0), ("x2", -2.0, 2.0), ("x3", -2.0, 2.0)],
        sources: &[
            "1.5 == var[1] + var[2] +/- 0.000000001",
            "1.5 == var[2] - var[3] +/- 0.000000001",
        ],
        wanted: 500,
        coverage: &[("x2", 0.3)],
        occupancy: Some(Occupancy::Joint {
            over: &["x2"],
            divisions: 80,
            least: 24,
        }),
    })
    .await;
}

// ---------------------------------------------------------------------------
// F — implicit, and refused
// ---------------------------------------------------------------------------

/// Row F is rejected when the system is built, rather than searched for and not
/// found.
///
/// `x == sin(x)` is a variable defined in terms of itself through a function no
/// solver will take. Self-reference means no rearrangement and the refused term
/// means no solver, so nothing is left but rejection sampling looking for a
/// measure-zero set by luck.
///
/// The old behaviour was to try: a solver call, several thousand samples, and
/// then `Unsatisfiable { NotFound }` — which is honest but tells a caller nothing
/// about what to change. This says what is wrong and names the operation.
///
/// **It is a refusal and not a claim of emptiness.** `sin(x) == x/2` has three
/// solutions and `x == sin(x)` has one at the origin; reporting them
/// unsatisfiable would be false. `SystemError` is the right shape for "we will
/// not try" and `Satisfiability::Unsatisfiable` is not.
#[pollster::test]
async fn an_implicit_equality_is_refused_by_name() {
    let error = ConstraintSystem::new(
        vec![InputVariable::new("x", -2.0, 2.0)],
        constraints(&["x == sin(x) +/- 0.001"]),
    )
    .expect_err("a self-referential transcendental should be refused");

    let SystemError::Implicit {
        variable, because, ..
    } = &error
    else {
        panic!("expected an implicit-equality refusal, got {error:?}");
    };
    assert_eq!(variable, "x");
    assert_eq!(*because, "sin");

    let message = error.to_string();
    for expected in ["x", "sin", "rearrange"] {
        assert!(
            message.contains(expected),
            "the message should mention {expected:?}: {message}"
        );
    }
}

/// The refusal must not reach past row F, and the cost of it doing so is high:
/// each of these is solved by the pool today.
///
/// `x2 == ...` is row C, one rearrangement from driven and carrying three of the
/// tests in this file. `x == x*x + 2` is a quadratic Z3 answers without
/// complaint. Both have the variable on both sides, which is why "self
/// referential" is the wrong line to draw and "self referential *and* beyond
/// every solver" is the right one.
#[pollster::test]
async fn a_solvable_equality_is_not_refused_for_naming_itself() {
    for source in [
        "x2 == x1 + 1/2*x2 - x3 / x4 +/- 0.001",
        "x == x*x + 2 +/- 0.001",
    ] {
        let variables = vec![
            InputVariable::new("x", 0.0, 10.0),
            InputVariable::new("x1", 0.0, 10.0),
            InputVariable::new("x2", 0.0, 10.0),
            InputVariable::new("x3", 0.0, 10.0),
            InputVariable::new("x4", 1.0, 10.0),
        ];
        assert!(
            ConstraintSystem::new(variables, constraints(&[source])).is_ok(),
            "{source} is solvable and must not be refused"
        );
    }
}

// ---------------------------------------------------------------------------
// The floor
// ---------------------------------------------------------------------------

/// Where the current pipeline stops handling an equality, measured rather than
/// guessed.
///
/// Every case above picks `1e-9` because it is obviously past the floor. This
/// walks a shape down from a tolerance that works until it stops, and reports
/// the last one that did. The number in the assertion is a *ratchet*: it records
/// what the pipeline could do when this was written, so tightening it later is a
/// deliberate edit and loosening it is a regression that has to be argued for.
///
/// `x1 == x2 +/- t` is the simplest possible equality — one linear equation,
/// two variables, one degree of freedom, no branches and nothing to invert. If
/// anything holds at a tight tolerance it is this, so it is the fairest place to
/// put the floor.
#[pollster::test]
async fn the_tolerance_floor_is_where_it_was_left() {
    /// The tightest tolerance `x1 == x2 +/- t` is currently expected to survive.
    ///
    /// Was `1e-4` before the classifier: rejection sampling and a chord drawn
    /// through both coordinates, so the band had to have width for anything to
    /// land in it. With `x1` driven from `x2` the tolerance stops mattering
    /// almost entirely — this passes at `1e-17` too, and the ladder's floor of
    /// `1e-12` is the loop bound rather than a limit anything was found at.
    const EXPECTED_FLOOR: f64 = 1e-12;

    let mut last_good: Option<f64> = None;
    for exponent in 1..=12 {
        let tolerance = 10f64.powi(-exponent);
        // Written out rather than in scientific notation: the `+/-` position
        // takes a `literal`, and `1e-1` is not one — the lexer stops at the `e`.
        let source = format!("x1 == x2 +/- {tolerance:.*}", exponent as usize);
        let compiled = constraints(&[source.as_str()]);
        let inputs = vec![
            InputVariable::new("x1", 0.0, 10.0),
            InputVariable::new("x2", 0.0, 10.0),
        ];
        let system = ConstraintSystem::new(inputs.clone(), compiled.clone())
            .expect("the fixture should bind to its own box");

        let solution = ConstraintSolver::new()
            .with_rng(Xoshiro256PlusPlus::seed_from_u64(SEED))
            .solve(system)
            .await
            .unwrap_or_else(|e| panic!("solving {source:?} failed: {e}"));

        let Satisfiability::Satisfied { samples } = solution else {
            break;
        };
        let mut pool = samples;
        let points = columns(&pool.take(100));

        let feasible = points.iter().all(|point| {
            let bindings = [("x1", point[0]), ("x2", point[1])];
            babel::eval_one(&compiled[0], &bindings).is_ok_and(|residual| residual <= 1e-10)
        });
        let spread = points
            .iter()
            .map(|point| point[0])
            .fold(f64::NEG_INFINITY, f64::max)
            - points
                .iter()
                .map(|point| point[0])
                .fold(f64::INFINITY, f64::min);

        if points.len() == 100 && feasible && spread > 1.0 {
            last_good = Some(tolerance);
        } else {
            break;
        }
    }

    let floor = last_good.unwrap_or(f64::INFINITY);
    assert!(
        floor <= EXPECTED_FLOOR,
        "the tightest workable tolerance on `x1 == x2` is {floor:e}, \
         and the floor this test ratchets is {EXPECTED_FLOOR:e}. \
         If that is an improvement, tighten EXPECTED_FLOOR to match."
    );
}
