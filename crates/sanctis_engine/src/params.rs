//! Parameters. `Params::reference()` is the default voicing.

pub const NUM_BANDS: usize = 3;

/// One lowpass of the band split. The target magnitude is `1 / (1 + (f/freq)^(2*order))`;
/// its zero-phase impulse response is cut to the split length with a Kaiser window.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SplitFilter {
    pub freq_hz: f32,
    /// Steepness of the target (2 = Linkwitz-Riley 4 magnitude).
    pub order: f32,
    /// Kaiser window beta. 0 = no window (plain cut), big = short, smooth filter.
    pub kaiser_beta: f32,
}

/// Linear-phase band split: low = LP(lo), high = input - LP(hi), mid = the rest.
/// The bands always sum back to the (delayed) input.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SplitParams {
    pub lo: SplitFilter,
    pub hi: SplitFilter,
    /// Length of the FIR filters. Half of it is latency.
    pub length_ms: f32,
}

/// Curve of the shaper. Both are the same below the top of the sine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Shape {
    /// `sin(d*x)/d`: hot peaks fold back toward zero.
    #[default]
    Sine,
    /// The sine up to its top, then flat: hot peaks are held at `1/d`.
    Soft,
}

/// What a band's compressor listens to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Detect {
    /// The mono sum (L+R)/2. Pure side content never triggers the compressor.
    #[default]
    Mid,
    /// The louder of left and right.
    Stereo,
}

/// One band: compressor, shaper, output gains.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BandParams {
    /// Threshold on the detector level (dBFS; a steady sine reads its peak level).
    pub threshold_db: f32,
    pub ratio: f32,
    /// Full knee width in dB.
    pub knee_db: f32,
    /// Detector time constants (one-pole on the power of the band's mono sum).
    pub attack_ms: f32,
    pub release_ms: f32,
    /// Parallel compression: 1 = only the compressed band, 0 = only the uncompressed one.
    #[cfg_attr(feature = "serde", serde(default = "one_f32"))]
    pub blend: f32,
    /// Shaper drive `d` in `sin(d*x)/d`. 0 = no saturation.
    pub drive: f32,
    /// Gain on the band's mid and side signal, after the shaper (dB).
    pub mid_gain_db: f32,
    pub side_gain_db: f32,
    #[cfg_attr(feature = "serde", serde(default))]
    pub shape: Shape,
    #[cfg_attr(feature = "serde", serde(default))]
    pub detect: Detect,
}

/// What the output stage does with peaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum OutputMode {
    /// Nothing: no drive, no ceiling (still delayed).
    Off,
    /// The two-stage true-peak limiter.
    #[default]
    Limit,
    /// Hard clip: samples above the ceiling are cut flat. No gain riding, so no pumping,
    /// but it distorts, and peaks between samples can pass the ceiling.
    Clip,
}

/// Output stage: two-stage true-peak limiter (output = input * input_gain * ceiling / envelope)
/// or a hard clipper (output = clamp(input * input_gain) * ceiling).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LimiterParams {
    #[cfg_attr(feature = "serde", serde(default))]
    pub mode: OutputMode,
    /// Gain into the limiter or clipper. Limiting starts at 0 dBFS true peak after this gain.
    pub input_gain_db: f32,
    /// Everything is scaled by this after limiting (dBFS).
    pub ceiling_db: f32,
    /// Fast stage: linear ramp down to each peak over this time (= look-ahead).
    pub lookahead_ms: f32,
    /// Fast stage: linear ramp back after a peak.
    pub release_ramp_ms: f32,
    /// Slow stage: follower of the fast envelope.
    pub slow_attack_ms: f32,
    pub slow_release_ms: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Params {
    pub input_gain_db: f32,
    pub split: SplitParams,
    /// false = the bands pass through untouched (still split and delayed).
    pub character_enabled: bool,
    /// Low, mid, high.
    pub bands: [BandParams; NUM_BANDS],
    pub limiter: LimiterParams,
    /// Oversampling of the shapers: 1 (off) or 2. Cuts the aliasing of the
    /// harmonics; costs CPU and a few samples of phase shift in the highs.
    #[cfg_attr(feature = "serde", serde(default = "one_u32"))]
    pub shaper_oversample: u32,
    pub output_gain_db: f32,
}

#[cfg(feature = "serde")]
fn one_u32() -> u32 {
    1
}

#[cfg(feature = "serde")]
fn one_f32() -> f32 {
    1.0
}

impl Params {
    /// The measured default voicing.
    pub fn reference() -> Self {
        Self {
            input_gain_db: 0.0,
            split: SplitParams {
                lo: SplitFilter { freq_hz: 161.2635, order: 3.33351, kaiser_beta: 2.34543 },
                hi: SplitFilter { freq_hz: 2509.422, order: 2.15333, kaiser_beta: 36.62993 },
                length_ms: 501.0 / 48.0,
            },
            character_enabled: true,
            bands: [
                BandParams { threshold_db: -9.667, ratio: 1.180, knee_db: 29.114, attack_ms: 1.502, release_ms: 2010.2, blend: 1.0, drive: 2.2358, mid_gain_db: 2.802, side_gain_db: 2.804, shape: Shape::Sine, detect: Detect::Mid },
                BandParams { threshold_db: -27.656, ratio: 1.269, knee_db: 27.812, attack_ms: 214.25, release_ms: 91.05, blend: 1.0, drive: 2.3933, mid_gain_db: 3.583, side_gain_db: 3.595, shape: Shape::Sine, detect: Detect::Mid },
                BandParams { threshold_db: -12.968, ratio: 1.616, knee_db: 0.05, attack_ms: 3.103, release_ms: 89.31, blend: 1.0, drive: 1.8086, mid_gain_db: 1.026, side_gain_db: 4.052, shape: Shape::Sine, detect: Detect::Mid },
            ],
            limiter: LimiterParams {
                mode: OutputMode::Limit,
                input_gain_db: 1.994,
                ceiling_db: -0.1,
                lookahead_ms: 21.0 / 48.0,
                release_ramp_ms: 22.0 / 48.0,
                slow_attack_ms: 131.5,
                slow_release_ms: 231.8,
            },
            shaper_oversample: 1,
            output_gain_db: 0.0,
        }
    }
}

impl Default for Params {
    fn default() -> Self {
        Self::reference()
    }
}
