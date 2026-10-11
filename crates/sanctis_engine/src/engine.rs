use crate::comp::BandComp;
use crate::fir::FirSplit;
use crate::limiter::Limiter;
use crate::params::{Detect, Params, Shape, NUM_BANDS};
#[cfg(test)]
use crate::params::OutputMode;
use sacria_dsp::oversample::Oversampler;
use sacria_dsp::fastmath;
use sacria_dsp::util::db_to_lin;

/// Shaper `sin(d*x)/d`. `Sine` folds back above `|x| = pi / (2d)`; `Soft` stays
/// at the top of the sine there. d = 0 is a wire.
#[inline]
fn shape(x: f32, d: f32, inv_d: f32, soft: bool) -> f32 {
    if d <= 0.0 {
        return x;
    }
    let u = d * x;
    let u = if soft { u.clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2) } else { u };
    fastmath::sin(u) * inv_d
}

pub struct Engine {
    sr: f32,
    params: Params,
    split: [FirSplit; 2],
    comps: [BandComp; NUM_BANDS],
    /// Band audio delay that lines up with the compressor gain: [band][L, R].
    delay: Vec<[[f32; 2]; NUM_BANDS]>,
    pos: usize,
    limiter: Limiter,
    in_gain: f32,
    out_gain: f32,
    drive: [f32; NUM_BANDS],
    inv_drive: [f32; NUM_BANDS],
    mid_gain: [f32; NUM_BANDS],
    side_gain: [f32; NUM_BANDS],
    soft: [bool; NUM_BANDS],
    stereo_detect: [bool; NUM_BANDS],
    /// One per shaped signal: [band][L, R].
    os: [[Oversampler; 2]; NUM_BANDS],
}

impl Engine {
    /// Allocates all buffers. Call again (not `set_params`) when the sample rate
    /// or the split length changes.
    pub fn new(sample_rate: f32, params: Params) -> Self {
        let comp = BandComp::new(sample_rate);
        let mut e = Self {
            sr: sample_rate,
            params,
            split: [FirSplit::new(&params.split, sample_rate), FirSplit::new(&params.split, sample_rate)],
            comps: [comp; NUM_BANDS],
            delay: vec![[[0.0; 2]; NUM_BANDS]; comp.latency().max(1)],
            pos: 0,
            limiter: Limiter::new(sample_rate),
            in_gain: 1.0,
            out_gain: 1.0,
            drive: [0.0; NUM_BANDS],
            inv_drive: [0.0; NUM_BANDS],
            mid_gain: [1.0; NUM_BANDS],
            side_gain: [1.0; NUM_BANDS],
            soft: [false; NUM_BANDS],
            stereo_detect: [false; NUM_BANDS],
            os: std::array::from_fn(|_| std::array::from_fn(|_| Oversampler::default())),
        };
        e.apply(&params, false);
        e
    }

    /// Does not allocate. A change of the split corners redesigns the FIR
    /// filters, which takes milliseconds: do that off the audio thread if you can.
    pub fn set_params(&mut self, p: &Params) {
        let redesign = p.split.lo != self.params.split.lo || p.split.hi != self.params.split.hi;
        self.apply(p, redesign);
        self.params = *p;
    }

    fn apply(&mut self, p: &Params, redesign: bool) {
        if redesign {
            for s in &mut self.split {
                s.set(&p.split, self.sr);
            }
        }
        for (k, b) in p.bands.iter().enumerate() {
            self.comps[k].set(b, self.sr);
            self.drive[k] = b.drive.max(0.0);
            self.inv_drive[k] = if b.drive > 0.0 { 1.0 / b.drive } else { 0.0 };
            self.mid_gain[k] = db_to_lin(b.mid_gain_db);
            self.side_gain[k] = db_to_lin(b.side_gain_db);
            self.soft[k] = b.shape == Shape::Soft;
            self.stereo_detect[k] = b.detect == Detect::Stereo;
            for os in &mut self.os[k] {
                os.set_factor(if p.shaper_oversample >= 2 { 2 } else { 1 });
            }
        }
        self.limiter.set(&p.limiter, self.sr);
        self.in_gain = db_to_lin(p.input_gain_db);
        self.out_gain = db_to_lin(p.output_gain_db);
    }

    pub fn params(&self) -> &Params {
        &self.params
    }

    pub fn sample_rate(&self) -> f32 {
        self.sr
    }

    /// Total delay in samples. Depends on the sample rate, the split length and
    /// the limiter look-ahead; the enable switches do not change it.
    pub fn latency_samples(&self) -> usize {
        self.split[0].latency() + self.delay.len() + self.limiter.latency()
    }

    pub fn reset(&mut self) {
        self.split.iter_mut().for_each(FirSplit::reset);
        self.comps.iter_mut().for_each(BandComp::reset);
        self.delay.fill([[0.0; 2]; NUM_BANDS]);
        self.pos = 0;
        self.os.iter_mut().flatten().for_each(Oversampler::reset);
        self.limiter.reset();
    }

