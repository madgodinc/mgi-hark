mod autostart;
mod topmost;
pub mod asr;
pub mod audio;
pub mod models;
mod sources;

use audio::{Capture, Stopped, Target};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;
use std::time::Duration;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

const OVERLAY: &str = "overlay";

struct Hark {
    settings: Mutex<Value>,
    settings_path: PathBuf,
    models_dir: PathBuf,
    engine: Mutex<Option<Sender<asr::Msg>>>,
    /// The generation lets a capture that ended on its own clear only itself,
    /// never a newer one started in the meantime.
    capture: Mutex<Option<(u64, Capture)>>,
    generation: AtomicU64,
    listening: Mutex<Option<Value>>,
    downloading: AtomicBool,
    engine_ready: AtomicBool,
    edit: AtomicBool,
    /// Set while the chosen program is closed and we wait for it to start again.
    /// Holds (generation, exe, display name); stopping by hand clears it.
    waiting: Mutex<Option<(u64, String, String)>>,
    /// Auto-listen is tried once per launch, when the model first becomes ready.
    auto_listen_done: AtomicBool,
}

impl Hark {
    fn lang(&self) -> models::Lang {
        let settings = self.settings.lock().unwrap();
        models::Lang::parse(settings.get("lang").and_then(|v| v.as_str()).unwrap_or("ru"))
    }

    fn persist(&self) {
        let settings = self.settings.lock().unwrap().clone();
        if let Some(dir) = self.settings_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&self.settings_path, serde_json::to_vec_pretty(&settings).unwrap_or_default());
    }
}

struct EventSink(AppHandle);

impl asr::Sink for EventSink {
    fn caption(&self, caption: asr::Caption) {
        let _ = self.0.emit("caption", caption);
    }
    fn level(&self, level: f32) {
        let _ = self.0.emit("level", level);
    }
    fn ready(&self) {
        self.0.state::<Hark>().engine_ready.store(true, Ordering::SeqCst);
        let _ = self.0.emit("models", json!({ "stage": "done" }));
        let app = self.0.clone();
        std::thread::spawn(move || auto_listen(&app));
    }
}

fn start_engine(app: &AppHandle) {
    let hark = app.state::<Hark>();
    let mut engine = hark.engine.lock().unwrap();
    let lang = hark.lang();
    if engine.is_some() || !models::ready(&hark.models_dir, lang) {
        return;
    }
    let (tx, rx) = mpsc::channel();
    *engine = Some(tx);
    let paths = models::paths(&hark.models_dir, lang);
    let handle = app.clone();
    let _ = app.emit("models", json!({ "stage": "loading" }));
    std::thread::Builder::new()
        .name("asr".into())
        .spawn(move || {
            let sink = EventSink(handle.clone());
            if let Err(e) = asr::run(paths, rx, sink) {
                *handle.state::<Hark>().engine.lock().unwrap() = None;
                let _ = handle.emit("models", json!({ "stage": "error", "message": e.to_string() }));
            }
        })
        .expect("spawn asr thread");
}

#[derive(Serialize)]
struct Snapshot {
    settings: Value,
    models_ready: bool,
    downloading: bool,
    engine_ready: bool,
    listening: Option<Value>,
    waiting: Option<String>,
    edit: bool,
    windows_ok: bool,
    autostart: bool,
    /// Started by Windows at sign-in: nobody is looking, updates may install at once.
    started_hidden: bool,
}

#[tauri::command]
fn get_state(hark: State<Hark>) -> Snapshot {
    // lang() locks the settings too; take it before the settings guard exists,
    // a temporary guard in the struct literal would live to the end of it.
    let lang = hark.lang();
    let settings = hark.settings.lock().unwrap().clone();
    let autostart = settings.get("autostart").and_then(|v| v.as_bool()).unwrap_or(true);
    Snapshot {
        settings,
        models_ready: models::ready(&hark.models_dir, lang),
        downloading: hark.downloading.load(Ordering::SeqCst),
        engine_ready: hark.engine_ready.load(Ordering::SeqCst),
        listening: hark.listening.lock().unwrap().clone(),
        waiting: hark.waiting.lock().unwrap().as_ref().map(|w| w.2.clone()),
        edit: hark.edit.load(Ordering::SeqCst),
        windows_ok: sources::windows_supports_capture(),
        autostart,
        started_hidden: autostart::started_hidden(),
    }
}

