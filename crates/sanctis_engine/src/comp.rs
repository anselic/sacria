//! Band compressor: power detector on the band's mono sum, soft-knee gain computer.
//!
//! The detector runs at a control rate (about 3 kHz): the mean power of each
//! block goes through a one-pole with separate attack and release. The gain
//! reduction is then interpolated linearly (in dB) between blocks. To line the
//! interpolated gain up with the audio, the band audio must be delayed by
//! `latency()` samples.
//!
//! Parallel compression needs no second signal path: the compressor is only a
//! gain `g`, so a blend `b` of compressed and uncompressed is the gain `1 + b * (g - 1)`.

use crate::params::BandParams;
use sacria_dsp::fastmath::{db_to_gain, power_to_db};
use sacria_dsp::util::gain_reduction_db;

const POWER_FLOOR: f32 = 1e-20;
const MIN_KNEE_DB: f32 = 1e-3;
/// A steady sine reads its peak level: 10*log10(2).
const SINE_PEAK_DB: f32 = 3.010_3;

/// Longest release of the display meter (`signal_level_db`).
const METER_MAX_RELEASE_MS: f32 = 120.0;

/// Control block length for a sample rate (16 at 48 kHz).
pub fn block_len(sr: f32) -> usize {
    ((sr / 3000.0).round() as usize).max(1)
}

#[derive(Clone, Copy, Debug)]
pub struct BandComp {
    block: usize,
    count: usize,
    acc: f32,
    /// Smoothed power.
    power: f32,
    att: f32,
    rel: f32,
    threshold_db: f32,
    slope: f32,
    knee_db: f32,
    blend: f32,
    /// Gain reduction ramp: value now, step per sample, target.
    gr_db: f32,
    gr_step: f32,
    gr_target: f32,
    level_db: f32,
    /// Fast power follower for the level display only (does not touch the gain).
    meter: f32,
    meter_att: f32,
    meter_rel: f32,
}

impl BandComp {
    pub fn new(sr: f32) -> Self {
        Self {
            block: block_len(sr),
            count: 0,
            acc: 0.0,
            power: 1e-12,
            att: 1.0,
            rel: 1.0,
            threshold_db: 0.0,
            slope: 0.0,
            knee_db: MIN_KNEE_DB,
            blend: 1.0,
            gr_db: 0.0,
            gr_step: 0.0,
            gr_target: 0.0,
            level_db: -200.0,
            meter: 1e-12,
            meter_att: 1.0,
            meter_rel: 1.0,
        }
    }

    pub fn set(&mut self, p: &BandParams, sr: f32) {
        let fs = sr as f64 / self.block as f64;
        let coef = |ms: f32| if ms <= 0.0 { 1.0 } else { (1.0 - (-1.0 / (ms as f64 * 1e-3 * fs)).exp()) as f32 };
        self.att = coef(p.attack_ms);
        self.rel = coef(p.release_ms);
        // the display meter is the same detector with a capped release, so on a
        // steady signal both read the same level
        self.meter_att = self.att;
        self.meter_rel = coef(p.release_ms.min(METER_MAX_RELEASE_MS));
        self.threshold_db = p.threshold_db;
        self.slope = 1.0 / p.ratio.max(1.0) - 1.0;
        self.knee_db = p.knee_db.max(MIN_KNEE_DB);
        self.blend = p.blend.clamp(0.0, 1.0);
    }

    /// Audio delay that lines the band up with the gain this returns.
    pub fn latency(&self) -> usize {
        self.block + self.block / 2
    }

    pub fn reset(&mut self) {
        self.count = 0;
        self.acc = 0.0;
        self.power = 1e-12;
        self.gr_db = 0.0;
        self.gr_step = 0.0;
        self.gr_target = 0.0;
        self.level_db = -200.0;
        self.meter = 1e-12;
    }

    /// Current gain reduction in dB (<= 0), after the blend, for metering.
    pub fn gain_reduction_db(&self) -> f32 {
        if self.blend >= 1.0 {
            self.gr_db
        } else {
            20.0 * (1.0 + self.blend * (db_to_gain(self.gr_db) - 1.0)).log10()
        }
    }

    /// Level the compressor works with right now, in dBFS (a steady sine reads
    /// its peak). It follows the band's attack and release, so it can fall slowly.
    pub fn detector_level_db(&self) -> f32 {
        self.level_db
    }

    /// Level of the band signal itself: the detector again, but with its release
    /// capped at 120 ms, so it drops as soon as the signal does. For displays.
    pub fn signal_level_db(&self) -> f32 {
        power_to_db(self.meter.max(POWER_FLOOR)) + SINE_PEAK_DB
    }

