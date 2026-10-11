//! CPU benchmark on 60 s of stereo noise, 512-sample blocks.
//!
//! cargo run --release --example bench

use vepres_engine::{Engine, Params};
use std::time::Instant;

fn run(name: &str, params: Params) {
    let sr = 48000.0;
    let n = 60 * 48000;
    let mut seed = 1u32;
    let mut noise = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        ((seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0) * 0.3
    };
    let mut l: Vec<f32> = (0..n).map(|_| noise()).collect();
    let mut r: Vec<f32> = (0..n).map(|_| noise()).collect();
    let mut e = Engine::new(sr, params);
    let t0 = Instant::now();
    for (cl, cr) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
        e.process(cl, cr);
    }
    let dt = t0.elapsed().as_secs_f64();
    println!("{name:16} {:7.1} ns/sample  {:6.0}x realtime  {:.3}% of one core", dt * 1e9 / n as f64, 60.0 / dt, dt / 60.0 * 100.0);
    std::hint::black_box(&l);
}

fn part(name: &str, mut f: impl FnMut(f32) -> f32) {
    let n = 10_000_000;
    let mut acc = 0.0f32;
    let t0 = Instant::now();
    for i in 0..n {
        acc += f(((i * 7919) % 1000) as f32 * 1e-3 - 0.5);
    }
    let dt = t0.elapsed().as_secs_f64();
    println!("  part {name:12} {:6.1} ns/call  ({acc})", dt * 1e9 / n as f64);
}

fn main() {
    use vepres_engine::{biquad::*, crossover::*, dynamics::*};
    part("calib mul", |x| x * 1.0001);
    part("log2", |x| vepres_engine::fastmath::log2(x + 1.0));
    part("exp2", |x| vepres_engine::fastmath::exp2(x));
    part("std exp2", |x| x.exp2());
    part("clamp", |x| x.clamp(-0.3, 0.3));
    let p = Params::up_and_wide();
    let mut bq = Biquad::default();
    bq.c = Coeffs::from_band(&p.mid.eq[0], 48000.0);
    part("biquad", |x| bq.tick(x));
    let mut xo = Crossover3::default();
    xo.set(454.0, 3150.0, 48000.0);
    part("crossover", |x| xo.tick(x)[2]);
    let mut d = DynBank::default();
    for k in 0..6 {
        d.set_lane(k, &p.mid.bands[k % 3], 48000.0);
    }
    part("dyn bank x6", |x| d.tick(&[x; LANES])[0]);
    part("db_to_gain", |x| vepres_engine::fastmath::db_to_gain(x * 10.0));
    run("full", p);
    let mut q = p;
    q.limiter.enabled = false;
    run("no limiter", q);
}
