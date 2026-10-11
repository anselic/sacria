//! 2x / 4x oversampling with polyphase IIR half-band filters (two allpass
//! chains, the design from Laurent de Soras' HIIR). Low latency (a few samples
//! of group delay at low frequencies), flat magnitude, no allocation.
//! Used around the soft clipper so its harmonics do not alias.

use std::f64::consts::PI;

const MAX_COEFS: usize = 12;

/// Allpass coefficients of a half-band filter with `n` coefficients and a
/// transition band of `tb` (fraction of the sample rate, 0 < tb < 0.5).
fn design(n: usize, tb: f64) -> [f32; MAX_COEFS] {
    let order = (n * 2 + 1) as f64;
    // transition parameters
    let k = ((1.0 - 2.0 * tb) * PI / 4.0).tan().powi(2);
    let kksqrt = (1.0 - k * k).powf(0.25);
    let e = 0.5 * (1.0 - kksqrt) / (1.0 + kksqrt);
    let e4 = e.powi(4);
    let q = e * (1.0 + e4 * (2.0 + e4 * (15.0 + 150.0 * e4)));
    let mut out = [0.0f32; MAX_COEFS];
    for (index, c_out) in out.iter_mut().enumerate().take(n) {
        let c = (index + 1) as f64;
        let mut num = 0.0;
        let (mut i, mut j) = (0i32, 1.0);
        loop {
            let t = q.powi(i * (i + 1)) * ((i * 2 + 1) as f64 * c * PI / order).sin() * j;
            num += t;
            j = -j;
            i += 1;
            if t.abs() <= 1e-100 || i > 1000 {
                break;
            }
        }
        num *= q.powf(0.25);
        let mut den = 0.0;
        let (mut i, mut j) = (1i32, -1.0);
        loop {
            let t = q.powi(i * i) * ((i * 2) as f64 * c * PI / order).cos() * j;
            den += t;
            j = -j;
            i += 1;
            if t.abs() <= 1e-100 || i > 1000 {
                break;
            }
        }
        den += 0.5;
        let ww = num / den;
        let wwsq = ww * ww;
        let x = ((1.0 - wwsq * k) * (1.0 - wwsq / k)).sqrt() / (1.0 + wwsq);
        *c_out = ((1.0 - x) / (1.0 + x)) as f32;
    }
    out
}

/// One 2x stage: an upsampler and a downsampler with their own state.
#[derive(Clone, Copy, Debug)]
pub struct Halfband {
    n: usize,
    c: [f32; MAX_COEFS],
    // allpass states, [x_old, y_old] per coefficient
    up: [[f32; 2]; MAX_COEFS],
    down: [[f32; 2]; MAX_COEFS],
}

impl Halfband {
    pub fn new(n: usize, tb: f64) -> Self {
        let n = n.min(MAX_COEFS);
        Self { n, c: design(n, tb), up: [[0.0; 2]; MAX_COEFS], down: [[0.0; 2]; MAX_COEFS] }
    }

    pub fn reset(&mut self) {
        self.up = [[0.0; 2]; MAX_COEFS];
        self.down = [[0.0; 2]; MAX_COEFS];
    }

    /// Runs the allpass chain of one path (coefficients first, first+2, ...).
    #[inline]
    fn path(c: &[f32; MAX_COEFS], st: &mut [[f32; 2]; MAX_COEFS], n: usize, first: usize, x: f32) -> f32 {
        let mut v = x;
        let mut i = first;
        while i < n {
            let s = &mut st[i];
            let y = (v - s[1]) * c[i] + s[0];
            s[0] = v;
            s[1] = y;
            v = y;
            i += 2;
        }
        v
    }

    /// One input sample -> two output samples at the double rate.
    #[inline]
    pub fn upsample(&mut self, x: f32) -> [f32; 2] {
        [Self::path(&self.c, &mut self.up, self.n, 0, x), Self::path(&self.c, &mut self.up, self.n, 1, x)]
    }

