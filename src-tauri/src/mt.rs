//! Translation on this computer, with nothing sent anywhere.
//!
//! NLLB-200 distilled 600M, quantised to int8: the encoder reads the phrase
//! once, then the decoder writes the translation one word piece at a time,
//! each step reusing the attention cache of the steps before it. Without that
//! cache every step would re-read the whole phrase and the wait would grow with
//! the square of the answer.
//!
//! ONNX Runtime is loaded from a DLL next to the models rather than linked in:
//! the speech models carry their own static copy, and two copies of the same
//! library in one program refuse to link. The DLL and the model are downloaded
//! together when the person turns this mode on.

use crate::glossary::{self, Section};
use anyhow::{anyhow, Result};
use regex::Regex;
use ort::session::{builder::GraphOptimizationLevel, Session, SessionInputValue};
use ort::value::{DynValue, Tensor};
use std::borrow::Cow;
use std::path::Path;
use std::sync::OnceLock;
use tokenizers::Tokenizer;

/// End of sentence, and what the decoder starts from.
const EOS: i64 = 2;
const HEADS: i64 = 16;
const HEAD_DIM: i64 = 64;
/// Long enough for any phrase someone says in a game, short enough that a model
/// that starts repeating itself stops on its own.
const MAX_TOKENS: usize = 96;

pub struct Local {
    encoder: Session,
    decoder: Session,
    tokenizer: Tokenizer,
    /// Names of the cache inputs, in the order the graph declares them.
    past: Vec<String>,
    present: Vec<String>,
}

/// The library can only be loaded once per run; a second attempt is harmless.
fn load_runtime(dll: &Path) -> Result<()> {
    static READY: OnceLock<bool> = OnceLock::new();
    let ok = *READY.get_or_init(|| ort::init_from(dll).map(|env| env.commit()).unwrap_or(false));
    if ok {
        Ok(())
    } else {
        Err(anyhow!("не удалось загрузить onnxruntime.dll"))
    }
}

impl Local {
    /// `threads` is how many CPU cores the translation may take from the game.
    pub fn load(dir: &Path, dll: &Path, threads: usize) -> Result<Local> {
        load_runtime(dll)?;
        // ONNX Runtime errors are not Send, so they are turned into text here
        // rather than carried up.
        let build = |file: &str| -> Result<Session> {
            let open = || -> ort::Result<Session> {
                let mut builder = Session::builder()?
                    .with_optimization_level(GraphOptimizationLevel::Level3)?
                    .with_intra_threads(threads)?;
                builder.commit_from_file(dir.join(file))
            };
            open().map_err(|e| anyhow!("не открылась модель {file}: {e}"))
        };
        let encoder = build("encoder_model.onnx")?;
        let decoder = build("decoder_merged.onnx")?;
        let tokenizer = Tokenizer::from_file(dir.join("tokenizer.json")).map_err(|e| anyhow!("{e}"))?;
        let past: Vec<String> =
            decoder.inputs().iter().map(|i| i.name().to_string()).filter(|n| n.starts_with("past_key_values")).collect();
        let present: Vec<String> =
            decoder.outputs().iter().map(|o| o.name().to_string()).filter(|n| n.starts_with("present")).collect();
        if past.is_empty() || past.len() != present.len() {
            anyhow::bail!("неожиданная модель перевода: {} входов кэша, {} выходов", past.len(), present.len());
        }
        Ok(Local { encoder, decoder, tokenizer, past, present })
    }

    /// Prints what the graphs take and return: the first thing to check when a
    /// model file and the code disagree.
    pub fn describe(&self) {
        for outlet in self.decoder.inputs().iter().filter(|o| !o.name().starts_with("past_key_values")) {
            println!("decoder {} :: {:?}", outlet.name(), outlet.dtype());
        }
        println!("past[0..4] {:?}", &self.past[..4.min(self.past.len())]);
        println!("present[0..4] {:?}", &self.present[..4.min(self.present.len())]);
        for (what, outlets) in [
            ("encoder in", self.encoder.inputs()),
            ("encoder out", self.encoder.outputs()),
            ("decoder in", self.decoder.inputs()),
        ] {
            let shown: Vec<String> =
                outlets.iter().take(5).map(|o| format!("{} {:?}", o.name(), o.dtype())).collect();
            println!("{what}: {} total, first {shown:#?}", outlets.len());
        }
        println!("cache: {} past, {} present", self.past.len(), self.present.len());
    }

    fn lang_id(&self, code: &str) -> Result<i64> {
        self.tokenizer.token_to_id(code).map(|id| id as i64).ok_or_else(|| anyhow!("язык {code} не знаком модели"))
    }

