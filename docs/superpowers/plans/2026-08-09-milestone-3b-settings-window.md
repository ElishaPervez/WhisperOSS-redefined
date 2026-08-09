# Milestone 3b — Settings Window Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A real settings window from the approved design: left-click the tray icon to open it, close it back to the tray, switch light/dark/auto themes, and toggle the four "instant-apply" controls — AI formatting, casual mode, start-with-Windows, and the API key (save + live validate). No hand-editing of config.json for these anymore.

**Architecture:** Settings become shared in-memory state (`Arc<Mutex<Config>>` + `Arc<Mutex<String>>` for the key) that both the running pipeline and a set of Tauri commands read. The pipeline reads formatter/casual/key at dictation time, so a toggle takes effect on the very next dictation with no restart. A second frameless webview window renders the design; its controls call the commands. Microphone selection and hotkey rebinding are intentionally display-only here — they restart live subsystems and get their own milestone (3c).

**Tech Stack:** Existing app. No new dependencies. New Rust module `commands.rs`; new frontend files `src/settings.html`, `src/settings.js`, `src/design-tokens.css`.

**Spec:** `docs/superpowers/specs/2026-08-08-whispeross-v2-design.md` §3 (settings surface), §4. Design source: `docs/design/claude-design-export/WhisperOSS.dc.html` (Artboard 1). User scope decisions (2026-08-09): tray **left-click opens settings**.

## Global Constraints

- Windows-only. Repo root: `C:\projects (code)\15. WhisperOSS redefined` (quote paths). `cargo` runs from `src-tauri\`. Never touch `src-reference\`.
- Design tokens (from `docs/design/claude-design-export/_ds/.../styles.css`): accent light `#ec3013` / dark `#ff563c`, dark bg `#191817` surface `#232120` text `#f3f2f2`, light bg `#f8f7f7` surface `#eae9e9` text `#201e1d`. Square corners. Window 960×640.
- **Deferred to Milestone 4 polish (do NOT build here, but leave clean seams):** acrylic/blur (3b ships an opaque themed window — `transparent: false`), and bundling the Archivo font (3b uses a system font stack: `"Segoe UI Variable Display","Segoe UI",system-ui,sans-serif`).
- **Deferred to Milestone 3c (render as display-only in 3b):** the Microphone dropdown (show current device name, not interactive) and the Change-hotkey button (show the combo, button disabled).
- Instant-apply rule: toggling formatter/casual/theme/autostart or saving a key must take effect with NO app restart. Formatter/casual/key are read live by the pipeline; theme is applied in the window; autostart calls `reconcile` immediately.
- New compiler warnings from your changes: stop and fix. New warnings in untouched code: report in DEVIATIONS. Existing 40 tests stay green.
- Not in this plan: mic live-switch, hotkey capture/rebind, the live status line ("Groq connected · Microphone OK" is rendered static in 3b) — all 3c.

---

### Task 1: Shared config state + live pipeline reads

**Files:**
- Modify: `src-tauri/src/keys.rs` (add `save`)
- Create: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/pipeline.rs` (read formatter/casual/key live)
- Modify: `src-tauri/src/lib.rs` (build shared state, pass to pipeline, `.manage` it)

**Interfaces:**
- Consumes: `config::Config`, `keys`.
- Produces: `state::AppState { config: Arc<Mutex<Config>>, key: Arc<Mutex<String>> }` (Tauri-managed); `keys::save(key: &str) -> bool`. `pipeline::start(app, audio, state: state::AppState)` (changed signature). Task 2's commands consume `AppState`.

- [ ] **Step 1: Add `keys::save` to `src-tauri/src/keys.rs`**

Add to the module (below `load`):

```rust
/// Persist the key into Windows Credential Manager. Returns false on failure.
pub fn save(key: &str) -> bool {
    match keyring::Entry::new(SERVICE, ACCOUNT) {
        Ok(entry) => {
            let ok = entry.set_password(key).is_ok();
            if ok {
                applog::log("api-key-saved");
            } else {
                applog::log("api-key-save-failed");
            }
            ok
        }
        Err(_) => false,
    }
}
```

- [ ] **Step 2: Create `src-tauri/src/state.rs`**

```rust
//! Shared, live application state. Both the running dictation pipeline and
//! the settings-window commands read/write these behind mutexes, so a
//! setting changed in the window takes effect on the next dictation with no
//! restart. The key is kept separate from Config because it lives in
//! Credential Manager, never in config.json.

