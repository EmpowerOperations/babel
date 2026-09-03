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
mod tests {
    //! The kernels against the language's own `apply`, bit for bit, on the best
    //! backend this machine has and on the scalar one.

    use pulp::{Arch, Simd, WithSimd};

    use super::super::EPSILON;
    use super::super::lane;
    use super::{any_non_finite, binary, compare, max_nan, min_nan, near_eq, unary};
    use crate::ast::{BinaryOp, CompareOp, UnaryOp};

    /// Values worth crossing with each other.
    const SPECIAL: [f64; 10] = [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -0.0,
        0.0,
        1.0,
        -1.0,
        1e308,
        f64::MIN_POSITIVE,
        5e-324,
    ];

    /// Eleven lanes: on AVX2 two full vectors and a three-lane tail, on the scalar
    /// backend eleven tails. Both halves of every kernel get exercised.
    const LANES: usize = 11;

    /// Runs a kernel on a backend and returns what it wrote plus its verdict.
    struct Binary<'a> {
        op: BinaryOp,
        a: &'a [f64],
        b: &'a [f64],
    }

    impl WithSimd for Binary<'_> {
        type Output = (Vec<f64>, bool);

        #[inline(always)]
        fn with_simd<S: Simd>(self, simd: S) -> Self::Output {
            let mut dst = vec![0.0; self.a.len()];
            let bad = match self.op {
                BinaryOp::Add => binary(
                    simd,
                    &mut dst,
                    self.a,
                    self.b,
                    |s, x, y| s.add_f64s(x, y),
                    |x, y| x + y,
                ),
                BinaryOp::Sub => binary(
                    simd,
                    &mut dst,
                    self.a,
                    self.b,
                    |s, x, y| s.sub_f64s(x, y),
                    |x, y| x - y,
                ),
                BinaryOp::Mul => binary(
                    simd,
                    &mut dst,
                    self.a,
                    self.b,
                    |s, x, y| s.mul_f64s(x, y),
                    |x, y| x * y,
                ),
                BinaryOp::Div => binary(
                    simd,
                    &mut dst,
                    self.a,
                    self.b,
                    |s, x, y| s.div_f64s(x, y),
                    |x, y| x / y,
                ),
                BinaryOp::Max => binary(simd, &mut dst, self.a, self.b, max_nan, |x, y| {
                    BinaryOp::Max.apply(x, y)
                }),
                BinaryOp::Min => binary(simd, &mut dst, self.a, self.b, min_nan, |x, y| {
                    BinaryOp::Min.apply(x, y)
                }),
                other => panic!("{other:?} has no vector kernel"),
            };
            (dst, bad)
        }
    }

    struct Unary<'a> {
        op: UnaryOp,
        a: &'a [f64],
    }

    impl WithSimd for Unary<'_> {
        type Output = (Vec<f64>, bool);

        #[inline(always)]
        fn with_simd<S: Simd>(self, simd: S) -> Self::Output {
            let mut dst = vec![0.0; self.a.len()];
            let bad = match self.op {
                UnaryOp::Negate => unary(simd, &mut dst, self.a, |s, x| s.neg_f64s(x), |x| -x),
                UnaryOp::Abs => unary(simd, &mut dst, self.a, |s, x| s.abs_f64s(x), |x| x.abs()),
                UnaryOp::Sqrt => unary(simd, &mut dst, self.a, |s, x| s.sqrt_f64s(x), |x| x.sqrt()),
                UnaryOp::Sqr => unary(simd, &mut dst, self.a, |s, x| s.mul_f64s(x, x), |x| x * x),
                UnaryOp::Cube => unary(
                    simd,
                    &mut dst,
                    self.a,
                    |s, x| s.mul_f64s(s.mul_f64s(x, x), x),
                    |x| x * x * x,
                ),
                other => panic!("{other:?} has no vector kernel"),
            };
            (dst, bad)
        }
    }

    struct Compare<'a> {
        op: CompareOp,
        a: &'a [f64],
        b: &'a [f64],
    }

    impl WithSimd for Compare<'_> {
        type Output = Vec<f64>;

        #[inline(always)]
        fn with_simd<S: Simd>(self, simd: S) -> Self::Output {
            let mut dst = vec![0.0; self.a.len()];
            let op = self.op;
            binary(
                simd,
                &mut dst,
                self.a,
                self.b,
                |s, x, y| compare(s, op, x, y),
                |x, y| lane::compare(op, x, y),
            );
            dst
        }
    }

    struct NearEq<'a> {
        a: &'a [f64],
        b: &'a [f64],
        t: f64,
    }

    impl WithSimd for NearEq<'_> {
        type Output = Vec<f64>;

        #[inline(always)]
        fn with_simd<S: Simd>(self, simd: S) -> Self::Output {
            let mut dst = vec![0.0; self.a.len()];
            let t = self.t;
            binary(
                simd,
                &mut dst,
                self.a,
                self.b,
                |s, x, y| near_eq(s, x, y, s.splat_f64s(t)),
                |x, y| lane::near_eq(x, y, t),
            );
            dst
        }
    }

    struct AnyNonFinite<'a>(&'a [f64]);

    impl WithSimd for AnyNonFinite<'_> {
        type Output = bool;

        #[inline(always)]
        fn with_simd<S: Simd>(self, simd: S) -> Self::Output {
            any_non_finite(simd, self.0)
        }
    }

    fn backends() -> Vec<(&'static str, Arch)> {
        vec![("detected", Arch::new()), ("scalar", Arch::Scalar)]
    }

    /// Every special pair, in lane slices that exercise both head and tail.
    fn special_pairs() -> (Vec<f64>, Vec<f64>) {
        let mut a = Vec::new();
        let mut b = Vec::new();
        for &x in &SPECIAL {
            for &y in &SPECIAL {
                a.push(x);
                b.push(y);
            }
        }
        (a, b)
    }

    fn assert_bits_equal(context: &str, got: &[f64], want: &[f64]) {
        for (i, (g, w)) in got.iter().zip(want).enumerate() {
            assert_eq!(
                g.to_bits(),
                w.to_bits(),
                "{context}, lane {i}: got {g:?}, want {w:?}"
            );
        }
    }

    /// The test that decides the signed-zero line in `max_nan`: whatever the
    /// scalar `f64::max` does on this toolchain, the kernel must do the same.
    #[test]
    fn max_and_min_kernels_match_apply_on_every_special_pair() {
        let (a, b) = special_pairs();
        for (name, arch) in backends() {
            for op in [BinaryOp::Max, BinaryOp::Min] {
                for chunk in 0..a.len().div_ceil(LANES) {
                    let range = chunk * LANES..((chunk + 1) * LANES).min(a.len());
                    let (got, _) = arch.dispatch(Binary {
                        op,
                        a: &a[range.clone()],
                        b: &b[range.clone()],
                    });
                    let want: Vec<f64> = range.clone().map(|i| op.apply(a[i], b[i])).collect();
                    assert_bits_equal(&format!("{op:?} on {name}"), &got, &want);
                }
            }
        }
    }

    #[test]
    fn every_vector_kernel_matches_apply_bit_for_bit() {
        let (a, b) = special_pairs();
        let mut rng = SplitMix64(0x5EED_0000_0000_0002);
        let mut ra: Vec<f64> = (0..1_000).map(|_| rng.uniform(-50.0, 50.0)).collect();
        let mut rb: Vec<f64> = (0..1_000).map(|_| rng.uniform(-50.0, 50.0)).collect();
        ra.extend_from_slice(&a);
        rb.extend_from_slice(&b);

        for (name, arch) in backends() {
            for op in [
                BinaryOp::Add,
                BinaryOp::Sub,
                BinaryOp::Mul,
                BinaryOp::Div,
                BinaryOp::Max,
                BinaryOp::Min,
            ] {
                for chunk in ra.chunks(LANES).zip(rb.chunks(LANES)) {
                    let (got, bad) = arch.dispatch(Binary {
                        op,
                        a: chunk.0,
                        b: chunk.1,
                    });
                    let want: Vec<f64> = chunk
                        .0
                        .iter()
                        .zip(chunk.1)
                        .map(|(&x, &y)| op.apply(x, y))
                        .collect();
                    assert_bits_equal(&format!("{op:?} on {name}"), &got, &want);
                    assert_eq!(
                        bad,
                        want.iter().any(|v| !v.is_finite()),
                        "{op:?} on {name}: verdict"
                    );
                }
            }
            for op in [
                UnaryOp::Negate,
                UnaryOp::Abs,
                UnaryOp::Sqrt,
                UnaryOp::Sqr,
                UnaryOp::Cube,
            ] {
                for chunk in ra.chunks(LANES) {
                    let (got, bad) = arch.dispatch(Unary { op, a: chunk });
                    let want: Vec<f64> = chunk.iter().map(|&x| op.apply(x)).collect();
                    assert_bits_equal(&format!("{op:?} on {name}"), &got, &want);
                    assert_eq!(
                        bad,
                        want.iter().any(|v| !v.is_finite()),
                        "{op:?} on {name}: verdict"
                    );
                }
            }
        }
    }

    #[test]
    fn the_finite_mask_flags_nan_and_both_infinities_and_nothing_else() {
        for (name, arch) in backends() {
            for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let mut values = vec![1.0; LANES];
                values[7] = bad;
                assert!(arch.dispatch(AnyNonFinite(&values)), "{bad} on {name}");
            }
            let fine = [
                0.0,
                -0.0,
                1e308,
                -1e308,
                f64::MIN_POSITIVE,
                5e-324,
                1.0,
                -1.0,
                2.5,
                3.0,
                4.0,
            ];
            assert!(
                !arch.dispatch(AnyNonFinite(&fine)),
                "finite values on {name}"
            );
        }
    }

    /// The lane report must be right whether the bad lane sits in a full vector or
    /// in the scalar tail — and on the scalar backend, where every lane is a tail
    /// and the any-lane reduction has nothing to reduce.
    #[test]
    fn a_bad_lane_in_the_head_or_the_tail_is_reported() {
        for (name, arch) in backends() {
            for position in [0, 5, 10] {
                let mut a = vec![4.0; LANES];
                a[position] = -1.0;
                let (got, bad) = arch.dispatch(Unary {
                    op: UnaryOp::Sqrt,
                    a: &a,
                });
                assert!(bad, "lane {position} on {name}");
                assert!(got[position].is_nan());
                assert!(
                    got.iter()
                        .enumerate()
                        .all(|(i, v)| i == position || *v == 2.0)
                );
            }
            let fine = vec![4.0; LANES];
            let (_, bad) = arch.dispatch(Unary {
                op: UnaryOp::Sqrt,
                a: &fine,
            });
            assert!(!bad, "no bad lane on {name}");
        }
    }

    #[test]
    fn compare_kernels_put_the_nudge_where_the_walker_did() {
        let a = [6.0, 4.0, 6.0, 1e200, 1e200, -3.0, 0.0, 0.0, 2.0, 2.0, 7.5];
        let b = [6.0, 6.0, 4.0, 1e200, 1e199, -3.0, -0.0, 0.0, 2.0, 3.0, 7.5];
        for (name, arch) in backends() {
            for op in [CompareOp::Lt, CompareOp::Lte, CompareOp::Gt, CompareOp::Gte] {
                let got = arch.dispatch(Compare { op, a: &a, b: &b });
                let want: Vec<f64> = a
                    .iter()
                    .zip(&b)
                    .map(|(&x, &y)| lane::compare(op, x, y))
                    .collect();
                assert_bits_equal(&format!("{op:?} on {name}"), &got, &want);
            }
            let strict = arch.dispatch(Compare {
                op: CompareOp::Lt,
                a: &a,
                b: &b,
            });
            assert_eq!(
                strict[0].to_bits(),
                EPSILON.to_bits(),
                "6 < 6 is exactly the nudge on {name}"
            );
            assert_eq!(strict[1], -2.0, "4 < 6 is exactly -2 on {name}");
        }
    }

    #[test]
    fn near_eq_kernel_matches_its_scalar_definition() {
        let a = [1.0, 1.05, 0.95, 1.2, -1.0, 0.0, -0.0, 1e300, 3.0, 2.0, 2.0];
        let b = [1.0, 1.0, 1.0, 1.0, -1.1, -0.0, 0.0, 1e300, 3.5, 2.1, 1.9];
        for (name, arch) in backends() {
            for t in [0.1, 0.0, 1e-9] {
                let got = arch.dispatch(NearEq { a: &a, b: &b, t });
                let want: Vec<f64> = a
                    .iter()
                    .zip(&b)
                    .map(|(&x, &y)| lane::near_eq(x, y, t))
                    .collect();
                assert_bits_equal(&format!("near_eq t={t} on {name}"), &got, &want);
            }
        }
    }

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
}
