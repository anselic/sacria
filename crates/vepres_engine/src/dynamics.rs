//! Band compressors: mean-square detector, soft-knee gain computer, and a
//! gain-reduction smoother in dB: one-pole attack, then hold, then a linear
//! release ramp (constant dB per second).
//!
//! All bands of M and S run together in one struct-of-arrays bank of `LANES`
//! SIMD lanes (`wide::f32x8`). The per-lane code is branchless.

use crate::params::{BandDyn, LowStepParams};
use sacria_dsp::fastmath::{db_to_gain_x8, power_to_db_x8};
pub use sacria_dsp::util::{gain_reduction_db, time_coef};
use wide::f32x8;

pub const LANES: usize = 8;
/// Lanes 6 and 7 only run the mean-square detector of the low-band step trigger
/// (mid, side). Their result goes to `STEP_TARGET` lanes.
pub const STEP_LANES: [usize; 2] = [6, 7];
const STEP_TARGET: [usize; 2] = [0, 3];

const MS_FLOOR: f32 = 1e-20; // -200 dB, keeps log2 input normal
const MIN_KNEE_DB: f32 = 1e-3;
const INSTANT_RATE: f32 = 1e9;

/// Release ramp in dB per sample. `release_ms` is the time to recover 10 dB.
pub fn release_rate(release_ms: f32, sr: f32) -> f32 {
    if release_ms <= 0.0 {
        INSTANT_RATE
    } else {
        10.0 / (release_ms * 1e-3 * sr)
    }
}

/// 8 compressors in SIMD lanes. Unused lanes have ratio 1 and do nothing.
#[derive(Clone, Copy, Debug)]
pub struct DynBank {
    ms: f32x8,
    gr_db: f32x8,
    rms_a: f32x8,
    att: f32x8,
    /// Release ramp in dB per sample.
    rate: f32x8,
    /// Hold left (samples), and hold length.
    hold: f32x8,
    hold_len: f32x8,
    threshold_db: f32x8,
    slope: f32x8,
    knee_db: f32x8,
    inv_2knee: f32x8,
    makeup_db: f32x8,
    /// Release deadband (dB) and the "release running" mask.
    deadband: f32x8,
    /// Deadband works only while level > threshold - floor.
    deadband_floor: f32x8,
    releasing: f32x8,
    /// Low-band step: trigger level (dB) and extra GR (dB) per channel.
    step_thr: [f32; 2],
    step_gr: [f32; 2],
    /// 1 / knee width (dB), or infinity for a hard switch.
    step_inv_knee: [f32; 2],
    step_hyst: [f32; 2],
    /// Trigger state for the hysteresis (true = on).
    step_on: [bool; 2],
}

impl Default for DynBank {
    fn default() -> Self {
        Self {
            ms: f32x8::ZERO,
            gr_db: f32x8::ZERO,
            rms_a: f32x8::ONE,
            att: f32x8::ZERO,
            rate: f32x8::splat(INSTANT_RATE),
            hold: f32x8::ZERO,
            hold_len: f32x8::ZERO,
            threshold_db: f32x8::ZERO,
            slope: f32x8::ZERO,
            knee_db: f32x8::splat(MIN_KNEE_DB),
            inv_2knee: f32x8::splat(0.5 / MIN_KNEE_DB),
            makeup_db: f32x8::ZERO,
            deadband: f32x8::ZERO,
            deadband_floor: f32x8::ZERO,
            releasing: f32x8::ZERO,
            step_thr: [f32::INFINITY; 2],
            step_gr: [0.0; 2],
            step_inv_knee: [f32::INFINITY; 2],
            step_hyst: [0.0; 2],
            step_on: [false; 2],
        }
    }
}

#[inline]
fn set(v: &mut f32x8, lane: usize, x: f32) {
    let mut a = v.to_array();
    a[lane] = x;
    *v = f32x8::new(a);
}

impl DynBank {
    pub fn set_lane(&mut self, lane: usize, p: &BandDyn, sr: f32) {
        set(&mut self.rms_a, lane, 1.0 - time_coef(p.rms_ms, sr));
        set(&mut self.att, lane, time_coef(p.attack_ms, sr));
        set(&mut self.rate, lane, release_rate(p.release_ms, sr));
        set(&mut self.hold_len, lane, (p.hold_ms.max(0.0) * 1e-3 * sr).round());
        set(&mut self.threshold_db, lane, p.threshold_db);
        set(&mut self.slope, lane, 1.0 / p.ratio.max(1.0) - 1.0);
        let k = p.knee_db.max(MIN_KNEE_DB);
        set(&mut self.knee_db, lane, k);
        set(&mut self.inv_2knee, lane, 0.5 / k);
        set(&mut self.makeup_db, lane, p.makeup_db);
    }

