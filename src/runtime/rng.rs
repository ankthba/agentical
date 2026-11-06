//! A tiny, dependency-free deterministic RNG (SplitMix64).
//!
//! We need *some* real pseudo-random source so the record run genuinely samples
//! (rather than picking a constant). SplitMix64 is a well-known, high-quality
//! 64-bit generator that is trivial to implement and fully deterministic given a
//! seed — perfect for the "seed differently on replay" test: replay must reproduce
//! the recorded trace from the *journal*, not from the RNG, so we deliberately seed
//! the replay RNG differently and still expect byte-identical output.
//!
//! Reference: Steele, Lea & Flood, "Fast Splittable Pseudorandom Number
//! Generators" (OOPSLA 2014). Standard-library only.

/// SplitMix64 state. Cheap to copy; advancing mutates `state`.
#[derive(Clone, Copy, Debug)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Draw the next 64-bit value and advance the state.
    pub fn next_u64(&mut self) -> u64 {
        // golden-gamma increment
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A uniform float in `[0, 1)` (53-bit mantissa precision).
    pub fn next_f64(&mut self) -> f64 {
        // Take the top 53 bits — the exactly-representable range for f64.
        let bits = self.next_u64() >> 11;
        (bits as f64) * (1.0 / ((1u64 << 53) as f64))
    }

    /// Sample an index in `0..weights.len()` proportional to `weights`
    /// (a categorical / weighted choice). `weights` must be non-empty and sum > 0.
    pub fn weighted_index(&mut self, weights: &[f64]) -> usize {
        let total: f64 = weights.iter().sum();
        debug_assert!(total > 0.0, "weights must sum to a positive value");
        let mut target = self.next_f64() * total;
        for (i, w) in weights.iter().enumerate() {
            target -= *w;
            if target < 0.0 {
                return i;
            }
        }
        weights.len() - 1 // floating-point fallthrough guard
    }
}