    /// Two samples at the double rate -> one output sample.
    #[inline]
    pub fn downsample(&mut self, x: [f32; 2]) -> f32 {
        let a = Self::path(&self.c, &mut self.down, self.n, 0, x[1]);
        let b = Self::path(&self.c, &mut self.down, self.n, 1, x[0]);
        0.5 * (a + b)
    }
}

/// Runs a sample-by-sample function at 1x, 2x or 4x the sample rate.
#[derive(Clone, Copy, Debug)]
pub struct Oversampler {
    factor: u32,
    s1: Halfband,
    s2: [Halfband; 2],
}

impl Default for Oversampler {
    fn default() -> Self {
        // stage 1 does the hard work (passband to ~20 kHz at 48 kHz, > 90 dB stop);
        // stage 2 runs at 2x where the audio band is far from its Nyquist
        Self { factor: 1, s1: Halfband::new(12, 0.035), s2: [Halfband::new(6, 0.2); 2] }
    }
}

impl Oversampler {
    /// 1, 2 or 4 (anything else is treated as 1). Resets on change.
    pub fn set_factor(&mut self, f: u32) {
        let f = match f {
            2 | 4 => f,
            _ => 1,
        };
        if f != self.factor {
            self.factor = f;
            self.reset();
        }
    }

    pub fn factor(&self) -> u32 {
        self.factor
    }

    pub fn reset(&mut self) {
        self.s1.reset();
        self.s2.iter_mut().for_each(Halfband::reset);
    }

    #[inline]
    pub fn process(&mut self, x: f32, mut f: impl FnMut(f32) -> f32) -> f32 {
        match self.factor {
            2 => {
                let [a, b] = self.s1.upsample(x);
                self.s1.downsample([f(a), f(b)])
            }
            4 => {
                let [a, b] = self.s1.upsample(x);
                let [a0, a1] = self.s2[0].upsample(a);
                let [b0, b1] = self.s2[1].upsample(b);
                let a = self.s2[0].downsample([f(a0), f(a1)]);
                let b = self.s2[1].downsample([f(b0), f(b1)]);
                self.s1.downsample([a, b])
            }
            _ => f(x),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn tone_level(y: &[f32], freq: f32, sr: f32) -> f32 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, v) in y.iter().enumerate() {
            let p = (TAU * freq * i as f32 / sr) as f64;
            re += *v as f64 * p.cos();
            im += *v as f64 * p.sin();
        }
        (2.0 * (re * re + im * im).sqrt() / y.len() as f64) as f32
    }

    #[test]
    fn passband_is_flat_and_alias_is_gone() {
        let sr = 48000.0;
        for factor in [2, 4] {
            let mut os = Oversampler::default();
            os.set_factor(factor);
            for freq in [100.0, 1000.0, 10000.0, 18000.0] {
                os.reset();
                let x: Vec<f32> = (0..48000).map(|i| 0.5 * (TAU * freq * i as f32 / sr).sin()).collect();
                let y: Vec<f32> = x.iter().map(|&v| os.process(v, |s| s)).collect();
                let g = 20.0 * (tone_level(&y[4800..], freq, sr) / 0.5).log10();
                assert!(g.abs() < 0.05, "{factor}x at {freq} Hz: {g} dB");
            }
        }
        // a hard-clipped 9 kHz tone: its 5th harmonic (45 kHz) folds to 3 kHz at 1x
        let freq = 9000.0;
        let clip = |s: f32| s.clamp(-0.3, 0.3);
        let alias = |factor| {
            let mut os = Oversampler::default();
            os.set_factor(factor);
            let y: Vec<f32> = (0..48000).map(|i| os.process((TAU * freq * i as f32 / sr).sin(), clip)).collect();
            20.0 * tone_level(&y[4800..], 3000.0, sr).log10()
        };
        let (a1, a4) = (alias(1), alias(4));
        // a hard clip has endless harmonics, so some fold even at 4x; ~29 dB less here
        assert!(a4 < a1 - 25.0, "alias 1x {a1} dB, 4x {a4} dB");
    }
}
