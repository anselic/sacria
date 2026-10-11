//! Render a stereo WAV through the engine, latency removed.
//!
//! cargo run --release --example render -- in.wav out.wav

use vepres_engine::{Engine, Params};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: render <in.wav> <out.wav>");
        std::process::exit(2);
    }
    let mut reader = hound::WavReader::open(&args[1]).expect("open input");
    let spec = reader.spec();
    assert_eq!(spec.channels, 2, "need a stereo file");
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(Result::unwrap).collect(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.unwrap() as f32 * scale).collect()
        }
    };
    let n = samples.len() / 2;
    let mut engine = Engine::new(spec.sample_rate as f32, Params::up_and_wide());
    let lat = engine.latency_samples();

    // pad the end so the delayed tail comes out, then drop the latency at the start
    let mut l: Vec<f32> = samples.iter().step_by(2).copied().chain(std::iter::repeat(0.0).take(lat)).collect();
    let mut r: Vec<f32> = samples.iter().skip(1).step_by(2).copied().chain(std::iter::repeat(0.0).take(lat)).collect();

    let t0 = Instant::now();
    for (cl, cr) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
        engine.process(cl, cr);
    }
    let dt = t0.elapsed().as_secs_f64();
    let audio_s = n as f64 / spec.sample_rate as f64;
    eprintln!(
        "{:.1} s of audio in {:.3} s ({:.0}x realtime, {:.3}% of one core), latency {} samples",
        audio_s,
        dt,
        audio_s / dt,
        100.0 * dt / audio_s,
        lat
    );

    let out_spec = hound::WavSpec { channels: 2, sample_rate: spec.sample_rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(&args[2], out_spec).expect("create output");
    for i in lat..lat + n {
        w.write_sample(l[i]).unwrap();
        w.write_sample(r[i]).unwrap();
    }
    w.finalize().unwrap();
}
