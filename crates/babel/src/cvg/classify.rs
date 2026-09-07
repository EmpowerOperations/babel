//! What an equality lets us conclude about the variables it mentions.
//!
//! Babel admits one equality form, `a == b +/- t`, and it covers at least six
//! structurally different problems — the taxonomy is in the root `todo.md`.
//! Downstream, all six look identical: one residual, satisfied when `<= 0`. This
//! module is what tells them apart.
//!
//! # What it is for
//!
//! Not labelling constraints for their own sake. `tests/cvg_equalities.rs`
//! measured the actual defect and it is singular: on a tight equality the pool
//! delivers two hundred *distinct* feasible points spanning `0.0000%` of the
//! free coordinates' range. It finds the feasible set and cannot move along it.
//!
//! So the useful output is not a label, it is a **split**: which coordinates the
//! walker moves, and which it computes from them. Move `x`, evaluate
//! `y = sin(x)`, and a point that was on the curve stays on it — where a chord
//! drawn through both coordinates leaves it almost immediately.
//!
//! # Why this is `cvg`'s and not the front end's
//!
//! `eval` wants a number and has no use for any of this. The front end must not
//! know it either: a rewrite that dropped `y` from the problem would be lying to
//! the evaluator, which still has to compute the residual of the whole
//! constraint. This is a *reading* of the AST, and it changes nothing.
//!
//! # What is deliberately not here
//!
//! Rows C (`x2 == x1 + x2/2 - x3/x4`, driven after gathering linear terms), D
//! (`abs(x1) == 1`, two branches) and E (an under-determined system, where which
//! variables get driven is a choice) classify as [`Shape::Opaque`]. They are
//! named in the taxonomy and not yet analysed, and saying so is better than
//! guessing at them.
//!
//! Row F — `x == sin(x)`, the variable inside and outside a function no solver
//! will take — is [`Shape::Implicit`], and is refused by
//! [`ConstraintSystem::new`](crate::cvg::ConstraintSystem::new) rather than
//! searched for. No use case has turned up for `x == f(x)` and Newton is a
//! great deal of machinery to carry for a shape nobody writes. The line is
//! narrower than "self-referential", which would also reject row C and
//! `x^2 == x + 2`; see [`Shape::Implicit`].

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::{BinaryOp, Expr, GlobalId, Kind, Program, UnaryOp};
use crate::{Ast, CompiledExpression, Schema};

/// What one equality lets us conclude.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Shape {
    /// `x1 == pi +/- t`. One variable equals an expression naming no variable at
    /// all, so the dimension is gone: there is nothing to search. Row A.
    ///
    /// Reported separately from [`Driven`](Shape::Driven), which it is a special
    /// case of, because "constant" is the stronger statement and a caller may
    /// want it — a pinned coordinate need not even be re-evaluated per move.
    Pinned {
        variable: GlobalId,
        value: Ast,
        tolerance: f64,
    },
    /// `y == sin(x) +/- t`. `variable` stands alone on one side and appears
    /// nowhere on the other, so it is *computed* rather than searched. Row B.
    ///
    /// This needs no inverse. That is the whole reason it is the row worth doing
    /// first: nothing has to solve `sin` for anything, because the variable is
    /// already isolated and the other side is a formula for it.
    Driven {
        variable: GlobalId,
        from: Ast,
        tolerance: f64,
    },
    /// `x == sin(x) +/- t`. Row F: the variable is on both sides *and* is inside
    /// something no solver will reason about, so there is nothing left to try.
    ///
    /// **Not merely "on both sides".** That was the first cut and it was far too
    /// wide: `x2 == x1 + x2/2 - x3/x4` is on both sides and is row C, one
    /// rearrangement from being driven, and `x^2 == x + 2` is on both sides and
    /// Z3 answers it without complaint. Rejecting either would refuse a
    /// constraint the pool solves today.
    ///
    /// The line is the *emitter's* — [`unexpressible`]. Self-reference means no
    /// rearrangement, and a term the emitter refuses means no solver either, so
    /// together they leave nothing: sampling would have to land on a
    /// measure-zero set by luck. That combination is what
    /// [`SystemError::Implicit`](crate::cvg::SystemError::Implicit) refuses at
    /// construction, and refusing it is kinder than the `NotFound` it produces
    /// today after a solver call and a few thousand wasted samples.
    Implicit {
        variable: GlobalId,
        /// The operation that put it beyond reach, for the diagnostic.
        because: &'static str,
    },
    /// Nothing structural to say: rows C, D and E, plus anything that is not an
    /// equality at all.
    Opaque,
}

