//! 3-band subtractive crossover with 1st-order lowpasses.
//!
//! low = LP1(x), rest = x - low, mid = LP2(rest), high = rest - mid.
//! The bands always sum back to x exactly (no phase rotation).

use std::f64::consts::PI;

/// 1st-order Butterworth lowpass (bilinear, prewarped), TDF-II.
#[derive(Clone, Copy, Debug, Default)]
pub struct OnePoleLp {
    b0: f32,
    a1: f32,
    s: f32,
}

impl OnePoleLp {
    pub fn set(&mut self, freq_hz: f32, sr: f32) {
        let k = (PI * (freq_hz as f64 / sr as f64).min(0.499)).tan();
        self.b0 = (k / (1.0 + k)) as f32;
        self.a1 = ((k - 1.0) / (k + 1.0)) as f32;
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.s;
        self.s = self.b0 * x - self.a1 * y;
        y
    }

    pub fn reset(&mut self) {
        self.s = 0.0;
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Crossover3 {
    lp1: OnePoleLp,
    lp2: OnePoleLp,
}

impl Crossover3 {
    pub fn set(&mut self, lo_hz: f32, hi_hz: f32, sr: f32) {
        self.lp1.set(lo_hz, sr);
        self.lp2.set(hi_hz, sr);
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> [f32; 3] {
        let lo = self.lp1.tick(x);
        let rest = x - lo;
        let mid = self.lp2.tick(rest);
        [lo, mid, rest - mid]
    }

    pub fn reset(&mut self) {
        self.lp1.reset();
        self.lp2.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_sum_to_input() {
        let mut xo = Crossover3::default();
        xo.set(454.0, 3150.0, 48000.0);
        let mut seed = 1u32;
        for _ in 0..48000 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let x = (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
            let b = xo.tick(x);
            assert!((b[0] + b[1] + b[2] - x).abs() < 1e-6);
        }
    }
}
