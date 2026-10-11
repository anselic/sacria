//! Linear-phase 3-band FIR split.
//!
//! low = LP_lo(x), high = x_delayed - LP_hi(x), mid = LP_hi(x) - low.
//! Both lowpasses are symmetric and share one delay line, so the three bands
//! are time-aligned and sum back to the delayed input exactly.
//! The long low filter runs at a reduced rate and is interpolated back.

use crate::params::{SplitFilter, SplitParams};
use std::f64::consts::PI;
use wide::f32x8;

/// Modified Bessel function I0 (power series).
fn bessel_i0(x: f64) -> f64 {
    let q = x * x / 4.0;
    let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
    loop {
        term *= q / (k * k);
        sum += term;
        if term < sum * 1e-17 {
            return sum;
        }
        k += 1.0;
    }
}

/// Number of taps (odd) for a split length at a sample rate.
pub fn taps_for(length_ms: f32, sr: f32) -> usize {
    ((length_ms as f64 * 1e-3 * sr as f64).round() as usize).max(3) | 1
}

/// Designs the lowpass into `out` (odd length, centre tap in the middle):
/// inverse DFT of `1 / (1 + (f/fc)^(2n))` on a dense grid, Kaiser window, DC gain 1.
pub fn design_lowpass(f: &SplitFilter, sr: f32, out: &mut [f32]) {
    let taps = out.len();
    assert!(taps % 2 == 1);
    let half = (taps / 2) as isize;
    // grid: about 5.9 Hz at any sample rate
    let ng = (8192.0 * (sr as f64 / 48000.0)).max(1024.0).round() as usize * 2 / 2;
    let nk = ng / 2;
    let (fc, n2) = ((f.freq_hz as f64).max(1.0), 2.0 * (f.order as f64).max(0.1));
    let target: Vec<f64> = (0..=nk)
        .map(|k| {
            let w = if k == 0 || k == nk { 1.0 } else { 2.0 };
            w / (1.0 + (k as f64 * sr as f64 / ng as f64 / fc).powf(n2))
        })
        .collect();
    let beta = (f.kaiser_beta as f64).max(0.0);
    let i0b = bessel_i0(beta);
    let mut h = vec![0.0f64; taps];
    let step = 2.0 * PI / ng as f64;
    for m in 0..=half {
        let mut acc = 0.0;
        for (k, t) in target.iter().enumerate() {
            // k*m mod ng keeps the cosine argument small and exact
            acc += t * (step * ((k * m as usize) % ng) as f64).cos();
        }
        let r = m as f64 / half as f64;
        let win = bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / i0b;
        let v = acc / ng as f64 * win;
        h[(half + m) as usize] = v;
        h[(half - m) as usize] = v;
    }
    let sum: f64 = h.iter().sum();
    for (o, v) in out.iter_mut().zip(&h) {
        *o = (v / sum) as f32;
    }
}

#[inline]
fn lanes(s: &[f32]) -> f32x8 {
    f32x8::new(<[f32; 8]>::try_from(s).unwrap())
}

/// Dot product with four accumulators, so the adds do not wait on each other.
#[inline]
fn dot(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    let mut acc = [f32x8::ZERO; 4];
    let (ca, cb) = (a.chunks_exact(32), b.chunks_exact(32));
    let (ra, rb) = (ca.remainder(), cb.remainder());
    for (x, y) in ca.zip(cb) {
        acc[0] += lanes(&x[0..8]) * lanes(&y[0..8]);
        acc[1] += lanes(&x[8..16]) * lanes(&y[8..16]);
        acc[2] += lanes(&x[16..24]) * lanes(&y[16..24]);
        acc[3] += lanes(&x[24..32]) * lanes(&y[24..32]);
    }
    let mut s = ((acc[0] + acc[1]) + (acc[2] + acc[3])).reduce_add();
    for (x, y) in ra.iter().zip(rb) {
        s += x * y;
    }
    s
}

/// Taps per phase of the low-band interpolator.
const IP: usize = 8;

