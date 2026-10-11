//! Output spectrum for the response strip: mid and side, 4096-point FFT.
//!
//! The audio thread only writes samples into `SpectrumTap` (while the editor is
//! open). The FFT, the tilt and the smoothing all run on the GUI thread.

use atomic_float::AtomicF32;
use sacria_dsp::fft::{hann, Fft};
use std::sync::atomic::{AtomicUsize, Ordering};

pub const FFT_SIZE: usize = 4096;
const TAP_SIZE: usize = 2 * FFT_SIZE;
/// Points of the drawn curve, log-spaced from `F_MIN` to `F_MAX`.
pub const POINTS: usize = 220;
pub const F_MIN: f32 = 20.0;
pub const F_MAX: f32 = 20000.0;
/// Display tilt around 1 kHz, so a balanced mix reads about flat.
const TILT_DB_PER_OCT: f32 = 4.5;
pub const FLOOR_DB: f32 = -120.0;
/// Seconds between two FFTs.
const INTERVAL: f64 = 1.0 / 30.0;

/// Last `TAP_SIZE` output samples (mid, side). Written by the audio thread,
/// read by the GUI. A torn read only costs one slightly wrong frame.
pub struct SpectrumTap {
    mid: Vec<AtomicF32>,
    side: Vec<AtomicF32>,
    pos: AtomicUsize,
    pub sample_rate: AtomicF32,
}

impl Default for SpectrumTap {
    fn default() -> Self {
        Self {
            mid: (0..TAP_SIZE).map(|_| AtomicF32::new(0.0)).collect(),
            side: (0..TAP_SIZE).map(|_| AtomicF32::new(0.0)).collect(),
            pos: AtomicUsize::new(0),
            sample_rate: AtomicF32::new(48000.0),
        }
    }
}

impl SpectrumTap {
    /// Audio thread: append one block of left/right output samples.
    #[inline]
    pub fn write(&self, left: &[f32], right: &[f32]) {
        let mut p = self.pos.load(Ordering::Relaxed);
        for (l, r) in left.iter().zip(right) {
            self.mid[p].store(0.5 * (l + r), Ordering::Relaxed);
            self.side[p].store(0.5 * (l - r), Ordering::Relaxed);
            p = (p + 1) & (TAP_SIZE - 1);
        }
        self.pos.store(p, Ordering::Relaxed);
    }

    fn read(&self, mid: &mut [f32], side: &mut [f32]) {
        let end = self.pos.load(Ordering::Relaxed);
        let start = (end + TAP_SIZE - FFT_SIZE) & (TAP_SIZE - 1);
        for i in 0..FFT_SIZE {
            let p = (start + i) & (TAP_SIZE - 1);
            mid[i] = self.mid[p].load(Ordering::Relaxed);
            side[i] = self.side[p].load(Ordering::Relaxed);
        }
    }
}

/// Frequency of curve point `i`.
pub fn point_freq(i: usize) -> f32 {
    F_MIN * (F_MAX / F_MIN).powf(i as f32 / (POINTS - 1) as f32)
}

/// Turns FFT magnitudes (dB per bin, a full-scale sine = 0 dB) into the tilted curve points.
fn to_points(bins_db: &[f32], sample_rate: f32, out: &mut [f32]) {
    let hz_per_bin = sample_rate / FFT_SIZE as f32;
    let half_step = (F_MAX / F_MIN).powf(0.5 / (POINTS - 1) as f32);
    for (i, o) in out.iter_mut().enumerate() {
        let f = point_freq(i);
        let (lo, hi) = (f / half_step / hz_per_bin, f * half_step / hz_per_bin);
        let (b0, b1) = (lo.ceil() as usize, hi.floor() as usize);
        let v = if b1 >= b0 && b0 < bins_db.len() {
            // several bins under this point: show the loudest
            bins_db[b0..=b1.min(bins_db.len() - 1)].iter().cloned().fold(FLOOR_DB, f32::max)
        } else {
            // fewer than one bin per point (low end): interpolate between bins
            let x = (f / hz_per_bin).clamp(0.0, (bins_db.len() - 1) as f32);
            let k = (x.floor() as usize).min(bins_db.len() - 2);
            bins_db[k] + (bins_db[k + 1] - bins_db[k]) * (x - k as f32)
        };
        *o = (v + TILT_DB_PER_OCT * (f / 1000.0).log2()).max(FLOOR_DB);
    }
}

