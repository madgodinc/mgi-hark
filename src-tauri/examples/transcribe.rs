//! Runs the live pipeline on a WAV file at real-time chunk sizes, without
//! waiting in real time. Checks VAD and recognition tuning offline.
//!   cargo run --release --example transcribe -- <models dir> <file.wav> [ru|en]

use mgi_hark_lib::{asr, models};
use std::sync::mpsc;

struct Print(std::time::Instant);

impl asr::Sink for Print {
    fn caption(&self, c: asr::Caption) {
        let kind = if c.is_final { "FINAL" } else { "draft" };
        println!("{:>6.2}s  #{} {kind}  {}", self.0.elapsed().as_secs_f32(), c.id, c.text);
    }
    fn level(&self, _: f32) {}
    fn ready(&self) {}
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let lang = models::Lang::parse(args.get(3).map(String::as_str).unwrap_or("ru"));
    let paths = models::paths(std::path::Path::new(&args[1]), lang);
    let wave = sherpa_onnx::Wave::read(&args[2]).expect("wav");
    assert_eq!(wave.sample_rate(), 16000);
    let (tx, rx) = mpsc::channel();
    let samples = wave.samples().to_vec();
    std::thread::spawn(move || {
        for chunk in samples.chunks(160) {
            tx.send(asr::Msg::Audio(chunk.to_vec())).unwrap();
        }
        // trailing silence so the last phrase closes
        for _ in 0..200 {
            tx.send(asr::Msg::Audio(vec![0.0; 160])).unwrap();
        }
    });
    let _ = asr::run(paths, rx, Print(std::time::Instant::now()));
}
