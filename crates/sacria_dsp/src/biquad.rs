//! RBJ-cookbook biquad, transposed direct form II.

use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EqKind {
    Off,
    Peaking,
    LowShelf,
    HighShelf,
}

/// One RBJ-cookbook biquad. `q` is the cookbook Q (also for shelves).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EqBand {
    pub kind: EqKind,
    pub freq_hz: f32,
    pub gain_db: f32,
    pub q: f32,
}

impl EqBand {
    pub const OFF: Self = Self { kind: EqKind::Off, freq_hz: 1000.0, gain_db: 0.0, q: 0.707 };
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coeffs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

impl Coeffs {
    pub const IDENTITY: Self = Self { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

    fn norm(b: [f64; 3], a: [f64; 3]) -> Self {
        Self {
            b0: (b[0] / a[0]) as f32,
            b1: (b[1] / a[0]) as f32,
            b2: (b[2] / a[0]) as f32,
            a1: (a[1] / a[0]) as f32,
            a2: (a[2] / a[0]) as f32,
        }
    }

    pub fn from_band(band: &EqBand, sr: f32) -> Self {
        let (sr, f, g, q) = (sr as f64, band.freq_hz as f64, band.gain_db as f64, band.q as f64);
        if band.kind == EqKind::Off || g == 0.0 {
            return Self::IDENTITY;
        }
        let a = 10f64.powf(g / 40.0);
        let w = 2.0 * PI * (f / sr).min(0.499);
        let (sn, c) = w.sin_cos();
        let al = sn / (2.0 * q.max(1e-3));
        let s = 2.0 * a.sqrt() * al;
        match band.kind {
            EqKind::Off => Self::IDENTITY,
            EqKind::Peaking => Self::norm(
                [1.0 + al * a, -2.0 * c, 1.0 - al * a],
                [1.0 + al / a, -2.0 * c, 1.0 - al / a],
            ),
            EqKind::HighShelf => Self::norm(
                [
                    a * ((a + 1.0) + (a - 1.0) * c + s),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
                    a * ((a + 1.0) + (a - 1.0) * c - s),
                ],
                [(a + 1.0) - (a - 1.0) * c + s, 2.0 * ((a - 1.0) - (a + 1.0) * c), (a + 1.0) - (a - 1.0) * c - s],
            ),
            EqKind::LowShelf => Self::norm(
                [
                    a * ((a + 1.0) - (a - 1.0) * c + s),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * c),
                    a * ((a + 1.0) - (a - 1.0) * c - s),
                ],
                [(a + 1.0) + (a - 1.0) * c + s, -2.0 * ((a - 1.0) + (a + 1.0) * c), (a + 1.0) + (a - 1.0) * c - s],
            ),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    pub c: Coeffs,
    s1: f32,
    s2: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        Self { c: Coeffs::IDENTITY, s1: 0.0, s2: 0.0 }
    }
}

impl Biquad {
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let c = &self.c;
        let y = c.b0 * x + self.s1;
        self.s1 = c.b1 * x - c.a1 * y + self.s2;
        self.s2 = c.b2 * x - c.a2 * y;
        y
    }

    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }
}
