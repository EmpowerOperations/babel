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
    /// `x == sin(x)`, `x2 == x1 + x2/2 - x3/x4`, `x == x*x + 2`. A variable
    /// defined in terms of itself, so the equality is *implicit* in it: there is
    /// no rearrangement-free way to write `v = ...`. Refused by
    /// [`SystemError::Implicit`](crate::cvg::SystemError::Implicit).
    ///
    /// "Implicit" rather than "cyclic": a cycle is a mutual dependency *between*
    /// equations, which [`plan`] handles by driving neither. One equation that
    /// cannot be solved for the variable it names is the textbook implicit
    /// form.
    ///
    /// **What is refused is a phrasing, not a problem.** `x2 == x1 + x2/2` and
    /// `x2/2 - x1 == 0` describe the same set, and only the first asks an
    /// evaluation order to resolve `x2` from `x2`. Every constraint this rejects
    /// can be written with the variable on one side, which is what the
    /// diagnostic says.
    ///
    /// Drawing the line here rather than at "and beyond every solver" is
    /// deliberate. The narrower rule let `x2 == x1 + x2/2 - x3/x4` through
    /// because Z3 can answer it, which is true and beside the point: nothing
    /// downstream can *drive* it, so it falls to whatever the sampler manages
    /// and reads as a capability we do not have. One rule stated once beats a
    /// rule that depends on what the emitter happens to support this month.
    ///
    Implicit { variable: GlobalId },
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

    // Any variable named on both sides makes the equality implicit in it, and
    // this runs first because it does not care how either side is *shaped*.
    // Asking only about a side that is a bare variable would make the rule
    // depend on spelling: `x == x*x + 2` refused and `x*x == x + 2` allowed,
    // which are the same set.
    if let Some(&variable) = globals(lhs).intersection(&globals(rhs)).next() {
        return Shape::Implicit { variable };
    }

    // Both orders, since `y == sin(x)` and `sin(x) == y` say the same thing.
    for (candidate, other) in [(lhs, rhs), (rhs, lhs)] {
        let Kind::Global(variable) = candidate.kind else {
            continue;
        };
        debug_assert!(
            !mentions(other, variable),
            "a shared variable is settled above"
        );

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

    // No side is a bare variable, so try to make one: peel the operators around
    // a variable that occurs exactly once and move them to the other side.
    // Schema order rather than discovery order, so the choice does not wander.
    let mut candidates: Vec<GlobalId> = globals(&body.result).into_iter().collect();
    candidates.sort_unstable();
    for variable in candidates {
        if occurrences(&body.result, variable) != 1 {
            continue;
        }
        let (side, other) = if mentions(lhs, variable) {
            (lhs, rhs)
        } else {
            (rhs, lhs)
        };
        if let Some(definition) = isolate(side, other.as_ref().clone(), variable) {
            let from = detach(constraint, &definition);
            return Shape::Driven {
                variable,
                from,
                tolerance: tolerance.abs(),
            };
        }
    }

    Shape::Opaque
}

