//! A small deterministic random number generator (SplitMix64).
//!
//! Integer-only, so output is identical on every platform. Not for
//! cryptography: the demo uses it only to vary generated data.

/// SplitMix64 generator ([Steele, Lea, Flood 2014](https://doi.org/10.1145/2714064.2660195)).
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// A generator for `seed` and a stream label. Different labels give
    /// independent streams for the same seed.
    pub fn new(seed: u64, label: &str) -> Rng {
        let mut h = seed ^ 0x9e37_79b9_7f4a_7c15;
        for b in label.bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        let mut r = Rng(h);
        r.next_u64();
        r
    }

    /// Next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n`. `n` must be > 0. Uses the high bits of a 128-bit
    /// product (bias below 2^-64 per draw; deterministic either way).
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "Rng::below(0)");
        ((u128::from(self.next_u64()) * u128::from(n)) >> 64) as u64
    }

    /// Uniform in `lo..=hi`.
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        assert!(lo <= hi, "Rng::range: lo > hi");
        lo + self.below(hi - lo + 1)
    }

    /// True with probability `num / den`.
    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        self.below(den) < num
    }

    /// A uniformly chosen element.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }

    /// 32 random bytes (for receipt nonces).
    pub fn bytes32(&mut self) -> [u8; 32] {
        let mut out = [0u8; 32];
        for chunk in out.chunks_mut(8) {
            chunk.copy_from_slice(&self.next_u64().to_le_bytes());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_label_separated() {
        let a: Vec<u64> = (0..4).map({
            let mut r = Rng::new(7, "x");
            move |_| r.next_u64()
        }).collect();
        let b: Vec<u64> = (0..4).map({
            let mut r = Rng::new(7, "x");
            move |_| r.next_u64()
        }).collect();
        let c = Rng::new(7, "y").next_u64();
        assert_eq!(a, b);
        assert_ne!(a[0], c);
    }

    #[test]
    fn below_in_range() {
        let mut r = Rng::new(1, "t");
        for n in 1..50 {
            assert!(r.below(n) < n);
        }
        assert_eq!(r.range(5, 5), 5);
    }
}
