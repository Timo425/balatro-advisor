//! Randomness for probabilistic effects (Lucky cards, Misprint, Bloodstone, …).

/// Source of the game's random rolls. The engine only asks yes/no chances and ranges.
pub trait Rolls {
    /// True with probability `p` (already multiplied by `probabilities.normal`).
    fn chance(&mut self, p: f64) -> bool;
    /// Uniform integer in `[min, max]` (Misprint).
    fn range(&mut self, min: i64, max: i64) -> i64;
    /// Uniform float in [0, 1).
    fn unit(&mut self) -> f64;
    /// Which `k` of `n` cards a random pick takes (The Hook's: blind.lua `Blind:press_play`).
    fn pick(&mut self, n: usize, k: usize) -> Pick;
}

/// A random pick of cards (`Rolls::pick`): every set it could take, averaged (each as likely: an
/// estimate's, D15), the set the hand loses least by (every roll for you), or these (indices,
/// ascending)
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    Average,
    Best,
    These(Vec<usize>),
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
    /// a pick isn't a roll that fails: its average (D15; the worst set is `Outcome::sure`)
    fn pick(&mut self, _n: usize, _k: usize) -> Pick {
        Pick::Average
    }
}

/// Every roll fails (`Unlucky`), and a pick takes these cards (indices, ascending): a hand
/// scored with a pick already known (The Hook's, for each set it could take)
pub struct Given(pub Vec<usize>);

impl Rolls for Given {
    fn chance(&mut self, p: f64) -> bool {
        Unlucky.chance(p)
    }
    fn range(&mut self, min: i64, max: i64) -> i64 {
        Unlucky.range(min, max)
    }
    fn unit(&mut self) -> f64 {
        Unlucky.unit()
    }
    fn pick(&mut self, _n: usize, _k: usize) -> Pick {
        Pick::These(self.0.clone())
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
    fn pick(&mut self, _n: usize, _k: usize) -> Pick {
        Pick::Best
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
    /// every set averaged, the other rolls drawn (D15: an estimate's pace)
    fn pick(&mut self, _n: usize, _k: usize) -> Pick {
        Pick::Average
    }
}
