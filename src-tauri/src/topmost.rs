//! Keeps the caption overlay on top.
//!
//! "Always on top" is a flag many windows share, and among them the one
//! activated last wins: a game launcher, a chat pop-up or the taskbar can end up
//! above the captions. Re-asserting HWND_TOPMOST every second puts the overlay
//! back without stealing focus, the same trick desktop pets and taskbar tools use.
//!
//! Exclusive fullscreen games stay out of reach: they bypass the window stack
//! entirely, and drawing inside them means injecting into the game, which
//! anti-cheat systems punish.

use std::time::Duration;
use tauri::{AppHandle, Manager};

const EVERY: Duration = Duration::from_millis(1000);

pub fn keep(app: AppHandle, label: &'static str) {
    std::thread::Builder::new()
        .name("topmost".into())
        .spawn(move || loop {
            std::thread::sleep(EVERY);
            let Some(window) = app.get_webview_window(label) else { return };
            if !window.is_visible().unwrap_or(false) {
                continue;
            }
            if let Ok(hwnd) = window.hwnd() {
                raise(hwnd.0 as isize);
            }
        })
        .expect("spawn topmost thread");
}

#[cfg(windows)]
fn raise(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
    };
    unsafe {
        SetWindowPos(
            hwnd as _,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
    }
}

#[cfg(not(windows))]
fn raise(_hwnd: isize) {}
