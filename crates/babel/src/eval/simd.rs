//! The tile executor's kernels: explicit SIMD, and explicit not-SIMD.
//!
//! Every kernel here is one of two things, and says which in its name. A
//! *vector* kernel takes whole slices, splits them into lanes of `S::f64s`
//! with the remainder as a scalar tail, runs the vector body over the head and
//! the language's own `apply` over the tail, and reports whether any lane went
//! non-finite. A `*_scalar` kernel is a plain loop and is used for the
//! operators that have no vector form worth having — libm, `%`, `pow`,
//! rounding — so that a loop which is not vectorised is a choice with a name
//! rather than a compiler outcome.
//!
//! Two rules the golden-era tests hold this to, bit for bit:
//!
//! - **Never `mul_add`.** pulp's fused multiply-add is fused on every backend,
//!   scalar included; the tape rounds a product and a sum separately, and so
//!   do these kernels.
//! - **Never pulp's `max`/`min`.** They are the x86 instructions, which return
//!   the second operand when either is NaN. Babel's are Java's, NaN-propagating
//!   and canonical, so [`max_nan`] and [`min_nan`] are built from compares and
//!   selects.
//!
//! Everything is `#[inline(always)]`: the dispatched instruction set only
//! reaches code that is inlined into pulp's `with_simd` frame, and a kernel
//! that fell out of line would silently run at the baseline.

use faer::MatRef;
use pulp::Simd;

use crate::ast::{BinaryOp, CompareOp};

use super::EPSILON;

/// An all-false mask, since pulp has no `splat` for masks.
#[inline(always)]
fn no_lanes<S: Simd>(simd: S) -> S::m64s {
    let zero = simd.splat_f64s(0.0);
    simd.less_than_f64s(zero, zero)
}

/// Exactly `v.is_finite()` per lane: an ordered `|v| < inf` is false for NaN
/// and for both infinities.
#[inline(always)]
fn finite<S: Simd>(simd: S, v: S::f64s) -> S::m64s {
    simd.less_than_f64s(simd.abs_f64s(v), simd.splat_f64s(f64::INFINITY))
}

/// Whether any lane of `mask` is set.
///
/// Not `first_true_m64s`: on the scalar backend a mask is a `bool`, its lane
/// count works out to zero, and "first true != lanes" is then always true. A
/// select to `1.0`/`0.0` and a horizontal max is correct on every backend and
/// costs one reduction per instruction, not per iteration.
#[inline(always)]
fn any_lane<S: Simd>(simd: S, mask: S::m64s) -> bool {
    simd.reduce_max_f64s(simd.select_f64s(mask, simd.splat_f64s(1.0), simd.splat_f64s(0.0))) > 0.0
}

/// Java's `Math.max`, as `BinaryOp::Max.apply` defines it: NaN if either side
/// is, otherwise the larger, and on equal operands the one whose sign is
/// positive. Equal operands are bitwise identical unless they are the two
/// zeros, so *and*-ing their bits returns the shared value in the first case
/// and `0.0` (all sign bits cleared) in the second — one instruction where a
/// sign test and a third select would be three.
///
/// `simd_tests` holds this to `apply` on every special pair, on both backends.
#[inline(always)]
pub(super) fn max_nan<S: Simd>(simd: S, a: S::f64s, b: S::f64s) -> S::f64s {
    let nan = simd.or_m64s(
        simd.not_m64s(simd.equal_f64s(a, a)),
        simd.not_m64s(simd.equal_f64s(b, b)),
    );
    let larger = simd.select_f64s(
        simd.greater_than_f64s(a, b),
        a,
        simd.select_f64s(simd.greater_than_f64s(b, a), b, simd.and_f64s(a, b)),
    );
    simd.select_f64s(nan, simd.splat_f64s(f64::NAN), larger)
}

