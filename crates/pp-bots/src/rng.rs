//! Deterministic draws for rider identity.
//!
//! SplitMix64 mixes the seed. xorshift64 then walks the stream. The same seed
//! always yields the same draws. This is not a cryptographic generator. The
//! stream matches the one in `pp-arcade` so a seed means the same thing in
//! both crates.

/// Seeded stream of draws.
#[derive(Debug)]
pub(crate) struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut state = splitmix(seed);
        if state == 0 {
            state = 0xA5A5_A5A5_A5A5_A5A5;
        }
        Self { state }
    }

    /// Uniform draw in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        let bits = self.next_u64() >> 11;
        (bits as f64) * (1.0 / ((1_u64 << 53) as f64))
    }

    /// Integer in `0..n`. Returns 0 when `n` is 0.
    pub fn index(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            u32::try_from(self.next_u64() % u64::from(n)).unwrap_or(0)
        }
    }

    /// Uniform draw in `[lo, hi)`. Returns `lo` when the span is empty.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        if !lo.is_finite() || !hi.is_finite() || hi <= lo {
            lo
        } else {
            lo + (hi - lo) * self.unit()
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }
}

fn splitmix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
