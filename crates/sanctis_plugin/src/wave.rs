//! Colored waveform for the response strip, in the style of the rekordbox RGB
//! waveform: the color shows which of three bands holds the most power (red =
//! lows, green = mids, blue = highs, the strongest band at full), the height is
//! the squared peak level.
//!
//! The band filters are a fit to what rekordbox stored for a file of test tones
//! (internal/analysis/rekordbox, model in rb_rgb_model.json).
//!
//! The audio thread runs the filters and writes fine columns (`FINE_PER_BAR` per
//! bar) into `WaveTap` while the editor is open. The GUI groups them into the
//! bins it draws, so the zoom is only a matter of how many columns go in a bin.

use sacria_dsp::biquad::{Biquad, Coeffs};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Fine columns per bar, and how many bars of them are kept.
pub const FINE_PER_BAR: usize = 4096;
pub const MAX_BARS: usize = 32;
const RING: usize = FINE_PER_BAR * MAX_BARS;
/// Most bins drawn across the strip.
pub const SCREEN: usize = 512;
/// Zoom steps: fine columns across the strip, 1/16 bar to 32 bars.
pub const SPANS: [usize; 10] = [
    FINE_PER_BAR / 16,
    FINE_PER_BAR / 8,
    FINE_PER_BAR / 4,
    FINE_PER_BAR / 2,
    FINE_PER_BAR,
    FINE_PER_BAR * 2,
    FINE_PER_BAR * 4,
    FINE_PER_BAR * 8,
    FINE_PER_BAR * 16,
    FINE_PER_BAR * 32,
];
pub const SPAN_NAMES: [&str; 10] = ["1/16 BAR", "1/8 BAR", "1/4 BAR", "1/2 BAR", "1 BAR", "2 BARS", "4 BARS", "8 BARS", "16 BARS", "32 BARS"];
/// The zoom the display opens with (4 bars).
pub const DEFAULT_SPAN: usize = 6;

// the fitted model: Butterworth sections, frequencies in Hz
const RED_LP: f32 = 108.4; // order 3
const GREEN_HP: f32 = 842.8; // order 1
const GREEN_LP: f32 = 3707.4; // order 2
const BLUE_HP: f32 = 1103.5; // order 3
const BLUE_LP: f32 = 12049.7; // order 4
const GREEN_WEIGHT: f32 = 0.737;
const BLUE_WEIGHT: f32 = 0.890;

/// The band powers are smoothed over this time, so the color does not flicker
/// inside one cycle of a bass note when zoomed in.
const POWER_MS: f32 = 8.0;
/// A column that is quieter than this is not kept while the host is stopped,
/// so the picture stays when you stop playback.
const SILENCE: f32 = 1e-5;
/// Stored levels are 16 bit, full scale = this amplitude.
const STORE_MAX: f32 = 2.0;

/// The last `RING` fine columns, each packed as four 16-bit levels: red, green
/// and blue band level (weighted, RMS) and the peak.
pub struct WaveTap {
    cols: Vec<AtomicU64>,
    /// Columns written so far.
    total: AtomicUsize,
}

impl Default for WaveTap {
    fn default() -> Self {
        Self { cols: (0..RING).map(|_| AtomicU64::new(0)).collect(), total: AtomicUsize::new(0) }
    }
}

/// One drawn bin: red, green, blue and the height (0..=1).
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Bin {
    pub rgb: [u8; 3],
    pub height: f32,
}

impl WaveTap {
    fn push(&self, col: u64) {
        let t = self.total.load(Ordering::Relaxed);
        self.cols[t % RING].store(col, Ordering::Relaxed);
        self.total.store(t + 1, Ordering::Relaxed);
    }