    /// One sentence through the model, nothing else.
    fn once(&mut self, text: &str, source: &str, target: &str) -> Result<String> {
        let (source_id, target_id) = (self.lang_id(source)?, self.lang_id(target)?);
        let encoded = self.tokenizer.encode(text, false).map_err(|e| anyhow!("{e}"))?;
        let mut ids: Vec<i64> = Vec::with_capacity(encoded.get_ids().len() + 2);
        ids.push(source_id);
        ids.extend(encoded.get_ids().iter().map(|&id| id as i64));
        ids.push(EOS);
        let len = ids.len() as i64;
        let mask = vec![1i64; ids.len()];

        let hidden = {
            let inputs: Vec<(Cow<str>, SessionInputValue)> = vec![
                ("input_ids".into(), value(Tensor::from_array((vec![1, len], ids)).map_err(ort_error)?)),
                ("attention_mask".into(), value(Tensor::from_array((vec![1, len], mask.clone())).map_err(ort_error)?)),
            ];
            let mut out = self.encoder.run(inputs).map_err(ort_error)?;
            let hidden = out.remove("last_hidden_state").ok_or_else(|| anyhow!("энкодер не вернул состояние"))?;
            let (shape, data) = hidden.try_extract_tensor::<f32>().map_err(ort_error)?;
            let hidden = (shape.iter().copied().collect::<Vec<i64>>(), data.to_vec());
            hidden
        };

        let allocator = ort::memory::Allocator::default();
        let empty = || -> Result<DynValue> {
            Ok(Tensor::<f32>::new(&allocator, [1, HEADS as usize, 0, HEAD_DIM as usize])
                .map_err(ort_error)?
                .into_dyn())
        };
        let hidden_tensor =
            || -> Result<DynValue> { Ok(value_of(Tensor::from_array((hidden.0.clone(), hidden.1.clone())).map_err(ort_error)?)) };
        let mask_tensor =
            || -> Result<DynValue> { Ok(value_of(Tensor::from_array((vec![1, len], mask.clone())).map_err(ort_error)?)) };

        // Pass one: the cross-attention cache, which depends on the phrase and
        // not on the words written so far, so it is computed once and then only
        // lent to each step, never taken away.
        let mut cross: Vec<Option<DynValue>> = vec![None; self.past.len()];
        {
            let mut inputs: Vec<(Cow<str>, SessionInputValue)> = vec![
                ("input_ids".into(), SessionInputValue::Owned(value_of(Tensor::from_array((vec![1, 1], vec![EOS])).map_err(ort_error)?))),
                ("encoder_attention_mask".into(), SessionInputValue::Owned(mask_tensor()?)),
                ("encoder_hidden_states".into(), SessionInputValue::Owned(hidden_tensor()?)),
                ("use_cache_branch".into(), SessionInputValue::Owned(value_of(Tensor::from_array((vec![1], vec![false])).map_err(ort_error)?))),
            ];
            for name in &self.past {
                inputs.push((name.as_str().into(), SessionInputValue::Owned(empty()?)));
            }
            let mut outputs = self.decoder.run(inputs).map_err(ort_error)?;
            for (slot, name) in self.present.iter().enumerate() {
                if name.contains(".encoder.") {
                    cross[slot] = outputs.remove(name);
                }
            }
        }
        if cross.iter().all(|v| v.is_none()) {
            anyhow::bail!("модель перевода не отдала кэш внимания");
        }

        let mut selves: Vec<Option<DynValue>> = vec![None; self.past.len()];
        let mut out_ids: Vec<i64> = vec![EOS, target_id];
        let mut written = 0usize;
        while written < MAX_TOKENS {
            let step: Vec<i64> = if written == 0 { out_ids.clone() } else { vec![out_ids[out_ids.len() - 1]] };
            let step_len = step.len() as i64;
            let mut inputs: Vec<(Cow<str>, SessionInputValue)> = vec![
                ("input_ids".into(), SessionInputValue::Owned(value_of(Tensor::from_array((vec![1, step_len], step)).map_err(ort_error)?))),
                ("encoder_attention_mask".into(), SessionInputValue::Owned(mask_tensor()?)),
                ("encoder_hidden_states".into(), SessionInputValue::Owned(hidden_tensor()?)),
                ("use_cache_branch".into(), SessionInputValue::Owned(value_of(Tensor::from_array((vec![1], vec![true])).map_err(ort_error)?))),
            ];
            for (slot, name) in self.past.iter().enumerate() {
                let given = match (&cross[slot], selves[slot].take()) {
                    (Some(kept), _) => SessionInputValue::from(kept),
                    (None, Some(grown)) => SessionInputValue::Owned(grown),
                    (None, None) => SessionInputValue::Owned(empty()?),
                };
                let _ = name;
                inputs.push((name.as_str().into(), given));
            }
            let mut outputs = self.decoder.run(inputs).map_err(ort_error)?;
            let token = {
                let (shape, data) = outputs["logits"].try_extract_tensor::<f32>().map_err(ort_error)?;
                let vocab = *shape.last().ok_or_else(|| anyhow!("пустые логиты"))? as usize;
                argmax(&data[data.len() - vocab..]) as i64
            };
            // Only the self-attention cache grows; the cross one stays as it was.
            for (slot, name) in self.present.iter().enumerate() {
                if name.contains(".decoder.") {
                    selves[slot] = outputs.remove(name);
                }
            }
            out_ids.push(token);
            written += 1;
            if token == EOS {
                break;
            }
        }

        let ids: Vec<u32> = out_ids.iter().skip(2).map(|&id| id as u32).filter(|&id| id != EOS as u32).collect();
        self.tokenizer.decode(&ids, true).map_err(|e| anyhow!("{e}"))
    }