/// The definition of `variable`, peeled out of `side == other`.
///
/// `x1 + x2 == 3` gives `3 - x2`: descend into the branch holding the variable
/// and move what is left of the node across, one step at a time, until the
/// descent lands on the variable itself.
///
/// # Only where the variable occurs once
///
/// The caller checks that. In term rewriting a term where a variable appears at
/// most once is *linear* in it, and that is exactly the class where isolating is
/// a walk down a path rather than an algebra problem. Two occurrences would need
/// like terms gathered — normalisation, and the start of a computer algebra
/// system — which is what `Shape::Implicit` refuses instead.
///
/// # Only arithmetic
///
/// `Pow`, `Rem`, `Max`, `Min`, `LogB` and every unary function return `None`. An
/// inverse for those is a second step: it needs a *symbolic* inverse table,
/// where [`crate::frontend::rewrite`]'s `monotone` holds numeric ones for
/// comparisons against a literal, and it runs into branches — `asin` gives a
/// principal value, so isolating through `sin` silently picks one solution out
/// of infinitely many.
///
/// # Why no divisor guard
///
/// `a * b` isolated through `a` gives `other / b`, which says nothing useful
/// where `b` is zero. That would matter for a rewrite; this is a **proposal**.
/// `Problem::retract` skips a coordinate whose definition does not evaluate, and
/// a non-finite result is an `Err`, so a division by zero leaves the coordinate
/// untouched and the candidate is judged like any other. A wrong isolation costs
/// rejected moves, never a wrong point.
fn isolate(side: &Expr, other: Expr, variable: GlobalId) -> Option<Expr> {
    if matches!(side.kind, Kind::Global(found) if found == variable) {
        return Some(other);
    }

    // Synthesised nodes take the span of what they replace, following
    // `rewrite::invert_comparison`, so a diagnostic still points at real source.
    let span = side.span;
    let build = |op, lhs: Expr, rhs: Expr| {
        Expr::new(
            Kind::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
            span,
        )
    };

    match &side.kind {
        Kind::Unary {
            op: UnaryOp::Negate,
            arg,
        } => isolate(
            arg,
            Expr::new(
                Kind::Unary {
                    op: UnaryOp::Negate,
                    arg: Box::new(other),
                },
                span,
            ),
            variable,
        ),

        Kind::Binary { op, lhs, rhs } => {
            let left = mentions(lhs, variable);
            let (inner, moved) = if left { (lhs, rhs) } else { (rhs, lhs) };
            let moved = moved.as_ref().clone();

            let carried = match (op, left) {
                // `u + b == c` and `a + u == c` are both `u == c - (the other)`.
                (BinaryOp::Add, _) => build(BinaryOp::Sub, other, moved),
                (BinaryOp::Sub, true) => build(BinaryOp::Add, other, moved),
                // `a - u == c` is `u == a - c`, and the operand order is the
                // whole content of this arm.
                (BinaryOp::Sub, false) => build(BinaryOp::Sub, moved, other),
                (BinaryOp::Mul, _) => build(BinaryOp::Div, other, moved),
                (BinaryOp::Div, true) => build(BinaryOp::Mul, other, moved),
                // `a / u == c` is `u == a / c`, order again.
                (BinaryOp::Div, false) => build(BinaryOp::Div, moved, other),
                _ => return None,
            };
            isolate(inner, carried, variable)
        }

        _ => None,
    }
}

