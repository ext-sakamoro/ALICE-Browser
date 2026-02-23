//! SIMD Mask Operations — The core of branchless programming
//!
//! In traditional code:
//!   if condition { do_a() } else { do_b() }  // CPU gambles on branch prediction
//!
//! Branchless:
//!   let mask = compute_condition();  // SIMD comparison → all 1s or all 0s
//!   mask.blend(result_a, result_b)   // Always computes both, selects by mask
//!
//! The CPU never stalls because there IS no branch to mispredict.
//!
//! This is the fundamental technique used in GPU shaders (which have NO branches),
//! and we bring it to CPU-side DOM processing.

/// A bitmask for N boolean decisions, stored as packed bits.
///
/// Used for batch operations on DOM nodes:
/// - "Which of these 64 nodes are ads?"
/// - "Which of these 64 nodes have text_density > 10?"
///
/// Operations are branchless at the bit level.
#[derive(Clone, Copy, Debug)]
pub struct BitMask64(pub u64);

impl BitMask64 {
    pub const ALL_TRUE: Self = Self(u64::MAX);
    pub const ALL_FALSE: Self = Self(0);

    /// Create from a boolean condition per bit position
    #[inline(always)]
    pub fn from_bool(val: bool) -> Self {
        Self(-(val as i64) as u64)
    }

    /// Set bit at position
    #[inline(always)]
    pub fn set(&mut self, pos: usize) {
        self.0 |= 1u64 << pos;
    }

    /// Clear bit at position
    #[inline(always)]
    pub fn clear(&mut self, pos: usize) {
        self.0 &= !(1u64 << pos);
    }

    /// Test bit at position
    #[inline(always)]
    pub fn test(&self, pos: usize) -> bool {
        (self.0 >> pos) & 1 != 0
    }

    /// Branchless AND: intersection of two masks
    #[inline(always)]
    pub fn and(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }

    /// Branchless OR: union of two masks
    #[inline(always)]
    pub fn or(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }

    /// Branchless NOT: invert all bits
    #[allow(clippy::should_implement_trait)]
    #[inline(always)]
    pub fn not(self) -> Self {
        Self(!self.0)
    }

    /// Branchless XOR
    #[inline(always)]
    pub fn xor(self, rhs: Self) -> Self {
        Self(self.0 ^ rhs.0)
    }

    /// Count set bits (popcount) — uses hardware POPCNT instruction
    #[inline(always)]
    pub fn count_ones(self) -> u32 {
        self.0.count_ones()
    }

    /// Leading zeros — useful for finding first set bit
    #[inline(always)]
    pub fn leading_zeros(self) -> u32 {
        self.0.leading_zeros()
    }

    /// Trailing zeros — index of lowest set bit
    #[inline(always)]
    pub fn trailing_zeros(self) -> u32 {
        self.0.trailing_zeros()
    }

    /// Is any bit set?
    #[inline(always)]
    pub fn any(self) -> bool {
        self.0 != 0
    }

    /// Are all bits set?
    #[inline(always)]
    pub fn all(self) -> bool {
        self.0 == u64::MAX
    }

    /// Is no bit set?
    #[inline(always)]
    pub fn none(self) -> bool {
        self.0 == 0
    }

    /// Blend two f32 slices based on this mask.
    ///
    /// For each bit i:
    ///   `out[i] = if mask.test(i) { a[i] } else { b[i] }`
    ///
    /// This is the scalar version of the SIMD blend operation.
    #[inline]
    pub fn blend_slices(self, a: &[f32], b: &[f32], out: &mut [f32]) {
        let len = a.len().min(b.len()).min(out.len()).min(64);
        for (i, (out_elem, (av, bv))) in out
            .iter_mut()
            .zip(a.iter().zip(b.iter()))
            .enumerate()
            .take(len)
        {
            let m = -((self.0 >> i) as i64 & 1) as u32; // 0xFFFFFFFF or 0x00000000
            let a_bits = av.to_bits();
            let b_bits = bv.to_bits();
            *out_elem = f32::from_bits((a_bits & m) | (b_bits & !m));
        }
    }

    /// Iterate over set bit positions (useful for sparse operations)
    #[inline]
    pub fn iter_set_bits(self) -> SetBitIterator {
        SetBitIterator(self.0)
    }
}

/// Iterator over set bit positions in a BitMask64.
/// Uses trailing_zeros + clear-lowest-bit trick for branchless iteration.
pub struct SetBitIterator(u64);

impl Iterator for SetBitIterator {
    type Item = usize;

    #[inline(always)]
    fn next(&mut self) -> Option<usize> {
        if self.0 == 0 {
            return None;
        }
        let pos = self.0.trailing_zeros() as usize;
        self.0 &= self.0 - 1; // Clear lowest set bit (branchless!)
        Some(pos)
    }
}

/// Comparison mask builder — creates BitMask64 from array comparisons.
///
/// Example:
///   let ad_mask = ComparisonMask::gt(text_densities, 10.0);
///   let nav_mask = ComparisonMask::gt(link_densities, 0.6);
///   let prune_mask = ad_mask.or(nav_mask);
pub struct ComparisonMask;

impl ComparisonMask {
    /// Create mask where `slice[i] > threshold`
    #[inline]
    pub fn gt(slice: &[f32], threshold: f32) -> BitMask64 {
        let mut mask = BitMask64::ALL_FALSE;
        let len = slice.len().min(64);
        for (i, val) in slice[..len].iter().enumerate() {
            // Branchless: (slice[i] > threshold) produces 0 or 1
            if *val > threshold {
                mask.set(i);
            }
        }
        mask
    }