use std::sync::{Arc, Mutex};

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Mutex<Config>>,
    pub key: Arc<Mutex<String>>,
}

impl AppState {
    pub fn new(config: Config, key: String) -> Self {
        AppState {
            config: Arc::new(Mutex::new(config)),
            key: Arc::new(Mutex::new(key)),
        }
    }
}
```

Add `mod state;` to `src-tauri/src/lib.rs`.

- [ ] **Step 3: Make the pipeline read live values**

In `src-tauri/src/pipeline.rs`, change the signature and setup. Replace the `pub fn start(...)` header and its combo/formatter/client setup with:

```rust
pub fn start(app: tauri::AppHandle, audio: Arc<audio::AudioEngine>, state: crate::state::AppState) {
    let (tx, rx) = channel();
    hook::spawn(tx);

    // Combo is read once here (live rebind is Milestone 3c).
    let combo = {
        let cfg = state.config.lock().unwrap();
        hotkey_logic::parse_combo(&cfg.hotkey).unwrap_or_else(|| {
            applog::log("config-invalid-hotkey-using-default");
            hotkey_logic::parse_combo(&["ctrl".into(), "win".into()]).expect("default combo")
        })
    };
    hook::set_suppression(
        hotkey_logic::combo_other_vk(&combo),
        &combo.iter().copied().filter(|k| k.is_modifier()).collect::<Vec<_>>(),
    );

    let generation = Arc::new(AtomicU64::new(0));
```

Delete the old `let client = Arc::new(GroqClient::new(...))` line and the old `use_formatter`/`casual` locals.

In the `Finish` worker (the `std::thread::spawn(move || { ... })` after `ui.emit(my_gen, "processing", "")`), the worker now needs `state`. Clone it for the worker: before the spawn add `let state = state.clone();`, and inside the worker build the client and read toggles live:

```rust
                    std::thread::spawn(move || {
                        let (key, use_formatter, casual) = {
                            let cfg = state.config.lock().unwrap();
                            (state.key.lock().unwrap().clone(), cfg.use_formatter, cfg.casual_mode)
                        };
                        let client = groq::GroqClient::new(
                            key,
                            groq::PROD_BASE_URL.to_string(),
                            REQUEST_TIMEOUT,
                        );
                        match client.transcribe(wav) {
                            // ... existing match arms unchanged, EXCEPT the
                            // Ok(text) formatter arm uses the `use_formatter`
                            // and `casual` bindings from just above ...
                        }
                    });
```

The formatter arm inside that match is unchanged from 3a (it already references `use_formatter` and `casual`; they are now the live locals). Because `state` is moved into the worker, and multiple dictations spawn multiple workers, keep the `let state = state.clone();` immediately before each `Finish` spawn. Also add `let state_for_loop = state.clone();` at the top of the event loop closure if the borrow checker needs the outer `state` to remain usable — simplest is to `let state = state.clone();` inside the `Finish` arm before the spawn.

- [ ] **Step 4: Build and manage the state in `src-tauri/src/lib.rs`**

Replace the config-load / audio / pipeline block in `setup` with:

```rust
            let cfg = config::load();
            config::save(&cfg);
            autostart::reconcile(cfg.run_on_startup);

            let key = keys::load().unwrap_or_default();
            let app_state = state::AppState::new(cfg.clone(), key.clone());
            app.manage(app_state.clone());

            let audio_engine =
                audio::AudioEngine::start(app.handle().clone(), cfg.input_device.clone());

            if key.is_empty() {
                applog::log("pipeline-started-without-key");
            }
            pipeline::start(app.handle().clone(), audio_engine, app_state);
```

Note: the pipeline now always starts (even with an empty key — a dictation then fails with an "Invalid API key" pill, which is correct once the settings window can fix it live).

- [ ] **Step 5: Verify**

Run: `cargo test` → expected 40 passed (no test count change; this is wiring).
Run: `cargo check` → zero new warnings.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/keys.rs src-tauri/src/state.rs src-tauri/src/pipeline.rs src-tauri/src/lib.rs
git commit -m "feat: shared live settings state read by the pipeline"
```

---

### Task 2: Settings commands

**Files:**
- Create: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod commands;`, register `invoke_handler`)

**Interfaces:**
- Consumes: `state::AppState`, `config`, `keys`, `autostart`, `groq`.
- Produces (the settings frontend calls these): `get_settings`, `set_formatter`, `set_casual`, `set_theme`, `set_autostart`, `save_api_key`, `has_api_key`. Plus a pure tested helper `commands::normalize_theme`.

- [ ] **Step 1: Write the failing test**

Create `src-tauri/src/commands.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_normalization() {
        assert_eq!(normalize_theme("dark"), "dark");
        assert_eq!(normalize_theme("light"), "light");
        assert_eq!(normalize_theme("auto"), "auto");
        assert_eq!(normalize_theme("AUTO"), "auto");
        assert_eq!(normalize_theme("nonsense"), "auto");
        assert_eq!(normalize_theme(""), "auto");
    }
}
```

Add `mod commands;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test commands`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

Add above the test module in `src-tauri/src/commands.rs`:

```rust
//! Tauri commands the settings window calls. Each mutates the shared state,
//! persists to config.json, and (where relevant) applies immediately. The
//! window never touches config.json directly — it goes through these.

