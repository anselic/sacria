//! Two-stage, stereo-linked true-peak limiter.
//!
//! The envelope `u` (>= 1) lives in the level domain; the gain is `1 / u`.
//! Fast stage: every 4x true peak above 0 dBFS draws a triangle on the
//! envelope: a straight ramp up over the look-ahead, a straight ramp back over
//! the release ramp. Slow stage: a one-pole follower of the fast envelope with
//! separate attack and release. `u = 1 + max(fast, slow)`.
//! Output = delayed input * input_gain * ceiling / u, so nothing ever passes the ceiling.
//!
//! In clip mode the same delayed input is hard clipped instead:
//! output = clamp(input * input_gain, -1, 1) * ceiling.

use crate::params::{LimiterParams, OutputMode};
use sacria_dsp::util::db_to_lin;
use std::f64::consts::PI;
use wide::f32x8;

pub const MAX_RAMP_MS: f32 = 5.0;
/// Clip mode: how fast the metered reduction falls back (the audio is not affected).
const CLIP_METER_MS: f32 = 50.0;

const OS: usize = 4;
/// Taps per phase of the true-peak interpolator.
const TPP: usize = 12;
/// Samples the interpolator looks ahead.
const TP_DELAY: usize = TPP / 2;

/// Kaiser-windowed sinc, phases 1..OS-1 (phase 0 is the sample itself).
/// `taps[p][i]` weights `x[j - TPP/2 + 1 + i]` for the point `j + (p+1)/OS`.
fn design_taps() -> [[f32; TPP]; OS - 1] {
    fn i0(x: f64) -> f64 {
        let q = x * x / 4.0;
        let (mut s, mut t, mut k) = (1.0, 1.0, 1.0);
        while t > s * 1e-17 {
            t *= q / (k * k);
            s += t;
            k += 1.0;
        }
        s
    }
    let beta = 8.0;
    let mut taps = [[0.0f32; TPP]; OS - 1];
    for (p, row) in taps.iter_mut().enumerate() {
        let frac = (p + 1) as f64 / OS as f64;
        let mut sum = 0.0;
        let mut h = [0.0f64; TPP];
        for (i, v) in h.iter_mut().enumerate() {
            let d = i as f64 - (TPP / 2 - 1) as f64 - frac;
            let sinc = if d.abs() < 1e-12 { 1.0 } else { (PI * d).sin() / (PI * d) };
            let r = d / (TPP / 2) as f64;
            *v = sinc * i0(beta * (1.0 - r * r).max(0.0).sqrt()) / i0(beta);
            sum += *v;
        }
        for (o, v) in row.iter_mut().zip(h) {
            *o = (v / sum) as f32;
        }
    }
    taps
}

fn lane_taps() -> [f32x8; TPP] {
    let t = design_taps();
    std::array::from_fn(|i| f32x8::new([t[0][i], t[1][i], t[2][i], t[0][i], t[1][i], t[2][i], 0.0, 0.0]))
}

pub struct Limiter {
    mode: OutputMode,
    /// input gain, and input gain * ceiling
    g_in: f32,
    g_out: f32,
    ceil: f32,
    look: usize,
    rel: usize,
    ka: f32,
    kr: f32,
    k_meter: f32,
    /// Interpolator taps by window position (oldest .. newest).
    /// Lanes: L phase 1..3, R phase 1..3, two unused.
    taps: [f32x8; TPP],
    /// Last TPP input samples (after the input gain), laid out like `taps`, written twice.
    xh: [f32x8; 2 * TPP],
    x_pos: usize,
    /// Peak excess history `e = max(true peak - 1, 0)`, written twice. Window = rel + look - 1.
    eh: Vec<f32>,
    e_pos: usize,
    /// Triangle weights for the window, oldest .. newest.
    w: Vec<f32>,
    slow: f32,
    audio: Vec<[f32; 2]>,
    a_pos: usize,
    gain: f32,
}

impl Limiter {
    pub fn new(sr: f32) -> Self {
        let max = Self::ramp(MAX_RAMP_MS, sr);
        let mut s = Self {
            mode: OutputMode::Limit,
            g_in: 1.0,
            g_out: 1.0,
            ceil: 1.0,
            look: 1,
            rel: 1,
            ka: 1.0,
            kr: 1.0,
            k_meter: 1.0,
            taps: lane_taps(),
            xh: [f32x8::ZERO; 2 * TPP],
            x_pos: 0,
            eh: vec![0.0; 2 * (2 * max)],
            e_pos: 0,
            w: vec![0.0; 2 * max],
            slow: 0.0,
            audio: vec![[0.0; 2]; TP_DELAY + max],
            a_pos: 0,
            gain: 1.0,
        };
        s.set_ramps(1, 1);
        s
    }

