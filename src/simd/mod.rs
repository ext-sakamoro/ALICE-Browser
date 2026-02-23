//! ALICE SIMD Engine — Structure of Arrays + Vectorized Processing
//!
//! "CPU/GPUのシリコンを限界までしゃぶり尽くす"
//!
//! This module provides:
//! - `SoA` (Structure of Arrays) data layout for cache-friendly SIMD access
//! - Platform-adaptive SIMD: AVX2 (8-wide) / SSE2 (4-wide) / NEON (4-wide) / Scalar fallback
//! - Batch DOM classification, ad-block matching, and layout computation

pub mod adblock;
pub mod classify;
pub mod layout;
pub mod soa;

/// SIMD lane width detected at compile time.
/// AVX2 = 8, SSE2/NEON = 4, Scalar = 1
pub const SIMD_WIDTH: usize = detect_simd_width();

const fn detect_simd_width() -> usize {
    #[cfg(target_arch = "x86_64")]
    {
        // AVX2 is available on most modern x86_64; runtime check done at init
        8
    }
    #[cfg(target_arch = "aarch64")]
    {
        4 // NEON: 128-bit = 4xf32
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        1 // Scalar fallback
    }
}

/// Align a count up to the next `SIMD_WIDTH` boundary.
#[inline(always)]
#[must_use] 
pub const fn align_up(n: usize) -> usize {
    (n + SIMD_WIDTH - 1) & !(SIMD_WIDTH - 1)
}

/// Portable 8-wide f32 vector (maps to AVX2 __m256 or 2x NEON `float32x4_t`)
#[derive(Clone, Copy)]
#[repr(C, align(32))]
pub struct F32x8 {
    pub v: [f32; 8],
}

impl F32x8 {
    #[inline(always)]
    #[must_use] 
    pub const fn splat(val: f32) -> Self {
        Self { v: [val; 8] }
    }

    #[inline(always)]
    #[must_use] 
    pub const fn zero() -> Self {
        Self::splat(0.0)
    }

    /// Load from aligned slice (must have >= 8 elements)
    #[inline(always)]
    #[must_use] 
    pub fn load(slice: &[f32]) -> Self {
        assert!(slice.len() >= 8, "F32x8::load requires >= 8 elements, got {}", slice.len());
        #[cfg(target_arch = "x86_64")]
        // SAFETY: AVX2 is checked at runtime. slice has >= 8 f32 elements (assert above).
        // F32x8 is repr(C, align(32)) and __m256 is 256-bit, so the transmute is valid.
        unsafe {
            if is_x86_feature_detected!("avx2") {
                let v = core::arch::x86_64::_mm256_loadu_ps(slice.as_ptr());
                return core::mem::transmute(v);
            }
        }
        // Fallback: scalar load
        let mut v = [0.0f32; 8];
        v.copy_from_slice(&slice[..8]);
        Self { v }
    }

    /// Store to aligned slice
    #[inline(always)]
    pub fn store(self, slice: &mut [f32]) {
        assert!(slice.len() >= 8, "F32x8::store requires >= 8 elements, got {}", slice.len());
        #[cfg(target_arch = "x86_64")]
        // SAFETY: AVX2 is checked at runtime. slice has >= 8 f32 elements (assert above).
        // F32x8 is repr(C, align(32)) matching __m256 layout; transmute is valid.
        unsafe {
            if is_x86_feature_detected!("avx2") {
                core::arch::x86_64::_mm256_storeu_ps(
                    slice.as_mut_ptr(),
                    core::mem::transmute(self),
                );
                return;
            }
        }
        slice[..8].copy_from_slice(&self.v);
    }

    /// Element-wise addition
    #[allow(clippy::should_implement_trait)]
    #[inline(always)]
    #[must_use] 
    pub fn add(self, rhs: Self) -> Self {
        #[cfg(target_arch = "x86_64")]
        // SAFETY: AVX2 is checked at runtime. F32x8 is repr(C, align(32)) matching __m256 layout.
        // All transmutes between F32x8 and __m256 are valid due to identical size and alignment.
        unsafe {
            if is_x86_feature_detected!("avx2") {
                let a: core::arch::x86_64::__m256 = core::mem::transmute(self);
                let b: core::arch::x86_64::__m256 = core::mem::transmute(rhs);
                return core::mem::transmute(core::arch::x86_64::_mm256_add_ps(a, b));
            }
        }
        let mut out = [0.0f32; 8];
        for (out_elem, (a, b)) in out.iter_mut().zip(self.v.iter().zip(rhs.v.iter())) {
            *out_elem = a + b;
        }
        Self { v: out }
    }

