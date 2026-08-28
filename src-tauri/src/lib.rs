mod applog;
mod audio;
mod autostart;
mod casualize;
mod clipboard;
mod commands;
mod config;
mod dsp;
mod gemini;
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
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, PhysicalPosition, WindowEvent};

pub fn run() {
    tauri::Builder::default()
        // Must be the first plugin: a second copy of the app (for example a
        // dev build while the installed one sits in the tray) would otherwise
        // fight it for the hotkey with its own stale key snapshot. The second
        // launch hands off and exits; the running copy surfaces its settings
        // so the launch visibly did something.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            applog::log("second-instance-redirected");
            show_settings(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::get_key_status,
            commands::has_api_key,
            commands::set_transcription_provider,
            commands::set_provider_model,
            commands::set_formatter,
            commands::set_casual,
            commands::set_show_live_transcript,
            commands::set_preroll_ms,
            commands::set_postroll_ms,
            commands::set_theme,
            commands::set_vocabulary,
            commands::set_autostart,
            commands::save_api_key,
            commands::save_provider_key,
            commands::list_microphones,
            commands::set_microphone,
            commands::finish_first_run,
            commands::microphone_status,
            commands::overlay_visible,
        ])
        .setup(|app| {
            applog::log("app-start");
            clipboard::init();

            // Overlay: hidden until a dictation starts, never clickable in M1.
            if let Some(w) = app.get_webview_window("overlay") {
                let _ = w.set_ignore_cursor_events(true);
                // The pill's material: Windows' transient backdrop, clipped
                // to rounded antialiased corners. On this build it renders as
                // translucent smoked grey rather than blurred acrylic — that
                // appearance IS the approved design (human decision, 5b). If
                // a later Windows build composites it fully, the same code
                // gains blur behind the same tint.
                if let Ok(handle) = w.hwnd() {
                    use windows::Win32::Graphics::Dwm::{
                        DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
                        DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE,
                        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
                    };
                    use windows::Win32::UI::Controls::MARGINS;
                    let hwnd = windows::Win32::Foundation::HWND(handle.0);
                    let pref = DWMWCP_ROUND;
                    let margins = MARGINS {
                        cxLeftWidth: -1,
                        cxRightWidth: -1,
                        cyTopHeight: -1,
                        cyBottomHeight: -1,
                    };
                    unsafe {
                        let _ = DwmSetWindowAttribute(
                            hwnd,
                            DWMWA_WINDOW_CORNER_PREFERENCE,
                            &pref as *const _ as *const core::ffi::c_void,
                            std::mem::size_of_val(&pref) as u32,
                        );
                        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
                        let backdrop = DWMSBT_TRANSIENTWINDOW;
                        let _ = DwmSetWindowAttribute(
                            hwnd,
                            DWMWA_SYSTEMBACKDROP_TYPE,
                            &backdrop as *const _ as *const core::ffi::c_void,
                            std::mem::size_of_val(&backdrop) as u32,
                        );
                    }
                }
            }

            let cfg = config::load();
            config::save(&cfg);
            autostart::reconcile(cfg.run_on_startup);

            let groq_key = keys::load(keys::Provider::Groq).unwrap_or_default();
            let gemini_key = keys::load(keys::Provider::Gemini).unwrap_or_default();
            let audio_engine = audio::AudioEngine::start(
                app.handle().clone(),
                cfg.input_device.clone(),
                cfg.preroll_ms,
            );
            let gemini_live =
                gemini::GeminiLive::spawn(gemini::PROD_WS_URL.to_string(), pipeline::REQUEST_TIMEOUT);
            let app_state = state::AppState::new(
                cfg.clone(),
                groq_key.clone(),
                gemini_key.clone(),
                audio_engine,
                gemini_live,
            );
            app.manage(app_state.clone());

            let selected_key_missing = match cfg.transcription_provider {
                config::TranscriptionProvider::Groq => groq_key.is_empty(),
                config::TranscriptionProvider::Gemini => gemini_key.is_empty(),
            };
            if selected_key_missing {
                applog::log("first-run-selected-provider-has-no-key");
                match cfg.transcription_provider {
                    config::TranscriptionProvider::Groq => show_first_run(app.handle()),
                    config::TranscriptionProvider::Gemini => show_settings_at_key(app.handle()),
                }
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

            applog::log("app-ready");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn show_settings_inner(app: &tauri::AppHandle, focus_key: bool) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
        // The webview loaded once, at startup, while this window was hidden.
        // Tell it to re-read everything so it never shows stale values.
        let _ = w.emit("settings-shown", focus_key);
    }
}

pub(crate) fn show_settings(app: &tauri::AppHandle) {
    show_settings_inner(app, false);
}

/// Groq rejected the key: open settings with the cursor already in the key
/// box, so the fix is one paste away rather than a hunt.
pub(crate) fn show_settings_at_key(app: &tauri::AppHandle) {
    show_settings_inner(app, true);
}

pub(crate) fn show_first_run(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("firstrun") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Bottom-center of the monitor the cursor is on, 26 logical px above the
/// taskbar (Milestone 0, unchanged).
pub(crate) fn position_overlay(app: &tauri::AppHandle, logical_w: f64) -> tauri::Result<()> {
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
    let (x, y) = position::pill_position(work_area, monitor.scale_factor(), logical_w);
    window.set_position(PhysicalPosition::new(x, y))?;
    Ok(())
}