    /// GUI: the newest `span` fine columns as bins, oldest first. Bins sit on a
    /// fixed grid of columns, so a bin does not change while it scrolls.
    pub fn bins(&self, span: usize, out: &mut Vec<Bin>) {
        let n = span.min(SCREEN);
        let per = span / n;
        let end = self.total.load(Ordering::Relaxed);
        let oldest = end.saturating_sub(RING);
        let last = (end.max(1) - 1) / per;
        out.clear();
        for i in 0..n {
            let Some(b) = (last + 1 + i).checked_sub(n) else {
                out.push(Bin::default());
                continue;
            };
            let (mut power, mut peak) = ([0.0f32; 3], 0.0f32);
            for c in (b * per).max(oldest)..((b + 1) * per).min(end) {
                let v = self.cols[c % RING].load(Ordering::Relaxed);
                let level = |shift: u32| ((v >> shift) & 0xffff) as f32 * (STORE_MAX / 65535.0);
                for (p, shift) in power.iter_mut().zip([48, 32, 16]) {
                    *p += level(shift) * level(shift);
                }
                peak = peak.max(level(0));
            }
            let max = power[0].max(power[1]).max(power[2]);
            let rgb = power.map(|p| if max > 1e-12 { (255.0 * p / max).round() as u8 } else { 0 });
            out.push(Bin { rgb, height: (peak * peak).min(1.0) });
        }
    }
}

/// Smoothed band powers and peak -> packed fine column.
fn pack(power: [f32; 3], peak: f32) -> u64 {
    let q = |amp: f32| (amp.clamp(0.0, STORE_MAX) * (65535.0 / STORE_MAX)).round() as u64;
    let level = |p: f32, weight: f32| q((weight * p.max(0.0)).sqrt());
    level(power[0], 1.0) << 48 | level(power[1], GREEN_WEIGHT) << 32 | level(power[2], BLUE_WEIGHT) << 16 | q(peak)
}

fn k(freq: f32, sr: f32) -> f64 {
    (std::f64::consts::PI * (freq as f64 / sr as f64).min(0.45)).tan()
}

fn first_order(freq: f32, sr: f32, highpass: bool) -> Coeffs {
    let k = k(freq, sr);
    let a1 = ((k - 1.0) / (k + 1.0)) as f32;
    if highpass {
        let b = (1.0 / (1.0 + k)) as f32;
        Coeffs { b0: b, b1: -b, b2: 0.0, a1, a2: 0.0 }
    } else {
        let b = (k / (1.0 + k)) as f32;
        Coeffs { b0: b, b1: b, b2: 0.0, a1, a2: 0.0 }
    }
}

fn second_order(freq: f32, q: f64, sr: f32, highpass: bool) -> Coeffs {
    let k = k(freq, sr);
    let n = 1.0 / (1.0 + k / q + k * k);
    let (a1, a2) = ((2.0 * (k * k - 1.0) * n) as f32, ((1.0 - k / q + k * k) * n) as f32);
    if highpass {
        Coeffs { b0: n as f32, b1: (-2.0 * n) as f32, b2: n as f32, a1, a2 }
    } else {
        let b = (k * k * n) as f32;
        Coeffs { b0: b, b1: 2.0 * b, b2: b, a1, a2 }
    }
}

/// The color a steady tone of this frequency gets in the waveform: the band
/// model's power responses, strongest band at full. For coloring the spectrum.
pub fn tone_color(freq: f32) -> [u8; 3] {
    // Butterworth power response of order `n`: 1 / (1 + (f / fc)^(2n))
    let lp = |fc: f32, n: i32| 1.0 / (1.0 + (freq / fc).powi(2 * n));
    let hp = |fc: f32, n: i32| 1.0 - lp(fc, n);
    let p = [lp(RED_LP, 3), GREEN_WEIGHT * hp(GREEN_HP, 1) * lp(GREEN_LP, 2), BLUE_WEIGHT * hp(BLUE_HP, 3) * lp(BLUE_LP, 4)];
    let max = p[0].max(p[1]).max(p[2]).max(1e-20);
    p.map(|v| (255.0 * v / max).round() as u8)
}

/// Audio-thread side: band filters and the column that is filling up.
#[derive(Default)]
pub struct WaveMeter {
    red: [Biquad; 2],
    green: [Biquad; 2],
    blue: [Biquad; 4],
    /// Smoothed band powers.
    power: [f32; 3],
    smooth: f32,
    peak: f32,
    /// Samples in the current column so far.
    count: f64,
}