/// How many times `variable` is read in `expr`.
///
/// [`globals`] answers *whether*, which is not enough: peeling needs the
/// variable to occur exactly once, or the path walk leaves it on both sides.
fn occurrences(expr: &Expr, variable: GlobalId) -> usize {
    match &expr.kind {
        Kind::Global(id) => usize::from(*id == variable),
        Kind::Literal(_) | Kind::Local(_) => 0,
        Kind::Unary { arg, .. } => occurrences(arg, variable),
        Kind::Binary { lhs, rhs, .. }
        | Kind::Compare { lhs, rhs, .. }
        | Kind::NearEq { lhs, rhs, .. } => occurrences(lhs, variable) + occurrences(rhs, variable),
        Kind::And { terms } | Kind::Fold { terms, .. } => {
            terms.iter().map(|term| occurrences(term, variable)).sum()
        }
        Kind::DynamicIndex(index) => occurrences(index, variable),
        Kind::Block(block) => {
            block
                .assignments
                .iter()
                .map(|assignment| occurrences(&assignment.value, variable))
                .sum::<usize>()
                + occurrences(&block.result, variable)
        }
        Kind::Aggregate {
            lower, upper, body, ..
        } => {
            occurrences(lower, variable)
                + occurrences(upper, variable)
                + body
                    .assignments
                    .iter()
                    .map(|assignment| occurrences(&assignment.value, variable))
                    .sum::<usize>()
                + occurrences(&body.result, variable)
        }
    }
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
        // Carried rather than assumed. A definition can hold a subscript
        // nothing resolved, and `Ast` documents why claiming otherwise is
        // dangerous: a caller must not prune columns it believes
        // unreferenced while this is true.
        contains_dynamic_lookup: parent.contains_dynamic_lookup,
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

    /// A variable on both sides is implicit in it, whatever it is wrapped in.
    ///
    /// The rule was once narrower — on both sides *and* inside something the
    /// emitter refuses — which let `x2 == x1 + x2/2 - x3/x4` through on the
    /// grounds that Z3 can answer it. True, and beside the point: nothing
    /// downstream can drive such a variable, so it fell to whatever the sampler
    /// managed and read as a capability we did not have.
    #[test]
    fn a_variable_on_both_sides_is_implicit() {
        for source in [
            "x == sin(x) +/- 0.001",
            "x == ln(x) +/- 0.001",
            "x2 == x1 + 1/2*x2 - x3 / x4 +/- 0.001",
            "y == y/2 + x1 +/- 0.001",
            // Neither side is a bare variable, and it makes no difference: the
            // rule is about the variable being on both sides, not about how
            // either side is written. `x == x*x + 2` and `x*x == x + 2` are the
            // same equation and must get the same answer.
            "sin(x) == x/2 +/- 0.001",
            "x*x == x + 2 +/- 0.001",
            "x == x*x + 2 +/- 0.001",
        ] {
            assert!(
                matches!(shape(&parse(source)), Shape::Implicit { .. }),
                "{source} defines a variable in terms of itself"
            );
        }
    }

    /// The rearrangement every implicit form has, and which the diagnostic
    /// names. Refusing these too would be refusing the *problem* rather than a
    /// phrasing.
    ///
    /// The linear one now does better than merely being accepted: `x1` occurs
    /// once, so it is isolated and driven. That is the rearranged
    /// `cvg_pools::simple_arithmetic` fixture, and it means asking a user to
    /// write the non-circular form buys them a driven variable rather than
    /// only avoiding a refusal.
    #[test]
    fn the_rearranged_form_is_explicit() {
        assert!(matches!(
            shape(&parse("x2/2 - x1 + x3 / x4 == 0 +/- 0.001")),
            Shape::Driven { .. }
        ));
        // Three occurrences of `x`, so nothing to peel — accepted, not driven.
        assert_eq!(shape(&parse("x*x - x + 2 == 0 +/- 0.001")), Shape::Opaque);
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

    /// The driven variable's name and the definition peeled out for it.
    fn driven(source: &str) -> (String, Ast) {
        let constraint = parse(source);
        match shape(&constraint) {
            Shape::Driven { variable, from, .. }
            | Shape::Pinned {
                variable,
                value: from,
                ..
            } => (constraint.symbols[variable.index()].clone(), from),
            other => panic!("{source} should drive, got {other:?}"),
        }
    }

    /// The definition peeled out of `source`, evaluated at `at`.
    ///
    /// The driven variable still needs *a* binding so the definition compiles
    /// against the same schema; its value is what the definition computes, so
    /// what it is bound to does not matter.
    fn definition_at(source: &str, at: &[(&str, f64)]) -> f64 {
        crate::eval_one(&driven(source).1, at)
            .unwrap_or_else(|e| panic!("{source}: the definition failed to evaluate: {e}"))
    }

    /// Drives the variable, substitutes what it computed, and requires the
    /// constraint to hold.
    ///
    /// **The constraint is its own oracle**, which is what makes this stronger
    /// than the per-rule assertions: a babel constraint evaluates to a residual
    /// that is `<= 0` exactly when it holds, so nothing has to predict what the
    /// definition should produce. A sign error in any arm shows up without
    /// anyone having worked out the answer first.
    ///
    /// `free` binds everything *except* the driven variable, which this
    /// computes.
    fn assert_substitution_satisfies(source: &str, free: &[(&str, f64)]) {
        let constraint = parse(source);
        let (name, definition) = driven(source);

        let mut bindings: Vec<(&str, f64)> = free.to_vec();
        bindings.push((name.as_str(), 0.0));
        let computed = crate::eval_one(&definition, &bindings)
            .unwrap_or_else(|e| panic!("{source}: the definition of {name} failed: {e}"));

        let mut substituted = free.to_vec();
        substituted.push((name.as_str(), computed));
        let residual = crate::eval_one(&constraint, &substituted)
            .unwrap_or_else(|e| panic!("{source}: evaluating at {substituted:?}: {e}"));

        assert!(
            residual <= 0.0,
            "{source}: driving {name} to {computed} leaves a residual of \
{residual}, so the definition does not satisfy the constraint it came from"
        );
    }

    // One test per rule `isolate` has, in two forms: what the definition
    // *evaluates to*, pegged against Rust rather than a number worked out by
    // hand, and whether substituting it back *satisfies the constraint*.
    //
    // `3.0 - 4.0` says what the rearrangement is, where `-1.0` says only that
    // the author and the code agree — which is the thing a test is meant to
    // establish rather than assume.
    //
    // Exact equality, following `corpus.rs`, which takes a tolerance only where
    // one is earned. This arithmetic is exact in `f64` and `sin` goes through
    // the same libm on both sides, so a difference would be real and not noise.
    //
    // Four of these rules had no test at all before: `*`, both branches of `/`,
    // and unary minus.

    #[test]
    fn addition_undoes_to_subtraction() {
        // `u + b == c` is `u == c - b`.
        assert_eq!(
            definition_at("x1 + x2 == 3 +/- 0.001", &[("x1", 0.0), ("x2", 4.0)]),
            3.0 - 4.0
        );
        assert_substitution_satisfies("x1 + x2 == 3 +/- 0.001", &[("x2", 4.0)]);
    }

    #[test]
    fn subtraction_on_the_left_undoes_to_addition() {
        // `u - b == c` is `u == c + b`.
        assert_eq!(
            definition_at("x1 - x2 == 1 +/- 0.001", &[("x1", 0.0), ("x2", 4.0)]),
            1.0 + 4.0
        );
        assert_substitution_satisfies("x1 - x2 == 1 +/- 0.001", &[("x2", 4.0)]);
    }

    /// One of two arms where the operands do not commute, so a swap is silently
    /// wrong rather than a compile error.
    #[test]
    fn subtraction_on_the_right_keeps_its_operand_order() {
        // `a - u == c` is `u == a - c`, and not `c - a`.
        assert_eq!(
            definition_at("3 - x1 == 1 +/- 0.001", &[("x1", 0.0)]),
            3.0 - 1.0
        );
        assert_substitution_satisfies("3 - x1 == 1 +/- 0.001", &[]);
    }

    #[test]
    fn multiplication_undoes_to_division() {
        // `u * b == c` is `u == c / b`.
        assert_eq!(
            definition_at("x1 * x2 == 12 +/- 0.001", &[("x1", 0.0), ("x2", 4.0)]),
            12.0 / 4.0
        );
        assert_substitution_satisfies("x1 * x2 == 12 +/- 0.001", &[("x2", 4.0)]);
    }

    #[test]
    fn division_on_the_left_undoes_to_multiplication() {
        // `u / b == c` is `u == c * b`.
        assert_eq!(
            definition_at("x1 / x2 == 4 +/- 0.001", &[("x1", 0.0), ("x2", 3.0)]),
            4.0 * 3.0
        );
        assert_substitution_satisfies("x1 / x2 == 4 +/- 0.001", &[("x2", 3.0)]);
    }

    /// The other non-commuting arm, and the one that had no test at all.
    #[test]
    fn division_on_the_right_keeps_its_operand_order() {
        // `a / u == c` is `u == a / c`.
        assert_eq!(
            definition_at("12 / x1 == 4 +/- 0.001", &[("x1", 0.0)]),
            12.0 / 4.0
        );
        assert_substitution_satisfies("12 / x1 == 4 +/- 0.001", &[]);
    }

    /// `0 - u` is a subtraction, not a negation, so this reaches the binary arm
    /// and [`negation_undoes_to_itself`] covers the unary one. Two tests because
    /// the first draft of these had only this one and believed it covered both.
    #[test]
    fn a_subtraction_from_zero_is_not_a_negation() {
        assert_eq!(
            definition_at("0 - x1 == 5 +/- 0.001", &[("x1", 0.0)]),
            0.0 - 5.0
        );
        assert_substitution_satisfies("0 - x1 == 5 +/- 0.001", &[]);
    }

    #[test]
    fn negation_undoes_to_itself() {
        // `-u == c` is `u == -c`.
        assert_eq!(definition_at("-x1 == 5 +/- 0.001", &[("x1", 0.0)]), -5.0);
        assert_substitution_satisfies("-x1 == 5 +/- 0.001", &[]);
    }

    /// Peeling past a term babel and Rust have to agree on, which makes the
    /// evaluator part of what this pins rather than only the rearrangement.
    #[test]
    fn peeling_past_a_transcendental_agrees_with_rust() {
        assert_eq!(
            definition_at("sin(x) + y == 3 +/- 0.001", &[("x", 1.0), ("y", 0.0)]),
            3.0 - 1.0_f64.sin()
        );
        assert_substitution_satisfies("sin(x) + y == 3 +/- 0.001", &[("x", 1.0)]);
    }

    /// Several rules at once, and the case that catches an assumption about
    /// *which* variable gets driven: this drives `x2` — first by `GlobalId` and
    /// occurring once — so `x1` is free here and `x2` is not.
    #[test]
    fn a_chain_of_rules_composes() {
        assert_substitution_satisfies(
            "x2/2 - x1 + x3 / x4 == 0 +/- 0.001",
            &[("x1", 1.0), ("x3", 8.0), ("x4", 2.0)],
        );
    }

    /// A compound side still drives, as long as one variable can be peeled out
    /// of it.
    ///
    /// `x1 + x2 == 3` is *easier* than `y == sin(x)`, which drives today: one
    /// subtraction isolates it. It failed only because the test was for a bare
    /// `Kind::Global` on one side, which is a statement about spelling.
    #[test]
    fn a_compound_side_isolates_its_lone_variable() {
        let constraint = parse("x1 + x2 == 3 +/- 0.001");
        let Shape::Driven { variable, .. } = shape(&constraint) else {
            panic!("x1 + x2 == 3 should drive one of its variables");
        };
        // Deterministic, and `addition_undoes_to_subtraction` is what pins the
        // definition it produces.
        assert_eq!(constraint.symbols[variable.index()], "x1");
    }

    /// Peeling needs the variable to occur *once* — "linear in it", in the term
    /// rewriting sense. Twice on one side and the path walk would leave it on
    /// both, which is a wrong answer rather than a missing one.
    #[test]
    fn a_variable_occurring_twice_is_not_isolated() {
        assert_eq!(shape(&parse("x1 + x1 == 3 +/- 0.001")), Shape::Opaque);
        assert_eq!(shape(&parse("x1 * x1 == 3 +/- 0.001")), Shape::Opaque);
    }

    /// One variable being stuck does not stop another from being peeled out.
    ///
    /// `x1 + x2 + x1 == 3` cannot be solved for `x1` without gathering, and
    /// needs nothing at all to be solved for `x2` — `3 - x1 - x1`. The rule is
    /// per variable, not per constraint, which this pins because the first
    /// version of the test assumed otherwise and was wrong.
    #[test]
    fn a_stuck_variable_does_not_block_a_free_one() {
        let constraint = parse("x1 + x2 + x1 == 3 +/- 0.001");
        let Shape::Driven { variable, from, .. } = shape(&constraint) else {
            panic!("x2 occurs once and should drive");
        };
        assert_eq!(constraint.symbols[variable.index()], "x2");
        assert_eq!(
            crate::eval_one(&from, &[("x1", 1.0), ("x2", 0.0)]).expect("evaluates"),
            1.0
        );
    }

    /// Only the arithmetic operators have an inverse here. Everything else
    /// declines rather than guesses — `^` and the unary functions are a second
    /// step with a branch problem of their own.
    #[test]
    fn an_operator_without_an_inverse_declines() {
        for source in [
            "x1 ^ 2 == 3 +/- 0.001",
            "max(x1, x2) == 3 +/- 0.001",
            "x1 % 3 == 1 +/- 0.001",
            // `sqrt` blocks `x1`, and `x1` is the only variable here. With a
            // second variable outside the `sqrt` this would drive *that* one —
            // see `a_stuck_variable_does_not_block_a_free_one`.
            "sqrt(x1) == 3 +/- 0.001",
            "sin(x1) + cos(x1) == 1 +/- 0.001",
        ] {
            assert_eq!(
                shape(&parse(source)),
                Shape::Opaque,
                "{source} has no arithmetic inverse and must decline"
            );
        }
    }

    /// Both variables of `x1 + x2 == 3` can be isolated and either is correct,
    /// so the choice must at least not wander between runs. Choosing *well* is
    /// row E's matching problem and is not this.
    #[test]
    fn isolation_is_deterministic() {
        let first = shape(&parse("x1 + x2 == 3 +/- 0.001"));
        for _ in 0..8 {
            assert_eq!(shape(&parse("x1 + x2 == 3 +/- 0.001")), first);
        }
    }

    /// A bare side is still read by the existing path, not peeled to.
    /// `y == x1 + x2` drives `y` — the whole right side is its definition —
    /// where peeling would have isolated `x1` and left `y` to be searched.
    #[test]
    fn a_bare_side_still_wins() {
        let constraint = parse("y == x1 + x2 +/- 0.001");
        let Shape::Driven { variable, .. } = shape(&constraint) else {
            panic!("y == x1 + x2 should drive y");
        };
        assert_eq!(constraint.symbols[variable.index()], "y");
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