    fn ramp(ms: f32, sr: f32) -> usize {
        ((ms.clamp(0.0, MAX_RAMP_MS) * 1e-3 * sr).round() as usize).max(1)
    }

    fn set_ramps(&mut self, look: usize, rel: usize) {
        self.look = look;
        self.rel = rel;
        // window index i holds e[m + i - (rel - 1)], m = the output sample
        for i in 0..self.window() {
            let o = i as isize - (rel as isize - 1);
            self.w[i] = if o >= 0 { 1.0 - o as f32 / look as f32 } else { 1.0 + o as f32 / rel as f32 };
        }
        self.reset();
    }

    fn window(&self) -> usize {
        self.rel + self.look - 1
    }

    /// Does not allocate. Changing a ramp length resets the state.
    pub fn set(&mut self, p: &LimiterParams, sr: f32) {
        self.mode = p.mode;
        self.g_in = db_to_lin(p.input_gain_db);
        self.ceil = db_to_lin(p.ceiling_db);
        self.g_out = self.g_in * self.ceil;
        let coef = |ms: f32| if ms <= 0.0 { 1.0 } else { (1.0 - (-1.0 / (ms as f64 * 1e-3 * sr as f64)).exp()) as f32 };
        self.ka = coef(p.slow_attack_ms);
        self.kr = coef(p.slow_release_ms);
        self.k_meter = coef(CLIP_METER_MS);
        let (look, rel) = (Self::ramp(p.lookahead_ms, sr), Self::ramp(p.release_ramp_ms, sr));
        if look != self.look || rel != self.rel {
            self.set_ramps(look, rel);
        }
    }

    pub fn latency(&self) -> usize {
        TP_DELAY + self.look - 1
    }

    /// Current gain reduction factor (linear, <= 1), for metering. In clip mode:
    /// how far the last peaks were over the ceiling.
    pub fn gain(&self) -> f32 {
        self.gain
    }

    pub fn reset(&mut self) {
        self.xh = [f32x8::ZERO; 2 * TPP];
        self.x_pos = 0;
        self.eh.fill(0.0);
        self.e_pos = 0;
        self.slow = 0.0;
        self.audio.fill([0.0; 2]);
        self.a_pos = 0;
        self.gain = 1.0;
    }

    /// Linked true peak of the newest complete interval, after the input gain.
    #[inline]
    fn true_peak(&mut self, l: f32, r: f32) -> f32 {
        let p = self.x_pos;
        let x = f32x8::new([l, l, l, r, r, r, 0.0, 0.0]);
        self.xh[p] = x;
        self.xh[p + TPP] = x;
        self.x_pos = (p + 1) % TPP;
        let w = &self.xh[self.x_pos..self.x_pos + TPP]; // oldest .. newest
        let mut acc = f32x8::ZERO;
        for (t, v) in self.taps.iter().zip(w) {
            acc += *t * *v;
        }
        // phase 0 = the samples themselves
        let s = w[TPP / 2 - 1].abs().to_array();
        acc.abs().to_array().into_iter().fold(s[0].max(s[3]), f32::max)
    }

