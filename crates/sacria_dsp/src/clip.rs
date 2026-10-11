//! Symmetric soft clipper. Only odd harmonics.

use crate::util::db_to_lin;

/// Linear below `threshold_db`, tanh-shaped above it, never louder than
/// `threshold_db + range_db`. `slope` is the gain just above the threshold:
/// 1 = smooth onset, < 1 = a hard corner. `hardness` picks the curve:
/// 0 = tanh, p > 0 = u / (1 + u^p)^(1/p) (bigger p = harder knee).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SoftClipParams {
    pub enabled: bool,
    pub threshold_db: f32,
    pub range_db: f32,
    #[cfg_attr(feature = "serde", serde(default = "one"))]
    pub slope: f32,
    #[cfg_attr(feature = "serde", serde(default))]
    pub hardness: f32,
}

#[cfg(feature = "serde")]
fn one() -> f32 {
    1.0
}

impl Default for SoftClipParams {
    fn default() -> Self {
        Self { enabled: false, threshold_db: 0.0, range_db: 3.0, slope: 1.0, hardness: 0.0 }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SoftClip {
    enabled: bool,
    t: f32,
    k: f32,
    /// slope / k
    s: f32,
    /// 0 = tanh, else the knee exponent of u / (1 + u^p)^(1/p)
    p: f32,
}

impl SoftClip {
    pub fn set(&mut self, p: &SoftClipParams) {
        self.enabled = p.enabled;
        self.t = db_to_lin(p.threshold_db);
        self.k = self.t * (db_to_lin(p.range_db.max(0.01)) - 1.0);
        self.s = p.slope.clamp(0.0, 1.0) / self.k;
        self.p = p.hardness.max(0.0);
    }

    #[inline]
    pub fn tick(&self, x: f32) -> f32 {
        let a = x.abs();
        if !self.enabled || a <= self.t {
            x
        } else {
            let u = (a - self.t) * self.s;
            let g = if self.p == 0.0 { u.tanh() } else { u / (1.0 + u.powf(self.p)).powf(1.0 / self.p) };
            (self.t + self.k * g).copysign(x)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_below_and_bounded_above() {
        let mut c = SoftClip::default();
        c.set(&SoftClipParams { enabled: true, threshold_db: -6.0, range_db: 3.0, slope: 1.0, hardness: 0.0 });
        assert_eq!(c.tick(0.3), 0.3);
        assert_eq!(c.tick(-0.3), -0.3);
        let ceil = db_to_lin(-3.0) * (1.0 + 1e-6);
        assert!(c.tick(100.0) <= ceil && c.tick(100.0) > 0.99 * ceil);
        assert!(c.tick(-100.0) >= -ceil);
        // continuous at the threshold
        let t = db_to_lin(-6.0);
        assert!((c.tick(t * 1.0001) - t * 1.0001).abs() < 1e-4);
    }
}