/// The page owns the look. Its keys are merged over what we keep ourselves
/// (overlay position, visibility, last source), never replacing them wholesale.
#[tauri::command]
fn save_settings(app: AppHandle, hark: State<Hark>, settings: Value) {
    let merged = {
        let mut current = hark.settings.lock().unwrap();
        if let (Some(target), Some(incoming)) = (current.as_object_mut(), settings.as_object()) {
            for (k, v) in incoming {
                target.insert(k.clone(), v.clone());
            }
        }
        current.clone()
    };
    hark.persist();
    let _ = app.emit_to(OVERLAY, "settings", merged);
}

#[tauri::command]
fn list_sources() -> Vec<sources::AudioApp> {
    sources::list()
}

#[tauri::command]
fn start_listening(app: AppHandle, target: Target, name: String, exe: Option<String>) -> Result<(), String> {
    begin_capture(&app, target, name, exe)
}

/// Starts recording a program (or the whole system) into the engine.
/// `exe` lets us find the program again if it closes and starts anew.
fn begin_capture(app: &AppHandle, target: Target, name: String, exe: Option<String>) -> Result<(), String> {
    let hark = app.state::<Hark>();
    let engine = hark
        .engine
        .lock()
        .unwrap()
        .clone()
        .ok_or("Модель распознавания ещё не готова")?;
    stop_capture(&hark);

    let generation = hark.generation.fetch_add(1, Ordering::SeqCst) + 1;
    let feed = engine.clone();
    let handle = app.clone();
    let exe_for_wait = exe.clone();
    let name_for_wait = name.clone();
    let capture = Capture::start(
        target.clone(),
        move |samples| feed.send(asr::Msg::Audio(samples)).is_ok(),
        move |stopped| {
            let hark = handle.state::<Hark>();
            {
                let mut slot = hark.capture.lock().unwrap();
                if !slot.as_ref().is_some_and(|(g, _)| *g == generation) {
                    return; // replaced or stopped by hand meanwhile
                }
                // This is our own thread; drop the handle without joining.
                slot.take();
                *hark.listening.lock().unwrap() = None;
            }
            match (stopped, exe_for_wait) {
                (Stopped::Requested, _) => {}
                (Stopped::AppClosed, Some(exe)) => wait_for_program(&handle, exe, name_for_wait),
                (Stopped::AppClosed, None) => {
                    let _ = handle.emit("listen", json!({ "on": false, "reason": "Программа закрылась" }));
                }
                (Stopped::Failed(e), _) => {
                    let _ = handle.emit("listen", json!({ "on": false, "reason": format!("Звук не захватывается: {e}") }));
                }
            }
        },
    );
    *hark.capture.lock().unwrap() = Some((generation, capture));
    *hark.waiting.lock().unwrap() = None;
    let _ = engine.send(asr::Msg::Reset);

    let state = json!({ "on": true, "name": name, "target": target_json(&target) });
    *hark.listening.lock().unwrap() = Some(state.clone());
    hark.settings.lock().unwrap()["last_source"] = json!({ "name": name, "exe": exe, "target": target_json(&target) });
    hark.persist();
    let _ = app.emit("listen", state);
    update_tray(app);
    Ok(())
}

/// The program closed (or is not running yet): look for it every few seconds
/// and pick it up again, so restarting Discord does not end the captions.
fn wait_for_program(app: &AppHandle, exe: String, name: String) {
    let hark = app.state::<Hark>();
    let generation = hark.generation.fetch_add(1, Ordering::SeqCst) + 1;
    *hark.waiting.lock().unwrap() = Some((generation, exe.clone(), name.clone()));
    let _ = app.emit("listen", json!({ "on": false, "waiting": name }));
    update_tray(app);
    let handle = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(3));
        let hark = handle.state::<Hark>();
        let still = hark.waiting.lock().unwrap().as_ref().is_some_and(|w| w.0 == generation);
        if !still {
            return;
        }
        if let Some(found) = sources::find(&exe) {
            let _ = begin_capture(&handle, Target::App { pid: found.pid }, found.name, Some(exe.clone()));
            return;
        }
    });
}

