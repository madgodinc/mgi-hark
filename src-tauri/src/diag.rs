//! A small diagnostic log for when Hark hears nothing on someone else's machine.
//!
//! Plain text in the app's log folder, capped at 512 KB (older half dropped).
//! It records what was opened and whether sound arrives, never what was said:
//! captions stay out of it on purpose.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
const MAX_BYTES: u64 = 512 * 1024;

pub fn init(dir: PathBuf) {
    let _ = fs::create_dir_all(&dir);
    *PATH.lock().unwrap() = Some(dir.join("hark.log"));
}

pub fn path() -> Option<PathBuf> {
    PATH.lock().unwrap().clone()
}

pub fn line(text: impl AsRef<str>) {
    let Some(path) = path() else { return };
    if fs::metadata(&path).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        if let Ok(old) = fs::read_to_string(&path) {
            let keep = &old[old.len() / 2..];
            let start = keep.find('\n').map(|i| i + 1).unwrap_or(0);
            let _ = fs::write(&path, &keep[start..]);
        }
    }
    let stamp = chrono_like_now();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{stamp} {}", text.as_ref());
    }
}

/// The last `n` lines, for the "copy report" button.
pub fn tail(n: usize) -> String {
    let Some(path) = path() else { return String::new() };
    let text = fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Seconds since the Unix epoch as a UTC clock time, without a date crate.
fn chrono_like_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    format!("d{days} {h:02}:{m:02}:{s:02}Z")
}

#[macro_export]
macro_rules! diag {
    ($($arg:tt)*) => { $crate::diag::line(format!($($arg)*)) };
}