    /// Element-wise multiplication
    #[allow(clippy::should_implement_trait)]
    #[inline(always)]
    #[must_use] 
    pub fn mul(self, rhs: Self) -> Self {
        #[cfg(target_arch = "x86_64")]
        // SAFETY: AVX2 is checked at runtime. F32x8 is repr(C, align(32)) matching __m256 layout.
        // Transmutes between F32x8 and __m256 are valid due to identical size and alignment.
        unsafe {
            if is_x86_feature_detected!("avx2") {
                let a: core::arch::x86_64::__m256 = core::mem::transmute(self);
                let b: core::arch::x86_64::__m256 = core::mem::transmute(rhs);
                return core::mem::transmute(core::arch::x86_64::_mm256_mul_ps(a, b));
            }
        }
        let mut out = [0.0f32; 8];
        for (out_elem, (a, b)) in out.iter_mut().zip(self.v.iter().zip(rhs.v.iter())) {
            *out_elem = a * b;
        }
        Self { v: out }
    }

    /// Fused multiply-add: self * a + b (1 instruction on FMA-capable CPUs)
    #[inline(always)]
    #[must_use] 
    pub fn fma(self, a: Self, b: Self) -> Self {
        #[cfg(target_arch = "x86_64")]
        // SAFETY: FMA support is checked at runtime. F32x8 is repr(C, align(32)) matching
        // __m256 layout. Transmutes between F32x8 and __m256 are valid.
        unsafe {
            if is_x86_feature_detected!("fma") {
                let s: core::arch::x86_64::__m256 = core::mem::transmute(self);
                let ma: core::arch::x86_64::__m256 = core::mem::transmute(a);
                let mb: core::arch::x86_64::__m256 = core::mem::transmute(b);
                return core::mem::transmute(core::arch::x86_64::_mm256_fmadd_ps(s, ma, mb));
            }
        }
        self.mul(a).add(b)
    }

    /// Element-wise maximum
    #[inline(always)]
    #[must_use] 
    pub fn max(self, rhs: Self) -> Self {
        #[cfg(target_arch = "x86_64")]
        // SAFETY: AVX2 is checked at runtime. F32x8 is repr(C, align(32)) matching __m256 layout.
        // Transmutes between F32x8 and __m256 are valid due to identical size and alignment.
        unsafe {
            if is_x86_feature_detected!("avx2") {
                let a: core::arch::x86_64::__m256 = core::mem::transmute(self);
                let b: core::arch::x86_64::__m256 = core::mem::transmute(rhs);
                return core::mem::transmute(core::arch::x86_64::_mm256_max_ps(a, b));
            }
        }
        let mut out = [0.0f32; 8];
        for (out_elem, (a, b)) in out.iter_mut().zip(self.v.iter().zip(rhs.v.iter())) {
            *out_elem = if a > b { *a } else { *b };
        }
        Self { v: out }
    }

    /// Compare greater-than, returns mask (all 1s or all 0s per lane)
    #[inline(always)]
    #[must_use] 
    pub fn cmp_gt(self, rhs: Self) -> MaskF32x8 {
        #[cfg(target_arch = "x86_64")]
        // SAFETY: AVX2 is checked at runtime. F32x8 and MaskF32x8 are repr(C, align(32)) matching
        // __m256 layout. _CMP_GT_OQ is a valid immediate for _mm256_cmp_ps. Transmutes are valid.
        unsafe {
            if is_x86_feature_detected!("avx2") {
                let a: core::arch::x86_64::__m256 = core::mem::transmute(self);
                let b: core::arch::x86_64::__m256 = core::mem::transmute(rhs);
                let cmp = core::arch::x86_64::_mm256_cmp_ps(a, b, core::arch::x86_64::_CMP_GT_OQ);
                return MaskF32x8 {
                    bits: core::mem::transmute(cmp),
                };
            }
        }
        let mut bits = [0u32; 8];
        for (bit, (a, b)) in bits.iter_mut().zip(self.v.iter().zip(rhs.v.iter())) {
            *bit = if a > b { 0xFFFF_FFFF } else { 0 };
        }
        MaskF32x8 { bits }
    }
}

/// 8-wide comparison mask for branchless select
#[derive(Clone, Copy)]
#[repr(C, align(32))]
pub struct MaskF32x8 {
    pub bits: [u32; 8],
}

impl MaskF32x8 {
    /// Branchless blend: select `a` where mask is true, `b` where false.
    /// This is THE key operation for branch elimination.
    ///
    /// mask.blend(a, b) ≡ (mask & a) | (!mask & b)
    #[inline(always)]
    #[must_use] 
    pub fn blend(self, a: F32x8, b: F32x8) -> F32x8 {
        #[cfg(target_arch = "x86_64")]
        // SAFETY: AVX2 is checked at runtime. MaskF32x8 and F32x8 are repr(C, align(32)) matching
        // __m256 layout. _mm256_blendv_ps uses the high bit of each lane for selection.
        // All transmutes are valid due to identical size and alignment.
        unsafe {
            if is_x86_feature_detected!("avx2") {
                let mask: core::arch::x86_64::__m256 = core::mem::transmute(self.bits);
                let va: core::arch::x86_64::__m256 = core::mem::transmute(a);
                let vb: core::arch::x86_64::__m256 = core::mem::transmute(b);
                return core::mem::transmute(core::arch::x86_64::_mm256_blendv_ps(vb, va, mask));
            }
        }
        // Scalar branchless: bit-level blend
        let mut out = [0.0f32; 8];
        for (out_elem, ((av, bv), m)) in out
            .iter_mut()
            .zip(a.v.iter().zip(b.v.iter()).zip(self.bits.iter()))
        {
            let a_bits = av.to_bits();
            let b_bits = bv.to_bits();
            *out_elem = f32::from_bits((a_bits & m) | (b_bits & !m));
        }
        F32x8 { v: out }
    }