    #[inline]
    pub fn tick(&mut self, l: f32, r: f32) -> (f32, f32) {
        let delay = self.latency();
        let out = self.audio[self.a_pos];
        self.audio[self.a_pos] = [l, r];
        self.a_pos += 1;
        if self.a_pos == delay {
            self.a_pos = 0;
        }
        match self.mode {
            OutputMode::Off => {
                self.gain = 1.0;
                return (out[0], out[1]);
            }
            OutputMode::Clip => {
                let (a, b) = (out[0] * self.g_in, out[1] * self.g_in);
                // meter: instant down, smooth back
                let g = 1.0 / a.abs().max(b.abs()).max(1.0);
                self.gain = g.min(self.gain + self.k_meter * (1.0 - self.gain));
                let c = self.ceil;
                return (a.clamp(-1.0, 1.0) * c, b.clamp(-1.0, 1.0) * c);
            }
            OutputMode::Limit => {}
        }

        let e = (self.true_peak(l * self.g_in, r * self.g_in) - 1.0).max(0.0);
        let n = self.window();
        self.eh[self.e_pos] = e;
        self.eh[self.e_pos + n] = e;
        self.e_pos += 1;
        if self.e_pos == n {
            self.e_pos = 0;
        }
        // fast stage: the highest triangle over this sample
        let win = &self.eh[self.e_pos..self.e_pos + n];
        let mut m8 = f32x8::ZERO;
        let (cw, ce) = (self.w[..n].chunks_exact(8), win.chunks_exact(8));
        let (rw, re) = (cw.remainder(), ce.remainder());
        for (a, b) in cw.zip(ce) {
            m8 = m8.fast_max(f32x8::new(<[f32; 8]>::try_from(a).unwrap()) * f32x8::new(<[f32; 8]>::try_from(b).unwrap()));
        }
        let mut fast = m8.to_array().into_iter().fold(0.0f32, f32::max);
        for (a, b) in rw.iter().zip(re) {
            fast = fast.max(a * b);
        }
        // slow stage follows the fast envelope
        self.slow += (if fast > self.slow { self.ka } else { self.kr }) * (fast - self.slow);
        let u = 1.0 + fast.max(self.slow);
        self.gain = 1.0 / u;
        let g = self.g_out * self.gain;
        let c = self.ceil;
        ((out[0] * g).clamp(-c, c), (out[1] * g).clamp(-c, c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::Params;

    fn lim() -> Limiter {
        let mut l = Limiter::new(48000.0);
        l.set(&Params::reference().limiter, 48000.0);
        l
    }

    #[test]
    fn ramp_lengths_and_latency_at_48k() {
        let l = lim();
        assert_eq!((l.look, l.rel), (21, 22));
        assert_eq!(l.latency(), 26);
    }

    #[test]
    fn quiet_signal_only_gets_the_gain() {
        let mut l = lim();
        let want = db_to_lin(1.994 - 0.1);
        let mut out = Vec::new();
        for i in 0..2000 {
            let x = 0.1 * (std::f32::consts::TAU * 997.0 * i as f32 / 48000.0).sin();
            out.push((x, l.tick(x, -x).0));
        }
        for i in 100..2000 {
            assert!((out[i].1 - out[i - 26].0 * want).abs() < 1e-6);
        }
    }

    #[test]
    fn never_passes_the_ceiling_and_hits_true_peaks() {
        let mut l = lim();
        let ceil = db_to_lin(-0.1);
        let mut seed = 7u32;
        let (mut peak, mut min_gain) = (0.0f32, 1.0f32);
        for _ in 0..96000 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let a = (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let b = (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
            let (ol, or) = l.tick(1.5 * a, 1.5 * b);
            peak = peak.max(ol.abs()).max(or.abs());
            min_gain = min_gain.min(l.gain());
        }
        assert!(peak <= ceil * 1.000001, "{peak}");
        assert!(min_gain < 0.6);
    }

    #[test]
    fn clip_mode_cuts_flat_at_the_ceiling() {
        let p = LimiterParams { mode: OutputMode::Clip, ..Params::reference().limiter };
        let mut l = Limiter::new(48000.0);
        l.set(&p, 48000.0);
        let (drive, ceil) = (db_to_lin(p.input_gain_db), db_to_lin(p.ceiling_db));
        let (mut flat, mut min_gain) = (0, 1.0f32);
        for i in 0..4800 {
            let x = 1.5 * (std::f32::consts::TAU * 997.0 * i as f32 / 48000.0).sin();
            let (y, _) = l.tick(x, 0.2 * x);
            assert!(y.abs() <= ceil * 1.000001);
            flat += (y.abs() > ceil * 0.999999) as usize;
            min_gain = min_gain.min(l.gain());
        }
        // most of a sine this hot is over the ceiling; the meter shows by how much
        assert!(flat > 2400, "{flat}");
        assert!((min_gain - 1.0 / (1.5 * drive)).abs() < 0.01, "{min_gain}");
        // below the ceiling: only drive and ceiling, same delay as the limiter
        let mut out = Vec::new();
        l.reset();
        for i in 0..2000 {
            let x = 0.1 * (std::f32::consts::TAU * 997.0 * i as f32 / 48000.0).sin();
            out.push((x, l.tick(x, x).0));
        }
        for i in 100..2000 {
            assert!((out[i].1 - out[i - 26].0 * drive * ceil).abs() < 1e-6);
        }
    }

    #[test]
    fn a_single_peak_draws_a_triangle() {
        // 21 samples down, 22 samples back, straight lines in the level domain
        let mut l = lim();
        let mut u = Vec::new();
        for i in 0..400 {
            let x = if i == 200 { 2.0 / db_to_lin(1.994) } else { 0.0 };
            l.tick(x, x);
            u.push(1.0 / l.gain());
        }
        let top = u.iter().cloned().fold(0.0f32, f32::max);
        let i_top = u.iter().position(|v| *v == top).unwrap();
        assert!((top - 2.0).abs() < 0.02, "{top}");
        // straight ramps: constant steps on both sides (the slow stage is tiny here)
        let up = u[i_top - 10] - u[i_top - 11];
        let down = u[i_top + 10] - u[i_top + 11];
        assert!((up - (top - 1.0) / 21.0).abs() < 2e-3, "{up}");
        assert!((down - (top - 1.0) / 22.0).abs() < 2e-3, "{down}");
        assert!(u[i_top - 25] < 1.001 && u[i_top + 60] < 1.01);
    }
}