    /// Feed one detector sample (the band's mono sum, or the louder of left and
    /// right). Returns the linear gain for the band sample `latency()` samples ago.
    #[inline]
    pub fn tick(&mut self, mono: f32) -> f32 {
        let g = 1.0 + self.blend * (db_to_gain(self.gr_db) - 1.0);
        self.gr_db += self.gr_step;
        self.acc += mono * mono;
        self.count += 1;
        if self.count == self.block {
            let p = self.acc / self.block as f32;
            self.acc = 0.0;
            self.count = 0;
            self.power += (if p > self.power { self.att } else { self.rel }) * (p - self.power);
            self.meter += (if p > self.meter { self.meter_att } else { self.meter_rel }) * (p - self.meter);
            let level = power_to_db(self.power.max(POWER_FLOOR)) + SINE_PEAK_DB;
            self.level_db = level;
            // land on the last target exactly, then ramp to the new one over the next block
            self.gr_db = self.gr_target;
            self.gr_target = gain_reduction_db(level, self.threshold_db, self.slope, self.knee_db);
            self.gr_step = (self.gr_target - self.gr_db) / self.block as f32;
        }
        g
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band(thr: f32, ratio: f32, knee: f32, att: f32, rel: f32) -> BandParams {
        BandParams { threshold_db: thr, ratio, knee_db: knee, attack_ms: att, release_ms: rel, blend: 1.0, drive: 0.0, mid_gain_db: 0.0, side_gain_db: 0.0, shape: Default::default(), detect: Default::default() }
    }

    fn settle(c: &mut BandComp, amp: f32, freq: f32, n: usize) -> f32 {
        let mut g = 1.0;
        for i in 0..n {
            g = c.tick(amp * (std::f32::consts::TAU * freq * i as f32 / 48000.0).sin());
        }
        20.0 * g.log10()
    }

    #[test]
    fn blend_mixes_the_uncompressed_band_back_in() {
        // same case as below: -5 dB of reduction at 100%. At 50%: half compressed, half dry
        let mut p = band(-20.0, 2.0, 0.0, 50.0, 50.0);
        p.blend = 0.5;
        let mut c = BandComp::new(48000.0);
        c.set(&p, 48000.0);
        let gr = settle(&mut c, 10f32.powf(-10.0 / 20.0), 1000.0, 48000);
        let want = 20.0 * (0.5 + 0.5 * 10f32.powf(-5.0 / 20.0)).log10();
        assert!((gr - want).abs() < 0.02, "{gr} {want}");
        assert!((c.gain_reduction_db() - want).abs() < 0.02);
        // 0% = no compression at all
        p.blend = 0.0;
        c.set(&p, 48000.0);
        assert!(settle(&mut c, 10f32.powf(-10.0 / 20.0), 1000.0, 4800).abs() < 1e-6);
    }

    #[test]
    fn steady_sine_reads_its_peak_level() {
        // attack = release: no peak riding. Sine at -10 dBFS peak, thr -20, 2:1, hard knee -> -5 dB
        let mut c = BandComp::new(48000.0);
        c.set(&band(-20.0, 2.0, 0.0, 50.0, 50.0), 48000.0);
        let gr = settle(&mut c, 10f32.powf(-10.0 / 20.0), 1000.0, 48000);
        assert!((gr + 5.0).abs() < 0.02, "{gr}");
    }

    #[test]
    fn quiet_signal_is_untouched() {
        let mut c = BandComp::new(48000.0);
        c.set(&band(-20.0, 4.0, 6.0, 5.0, 100.0), 48000.0);
        let gr = settle(&mut c, 10f32.powf(-40.0 / 20.0), 1000.0, 24000);
        assert_eq!(gr, 0.0);
    }

    #[test]
    fn display_meter_is_fast_even_with_a_slow_release() {
        // 2 s release: the detector is still high half a second after the tone stops, the meter is not
        let mut c = BandComp::new(48000.0);
        c.set(&band(-10.0, 1.2, 0.0, 1.5, 2000.0), 48000.0);
        settle(&mut c, 0.5, 100.0, 48000);
        // steady signal: meter and detector agree
        assert!((c.signal_level_db() - c.detector_level_db()).abs() < 1.0, "{} vs {}", c.signal_level_db(), c.detector_level_db());
        for _ in 0..24000 {
            c.tick(0.0);
        }
        assert!(c.detector_level_db() > -10.0, "{}", c.detector_level_db());
        assert!(c.signal_level_db() < -20.0, "{}", c.signal_level_db());
    }

    #[test]
    fn release_is_linear_in_db() {
        // one-pole on power: after the signal stops, the level falls 4.34/tau dB per second
        let mut c = BandComp::new(48000.0);
        c.set(&band(-40.0, 2.0, 0.0, 1.0, 100.0), 48000.0);
        settle(&mut c, 1.0, 1000.0, 24000);
        let mut gr = Vec::new();
        for i in 0..9600 {
            c.tick(0.0);
            if i % 960 == 959 {
                gr.push(c.gain_reduction_db());
            }
        }
        // level slope 43.4 dB/s, 2:1 -> GR slope 21.7 dB/s = 0.434 dB per 20 ms
        for w in gr.windows(2).take(6) {
            assert!((w[1] - w[0] - 0.434).abs() < 0.02, "{:?}", gr);
        }
    }
}
