//! Fast Math — Instruction-Level Optimization
//!
//! "重い命令を軽い命令に置き換える"
//!
//! ## Division Exorcism (除算排除)
//! Division is 10-20x slower than multiplication.
//!   x / k  →  x * (1.0/k)   (pre-compute reciprocal)
//!   x / y  →  x * fast_rcp(y) (approximate reciprocal, 1 cycle vs 20)
//!
//! ## FMA (Fused Multiply-Add)
//! a * b + c in 1 instruction instead of 2 (also reduces rounding error).
//!
//! ## Sqrt Elimination
//! length() requires sqrt. length_squared() doesn't.
//! For comparisons: |a| < |b|  ↔  a² < b²  (no sqrt needed)
//!
//! ## Fast Inverse Square Root
//! The legendary Quake III trick, adapted for f32/f64.

/// Pre-computed reciprocals for common divisors.
/// Computed once at init, reused everywhere.
///
/// Usage: instead of `x / 255.0`, use `x * RECIPROCALS.inv_255`
pub struct Reciprocals {
    pub inv_255: f32,
    pub inv_128: f32,
    pub inv_64: f32,
    pub inv_32: f32,
    pub inv_16: f32,
    pub inv_8: f32,
    pub inv_4: f32,
    pub inv_2: f32,
    pub inv_1024: f32,
    pub inv_pi: f32,
    pub inv_2pi: f32,
    pub inv_180: f32,
}

/// Global pre-computed reciprocals (const-evaluated at compile time!)
pub const RECIPROCALS: Reciprocals = Reciprocals {
    inv_255: 1.0 / 255.0,
    inv_128: 1.0 / 128.0,
    inv_64: 1.0 / 64.0,
    inv_32: 1.0 / 32.0,
    inv_16: 1.0 / 16.0,
    inv_8: 1.0 / 8.0,
    inv_4: 1.0 / 4.0,
    inv_2: 1.0 / 2.0,
    inv_1024: 1.0 / 1024.0,
    inv_pi: 1.0 / std::f32::consts::PI,
    inv_2pi: 1.0 / (2.0 * std::f32::consts::PI),
    inv_180: 1.0 / 180.0,
};

/// Fast reciprocal approximation.
/// Uses the bit-level trick for an initial estimate, then one Newton-Raphson step.
///
/// Accuracy: ~23 bits (sufficient for layout/rendering, NOT for scientific computing)
/// Speed: ~4 cycles vs ~20 for hardware division
#[inline(always)]
pub fn fast_rcp(x: f32) -> f32 {
    #[cfg(target_arch = "x86_64")]
    // SAFETY: SSE support is checked at runtime via is_x86_feature_detected!.
    // _mm_set_ss, _mm_rcp_ss, _mm_mul_ss, _mm_sub_ss, _mm_cvtss_f32 are all valid SSE
    // intrinsics operating on scalar single-precision values. No pointers are dereferenced.
    unsafe {
        if is_x86_feature_detected!("sse") {
            let v = core::arch::x86_64::_mm_set_ss(x);
            let rcp = core::arch::x86_64::_mm_rcp_ss(v);
            // One Newton-Raphson step for better accuracy:
            // rcp = rcp * (2.0 - x * rcp)
            let two = core::arch::x86_64::_mm_set_ss(2.0);
            let xr = core::arch::x86_64::_mm_mul_ss(v, rcp);
            let diff = core::arch::x86_64::_mm_sub_ss(two, xr);
            let result = core::arch::x86_64::_mm_mul_ss(rcp, diff);
            return core::arch::x86_64::_mm_cvtss_f32(result);
        }
    }
    // Fallback: just divide (compiler will optimize on NEON)
    1.0 / x
}

/// Fast inverse square root (1/√x) — the legendary Quake III algorithm.
///
/// Uses bit-level magic number 0x5f3759df for initial guess,
/// then one Newton-Raphson iteration for refinement.
///
/// Accuracy: ~0.17% error (perfect for layout, rendering, collision)
/// Speed: ~5 cycles vs ~25 for sqrt + div
#[inline(always)]
pub fn fast_inv_sqrt(x: f32) -> f32 {
    #[cfg(target_arch = "x86_64")]
    // SAFETY: SSE support is checked at runtime via is_x86_feature_detected!.
    // _mm_set_ss, _mm_rsqrt_ss, _mm_cvtss_f32 are valid SSE intrinsics operating
    // on scalar single-precision values. No pointers are dereferenced.
    unsafe {
        if is_x86_feature_detected!("sse") {
            let v = core::arch::x86_64::_mm_set_ss(x);
            let rsqrt = core::arch::x86_64::_mm_rsqrt_ss(v);
            return core::arch::x86_64::_mm_cvtss_f32(rsqrt);
        }
    }
    // Quake III fast inverse sqrt
    let half_x = 0.5 * x;
    let mut i = x.to_bits();
    i = 0x5f3759df - (i >> 1); // Magic!
    let y = f32::from_bits(i);
    // One Newton-Raphson step
    y * (1.5 - half_x * y * y)
}