use tauri::State;

use crate::{applog, autostart, config, groq, keys, state::AppState};

/// Only three themes are valid; anything else falls back to "auto".
pub fn normalize_theme(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "dark" => "dark".into(),
        "light" => "light".into(),
        _ => "auto".into(),
    }
}

fn persist(state: &AppState) {
    let cfg = state.config.lock().unwrap();
    config::save(&cfg);
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> config::Config {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
pub fn has_api_key(state: State<AppState>) -> bool {
    !state.key.lock().unwrap().is_empty()
}

#[tauri::command]
pub fn set_formatter(state: State<AppState>, value: bool) {
    state.config.lock().unwrap().use_formatter = value;
    persist(&state);
    applog::log("setting-formatter-changed");
}

#[tauri::command]
pub fn set_casual(state: State<AppState>, value: bool) {
    state.config.lock().unwrap().casual_mode = value;
    persist(&state);
    applog::log("setting-casual-changed");
}

#[tauri::command]
pub fn set_theme(state: State<AppState>, value: String) {
    state.config.lock().unwrap().theme = normalize_theme(&value);
    persist(&state);
    applog::log("setting-theme-changed");
}

#[tauri::command]
pub fn set_autostart(state: State<AppState>, value: bool) {
    state.config.lock().unwrap().run_on_startup = value;
    persist(&state);
    autostart::reconcile(value);
}

/// Validate the key against Groq; on success persist it to Credential Manager
/// and update the live key so the next dictation uses it. Returns Ok(()) or
/// a short error message for inline display.
#[tauri::command]
pub fn save_api_key(state: State<AppState>, key: String) -> Result<(), String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("Enter a key".into());
    }
    let client = groq::GroqClient::new(
        key.clone(),
        groq::PROD_BASE_URL.to_string(),
        std::time::Duration::from_secs(15),
    );
    match client.validate_key() {
        Ok(()) => {
            if !keys::save(&key) {
                return Err("Couldn't save to Credential Manager".into());
            }
            *state.key.lock().unwrap() = key;
            Ok(())
        }
        Err(groq::GroqError::Unauthorized) => Err("Groq rejected this key".into()),
        Err(groq::GroqError::Network(_)) => Err("Couldn't reach Groq".into()),
        Err(groq::GroqError::Server(_)) => Err("Groq error — try again".into()),
    }
}
```

- [ ] **Step 4: Register the handlers in `src-tauri/src/lib.rs`**

Add `.invoke_handler(...)` to the builder chain, right after `tauri::Builder::default()`:

```rust
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::has_api_key,
            commands::set_formatter,
            commands::set_casual,
            commands::set_theme,
            commands::set_autostart,
            commands::save_api_key,
        ])
```

- [ ] **Step 5: Verify**

Run: `cargo test commands` → `1 passed` (suite total 41).
Run: `cargo check` → zero new warnings.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat: settings commands (formatter, casual, theme, autostart, key)"
```

---

### Task 3: Second window + tray left-click + close-to-tray

**Files:**
- Modify: `src-tauri/tauri.conf.json` (add the `settings` window)
- Modify: `src-tauri/capabilities/default.json` (add `settings`)
- Modify: `src-tauri/src/lib.rs` (tray left-click shows settings; add "Show settings" menu item; close-to-tray)

**Interfaces:**
- Consumes: nothing new.
- Produces: a `settings` window that opens on tray left-click and hides (not quits) on close.

