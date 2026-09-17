//! The speech models live outside the installer and are fetched on first use of
//! a language, from the sherpa-onnx releases on GitHub.
//!
//! Russian: GigaAM v3 transducer with punctuation, 162 MB download.
//! English: Parakeet TDT-CTC 110M, 99 MB download, also punctuated.
//! Both reuse one Silero VAD file.

use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const RELEASES: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Ru,
    En,
}

impl Lang {
    pub fn parse(s: &str) -> Lang {
        if s == "en" { Lang::En } else { Lang::Ru }
    }

    /// Archive name (also the folder it unpacks to) and the files we keep.
    fn archive(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Lang::Ru => (
                "sherpa-onnx-nemo-transducer-punct-giga-am-v3-russian-2025-12-16",
                &["encoder.int8.onnx", "decoder.onnx", "joiner.onnx", "tokens.txt"],
            ),
            Lang::En => ("sherpa-onnx-nemo-parakeet_tdt_ctc_110m-en-36000-int8", &["model.int8.onnx", "tokens.txt"]),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Model {
    /// GigaAM: encoder, decoder, joiner.
    Transducer { encoder: String, decoder: String, joiner: String },
    /// Parakeet CTC head: one file.
    NemoCtc { model: String },
}

#[derive(Clone, Debug)]
pub struct ModelPaths {
    pub models_dir: std::path::PathBuf,
    pub vad: String,
    pub tokens: String,
    pub model: Model,
}

pub fn paths(dir: &Path, lang: Lang) -> ModelPaths {
    let (name, _) = lang.archive();
    let g = dir.join(name);
    let s = |p: PathBuf| p.to_string_lossy().to_string();
    ModelPaths {
        models_dir: dir.to_path_buf(),
        vad: s(dir.join("silero_vad.onnx")),
        tokens: s(g.join("tokens.txt")),
        model: match lang {
            Lang::Ru => Model::Transducer {
                encoder: s(g.join("encoder.int8.onnx")),
                decoder: s(g.join("decoder.onnx")),
                joiner: s(g.join("joiner.onnx")),
            },
            Lang::En => Model::NemoCtc { model: s(g.join("model.int8.onnx")) },
        },
    }
}

pub fn ready(dir: &Path, lang: Lang) -> bool {
    let (name, files) = lang.archive();
    dir.join("silero_vad.onnx").is_file() && files.iter().all(|f| dir.join(name).join(f).is_file())
}

#[derive(Serialize, Clone)]
pub struct Progress {
    /// "download", "unpack", "done" or "error"
    pub stage: &'static str,
    pub done: u64,
    pub total: u64,
    pub message: String,
}

pub fn download(dir: &Path, lang: Lang, report: impl Fn(Progress)) -> anyhow::Result<()> {
    fs::create_dir_all(dir)?;
    let client = reqwest::blocking::Client::builder()
        .user_agent("mgi-hark")
        .timeout(None)
        .build()?;

    if !dir.join("silero_vad.onnx").is_file() {
        fetch(&client, &format!("{RELEASES}/silero_vad.onnx"), &dir.join("silero_vad.onnx"), |_, _| {})?;
    }

    let (name, files) = lang.archive();
    fetch_archive(&client, dir, RELEASES, name, files, &report)?;
    // The sound tagger is small and optional: a failure here must not block speech.
    if let Err(e) = fetch_archive(&client, dir, TAGGER_RELEASES, TAGGER, TAGGER_FILES, &|_| {}) {
        crate::diag::line(format!("sound tagger download failed: {e}"));
    }

    if !ready(dir, lang) {
        anyhow::bail!("файлы модели не появились после распаковки");
    }
    Ok(())
}

/// Translation on this computer: NLLB-200 600M int8 and the ONNX Runtime it
/// runs on, both served from our own site because neither has a stable public
/// download that is safe to depend on.
const MT_HOME: &str = "https://madgodinc.net/hark/models";
const MT: &str = "hark-nllb-600m-int8";
const MT_FILES: &[&str] = &["encoder_model.onnx", "encoder_model.onnx.data", "decoder_merged.onnx", "tokenizer.json"];
const RUNTIME_DLL: &str = "onnxruntime-1.28.2-win-x64.dll";

/// (model folder, runtime library) when the translation model is on disk.
pub fn mt_paths(dir: &Path) -> Option<(PathBuf, PathBuf)> {
    let model = dir.join(MT);
    let dll = dir.join(RUNTIME_DLL);
    let ready = dll.is_file() && MT_FILES.iter().all(|f| model.join(f).is_file());
    ready.then_some((model, dll))
}

pub fn download_mt(dir: &Path, report: impl Fn(Progress)) -> anyhow::Result<()> {
    fs::create_dir_all(dir)?;
    let client = reqwest::blocking::Client::builder().user_agent("mgi-hark").timeout(None).build()?;
    let dll = dir.join(RUNTIME_DLL);
    if !dll.is_file() {
        fetch(&client, &format!("{MT_HOME}/{RUNTIME_DLL}"), &dll, |done, total| {
            report(Progress { stage: "download", done, total, message: String::new() })
        })?;
    }
    fetch_archive(&client, dir, MT_HOME, MT, MT_FILES, &report)?;
    if mt_paths(dir).is_none() {
        anyhow::bail!("файлы перевода не появились после распаковки");
    }
    Ok(())
}

/// Recognises non-speech sounds (laughter, music, gunshots...) for caption tags.
/// CED mini, 10 MB int8, 527 AudioSet classes.
const TAGGER: &str = "sherpa-onnx-ced-mini-audio-tagging-2024-04-19";
const TAGGER_RELEASES: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/audio-tagging-models";
const TAGGER_FILES: &[&str] = &["model.int8.onnx", "class_labels_indices.csv"];

/// (model, labels) when the sound tagger is on disk.
pub fn tagger_paths(dir: &Path) -> Option<(String, String)> {
    let d = dir.join(TAGGER);
    let (m, l) = (d.join(TAGGER_FILES[0]), d.join(TAGGER_FILES[1]));
    (m.is_file() && l.is_file()).then(|| (m.to_string_lossy().to_string(), l.to_string_lossy().to_string()))
}

/// Fetches just the sound tagger, for installs that already have the speech model.
pub fn download_tagger(dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dir)?;
    let client = reqwest::blocking::Client::builder().user_agent("mgi-hark").timeout(None).build()?;
    fetch_archive(&client, dir, TAGGER_RELEASES, TAGGER, TAGGER_FILES, &|_| {})
}

fn fetch_archive(
    client: &reqwest::blocking::Client,
    dir: &Path,
    releases: &str,
    name: &str,
    files: &[&str],
    report: &dyn Fn(Progress),
) -> anyhow::Result<()> {
    if files.iter().all(|f| dir.join(name).join(f).is_file()) {
        return Ok(());
    }
    let archive = dir.join(format!("{name}.tar.bz2"));
    fetch(client, &format!("{releases}/{name}.tar.bz2"), &archive, |done, total| {
        report(Progress { stage: "download", done, total, message: String::new() })
    })?;
    report(Progress { stage: "unpack", done: 0, total: 0, message: String::new() });
    let staging = dir.join(format!("{name}.unpacking"));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;
    let mut tar = tar::Archive::new(bzip2::read::BzDecoder::new(File::open(&archive)?));
    for entry in tar.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let Some(file) = path.file_name().and_then(|n| n.to_str()) else { continue };
        // Some archives are packed as "./name/file" and some as "name/file"; the
        // leading "." is not a folder. Ignoring that kept the English model from
        // ever unpacking.
        let depth = path.components().filter(|c| !matches!(c, std::path::Component::CurDir)).count();
        // Test recordings share file names across folders; keep only top-level model files.
        if depth == 2 && (files.contains(&file) || file == "LICENSE") {
            entry.unpack(staging.join(file))?;
        }
    }
    let _ = fs::remove_dir_all(dir.join(name));
    fs::rename(&staging, dir.join(name))?;
    let _ = fs::remove_file(&archive);
    Ok(())
}

/// Downloads to a .part file first so a broken download never looks finished.
fn fetch(
    client: &reqwest::blocking::Client,
    url: &str,
    to: &Path,
    progress: impl Fn(u64, u64),
) -> anyhow::Result<()> {
    let mut response = client.get(url).send()?.error_for_status()?;
    let total = response.content_length().unwrap_or(0);
    let part = to.with_extension("part");
    let mut file = File::create(&part)?;
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    let mut reported = 0u64;
    loop {
        let n = response.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        if done - reported >= 1 << 20 {
            progress(done, total);
            reported = done;
        }
    }
    file.flush()?;
    drop(file);
    if total > 0 && done != total {
        anyhow::bail!("загрузка оборвалась: {done} из {total} байт");
    }
    progress(done, total);
    fs::rename(part, to)?;
    Ok(())
}
