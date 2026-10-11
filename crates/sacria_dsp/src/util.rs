//! Small helpers shared by the DSP blocks.

#[inline]
pub fn db_to_lin(db: f32) -> f32 {
    (db * (std::f32::consts::LN_10 / 20.0)).exp()
}

/// One-pole coefficient for a time constant. 0 ms means "instant".
pub fn time_coef(ms: f32, sr: f32) -> f32 {
    if ms <= 0.0 {
        0.0
    } else {
        (-1.0 / (ms as f64 * 1e-3 * sr as f64)).exp() as f32
    }
}

/// Soft-knee gain reduction (dB, <= 0) for a detector level in dB.
/// `slope` = 1/ratio - 1. Scalar (branchy) reference version.
#[inline]
pub fn gain_reduction_db(level_db: f32, threshold_db: f32, slope: f32, knee_db: f32) -> f32 {
    let d = level_db - threshold_db;
    if 2.0 * d <= -knee_db {
        0.0
    } else if 2.0 * d >= knee_db {
        slope * d
    } else {
        let t = d + 0.5 * knee_db;
        slope * t * t / (2.0 * knee_db)
    }
}
