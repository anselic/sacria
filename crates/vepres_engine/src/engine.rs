use crate::dynamics::{DynBank, LANES};
use sacria_dsp::biquad::{Biquad, Coeffs};
use sacria_dsp::clip::SoftClip;
use sacria_dsp::crossover::{Crossover3, OnePoleLp};
use sacria_dsp::limiter::Limiter;
use sacria_dsp::oversample::Oversampler;
use sacria_dsp::util::db_to_lin;
use crate::params::{ChannelParams, EqKind, Params, NUM_BANDS, NUM_EQ};

pub const MAX_LOOKAHEAD_MS: f32 = 20.0;

/// Dynamics lanes: 0..3 = mid low/mid/high, 3..6 = side low/mid/high.
const MID: usize = 0;
const SIDE: usize = NUM_BANDS;

/// One of M or S: EQ -> soft clip -> crossover. The detectors can listen
/// through a second crossover with other corners (`det_xo`).
struct Channel {
    eq: [Biquad; NUM_EQ],
    eq_active: usize,
    clip: SoftClip,
    os: Oversampler,
    xo: Crossover3,
    det_xo: Crossover3,
    separate_det: bool,
    /// Lowpass whose complement is the low-band step trigger highpass.
    step_lp: OnePoleLp,
    /// Optional lowpass after the trigger highpass.
    step_lp2: Option<OnePoleLp>,
}

impl Channel {
    fn new() -> Self {
        Self {
            eq: [Biquad::default(); NUM_EQ],
            eq_active: 0,
            clip: SoftClip::default(),
            os: Oversampler::default(),
            xo: Crossover3::default(),
            det_xo: Crossover3::default(),
            separate_det: false,
            step_lp: OnePoleLp::default(),
            step_lp2: None,
        }
    }

    fn set(&mut self, p: &ChannelParams, xo: [f32; 2], det: [f32; 2], os: u32, sr: f32) {
        // active filters first, so identity slots are skipped in `split`
        let mut n = 0;
        for band in p.eq.iter().filter(|b| b.kind != EqKind::Off && b.gain_db != 0.0) {
            let c = Coeffs::from_band(band, sr);
            if self.eq[n].c != c {
                self.eq[n].c = c;
            }
            n += 1;
        }
        if n != self.eq_active {
            self.eq.iter_mut().for_each(Biquad::reset);
            self.eq_active = n;
        }
        self.clip.set(&p.clip);
        self.os.set_factor(if p.clip.enabled { os } else { 1 });
        self.xo.set(xo[0], xo[1], sr);
        self.det_xo.set(det[0], det[1], sr);
        if !self.separate_det && det != xo {
            self.det_xo.reset();
        }
        self.separate_det = det != xo;
        self.step_lp.set(p.low_step.hp_hz, sr);
        self.step_lp2 = if p.low_step.lp_hz > 0.0 {
            let mut lp = self.step_lp2.unwrap_or_default();
            lp.set(p.low_step.lp_hz, sr);
            Some(lp)
        } else {
            None
        };
    }

    fn reset(&mut self) {
        self.eq.iter_mut().for_each(Biquad::reset);
        self.os.reset();
        self.xo.reset();
        self.det_xo.reset();
        self.step_lp.reset();
        if let Some(lp) = self.step_lp2.as_mut() {
            lp.reset();
        }
    }

    /// Returns (audio bands, detector bands, step trigger sample).
    #[inline]
    fn split(&mut self, x: f32) -> ([f32; NUM_BANDS], [f32; NUM_BANDS], f32) {
        let mut y = x;
        for bq in &mut self.eq[..self.eq_active] {
            y = bq.tick(y);
        }
        let clip = self.clip;
        let y = self.os.process(y, |v| clip.tick(v));
        let audio = self.xo.tick(y);
        let det = if self.separate_det { self.det_xo.tick(y) } else { audio };
        let hp = y - self.step_lp.tick(y);
        let trig = match self.step_lp2.as_mut() {
            Some(lp) => lp.tick(hp),
            None => hp,
        };
        (audio, det, trig)
    }
}

pub struct Engine {
    sr: f32,
    params: Params,
    mid: Channel,
    side: Channel,
    dynamics: DynBank,
    /// Look-ahead delay of the band signals (same lane layout as `dynamics`).
    delay: Vec<[f32; LANES]>,
    limiter: Limiter,
    lookahead: usize,
    pos: usize,
    in_gain: f32,
    out_gain: f32,
}

