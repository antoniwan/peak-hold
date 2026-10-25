mod audio;

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

const TRAY_ID: &str = "meter";
/// Sounds right after start or reset are ignored, so the click of the button
/// isn't the new peak.
const SETTLE: Duration = Duration::from_millis(300);

/// The peak lives here, not in the window, so it keeps counting while the
/// window is hidden and the tray can reset it.
#[derive(Default)]
struct Meter(Mutex<MeterState>);

#[derive(Default)]
struct MeterState {
    peak_fs: Option<f32>,
    peak_at: Option<u64>,
    offset: f32,
    ignore_until: Option<Instant>,
    device: Option<String>,
    error: Option<String>,
    tray_dirty: bool,
}

/// Levels are in dBFS (0 = the mic's maximum). The window adds the
/// calibration offset to show estimated dB SPL.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    level_fs: Option<f32>,
    peak_fs: Option<f32>,
    peak_at: Option<u64>,
    device: Option<String>,
    error: Option<String>,
}

impl MeterState {
    fn snapshot(&self, level_fs: Option<f32>) -> Snapshot {
        Snapshot {
            level_fs,
            peak_fs: self.peak_fs,
            peak_at: self.peak_at,
            device: self.device.clone(),
            error: self.error.clone(),
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn with_state<R>(app: &AppHandle, f: impl FnOnce(&mut MeterState) -> R) -> R {
    let meter = app.state::<Meter>();
    let mut state = meter.0.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut state)
}

pub(crate) fn on_started(app: &AppHandle, device: String) {
    let snap = with_state(app, |s| {
        s.device = Some(device);
        s.error = None;
        s.ignore_until = Some(Instant::now() + SETTLE);
        s.tray_dirty = true;
        s.snapshot(None)
    });
    let _ = app.emit("meter", snap);
}

pub(crate) fn on_error(app: &AppHandle, message: String) {
    let snap = with_state(app, |s| {
        s.error = Some(message);
        s.snapshot(None)
    });
    let _ = app.emit("meter", snap);
}

pub(crate) fn on_level(app: &AppHandle, level_fs: f32) {
    let snap = with_state(app, |s| {
        let settled = s.ignore_until.map_or(true, |t| Instant::now() >= t);
        if settled && s.peak_fs.map_or(true, |p| level_fs > p) {
            s.peak_fs = Some(level_fs);
            s.peak_at = Some(now_ms());
            s.tray_dirty = true;
        }
        s.snapshot(Some(level_fs))
    });
    let _ = app.emit("meter", snap);
}

pub(crate) fn flush_tray(app: &AppHandle) {
    let text = with_state(app, |s| {
        if !s.tray_dirty {
            return None;
        }
        s.tray_dirty = false;
        Some(s.peak_fs.map(|p| (p + s.offset).max(0.0)))
    });
    let Some(peak_db) = text else { return };
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    match peak_db {
        Some(db) => {
            let _ = tray.set_tooltip(Some(format!("Peak Hold: loudest {db:.1} dB")));
            // Shown beside the icon in the macOS menu bar; ignored on Windows.
            let _ = tray.set_title(Some(format!("{db:.0} dB")));
        }
        None => {
            let _ = tray.set_tooltip(Some("Peak Hold: listening"));
            let _ = tray.set_title(None::<&str>);
        }
    }
}

fn reset(app: &AppHandle) {
    let snap = with_state(app, |s| {
        s.peak_fs = None;
        s.peak_at = None;
        s.ignore_until = Some(Instant::now() + SETTLE);
        s.tray_dirty = true;
        s.snapshot(None)
    });
    let _ = app.emit("meter", snap);
    flush_tray(app);
}

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[tauri::command]
fn get_snapshot(app: AppHandle) -> Snapshot {
    with_state(&app, |s| s.snapshot(None))
}

#[tauri::command]
fn reset_peak(app: AppHandle) {
    reset(&app);
}

#[tauri::command]
fn set_offset(app: AppHandle, offset: f32) {
    with_state(&app, |s| {
        s.offset = offset;
        s.tray_dirty = true;
    });
    flush_tray(&app);
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show meter", true, None::<&str>)?;
    let reset_item = MenuItem::with_id(app, "reset", "Reset peak", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Peak Hold", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &reset_item, &separator, &quit])?;

    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Peak Hold: starting")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_window(app),
            "reset" => reset(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .manage(Meter::default())
        .invoke_handler(tauri::generate_handler![get_snapshot, reset_peak, set_offset])
        .setup(|app| {
            build_tray(app.handle())?;
            audio::spawn(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps the meter running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("Peak Hold failed to start");
}
