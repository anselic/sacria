//! Cheap log/exp for the per-sample gain path. Errors are far below 0.001 dB.

const DB_PER_LOG2_POWER: f32 = 3.010_299_956_6; // 10*log10(2)
const LOG2_PER_DB_AMP: f32 = 0.166_096_404_7; // log2(10)/20

/// log2 for positive, normal floats.
#[inline]
pub fn log2(x: f32) -> f32 {
    let bits = x.to_bits();
    let e = ((bits >> 23) & 0xff) as i32 - 127;
    let m = f32::from_bits((bits & 0x007f_ffff) | 0x3f80_0000); // [1, 2)
    // log2(m) = 2/ln2 * atanh(t), t = (m-1)/(m+1) in [0, 1/3)
    let t = (m - 1.0) / (m + 1.0);
    let t2 = t * t;
    let p = t * (1.0 + t2 * (1.0 / 3.0 + t2 * (1.0 / 5.0 + t2 * (1.0 / 7.0 + t2 * (1.0 / 9.0)))));
    e as f32 + p * 2.885_390_081_8 // 2/ln(2)
}

/// 2^x for x in a sane audio range (about -120..120).
#[inline]
pub fn exp2(x: f32) -> f32 {
    // f32::round is a libm call without SSE4.1; the magic-number trick is plain SSE2.
    const MAGIC: f32 = 12_582_912.0; // 1.5 * 2^23
    let x = x.clamp(-126.0, 126.0);
    let i = (x + MAGIC) - MAGIC;
    let f = (x - i) * std::f32::consts::LN_2; // |f| <= 0.347
    let p = 1.0 + f * (1.0 + f * (0.5 + f * (1.0 / 6.0 + f * (1.0 / 24.0 + f * (1.0 / 120.0 + f * (1.0 / 720.0))))));
    p * f32::from_bits(((i as i32 + 127) as u32) << 23)
}

/// 10*log10 of a power value (mean square), input must be > 0 and normal.
#[inline]
pub fn power_to_db(ms: f32) -> f32 {
    DB_PER_LOG2_POWER * log2(ms)
}

/// dB (amplitude) to linear gain.
#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    exp2(db * LOG2_PER_DB_AMP)
}

/// sin(x) for any sane argument (|x| < 1e5). Max error about 1e-6.
#[inline]
pub fn sin(x: f32) -> f32 {
    const MAGIC: f32 = 12_582_912.0; // 1.5 * 2^23, round-to-nearest without libm
    // pi split in two parts so `x - k*pi` keeps its low bits
    const PI_A: f32 = 3.140_625;
    const PI_B: f32 = 9.676_535_9e-4;
    let k = (x * std::f32::consts::FRAC_1_PI + MAGIC) - MAGIC;
    let r = (x - k * PI_A) - k * PI_B; // |r| <= pi/2
    let r2 = r * r;
    let p = r * (1.0
        + r2 * (-1.0 / 6.0 + r2 * (1.0 / 120.0 + r2 * (-1.0 / 5040.0 + r2 * (1.0 / 362_880.0 + r2 * (-1.0 / 39_916_800.0))))));
    if (k as i32) & 1 == 0 {
        p
    } else {
        -p
    }
}

// ---- 8-lane versions (same math). Only SSE2-level ops: masks, int<->float, mul. ----

use wide::{f32x8, u32x8};

#[inline]
pub fn log2_x8(x: f32x8) -> f32x8 {
    let bits = x.to_bits();
    // exponent field as a float: (bits & exp_mask) is a multiple of 2^23, exact in f32
    let e_field = f32x8::from_i32x8((bits & u32x8::splat(0x7f80_0000)).cast_signed());
    let e = e_field * f32x8::splat(1.0 / 8_388_608.0) - f32x8::splat(127.0);
    let m = f32x8::from_bits((bits & u32x8::splat(0x007f_ffff)) | u32x8::splat(0x3f80_0000));
    let one = f32x8::splat(1.0);
    let t = (m - one) / (m + one);
    let t2 = t * t;
    let mut p = f32x8::splat(1.0 / 9.0);
    p = p * t2 + f32x8::splat(1.0 / 7.0);
    p = p * t2 + f32x8::splat(1.0 / 5.0);
    p = p * t2 + f32x8::splat(1.0 / 3.0);
    p = p * t2 + one;
    e + p * t * f32x8::splat(2.885_390_081_8)
}

#[inline]
pub fn exp2_x8(x: f32x8) -> f32x8 {
    const MAGIC: f32 = 12_582_912.0;
    let x = x.fast_max(f32x8::splat(-126.0)).fast_min(f32x8::splat(126.0));
    let i = (x + f32x8::splat(MAGIC)) - f32x8::splat(MAGIC);
    let f = (x - i) * f32x8::splat(std::f32::consts::LN_2);
    let mut p = f32x8::splat(1.0 / 720.0);
    p = p * f + f32x8::splat(1.0 / 120.0);
    p = p * f + f32x8::splat(1.0 / 24.0);
    p = p * f + f32x8::splat(1.0 / 6.0);
    p = p * f + f32x8::splat(0.5);
    p = p * f + f32x8::splat(1.0);
    p = p * f + f32x8::splat(1.0);
    // 2^i: build the exponent field by multiply + truncate, no shifts needed
    let field = ((i + f32x8::splat(127.0)) * f32x8::splat(8_388_608.0)).fast_trunc_int();
    p * f32x8::from_bits(field.cast_unsigned())
}

#[inline]
pub fn power_to_db_x8(ms: f32x8) -> f32x8 {
    log2_x8(ms) * f32x8::splat(DB_PER_LOG2_POWER)
}

#[inline]
pub fn db_to_gain_x8(db: f32x8) -> f32x8 {
    exp2_x8(db * f32x8::splat(LOG2_PER_DB_AMP))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simd_matches_scalar() {
        let xs = [1e-18f32, 3.7e-9, 0.001, 0.5, 1.0, 1.999, 7.3, 900.0];
        let ds = [-100.0f32, -37.2, -6.0, -0.01, 0.0, 0.3, 12.0, 39.9];
        let a = power_to_db_x8(f32x8::new(xs)).to_array();
        let b = db_to_gain_x8(f32x8::new(ds)).to_array();
        for i in 0..8 {
            assert!((a[i] - power_to_db(xs[i])).abs() < 1e-5);
            assert!((b[i] / db_to_gain(ds[i]) - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn sin_is_accurate() {
        let mut x = -40.0f32;
        while x < 40.0 {
            assert!((sin(x) - x.sin()).abs() < 2e-6, "{x}: {} vs {}", sin(x), x.sin());
            x += 0.0137;
        }
    }

    #[test]
    fn accurate_enough() {
        let mut x = 1e-18f32;
        while x < 1e3 {
            let want = 10.0 * x.log10();
            assert!((power_to_db(x) - want).abs() < 1e-4, "{x}");
            x *= 1.37;
        }
        let mut d = -100.0f32;
        while d < 40.0 {
            let want = 10f32.powf(d / 20.0);
            assert!(((db_to_gain(d) / want).log10() * 20.0).abs() < 1e-4, "{d}");
            d += 0.173;
        }
    }
}
