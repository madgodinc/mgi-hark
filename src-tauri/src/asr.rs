//! Speech to text.
//!
//! Silero VAD cuts the stream into phrases. While a phrase is still being
//! spoken, GigaAM re-reads the audio so far and the overlay shows a draft that
//! grows word by word. When the phrase ends, the same model reads the finished
//! segment once more and that text, with punctuation, replaces the draft.
//! GigaAM reads 11 s of speech in about 0.45 s on a desktop CPU and Parakeet
//! (English) 7 s in 0.12 s, which is what makes one model enough for both passes.

use crate::audio::RATE;
use crate::models::{Model, ModelPaths};
use serde::Serialize;
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, VadModelConfig, VoiceActivityDetector};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// Silero wants exactly this many samples per call at 16 kHz.
const WINDOW: usize = 512;
/// Audio kept before speech is detected, so the first syllable is not cut.
const PREROLL_WINDOWS: usize = 32;

pub enum Msg {
    Audio(Vec<f32>),
    /// Listening stopped or switched: drop the half-heard phrase.
    Reset,
}

#[derive(Serialize, Clone)]
pub struct Caption {
    pub id: u64,
    pub text: String,
    #[serde(rename = "final")]
    pub is_final: bool,
}

pub trait Sink: Send + 'static {
    fn caption(&self, caption: Caption);
    /// Loudness 0..1 of the incoming audio, a few times per second.
    fn level(&self, level: f32);
    /// Models are loaded and audio is being listened for.
    fn ready(&self);
}

fn load(paths: &ModelPaths) -> anyhow::Result<(VoiceActivityDetector, OfflineRecognizer)> {
    let mut vad = VadModelConfig::default();
    vad.silero_vad.model = Some(paths.vad.clone());
    vad.silero_vad.threshold = std::env::var("HARK_VAD_THRESHOLD").ok().and_then(|v| v.parse().ok()).unwrap_or(0.4);
    // Speakers pause between words; shorter than this would cut a phrase into
    // one-word pieces that the model cannot read on their own.
    vad.silero_vad.min_silence_duration = 0.5;
    vad.silero_vad.min_speech_duration = 0.2;
    // A monologue is split so the text keeps arriving in readable pieces.
    vad.silero_vad.max_speech_duration = 8.0;
    vad.silero_vad.window_size = WINDOW as i32;
    vad.sample_rate = RATE;
    vad.num_threads = 1;
    let vad = VoiceActivityDetector::create(&vad, 60.0)
        .ok_or_else(|| anyhow::anyhow!("не удалось загрузить детектор речи"))?;

    let mut asr = OfflineRecognizerConfig::default();
    match &paths.model {
        Model::Transducer { encoder, decoder, joiner } => {
            asr.model_config.transducer.encoder = Some(encoder.clone());
            asr.model_config.transducer.decoder = Some(decoder.clone());
            asr.model_config.transducer.joiner = Some(joiner.clone());
            asr.model_config.model_type = Some("nemo_transducer".into());
        }
        Model::NemoCtc { model } => asr.model_config.nemo_ctc.model = Some(model.clone()),
    }
    asr.model_config.tokens = Some(paths.tokens.clone());
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    asr.model_config.num_threads = (threads / 2).clamp(1, 4) as i32;
    let asr = OfflineRecognizer::create(&asr)
        .ok_or_else(|| anyhow::anyhow!("не удалось загрузить модель распознавания"))?;
    Ok((vad, asr))
}

fn decode(asr: &OfflineRecognizer, samples: &[f32]) -> String {
    let stream = asr.create_stream();
    stream.accept_waveform(RATE, samples);
    asr.decode(&stream);
    stream.get_result().map(|r| r.text.trim().to_string()).unwrap_or_default()
}

pub fn run(paths: ModelPaths, rx: Receiver<Msg>, sink: impl Sink) -> anyhow::Result<()> {
    let (vad, asr) = load(&paths)?;
    sink.ready();

    let mut buffer: Vec<f32> = Vec::new();
    let mut fed = 0usize;
    let mut speaking = false;
    let mut id = 0u64;
    let mut last_draft = String::new();
    let mut next_draft = Instant::now();
    let mut draft_every = Duration::from_millis(300);
    let mut level_peak = 0f32;
    let mut next_level = Instant::now();

    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Msg::Audio(samples)) => {
                for s in &samples {
                    level_peak = level_peak.max(s.abs());
                }
                buffer.extend_from_slice(&samples);
            }
            Ok(Msg::Reset) => {
                vad.reset();
                buffer.clear();
                fed = 0;
                if speaking && !last_draft.is_empty() {
                    // Leave what was heard on screen instead of a dangling draft.
                    sink.caption(Caption { id, text: last_draft.clone(), is_final: true });
                }
                speaking = false;
                last_draft.clear();
                continue;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }

        if next_level <= Instant::now() {
            sink.level(level_peak.min(1.0));
            level_peak = 0.0;
            next_level = Instant::now() + Duration::from_millis(120);
        }

        while fed + WINDOW <= buffer.len() {
            vad.accept_waveform(&buffer[fed..fed + WINDOW]);
            fed += WINDOW;
            if !speaking && vad.detected() {
                speaking = true;
                id += 1;
                last_draft.clear();
                next_draft = Instant::now() + draft_every;
            }
        }

        if !speaking && buffer.len() > PREROLL_WINDOWS * WINDOW {
            let cut = buffer.len() - PREROLL_WINDOWS * WINDOW;
            buffer.drain(..cut);
            fed = fed.saturating_sub(cut);
        }

        if speaking && Instant::now() >= next_draft {
            let started = Instant::now();
            let text = decode(&asr, &buffer);
            // Keep drafts from eating the CPU on slow machines: wait at least
            // two decode times before the next one.
            draft_every = (started.elapsed() * 2).clamp(Duration::from_millis(250), Duration::from_millis(1500));
            next_draft = Instant::now() + draft_every;
            if !text.is_empty() && text != last_draft {
                last_draft = text.clone();
                sink.caption(Caption { id, text, is_final: false });
            }
        }

        let mut first = true;
        while let Some(segment) = vad.front() {
            vad.pop();
            let first_of_phrase = first && speaking;
            if !first || !speaking {
                // A phrase nobody saw start, or the second one finished in the
                // same pass, gets its own line instead of overwriting the last.
                id += 1;
            }
            first = false;
            // Our buffer holds the same phrase plus the pre-roll, which keeps
            // the first syllable that VAD needs a moment to notice.
            let samples = if first_of_phrase { &buffer[..] } else { segment.samples() };
            let text = decode(&asr, samples);
            if !text.is_empty() {
                sink.caption(Caption { id, text, is_final: true });
            } else if !last_draft.is_empty() {
                // The final pass heard nothing: withdraw the draft.
                sink.caption(Caption { id, text: String::new(), is_final: true });
            }
            buffer.clear();
            fed = 0;
            speaking = false;
            last_draft.clear();
        }
    }
}
