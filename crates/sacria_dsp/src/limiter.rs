//! Stereo-linked look-ahead limiter with a 4x true-peak detector.
//!
//! Gain path: required gain -> sliding-window min (hold) -> release one-pole
//! (instant attack) -> box average over the look-ahead window. The audio is
//! delayed so the box average has fully ramped down before a peak arrives.
//! The true-peak FIR is only used for detection, never on the audio path.

use crate::util::{db_to_lin, time_coef};
use std::f64::consts::PI;
use wide::f32x8;

#[cfg(feature = "serde")]
fn yes() -> bool {
    true
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LimiterParams {
    pub enabled: bool,
    /// Level where gain reduction starts (dBFS).
    pub threshold_db: f32,
    /// Detect 4x-oversampled true peaks (true) or sample peaks (false).
    #[cfg_attr(feature = "serde", serde(default = "yes"))]
    pub true_peak: bool,
    /// Hard safety clamp after the limiter (sample peak, dBFS).
    pub ceiling_db: f32,
    pub release_ms: f32,
    /// Look-ahead (= attack time) of the limiter.
    pub lookahead_ms: f32,
}

pub const MAX_LIMITER_LOOKAHEAD_MS: f32 = 5.0;

const OS: usize = 4;
const TPP: usize = 8; // taps per phase
/// Audio samples that can influence one true-peak estimate.
const TP_SPREAD: usize = TPP;

pub struct Limiter {
    enabled: bool,
    true_peak: bool,
    thr: f32,
    ceil: f32,
    rel: f32,
    /// Box length (= look-ahead in samples).
    w: usize,
    /// Hold length.
    h: usize,
    /// Audio delay.
    delay: usize,
    /// FIR taps by delay j. Lanes: L phase 1..3, R phase 1..3, L phase 0, R phase 0.
    taps: [f32x8; TPP],
    /// Input history ring, one row per delay, lanes laid out like `taps`.
    rows: [f32x8; TPP],
    row_pos: usize,
    dq: Vec<(u64, f32)>,
    dq_head: usize,
    dq_len: usize,
    n: u64,
    env: f32,
    boxbuf: Vec<f32>,
    box_pos: usize,
    box_sum: f64,
    audio: Vec<[f32; 2]>,
    a_pos: usize,
    gain: f32,
}

impl Limiter {
    pub fn new(sr: f32) -> Self {
        let w_max = Self::window(MAX_LIMITER_LOOKAHEAD_MS, sr);
        let mut s = Self {
            enabled: false,
            true_peak: true,
            thr: 1.0,
            ceil: 1.0,
            rel: 0.0,
            w: 1,
            h: 1 + TP_SPREAD,
            delay: TP_SPREAD,
            taps: lane_taps(),
            rows: [f32x8::ZERO; TPP],
            row_pos: 0,
            dq: vec![(0, 1.0); w_max + TP_SPREAD + 1],
            dq_head: 0,
            dq_len: 0,
            n: 0,
            env: 1.0,
            boxbuf: vec![1.0; w_max],
            box_pos: 0,
            box_sum: 0.0,
            audio: vec![[0.0; 2]; w_max + TP_SPREAD],
            a_pos: 0,
            gain: 1.0,
        };
        s.reset();
        s
    }

    fn window(ms: f32, sr: f32) -> usize {
        ((ms.clamp(0.0, MAX_LIMITER_LOOKAHEAD_MS) * 1e-3 * sr).round() as usize).max(1)
    }

    /// Does not allocate. Changing the look-ahead resets the gain state.
    pub fn set(&mut self, p: &LimiterParams, sr: f32) {
        self.enabled = p.enabled;
        self.true_peak = p.true_peak;
        self.thr = db_to_lin(p.threshold_db);
        self.ceil = db_to_lin(p.ceiling_db);
        self.rel = time_coef(p.release_ms, sr);
        let w = Self::window(p.lookahead_ms, sr);
        if w != self.w {
            self.w = w;
            self.h = w + TP_SPREAD;
            self.delay = w + TP_SPREAD - 1;
            self.reset();
        }
    }

    pub fn latency(&self) -> usize {
        self.delay
    }

    /// Current limiter gain (linear), for metering.
    pub fn gain(&self) -> f32 {
        self.gain
    }

    pub fn reset(&mut self) {
        self.rows = [f32x8::ZERO; TPP];
        self.row_pos = 0;
        self.dq_head = 0;
        self.dq_len = 0;
        self.n = 0;
        self.env = 1.0;
        self.boxbuf.fill(1.0);
        self.box_pos = 0;
        self.box_sum = self.w as f64;
        self.audio.fill([0.0; 2]);
        self.a_pos = 0;
        self.gain = 1.0;
    }

    /// Pushes one sample per channel and returns the true-peak estimate (max of L, R).
    /// All 4 phases of both channels are computed at once in 8 SIMD lanes.
    #[inline]
    fn true_peak(&mut self, l: f32, r: f32) -> f32 {
        self.row_pos = (self.row_pos + 1) & (TPP - 1);
        self.rows[self.row_pos] = f32x8::new([l, l, l, r, r, r, l, r]);
        let mut y = f32x8::ZERO;
        for j in 0..TPP {
            y = y + self.taps[j] * self.rows[(self.row_pos + TPP - j) & (TPP - 1)];
        }
        y.abs().to_array().into_iter().fold(0.0, f32::max)
    }

    #[inline]
    pub fn tick(&mut self, l: f32, r: f32) -> (f32, f32) {
        let len = self.audio.len();
        self.audio[self.a_pos] = [l, r];
        let rd = if self.a_pos >= self.delay { self.a_pos - self.delay } else { self.a_pos + len - self.delay };
        let out = self.audio[rd];
        self.a_pos = if self.a_pos + 1 == len { 0 } else { self.a_pos + 1 };
        if !self.enabled {
            return (out[0], out[1]);
        }

        // sample-peak mode keeps the same delay and hold, so latency does not change
        let tp = if self.true_peak { self.true_peak(l, r) } else { l.abs().max(r.abs()) };
        let req = if tp > self.thr { self.thr / tp } else { 1.0 };

        // sliding-window minimum over the last `h` samples
        let cap = self.dq.len();
        let wrap = |i: usize| if i >= cap { i - cap } else { i };
        while self.dq_len > 0 && self.dq[wrap(self.dq_head + self.dq_len - 1)].1 >= req {
            self.dq_len -= 1;
        }
        self.dq[wrap(self.dq_head + self.dq_len)] = (self.n, req);
        self.dq_len += 1;
        while self.dq[self.dq_head].0 + self.h as u64 <= self.n {
            self.dq_head = wrap(self.dq_head + 1);
            self.dq_len -= 1;
        }
        let hold = self.dq[self.dq_head].1;
        self.n += 1;

        self.env = if hold < self.env { hold } else { hold + self.rel * (self.env - hold) };

        self.box_sum += (self.env - self.boxbuf[self.box_pos]) as f64;
        self.boxbuf[self.box_pos] = self.env;
        self.box_pos += 1;
        if self.box_pos == self.w {
            // re-sum once per window so float drift cannot build up
            self.box_pos = 0;
            self.box_sum = self.boxbuf[..self.w].iter().map(|&v| v as f64).sum();
        }
        let g = (self.box_sum / self.w as f64) as f32;
        self.gain = g;
        let c = self.ceil;
        ((out[0] * g).clamp(-c, c), (out[1] * g).clamp(-c, c))
    }
}

/// 32-tap windowed sinc for 4x interpolation, centered on tap 16, split into phases.
fn design_taps() -> [[f32; TPP]; OS] {
    let n = OS * TPP;
    let c = (n / 2) as f64;
    let mut taps = [[0.0f32; TPP]; OS];
    for p in 0..OS {
        let mut row = [0.0f64; TPP];
        for j in 0..TPP {
            let k = (OS * j + p) as f64;
            let x = (k - c) / OS as f64;
            let sinc = if x == 0.0 { 1.0 } else { (PI * x).sin() / (PI * x) };
            // Blackman over 2c+1 points
            let t = k / (2.0 * c);
            let win = 0.42 - 0.5 * (2.0 * PI * t).cos() + 0.08 * (4.0 * PI * t).cos();
            row[j] = sinc * win;
        }
        let sum: f64 = row.iter().sum();
        for j in 0..TPP {
            taps[p][j] = (row[j] / sum) as f32;
        }
    }
    taps
}

/// Rearranges the per-phase taps into the 8-lane layout used by `true_peak`.
fn lane_taps() -> [f32x8; TPP] {
    let t = design_taps();
    std::array::from_fn(|j| f32x8::new([t[1][j], t[2][j], t[3][j], t[1][j], t[2][j], t[3][j], t[0][j], t[0][j]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_exceeds_threshold_on_inter_sample_peaks() {
        let sr = 48000.0;
        let mut lim = Limiter::new(sr);
        let p = LimiterParams { enabled: true, threshold_db: -1.0, true_peak: true, ceiling_db: 0.0, release_ms: 50.0, lookahead_ms: 1.0 };
        lim.set(&p, sr);
        let thr = db_to_lin(-1.0);
        let mut worst = 0.0f32;
        // fs/4 sine at 45 deg phase: sample peaks are 0.707 of the true peak
        for i in 0..48000 {
            let t = i as f32;
            let amp = if (i / 4000) % 2 == 0 { 2.0 } else { 0.3 };
            let x = amp * (std::f32::consts::FRAC_PI_2 * t + std::f32::consts::FRAC_PI_4).sin();
            let (l, r) = lim.tick(x, -x);
            worst = worst.max(l.abs()).max(r.abs());
        }
        assert!(worst <= thr * 1.0001, "worst {worst} thr {thr}");
    }

    #[test]
    fn sample_peak_mode_holds_sample_peaks() {
        let sr = 48000.0;
        let mut lim = Limiter::new(sr);
        let p = LimiterParams { enabled: true, threshold_db: -1.0, true_peak: false, ceiling_db: 0.0, release_ms: 50.0, lookahead_ms: 1.0 };
        lim.set(&p, sr);
        let thr = db_to_lin(-1.0);
        let mut worst = 0.0f32;
        for i in 0..48000 {
            let amp = if (i / 4000) % 2 == 0 { 2.0 } else { 0.3 };
            let x = amp * (i as f32 * 0.05).sin();
            let (l, r) = lim.tick(x, 0.5 * x);
            worst = worst.max(l.abs()).max(r.abs());
        }
        assert!(worst <= thr * 1.0001 && worst > 0.95 * thr, "worst {worst} thr {thr}");
    }
}
