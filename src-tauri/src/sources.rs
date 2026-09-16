//! Which programs can be listened to.
//!
//! Windows keeps an audio session for every program that has opened a sound
//! device. We list those sessions, fold helper processes into the program that
//! owns them (Discord and Steam play voice from child processes), and let the
//! capture side record the whole process tree from its root.

use serde::Serialize;
use std::collections::HashMap;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};
use wasapi::{initialize_mta, DeviceEnumerator, Direction, SessionState};

#[derive(Serialize, Clone, Debug)]
pub struct AudioApp {
    /// Root of the process tree; capture uses it with the whole tree included.
    pub pid: u32,
    pub exe: String,
    pub name: String,
    /// A voice program we know about, shown first in the list.
    pub voice: bool,
    /// Something is audible from it right now.
    pub playing: bool,
}

/// Executable name to (family, display name, is a voice program).
/// The family lets a helper like steamwebhelper.exe fold into steam.exe.
fn describe(exe: &str) -> Option<(&'static str, &'static str, bool)> {
    Some(match exe.to_ascii_lowercase().as_str() {
        "discord.exe" | "discordptb.exe" | "discordcanary.exe" => ("discord", "Discord", true),
        "steam.exe" | "steamwebhelper.exe" => ("steam", "Steam", true),
        "ts3client_win64.exe" | "ts3client_win32.exe" => ("ts3", "TeamSpeak 3", true),
        "teamspeak.exe" => ("ts", "TeamSpeak", true),
        "telegram.exe" => ("telegram", "Telegram", true),
        "zoom.exe" => ("zoom", "Zoom", true),
        "ms-teams.exe" | "teams.exe" => ("teams", "Microsoft Teams", true),
        "skype.exe" => ("skype", "Skype", true),
        "mumble.exe" => ("mumble", "Mumble", true),
        "whatsapp.exe" | "whatsapp.root.exe" => ("whatsapp", "WhatsApp", true),
        "chrome.exe" => ("chrome", "Google Chrome", false),
        "msedge.exe" | "msedgewebview2.exe" => ("edge", "Microsoft Edge", false),
        "firefox.exe" => ("firefox", "Firefox", false),
        "opera.exe" => ("opera", "Opera", false),
        "browser.exe" => ("yandex", "Яндекс Браузер", false),
        _ => return None,
    })
}

fn family(exe: &str) -> String {
    describe(exe).map(|d| d.0.to_string()).unwrap_or_else(|| exe.to_ascii_lowercase())
}

fn pretty(exe: &str) -> String {
    describe(exe)
        .map(|d| d.1.to_string())
        .unwrap_or_else(|| exe.trim_end_matches(".exe").trim_end_matches(".EXE").to_string())
}

/// Sessions live in COM objects, and COM wants its own thread here: Tauri
/// commands may run on a thread the webview already initialised differently.
pub fn list() -> Vec<AudioApp> {
    std::thread::spawn(list_on_this_thread).join().unwrap_or_default()
}

fn list_on_this_thread() -> Vec<AudioApp> {
    let _ = initialize_mta();

    // pid -> loudest peak across devices
    let mut sessions: HashMap<u32, f32> = HashMap::new();
    if let Ok(enumerator) = DeviceEnumerator::new() {
        if let Ok(devices) = enumerator.get_device_collection(&Direction::Render) {
            for device in &devices {
                let Ok(device) = device else { continue };
                let Ok(manager) = device.get_iaudiosessionmanager() else { continue };
                let Ok(list) = manager.get_audiosessionenumerator() else { continue };
                for i in 0..list.get_count().unwrap_or(0) {
                    let Ok(control) = list.get_session(i) else { continue };
                    if matches!(control.get_state(), Ok(SessionState::Expired)) {
                        continue;
                    }
                    let Ok(pid) = control.get_process_id() else { continue };
                    if pid == 0 {
                        continue; // system sounds
                    }
                    let peak = control
                        .get_audiometerinformation()
                        .and_then(|m| m.get_peak_value())
                        .unwrap_or(0.0);
                    let entry = sessions.entry(pid).or_insert(0.0);
                    *entry = entry.max(peak);
                }
            }
        }
    }

    let system = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::nothing()),
    );
    let exe_of = |pid: Pid| -> Option<String> {
        system.process(pid).map(|p| p.name().to_string_lossy().to_string())
    };

    // Climb to the topmost ancestor of the same family.
    let root_of = |pid: Pid| -> Pid {
        let Some(exe) = exe_of(pid) else { return pid };
        let fam = family(&exe);
        let mut current = pid;
        while let Some(parent) = system.process(current).and_then(|p| p.parent()) {
            match exe_of(parent) {
                Some(parent_exe) if family(&parent_exe) == fam => current = parent,
                _ => break,
            }
        }
        current
    };

    let mut apps: HashMap<u32, AudioApp> = HashMap::new();
    let mut add = |pid: Pid, peak: f32| {
        let root = root_of(pid);
        let Some(exe) = exe_of(root) else { return };
        let voice = describe(&exe).map(|d| d.2).unwrap_or(false);
        let app = apps.entry(root.as_u32()).or_insert_with(|| AudioApp {
            pid: root.as_u32(),
            name: pretty(&exe),
            exe: exe.clone(),
            voice,
            playing: false,
        });
        app.playing |= peak > 0.0005;
    };

    for (pid, peak) in &sessions {
        add(Pid::from_u32(*pid), *peak);
    }
    // A voice program that is running but silent has no session until a call
    // starts. Offer it anyway: people pick Discord before joining the channel.
    for (pid, process) in system.processes() {
        let exe = process.name().to_string_lossy();
        if describe(&exe).is_some_and(|d| d.2) {
            add(*pid, 0.0);
        }
    }

    let mut apps: Vec<AudioApp> = apps.into_values().collect();
    // Same program listed twice (two roots, e.g. two Discord builds) stays, but
    // voice programs first, then whatever is audible, then by name.
    apps.sort_by(|a, b| {
        b.voice
            .cmp(&a.voice)
            .then(b.playing.cmp(&a.playing))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    apps
}

/// The running program with this executable name, if any (for picking a
/// program up again after it restarts with a new process id).
pub fn find(exe: &str) -> Option<AudioApp> {
    list().into_iter().find(|a| a.exe.eq_ignore_ascii_case(exe))
}

/// Per-process audio capture exists since Windows 10 version 2004 (build 19041).
pub fn windows_supports_capture() -> bool {
    System::kernel_version()
        .and_then(|v| v.split('.').next_back().and_then(|b| b.parse::<u32>().ok()))
        .is_none_or(|build| build >= 19041)
}

pub fn process_alive(pid: u32) -> bool {
    let mut system = System::new();
    let pid = Pid::from_u32(pid);
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    system.process(pid).is_some()
}
