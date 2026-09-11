//! Regressions reported by callers, kept in the form they arrived in.
//!
//! Each case is a bug report: the system, the seed, and what the caller saw.
//! They are pinned to seeds on purpose — a regression that depends on the
//! probe's luck is only reproducible with the luck held still — and they stay
//! here after the fix so the luck can never turn again.

use sojourn::{ConstraintSolver, ConstraintSystem, InputVariable, Satisfiability, Status};

/// Artemis, 2026-09-11. `x1 == x2 + 1 +/- 0.01` over `[-32.768, 32.768]^20`:
/// a 0.02-wide slab in a 65.5-wide box, one driven variable, the rest free.
/// Seeds 0, 1 and 3..=9 streamed 1500 points. Seed 2 reported `Satisfied`,
/// delivered exactly 25 points, and ended in `Status::Exhausted`.
///
/// The pool used to pick a route on the probe, one batch, and on seed 2 it
/// landed enough hits to choose plain sampling for a region whose true rate
/// is a third of the threshold. The delivery batches then came back empty
/// three times in a row, which the fill loop read as the region running dry.
/// A connected region with 25 known feasible points cannot run dry. There is
/// no route now: every batch is sampled first and walked for the rest.
mod a_lucky_probe_must_not_strand_the_sampling_route {
    use super::*;

    const DIM: usize = 20;
    const HALF_WIDTH: f64 = 32.768;
    const SLAB: &str = "x1 == x2 + 1 +/- 0.01";
    const WANTED: usize = 1500;

    fn system() -> ConstraintSystem {
        let inputs: Vec<InputVariable> = (1..=DIM)
            .map(|i| InputVariable::new(format!("x{i}"), -HALF_WIDTH, HALF_WIDTH))
            .collect();
        ConstraintSystem::new(inputs, [SLAB]).expect("the slab binds to its box")
    }

    async fn points_from(seed: u64) -> (usize, Status) {
        let verdict = ConstraintSolver::new()
            .with_seed(seed)
            .solve(system())
            .await
            .expect("solve does not error");
        let mut samples = match verdict {
            Satisfiability::Satisfied { samples } => samples,
            Satisfiability::Unsatisfiable { because } => {
                panic!("seed {seed}: unsatisfiable: {because:?}")
            }
        };
        let got = samples.take(WANTED).ncols();
        (got, samples.status())
    }

    #[pollster::test]
    async fn seed_2_streams_the_whole_request() {
        let (got, status) = points_from(2).await;
        assert_eq!(
            got, WANTED,
            "seed 2 delivered {got} of {WANTED} and ended in {status:?}; a connected slab \
             with points in hand should never exhaust"
        );
        assert_eq!(status, Status::Filling);
    }

    #[pollster::test]
    async fn every_other_seed_streams_the_same_slab() {
        for seed in (0..10u64).filter(|s| *s != 2) {
            let (got, status) = points_from(seed).await;
            assert_eq!(got, WANTED, "seed {seed} ended early in {status:?}");
        }
    }
}