pub struct Analyzer {
    fft: Fft,
    window: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
    side_buf: Vec<f32>,
    bins_db: Vec<f32>,
    fresh: Vec<f32>,
    /// Smoothed curves: [mid, side], dB, tilted.
    pub shown: [Vec<f32>; 2],
    last: f64,
    /// Debug builds only: draw a synthetic noise instead of the tap (layout screenshots).
    demo: Option<u32>,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self {
            fft: Fft::new(FFT_SIZE),
            window: hann(FFT_SIZE),
            re: vec![0.0; FFT_SIZE],
            im: vec![0.0; FFT_SIZE],
            side_buf: vec![0.0; FFT_SIZE],
            bins_db: vec![FLOOR_DB; FFT_SIZE / 2],
            fresh: vec![FLOOR_DB; POINTS],
            shown: [vec![FLOOR_DB; POINTS], vec![FLOOR_DB; POINTS]],
            last: f64::NEG_INFINITY,
            demo: if cfg!(debug_assertions) && std::env::var_os("SANCTIS_UI_DEMO").is_some() { Some(1) } else { None },
        }
    }
}

impl Analyzer {
    /// Windowed FFT of `self.re` -> `self.bins_db`.
    fn transform(&mut self) {
        for (v, w) in self.re.iter_mut().zip(&self.window) {
            *v *= w;
        }
        self.im.fill(0.0);
        self.fft.forward(&mut self.re, &mut self.im);
        // Hann halves a sine's peak: |X| * 4 / N is its amplitude
        let scale = 4.0 / FFT_SIZE as f32;
        for (b, o) in self.bins_db.iter_mut().enumerate() {
            let mag = (self.re[b] * self.re[b] + self.im[b] * self.im[b]).sqrt() * scale;
            *o = 20.0 * mag.max(1e-6).log10();
        }
    }

    /// GUI thread, every frame: runs a new FFT when it is due. `now` in seconds.
    /// Returns true while something is above the floor (keep repainting).
    pub fn update(&mut self, tap: &SpectrumTap, now: f64) -> bool {
        // `now` starts again at 0 each time the editor window opens, `last` does not
        if now - self.last >= INTERVAL || now < self.last {
            self.last = now;
            let sr = tap.sample_rate.load(Ordering::Relaxed);
            tap.read(&mut self.re, &mut self.side_buf);
            if let Some(seed) = self.demo.as_mut() {
                // pink-ish mid, quieter and brighter side
                let (mut lp, mut prev) = (0.0f32, 0.0f32);
                for i in 0..FFT_SIZE {
                    *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let w = (*seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
                    lp += 0.02 * (w - lp);
                    self.re[i] = 0.9 * lp + 0.02 * w;
                    self.side_buf[i] = 0.02 * (w - prev);
                    prev = w;
                }
            }
            for ch in 0..2 {
                if ch == 1 {
                    self.re.copy_from_slice(&self.side_buf);
                }
                self.transform();
                to_points(&self.bins_db, sr, &mut self.fresh);
                // fast up, slow down
                for (s, v) in self.shown[ch].iter_mut().zip(&self.fresh) {
                    *s += (if *v > *s { 0.6 } else { 0.18 }) * (*v - *s);
                }
            }
        }
        // the tilt lifts the empty floor by up to 19 dB at the top end: stay clear of it
        self.shown.iter().flatten().any(|v| *v > FLOOR_DB + 30.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sine_shows_up_at_its_frequency_and_level() {
        // 1 kHz mid sine at -20 dBFS: the 1 kHz point reads about -20 dB (tilt is 0 there), side stays at the floor
        let tap = SpectrumTap::default();
        let a = 10f32.powf(-20.0 / 20.0);
        let x: Vec<f32> = (0..TAP_SIZE).map(|i| a * (std::f32::consts::TAU * 1000.0 * i as f32 / 48000.0).sin()).collect();
        tap.write(&x, &x);
        let mut an = Analyzer::default();
        for k in 0..40 {
            an.update(&tap, k as f64);
        }
        let i1k = (0..POINTS).min_by(|a, b| (point_freq(*a) - 1000.0).abs().total_cmp(&(point_freq(*b) - 1000.0).abs())).unwrap();
        let peak = an.shown[0][i1k - 2..=i1k + 2].iter().cloned().fold(FLOOR_DB, f32::max);
        assert!((peak + 20.0).abs() < 1.5, "{peak}");
        assert!(an.shown[0][10] < -80.0 && an.shown[0][POINTS - 10] < -70.0);
        assert!(an.shown[1].iter().all(|v| *v < -100.0));
    }

    #[test]
    fn keeps_running_when_the_clock_starts_over() {
        // the editor was open for 100 s, then closed and opened again: time is back at 0
        let tap = SpectrumTap::default();
        let x = vec![0.5f32; TAP_SIZE];
        let mut an = Analyzer::default();
        an.update(&tap, 100.0);
        tap.write(&x, &x);
        let before = an.shown[0].clone();
        an.update(&tap, 0.5);
        assert_ne!(an.shown[0], before);
    }
}