impl Engine {
    /// Allocates all buffers. Call again (not `set_params`) when the sample rate changes.
    pub fn new(sample_rate: f32, params: Params) -> Self {
        let len = (MAX_LOOKAHEAD_MS * 1e-3 * sample_rate).ceil() as usize + 1;
        let mut e = Self {
            sr: sample_rate,
            params,
            mid: Channel::new(),
            side: Channel::new(),
            dynamics: DynBank::default(),
            delay: vec![[0.0; LANES]; len],
            limiter: Limiter::new(sample_rate),
            lookahead: 0,
            pos: 0,
            in_gain: 1.0,
            out_gain: 1.0,
        };
        e.set_params(&params);
        e
    }

    /// Updates coefficients without clearing filter state. Does not allocate.
    /// Changing `lookahead_ms` or the limiter look-ahead changes the latency.
    pub fn set_params(&mut self, p: &Params) {
        self.params = *p;
        let sr = self.sr;
        let xo = [p.lo_xover_hz, p.hi_xover_hz];
        let det = p.detector_xover_hz.unwrap_or(xo);
        self.mid.set(&p.mid, xo, det, p.clip_oversample, sr);
        self.side.set(&p.side, xo, det, p.clip_oversample, sr);
        for k in 0..NUM_BANDS {
            self.dynamics.set_lane(MID + k, &p.mid.bands[k], sr);
            self.dynamics.set_lane(SIDE + k, &p.side.bands[k], sr);
        }
        self.dynamics.set_step(0, &p.mid.low_step, sr);
        self.dynamics.set_step(1, &p.side.low_step, sr);
        self.dynamics.set_deadband(p.release_deadband_db, p.release_deadband_floor_db);
        self.limiter.set(&p.limiter, sr);
        let max = self.delay.len() - 1;
        self.lookahead = ((p.lookahead_ms.max(0.0) * 1e-3 * sr).round() as usize).min(max);
        self.in_gain = db_to_lin(p.input_gain_db);
        self.out_gain = db_to_lin(p.output_gain_db);
    }

    pub fn params(&self) -> &Params {
        &self.params
    }

    pub fn sample_rate(&self) -> f32 {
        self.sr
    }

    /// Total latency in samples. Report this to the host.
    pub fn latency_samples(&self) -> usize {
        self.lookahead + self.limiter.latency()
    }

    pub fn reset(&mut self) {
        self.mid.reset();
        self.side.reset();
        self.dynamics.reset();
        self.delay.fill([0.0; LANES]);
        self.limiter.reset();
        self.pos = 0;
    }

    /// Gain reduction per band in dB, `[mid, side]`, for metering.
    pub fn band_gain_reduction_db(&self) -> [[f32; NUM_BANDS]; 2] {
        let d = &self.dynamics;
        [
            std::array::from_fn(|k| d.gain_reduction(MID + k)),
            std::array::from_fn(|k| d.gain_reduction(SIDE + k)),
        ]
    }

    /// Limiter gain reduction in dB, for metering.
    pub fn limiter_gain_reduction_db(&self) -> f32 {
        20.0 * self.limiter.gain().log10()
    }

    /// Processes a stereo block in place. `left` and `right` must have the same length.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        debug_assert_eq!(left.len(), right.len());
        // turns on Flush-To-Zero/Denormals-Are-Zero for the scope, restoring the
        // old state on drop (scope end)
        let _ftz = FtzGuard::new();
        let len = self.delay.len();
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let w = self.pos; // write index
            // where to read from, wrapping around the delay buffer
            let rd = if w >= self.lookahead { w - self.lookahead }
                else { w + len - self.lookahead };
            // advance write index (wrapping)
            self.pos = if w + 1 == len { 0 } else { w + 1 };

            // split into M/S & apply input gain
            // `m` & `s` are (audio bands, detector bands, step trigger sample)
            // audio bands[f32; 3]: low/mid/high signal, what we hear
            // detector bands[f32; 3]: measurement l/m/h signal, each one tells
            //   loud that band is for the dynamics/compressors
            // step trigger sample(f32): a switch for extra reduction on the low band
            let m = self.mid.split(0.5 * (*l + *r) * self.in_gain);
            let s = self.side.split(0.5 * (*l - *r) * self.in_gain);

            // store the M/S bands (3 mid, 3 side) for the look-ahead delay
            // last 2 are for padding because the dynamics lanes are 8-wide SIMD lanes
            self.delay[w] = [m.0[0], m.0[1], m.0[2], s.0[0], s.0[1], s.0[2], 0.0, 0.0];
            // pull out the audio bands from `self.lookahead` samples ago
            let d = self.delay[rd];
            // run the dynamics/compressors for this one sample, feed 8 detector
            // samples and gets 8 linear gains back
            let g = self.dynamics.tick(&[
                m.1[0], // mid  low  detector
                m.1[1], // mid  mid  detector
                m.1[2], // mid  high detector
                s.1[0], // side low  detector
                s.1[1], // side mid  detector
                s.1[2], // side high detector
                m.2,    // mid  step trigger
                s.2,    // side step trigger
            ]);
            // `self.dynamics` is `DynBank` and holds 8 compressors as SIMD lanes,
            // so all 8 are computed at once with the same instruction!

