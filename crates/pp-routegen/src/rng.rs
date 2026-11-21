//! Deterministic draws for route generation.
//!
//! The stream is ChaCha12 via `rand_chacha` 0.3.1 and `rand` 0.8.5.
//! `ChaCha12Rng::seed_from_u64` expands the key. Attempt 0 uses the spec seed
//! unchanged. Later attempts use SplitMix64 of `seed + attempt * golden ratio`
//! so a rejected candidate does not restart the same stream.

use rand::Rng;
use rand::SeedableRng;
use rand_chacha::ChaCha12Rng;

/// Random draws for one generation attempt.
pub struct Stream {
    inner: ChaCha12Rng,
}

impl Stream {
    /// ChaCha12 stream keyed by `seed`.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            inner: ChaCha12Rng::seed_from_u64(seed),
        }
    }

    /// A uniform draw in `[0, 1)`.
    pub fn f64(&mut self) -> f64 {
        self.inner.r#gen::<f64>()
    }

    /// A uniform draw in `[lo, hi)`.
    ///
    /// Returns `lo` when `hi <= lo`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        if !(hi > lo) {
            return lo;
        }
        lo + (hi - lo) * self.f64()
    }

    /// An integer in `0..n`. Returns 0 when `n` is 0.
    pub fn index(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.inner.gen_range(0..n)
        }
    }

    /// True half the time.
    pub fn coin(&mut self) -> bool {
        self.f64() < 0.5
    }
}

/// Seed for attempt `attempt`. Attempt 0 is `seed` itself.
#[must_use]
pub fn mix_attempt(seed: u64, attempt: u32) -> u64 {
    if attempt == 0 {
        return seed;
    }
    let mixed = seed.wrapping_add((attempt as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    splitmix64(mixed)
}

fn splitmix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// FNV-1a 64-bit fingerprint of UTF-8 bytes.
///
/// Used to lock generated JSON in tests. Not a cryptographic hash.
#[must_use]
pub fn fingerprint(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempt_zero_is_the_spec_seed() {
        assert_eq!(mix_attempt(0x1234, 0), 0x1234);
        assert_ne!(mix_attempt(0x1234, 1), 0x1234);
        assert_ne!(mix_attempt(0x1234, 1), mix_attempt(0x1234, 2));
    }

    #[test]
    fn stream_is_repeatable() {
        let mut a = Stream::new(9);
        let mut b = Stream::new(9);
        for _ in 0..8 {
            assert_eq!(a.f64(), b.f64());
        }
    }
}
