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
use sherpa_onnx::{
    AudioTagging, AudioTaggingConfig, OfflineRecognizer, OfflineRecognizerConfig, VadModelConfig, VoiceActivityDetector,
};
use std::collections::HashMap;
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
    /// A non-speech sound was heard: a key like "laughter" or "music".
    fn sound(&self, key: &'static str);
}

/// AudioSet class names to the sound keys shown as caption tags.
fn sound_key(label: &str) -> Option<&'static str> {
    Some(match label {
        "Laughter" | "Giggle" | "Snicker" | "Belly laugh" | "Chuckle, chortle" => "laughter",
        "Screaming" | "Shout" | "Yell" | "Children shouting" => "shouting",
        "Crying, sobbing" => "crying",
        "Singing" => "singing",
        "Whistling" => "whistling",
        "Cough" => "cough",
        "Cheering" | "Applause" => "cheering",
        "Music" => "music",
        "Explosion" | "Boom" => "explosion",
        "Gunshot, gunfire" | "Machine gun" | "Fusillade" => "gunfire",
        "Siren" | "Police car (siren)" | "Ambulance (siren)" | "Fire engine, fire truck (siren)" | "Civil defense siren" => "siren",
        "Alarm" | "Alarm clock" | "Car alarm" | "Smoke detector, smoke alarm" => "alarm",
        "Bark" | "Dog" => "dog",
        "Meow" | "Cat" => "cat",
        "Knock" => "knock",
        "Telephone bell ringing" | "Ringtone" => "phone",
        "Shatter" | "Glass" => "glass",
        "Thunder" => "thunder",
        _ => return None,
    })
}

struct Tagger {
    tagging: AudioTagging,
    last: HashMap<&'static str, Instant>,
}

impl Tagger {
    fn load(paths: &ModelPaths) -> Option<Tagger> {
        let (model, labels) = crate::models::tagger_paths(&paths.models_dir)?;
        let mut config = AudioTaggingConfig::default();
        config.model.ced = Some(model);
        config.model.num_threads = 1;
        config.labels = Some(labels);
        config.top_k = 5;
        let tagging = AudioTagging::create(&config)?;
        crate::diag::line("sound tagger loaded");
        Some(Tagger { tagging, last: HashMap::new() })
    }

    /// The strongest recognisable sound in a clip, at most once per key per
    /// few seconds (music, which plays on and on, far less often).
    fn tag(&mut self, samples: &[f32], sink: &impl Sink) {
        let stream = self.tagging.create_stream();
        stream.accept_waveform(RATE, samples);
        let mut candidates: Vec<(&'static str, f32)> = self
            .tagging
            .compute(&stream, 8)
            .into_iter()
            .filter_map(|e| sound_key(&e.name).map(|k| (k, e.prob)))
            .filter(|(k, p)| *p >= if *k == "music" { 0.55 } else { 0.3 })
            .collect();
        // Background music is almost always there in games; anything else
        // heard over it matters more.
        candidates.sort_by(|a, b| (a.0 == "music").cmp(&(b.0 == "music")).then(b.1.total_cmp(&a.1)));
        for (key, _) in candidates {
            let gap = if key == "music" { 45 } else { 5 };
            if self.last.get(key).is_some_and(|t| t.elapsed() < Duration::from_secs(gap)) {
                continue;
            }
            self.last.insert(key, Instant::now());
            sink.sound(key);
            return;
        }
    }
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
    let mut tagger = Tagger::load(&paths);
    let mut next_tagger_look = Instant::now() + Duration::from_secs(30);
    // Audio heard while nobody speaks, tagged in 1.6 s pieces.
    let mut quiet: Vec<f32> = Vec::new();
    const QUIET_CLIP: usize = (RATE as usize * 16) / 10;

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
                if !speaking && tagger.is_some() {
                    quiet.extend_from_slice(&samples);
                }
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

        if tagger.is_none() && Instant::now() >= next_tagger_look {
            // Installs from before the tagger existed fetch it in the background.
            next_tagger_look = Instant::now() + Duration::from_secs(30);
            tagger = Tagger::load(&paths);
        }
        if quiet.len() >= QUIET_CLIP {
            let rms = (quiet.iter().map(|v| v * v).sum::<f32>() / quiet.len() as f32).sqrt();
            if rms > 0.01 {
                if let Some(t) = tagger.as_mut() {
                    t.tag(&quiet, &sink);
                }
            }
            quiet.clear();
        }
        if speaking {
            quiet.clear();
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
            // Laughter and shouting often open a "speech" segment of their own.
            if let Some(t) = tagger.as_mut() {
                t.tag(samples, &sink);
            }
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
