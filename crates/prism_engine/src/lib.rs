//! Pure DSP engine for S-PRISM.
//!
//! Only what sits on the notes of a key and scale gets through, and it rings out at exactly
//! those notes. The rest is turned down. Color from any dense sound.
//!
//! Signal flow:
//! spectrum (STFT) -> note mask -> in-tune ring -> level lock -> source cut -> back to samples.
//!
//! No allocation happens in `Engine::process`. All buffers are sized in `Engine::new`.

mod engine;
pub mod params;

pub use engine::Engine;
pub use params::*;
pub use sacria_dsp::scale::{KEY_NAMES, SCALE_NAMES};
