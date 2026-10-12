//! Parameters. `Params::default()` is the default voicing.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// The root note of the scale: index into `KEY_NAMES` (0 = C).
    pub key: usize,
    /// Which notes are let through: index into `SCALE_NAMES`.
    pub scale: usize,
    /// How far everything between the notes is turned down, 0..1. 1 = only the notes are left.
    pub amount: f32,
    /// How wide the gap around each note is, in cents.
    pub width_ct: f32,
    /// How long each note rings on after the input moves away (time to fall 60 dB).
    pub ring_ms: f32,
    /// Everything under this frequency passes untouched.
    pub low_keep_hz: f32,
    /// How much of the input stays in the output, 0..1. 0 = only what the engine added or
    /// took away (a null test built in).
    pub source: f32,
    /// Brings the level back up to the input level after the mask (up to 18 dB).
    pub level_lock: bool,
}

impl Default for Params {
    fn default() -> Self {
        // D# minor chord
        Self { key: 3, scale: 3, amount: 1.0, width_ct: 30.0, ring_ms: 400.0, low_keep_hz: 150.0, source: 1.0, level_lock: true }
    }
}
