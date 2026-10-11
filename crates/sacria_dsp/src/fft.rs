//! Small radix-2 FFT for displays (spectrum analyzers). Not for the audio path.

use std::f64::consts::PI;

/// Complex FFT of a fixed power-of-two size. `new` allocates, `forward` does not.
pub struct Fft {
    n: usize,
    /// cos / sin of -2*pi*k/n for k < n/2
    cos: Vec<f32>,
    sin: Vec<f32>,
    /// bit-reversed index of each position
    rev: Vec<u32>,
}

impl Fft {
    pub fn new(n: usize) -> Self {
        assert!(n.is_power_of_two() && n >= 2);
        let bits = n.trailing_zeros();
        Self {
            n,
            cos: (0..n / 2).map(|k| (-2.0 * PI * k as f64 / n as f64).cos() as f32).collect(),
            sin: (0..n / 2).map(|k| (-2.0 * PI * k as f64 / n as f64).sin() as f32).collect(),
            rev: (0..n as u32).map(|i| i.reverse_bits() >> (32 - bits)).collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    /// In-place forward transform, no scaling: a sine of amplitude 1 on bin k gives |X[k]| = n/2.
    pub fn forward(&self, re: &mut [f32], im: &mut [f32]) {
        let n = self.n;
        assert!(re.len() == n && im.len() == n);
        for i in 0..n {
            let j = self.rev[i] as usize;
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let (half, step) = (len / 2, n / len);
            for start in (0..n).step_by(len) {
                for k in 0..half {
                    let (c, s) = (self.cos[k * step], self.sin[k * step]);
                    let (a, b) = (start + k, start + k + half);
                    let (tr, ti) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len *= 2;
        }
    }
}

/// Hann window of length `n` (periodic form, for spectrum analysis).
pub fn hann(n: usize) -> Vec<f32> {
    (0..n).map(|i| (0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos()) as f32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_lands_on_its_bin() {
        let n = 1024;
        let fft = Fft::new(n);
        let k = 37;
        let mut re: Vec<f32> = (0..n).map(|i| (2.0 * PI * k as f64 * i as f64 / n as f64).sin() as f32).collect();
        let mut im = vec![0.0f32; n];
        fft.forward(&mut re, &mut im);
        for b in 0..n / 2 {
            let mag = (re[b] * re[b] + im[b] * im[b]).sqrt();
            let want = if b == k { n as f32 / 2.0 } else { 0.0 };
            assert!((mag - want).abs() < 0.02, "bin {b}: {mag}");
        }
    }

    #[test]
    fn impulse_is_flat() {
        let fft = Fft::new(64);
        let (mut re, mut im) = (vec![0.0f32; 64], vec![0.0f32; 64]);
        re[5] = 1.0;
        fft.forward(&mut re, &mut im);
        for b in 0..64 {
            assert!(((re[b] * re[b] + im[b] * im[b]).sqrt() - 1.0).abs() < 1e-5);
        }
    }
}
