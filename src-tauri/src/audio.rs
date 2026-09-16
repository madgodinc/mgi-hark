//! Records what one program plays, or everything every output device plays.
//!
//! Per-process loopback needs Windows 10 2004 or newer. The recognizer wants
//! 16 kHz mono, so we ask Windows to convert for us and fall back to 48 kHz
//! stereo plus our own downmix and resampling when it refuses.
//!
//! "The whole computer" means every active output device, mixed: gaming
//! headsets often expose separate "Game" and "Chat" devices, and voice goes to
//! the one that is not the Windows default.

use crate::sources::process_alive;
use serde::Deserialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use wasapi::{
    initialize_mta, AudioCaptureClient, AudioClient, DeviceEnumerator, DeviceState, Direction, SampleType,
    StreamMode, WaveFormat,
};

pub const RATE: i32 = 16_000;
const ALIVE_EVERY: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(15);
const REPORT_EVERY: Duration = Duration::from_secs(10);

#[derive(Deserialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Target {
    /// Everything that comes out of any output device.
    System,
    /// One program: every process tree it runs (`pids`), or just `pid`.
    App {
        pid: u32,
        #[serde(default)]
        pids: Vec<u32>,
    },
    /// Everything one output device plays.
    Device { id: String },
}

impl Target {
    fn app_pids(&self) -> Vec<u32> {
        match self {
            Target::App { pid, pids } => {
                let mut all = pids.clone();
                if !all.contains(pid) {
                    all.insert(0, *pid);
                }
                all
            }
            _ => Vec::new(),
        }
    }
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
                if let Err(e) = &result {
                    crate::report::error("capture", &format!("{target:?}: {e}"));
                }
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

/// One open loopback stream and what it has delivered so far.
struct Stream {
    name: String,
    client: AudioClient,
    capture: AudioCaptureClient,
    channels: usize,
    resampler: Option<sherpa_onnx::LinearResampler>,
    bytes: VecDeque<u8>,
    queue: VecDeque<f32>,
    frames_total: u64,
    peak: f32,
}

impl Stream {
    fn open(name: String, make: &dyn Fn() -> anyhow::Result<AudioClient>) -> anyhow::Result<Stream> {
        let mode = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 0 };
        // Ask for the rate the audio engine already runs at, so Windows does not
        // resample: its automatic conversion has no proper low-pass filter, and
        // the aliasing it leaves measurably hurts recognition. We resample to
        // 16 kHz ourselves with the filtered resampler from sherpa-onnx.
        let candidates = [(48_000usize, 2usize), (44_100, 2), (RATE as usize, 1)];
        let mut opened = None;
        let mut last_error = None;
        for (rate, channels) in candidates {
            let mut client = make()?;
            let format = WaveFormat::new(32, 32, &SampleType::Float, rate, channels, None);
            match client.initialize_client(&format, &Direction::Capture, &mode) {
                Ok(()) => {
                    opened = Some((client, format));
                    break;
                }
                Err(e) => {
                    // A client that failed to initialise cannot be retried; the loop opens a new one.
                    diag!("{name}: {rate} Hz {channels} ch refused ({e})");
                    last_error = Some(e);
                }
            }
        }
        let Some((client, format)) = opened else {
            return Err(last_error.map(anyhow::Error::from).unwrap_or_else(|| anyhow::anyhow!("no format accepted")));
        };
        let channels = format.get_nchannels() as usize;
        let rate = format.get_samplespersec() as i32;
        let resampler = (rate != RATE).then(|| sherpa_onnx::LinearResampler::create(rate, RATE)).flatten();
        // Polled rather than event-driven so several streams share one thread,
        // but shared event mode still needs its handle set before starting.
        let _event = client.set_get_eventhandle()?;
        let capture = client.get_audiocaptureclient()?;
        client.start_stream()?;
        diag!("{name}: open, {rate} Hz, {channels} ch");
        Ok(Stream {
            name,
            client,
            capture,
            channels,
            resampler,
            bytes: VecDeque::new(),
            queue: VecDeque::new(),
            frames_total: 0,
            peak: 0.0,
        })
    }

    fn pull(&mut self) -> anyhow::Result<()> {
        if self.capture.get_next_packet_size()?.unwrap_or(0) == 0 {
            return Ok(());
        }
        self.capture.read_from_device_to_deque(&mut self.bytes)?;
        let frame_bytes = 4 * self.channels;
        let frames = self.bytes.len() / frame_bytes;
        if frames == 0 {
            return Ok(());
        }
        let mut mono = Vec::with_capacity(frames);
        for frame in self.bytes.make_contiguous()[..frames * frame_bytes].chunks_exact(frame_bytes) {
            let mut sum = 0.0f32;
            for s in frame.chunks_exact(4) {
                sum += f32::from_le_bytes([s[0], s[1], s[2], s[3]]);
            }
            let v = sum / self.channels as f32;
            self.peak = self.peak.max(v.abs());
            mono.push(v);
        }
        self.bytes.drain(..frames * frame_bytes);
        self.frames_total += frames as u64;
        let mono = match &self.resampler {
            Some(r) => r.resample(&mono, false),
            None => mono,
        };
        self.queue.extend(mono);
        Ok(())
    }
}

