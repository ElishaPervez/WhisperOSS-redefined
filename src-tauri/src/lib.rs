mod applog;
mod audio;
mod clipboard;
#[allow(dead_code)]
mod config;
mod dsp;
mod groq;
mod hook;
mod hotkey_logic;
mod keys;
mod overlay_state;
mod pipeline;
mod position;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, PhysicalPosition};

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            applog::log("app-start");
            clipboard::init();

            // Overlay: hidden until a dictation starts, never clickable in M1.
            if let Some(w) = app.get_webview_window("overlay") {
                let _ = w.set_ignore_cursor_events(true);
            }

            let audio_engine = audio::AudioEngine::start(app.handle().clone());

            match keys::load() {
                Some(key) => {
                    pipeline::start(app.handle().clone(), audio_engine, key)
                }
                None => {
                    // Headless M1: without a key the hotkey does nothing.
                    // Set WHISPEROSS_GROQ_KEY once and restart (see keys.rs).
                    applog::log("pipeline-not-started-no-key");
                }
            }

            let quit = MenuItem::with_id(app, "quit", "Quit WhisperOSS", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&quit])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().expect("icon").clone())
                .menu(&menu)
                .tooltip("WhisperOSS")
                .on_menu_event(|app, event| {
                    if event.id.as_ref() == "quit" {
                        applog::log("quit-from-tray");
                        app.exit(0);
                    }
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Bottom-center of the monitor the cursor is on, 26 logical px above the
/// taskbar (Milestone 0, unchanged).
pub(crate) fn position_overlay(app: &tauri::AppHandle) -> tauri::Result<()> {
    let window = app
        .get_webview_window("overlay")
        .expect("overlay window missing from tauri.conf.json");

    let cursor = app.cursor_position()?;
    let monitor = app
        .available_monitors()?
        .into_iter()
        .find(|m| {
            let pos = m.position();
            let size = m.size();
            let bounds = position::Rect {
                x: pos.x,
                y: pos.y,
                width: size.width,
                height: size.height,
            };
            position::contains(bounds, cursor.x, cursor.y)
        })
        .or_else(|| app.primary_monitor().ok().flatten())
        .expect("no monitor found");

    let wa = monitor.work_area();
    let work_area = position::Rect {
        x: wa.position.x,
        y: wa.position.y,
        width: wa.size.width,
        height: wa.size.height,
    };
    let (x, y) = position::pill_position(work_area, monitor.scale_factor());
    window.set_position(PhysicalPosition::new(x, y))?;
    Ok(())
}
