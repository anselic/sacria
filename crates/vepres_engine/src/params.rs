//! Parameters. `Params::up_and_wide()` is the default voicing.

pub const NUM_BANDS: usize = 3;
pub const NUM_EQ: usize = 2;

pub use sacria_dsp::biquad::{EqBand, EqKind};
pub use sacria_dsp::clip::SoftClipParams;
pub use sacria_dsp::limiter::LimiterParams;

/// Dynamics of one band.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BandDyn {
    /// Threshold on the band RMS level (dBFS RMS, after EQ, before makeup).
    pub threshold_db: f32,
    pub ratio: f32,
    /// Full knee width in dB.
    pub knee_db: f32,
    /// Time constant of the mean-square detector.
    pub rms_ms: f32,
    /// One-pole time constant of the attack.
    pub attack_ms: f32,
    /// Time the gain reduction is held before the release starts.
    pub hold_ms: f32,
    /// Release is a linear ramp in dB: time to recover 10 dB.
    pub release_ms: f32,
    /// Gain added to the band after compression (dB).
    pub makeup_db: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ChannelParams {
    pub eq: [EqBand; NUM_EQ],
    /// Soft clipper after the EQ, before the crossover.
    #[cfg_attr(feature = "serde", serde(default))]
    pub clip: SoftClipParams,
    /// Low, mid, high.
    pub bands: [BandDyn; NUM_BANDS],
    /// Extra low-band gain reduction step.
    #[cfg_attr(feature = "serde", serde(default))]
    pub low_step: LowStepParams,
}

/// Adds a fixed extra GR to the low band once a second detector crosses a
/// threshold. That detector hears the channel (after EQ and clip) through a
/// 1st-order highpass, as an RMS level. The extra GR goes through the low band's
/// own attack / hold / release.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LowStepParams {
    pub enabled: bool,
    pub hp_hz: f32,
    /// Trigger level (dBFS RMS of the highpassed signal).
    pub threshold_db: f32,
    /// Extra gain reduction while triggered (dB, <= 0).
    pub gr_db: f32,
    /// Time constant of the trigger's mean-square detector.
    pub rms_ms: f32,
    /// 1st-order lowpass after the highpass (0 = none), so the trigger ignores the mids.
    #[cfg_attr(feature = "serde", serde(default))]
    pub lp_hz: f32,
    /// Width of the trigger knee (dB). 0 = a hard switch. Note: a knee
    /// does not stop flutter, it spreads it (acts like a higher ratio).
    #[cfg_attr(feature = "serde", serde(default))]
    pub knee_db: f32,
    /// The trigger switches off only this many dB below `threshold_db`, so bass
    /// hovering at the threshold does not flip the step on and off.
    #[cfg_attr(feature = "serde", serde(default))]
    pub hysteresis_db: f32,
}

impl Default for LowStepParams {
    fn default() -> Self {
        Self { enabled: false, hp_hz: 150.0, threshold_db: 0.0, gr_db: 0.0, rms_ms: 5.0, lp_hz: 0.0, knee_db: 0.0, hysteresis_db: 0.0 }
    }
}