/// Which coordinates the walker moves, and which it computes from them.
///
/// Positions are into the bound [`Schema`], not into any one constraint's
/// symbol list, because the walker works in whole points.
pub(crate) struct Plan {
    /// Driven coordinates with the expression that defines each and the
    /// tolerance of the equality that defined it, **in evaluation order**. A
    /// driven variable may be defined in terms of another driven variable, and
    /// computing them out of order reads a stale value.
    driven: Vec<Drive>,
    /// Positions the walker is free to move. Everything not driven.
    free: Vec<usize>,
}

impl Plan {
    pub(crate) fn free(&self) -> &[usize] {
        &self.free
    }

    pub(crate) fn driven(&self) -> &[Drive] {
        &self.driven
    }
}

/// One coordinate the walker computes rather than searches.
pub(crate) struct Drive {
    /// Position in the bound [`Schema`].
    pub(crate) position: usize,
    /// The other side of the equality, as a scalar expression.
    pub(crate) definition: CompiledExpression,
    /// The equality's tolerance, which is **half the width of the slice this
    /// coordinate may occupy** once the others are fixed.
    ///
    /// Carried because driving to the centre of that slice would be wrong. See
    /// [`Plan`] and `SearchContext::retract`.
    pub(crate) tolerance: f64,
}

/// The shape of one constraint.
///
/// Infallible and total: anything it cannot read is [`Shape::Opaque`], which is
/// the same answer as "not analysed yet" and is always safe — a caller that
/// learns nothing does what it did before.
pub(crate) fn shape(constraint: &Ast) -> Shape {
    // A `var[i]` subscript reads a variable the expression never names, so the
    // static symbol list is not the whole story and a "does it appear on the
    // other side" test cannot be answered. `Ast` documents this and it is
    // exactly the trap it warns about.
    if constraint.contains_dynamic_lookup {
        return Shape::Opaque;
    }
    if !constraint.is_constraint {
        return Shape::Opaque;
    }

    // A block with local bindings could still be an equality, but its sides are
    // not reachable without substituting the assignments through, which is a
    // rewrite and not a reading.
    let Program { body, .. } = &constraint.program;
    if !body.assignments.is_empty() {
        return Shape::Opaque;
    }
    let Kind::NearEq {
        lhs,
        rhs,
        tolerance,
    } = &body.result.kind
    else {
        return Shape::Opaque;
    };

    // Both orders, since `y == sin(x)` and `sin(x) == y` say the same thing.
    for (candidate, other) in [(lhs, rhs), (rhs, lhs)] {
        let Kind::Global(variable) = candidate.kind else {
            continue;
        };

        if mentions(other, variable) {
            // On both sides, so no evaluation order defines one from the other.
            // Whether that is fatal depends on what it is *inside*: a linear or
            // polynomial occurrence is rearrangeable or solvable and stays
            // `Opaque` for the row C work, while one under a transcendental is
            // beyond both and is refused.
            return match unexpressible(&body.result) {
                Some(because) => Shape::Implicit { variable, because },
                None => Shape::Opaque,
            };
        }

        let from = detach(constraint, other);
        let tolerance = tolerance.abs();
        return if globals(other).is_empty() {
            Shape::Pinned {
                variable,
                value: from,
                tolerance,
            }
        } else {
            Shape::Driven {
                variable,
                from,
                tolerance,
            }
        };
    }

    Shape::Opaque
}

