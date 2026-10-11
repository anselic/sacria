//! Rough timing of the engine and its parts on 60 s of noise.
//!
//! cargo run --release --example bench

use sanctis_engine::comp::BandComp;
use sanctis_engine::fir::FirSplit;
use sanctis_engine::limiter::Limiter;
use sanctis_engine::{Engine, Params};
use std::hint::black_box;
use std::time::Instant;

const SR: f32 = 48000.0;
const N: usize = 60 * 48000;

fn report(name: &str, dt: f64) {
    println!("{name:22} {:7.1} ns/sample  {:6.0}x realtime  {:.3}% of one core", dt * 1e9 / N as f64, 60.0 / dt, dt / 60.0 * 100.0);
}

fn main() {
    let mut seed = 1u32;
    let mut noise = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        ((seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0) * 0.5
    };
    let l: Vec<f32> = (0..N).map(|_| noise()).collect();
    let r: Vec<f32> = (0..N).map(|_| noise()).collect();
    let p = Params::reference();

    let mut e = Engine::new(SR, p);
    let (mut a, mut b) = (l.clone(), r.clone());
    let t = Instant::now();
    for (cl, cr) in a.chunks_mut(512).zip(b.chunks_mut(512)) {
        e.process(cl, cr);
    }
    report("engine (full)", t.elapsed().as_secs_f64());

    let mut split = [FirSplit::new(&p.split, SR), FirSplit::new(&p.split, SR)];
    let t = Instant::now();
    let mut acc = 0.0f32;
    for i in 0..N {
        acc += split[0].tick(l[i])[1] + split[1].tick(r[i])[1];
    }
    black_box(acc);
    report("  fir split (L+R)", t.elapsed().as_secs_f64());

    let mut comps = [BandComp::new(SR); 3];
    for (c, bp) in comps.iter_mut().zip(&p.bands) {
        c.set(bp, SR);
    }
    let t = Instant::now();
    let mut acc = 0.0f32;
    for i in 0..N {
        for c in &mut comps {
            acc += c.tick(l[i]);
        }
    }
    black_box(acc);
    report("  3 band comps", t.elapsed().as_secs_f64());

    let t = Instant::now();
    let mut acc = 0.0f32;
    for i in 0..N {
        for d in [2.2f32, 2.4, 1.8] {
            acc += sacria_dsp::fastmath::sin(d * l[i]) + sacria_dsp::fastmath::sin(d * r[i]);
        }
    }
    black_box(acc);
    report("  6 shapers", t.elapsed().as_secs_f64());

    let mut lim = Limiter::new(SR);
    lim.set(&p.limiter, SR);
    let t = Instant::now();
    let mut acc = 0.0f32;
    for i in 0..N {
        acc += lim.tick(2.0 * l[i], 2.0 * r[i]).0;
    }
    black_box(acc);
    report("  limiter", t.elapsed().as_secs_f64());
}
