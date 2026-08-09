mod applog;
mod audio;
mod autostart;
mod clipboard;
mod commands;
mod config;
mod dsp;
mod groq;
mod hook;
mod hotkey_logic;
mod keys;
mod overlay_state;
mod pipeline;
mod position;
mod prompts;
mod state;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState};
use tauri::{Manager, PhysicalPosition, WindowEvent};

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::has_api_key,
            commands::set_formatter,
            commands::set_casual,
            commands::set_theme,
            commands::set_autostart,
            commands::save_api_key,
            commands::list_microphones,
            commands::set_microphone,
            commands::begin_hotkey_capture,
            commands::cancel_hotkey_capture,
        ])
        .setup(|app| {
            applog::log("app-start");
            clipboard::init();

            // Overlay: hidden until a dictation starts, never clickable in M1.
            if let Some(w) = app.get_webview_window("overlay") {
                let _ = w.set_ignore_cursor_events(true);
            }

            let cfg = config::load();
            config::save(&cfg);
            autostart::reconcile(cfg.run_on_startup);

            let key = keys::load().unwrap_or_default();
            let audio_engine =
                audio::AudioEngine::start(app.handle().clone(), cfg.input_device.clone());
            let app_state = state::AppState::new(cfg.clone(), key.clone(), audio_engine);
            app.manage(app_state.clone());

            if key.is_empty() {
                applog::log("pipeline-started-without-key");
            }
            pipeline::start(app.handle().clone(), app_state);

            let show = MenuItem::with_id(app, "show", "Show settings", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit WhisperOSS", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().expect("icon").clone())
                .menu(&menu)
                .tooltip("WhisperOSS")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_settings(app),
                    "quit" => {
                        applog::log("quit-from-tray");
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_settings(tray.app_handle());
                    }
                })
                .build(app)?;

            if let Some(settings) = app.get_webview_window("settings") {
                let handle = settings.clone();
                settings.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = handle.hide();
                        applog::log("settings-hidden-to-tray");
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

pub(crate) fn show_settings(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
    }
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