/// Java's `Math.min`: the mirror of [`max_nan`], with *or* in place of *and*
/// so that the two zeros answer `-0.0`.
#[inline(always)]
pub(super) fn min_nan<S: Simd>(simd: S, a: S::f64s, b: S::f64s) -> S::f64s {
    let nan = simd.or_m64s(
        simd.not_m64s(simd.equal_f64s(a, a)),
        simd.not_m64s(simd.equal_f64s(b, b)),
    );
    let smaller = simd.select_f64s(
        simd.less_than_f64s(a, b),
        a,
        simd.select_f64s(simd.less_than_f64s(b, a), b, simd.or_f64s(a, b)),
    );
    simd.select_f64s(nan, simd.splat_f64s(f64::NAN), smaller)
}

/// The residual of `a op b` under the `<= 0` convention, the same four
/// expressions as `lane::compare` in the same order, so the nudge lands in the
/// same place.
#[inline(always)]
pub(super) fn compare<S: Simd>(simd: S, op: CompareOp, a: S::f64s, b: S::f64s) -> S::f64s {
    match op {
        CompareOp::Lte => simd.sub_f64s(a, b),
        CompareOp::Gte => simd.sub_f64s(b, a),
        CompareOp::Lt => simd.add_f64s(simd.sub_f64s(a, b), simd.splat_f64s(EPSILON)),
        CompareOp::Gt => simd.add_f64s(simd.sub_f64s(b, a), simd.splat_f64s(EPSILON)),
    }
}

/// `|a - b| <= t` as the larger of the two one-sided residuals: `lane::near_eq`
/// verbatim, over [`max_nan`].
#[inline(always)]
pub(super) fn near_eq<S: Simd>(simd: S, a: S::f64s, b: S::f64s, tolerance: S::f64s) -> S::f64s {
    let at_least = simd.sub_f64s(simd.sub_f64s(b, tolerance), a);
    let at_most = simd.sub_f64s(a, simd.add_f64s(b, tolerance));
    max_nan(simd, at_least, at_most)
}

// ------------------------------------------------------------ vector kernels

/// `dst[i] = op(a[i])`. Returns whether any lane came out non-finite.
#[inline(always)]
pub(super) fn unary<S: Simd>(
    simd: S,
    dst: &mut [f64],
    a: &[f64],
    vector: impl Fn(S, S::f64s) -> S::f64s,
    scalar: impl Fn(f64) -> f64,
) -> bool {
    let (d, d_tail) = S::as_mut_simd_f64s(dst);
    let (x, x_tail) = S::as_simd_f64s(a);
    let mut bad = no_lanes(simd);
    for (d, &x) in d.iter_mut().zip(x) {
        let v = vector(simd, x);
        *d = v;
        bad = simd.or_m64s(bad, simd.not_m64s(finite(simd, v)));
    }
    let mut bad_tail = false;
    for (d, &x) in d_tail.iter_mut().zip(x_tail) {
        let v = scalar(x);
        *d = v;
        bad_tail |= !v.is_finite();
    }
    any_lane(simd, bad) || bad_tail
}

/// `dst[i] = op(a[i], b[i])`. `dst` aliases neither operand; `a == b` is fine.
#[inline(always)]
pub(super) fn binary<S: Simd>(
    simd: S,
    dst: &mut [f64],
    a: &[f64],
    b: &[f64],
    vector: impl Fn(S, S::f64s, S::f64s) -> S::f64s,
    scalar: impl Fn(f64, f64) -> f64,
) -> bool {
    let (d, d_tail) = S::as_mut_simd_f64s(dst);
    let (x, x_tail) = S::as_simd_f64s(a);
    let (y, y_tail) = S::as_simd_f64s(b);
    let mut bad = no_lanes(simd);
    for ((d, &x), &y) in d.iter_mut().zip(x).zip(y) {
        let v = vector(simd, x, y);
        *d = v;
        bad = simd.or_m64s(bad, simd.not_m64s(finite(simd, v)));
    }
    let mut bad_tail = false;
    for ((d, &x), &y) in d_tail.iter_mut().zip(x_tail).zip(y_tail) {
        let v = scalar(x, y);
        *d = v;
        bad_tail |= !v.is_finite();
    }
    any_lane(simd, bad) || bad_tail
}