    /// The whole job: the glossary before and after the model, and one sentence
    /// at a time, because NLLB translates the first one and drops the rest.
    pub fn translate(&mut self, text: &str, source: &str, target: &str, game: &str) -> Result<String> {
        let section = Section::build(&source[..2.min(source.len())], game);
        if let Some(known) = section.as_ref().and_then(|s| s.call(text)) {
            return Ok(known.to_string());
        }
        let (prepared, marks) = match &section {
            Some(s) => s.prepare(text),
            None => (text.to_string(), Vec::new()),
        };
        let mut done: Vec<String> = Vec::new();
        for sentence in sentences(&prepared) {
            done.push(self.once(sentence, source, target)?);
        }
        let out = glossary::restore(&done.join(" "), &marks);
        let out = match &section {
            Some(s) => s.repair(&out),
            None => out,
        };
        Ok(capitalize(&tidy(&out)))
    }
}

/// Splits on sentence ends, keeping the punctuation with its sentence.
fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes: Vec<(usize, char)> = text.char_indices().collect();
    for (i, (at, c)) in bytes.iter().enumerate() {
        let ends = matches!(c, '.' | '!' | '?' | '…');
        let last = i + 1 == bytes.len();
        let next_is_end = bytes.get(i + 1).map(|(_, n)| matches!(n, '.' | '!' | '?' | '…')).unwrap_or(false);
        if (ends && !next_is_end) || last {
            let end = at + c.len_utf8();
            let piece = text[start..end].trim();
            if !piece.is_empty() {
                out.push(piece);
            }
            start = end;
        }
    }
    if out.is_empty() && !text.trim().is_empty() {
        out.push(text.trim());
    }
    out
}

/// The tokenizer leaves a space before punctuation now and then, and a
/// placeholder right after a pronoun makes the model write a dash.
fn tidy(text: &str) -> String {
    static SPACE: OnceLock<Regex> = OnceLock::new();
    static DASH: OnceLock<Regex> = OnceLock::new();
    let space = SPACE.get_or_init(|| Regex::new(r" +([,.!?;:…])").expect("space pattern"));
    let dash = DASH.get_or_init(|| {
        Regex::new(r"(Он|Она|Оно|Они|Я|Ты|Мы|Вы|He|She|They|I|We|You) [-–—] ").expect("dash pattern")
    });
    let out = space.replace_all(text, "$1").into_owned();
    dash.replace_all(&out, "$1 ").into_owned()
}

/// A phrase that starts with a placeholder comes back in lower case.
fn capitalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut done = false;
    for c in text.chars() {
        if !done && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            done = true;
        } else {
            out.push(c);
        }
    }
    out
}

fn value<T: ort::value::ValueTypeMarker + 'static>(tensor: ort::value::Value<T>) -> SessionInputValue<'static> {
    SessionInputValue::Owned(tensor.into_dyn())
}

fn value_of<T: ort::value::ValueTypeMarker + 'static>(tensor: ort::value::Value<T>) -> DynValue {
    tensor.into_dyn()
}

/// ONNX Runtime errors carry raw pointers and are neither Send nor Sync.
fn ort_error(e: ort::Error) -> anyhow::Error {
    anyhow!("{e}")
}

fn argmax(values: &[f32]) -> usize {
    let mut best = 0;
    for (i, v) in values.iter().enumerate() {
        if *v > values[best] {
            best = i;
        }
    }
    best
}

/// How many cores the translation may take. Half of them, at least two and at
/// most six: the game needs the rest, and past six the model stops speeding up.
pub fn threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get() / 2).unwrap_or(4).clamp(2, 6)
}
