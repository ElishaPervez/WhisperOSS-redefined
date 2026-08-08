mod applog;
mod audio;
mod clipboard;
mod dsp;
mod groq;
mod hook;
mod hotkey_logic;
mod keys;
mod position;

use tauri::{Manager, PhysicalPosition};

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            clipboard::init();
            position_overlay(app.handle())?;
            let audio_engine = audio::AudioEngine::start(app.handle().clone());
            // TEMPORARY (removed in Task 9): record continuously so the pill
            // shows live mic levels for this task's verification.
            audio_engine.start_recording();
            // TEMPORARY (removed in Task 9): prove the hook + tracker work.
            {
                let (tx, rx) = std::sync::mpsc::channel();
                hook::spawn(tx);
                std::thread::spawn(move || {
                    let mut tracker = hotkey_logic::HoldTracker::new();
                    for ev in rx {
                        match tracker.on_event(ev) {
                            hotkey_logic::Action::Start => applog::log("hook-test-start"),
                            hotkey_logic::Action::Finish { held_ms } => {
                                applog::log(&format!("hook-test-finish held_ms={held_ms}"))
                            }
                            hotkey_logic::Action::Cancel => applog::log("hook-test-cancel"),
                            hotkey_logic::Action::None => {}
                        }
                    }
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Move the overlay to bottom-center of the monitor the cursor is on,
/// 26 logical px above the taskbar.
fn position_overlay(app: &tauri::AppHandle) -> tauri::Result<()> {
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
