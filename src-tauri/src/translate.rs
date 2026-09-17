//! Cloud translation.
//!
//! When a phrase ends, its audio goes to the service on madgodinc.net, which
//! recognises the speech in any of 99 languages and answers with the text in
//! the language the person reads. This is the only mode in which sound leaves
//! the computer, so it stays off until it is turned on; the service keeps
//! nothing, the audio lives in its memory only while the request runs.

use crate::audio::RATE;
use serde_json::{json, Value};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const URL: &str = "https://madgodinc.net/hark/api/translate";
/// Phrases waiting for the server. Speech outruns a slow network, and a
/// translation that arrives a minute late is worse than none.
const QUEUE: usize = 3;
/// Longer than any VAD segment; a stray buffer is not worth the traffic.
const MAX_SECONDS: usize = 20;

struct Job {
    id: u64,
    samples: Vec<f32>,
}

static WORKER: OnceLock<SyncSender<Job>> = OnceLock::new();

/// Hands the phrase to the translation thread, starting it on first use.
pub fn submit(app: &AppHandle, id: u64, samples: &[f32]) {
    if samples.is_empty() || samples.len() > RATE as usize * MAX_SECONDS {
        return;
    }
    let tx = WORKER.get_or_init(|| {
        let (tx, rx) = sync_channel(QUEUE);
        let app = app.clone();
        std::thread::Builder::new()
            .name("translate".into())
            .spawn(move || run(app, rx))
            .expect("spawn translate thread");
        tx
    });
    if let Err(TrySendError::Full(_)) = tx.try_send(Job { id, samples: samples.to_vec() }) {
        diag!("translation queue full, phrase skipped");
    }
}

fn run(app: AppHandle, rx: Receiver<Job>) {
    let client = match reqwest::blocking::Client::builder()
        .user_agent("mgi-hark")
        .timeout(Duration::from_secs(20))
        .build()
    {
        Ok(client) => client,
        Err(e) => return crate::report::error("translate", &e.to_string()),
    };
    let mut done = 0u64;
    for job in rx {
        // Read the settings per phrase: the mode can be turned off mid-session.
        let Some((target, game)) = crate::translation(&app) else {
            diag!("translate: turned off, phrase dropped");
            continue;
        };
        let started = Instant::now();
        match send(&client, &job, &target, &game) {
            Ok(answer) => {
                let text = answer.get("text").and_then(|v| v.as_str()).unwrap_or_default();
                if text.is_empty() {
                    continue;
                }
                done += 1;
                if done == 1 || done % 20 == 0 {
                    diag!("translated phrases: {done}, last took {} ms", started.elapsed().as_millis());
                }
                let _ = app.emit(
                    "translation",
                    json!({
                        "id": job.id,
                        "text": text,
                        "original": answer.get("original").and_then(|v| v.as_str()).unwrap_or_default(),
                        "lang": answer.get("lang").and_then(|v| v.as_str()).unwrap_or_default(),
                        "translated": answer.get("translated").and_then(|v| v.as_bool()).unwrap_or(false),
                    }),
                );
            }
            Err(e) => crate::report::error("translate", &e),
        }
    }
}

fn send(client: &reqwest::blocking::Client, job: &Job, target: &str, game: &str) -> Result<Value, String> {
    let install = crate::report::install();
    let response = client
        .post(format!("{URL}?target={target}&game={game}&install={install}"))
        .header("content-type", "audio/wav")
        .body(wav(&job.samples))
        .send()
        .map_err(|_| "нет связи с сервером перевода".to_string())?;
    match response.status().as_u16() {
        200 => response.json::<Value>().map_err(|e| e.to_string()),
        429 => Err("перевод: слишком много запросов".into()),
        code => Err(format!("перевод: сервер ответил {code}")),
    }
}

/// 16 kHz mono 16-bit, the only shape the service reads.
fn wav(samples: &[f32]) -> Vec<u8> {
    let rate = RATE as u32;
    let data = samples.len() * 2;
    let mut out = Vec::with_capacity(44 + data);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data as u32).to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    out
}