/// `dst[i] = op(dst[i], b[i])`: a fold step accumulating in place.
#[inline(always)]
pub(super) fn in_place<S: Simd>(
    simd: S,
    dst: &mut [f64],
    b: &[f64],
    vector: impl Fn(S, S::f64s, S::f64s) -> S::f64s,
    scalar: impl Fn(f64, f64) -> f64,
) -> bool {
    let (d, d_tail) = S::as_mut_simd_f64s(dst);
    let (y, y_tail) = S::as_simd_f64s(b);
    let mut bad = no_lanes(simd);
    for (d, &y) in d.iter_mut().zip(y) {
        let v = vector(simd, *d, y);
        *d = v;
        bad = simd.or_m64s(bad, simd.not_m64s(finite(simd, v)));
    }
    let mut bad_tail = false;
    for (d, &y) in d_tail.iter_mut().zip(y_tail) {
        let v = scalar(*d, y);
        *d = v;
        bad_tail |= !v.is_finite();
    }
    any_lane(simd, bad) || bad_tail
}

/// Whether any value in `values` is non-finite: `Insn::Check`.
#[inline(always)]
pub(super) fn any_non_finite<S: Simd>(simd: S, values: &[f64]) -> bool {
    let (head, tail) = S::as_simd_f64s(values);
    let mut bad = no_lanes(simd);
    for &v in head {
        bad = simd.or_m64s(bad, simd.not_m64s(finite(simd, v)));
    }
    any_lane(simd, bad) || tail.iter().any(|v| !v.is_finite())
}

// ------------------------------------------------------------ scalar kernels
//
// Plain loops, named so. Used for the operators with no vector form: libm's
// transcendentals, `%` (`fmod`), `pow`, `log` to a base, `floor`/`ceil` (pulp's
// generic trait has no rounding), `sgn`, and the strided load.

pub(super) fn unary_scalar(dst: &mut [f64], a: &[f64], f: impl Fn(f64) -> f64) -> bool {
    let mut bad = false;
    for (d, &x) in dst.iter_mut().zip(a) {
        let v = f(x);
        *d = v;
        bad |= !v.is_finite();
    }
    bad
}

pub(super) fn binary_scalar(
    dst: &mut [f64],
    a: &[f64],
    b: &[f64],
    f: impl Fn(f64, f64) -> f64,
) -> bool {
    let mut bad = false;
    for ((d, &x), &y) in dst.iter_mut().zip(a).zip(b) {
        let v = f(x, y);
        *d = v;
        bad |= !v.is_finite();
    }
    bad
}

/// `Insn::Load`: one column of the sample matrix is a lane, so an input row is
/// a strided read. Scalar, and staying so: every input is loaded once per
/// tape, so a transpose into contiguous scratch would be the same traffic plus
/// a copy.
pub(super) fn load_strided(
    dst: &mut [f64],
    samples: MatRef<'_, f64>,
    input: usize,
    first_column: usize,
) -> bool {
    let mut bad = false;
    for (lane, d) in dst.iter_mut().enumerate() {
        let x = samples[(input, first_column + lane)];
        *d = x;
        bad |= !x.is_finite();
    }
    bad
}

/// The scalar twin of [`max_nan`], which is the language's definition.
pub(super) fn max_scalar(a: f64, b: f64) -> f64 {
    BinaryOp::Max.apply(a, b)
}

/// The scalar twin of [`min_nan`].
pub(super) fn min_scalar(a: f64, b: f64) -> f64 {
    BinaryOp::Min.apply(a, b)
}

#[cfg(test)]
#[path = "simd_tests.rs"]
mod tests;