/// The split the walker consumes, over a whole system.
///
/// Returns `None` when nothing is driven, so a caller can keep its existing path
/// rather than carry an empty plan through it.
///
/// # Which drives are taken
///
/// Not all of them. A variable can only be defined once, a definition must not
/// depend on itself however indirectly, and a variable the schema does not name
/// cannot be a coordinate. Every drive refused this way leaves its constraint
/// exactly as it was — **no constraint is ever dropped**, because the walker
/// still checks feasibility against all of them. A refused drive costs
/// efficiency and can never cost correctness.
pub(crate) fn plan(constraints: &[Ast], schema: &Schema) -> Option<Plan> {
    // Position in the schema, and the expression defining it.
    let mut definitions: BTreeMap<usize, (Ast, f64, BTreeSet<usize>)> = BTreeMap::new();

    for constraint in constraints {
        // Pinned and driven are handled identically here: a pinned variable is
        // a driven one whose definition happens to read nothing.
        let (variable, from, tolerance) = match shape(constraint) {
            Shape::Driven {
                variable,
                from,
                tolerance,
            }
            | Shape::Pinned {
                variable,
                value: from,
                tolerance,
            } => (variable, from, tolerance),
            Shape::Implicit { .. } | Shape::Opaque => continue,
        };

        let Some(position) = position_in(schema, constraint, variable) else {
            continue;
        };
        // Defined twice. Only one definition can hold, and choosing between them
        // is row E's matching problem rather than something to decide by which
        // constraint was written first — so take neither, and let both stay
        // ordinary constraints.
        if definitions.contains_key(&position) {
            definitions.remove(&position);
            continue;
        }

        let dependencies: BTreeSet<usize> = globals(&from.program.body.result)
            .into_iter()
            .filter_map(|global| position_in(schema, &from, global))
            .collect();
        definitions.insert(position, (from, tolerance, dependencies));
    }

    // Evaluation order, by repeatedly taking a definition whose dependencies are
    // all either free or already ordered. What is left over when nothing more
    // can be taken is a cycle, and every member of it stays undriven.
    let mut ordered: Vec<usize> = Vec::new();
    let mut settled: BTreeSet<usize> = BTreeSet::new();
    loop {
        let ready: Vec<usize> = definitions
            .iter()
            .filter(|(position, _)| !settled.contains(position))
            .filter(|(_, (_, _, dependencies))| {
                dependencies
                    .iter()
                    .all(|d| settled.contains(d) || !definitions.contains_key(d))
            })
            .map(|(position, _)| *position)
            .collect();
        if ready.is_empty() {
            break;
        }
        for position in ready {
            settled.insert(position);
            ordered.push(position);
        }
    }

    let mut driven = Vec::new();
    for position in ordered {
        let (from, tolerance, _) = &definitions[&position];
        // A definition that will not compile against the schema is no use, and
        // is not an error: the constraint stays, undriven.
        if let Ok(definition) = crate::compile(from, schema) {
            driven.push(Drive {
                position,
                definition,
                tolerance: *tolerance,
            });
        }
    }

    if driven.is_empty() {
        return None;
    }

    let taken: BTreeSet<usize> = driven.iter().map(|drive| drive.position).collect();
    let free = (0..schema.len()).filter(|p| !taken.contains(p)).collect();

    Some(Plan { driven, free })
}

/// Where `variable` sits in the schema, given the constraint that named it.
///
/// [`GlobalId`] indexes a constraint's own symbol list, not the schema, so this
/// is a hop through the name. `None` means the schema does not carry it, which
/// binding would already have rejected — but this module must not panic on a
/// caller's behalf.
fn position_in(schema: &Schema, constraint: &Ast, variable: GlobalId) -> Option<usize> {
    let name = constraint.symbols.get(variable.index())?;
    schema
        .names()
        .iter()
        .position(|candidate| candidate == name)
}

/// One side of an equality as a standalone scalar expression.
///
/// Shares the parent's symbol list so [`GlobalId`]s keep meaning what they meant,
/// and is *not* a constraint: it computes a value rather than a residual.
fn detach(parent: &Ast, side: &Expr) -> Ast {
    Ast {
        source: format!("{} [from {}]", side_source(side), parent.source),
        program: Program {
            body: crate::ast::Block {
                assignments: Vec::new(),
                result: side.clone(),
            },
            frame_size: parent.program.frame_size,
        },
        symbols: parent.symbols.clone(),
        contains_dynamic_lookup: false,
        is_constraint: false,
    }
}

/// A name for a detached side, for diagnostics only.
///
/// The AST does not carry source text per node, and the [`Span`] it does carry
/// indexes the parent's source — which `detach` no longer has in the same
/// offsets. So this names the shape rather than quoting it.
///
/// [`Span`]: crate::diagnostics::Span
fn side_source(side: &Expr) -> &'static str {
    match side.kind {
        Kind::Literal(_) => "literal",
        Kind::Global(_) => "variable",
        _ => "expression",
    }
}