/// Fast approximate square root using fast_inv_sqrt.
/// sqrt(x) = x * (1/sqrt(x))
///
/// Avoids the slow hardware sqrt instruction.
#[inline(always)]
pub fn fast_sqrt(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    x * fast_inv_sqrt(x)
}

/// Fused Multiply-Add: a * b + c
///
/// On FMA-capable CPUs, this is ONE instruction (not two).
/// Benefits:
/// 1. Speed: 1 cycle latency instead of 2
/// 2. Precision: only 1 rounding step instead of 2
#[inline(always)]
pub fn fma(a: f32, b: f32, c: f32) -> f32 {
    // std::intrinsics::fmaf32 is not stable; use the mul_add method
    a.mul_add(b, c)
}

/// FMA chain: a * b + c * d
/// = fma(a, b, c * d)
#[inline(always)]
pub fn fma_chain(a: f32, b: f32, c: f32, d: f32) -> f32 {
    a.mul_add(b, c * d)
}

/// Squared distance (no sqrt needed for comparisons).
///
/// Instead of:
///   let dist = ((x2-x1)² + (y2-y1)²).sqrt();
///   if dist < threshold { ... }
///
/// Use:
///   let dist_sq = distance_squared(x1,y1,x2,y2);
///   if dist_sq < threshold * threshold { ... }
///
/// Saves one sqrt (25 cycles) per comparison.
#[inline(always)]
pub fn distance_squared(x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    fma(dx, dx, dy * dy)
}

/// Length squared of a 2D vector (no sqrt)
#[inline(always)]
pub fn length_squared(x: f32, y: f32) -> f32 {
    fma(x, x, y * y)
}

/// Linear interpolation using FMA for precision.
/// lerp(a, b, t) = a + t * (b - a) = fma(t, b-a, a)
#[inline(always)]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    fma(t, b - a, a)
}

/// Batch multiply: multiply all elements by a constant.
/// Uses SIMD when available (Rayon for parallelism is handled externally).
#[inline]
pub fn batch_mul_scalar(data: &mut [f32], scalar: f32) {
    // Let auto-vectorization handle this
    for v in data.iter_mut() {
        *v *= scalar;
    }
}

/// Batch FMA: `data[i] = data[i] * a + b`
#[inline]
pub fn batch_fma(data: &mut [f32], a: f32, b: f32) {
    for v in data.iter_mut() {
        *v = v.mul_add(a, b);
    }
}

/// Convert degrees to radians using reciprocal multiplication.
/// deg * (π/180) = deg * π * inv_180
#[inline(always)]
pub fn deg_to_rad(deg: f32) -> f32 {
    deg * std::f32::consts::PI * RECIPROCALS.inv_180
}