    /// Bitwise AND of two masks
    #[inline(always)]
    #[must_use] 
    pub fn and(self, rhs: Self) -> Self {
        let mut bits = [0u32; 8];
        for (out_bit, (a, b)) in bits.iter_mut().zip(self.bits.iter().zip(rhs.bits.iter())) {
            *out_bit = a & b;
        }
        Self { bits }
    }

    /// Bitwise OR of two masks
    #[inline(always)]
    #[must_use] 
    pub fn or(self, rhs: Self) -> Self {
        let mut bits = [0u32; 8];
        for (out_bit, (a, b)) in bits.iter_mut().zip(self.bits.iter().zip(rhs.bits.iter())) {
            *out_bit = a | b;
        }
        Self { bits }
    }

    /// Invert mask
    #[allow(clippy::should_implement_trait)]
    #[inline(always)]
    #[must_use] 
    pub fn not(self) -> Self {
        let mut bits = [0u32; 8];
        for (out_bit, b) in bits.iter_mut().zip(self.bits.iter()) {
            *out_bit = !b;
        }
        Self { bits }
    }

    /// True if any lane is set
    #[inline(always)]
    #[must_use] 
    pub fn any(self) -> bool {
        self.bits.iter().any(|&b| b != 0)
    }

    /// Count set lanes
    #[inline(always)]
    #[must_use] 
    pub fn count(self) -> usize {
        self.bits.iter().filter(|&&b| b != 0).count()
    }
}

/// 8-wide i32 vector for integer SIMD operations (classification indices, etc.)
#[derive(Clone, Copy)]
#[repr(C, align(32))]
pub struct I32x8 {
    pub v: [i32; 8],
}

impl I32x8 {
    #[inline(always)]
    #[must_use] 
    pub const fn splat(val: i32) -> Self {
        Self { v: [val; 8] }
    }

    #[inline(always)]
    #[must_use] 
    pub fn load(slice: &[i32]) -> Self {
        assert!(slice.len() >= 8, "I32x8::load requires >= 8 elements, got {}", slice.len());
        let mut v = [0i32; 8];
        v.copy_from_slice(&slice[..8]);
        Self { v }
    }

    #[inline(always)]
    pub fn store(self, slice: &mut [i32]) {
        assert!(slice.len() >= 8, "I32x8::store requires >= 8 elements, got {}", slice.len());
        slice[..8].copy_from_slice(&self.v);
    }

    /// Element-wise addition
    #[allow(clippy::should_implement_trait)]
    #[inline(always)]
    #[must_use] 
    pub fn add(self, rhs: Self) -> Self {
        let mut out = [0i32; 8];
        for (out_elem, (a, b)) in out.iter_mut().zip(self.v.iter().zip(rhs.v.iter())) {
            *out_elem = a + b;
        }
        Self { v: out }
    }

    /// Compare equal
    #[inline(always)]
    #[must_use] 
    pub fn cmp_eq(self, rhs: Self) -> MaskF32x8 {
        let mut bits = [0u32; 8];
        for (bit, (a, b)) in bits.iter_mut().zip(self.v.iter().zip(rhs.v.iter())) {
            *bit = if a == b { 0xFFFF_FFFF } else { 0 };
        }
        MaskF32x8 { bits }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_f32x8_add() {
        let a = F32x8::splat(1.0);
        let b = F32x8::splat(2.0);
        let c = a.add(b);
        for &v in &c.v {
            assert!((v - 3.0).abs() < 1e-6);
        }
    }

    #[test]
    fn test_branchless_blend() {
        let a = F32x8::splat(10.0);
        let b = F32x8::splat(20.0);
        let mask = MaskF32x8 {
            bits: [0xFFFFFFFF, 0, 0xFFFFFFFF, 0, 0xFFFFFFFF, 0, 0xFFFFFFFF, 0],
        };
        let result = mask.blend(a, b);
        assert!((result.v[0] - 10.0).abs() < 1e-6);
        assert!((result.v[1] - 20.0).abs() < 1e-6);
        assert!((result.v[2] - 10.0).abs() < 1e-6);
    }

    #[test]
    fn test_fma() {
        let a = F32x8::splat(2.0);
        let b = F32x8::splat(3.0);
        let c = F32x8::splat(4.0);
        // a * b + c = 2*3+4 = 10
        let result = a.fma(b, c);
        for &v in &result.v {
            assert!((v - 10.0).abs() < 1e-6);
        }
    }
}
