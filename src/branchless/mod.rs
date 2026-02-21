//! Branchless Operations — "CPUの迷いをなくす"
//!
//! Modern CPUs use deep pipelines (14-20 stages). A mispredicted branch
//! flushes the entire pipeline, wasting ~15-20 cycles.
//!
//! This module provides branchless alternatives:
//! - mask.blend(a, b): compute both paths, select by bitmask
//! - Branchless min/max/clamp/abs
//! - Branchless CSS color parsing
//! - Branchless DOM filtering

pub mod mask;
pub mod color;
pub mod filter;

/// Branchless select: if cond { a } else { b }
/// Works by computing both and masking.
#[inline(always)]
pub fn select_f32(cond: bool, a: f32, b: f32) -> f32 {
    let m = -(cond as i32) as u32; // 0xFFFFFFFF if true, 0x00000000 if false
    let a_bits = a.to_bits();
    let b_bits = b.to_bits();
    f32::from_bits((a_bits & m) | (b_bits & !m))
}

/// Branchless select for i32
#[inline(always)]
pub fn select_i32(cond: bool, a: i32, b: i32) -> i32 {
    let m = -(cond as i32); // -1 if true, 0 if false
    (a & m) | (b & !m)
}

/// Branchless select for u8
#[inline(always)]
pub fn select_u8(cond: bool, a: u8, b: u8) -> u8 {
    let m = (-(cond as i8)) as u8; // 0xFF if true, 0x00 if false
    (a & m) | (b & !m)
}

/// Branchless min
#[inline(always)]
pub fn min_f32(a: f32, b: f32) -> f32 {
    select_f32(a < b, a, b)
}

/// Branchless max
#[inline(always)]
pub fn max_f32(a: f32, b: f32) -> f32 {
    select_f32(a > b, a, b)
}

/// Branchless clamp
#[inline(always)]
pub fn clamp_f32(val: f32, lo: f32, hi: f32) -> f32 {
    max_f32(lo, min_f32(val, hi))
}

/// Branchless absolute value (no branch, pure bit manipulation)
#[inline(always)]
pub fn abs_f32(x: f32) -> f32 {
    f32::from_bits(x.to_bits() & 0x7FFFFFFF) // clear sign bit
}

/// Branchless sign: returns -1.0, 0.0, or 1.0
#[inline(always)]
pub fn sign_f32(x: f32) -> f32 {
    let pos = (x > 0.0) as u32 as f32;  // 1.0 if positive
    let neg = (x < 0.0) as u32 as f32;  // 1.0 if negative
    pos - neg
}

/// Branchless step function: 0.0 if x < edge, 1.0 if x >= edge
#[inline(always)]
pub fn step_f32(edge: f32, x: f32) -> f32 {
    (x >= edge) as u32 as f32
}

/// Branchless smoothstep: Hermite interpolation between 0 and 1
#[inline(always)]
pub fn smoothstep_f32(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = clamp_f32((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    // t * t * (3.0 - 2.0 * t) — using FMA-friendly form:
    // t² * (3 - 2t) = t² * 3 - t³ * 2
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_f32() {
        assert!((select_f32(true, 10.0, 20.0) - 10.0).abs() < 1e-6);
        assert!((select_f32(false, 10.0, 20.0) - 20.0).abs() < 1e-6);
    }

    #[test]
    fn test_select_i32() {
        assert_eq!(select_i32(true, 5, 10), 5);
        assert_eq!(select_i32(false, 5, 10), 10);
    }

    #[test]
    fn test_clamp() {
        assert!((clamp_f32(0.5, 0.0, 1.0) - 0.5).abs() < 1e-6);
        assert!((clamp_f32(-1.0, 0.0, 1.0) - 0.0).abs() < 1e-6);
        assert!((clamp_f32(2.0, 0.0, 1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_abs() {
        assert!((abs_f32(-5.0) - 5.0).abs() < 1e-6);
        assert!((abs_f32(5.0) - 5.0).abs() < 1e-6);
        assert!((abs_f32(0.0) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_smoothstep() {
        assert!((smoothstep_f32(0.0, 1.0, 0.0) - 0.0).abs() < 1e-6);
        assert!((smoothstep_f32(0.0, 1.0, 0.5) - 0.5).abs() < 1e-6);
        assert!((smoothstep_f32(0.0, 1.0, 1.0) - 1.0).abs() < 1e-6);
    }
}