#[cfg(feature = "serde")]
fn one_u32() -> u32 {
    1
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Params {
    pub input_gain_db: f32,
    /// Corner of the 1st-order low/mid split.
    pub lo_xover_hz: f32,
    /// Corner of the 1st-order mid/high split.
    pub hi_xover_hz: f32,
    /// Corners of the detector (sidechain) split. `None` = same as the audio split.
    #[cfg_attr(feature = "serde", serde(default))]
    pub detector_xover_hz: Option<[f32; 2]>,
    /// Look-ahead of the band compressors (clamped to `MAX_LOOKAHEAD_MS`).
    pub lookahead_ms: f32,
    pub mid: ChannelParams,
    pub side: ChannelParams,
    pub limiter: LimiterParams,
    /// Release starts only when the GR is this much deeper than the target, then
    /// runs to the target (all bands), so slow level drops release in steps.
    #[cfg_attr(feature = "serde", serde(default))]
    pub release_deadband_db: f32,
    /// Oversampling of the soft clipper: 1 (off), 2 or 4. Cuts aliasing when the
    /// clipper works; costs a little CPU and a few samples of phase shift.
    #[cfg_attr(feature = "serde", serde(default = "one_u32"))]
    pub clip_oversample: u32,
    /// The deadband only works while the band level is less than this many dB
    /// below its threshold. Quieter (or silence) releases normally.
    #[cfg_attr(feature = "serde", serde(default))]
    pub release_deadband_floor_db: f32,
    pub output_gain_db: f32,
}

impl Params {
    /// The default opinionated tuning.
    pub fn up_and_wide() -> Self {
        // BEGIN GENERATED (do not hand-edit)
        Self {
            input_gain_db: 0.0,
            lo_xover_hz: 454.1143,
            hi_xover_hz: 3150.2164,
            detector_xover_hz: Some([576.879734, 4147.78518]),
            lookahead_ms: 10.0,
            mid: ChannelParams {
                eq: [EqBand { kind: EqKind::Peaking, freq_hz: 115.84169, gain_db: 2.64946, q: 0.27835 }, EqBand::OFF],
                clip: SoftClipParams { enabled: true, threshold_db: -0.194364174, range_db: 6.21550916, slope: 0.886980956, hardness: 2.06036252 },
                bands: [
                    BandDyn { threshold_db: -13.0006488, ratio: 2.97946279, knee_db: 2.45353006, rms_ms: 4.52083483, attack_ms: 3.00541423, hold_ms: 28.1667156, release_ms: 775.371973, makeup_db: 3.09368 },
                    BandDyn { threshold_db: -17.2067219, ratio: 2.07399083, knee_db: 4.58861916, rms_ms: 13.1502117, attack_ms: 7.41832411, hold_ms: 29.2237364, release_ms: 491.367101, makeup_db: 2.27892 },
                    BandDyn { threshold_db: -25.3302816, ratio: 1.92282122, knee_db: 0.698420273, rms_ms: 9.65602782, attack_ms: 8.47740331, hold_ms: 6.39407199, release_ms: 362.595944, makeup_db: 2.7508 },
                ],
                low_step: LowStepParams { enabled: true, hp_hz: 125.325885, threshold_db: -14.3588615, gr_db: -1.40409071, rms_ms: 60.0, lp_hz: 200.0, knee_db: 0.0, hysteresis_db: 1.0 },
            },
            side: ChannelParams {
                eq: [EqBand { kind: EqKind::HighShelf, freq_hz: 3004.16409, gain_db: 3.0, q: 0.380755954 }, EqBand::OFF],
                clip: SoftClipParams { enabled: true, threshold_db: -0.194364174, range_db: 6.21550916, slope: 0.886980956, hardness: 2.06036252 },
                bands: [
                    BandDyn { threshold_db: -18.3655514, ratio: 3.78070995, knee_db: 0.0100526942, rms_ms: 10.8885255, attack_ms: 3.50091185, hold_ms: 22.1496385, release_ms: 244.988455, makeup_db: 3.19790327 },
                    BandDyn { threshold_db: -22.1746869, ratio: 1.88291332, knee_db: 8.63689907, rms_ms: 9.38220864, attack_ms: 4.61091706, hold_ms: 23.665581, release_ms: 323.566685, makeup_db: 4.22537115 },
                    BandDyn { threshold_db: -30.0819424, ratio: 1.81890749, knee_db: 0.13984459, rms_ms: 12.4679971, attack_ms: 1.87058154, hold_ms: 21.0157547, release_ms: 142.926812, makeup_db: 7.08504335 },
                ],
                low_step: LowStepParams { enabled: true, hp_hz: 125.325885, threshold_db: -19.0222027, gr_db: -0.406070121, rms_ms: 60.0, lp_hz: 200.0, knee_db: 0.0, hysteresis_db: 1.0 },
            },
            limiter: LimiterParams {
                enabled: true,
                threshold_db: -1.06,
                true_peak: true,
                ceiling_db: 0.0,
                release_ms: 120.0,
                lookahead_ms: 0.6,
            },
            release_deadband_db: 0.0,
            clip_oversample: 1,
            release_deadband_floor_db: 0.0,
            output_gain_db: 0.0,
        }
        // END GENERATED
    }
}

impl Default for Params {
    fn default() -> Self {
        Self::up_and_wide()
    }
}