/// The first operation in `expr` that no solver will be asked about, if there is
/// one.
///
/// Deliberately the same set [`crate::cvg::emit`] refuses — the transcendentals,
/// a logarithm, and a non-constant exponent. Keeping one line rather than two
/// means this cannot come to disagree with the emitter about what is reachable,
/// and disagreeing would be the whole bug: refusing a constraint at construction
/// that a solver would in fact have answered.
fn unexpressible(expr: &Expr) -> Option<&'static str> {
    match &expr.kind {
        Kind::Unary { op, arg } => match op {
            UnaryOp::Ln => Some("ln"),
            UnaryOp::Log10 => Some("log10"),
            UnaryOp::Sin => Some("sin"),
            UnaryOp::Cos => Some("cos"),
            UnaryOp::Tan => Some("tan"),
            UnaryOp::Asin => Some("asin"),
            UnaryOp::Acos => Some("acos"),
            UnaryOp::Atan => Some("atan"),
            UnaryOp::Sinh => Some("sinh"),
            UnaryOp::Cosh => Some("cosh"),
            UnaryOp::Tanh => Some("tanh"),
            UnaryOp::Cot => Some("cot"),
            _ => unexpressible(arg),
        },
        Kind::Binary { op, lhs, rhs } => match op {
            BinaryOp::LogB => Some("log"),
            // A whole-number exponent is expanded to multiplication before this
            // runs, so anything still here is a real or variable one.
            BinaryOp::Pow => Some("^"),
            _ => unexpressible(lhs).or_else(|| unexpressible(rhs)),
        },
        Kind::Literal(_) | Kind::Global(_) | Kind::Local(_) => None,
        Kind::Compare { lhs, rhs, .. } | Kind::NearEq { lhs, rhs, .. } => {
            unexpressible(lhs).or_else(|| unexpressible(rhs))
        }
        Kind::And { terms } | Kind::Fold { terms, .. } => terms.iter().find_map(unexpressible),
        Kind::DynamicIndex(index) => unexpressible(index),
        Kind::Block(block) => block
            .assignments
            .iter()
            .find_map(|assignment| unexpressible(&assignment.value))
            .or_else(|| unexpressible(&block.result)),
        Kind::Aggregate {
            lower, upper, body, ..
        } => unexpressible(lower)
            .or_else(|| unexpressible(upper))
            .or_else(|| {
                body.assignments
                    .iter()
                    .find_map(|assignment| unexpressible(&assignment.value))
            })
            .or_else(|| unexpressible(&body.result)),
    }
}

/// Whether `variable` is read anywhere in `expr`.
fn mentions(expr: &Expr, variable: GlobalId) -> bool {
    globals(expr).contains(&variable)
}

/// Every global read anywhere in `expr`.
fn globals(expr: &Expr) -> BTreeSet<GlobalId> {
    let mut found = BTreeSet::new();
    collect(expr, &mut found);
    found
}