/// Decimation factor of the low band for a sample rate (16 at 48 kHz).
/// The low lowpass leaves very little above a few hundred Hz, so its output is
/// only computed every `factor` samples and interpolated in between. The price:
/// its stopband leak folds back, about -70 dB below the input on pink noise,
/// and most of that cancels again when low and mid are summed.
fn low_factor(sr: f32) -> usize {
    ((sr / 3000.0).round() as usize).max(1)
}

/// Polyphase interpolator: Kaiser-windowed sinc of length IP*r + 1, centre IP*r/2.
/// `out[phase*IP + i]` weights the i-th newest decimated sample. Each phase sums to 1.
fn design_interp(r: usize, out: &mut [f32]) {
    let c = (IP * r / 2) as f64;
    let (beta, i0b) = (8.0, bessel_i0(8.0));
    for phase in 0..r {
        let mut h = [0.0f64; IP];
        let mut sum = 0.0;
        for (i, v) in h.iter_mut().enumerate() {
            let d = (phase + i * r) as f64 - c;
            let sinc = if d == 0.0 { 1.0 } else { (PI * d / r as f64).sin() / (PI * d / r as f64) };
            let q = d / c;
            *v = sinc * bessel_i0(beta * (1.0 - q * q).max(0.0).sqrt()) / i0b;
            sum += *v;
        }
        for (i, v) in h.iter().enumerate() {
            out[phase * IP + i] = (v / sum) as f32;
        }
    }
}

/// The split of one channel.
pub struct FirSplit {
    taps: usize,
    lo: Vec<f32>,
    hi: Vec<f32>,
    /// `hi` is only non-zero in `hi[hi_off..taps - hi_off]`.
    hi_off: usize,
    /// Low-band decimation factor and position inside the current block.
    r: usize,
    phase: usize,
    /// Interpolator taps, `r` phases of `IP`.
    interp: Vec<f32>,
    /// Last `IP` decimated low samples, newest first, written twice.
    dec: [f32; 2 * IP],
    dec_pos: usize,
    /// Input history, written twice so the last `span` samples are one slice.
    /// `span` = taps + the delay of the interpolator.
    hist: Vec<f32>,
    span: usize,
    pos: usize,
}

impl FirSplit {
    pub fn new(p: &SplitParams, sr: f32) -> Self {
        let taps = taps_for(p.length_ms, sr);
        let r = low_factor(sr);
        let span = taps + IP * r / 2;
        let mut s = Self {
            taps,
            lo: vec![0.0; taps],
            hi: vec![0.0; taps],
            hi_off: 0,
            r,
            phase: 0,
            interp: vec![0.0; r * IP],
            dec: [0.0; 2 * IP],
            dec_pos: 0,
            hist: vec![0.0; 2 * span],
            span,
            pos: 0,
        };
        design_interp(r, &mut s.interp);
        s.set(p, sr);
        s
    }

    /// Redesigns the filters. Slow (milliseconds): call it only when the split changes.
    /// The length is fixed at construction; `p.length_ms` is ignored here.
    pub fn set(&mut self, p: &SplitParams, sr: f32) {
        design_lowpass(&p.lo, sr, &mut self.lo);
        design_lowpass(&p.hi, sr, &mut self.hi);
        // skip the window tails of the short filter
        let peak = self.hi.iter().fold(0.0f32, |a, v| a.max(v.abs()));
        let first = self.hi.iter().position(|v| v.abs() > peak * 1e-6).unwrap_or(self.taps / 2);
        self.hi_off = first.min(self.taps / 2);
    }

    /// Delay of all three bands, in samples.
    pub fn latency(&self) -> usize {
        self.taps / 2 + IP * self.r / 2
    }

    pub fn reset(&mut self) {
        self.hist.fill(0.0);
        self.pos = 0;
        self.dec = [0.0; 2 * IP];
        self.dec_pos = 0;
        self.phase = 0;
    }