    /// Gain reduction of the low, mid and high band (dB, <= 0), for metering.
    /// Zero while Character is off: the compressors still listen, but nothing is applied.
    pub fn band_gain_reduction_db(&self) -> [f32; NUM_BANDS] {
        if !self.params.character_enabled {
            return [0.0; NUM_BANDS];
        }
        [self.comps[0].gain_reduction_db(), self.comps[1].gain_reduction_db(), self.comps[2].gain_reduction_db()]
    }

    /// Signal level of the low, mid and high band (dBFS, fast meter), for displays.
    /// Same scale as the thresholds: a steady sine reads its peak.
    pub fn band_level_db(&self) -> [f32; NUM_BANDS] {
        [self.comps[0].signal_level_db(), self.comps[1].signal_level_db(), self.comps[2].signal_level_db()]
    }

    /// Gain reduction of the output stage (dB, <= 0), for metering.
    pub fn limiter_gain_reduction_db(&self) -> f32 {
        20.0 * self.limiter.gain().log10()
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let character = self.params.character_enabled;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let bl = self.split[0].tick(*l * self.in_gain);
            let br = self.split[1].tick(*r * self.in_gain);

            // the compressors run ahead of the delayed band audio
            let old = self.delay[self.pos];
            self.delay[self.pos] = [[bl[0], br[0]], [bl[1], br[1]], [bl[2], br[2]]];
            self.pos += 1;
            if self.pos == self.delay.len() {
                self.pos = 0;
            }

            let (mut m, mut s) = (0.0f32, 0.0f32);
            for k in 0..NUM_BANDS {
                let det = if self.stereo_detect[k] { bl[k].abs().max(br[k].abs()) } else { 0.5 * (bl[k] + br[k]) };
                let g = self.comps[k].tick(det);
                let [x, y] = old[k];
                if character {
                    let (d, inv_d, soft) = (self.drive[k], self.inv_drive[k], self.soft[k]);
                    let [os_l, os_r] = &mut self.os[k];
                    let x = os_l.process(g * x, |v| shape(v, d, inv_d, soft));
                    let y = os_r.process(g * y, |v| shape(v, d, inv_d, soft));
                    m += self.mid_gain[k] * 0.5 * (x + y);
                    s += self.side_gain[k] * 0.5 * (x - y);
                } else {
                    m += 0.5 * (x + y);
                    s += 0.5 * (x - y);
                }
            }

            let (ol, or) = self.limiter.tick(m + s, m - s);
            *l = ol * self.out_gain;
            *r = or * self.out_gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_sine(e: &mut Engine, freq: f32, level_db: f32, side: bool, n: usize) -> (f32, f32) {
        let a = db_to_lin(level_db);
        let mut l: Vec<f32> = (0..n).map(|i| a * (std::f32::consts::TAU * freq * i as f32 / 48000.0).sin()).collect();
        let mut r: Vec<f32> = if side { l.iter().map(|v| -v).collect() } else { l.clone() };
        for (cl, cr) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            e.process(cl, cr);
        }
        // fundamental amplitude over the last 4800 samples
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, v) in l.iter().enumerate().skip(n - 4800) {
            let ph = std::f64::consts::TAU * freq as f64 * i as f64 / 48000.0;
            re += *v as f64 * ph.cos();
            im += *v as f64 * ph.sin();
        }
        let amp = 2.0 * (re * re + im * im).sqrt() / 4800.0;
        let peak = l[n - 4800..].iter().chain(&r[n - 4800..]).fold(0.0f32, |p, v| p.max(v.abs()));
        (20.0 * (amp as f32 / a).log10(), peak)
    }

    #[test]
    fn silence_stays_silent() {
        let mut e = Engine::new(48000.0, Params::reference());
        let (mut l, mut r) = (vec![0.0f32; 4096], vec![0.0f32; 4096]);
        e.process(&mut l, &mut r);
        assert!(l.iter().chain(&r).all(|v| *v == 0.0));
    }

    #[test]
    fn impulse_arrives_at_reported_latency() {
        let mut p = Params::reference();
        p.character_enabled = false;
        p.limiter.mode = OutputMode::Off;
        let mut e = Engine::new(48000.0, p);
        let lat = e.latency_samples();
        assert_eq!(lat, 250 + 64 + 24 + 26);
        let (mut l, mut r) = (vec![0.0f32; 1024], vec![0.0f32; 1024]);
        l[0] = 0.5;
        r[0] = 0.5;
        e.process(&mut l, &mut r);
        // everything off = a pure delay
        for (i, v) in l.iter().enumerate() {
            let want = if i == lat { 0.5 } else { 0.0 };
            assert!((v - want).abs() < 1e-5, "{i}: {v}");
        }
    }

    #[test]
    fn low_level_gains_match_the_measurement() {
        // small-signal gain of the original at -60 dBFS: Character band gain + 1.894 dB of the limiter
        for (freq, side, want) in [(60.0, false, 4.72), (1000.0, false, 5.44), (12000.0, false, 2.92), (1000.0, true, 5.50), (12000.0, true, 5.95)] {
            let mut e = Engine::new(48000.0, Params::reference());
            let (g, _) = run_sine(&mut e, freq, -60.0, side, 48000);
            assert!((g - want).abs() < 0.1, "{freq} Hz side={side}: {g} dB, want {want}");
        }
    }

    #[test]
    fn mid_band_compresses_like_the_measurement() {
        // 1 kHz mid at -18 dBFS: +3.39 dB on the original (2 dB of gain reduction + a little saturation)
        let mut e = Engine::new(48000.0, Params::reference());
        let (g, _) = run_sine(&mut e, 1000.0, -18.0, false, 96000);
        assert!((g - 3.39).abs() < 0.15, "{g}");
        let gr = e.band_gain_reduction_db();
        assert!((gr[1] + 2.04).abs() < 0.15, "{gr:?}");
        assert!(gr[0] > -0.01 && gr[2] > -0.01, "{gr:?}");
    }

    #[test]
    fn side_only_input_is_not_compressed() {
        // the detectors listen to the mono sum
        let mut e = Engine::new(48000.0, Params::reference());
        run_sine(&mut e, 1000.0, -6.0, true, 48000);
        assert!(e.band_gain_reduction_db().iter().all(|g| *g > -0.01));
    }

    #[test]
    fn soft_shape_holds_hot_peaks_instead_of_folding() {
        assert!((shape(0.3, 2.0, 0.5, false) - shape(0.3, 2.0, 0.5, true)).abs() < 1e-7);
        // far past the top of the sine: SINE comes back down, SOFT stays at 1/d
        assert!(shape(1.5, 2.0, 0.5, false) < 0.1);
        assert!((shape(1.5, 2.0, 0.5, true) - 0.5).abs() < 1e-6);
        assert!((shape(-9.0, 2.0, 0.5, true) + 0.5).abs() < 1e-6);
    }

    #[test]
    fn stereo_detect_reacts_to_side_only_input() {
        let mut p = Params::reference();
        p.bands[1].detect = Detect::Stereo;
        let mut e = Engine::new(48000.0, p);
        run_sine(&mut e, 1000.0, -6.0, true, 48000);
        let gr = e.band_gain_reduction_db();
        assert!(gr[1] < -3.0, "{gr:?}");
        // mid-only input reads the same in both modes
        let (mut a, mut b) = (Engine::new(48000.0, Params::reference()), Engine::new(48000.0, p));
        run_sine(&mut a, 1000.0, -12.0, false, 48000);
        run_sine(&mut b, 1000.0, -12.0, false, 48000);
        assert!((a.band_gain_reduction_db()[1] - b.band_gain_reduction_db()[1]).abs() < 0.05);
    }

    #[test]
    fn oversampling_cuts_the_aliasing() {
        // 9 kHz side tone: H3 = 27 kHz folds to 21 kHz at 48 kHz. 2x keeps it out.
        let alias = |os: u32| {
            let mut p = Params::reference();
            p.shaper_oversample = os;
            p.limiter.mode = OutputMode::Off;
            let mut e = Engine::new(48000.0, p);
            let n = 48000;
            let a = db_to_lin(-6.0);
            let mut l: Vec<f32> = (0..n).map(|i| a * (std::f32::consts::TAU * 9000.0 * i as f32 / 48000.0).sin()).collect();
            let mut r: Vec<f32> = l.iter().map(|v| -v).collect();
            e.process(&mut l, &mut r);
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, v) in l.iter().enumerate().skip(n - 4800) {
                let ph = std::f64::consts::TAU * 21000.0 * i as f64 / 48000.0;
                re += *v as f64 * ph.cos();
                im += *v as f64 * ph.sin();
            }
            20.0 * (2.0 * (re * re + im * im).sqrt() / 4800.0).log10()
        };
        let (x1, x2) = (alias(1), alias(2));
        assert!(x1 > -45.0, "{x1}");
        assert!(x2 < x1 - 25.0, "1x {x1} dB, 2x {x2} dB");
    }

    #[test]
    fn loud_noise_stays_under_the_ceiling() {
        let mut e = Engine::new(48000.0, Params::reference());
        let mut seed = 3u32;
        let mut next = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
        };
        let mut l: Vec<f32> = (0..48000).map(|_| 2.0 * next()).collect();
        let mut r: Vec<f32> = (0..48000).map(|_| 2.0 * next()).collect();
        e.process(&mut l, &mut r);
        let ceil = db_to_lin(-0.1) * 1.000001;
        assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() <= ceil));
        assert!(e.limiter_gain_reduction_db() < 0.0);
    }
}
