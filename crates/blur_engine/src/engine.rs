use crate::params::Params;
use sacria_dsp::rng::Rng;
use sacria_dsp::stft::Stft;
use std::f32::consts::{PI, TAU};

struct Chan {
    held: Vec<f32>,
    last_phase: Vec<f32>,
    freq: Vec<f32>,
    phase: Vec<f32>,
    rng: Rng,
}

fn chan(bins: usize, bin_hz: f32, seed: u32) -> Chan {
    Chan {
        held: vec![0.0; bins],
        last_phase: vec![0.0; bins],
        freq: (0..bins).map(|k| k as f32 * bin_hz).collect(),
        phase: vec![0.0; bins],
        rng: Rng::new(seed),
    }
}

pub struct Engine {
    stft: Stft,
    ch: [Chan; 2],
    params: Params,
}

impl Engine {
    pub fn new(sample_rate: f32, params: Params) -> Self {
        let stft = Stft::new(sample_rate);
        Self { ch: [chan(stft.bins, stft.bin_hz(), 11), chan(stft.bins, stft.bin_hz(), 23)], stft, params }
    }

    pub fn set_params(&mut self, p: &Params) {
        self.params = *p;
    }

    pub fn params(&self) -> &Params {
        &self.params
    }

    pub fn sample_rate(&self) -> f32 {
        self.stft.sr
    }

    /// Delay of the output in samples.
    pub fn latency_samples(&self) -> usize {
        self.stft.n
    }

    pub fn reset(&mut self) {
        self.stft.reset();
        for c in self.ch.iter_mut() {
            c.held.fill(0.0);
            c.last_phase.fill(0.0);
        }
    }

    /// In place.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let Self { stft, ch, params: p } = self;
        let (n, hop, bin_hz, sr, dt) = (stft.n as f32, stft.hop as f32, stft.bin_hz(), stft.sr, stft.dt());
        // attack: share of the gap closed per frame. decay: the time to fall 60 dB.
        let rise = if p.attack_ms < 1.0 { 1.0 } else { 1.0 - (-dt / (p.attack_ms * 0.001)).exp() };
        let fall = if p.decay_ms < 1.0 { 0.0 } else { 10f32.powf(-3.0 * dt / (p.decay_ms * 0.001)) };
        let scatter = p.scatter * PI;
        let freeze = p.freeze;
        let cut = 1.0 - p.source;
        stft.process(left, right, None, |c, re, im, _| {
            let st = &mut ch[c];
            for k in 0..re.len() {
                let mag = (re[k] * re[k] + im[k] * im[k]).sqrt();
                let ph = im[k].atan2(re[k]);
                let mut d = ph - st.last_phase[k] - TAU * hop * k as f32 / n;
                d -= TAU * (d / TAU).round();
                st.last_phase[k] = ph;

                let held = &mut st.held[k];
                if !freeze {
                    *held = if mag > *held { *held + (mag - *held) * rise } else { (*held * fall).max(mag) };
                }
                // the input still drives this partial: keep its phase, remember where it rings
                if !freeze && mag >= 0.7 * *held {
                    st.freq[k] = (k as f32 + d * n / (TAU * hop)) * bin_hz;
                    st.phase[k] = ph;
                } else {
                    let mut next = st.phase[k] + TAU * hop * st.freq[k] / sr + scatter * st.rng.bipolar();
                    next -= TAU * (next / TAU).round();
                    st.phase[k] = next;
                }
                let (dry_re, dry_im) = (re[k], im[k]);
                re[k] = *held * st.phase[k].cos() - cut * dry_re;
                im[k] = *held * st.phase[k].sin() - cut * dry_im;
            }
        });
    }
}