    /// Returns [low, mid, high], delayed by `latency()`.
    #[inline]
    pub fn tick(&mut self, x: f32) -> [f32; 3] {
        let (n, span) = (self.taps, self.span);
        self.hist[self.pos] = x;
        self.hist[self.pos + span] = x;
        self.pos += 1;
        if self.pos == span {
            self.pos = 0;
        }
        // oldest .. newest
        let w = &self.hist[self.pos..self.pos + span];

        // low band: the long filter only at the start of each block
        if self.phase == 0 {
            let v = dot(&self.lo, &w[span - n..]);
            self.dec_pos = if self.dec_pos == 0 { IP - 1 } else { self.dec_pos - 1 };
            self.dec[self.dec_pos] = v;
            self.dec[self.dec_pos + IP] = v;
        }
        let taps = &self.interp[self.phase * IP..(self.phase + 1) * IP];
        let newest_first = &self.dec[self.dec_pos..self.dec_pos + IP];
        let mut low = 0.0;
        for (a, b) in taps.iter().zip(newest_first) {
            low += a * b;
        }
        self.phase += 1;
        if self.phase == self.r {
            self.phase = 0;
        }

        // the short filter and the dry tap sit `latency()` samples back
        let o = self.hi_off;
        let lp_hi = dot(&self.hi[o..n - o], &w[o..n - o]);
        let delayed = w[n / 2];
        [low, lp_hi - low, delayed - lp_hi]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::Params;

    fn response(h: &[f32], f: f64, sr: f64) -> f64 {
        let half = (h.len() / 2) as f64;
        h.iter().enumerate().map(|(i, v)| *v as f64 * (2.0 * PI * f * (i as f64 - half) / sr).cos()).sum()
    }

    #[test]
    fn bands_sum_to_delayed_input() {
        let p = Params::reference().split;
        let mut s = FirSplit::new(&p, 48000.0);
        let lat = s.latency();
        let mut seed = 1u32;
        let xs: Vec<f32> = (0..4000)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
            })
            .collect();
        for (i, &x) in xs.iter().enumerate() {
            let b = s.tick(x);
            let want = if i >= lat { xs[i - lat] } else { 0.0 };
            assert!((b[0] + b[1] + b[2] - want).abs() < 1e-5);
        }
    }

    #[test]
    fn shapes_match_the_measurement() {
        // band weights measured on the original (see internal notes), +-0.05
        let p = Params::reference().split;
        let s = FirSplit::new(&p, 48000.0);
        assert_eq!(s.taps, 501);
        for (f, low, high) in [(60.0, 0.974, 0.0), (160.0, 0.50, 0.0), (250.0, 0.11, 0.0), (1000.0, 0.0, 0.02), (2413.0, 0.0, 0.46), (4000.0, 0.0, 0.87), (6000.0, 0.0, 1.0)] {
            let l = response(&s.lo, f, 48000.0);
            let h = 1.0 - response(&s.hi, f, 48000.0);
            assert!((l - low).abs() < 0.05, "low at {f}: {l}");
            assert!((h - high).abs() < 0.05, "high at {f}: {h}");
        }
    }

    #[test]
    fn decimated_low_band_matches_the_design() {
        // impulse response of the low band = the designed taps, delayed by the interpolator.
        // Not exact: what the lowpass leaves above the reduced Nyquist folds back
        // (about -70 dB below the input on music-like signals).
        let p = Params::reference().split;
        for start in [0usize, 5, 11] {
            let mut s = FirSplit::new(&p, 48000.0);
            let extra = s.latency() - s.taps / 2;
            for _ in 0..start {
                s.tick(0.0);
            }
            let mut err = 0.0f32;
            for i in 0..s.taps + extra + 40 {
                let low = s.tick(if i == 0 { 1.0 } else { 0.0 })[0];
                let want = if i >= extra && i - extra < s.taps { s.lo[i - extra] } else { 0.0 };
                err = err.max((low - want).abs());
            }
            let peak = s.lo.iter().fold(0.0f32, |a, v| a.max(*v));
            assert!(err < peak * 0.04, "start {start}: err {err}, peak {peak}");
        }
    }

    #[test]
    fn same_shape_at_96k() {
        let p = Params::reference().split;
        let (a, b) = (FirSplit::new(&p, 48000.0), FirSplit::new(&p, 96000.0));
        for f in [60.0, 160.0, 250.0, 2413.0, 6000.0] {
            assert!((response(&a.lo, f, 48000.0) - response(&b.lo, f, 96000.0)).abs() < 0.01);
            assert!((response(&a.hi, f, 48000.0) - response(&b.hi, f, 96000.0)).abs() < 0.01);
        }
    }
}
