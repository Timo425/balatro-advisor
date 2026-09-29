//! Randomness for probabilistic effects (Lucky cards, Misprint, Bloodstone, …).

/// Source of the game's random rolls. The engine only asks yes/no chances and ranges.
pub trait Rolls {
    /// True with probability `p` (already multiplied by `probabilities.normal`).
    fn chance(&mut self, p: f64) -> bool;
    /// Uniform integer in `[min, max]` (Misprint).
    fn range(&mut self, min: i64, max: i64) -> i64;
    /// Uniform float in [0, 1).
    fn unit(&mut self) -> f64;
}

/// Every roll fails, Misprint rolls its minimum: the guaranteed floor.
pub struct Unlucky;

impl Rolls for Unlucky {
    fn chance(&mut self, p: f64) -> bool {
        p >= 1.0
    }
    fn range(&mut self, min: i64, _max: i64) -> i64 {
        min
    }
    fn unit(&mut self) -> f64 {
        0.0
    }
}

/// Every roll succeeds, Misprint rolls its maximum: the ceiling.
pub struct Lucky;

impl Rolls for Lucky {
    fn chance(&mut self, p: f64) -> bool {
        p > 0.0
    }
    fn range(&mut self, _min: i64, max: i64) -> i64 {
        max
    }
    fn unit(&mut self) -> f64 {
        0.0
    }
}

/// xoshiro256** seeded through splitmix64. Small, fast, reproducible.
#[derive(Debug, Clone)]
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut x = seed;
        let mut next = || {
            x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = x;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };
        Rng { s: [next(), next(), next(), next()] }
    }

    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// Uniform in `0..n`.
    pub fn below(&mut self, n: usize) -> usize {
        ((self.next_u64() >> 11) as f64 / (1u64 << 53) as f64 * n as f64) as usize
    }
}

impl Rolls for Rng {
    fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }
    fn range(&mut self, min: i64, max: i64) -> i64 {
        min + self.below((max - min + 1).max(1) as usize) as i64
    }
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}
