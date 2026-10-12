//! Parameters. `Params::default()` is the default voicing.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// How slowly each partial fades in. 0 = hits stay sharp.
    pub attack_ms: f32,
    /// How long each partial rings after the input lets go of it (time to fall 60 dB).
    pub decay_ms: f32,
    /// Random phase on the ringing partials, 0..1. 0 = a clean tone, 1 = noise.
    pub scatter: f32,
    /// How much of the input stays in the output, 0..1. 0 = only what the engine added or
    /// took away (a null test built in).
    pub source: f32,
    /// Holds the tone as it is and ignores the input.
    pub freeze: bool,
}

impl Default for Params {
    fn default() -> Self {
        Self { attack_ms: 0.0, decay_ms: 600.0, scatter: 0.2, source: 1.0, freeze: false }
    }
}