/// Once per launch, when the model is loaded: resume the last source.
fn auto_listen(app: &AppHandle) {
    let hark = app.state::<Hark>();
    if hark.auto_listen_done.swap(true, Ordering::SeqCst) || hark.listening.lock().unwrap().is_some() {
        return;
    }
    let last = hark.settings.lock().unwrap().get("last_source").cloned();
    let Some(last) = last else { return };
    let name = last.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    if last.pointer("/target/kind").and_then(|v| v.as_str()) == Some("system") {
        let _ = begin_capture(app, Target::System, name, None);
        return;
    }
    let Some(exe) = last.get("exe").and_then(|v| v.as_str()).map(String::from) else { return };
    match sources::find(&exe) {
        Some(found) => {
            let _ = begin_capture(app, Target::App { pid: found.pid }, found.name, Some(exe));
        }
        None => wait_for_program(app, exe, name),
    }
}

fn target_json(target: &Target) -> Value {
    match target {
        Target::System => json!({ "kind": "system" }),
        Target::App { pid } => json!({ "kind": "app", "pid": pid }),
    }
}

fn stop_capture(hark: &Hark) {
    *hark.waiting.lock().unwrap() = None;
    // Take it out first: the capture thread's own exit path locks this slot.
    let running = hark.capture.lock().unwrap().take();
    if let Some((_, capture)) = running {
        capture.stop();
    }
    *hark.listening.lock().unwrap() = None;
    if let Some(engine) = hark.engine.lock().unwrap().as_ref() {
        let _ = engine.send(asr::Msg::Reset);
    }
}

#[tauri::command]
fn stop_listening(app: AppHandle, hark: State<Hark>) {
    stop_capture(&hark);
    let _ = app.emit("listen", json!({ "on": false }));
    update_tray(&app);
}

#[tauri::command]
fn set_autostart(hark: State<Hark>, on: bool) -> Result<(), String> {
    autostart::set(on)?;
    hark.settings.lock().unwrap()["autostart"] = json!(on);
    hark.persist();
    Ok(())
}