fn collect(expr: &Expr, found: &mut BTreeSet<GlobalId>) {
    match &expr.kind {
        Kind::Global(id) => {
            found.insert(*id);
        }
        Kind::Literal(_) | Kind::Local(_) => {}
        Kind::Unary { arg, .. } => collect(arg, found),
        Kind::Binary { lhs, rhs, .. } | Kind::Compare { lhs, rhs, .. } => {
            collect(lhs, found);
            collect(rhs, found);
        }
        Kind::NearEq { lhs, rhs, .. } => {
            collect(lhs, found);
            collect(rhs, found);
        }
        Kind::And { terms } | Kind::Fold { terms, .. } => {
            for term in terms {
                collect(term, found);
            }
        }
        Kind::DynamicIndex(index) => collect(index, found),
        Kind::Block(block) => {
            for assignment in &block.assignments {
                collect(&assignment.value, found);
            }
            collect(&block.result, found);
        }
        Kind::Aggregate {
            lower, upper, body, ..
        } => {
            collect(lower, found);
            collect(upper, found);
            for assignment in &body.assignments {
                collect(&assignment.value, found);
            }
            collect(&body.result, found);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Ast {
        crate::parse(source).expect("test constraint should compile")
    }

    /// The variable a shape names, by name rather than by `GlobalId`, since an
    /// id is only meaningful against the constraint that produced it.
    fn driven_name(constraint: &Ast) -> Option<&str> {
        match shape(constraint) {
            Shape::Driven { variable, .. } | Shape::Pinned { variable, .. } => {
                Some(&constraint.symbols[variable.index()])
            }
            _ => None,
        }
    }

    #[test]
    fn a_bare_variable_on_one_side_is_driven() {
        let constraint = parse("y == sin(x) +/- 0.001");
        assert_eq!(driven_name(&constraint), Some("y"));
        assert!(matches!(shape(&constraint), Shape::Driven { .. }));
    }

    /// `sin(x) == y` says exactly what `y == sin(x)` says. Reading only the left
    /// would classify half of a symmetric relation.
    #[test]
    fn the_driven_side_may_be_either() {
        let constraint = parse("sin(x) == y +/- 0.001");
        assert_eq!(driven_name(&constraint), Some("y"));
    }

    /// Row F: on both sides *and* under something no solver will take.
    #[test]
    fn a_variable_inside_and_outside_a_transcendental_is_implicit() {
        assert!(matches!(
            shape(&parse("x == sin(x) +/- 0.001")),
            Shape::Implicit { because: "sin", .. }
        ));
        assert!(matches!(
            shape(&parse("x == ln(x) +/- 0.001")),
            Shape::Implicit { because: "ln", .. }
        ));
    }

    /// **On both sides is not enough**, and getting this wrong would refuse
    /// constraints the pool solves today.
    ///
    /// The first cut of row F fired on any variable appearing on both sides,
    /// which is also true of every row C case — `x2` here is one rearrangement
    /// from being driven — and of `x^2 == x + 2`, which Z3 answers without
    /// complaint. Both must stay `Opaque` and reachable.
    #[test]
    fn a_variable_on_both_sides_of_something_solvable_is_not_implicit() {
        for source in [
            "x2 == x1 + 1/2*x2 - x3 / x4 +/- 0.001",
            "x == x*x + 2 +/- 0.001",
            "y == y/2 + x1 +/- 0.001",
        ] {
            assert_eq!(
                shape(&parse(source)),
                Shape::Opaque,
                "{source} is solvable and must not be refused"
            );
        }
    }

    /// Appearing repeatedly on the *far* side is not a self-reference. A count of
    /// occurrences would get this wrong; membership is the right question.
    #[test]
    fn a_variable_appearing_twice_on_the_far_side_is_still_driven() {
        let constraint = parse("y == x*x + x +/- 0.001");
        assert_eq!(driven_name(&constraint), Some("y"));
    }

    /// Row A. Pinned is a special case of driven, and reporting the weaker of the
    /// two would lose the fact that the coordinate never has to be recomputed.
    #[test]
    fn a_constant_side_is_pinned_not_driven() {
        let constraint = parse("x1 == pi +/- 0.001");
        assert!(matches!(shape(&constraint), Shape::Pinned { .. }));
    }

    /// Row C, declined rather than guessed at. Gathering `x + y` into a
    /// definition of either is a rewrite this does not do.
    #[test]
    fn a_compound_side_is_not_driven() {
        assert_eq!(shape(&parse("x + y == sin(z) +/- 0.001")), Shape::Opaque);
    }

    /// The taxonomy is about equalities. A comparison has no side that defines
    /// the other, and reading `x > 4` as pinning `x` would be badly wrong.
    #[test]
    fn an_inequality_is_not_classified() {
        assert_eq!(shape(&parse("x > 4")), Shape::Opaque);
        assert_eq!(shape(&parse("x1 + x2 > 20 - x3^2")), Shape::Opaque);
    }

    /// A scalar expression is not a constraint and has no sides to read.
    #[test]
    fn a_scalar_expression_is_not_classified() {
        assert_eq!(shape(&parse("x + 1")), Shape::Opaque);
    }

    /// `var[i]` reads a variable the expression never names, so "does it appear
    /// on the other side" has no answer. `Ast::contains_dynamic_lookup` exists to
    /// warn about exactly this and it would be careless to ignore it here.
    #[test]
    fn a_dynamic_lookup_defeats_classification() {
        assert_eq!(
            shape(&parse("1.5 == var[1] + var[2] +/- 0.001")),
            Shape::Opaque
        );
    }

    // -----------------------------------------------------------------------
    // The system level
    // -----------------------------------------------------------------------

    fn plan_over(names: &[&str], sources: &[&str]) -> Option<Plan> {
        let schema = Schema::new(names.iter().copied());
        let constraints: Vec<Ast> = sources.iter().map(|s| parse(s)).collect();
        plan(&constraints, &schema)
    }

    #[test]
    fn two_independent_drives_are_both_taken() {
        let plan = plan_over(
            &["x1", "x2", "x3", "x4"],
            &["x1 == sqrt(x2) +/- 0.001", "x3 == cbrt(x4) +/- 0.001"],
        )
        .expect("both should drive");

        let driven: Vec<usize> = plan.driven().iter().map(|d| d.position).collect();
        assert_eq!(driven, vec![0, 2]);
        assert_eq!(plan.free(), &[1, 3]);
    }

    /// `z` is defined from `y`, which is itself defined. Computing `z` first
    /// reads whatever `y` happened to hold, so the order is not cosmetic.
    #[test]
    fn a_chain_of_drives_is_ordered() {
        let plan = plan_over(
            &["x", "y", "z"],
            &["y == sin(x) +/- 0.001", "z == y + 1 +/- 0.001"],
        )
        .expect("both should drive");

        let driven: Vec<usize> = plan.driven().iter().map(|d| d.position).collect();
        assert_eq!(driven, vec![1, 2], "y must be computed before z");
        assert_eq!(plan.free(), &[0]);
    }

    /// The same chain with the schema declared backwards, so the correct order
    /// is the *reverse* of the numeric one.
    ///
    /// Without this, `a_chain_of_drives_is_ordered` proves nothing: definitions
    /// are held in a `BTreeMap` and iterate by position, so `[1, 2]` comes out
    /// right whether or not any topological sort happened. Here `z` sits at 0 and
    /// `y` at 1, and taking them in position order would compute `z` from a stale
    /// `y`.
    #[test]
    fn evaluation_order_beats_declaration_order() {
        let plan = plan_over(
            &["z", "y", "x"],
            &["y == sin(x) +/- 0.001", "z == y + 1 +/- 0.001"],
        )
        .expect("both should drive");

        let driven: Vec<usize> = plan.driven().iter().map(|d| d.position).collect();
        assert_eq!(
            driven,
            vec![1, 0],
            "y (at 1) must be computed before z (at 0)"
        );
    }

    /// A cycle of length two, caught by the same graph that catches row F at
    /// length one. Neither can be computed first, so neither is driven — and
    /// both constraints stay in force.
    #[test]
    fn a_cycle_between_drives_is_refused() {
        assert!(
            plan_over(
                &["x", "y"],
                &["y == x + 1 +/- 0.001", "x == y + 1 +/- 0.001"]
            )
            .is_none(),
            "a cycle should drive nothing"
        );
    }

    /// Two definitions of one variable. Choosing between them is row E's matching
    /// problem; picking the first written would be arbitrary, so neither is
    /// taken. What must *not* happen is one constraint quietly ceasing to apply.
    #[test]
    fn a_variable_driven_twice_is_driven_once_or_not_at_all() {
        let plan = plan_over(
            &["x", "y", "z"],
            &["y == x + 1 +/- 0.001", "y == z * 2 +/- 0.001"],
        );
        assert!(
            plan.is_none(),
            "an ambiguous definition should not be resolved by source order"
        );
    }

    /// Nothing to drive is the common case and must not cost the caller a plan
    /// it then has to check.
    #[test]
    fn a_system_with_nothing_driven_has_no_plan() {
        assert!(plan_over(&["x1", "x2", "x3"], &["x1 + x2 > 20 - x3^2"]).is_none());
    }

    /// Row A across every dimension: both pinned, nothing left free. Legal, and
    /// the walker has to cope with an empty free list rather than divide by its
    /// length.
    #[test]
    fn a_fully_pinned_system_leaves_nothing_free() {
        let plan = plan_over(&["x1", "x2"], &["x1 == pi +/- 0.001", "x2 == e +/- 0.001"])
            .expect("both should pin");
        assert_eq!(plan.free(), &[] as &[usize]);
        assert_eq!(plan.driven().len(), 2);
    }
}
