//! A seeded random generator and a hash for the room effects: the same seed always gives the
//! same stream, on every platform, with no system state.

/// 2^53, the number of distinct doubles in `[0, 1)` a 53-bit draw gives.
const TWO_53: f64 = 9_007_199_254_740_992.0;

/// xorshift64* (Vigna), seeded through [`mix`] so nearby seeds give unrelated streams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rng(u64);

impl Rng {
    /// A generator for `seed`.
    pub(crate) fn new(seed: u64) -> Self {
        let state = mix(seed);
        // Zero is xorshift's only fixed point.
        Self(if state == 0 { 0x9E37_79B9_7F4A_7C15 } else { state })
    }

    /// The next 64 random bits.
    pub(crate) fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `[0, 1)`.
    pub(crate) fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / TWO_53
    }

    /// Uniform in `[lo, hi)`.
    pub(crate) fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}

/// The splitmix64 finaliser: a well-mixed 64-bit hash of `x`.
pub(crate) fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A hash of `(a, b)` spread over `[0, 1)`: per-item randomness that needs no state, such as a
/// sparkle's phase or a chain's start delay.
pub(crate) fn hash01(a: u64, b: u64) -> f64 {
    (mix(a ^ mix(b)) >> 11) as f64 / TWO_53
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_generator_repeats_per_seed_and_stays_in_range() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        let mut c = Rng::new(8);
        let xs: Vec<u64> = (0..16).map(|_| a.next_u64()).collect();
        let ys: Vec<u64> = (0..16).map(|_| b.next_u64()).collect();
        let zs: Vec<u64> = (0..16).map(|_| c.next_u64()).collect();
        assert_eq!(xs, ys);
        assert_ne!(xs, zs);
        let mut r = Rng::new(0);
        let mut sum = 0.0;
        for _ in 0..4000 {
            let u = r.unit();
            assert!((0.0..1.0).contains(&u));
            let v = r.range(-2.0, 3.0);
            assert!((-2.0..3.0).contains(&v));
            sum += u;
        }
        // A fair stream averages one half.
        assert!((sum / 4000.0 - 0.5).abs() < 0.03);
        let h = hash01(3, 4);
        assert!((0.0..1.0).contains(&h));
        assert!((hash01(3, 4) - h).abs() < f64::EPSILON);
        assert!((hash01(4, 3) - h).abs() > 1e-6);
    }
}