- [ ] **Step 1: Add the window to `src-tauri/tauri.conf.json`**

Append this object to the `app.windows` array (after the existing `overlay` entry):

```json
{
  "label": "settings",
  "title": "WhisperOSS",
  "url": "settings.html",
  "width": 960,
  "height": 640,
  "resizable": false,
  "maximizable": false,
  "decorations": false,
  "transparent": false,
  "center": true,
  "skipTaskbar": false,
  "visible": false
}
```

- [ ] **Step 2: Allow the window in `src-tauri/capabilities/default.json`**

Set the `windows` array to include both:

```json
"windows": ["overlay", "settings"]
```

- [ ] **Step 3: Tray behavior + close-to-tray in `src-tauri/src/lib.rs`**

Add the imports at the top:

```rust
use tauri::tray::{TrayIconEvent, MouseButton, MouseButtonState};
use tauri::WindowEvent;
```

Add a helper near `position_overlay`:

```rust
pub(crate) fn show_settings(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}
```

Replace the `TrayIconBuilder` block with one that adds a "Show settings" item, handles left-click, and keeps Quit:

```rust
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
```

Add close-to-tray for the settings window at the end of the `setup` closure, before `Ok(())`:

```rust
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
```

- [ ] **Step 4: Verify build**

Run: `cargo test` → 41 passed.
Run: `npm run tauri dev` → the app launches; nothing visible yet (settings.html doesn't exist → Task 4). Left-clicking the tray will error until Task 4 adds the file — that's expected; just confirm it COMPILES and the tray icon appears. Stop the app.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/tauri.conf.json src-tauri/capabilities/default.json src-tauri/src/lib.rs
git commit -m "feat: settings window shell, tray left-click opens, close hides to tray"
```

---

### Task 4: Settings window — HTML + theme tokens

**Files:**
- Create: `src/design-tokens.css`
- Create: `src/settings.html`

**Interfaces:**
- Consumes: nothing (static until Task 5's JS).
- Produces: the rendered window matching the design, theme-switchable via `document.documentElement.dataset.theme`.

- [ ] **Step 1: Create `src/design-tokens.css`**

```css
/* WhisperOSS design tokens (from the Modernist design system export).
   Light is the default; dark applies via OS preference OR an explicit
   data-theme, so "auto" = follow Windows, "light"/"dark" = force. */
:root {
  --bg: #f8f7f7;
  --surface: #eae9e9;
  --text: #201e1d;
  --accent: #ec3013;
  --divider: color-mix(in srgb, #201e1d 30%, transparent);
  --muted: color-mix(in srgb, #201e1d 55%, transparent);
  --font-heading: "Segoe UI Variable Display", "Segoe UI", system-ui, sans-serif;
  --font-body: "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
}
@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
    --bg: #191817;
    --surface: #232120;
    --text: #f3f2f2;
    --accent: #ff563c;
    --divider: color-mix(in srgb, #f3f2f2 30%, transparent);
    --muted: color-mix(in srgb, #f3f2f2 55%, transparent);
  }
}
:root[data-theme="dark"] {
  --bg: #191817;
  --surface: #232120;
  --text: #f3f2f2;
  --accent: #ff563c;
  --divider: color-mix(in srgb, #f3f2f2 30%, transparent);
  --muted: color-mix(in srgb, #f3f2f2 55%, transparent);
}
:root[data-theme="light"] {
  --bg: #f8f7f7;
  --surface: #eae9e9;
  --text: #201e1d;
  --accent: #ec3013;
  --divider: color-mix(in srgb, #201e1d 30%, transparent);
  --muted: color-mix(in srgb, #201e1d 55%, transparent);
}
```

- [ ] **Step 2: Create `src/settings.html`**

```html
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8" />
<link rel="stylesheet" href="design-tokens.css" />
<style>
  * { box-sizing: border-box; }
  html, body { margin: 0; height: 100vh; overflow: hidden; }
  body {
    background: var(--bg); color: var(--text);
    font-family: var(--font-body);
    display: flex; flex-direction: column;
    border: 1px solid var(--divider);
  }
  .titlebar {
    height: 44px; flex: none; display: flex; align-items: center;
    justify-content: space-between; padding-left: 20px;
  }
  .brand { display: flex; align-items: center; gap: 11px; }
  .brand .mark { width: 9px; height: 9px; background: var(--accent); }
  .brand .name {
    font-family: var(--font-heading); font-weight: 700; font-size: 11px;
    letter-spacing: 0.2em;
  }
  .winbtns { display: flex; }
  .winbtn {
    width: 46px; height: 44px; display: flex; align-items: center;
    justify-content: center; color: var(--muted); background: none;
    border: none; cursor: pointer;
  }
  .winbtn:hover { background: color-mix(in srgb, var(--text) 8%, transparent); }
  .winbtn.close:hover { background: var(--accent); color: #fff; }

  .hero { padding: 32px 48px 26px; flex: none; }
  .hero .kicker {
    font-size: 11px; letter-spacing: 0.18em; font-weight: 700;
    color: var(--muted); margin-bottom: 18px;
  }
  .hero .combo {
    font-family: var(--font-heading); font-weight: 700; font-size: 30px;
    letter-spacing: -0.02em; display: flex; align-items: center; gap: 11px;
    flex-wrap: wrap;
  }
  .key { border: 2px solid var(--divider); padding: 2px 12px; font-size: 23px; }
  .plus { font-size: 22px; color: var(--muted); }
  .btn {
    font-family: var(--font-heading); font-weight: 700; cursor: pointer;
    border: 1px solid var(--divider); background: none; color: var(--text);
  }
  .btn:disabled { opacity: 0.5; cursor: default; }
  .btn-primary { background: var(--accent); color: #fff; border-color: var(--accent); }
  .change { margin-top: 20px; font-size: 12px; padding: 7px 13px; }

  .divider { height: 2px; background: var(--divider); flex: none; }
  .rows { flex: 1; display: flex; flex-direction: column; padding: 0 48px; }
  .row {
    flex: 1; display: flex; align-items: center; justify-content: space-between;
    gap: 24px;
  }
  .row + .row { border-top: 1px solid color-mix(in srgb, var(--text) 14%, transparent); }
  .row .label { font-family: var(--font-heading); font-weight: 700; font-size: 15px; }
  .row .desc { font-size: 12px; color: var(--muted); margin-top: 3px; }
  .row .ctl { display: flex; align-items: center; gap: 8px; }

  input[type="text"], input[type="password"] {
    width: 232px; font-size: 13px; padding: 9px 12px; background: var(--surface);
    color: var(--text); border: 1px solid var(--divider); font-family: var(--font-body);
  }
  .icon-btn { padding: 9px 10px; }

  .toggle {
    width: 48px; height: 26px; border: 2px solid var(--divider); padding: 2px;
    display: flex; align-items: center; justify-content: flex-start; cursor: pointer;
  }
  .toggle .knob { width: 18px; height: 18px; background: var(--muted); }
  .toggle.on { border-color: var(--accent); background: var(--accent); justify-content: flex-end; }
  .toggle.on .knob { background: var(--bg); }

  .seg { display: flex; border: 1px solid var(--divider); }
  .seg button {
    padding: 8px 18px; font-size: 13px; font-family: var(--font-heading);
    background: none; color: var(--text); border: none; cursor: pointer;
  }
  .seg button + button { border-left: 1px solid var(--divider); }
  .seg button.active { background: var(--text); color: var(--bg); font-weight: 700; }

  .mic-static {
    width: 232px; border: 1px solid var(--divider); padding: 9px 12px;
    font-size: 13px; color: var(--muted);
  }

  .statusbar {
    height: 46px; flex: none;
    border-top: 1px solid color-mix(in srgb, var(--text) 14%, transparent);
    display: flex; align-items: center; justify-content: space-between;
    padding: 0 48px; font-size: 12px; color: var(--muted);
  }
  .status-left { display: flex; align-items: center; gap: 10px; }
  .status-left .dot { width: 7px; height: 7px; background: var(--accent); }
  .keyfeedback { font-size: 12px; margin-left: 4px; }
  .keyfeedback.ok { color: #3fb970; }
  .keyfeedback.err { color: var(--accent); }
</style>
</head>
<body>
  <div class="titlebar" data-tauri-drag-region>
    <div class="brand"><div class="mark"></div><div class="name">WHISPEROSS</div></div>
    <div class="winbtns">
      <button class="winbtn" id="min" title="Minimize">
        <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.4"><path d="M1 6h10"/></svg>
      </button>
      <button class="winbtn close" id="close" title="Close to tray">
        <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.4"><path d="M1.5 1.5l9 9M10.5 1.5l-9 9"/></svg>
      </button>
    </div>
  </div>

  <div class="hero">
    <div class="kicker">GLOBAL HOTKEY</div>
    <div class="combo">
      <span>Hold</span>
      <span class="key" id="key1">Ctrl</span>
      <span class="plus">+</span>
      <span class="key" id="key2">Win</span>
      <span>— speak — release</span>
    </div>
    <button class="btn change" id="change-hotkey" disabled title="Rebind arrives in the next update">Change hotkey</button>
  </div>
  <div class="divider"></div>

  <div class="rows">
    <div class="row">
      <div>
        <div class="label">Groq API key</div>
        <div class="desc">Stored locally on this PC <span class="keyfeedback" id="key-feedback"></span></div>
      </div>
      <div class="ctl">
        <input type="password" id="api-key" placeholder="gsk_…" />
        <button class="btn icon-btn" id="toggle-key" title="Show key">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M2 12s3.6-6.5 10-6.5S22 12 22 12s-3.6 6.5-10 6.5S2 12 2 12z"/><circle cx="12" cy="12" r="2.6"/></svg>
        </button>
        <button class="btn btn-primary" id="save-key" style="padding:9px 16px">Save</button>
      </div>
    </div>

    <div class="row">
      <div>
        <div class="label">AI formatting</div>
        <div class="desc">Cleans punctuation and paragraph breaks</div>
      </div>
      <div class="toggle" id="formatter" role="switch"><div class="knob"></div></div>
    </div>

    <div class="row">
      <div>
        <div class="label">Casual mode</div>
        <div class="desc">Lowercase, light emoji</div>
      </div>
      <div class="toggle" id="casual" role="switch"><div class="knob"></div></div>
    </div>

    <div class="row">
      <div>
        <div class="label">Microphone</div>
        <div class="desc">Input device used while dictating · picker in next update</div>
      </div>
      <div class="mic-static" id="mic-name">System default</div>
    </div>

    <div class="row">
      <div>
        <div class="label">Theme</div>
        <div class="desc">Follows Windows by default</div>
      </div>
      <div class="seg" id="theme">
        <button data-theme="auto">Auto</button>
        <button data-theme="light">Light</button>
        <button data-theme="dark">Dark</button>
      </div>
    </div>

    <div class="row">
      <div>
        <div class="label">Start with Windows</div>
        <div class="desc">Runs quietly in the tray</div>
      </div>
      <div class="toggle" id="autostart" role="switch"><div class="knob"></div></div>
    </div>
  </div>

  <div class="statusbar">
    <div class="status-left"><span class="dot"></span><span id="status-text">Ready</span></div>
    <div>v1.0</div>
  </div>

  <script src="settings.js"></script>
</body>
</html>
```

- [ ] **Step 3: Commit (visual wiring is Task 5; verify it renders)**

Run `npm run tauri dev`, left-click the tray icon. Expected: the window opens, laid out like the design (hero hotkey, six rows, status bar), in your OS theme. Controls don't respond yet. Close button hides it; left-click tray reopens. Stop the app.

```bash
git add src/design-tokens.css src/settings.html
git commit -m "feat: settings window markup and theme tokens"
```

---

### Task 5: Settings window — wiring

**Files:**
- Create: `src/settings.js`

**Interfaces:**
- Consumes: the Task 2 commands and the Task 4 markup.
- Produces: fully live formatter/casual/theme/autostart toggles, API key save with inline feedback, window buttons.

- [ ] **Step 1: Create `src/settings.js`**

```js
const { invoke } = window.__TAURI__.core;
const { getCurrentWindow } = window.__TAURI__.window;

const win = getCurrentWindow();
document.getElementById("min").onclick = () => win.minimize();
document.getElementById("close").onclick = () => win.hide();

const el = (id) => document.getElementById(id);

function paintToggle(node, on) {
  node.classList.toggle("on", on);
}

function applyTheme(theme) {
  if (theme === "auto") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = theme;
  for (const b of el("theme").children) {
    b.classList.toggle("active", b.dataset.theme === theme);
  }
}

async function load() {
  const cfg = await invoke("get_settings");
  paintToggle(el("formatter"), cfg.use_formatter);
  paintToggle(el("casual"), cfg.casual_mode);
  paintToggle(el("autostart"), cfg.run_on_startup);
  applyTheme(cfg.theme);

  // Hotkey display (rebind is a later update).
  const keys = cfg.hotkey.map((k) => k.charAt(0).toUpperCase() + k.slice(1));
  el("key1").textContent = keys[0] || "Ctrl";
  el("key2").textContent = keys[1] || "Win";

  el("mic-name").textContent = cfg.input_device || "System default";

  const hasKey = await invoke("has_api_key");
  if (hasKey) {
    el("api-key").placeholder = "••••••••••••••••";
    setKeyFeedback("Saved", "ok");
  }
}

function setKeyFeedback(text, kind) {
  const f = el("key-feedback");
  f.textContent = text ? `· ${text}` : "";
  f.className = `keyfeedback ${kind || ""}`;
}

// --- toggles: optimistic paint, then persist ---
function wireToggle(id, command) {
  const node = el(id);
  node.onclick = async () => {
    const next = !node.classList.contains("on");
    paintToggle(node, next);
    await invoke(command, { value: next });
  };
}
wireToggle("formatter", "set_formatter");
wireToggle("casual", "set_casual");
wireToggle("autostart", "set_autostart");

// --- theme ---
for (const b of el("theme").children) {
  b.onclick = async () => {
    applyTheme(b.dataset.theme);
    await invoke("set_theme", { value: b.dataset.theme });
  };
}

// --- api key ---
el("toggle-key").onclick = () => {
  const input = el("api-key");
  input.type = input.type === "password" ? "text" : "password";
};
el("save-key").onclick = async () => {
  const key = el("api-key").value.trim();
  if (!key) { setKeyFeedback("Enter a key", "err"); return; }
  setKeyFeedback("Checking…", "");
  el("save-key").disabled = true;
  try {
    await invoke("save_api_key", { key });
    setKeyFeedback("Saved", "ok");
    el("api-key").value = "";
    el("api-key").placeholder = "••••••••••••••••";
  } catch (msg) {
    setKeyFeedback(String(msg), "err");
  } finally {
    el("save-key").disabled = false;
  }
};

load();
```

- [ ] **Step 2: Verify build**

Run: `cargo test` → 41 passed. `npm run tauri dev` compiles and the window opens. Full behavior is verified in Task 6.

- [ ] **Step 3: Commit**

```bash
git add src/settings.js
git commit -m "feat: settings window wiring - live toggles, theme, key save"
```

---

### Task 6: Verification and milestone report

**Files:**
- Create: `docs/reports/milestone-3b-results.md`

- [ ] **Step 1: Protocol (human at the keyboard)**

`npm run tauri dev`, then:

1. **Open/close:** left-click the tray icon → window opens. Click the ✕ → it hides (app still running, tray icon still there). Left-click tray again → reopens. Right-click tray → menu shows "Show settings" and "Quit"; "Show settings" opens it.
2. **Reflects real state:** the toggles match your current config.json (the one you restored at the end of 3a). The hotkey hero shows your combo.
3. **Formatter live:** turn AI formatting ON in the window (don't restart). Immediately dictate a messy sentence. Expected: it comes back punctuated — the toggle applied with no restart. Turn it OFF, dictate again → raw text.
4. **Casual live:** turn Casual ON, dictate "hey what's up three crying emojis". Expected: lowercase + 😭😭😭.
5. **Theme:** click Light / Dark / Auto. Expected: the window recolors instantly; Auto follows your Windows theme. Close and reopen → the theme you chose persists.
6. **Autostart:** toggle Start-with-Windows OFF. In PowerShell `reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v WhisperOSS` → value gone. Toggle ON → value present.
7. **API key — good:** paste your real key, click the eye to confirm it, click Save. Expected: "Checking…" then "Saved", field clears to dots. Dictate → works.
8. **API key — bad:** type `gsk_wrong`, Save. Expected: red "Groq rejected this key"; dictation still uses the previously saved good key.
9. **Persistence:** fully quit (tray → Quit), relaunch, open settings. Expected: every toggle/theme is where you left it.

- [ ] **Step 2: Write `docs/reports/milestone-3b-results.md`** — one row per check above, automated-test count, verdict GO / NO-GO for Milestone 3c, deviations list.

- [ ] **Step 3: Commit**

```bash
git add docs/reports/milestone-3b-results.md
git commit -m "docs: milestone 3b settings window results"
```

If any check fails: STOP and report with log lines and the failing command output.