fn open_streams(target: &Target) -> anyhow::Result<Vec<Stream>> {
    match target {
        Target::App { .. } => {
            let mut streams = Vec::new();
            let mut last_error = None;
            for pid in target.app_pids() {
                let make = move || Ok(AudioClient::new_application_loopback_client(pid, true)?);
                match Stream::open(format!("process {pid}"), &make) {
                    Ok(s) => streams.push(s),
                    Err(e) => {
                        diag!("process {pid}: cannot open ({e})");
                        last_error = Some(e);
                    }
                }
            }
            match (streams.is_empty(), last_error) {
                (true, Some(e)) => Err(e),
                (true, None) => anyhow::bail!("у программы нет процессов"),
                _ => Ok(streams),
            }
        }
        Target::Device { id } => {
            let enumerator = DeviceEnumerator::new()?;
            let device = enumerator.get_device(id)?;
            let name = device.get_friendlyname().unwrap_or_else(|_| "device".into());
            let make = || Ok(device.get_iaudioclient()?);
            Ok(vec![Stream::open(name, &make)?])
        }
        Target::System => {
            let enumerator = DeviceEnumerator::new()?;
            let devices = enumerator.get_device_collection(&Direction::Render)?;
            let default_id = enumerator.get_default_device(&Direction::Render).and_then(|d| d.get_id()).ok();
            let mut streams = Vec::new();
            for device in &devices {
                let Ok(device) = device else { continue };
                if !matches!(device.get_state(), Ok(DeviceState::Active)) {
                    continue;
                }
                let name = device.get_friendlyname().unwrap_or_else(|_| "device".into());
                let id = device.get_id().ok();
                let label = if id.is_some() && id == default_id { format!("{name} (default)") } else { name };
                let make = || Ok(device.get_iaudioclient()?);
                match Stream::open(label.clone(), &make) {
                    Ok(s) => streams.push(s),
                    Err(e) => diag!("{label}: cannot open ({e})"),
                }
            }
            if streams.is_empty() {
                anyhow::bail!("не удалось открыть ни одно устройство вывода звука");
            }
            Ok(streams)
        }
    }
}

fn run(target: &Target, out: &mut dyn FnMut(Vec<f32>) -> bool, stop: &AtomicBool) -> anyhow::Result<Stopped> {
    let _ = initialize_mta();
    diag!("capture start: {target:?}");
    let mut streams = open_streams(target)?;

    let mut next_alive_check = Instant::now() + ALIVE_EVERY;
    let mut next_report = Instant::now() + REPORT_EVERY;

    let reason = loop {
        if stop.load(Ordering::SeqCst) {
            break Stopped::Requested;
        }
        // Checked on a clock, not on silence: after a program exits, Windows
        // can keep delivering empty buffers for it.
        if Instant::now() >= next_alive_check {
            next_alive_check = Instant::now() + ALIVE_EVERY;
            let pids = target.app_pids();
            if !pids.is_empty() && !pids.iter().any(|p| process_alive(*p)) {
                break Stopped::AppClosed;
            }
        }
        if Instant::now() >= next_report {
            next_report = Instant::now() + REPORT_EVERY;
            for s in &mut streams {
                diag!("{}: {} frames so far, peak {:.3} in the last 10 s", s.name, s.frames_total, s.peak);
                s.peak = 0.0;
            }
        }

        std::thread::sleep(POLL);
        for s in &mut streams {
            s.pull()?;
        }

        // Mix: whatever each stream has, summed; a silent device delivers
        // nothing, so shorter queues are padded with silence.
        let n = streams.iter().map(|s| s.queue.len()).max().unwrap_or(0);
        if n == 0 {
            continue;
        }
        let mut mixed = vec![0.0f32; n];
        for s in &mut streams {
            let take = s.queue.len().min(n);
            for (i, v) in s.queue.drain(..take).enumerate() {
                mixed[i] += v;
            }
        }
        if streams.len() > 1 {
            for v in &mut mixed {
                *v = v.clamp(-1.0, 1.0);
            }
        }
        if !out(mixed) {
            break Stopped::Requested;
        }
    };

    for s in &streams {
        let _ = s.client.stop_stream();
    }
    diag!("capture stop");
    Ok(reason)
}
