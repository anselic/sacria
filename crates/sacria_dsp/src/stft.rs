//! Stereo overlap-add STFT (Hann in, Hann out, 75% overlap). Latency is exactly `n` samples.

use crate::fft::{hann, Fft};

pub struct Stft {
    pub n: usize,
    pub hop: usize,
    /// n / 2 + 1
    pub bins: usize,
    pub sr: f32,
    fft: Fft,
    win: Vec<f32>,
    /// 0, 1 = main in. 2 = sidechain (mono sum).
    inp: [Vec<f32>; 3],
    out: [Vec<f32>; 2],
    pos: usize,
    count: usize,
    re: Vec<f32>,
    im: Vec<f32>,
    sc_re: Vec<f32>,
    sc_im: Vec<f32>,
}

impl Stft {
    /// 2048 points up to 50 kHz sample rate, 4096 above.
    pub fn new(sr: f32) -> Self {
        let n = if sr > 50_000.0 { 4096 } else { 2048 };
        Self {
            n,
            hop: n / 4,
            bins: n / 2 + 1,
            sr,
            fft: Fft::new(n),
            win: hann(n),
            inp: [vec![0.0; n], vec![0.0; n], vec![0.0; n]],
            out: [vec![0.0; n], vec![0.0; n]],
            pos: 0,
            count: 0,
            re: vec![0.0; n],
            im: vec![0.0; n],
            sc_re: vec![0.0; n],
            sc_im: vec![0.0; n],
        }
    }

    pub fn bin_hz(&self) -> f32 {
        self.sr / self.n as f32
    }

    /// Seconds between two frames.
    pub fn dt(&self) -> f32 {
        self.hop as f32 / self.sr
    }

    /// Samples until the next frame runs (1..=hop).
    pub fn until_frame(&self) -> usize {
        self.hop - self.count
    }

    pub fn reset(&mut self) {
        for b in self.inp.iter_mut() {
            b.fill(0.0);
        }
        for b in self.out.iter_mut() {
            b.fill(0.0);
        }
        self.pos = 0;
        self.count = 0;
    }

    /// `f(channel, re, im, sidechain)` is called once per channel per hop with `bins` values.
    /// The sidechain spectrum is the mono sum of the sidechain input.
    pub fn process<F>(&mut self, l: &mut [f32], r: &mut [f32], sc: Option<(&[f32], &[f32])>, mut f: F)
    where
        F: FnMut(usize, &mut [f32], &mut [f32], Option<(&[f32], &[f32])>),
    {
        let n = self.n;
        for i in 0..l.len() {
            self.inp[0][self.pos] = l[i];
            self.inp[1][self.pos] = r[i];
            self.inp[2][self.pos] = sc.map_or(0.0, |(a, b)| 0.5 * (a[i] + b[i]));
            l[i] = self.out[0][self.pos];
            r[i] = self.out[1][self.pos];
            self.out[0][self.pos] = 0.0;
            self.out[1][self.pos] = 0.0;
            self.pos = (self.pos + 1) % n;
            self.count += 1;
            if self.count == self.hop {
                self.count = 0;
                self.frame(sc.is_some(), &mut f);
            }
        }
    }

    fn frame<F>(&mut self, has_sc: bool, f: &mut F)
    where
        F: FnMut(usize, &mut [f32], &mut [f32], Option<(&[f32], &[f32])>),
    {
        let (n, bins, pos) = (self.n, self.bins, self.pos);
        if has_sc {
            for i in 0..n {
                self.sc_re[i] = self.inp[2][(pos + i) % n] * self.win[i];
                self.sc_im[i] = 0.0;
            }
            self.fft.forward(&mut self.sc_re, &mut self.sc_im);
        }
        // Hann squared at 75% overlap sums to 1.5. The inverse FFT needs 1/n.
        let norm = 1.0 / (1.5 * n as f32);
        for ch in 0..2 {
            for i in 0..n {
                self.re[i] = self.inp[ch][(pos + i) % n] * self.win[i];
                self.im[i] = 0.0;
            }
            self.fft.forward(&mut self.re, &mut self.im);
            let sc = if has_sc { Some((&self.sc_re[..bins], &self.sc_im[..bins])) } else { None };
            f(ch, &mut self.re[..bins], &mut self.im[..bins], sc);
            // inverse = conj(forward(conj(X))); the spectrum of a real signal is mirrored
            self.im[0] = 0.0;
            self.im[n / 2] = 0.0;
            for k in 1..n / 2 {
                self.re[n - k] = self.re[k];
                self.im[n - k] = self.im[k];
                self.im[k] = -self.im[k];
            }
            self.fft.forward(&mut self.re, &mut self.im);
            for i in 0..n {
                self.out[ch][(pos + i) % n] += self.re[i] * self.win[i] * norm;
            }
        }
    }
}

/// Power of one frame.
pub fn power(re: &[f32], im: &[f32]) -> f32 {
    re.iter().zip(im).map(|(a, b)| a * a + b * b).sum()
}

/// Scale the frame so its power is `target` again (limited to +-`max_db`).
pub fn lock_level(re: &mut [f32], im: &mut [f32], target: f32, max_db: f32) {
    let now = power(re, im);
    if now < 1e-20 || target < 1e-20 {
        return;
    }
    let lim = 10f32.powf(max_db / 20.0);
    let g = (target / now).sqrt().clamp(1.0 / lim, lim);
    for (a, b) in re.iter_mut().zip(im.iter_mut()) {
        *a *= g;
        *b *= g;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untouched_frames_give_the_input_back_n_samples_late() {
        let mut s = Stft::new(48_000.0);
        let n = s.n;
        let len = n * 6;
        let src: Vec<f32> = (0..len).map(|i| ((i * 7919) % 1000) as f32 / 500.0 - 1.0).collect();
        let (mut l, mut r) = (src.clone(), src.clone());
        // odd block sizes on purpose
        let mut at = 0;
        for size in [1usize, 63, 500, 1024, 77].iter().cycle() {
            let end = (at + size).min(len);
            s.process(&mut l[at..end], &mut r[at..end], None, |_, _, _, _| {});
            at = end;
            if at == len {
                break;
            }
        }
        for i in 2 * n..len {
            assert!((l[i] - src[i - n]).abs() < 1e-3, "sample {i}: {} vs {}", l[i], src[i - n]);
        }
    }
}
