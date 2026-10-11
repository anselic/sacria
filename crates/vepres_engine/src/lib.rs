//! Pure DSP engine for the anselic compressor.
//!
//! Signal flow (per M and S channel):
//! input gain -> M/S encode -> EQ -> soft clip -> 3-band subtractive crossover
//! -> per-band RMS detector + gain computer (with look-ahead)
//! -> band gains + makeup -> sum -> M/S decode -> true-peak limiter -> output gain.
//!
//! No allocation happens in `Engine::process`. All buffers are sized in `Engine::new`.

pub mod dynamics;
mod engine;
pub mod params;

// shared blocks, kept at their old paths
pub use sacria_dsp::{biquad, clip, crossover, fastmath, limiter, oversample};

pub use engine::{Engine, MAX_LOOKAHEAD_MS};
pub use params::*;
