//! Records what one program plays, or everything the speakers play.
//!
//! Per-process loopback needs Windows 10 2004 or newer. The recognizer wants
//! 16 kHz mono, so we ask Windows to convert for us and fall back to 48 kHz
//! stereo plus our own downmix and resampling when it refuses.

use crate::sources::process_alive;
use serde::Deserialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use wasapi::{
    initialize_mta, AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat,
};

pub const RATE: i32 = 16_000;
const ALIVE_EVERY: Duration = Duration::from_secs(2);

#[derive(Deserialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Target {
    /// Everything that comes out of the default speakers.
    System,
    /// One program and all its child processes.
    App { pid: u32 },
}

pub enum Stopped {
    /// We were asked to stop.
    Requested,
    /// The program we were listening to has closed.
    AppClosed,
    Failed(String),
}

pub struct Capture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Capture {
    pub fn start(
        target: Target,
        mut out: impl FnMut(Vec<f32>) -> bool + Send + 'static,
        on_stop: impl FnOnce(Stopped) + Send + 'static,
    ) -> Capture {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name("capture".into())
            .spawn(move || {
                let result = run(&target, &mut out, &flag);
                on_stop(match result {
                    Ok(reason) => reason,
                    Err(e) => Stopped::Failed(e.to_string()),
                });
            })
            .expect("spawn capture thread");
        Capture { stop, thread: Some(thread) }
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn open(target: &Target) -> anyhow::Result<AudioClient> {
    Ok(match target {
        Target::App { pid } => AudioClient::new_application_loopback_client(*pid, true)?,
        Target::System => DeviceEnumerator::new()?
            .get_default_device(&Direction::Render)?
            .get_iaudioclient()?,
    })
}

fn run(target: &Target, out: &mut dyn FnMut(Vec<f32>) -> bool, stop: &AtomicBool) -> anyhow::Result<Stopped> {
    let _ = initialize_mta();

    let mode = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 0 };
    let mut client = open(target)?;
    let mut format = WaveFormat::new(32, 32, &SampleType::Float, RATE as usize, 1, None);
    if client.initialize_client(&format, &Direction::Capture, &mode).is_err() {
        // A client that failed to initialise cannot be retried; open a new one.
        client = open(target)?;
        format = WaveFormat::new(32, 32, &SampleType::Float, 48_000, 2, None);
        client.initialize_client(&format, &Direction::Capture, &mode)?;
    }
    let channels = format.get_nchannels() as usize;
    let rate = format.get_samplespersec() as i32;
    let resampler = (rate != RATE)
        .then(|| sherpa_onnx::LinearResampler::create(rate, RATE))
        .flatten();

    let event = client.set_get_eventhandle()?;
    let capture = client.get_audiocaptureclient()?;
    client.start_stream()?;

    let frame_bytes = 4 * channels;
    let mut bytes: VecDeque<u8> = VecDeque::new();
    let mut next_alive_check = Instant::now() + ALIVE_EVERY;

    let reason = loop {
        if stop.load(Ordering::SeqCst) {
            break Stopped::Requested;
        }
        // Checked on a clock, not on silence: after a program exits, Windows
        // can keep delivering empty buffers for it, so events never stop.
        if Instant::now() >= next_alive_check {
            next_alive_check = Instant::now() + ALIVE_EVERY;
            if let Target::App { pid } = target {
                if !process_alive(*pid) {
                    break Stopped::AppClosed;
                }
            }
        }
        // No event means nothing is playing, which is normal between phrases.
        if event.wait_for_event(250).is_err() {
            continue;
        }

        capture.read_from_device_to_deque(&mut bytes)?;
        let frames = bytes.len() / frame_bytes;
        if frames == 0 {
            continue;
        }
        let mut mono = Vec::with_capacity(frames);
        for frame in bytes.make_contiguous()[..frames * frame_bytes].chunks_exact(frame_bytes) {
            let mut sum = 0.0f32;
            for s in frame.chunks_exact(4) {
                sum += f32::from_le_bytes([s[0], s[1], s[2], s[3]]);
            }
            mono.push(sum / channels as f32);
        }
        bytes.drain(..frames * frame_bytes);

        let mono = match &resampler {
            Some(r) => r.resample(&mono, false),
            None => mono,
        };
        if !out(mono) {
            break Stopped::Requested;
        }
    };

    let _ = client.stop_stream();
    Ok(reason)
}