impl WaveMeter {
    pub fn set_sample_rate(&mut self, sr: f32) {
        // Butterworth: order 3 = first order + Q 1, order 4 = Q 0.5412 + Q 1.3066
        self.red[0].c = first_order(RED_LP, sr, false);
        self.red[1].c = second_order(RED_LP, 1.0, sr, false);
        self.green[0].c = first_order(GREEN_HP, sr, true);
        self.green[1].c = second_order(GREEN_LP, std::f64::consts::FRAC_1_SQRT_2, sr, false);
        self.blue[0].c = first_order(BLUE_HP, sr, true);
        self.blue[1].c = second_order(BLUE_HP, 1.0, sr, true);
        self.blue[2].c = second_order(BLUE_LP, 0.5412, sr, false);
        self.blue[3].c = second_order(BLUE_LP, 1.3066, sr, false);
        self.smooth = 1.0 - (-1.0 / (POWER_MS * 1e-3 * sr)).exp();
        self.reset();
    }

    pub fn reset(&mut self) {
        self.red.iter_mut().chain(&mut self.green).chain(&mut self.blue).for_each(Biquad::reset);
        self.power = [0.0; 3];
        self.peak = 0.0;
        self.count = 0.0;
    }

    /// One block of output. `column_len`: samples per fine column. `playing`: the host transport runs.
    pub fn process(&mut self, left: &[f32], right: &[f32], column_len: f64, playing: bool, tap: &WaveTap) {
        for (l, r) in left.iter().zip(right) {
            let x = 0.5 * (l + r);
            let run = |chain: &mut [Biquad]| chain.iter_mut().fold(x, |v, f| f.tick(v));
            let bands = [run(&mut self.red), run(&mut self.green), run(&mut self.blue)];
            for (p, b) in self.power.iter_mut().zip(bands) {
                *p += self.smooth * (b * b - *p);
            }
            self.peak = self.peak.max(x.abs());
            self.count += 1.0;
            if self.count >= column_len {
                self.count -= column_len;
                if playing || self.peak > SILENCE {
                    tap.push(pack(self.power, self.peak));
                }
                self.peak = 0.0;
            }
        }
    }
}