    /// `ch` 0 = mid, 1 = side.
    pub fn set_step(&mut self, ch: usize, p: &LowStepParams, sr: f32) {
        set(&mut self.rms_a, STEP_LANES[ch], 1.0 - time_coef(p.rms_ms, sr));
        self.step_thr[ch] = if p.enabled { p.threshold_db } else { f32::INFINITY };
        self.step_gr[ch] = p.gr_db.min(0.0);
        self.step_inv_knee[ch] = if p.knee_db > 0.0 { 1.0 / p.knee_db } else { f32::INFINITY };
        self.step_hyst[ch] = p.hysteresis_db.max(0.0);
    }

    pub fn set_deadband(&mut self, db: f32, floor_db: f32) {
        self.deadband = f32x8::splat(db.max(0.0));
        self.deadband_floor = f32x8::splat(floor_db.max(0.0));
    }

    /// Feeds one detector sample per lane. Returns the linear band gains (GR + makeup).
    #[inline]
    pub fn tick(&mut self, x: &[f32; LANES]) -> [f32; LANES] {
        let x = f32x8::new(*x);

        // one-pole lowpass on x^2 gives a running mean square (power)
        // rms_a = 1 - coef, so rms_a = 1 means no smoothing
        self.ms = self.ms + self.rms_a * (x * x - self.ms);
        // power to dB (10*log10), the floor stops log of 0 on silence
        let level = power_to_db_x8(self.ms + f32x8::splat(MS_FLOOR));

        // d = level - threshold, k = knee width
        // u = d + k/2 is 0 at the bottom of the knee and k at the top
        // q = clamp(u, 0, k) is how far into the knee we are
        //   below knee: q = 0, (u - k)+ = 0      -> target = 0
        //   in knee:    q = u, (u - k)+ = 0      -> target = slope * u^2 / 2k
        //   above knee: q = k, (u - k)+ = d - k/2 -> target = slope * d
        // slope = 1/ratio - 1 is negative, so target is <= 0 dB
        let u = level - self.threshold_db + self.knee_db * f32x8::splat(0.5);
        let q = u.fast_max(f32x8::ZERO).fast_min(self.knee_db);
        let mut target = self.slope * (q * q * self.inv_2knee + (u - self.knee_db).fast_max(f32x8::ZERO));

        // low-band step: the trigger lanes switch a fixed extra GR on the low lanes
        let lv = level.to_array();
        let mut extra = [0.0f32; LANES];
        for ch in 0..2 {
            // hysteresis: once on, the threshold drops by `step_hyst` until it switches off
            let l = lv[STEP_LANES[ch]];
            let thr = if self.step_on[ch] { self.step_thr[ch] - self.step_hyst[ch] } else { self.step_thr[ch] };
            self.step_on[ch] = l > thr;
            // 0 below the knee, 1 above, linear in dB inside (centered on the threshold)
            let d = (l - thr) * self.step_inv_knee[ch];
            let amount = if d.is_nan() { 0.0 } else { (d + 0.5).clamp(0.0, 1.0) };
            extra[STEP_TARGET[ch]] = self.step_gr[ch] * amount;
        }
        target = target + f32x8::new(extra);

        // smoother: GR is negative dB, so "target below current" means
        // we need more reduction: that is the attack case
        let attacking = target.simd_lt(self.gr_db);
        // (Attack) one-pole glide from current GR toward target.
        let gr_att = target + self.att * (self.gr_db - target);
        // (Release) while hold > 0, freeze GR. after that, wait until GR is more
        // than `deadband` dB deeper than target, then ramp up by `rate` dB per
        // sample until it reaches target (never past it)
        let near = (level - self.threshold_db + self.deadband_floor).simd_gt(f32x8::ZERO);
        let deadband = near.bitselect(self.deadband, f32x8::ZERO);
        let free = self.hold.simd_le(f32x8::ZERO) & (self.releasing | (target - self.gr_db).simd_gt(deadband));
        let gr_rel = free.bitselect((self.gr_db + self.rate).fast_min(target), self.gr_db);
        // pick attack or release per lane
        self.gr_db = attacking.bitselect(gr_att, gr_rel);
        self.releasing = attacking.bitselect(f32x8::ZERO, free & self.gr_db.simd_lt(target));
        // hold counter, reload to full length on every attack sample,
        // otherwise count down one sample, stopping at 0
        self.hold = attacking.bitselect(self.hold_len, (self.hold - f32x8::ONE).fast_max(f32x8::ZERO));

        // GR plus makeup, back to a linear gain
        db_to_gain_x8(self.gr_db + self.makeup_db).to_array()
    }

