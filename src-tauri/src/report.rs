//! Sends error reports and support messages to madgodinc.net/hark/api.
//!
//! Error reports carry the version, Windows version, an anonymous install id,
//! the kind of failure and its message. Never caption text. They can be turned
//! off in the settings ("send_errors"). The same error is sent at most once per
//! ten minutes per run, so a failure in a loop cannot flood the server.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const BASE: &str = "https://madgodinc.net/hark/api";
const REPEAT_AFTER: Duration = Duration::from_secs(600);

pub struct Identity {
    pub install: String,
    pub version: String,
    pub os: String,
    /// Read on every send, so the settings toggle applies at once.
    pub enabled: Box<dyn Fn() -> bool + Send + Sync>,
}

static IDENTITY: OnceLock<Identity> = OnceLock::new();
static SENT: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

pub fn init(identity: Identity) {
    let _ = IDENTITY.set(identity);
    // A crash anywhere still leaves a line in the log and a report.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        error("panic", &info.to_string());
        previous(info);
    }));
}

/// An anonymous id for this installation: random enough to tell copies apart,
/// tied to nothing about the person.
pub fn new_install_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mixed = nanos ^ ((std::process::id() as u128) << 64) ^ (&nanos as *const u128 as u128);
    format!("{:032x}", mixed.wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C835))
}

/// Logs the failure and, unless turned off, reports it in the background.
pub fn error(kind: &str, message: &str) {
    crate::diag::line(format!("error [{kind}]: {message}"));
    let Some(id) = IDENTITY.get() else { return };
    if !(id.enabled)() {
        return;
    }
    let key = format!("{kind}:{}", message.chars().take(160).collect::<String>());
    {
        let mut sent = SENT.lock().unwrap();
        let sent = sent.get_or_insert_with(HashMap::new);
        if sent.get(&key).is_some_and(|t| t.elapsed() < REPEAT_AFTER) {
            return;
        }
        sent.insert(key, Instant::now());
    }
    let body = json!({
        "install": id.install, "version": id.version, "os": id.os,
        "kind": kind, "message": message,
        "detail": crate::diag::tail(30),
    });
    std::thread::spawn(move || {
        let _ = post("error", &body);
    });
}

/// A support message. Blocking: call it off the main thread.
pub fn feedback(category: &str, message: &str, contact: &str, report: Option<String>) -> Result<u64, String> {
    let id = IDENTITY.get().ok_or("не готово")?;
    let body = json!({
        "install": id.install, "version": id.version, "os": id.os,
        "category": category, "message": message, "contact": contact,
        "report": report.unwrap_or_default(),
    });
    let answer = post("feedback", &body)?;
    Ok(answer.get("number").and_then(|n| n.as_u64()).unwrap_or(0))
}

fn post(route: &str, body: &Value) -> Result<Value, String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("mgi-hark")
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .post(format!("{BASE}/{route}"))
        .json(body)
        .send()
        .map_err(|_| "нет связи с сервером".to_string())?;
    match response.status().as_u16() {
        200 => response.json::<Value>().map_err(|e| e.to_string()),
        429 => Err("слишком много сообщений, попробуйте через час".into()),
        413 => Err("сообщение слишком длинное".into()),
        code => Err(format!("сервер ответил {code}")),
    }
}