            // apply the gains to the delayed audio bands and sum back to M/S
            let m = d[MID] * g[MID] + d[MID + 1] * g[MID + 1] + d[MID + 2] * g[MID + 2];
            let s = d[SIDE] * g[SIDE] + d[SIDE + 1] * g[SIDE + 1] + d[SIDE + 2] * g[SIDE + 2];
            let (ol, or) = self.limiter.tick(m + s, m - s);

            // goodbye!
            *l = ol * self.out_gain;
            *r = or * self.out_gain;
        }
    }
}

/// Sets flush-to-zero / denormals-are-zero for the scope and restores the old
/// state on drop. The filters decay into denormals in silence, which is very slow on x86.
struct FtzGuard {
    #[cfg(target_arch = "x86_64")]
    old: u32,
}

impl FtzGuard {
    #[inline]
    #[allow(deprecated)]
    fn new() -> Self {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
            let old = _mm_getcsr();
            _mm_setcsr(old | 0x8040); // FTZ | DAZ
            Self { old }
        }
        #[cfg(not(target_arch = "x86_64"))]
        Self {}
    }
}

impl Drop for FtzGuard {
    #[inline]
    #[allow(deprecated)]
    fn drop(&mut self) {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            std::arch::x86_64::_mm_setcsr(self.old);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    const SR: f32 = 48000.0;

    /// Steady-state M or S gain (dB) for a sine.
    fn sine_gain(freq: f32, level_db: f32, side: bool) -> f32 {
        let mut e = Engine::new(SR, Params::up_and_wide());
        let n = (1.5 * SR) as usize;
        let a = db_to_lin(level_db);
        let x: Vec<f32> = (0..n).map(|i| a * (TAU * freq * i as f32 / SR).sin()).collect();
        let mut l = x.clone();
        let mut r: Vec<f32> = if side { x.iter().map(|v| -v).collect() } else { x.clone() };
        e.process(&mut l, &mut r);
        let lat = e.latency_samples();
        let tail = (0.5 * SR) as usize;
        let (mut pi, mut po) = (0.0f64, 0.0f64);
        for i in n - tail..n {
            let o = if side { 0.5 * (l[i] - r[i]) } else { 0.5 * (l[i] + r[i]) };
            po += (o as f64).powi(2);
            pi += (x[i - lat] as f64).powi(2);
        }
        10.0 * (po / pi).log10() as f32
    }

    #[test]
    fn low_level_response() {
        // expected gains at -40 dBFS (dB)
        let mid = [(60.0, 5.37), (250.0, 5.07), (1000.0, 2.81), (4000.0, 2.59), (12000.0, 2.73)];
        let side = [(60.0, 3.21), (250.0, 3.41), (1000.0, 4.56), (4000.0, 7.86), (12000.0, 9.72)];
        for (is_side, table) in [(false, &mid), (true, &side)] {
            for &(f, want) in table.iter() {
                let got = sine_gain(f, -40.0, is_side);
                assert!((got - want).abs() < 0.05, "side={is_side} {f} Hz: got {got:.3} want {want}");
            }
        }
    }

    #[test]
    fn impulse_arrives_at_reported_latency() {
        let mut e = Engine::new(SR, Params::up_and_wide());
        let mut l = vec![0.0f32; 4096];
        l[100] = 0.01;
        let mut r = l.clone();
        e.process(&mut l, &mut r);
        let peak = l.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap().0;
        assert_eq!(peak, 100 + e.latency_samples());
    }

    #[test]
    fn loud_noise_is_limited_and_finite() {
        let mut e = Engine::new(SR, Params::up_and_wide());
        let mut seed = 7u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            ((seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0) * 4.0
        };
        let mut l: Vec<f32> = (0..96000).map(|_| noise()).collect();
        let mut r: Vec<f32> = (0..96000).map(|_| noise()).collect();
        e.process(&mut l, &mut r);
        let thr = db_to_lin(-1.0) * 1.0001;
        assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() <= thr));
    }

    #[test]
    fn silence_stays_silent() {
        let mut e = Engine::new(SR, Params::up_and_wide());
        let mut l = vec![0.0f32; 48000];
        let mut r = l.clone();
        e.process(&mut l, &mut r);
        assert!(l.iter().chain(&r).all(|&v| v == 0.0));
    }
}
