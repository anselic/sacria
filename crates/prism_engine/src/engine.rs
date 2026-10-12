use crate::params::Params;
use sacria_dsp::rng::Rng;
use sacria_dsp::scale::{hz_to_midi, midi_to_hz, Scale};
use sacria_dsp::stft::{lock_level, power, Stft};
use std::f32::consts::TAU;

struct Chan {
    held: Vec<f32>,
    phase: Vec<f32>,
    /// the input frame, kept for the source cut
    dry_re: Vec<f32>,
    dry_im: Vec<f32>,
    rng: Rng,
}

pub struct Engine {
    stft: Stft,
    ch: [Chan; 2],
    /// 1 on a scale note, falls to 0 between notes
    tooth: Vec<f32>,
    /// the scale note each bin rings at (Hz)
    ring_hz: Vec<f32>,
    built_for: Option<(Scale, f32)>,
    params: Params,
}

impl Engine {
    pub fn new(sample_rate: f32, params: Params) -> Self {
        let stft = Stft::new(sample_rate);
        let b = stft.bins;
        let chan = |seed| Chan {
            held: vec![0.0; b],
            phase: vec![0.0; b],
            dry_re: vec![0.0; b],
            dry_im: vec![0.0; b],
            rng: Rng::new(seed),
        };
        Self { ch: [chan(5), chan(9)], tooth: vec![1.0; b], ring_hz: vec![0.0; b], built_for: None, stft, params }
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
        }
    }

    /// In place.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let Self { stft, ch, tooth, ring_hz, built_for, params: p } = self;
        let (bin_hz, sr, hop, dt) = (stft.bin_hz(), stft.sr, stft.hop as f32, stft.dt());
        let scale = Scale::new(p.key as f32, p.scale as f32);
        if *built_for != Some((scale, p.width_ct)) {
            *built_for = Some((scale, p.width_ct));
            for k in 1..tooth.len() {
                let m = hz_to_midi(k as f32 * bin_hz);
                let note = scale.nearest(m);
                // a bin is wide at the bottom: never ask for less than the bin itself
                let bin_cents = 1200.0 * (1.0 + 0.5 / k as f32).log2();
                let d = (m - note).abs() * 100.0 / p.width_ct.max(bin_cents);
                tooth[k] = (-0.5 * d * d).exp();
                ring_hz[k] = midi_to_hz(note);
            }
        }
        let depth = p.amount;
        let fall = if p.ring_ms < 1.0 { 0.0 } else { 10f32.powf(-3.0 * dt / (p.ring_ms * 0.001)) };
        let low = ((p.low_keep_hz / bin_hz).ceil() as usize).max(1);
        let cut = 1.0 - p.source;
        let lock = p.level_lock;
        stft.process(left, right, None, |c, re, im, _| {
            let st = &mut ch[c];
            let level = power(re, im);
            if cut > 0.0 {
                st.dry_re.copy_from_slice(re);
                st.dry_im.copy_from_slice(im);
            }
            for k in low..re.len() {
                let g = 1.0 - depth * (1.0 - tooth[k]);
                let mag = (re[k] * re[k] + im[k] * im[k]).sqrt() * g;
                // only bins on a note are allowed to ring
                let held = (st.held[k] * fall * tooth[k]).max(mag);
                st.held[k] = held;
                if mag >= 0.7 * held {
                    st.phase[k] = im[k].atan2(re[k]);
                    re[k] *= g;
                    im[k] *= g;
                } else {
                    let mut ph = st.phase[k] + TAU * hop * ring_hz[k] / sr + 0.2 * st.rng.bipolar();
                    ph -= TAU * (ph / TAU).round();
                    st.phase[k] = ph;
                    re[k] = held * ph.cos();
                    im[k] = held * ph.sin();
                }
            }
            if lock {
                lock_level(re, im, level, 18.0);
            }
            if cut > 0.0 {
                for k in 0..re.len() {
                    re[k] -= cut * st.dry_re[k];
                    im[k] -= cut * st.dry_im[k];
                }
            }
        });
    }
}