/// Normalize a value from [0, max] to [0.0, 1.0] using reciprocal.
/// val / max → val * (1/max)
#[inline(always)]
pub fn normalize(val: f32, inv_max: f32) -> f32 {
    val * inv_max
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fast_rcp() {
        let x = 4.0f32;
        let rcp = fast_rcp(x);
        assert!(
            (rcp - 0.25).abs() < 0.001,
            "fast_rcp(4) = {}, expected ~0.25",
            rcp
        );
    }

    #[test]
    fn test_fast_inv_sqrt() {
        let x = 4.0f32;
        let inv_sqrt = fast_inv_sqrt(x);
        assert!(
            (inv_sqrt - 0.5).abs() < 0.01,
            "fast_inv_sqrt(4) = {}, expected ~0.5",
            inv_sqrt
        );
    }

    #[test]
    fn test_fast_sqrt() {
        let x = 9.0f32;
        let sqrt = fast_sqrt(x);
        assert!(
            (sqrt - 3.0).abs() < 0.1,
            "fast_sqrt(9) = {}, expected ~3.0",
            sqrt
        );
    }

    #[test]
    fn test_fma() {
        assert!((fma(2.0, 3.0, 4.0) - 10.0).abs() < 1e-6);
    }

    #[test]
    fn test_distance_squared() {
        let d2 = distance_squared(0.0, 0.0, 3.0, 4.0);
        assert!((d2 - 25.0).abs() < 1e-6); // 3² + 4² = 25, no sqrt needed!
    }

    #[test]
    fn test_lerp() {
        assert!((lerp(0.0, 10.0, 0.5) - 5.0).abs() < 1e-6);
        assert!((lerp(0.0, 10.0, 0.0) - 0.0).abs() < 1e-6);
        assert!((lerp(0.0, 10.0, 1.0) - 10.0).abs() < 1e-6);
    }

    #[test]
    fn test_reciprocals_accuracy() {
        // Verify pre-computed reciprocals match expected values
        assert!((RECIPROCALS.inv_255 - 1.0 / 255.0).abs() < 1e-10);
        assert!((RECIPROCALS.inv_32 - 1.0 / 32.0).abs() < 1e-10);
    }

    #[test]
    fn test_division_exorcism_equivalence() {
        // Prove that multiplication by reciprocal gives same result as division
        let x = 42.0f32;

        let div_result = x / 255.0;
        let mul_result = x * RECIPROCALS.inv_255;
        assert!((div_result - mul_result).abs() < 1e-6);

        let div_result = x / 32.0;
        let mul_result = x * RECIPROCALS.inv_32;
        assert!((div_result - mul_result).abs() < 1e-6);
    }

    #[test]
    fn test_batch_fma() {
        let mut data = [1.0, 2.0, 3.0, 4.0];
        batch_fma(&mut data, 2.0, 1.0); // data[i] = data[i] * 2 + 1
        assert!((data[0] - 3.0).abs() < 1e-6);
        assert!((data[1] - 5.0).abs() < 1e-6);
        assert!((data[2] - 7.0).abs() < 1e-6);
        assert!((data[3] - 9.0).abs() < 1e-6);
    }

    #[test]
    fn test_fma_chain() {
        // a*b + c*d = 2*3 + 4*5 = 6 + 20 = 26
        let result = fma_chain(2.0, 3.0, 4.0, 5.0);
        assert!((result - 26.0).abs() < 1e-6);
    }

    #[test]
    fn test_length_squared() {
        // |<3,4>|^2 = 9 + 16 = 25
        let ls = length_squared(3.0, 4.0);
        assert!((ls - 25.0).abs() < 1e-6);

        // |<0,0>|^2 = 0
        assert!((length_squared(0.0, 0.0) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_deg_to_rad() {
        let rad_180 = deg_to_rad(180.0);
        assert!((rad_180 - std::f32::consts::PI).abs() < 1e-4);

        let rad_90 = deg_to_rad(90.0);
        assert!((rad_90 - std::f32::consts::FRAC_PI_2).abs() < 1e-4);

        let rad_0 = deg_to_rad(0.0);
        assert!((rad_0 - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_normalize() {
        let result = normalize(128.0, RECIPROCALS.inv_255);
        assert!((result - 128.0 / 255.0).abs() < 1e-5);

        let result_zero = normalize(0.0, RECIPROCALS.inv_255);
        assert!((result_zero - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_batch_mul_scalar() {
        let mut data = [1.0, 2.0, 3.0, 4.0];
        batch_mul_scalar(&mut data, 3.0);
        assert!((data[0] - 3.0).abs() < 1e-6);
        assert!((data[1] - 6.0).abs() < 1e-6);
        assert!((data[2] - 9.0).abs() < 1e-6);
        assert!((data[3] - 12.0).abs() < 1e-6);
    }

    #[test]
    fn test_fast_sqrt_zero() {
        assert!((fast_sqrt(0.0) - 0.0).abs() < 1e-6);
        assert!((fast_sqrt(-1.0) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_fast_rcp_various() {
        // Test reciprocals of several values
        for &x in &[1.0, 2.0, 10.0, 100.0, 0.5] {
            let rcp = fast_rcp(x);
            let expected = 1.0 / x;
            assert!(
                (rcp - expected).abs() < 0.01,
                "fast_rcp({}) = {}, expected {}",
                x,
                rcp,
                expected
            );
        }
    }
}
