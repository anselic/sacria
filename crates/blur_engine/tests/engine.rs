use blur_engine::{Engine, Params};

const SR: f32 = 48_000.0;

/// Two seconds: a few sines, noise, and a click every 300 ms. Then one second of silence.
fn test_signal() -> Vec<f32> {
    let n = SR as usize * 3;
    let mut seed = 12345u32;
    (0..n)
        .map(|i| {
            if i >= SR as usize * 2 {
                return 0.0;
            }
            let t = i as f32 / SR;
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = (seed >> 8) as f32 / 8_388_608.0 - 1.0;
            let tone: f32 = [55.0f32, 110.0, 440.0, 1230.0, 5000.0].iter().map(|f| (t * f * std::f32::consts::TAU).sin()).sum();
            let click = if i % 14_400 < 40 { 0.8 } else { 0.0 };
            0.1 * tone + 0.05 * noise + click
        })
        .collect()
}

fn run(p: Params) -> (Vec<f32>, Vec<f32>, usize) {
    let src = test_signal();
    let (mut l, mut r) = (src.clone(), src.clone());
    let mut e = Engine::new(SR, p);
    for (a, b) in l.chunks_mut(441).zip(r.chunks_mut(441)) {
        e.process(a, b);
    }
    (src, l, e.latency_samples())
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

fn check_sane(p: Params) {
    let (src, out, _) = run(p);
    assert!(out.iter().all(|v| v.is_finite()), "not finite with {p:?}");
    let db = 20.0 * (rms(&out[..96_000]) / rms(&src[..96_000])).log10();
    assert!(db > -30.0 && db < 12.0, "level {db:+.1} dB with {p:?}");
}

#[test]
fn defaults_are_sane() {
    check_sane(Params::default());
}

#[test]
fn extremes_are_sane() {
    check_sane(Params { attack_ms: 2000.0, decay_ms: 10000.0, scatter: 1.0, ..Params::default() });
}

/// Source at 0 = the normal output minus the input (delayed by the latency).
#[test]
fn source_at_zero_is_wet_minus_dry() {
    let (src, wet, lat) = run(Params::default());
    let (_, delta, _) = run(Params { source: 0.0, ..Params::default() });
    let mut worst = 0.0f32;
    for i in (lat + 4096)..src.len() {
        worst = worst.max((delta[i] - (wet[i] - src[i - lat])).abs());
    }
    assert!(worst < 2e-3, "off by {worst}");
    assert!(rms(&delta[..96_000]) > 1e-3, "delta is silent");
}