/// Samples per fine column at this tempo and time signature.
pub fn column_len(sample_rate: f32, tempo: Option<f64>, numerator: Option<i32>, denominator: Option<i32>) -> f64 {
    let tempo = tempo.filter(|t| *t > 1.0).unwrap_or(120.0);
    let beats_per_bar = match (numerator, denominator) {
        (Some(n), Some(d)) if n > 0 && d > 0 => 4.0 * n as f64 / d as f64,
        _ => 4.0,
    };
    (sample_rate as f64 * 60.0 / tempo * beats_per_bar / FINE_PER_BAR as f64).max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Color (in rekordbox's 0..7 steps) and height of a steady sine.
    fn sine(freq: f32, amp: f32) -> ([f32; 3], f32) {
        let sr = 44100.0;
        let mut m = WaveMeter::default();
        m.set_sample_rate(sr);
        let tap = WaveTap::default();
        let x: Vec<f32> = (0..44100).map(|i| amp * (std::f32::consts::TAU * freq * i as f32 / sr).sin()).collect();
        m.process(&x, &x, 20.0, true, &tap);
        let mut bins = Vec::new();
        tap.bins(SPANS[3], &mut bins);
        let b = bins[bins.len() - 2];
        (b.rgb.map(|v| v as f32 * 7.0 / 255.0), b.height)
    }

    #[test]
    fn tone_colors_are_close_to_rekordbox() {
        // (Hz, red, green, blue) as rekordbox stored them for the test tones
        for (f, want) in [
            (40.0, [7.0, 0.0, 0.0]),
            (100.8, [7.0, 0.0, 0.0]),
            (201.6, [3.0, 5.0, 0.0]),
            (403.2, [0.0, 7.0, 0.0]),
            (640.0, [0.0, 7.0, 1.0]),
            (1140.3, [0.0, 5.0, 5.0]),
            (2031.9, [0.0, 5.0, 7.0]),
            (4063.8, [0.0, 2.0, 7.0]),
            (8127.5, [0.0, 0.0, 7.0]),
        ] {
            let (got, _) = sine(f, 0.25);
            for (g, w) in got.iter().zip(want) {
                assert!((g - w).abs() <= 2.0, "{f} Hz: {got:?} vs {want:?}");
            }
        }
    }

    #[test]
    fn spectrum_colors_match_the_waveform_colors() {
        // the formula and the real filters give a tone the same color
        for f in [40.0, 150.0, 250.0, 700.0, 1100.0, 3000.0, 9000.0] {
            let (got, _) = sine(f, 0.25);
            let want = tone_color(f).map(|v| v as f32 * 7.0 / 255.0);
            for (g, w) in got.iter().zip(want) {
                assert!((g - w).abs() < 0.35, "{f} Hz: {got:?} vs {want:?}");
            }
        }
        assert_eq!(tone_color(30.0), [255, 0, 0]);
    }

    #[test]
    fn color_ignores_level_and_height_follows_power() {
        let (loud, h0) = sine(1000.0, 0.99);
        let (soft, h6) = sine(1000.0, 0.99 * 0.5);
        for (a, b) in loud.iter().zip(soft) {
            assert!((a - b).abs() < 0.1);
        }
        // rekordbox: 30 of 31 at full scale, 7 of 31 at -6 dB
        assert!((h0 - 0.98).abs() < 0.02 && (h6 - 0.245).abs() < 0.02, "{h0} {h6}");
    }

    #[test]
    fn zoom_steps_span_bars() {
        // 140 bpm, 4/4, 48 kHz: one bar = 1.714 s
        let len = column_len(48000.0, Some(140.0), Some(4), Some(4));
        assert!((len * FINE_PER_BAR as f64 / 48000.0 - 1.7143).abs() < 0.001);
        assert_eq!(column_len(48000.0, None, None, None), column_len(48000.0, Some(120.0), Some(4), Some(4)));
        assert_eq!(SPANS[DEFAULT_SPAN], 4 * FINE_PER_BAR);
        assert_eq!(*SPANS.last().unwrap(), RING);
        assert_eq!(SPANS.len(), SPAN_NAMES.len());
        // every span is a whole number of bins
        for s in SPANS {
            assert_eq!(s % s.min(SCREEN), 0);
        }
    }

    #[test]
    fn bins_keep_the_peak_and_do_not_change_while_scrolling() {
        let tap = WaveTap::default();
        // one loud column in a stream of quiet ones
        for i in 0..40000 {
            tap.push(pack([0.01, 0.0, 0.0], if i == 30000 { 1.0 } else { 0.1 }));
        }
        let (mut a, mut b) = (Vec::new(), Vec::new());
        tap.bins(SPANS[DEFAULT_SPAN], &mut a);
        assert_eq!(a.len(), SCREEN);
        let per = SPANS[DEFAULT_SPAN] / SCREEN;
        let hit = a.iter().position(|x| x.height > 0.9).expect("the peak is drawn");
        assert_eq!(a[hit].rgb, [255, 0, 0]);
        // one bin later the same bin has moved one place to the left, unchanged
        for _ in 0..per {
            tap.push(pack([0.01, 0.0, 0.0], 0.1));
        }
        tap.bins(SPANS[DEFAULT_SPAN], &mut b);
        assert_eq!(b[hit - 1], a[hit]);
        // more span than history: the left part is empty
        tap.bins(SPANS[9], &mut a);
        assert_eq!(a[0], Bin::default());
        assert!(a[SCREEN - 1].height > 0.0);
    }

    #[test]
    fn silence_does_not_scroll_while_stopped() {
        let mut m = WaveMeter::default();
        m.set_sample_rate(48000.0);
        let tap = WaveTap::default();
        m.process(&[0.0; 4800], &[0.0; 4800], 100.0, false, &tap);
        assert_eq!(tap.total.load(Ordering::Relaxed), 0);
        m.process(&[0.0; 4800], &[0.0; 4800], 100.0, true, &tap);
        assert_eq!(tap.total.load(Ordering::Relaxed), 48);
    }
}
