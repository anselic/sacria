//! Pure DSP engine for S-BLUR.
//!
//! Every partial gets its own attack and decay time. Long decay = each partial rings out
//! after it stops (a tail with no room in it). Long attack = hits fade in. Freeze holds the tone.
//!
//! Signal flow:
//! spectrum (STFT) -> per-bin attack and decay -> ringing bins keep their own pitch
//! -> source cut -> back to samples.
//!
//! No allocation happens in `Engine::process`. All buffers are sized in `Engine::new`.

mod engine;
pub mod params;

pub use engine::Engine;
pub use params::*;
