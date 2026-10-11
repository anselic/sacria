//! Pure DSP engine for C-SANCTIS.
//!
//! Signal flow:
//! input gain -> linear-phase 3-band FIR split (L and R)
//! -> per band: power detector on the mono sum + soft-knee gain computer (gain on the whole band)
//! -> per band: sine shaper `sin(d*x)/d` on L and R -> M/S band gains -> sum
//! -> output stage (two-stage true-peak limiter, or a hard clipper) -> output gain.
//!
//! No allocation happens in `Engine::process`. All buffers are sized in `Engine::new`.

pub mod comp;
mod engine;
pub mod fir;
pub mod limiter;
pub mod params;

pub use engine::Engine;
pub use params::*;
