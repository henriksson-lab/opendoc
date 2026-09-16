//! A reproducible PRNG, in the shape the rest of this repository already uses.
//!
//! Every corpus entry is a pure function of a seed, so a failure is replayable
//! from the seed the panic prints and nothing has to be checked in.

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(6364136223846793005).wrapping_add(1))
    }

    /// Named `step` rather than `next` so it cannot be mistaken for
    /// `Iterator::next`; the other fuzz generators in this repository are
    /// private modules and do not have that problem.
    pub fn step(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }

    pub fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.step() % bound as u64) as usize
        }
    }

    pub fn byte(&mut self) -> u8 {
        (self.step() & 0xff) as u8
    }
}