/// Brings the settings window forward (tray click, second launch).
fn reveal(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn update_tray(app: &AppHandle) {
    let Some(tray) = app.tray_by_id("main") else { return };
    let hark = app.state::<Hark>();
    let tip = match (hark.listening.lock().unwrap().as_ref(), hark.waiting.lock().unwrap().as_ref()) {
        (Some(l), _) => format!("Hark: слушаю {}", l.get("name").and_then(|v| v.as_str()).unwrap_or("")),
        (None, Some(w)) => format!("Hark: жду, когда запустится {}", w.2),
        _ => "Hark: не слушаю".to_string(),
    };
    let _ = tray.set_tooltip(Some(tip));
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Открыть Hark", true, None::<&str>)?;
    let captions = MenuItem::with_id(app, "captions", "Скрыть или показать субтитры", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Выйти", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &captions, &quit])?;
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().ok_or(tauri::Error::UnknownPath)?)
        .tooltip("Hark")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => reveal(app),
            "captions" => {
                let visible = app.get_webview_window(OVERLAY).and_then(|w| w.is_visible().ok()).unwrap_or(false);
                set_overlay_visible(app.clone(), app.state::<Hark>(), !visible);
            }
            "quit" => {
                let hark = app.state::<Hark>();
                stop_capture(&hark);
                hark.persist();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                reveal(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

#[tauri::command]
fn download_models(app: AppHandle, hark: State<Hark>) {
    if hark.downloading.swap(true, Ordering::SeqCst) {
        return;
    }
    let dir = hark.models_dir.clone();
    let lang = hark.lang();
    let handle = app.clone();
    std::thread::spawn(move || {
        let progress = handle.clone();
        let result = models::download(&dir, lang, move |p| {
            let _ = progress.emit("models", p);
        });
        let hark = handle.state::<Hark>();
        hark.downloading.store(false, Ordering::SeqCst);
        match result {
            // The user may have switched language while this was downloading.
            Ok(()) if hark.lang() == lang => start_engine(&handle),
            Ok(()) => {}
            Err(e) => {
                let _ = handle.emit("models", json!({ "stage": "error", "message": e.to_string() }));
            }
        }
    });
}

/// Switching language swaps the speech model: listening stops, the old engine
/// thread ends when its last sender is dropped, and the new one loads.
#[tauri::command]
fn set_language(app: AppHandle, hark: State<Hark>, lang: models::Lang) {
    if hark.lang() == lang {
        return;
    }
    stop_capture(&hark);
    let _ = app.emit("listen", json!({ "on": false }));
    hark.settings.lock().unwrap()["lang"] = serde_json::to_value(lang).unwrap();
    hark.persist();
    hark.engine.lock().unwrap().take();
    hark.engine_ready.store(false, Ordering::SeqCst);
    let _ = app.emit("language", lang);
    if models::ready(&hark.models_dir, lang) {
        start_engine(&app);
    } else if !hark.downloading.load(Ordering::SeqCst) {
        let _ = app.emit("models", json!({ "stage": "missing" }));
    }
}

fn apply_edit(app: &AppHandle, on: bool) {
    let hark = app.state::<Hark>();
    hark.edit.store(on, Ordering::SeqCst);
    if let Some(overlay) = app.get_webview_window(OVERLAY) {
        let _ = overlay.set_ignore_cursor_events(!on);
        if on {
            let _ = overlay.show();
            let _ = overlay.set_focus();
        }
    }
    if !on {
        hark.persist();
    }
    let _ = app.emit("edit", on);
}

#[tauri::command]
fn set_edit(app: AppHandle, on: bool) {
    apply_edit(&app, on);
}

#[tauri::command]
fn set_overlay_visible(app: AppHandle, hark: State<Hark>, visible: bool) {
    if let Some(overlay) = app.get_webview_window(OVERLAY) {
        let _ = if visible { overlay.show() } else { overlay.hide() };
    }
    hark.settings.lock().unwrap()["overlay_visible"] = json!(visible);
    hark.persist();
    let _ = app.emit("overlay-visible", visible);
}

#[tauri::command]
fn reset_overlay(app: AppHandle) {
    if let Some(overlay) = app.get_webview_window(OVERLAY) {
        place_default(&app, &overlay);
    }
}

fn place_default(app: &AppHandle, overlay: &tauri::WebviewWindow) {
    let Ok(Some(monitor)) = app.primary_monitor() else { return };
    let (size, origin) = (monitor.size(), monitor.position());
    let w = (size.width as f64 * 0.56) as u32;
    let h = (size.height as f64 * 0.2) as u32;
    let _ = overlay.set_size(PhysicalSize::new(w, h));
    let _ = overlay.set_position(PhysicalPosition::new(
        origin.x + ((size.width - w) / 2) as i32,
        origin.y + (size.height as f64 * 0.7) as i32,
    ));
}

/// Far above anything the recognizer will number in one session.
static DEMO_ID: AtomicU64 = AtomicU64::new(1 << 40);

/// Plays a scripted conversation so the look can be tuned without anyone talking.
#[tauri::command]
fn demo(app: AppHandle, hark: State<Hark>) {
    const RU: [&str; 5] = [
        "Привет! Слышишь меня? Заходи в голосовой канал.",
        "Я иду на центральную линию, прикрой меня справа.",
        "Осторожно, их трое, отходим к башне.",
        "Отлично сыграли, давай ещё одну.",
        "Ставлю вард у реки, смотри на карту.",
    ];
    const EN: [&str; 5] = [
        "Hey! Can you hear me? Join the voice channel.",
        "I am going mid, cover me on the right.",
        "Careful, there are three of them, fall back.",
        "Good game, let us play one more.",
        "Placing a ward by the river, check the map.",
    ];
    let lines = if hark.lang() == models::Lang::En { EN } else { RU };
    std::thread::spawn(move || {
        for line in lines {
            let id = DEMO_ID.fetch_add(1, Ordering::SeqCst);
            let words: Vec<&str> = line.split(' ').collect();
            for n in 1..words.len() {
                let draft = words[..n].join(" ").to_lowercase().replace([',', '.', '!', '?'], "");
                let _ = app.emit("caption", asr::Caption { id, text: draft, is_final: false });
                std::thread::sleep(Duration::from_millis(230));
            }
            let _ = app.emit("caption", asr::Caption { id, text: line.to_string(), is_final: true });
            std::thread::sleep(Duration::from_millis(1400));
        }
    });
}

fn with_args<'a, M: Manager<tauri::Wry>>(
    builder: WebviewWindowBuilder<'a, tauri::Wry, M>,
    args: &Option<String>,
) -> WebviewWindowBuilder<'a, tauri::Wry, M> {
    match args {
        Some(args) => builder.additional_browser_args(args),
        None => builder,
    }
}

fn toggle_edit_shortcut() -> Shortcut {
    // Ctrl+~, the key above Tab, same as the CreepiDota overlay.
    Shortcut::new(Some(Modifiers::CONTROL), Code::Backquote)
}

fn toggle_visible_shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyJ)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| reveal(app)))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    if shortcut == &toggle_edit_shortcut() {
                        let on = !app.state::<Hark>().edit.load(Ordering::SeqCst);
                        apply_edit(app, on);
                    } else if shortcut == &toggle_visible_shortcut() {
                        let visible = app
                            .get_webview_window(OVERLAY)
                            .and_then(|w| w.is_visible().ok())
                            .unwrap_or(false);
                        set_overlay_visible(app.clone(), app.state::<Hark>(), !visible);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let settings_path = config_dir.join("settings.json");
            let settings: Value = std::fs::read(&settings_path)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_else(|| json!({}));
            let models_dir = app.path().app_local_data_dir()?.join("models");

            app.manage(Hark {
                settings: Mutex::new(settings.clone()),
                settings_path,
                models_dir,
                engine: Mutex::new(None),
                capture: Mutex::new(None),
                generation: AtomicU64::new(0),
                listening: Mutex::new(None),
                downloading: AtomicBool::new(false),
                engine_ready: AtomicBool::new(false),
                edit: AtomicBool::new(false),
                waiting: Mutex::new(None),
                auto_listen_done: AtomicBool::new(false),
            });

            // First run: start with Windows unless the person turns it off.
            if settings.get("autostart").is_none() {
                let _ = autostart::set(true);
            }

            // Both windows share one WebView2 environment, so they must get
            // identical browser arguments. HARK_CDP_PORT opens DevTools
            // protocol access for automated checks; users never set it.
            let browser_args = std::env::var("HARK_CDP_PORT").ok().map(|port| {
                format!("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --remote-debugging-port={port}")
            });

            with_args(WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into())), &browser_args)
                .title("Hark")
                .inner_size(1180.0, 780.0)
                .min_inner_size(960.0, 620.0)
                .decorations(false)
                .background_color(tauri::window::Color(11, 13, 12, 255))
                .center()
                // Started by Windows at sign-in: captions run, settings stay in the tray.
                .visible(!autostart::started_hidden())
                .build()?;
            build_tray(app.handle())?;

            let overlay = with_args(WebviewWindowBuilder::new(app, OVERLAY, WebviewUrl::App("overlay.html".into())), &browser_args)
                .title("Hark · субтитры")
                .transparent(true)
                .decorations(false)
                .shadow(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .focused(false)
                .resizable(true)
                .visible(false)
                .min_inner_size(240.0, 80.0)
                .build()?;
            match settings.get("overlay_rect") {
                Some(r) => {
                    let n = |k: &str| r.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let _ = overlay.set_size(PhysicalSize::new(n("w") as u32, n("h") as u32));
                    let _ = overlay.set_position(PhysicalPosition::new(n("x") as i32, n("y") as i32));
                }
                None => place_default(app.handle(), &overlay),
            }
            overlay.set_ignore_cursor_events(true)?;
            topmost::keep(app.handle().clone(), OVERLAY);
            if settings.get("overlay_visible").and_then(|v| v.as_bool()).unwrap_or(true) {
                overlay.show()?;
            }

            let shortcuts = app.global_shortcut();
            let _ = shortcuts.register(toggle_edit_shortcut());
            let _ = shortcuts.register(toggle_visible_shortcut());

            start_engine(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            let app = window.app_handle();
            match (window.label(), event) {
                // Closing the settings keeps the captions running; the tray has "Выйти".
                ("main", WindowEvent::CloseRequested { api, .. }) => {
                    api.prevent_close();
                    let _ = window.hide();
                    app.state::<Hark>().persist();
                    let _ = app.emit("hidden-to-tray", ());
                }
                (OVERLAY, WindowEvent::Moved(_) | WindowEvent::Resized(_)) => {
                    let Some(overlay) = app.get_webview_window(OVERLAY) else { return };
                    let (Ok(pos), Ok(size)) = (overlay.outer_position(), overlay.inner_size()) else { return };
                    if size.width == 0 || size.height == 0 {
                        return; // minimised or hidden
                    }
                    app.state::<Hark>().settings.lock().unwrap()["overlay_rect"] =
                        json!({ "x": pos.x, "y": pos.y, "w": size.width, "h": size.height });
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            save_settings,
            list_sources,
            start_listening,
            stop_listening,
            download_models,
            set_language,
            set_autostart,
            set_edit,
            set_overlay_visible,
            reset_overlay,
            demo
        ])
        .run(tauri::generate_context!())
        .expect("error while running Hark");
}