    /// Create mask where `slice[i] == value`
    #[inline]
    pub fn eq_i32(slice: &[i32], value: i32) -> BitMask64 {
        let mut mask = BitMask64::ALL_FALSE;
        let len = slice.len().min(64);
        for (i, val) in slice[..len].iter().enumerate() {
            if *val == value {
                mask.set(i);
            }
        }
        mask
    }

    /// Create mask where `slice[i] != 0.0` (truthy)
    #[inline]
    pub fn nonzero(slice: &[f32]) -> BitMask64 {
        let mut mask = BitMask64::ALL_FALSE;
        let len = slice.len().min(64);
        for (i, val) in slice[..len].iter().enumerate() {
            if *val != 0.0 {
                mask.set(i);
            }
        }
        mask
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitmask_basic() {
        let mut m = BitMask64::ALL_FALSE;
        m.set(0);
        m.set(3);
        m.set(7);
        assert!(m.test(0));
        assert!(!m.test(1));
        assert!(m.test(3));
        assert_eq!(m.count_ones(), 3);
    }

    #[test]
    fn test_bitmask_blend() {
        let mask = BitMask64(0b1010); // bits 1, 3 set
        let a = [10.0, 20.0, 30.0, 40.0];
        let b = [1.0, 2.0, 3.0, 4.0];
        let mut out = [0.0f32; 4];
        mask.blend_slices(&a, &b, &mut out);

        assert!((out[0] - 1.0).abs() < 1e-6); // bit 0 clear → b
        assert!((out[1] - 20.0).abs() < 1e-6); // bit 1 set → a
        assert!((out[2] - 3.0).abs() < 1e-6); // bit 2 clear → b
        assert!((out[3] - 40.0).abs() < 1e-6); // bit 3 set → a
    }

    #[test]
    fn test_set_bit_iterator() {
        let m = BitMask64(0b10110);
        let bits: Vec<usize> = m.iter_set_bits().collect();
        assert_eq!(bits, vec![1, 2, 4]);
    }

    #[test]
    fn test_comparison_mask() {
        let data = [5.0, 15.0, 3.0, 20.0, 8.0, 11.0];
        let mask = ComparisonMask::gt(&data, 10.0);
        assert!(!mask.test(0));
        assert!(mask.test(1));
        assert!(!mask.test(2));
        assert!(mask.test(3));
        assert!(!mask.test(4));
        assert!(mask.test(5));
    }

    #[test]
    fn test_bitmask_xor() {
        let a = BitMask64(0b1100);
        let b = BitMask64(0b1010);
        let xor = a.xor(b);
        assert_eq!(xor.0, 0b0110);
        assert!(!xor.test(0));
        assert!(xor.test(1));
        assert!(xor.test(2));
        assert!(!xor.test(3));
    }

    #[test]
    fn test_bitmask_not() {
        let m = BitMask64::ALL_TRUE;
        let inv = m.not();
        assert!(inv.none());
        assert!(!inv.any());
        assert_eq!(inv.count_ones(), 0);

        let inv2 = BitMask64::ALL_FALSE.not();
        assert!(inv2.all());
        assert_eq!(inv2.count_ones(), 64);
    }

    #[test]
    fn test_bitmask_all_none_any() {
        assert!(BitMask64::ALL_TRUE.all());
        assert!(!BitMask64::ALL_TRUE.none());
        assert!(BitMask64::ALL_TRUE.any());

        assert!(!BitMask64::ALL_FALSE.all());
        assert!(BitMask64::ALL_FALSE.none());
        assert!(!BitMask64::ALL_FALSE.any());

        let single = BitMask64(1);
        assert!(!single.all());
        assert!(!single.none());
        assert!(single.any());
    }

    #[test]
    fn test_bitmask_from_bool() {
        let t = BitMask64::from_bool(true);
        assert_eq!(t.0, u64::MAX);
        assert!(t.all());

        let f = BitMask64::from_bool(false);
        assert_eq!(f.0, 0);
        assert!(f.none());
    }

    #[test]
    fn test_bitmask_clear() {
        let mut m = BitMask64::ALL_TRUE;
        m.clear(0);
        assert!(!m.test(0));
        assert!(m.test(1));
        assert_eq!(m.count_ones(), 63);
    }

    #[test]
    fn test_bitmask_leading_trailing_zeros() {
        let m = BitMask64(0b1000_0100);
        assert_eq!(m.trailing_zeros(), 2);
        assert_eq!(m.leading_zeros(), 56);

        assert_eq!(BitMask64::ALL_FALSE.trailing_zeros(), 64);
        assert_eq!(BitMask64::ALL_FALSE.leading_zeros(), 64);
    }

    #[test]
    fn test_set_bit_iterator_empty() {
        let m = BitMask64::ALL_FALSE;
        let bits: Vec<usize> = m.iter_set_bits().collect();
        assert!(bits.is_empty());
    }

    #[test]
    fn test_comparison_mask_eq_i32() {
        let data = [1, 2, 3, 2, 5, 2];
        let mask = ComparisonMask::eq_i32(&data, 2);
        assert!(!mask.test(0));
        assert!(mask.test(1));
        assert!(!mask.test(2));
        assert!(mask.test(3));
        assert!(!mask.test(4));
        assert!(mask.test(5));
        assert_eq!(mask.count_ones(), 3);
    }

    #[test]
    fn test_comparison_mask_nonzero() {
        let data = [0.0, 1.0, 0.0, -1.0, 0.0];
        let mask = ComparisonMask::nonzero(&data);
        assert!(!mask.test(0));
        assert!(mask.test(1));
        assert!(!mask.test(2));
        assert!(mask.test(3));
        assert!(!mask.test(4));
        assert_eq!(mask.count_ones(), 2);
    }
}