    /// Current gain reduction (dB, <= 0) of a lane, for metering.
    pub fn gain_reduction(&self, lane: usize) -> f32 {
        self.gr_db.to_array()[lane]
    }

    pub fn reset(&mut self) {
        self.ms = f32x8::ZERO;
        self.gr_db = f32x8::ZERO;
        self.hold = f32x8::ZERO;
        self.releasing = f32x8::ZERO;
        self.step_on = [false; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knee_is_continuous_and_hits_ratio() {
        let s = 1.0 / 2.0 - 1.0;
        assert_eq!(gain_reduction_db(-30.0, -20.0, s, 6.0), 0.0);
        assert!((gain_reduction_db(0.0, -20.0, s, 6.0) + 10.0).abs() < 1e-5);
        for &e in &[-3.0f32, 3.0] {
            let a = gain_reduction_db(-20.0 + e - 1e-4, -20.0, s, 6.0);
            let b = gain_reduction_db(-20.0 + e + 1e-4, -20.0, s, 6.0);
            assert!((a - b).abs() < 1e-3);
        }
    }

    #[test]
    fn bank_knee_matches_reference() {
        let p = BandDyn { threshold_db: -20.0, ratio: 2.5, knee_db: 6.0, rms_ms: 0.0, attack_ms: 0.0, hold_ms: 0.0, release_ms: 0.0, makeup_db: 0.0 };
        let mut bank = DynBank::default();
        bank.set_lane(0, &p, 48000.0);
        let mut lvl = -40.0f32;
        while lvl < 0.0 {
            bank.reset();
            let a = 10f32.powf(lvl / 20.0);
            let g = bank.tick(&[a, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])[0];
            let want = gain_reduction_db(lvl, -20.0, 1.0 / 2.5 - 1.0, 6.0);
            assert!((20.0 * g.log10() - want).abs() < 1e-3, "{lvl}: {} vs {want}", 20.0 * g.log10());
            lvl += 0.37;
        }
    }

    #[test]
    fn release_is_hold_then_linear_ramp() {
        let sr = 48000.0;
        let p = BandDyn { threshold_db: -40.0, ratio: 100.0, knee_db: 0.0, rms_ms: 0.0, attack_ms: 0.0, hold_ms: 10.0, release_ms: 100.0, makeup_db: 0.0 };
        let mut bank = DynBank::default();
        bank.set_lane(0, &p, sr);
        let gr = |b: &mut DynBank, x: f32| {
            b.tick(&[x, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
            b.gain_reduction(0)
        };
        let loud = gr(&mut bank, 0.1); // -20 dB in, ~ -19.8 dB GR
        for _ in 0..480 {
            assert_eq!(gr(&mut bank, 1e-6), loud); // hold 10 ms
        }
        let after = (0..4800).map(|_| gr(&mut bank, 1e-6)).last().unwrap(); // +100 ms
        assert!((after - (loud + 10.0)).abs() < 0.01, "{after} vs {}", loud + 10.0);
    }

    #[test]
    fn deadband_holds_small_drops() {
        let sr = 48000.0;
        let p = BandDyn { threshold_db: -40.0, ratio: 100.0, knee_db: 0.0, rms_ms: 0.0, attack_ms: 0.0, hold_ms: 0.0, release_ms: 100.0, makeup_db: 0.0 };
        let mut bank = DynBank::default();
        bank.set_lane(0, &p, sr);
        bank.set_deadband(1.0, 30.0);
        let gr = |b: &mut DynBank, db: f32| {
            b.tick(&[10f32.powf(db / 20.0), 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
            b.gain_reduction(0)
        };
        let start = (0..100).map(|_| gr(&mut bank, -20.0)).last().unwrap();
        // 0.5 dB less input: inside the deadband, GR stays
        let held = (0..4800).map(|_| gr(&mut bank, -20.5)).last().unwrap();
        assert_eq!(held, start);
        // 1.5 dB less: outside, GR releases all the way to the new target
        let target = (0..4800).map(|_| gr(&mut bank, -21.5)).last().unwrap();
        assert!((target - (start + 1.5 * 0.99)).abs() < 0.01, "{target} vs {start}");
    }
}
