//! Small fast random generator (xorshift), for phase scatter and the like.

#[derive(Clone)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }
    /// Uniform in -1..1.
    #[inline]
    pub fn bipolar(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 * (2.0 / 16_777_216.0) - 1.0
    }
    /// Uniform in 0..1.
    #[inline]
    pub fn unit(&mut self) -> f32 {
        0.5 * self.bipolar() + 0.5
    }
}
