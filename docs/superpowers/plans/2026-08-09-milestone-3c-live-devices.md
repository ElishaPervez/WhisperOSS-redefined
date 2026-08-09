# Milestone 3c — Live Microphone + Hotkey Rebind Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** The two settings that need a live subsystem restart become real: pick a microphone and it takes effect immediately, and press a new key combination to rebind the hotkey without restarting the app.

**Why this is its own milestone:** every other setting is a value the next dictation reads. These two tear down and rebuild something that is already running — the audio stream and the keyboard combo tracker. The mic stream object cannot move between threads, and the rebind capture has to swallow every key on the machine while it listens, so both need care that a toggle does not.

**Tech Stack:** Existing app. No new dependencies.

## Global Constraints

- Windows-only. Repo root: `C:\projects (code)\15. WhisperOSS redefined` (quote paths in shell commands). `cargo` runs from `src-tauri\`. Never touch `src-reference\`.
- Shell: commands are written for PowerShell. Adapt freely if your shell differs — that is a HOW decision, report it in DEVIATIONS.
- The existing 42 tests must stay green. Task 1 adds 10 → **52 total**. Zero new compiler warnings.
- Pure logic is tested; Windows-integration behaviour is verified by the human protocol in Task 7. Do not write tests that need real hardware.
- Do not pause between tasks. Post a short report after each commit and continue. Stop only for: the human-only step (Task 7), a failed verification, or a mismatch that is not mechanical.

**Safety rule that outranks everything else in this plan:** while the app is capturing a new hotkey it swallows every keystroke system-wide. There must be no path where that state can persist. The watchdog in Task 5 is not optional and its timeout must never be raised.

---

### Task 1: Combo naming + the capture state machine (pure logic)

**Files:**
- Modify: `src-tauri/src/hotkey_logic.rs`

**Interfaces:**
- Produces: `canonical(Vec<Key>) -> Vec<Key>`, `combo_names(&[Key]) -> Option<Vec<String>>`, `CaptureBuffer`, `Capture`, `CAPTURE_TIMEOUT_MS`.
- Consumes: nothing new.

- [ ] **Step 1: Write the failing tests — append to the existing `mod tests` in `src-tauri/src/hotkey_logic.rs`:**

```rust
    fn down(k: Key) -> KeyEvent { KeyEvent::Down(k, 0) }
    fn up(k: Key) -> KeyEvent { KeyEvent::Up(k, 0) }

    #[test]
    fn canonical_order_is_press_order_independent() {
        let a = canonical(vec![Key::Other(0x20), Key::Ctrl]);
        let b = canonical(vec![Key::Ctrl, Key::Other(0x20)]);
        assert_eq!(a, b);
        assert_eq!(a, vec![Key::Ctrl, Key::Other(0x20)]);
        assert_eq!(
            canonical(vec![Key::Shift, Key::Win, Key::Ctrl]),
            vec![Key::Ctrl, Key::Win, Key::Shift]
        );
    }

    #[test]
    fn combo_names_maps_every_supported_key() {
        assert_eq!(
            combo_names(&[Key::Ctrl, Key::Win, Key::Alt, Key::Shift]).unwrap(),
            vec!["ctrl", "win", "alt", "shift"]
        );
        assert_eq!(combo_names(&[Key::Other(0x20)]).unwrap(), vec!["space"]);
        assert_eq!(combo_names(&[Key::Other(0x09)]).unwrap(), vec!["tab"]);
        assert_eq!(combo_names(&[Key::Other(0x14)]).unwrap(), vec!["capslock"]);
        assert_eq!(combo_names(&[Key::Other(0x44)]).unwrap(), vec!["d"]);
        assert_eq!(combo_names(&[Key::Other(0x35)]).unwrap(), vec!["5"]);
    }

    #[test]
    fn combo_names_rejects_keys_with_no_config_name() {
        // F1 has no name in key_from_name, so it can never be persisted.
        assert!(combo_names(&[Key::Ctrl, Key::Other(0x70)]).is_none());
    }

    #[test]
    fn combo_names_round_trips_through_parse_combo() {
        for input in [
            vec!["ctrl".to_string(), "win".to_string()],
            vec!["ctrl".to_string(), "space".to_string()],
            vec!["alt".to_string(), "shift".to_string(), "d".to_string()],
        ] {
            let keys = parse_combo(&input).unwrap();
            assert_eq!(combo_names(&canonical(keys)).unwrap(), input);
        }
    }

    #[test]
    fn capture_completes_on_first_release() {
        let mut c = CaptureBuffer::new();
        assert_eq!(c.on_event(down(Key::Ctrl)), Capture::Pending(vec![Key::Ctrl]));
        assert_eq!(
            c.on_event(down(Key::Win)),
            Capture::Pending(vec![Key::Ctrl, Key::Win])
        );
        assert_eq!(
            c.on_event(up(Key::Ctrl)),
            Capture::Done(vec![Key::Ctrl, Key::Win])
        );
    }

    #[test]
    fn capture_is_press_order_independent() {
        let mut c = CaptureBuffer::new();
        c.on_event(down(Key::Other(0x20)));
        c.on_event(down(Key::Ctrl));
        assert_eq!(
            c.on_event(up(Key::Other(0x20))),
            Capture::Done(vec![Key::Ctrl, Key::Other(0x20)])
        );
    }

    #[test]
    fn capture_ignores_key_repeat() {
        let mut c = CaptureBuffer::new();
        c.on_event(down(Key::Ctrl));
        c.on_event(down(Key::Ctrl));
        c.on_event(down(Key::Win));
        assert_eq!(
            c.on_event(up(Key::Win)),
            Capture::Done(vec![Key::Ctrl, Key::Win])
        );
    }

    #[test]
    fn escape_cancels_capture() {
        let mut c = CaptureBuffer::new();
        c.on_event(down(Key::Ctrl));
        assert_eq!(c.on_event(down(Key::Other(0x1B))), Capture::Cancelled);
    }

    #[test]
    fn stray_single_tap_keeps_listening() {
        let mut c = CaptureBuffer::new();
        c.on_event(down(Key::Ctrl));
        assert_eq!(c.on_event(up(Key::Ctrl)), Capture::Pending(Vec::new()));
        // still usable afterwards
        c.on_event(down(Key::Alt));
        c.on_event(down(Key::Other(0x44)));
        assert_eq!(
            c.on_event(up(Key::Alt)),
            Capture::Done(vec![Key::Alt, Key::Other(0x44)])
        );
    }

    #[test]
    fn unusable_combos_are_rejected() {
        // two regular keys, no modifier
        let mut c = CaptureBuffer::new();
        c.on_event(down(Key::Other(0x41)));
        c.on_event(down(Key::Other(0x42)));
        assert_eq!(c.on_event(up(Key::Other(0x41))), Capture::Invalid);
        // a key that cannot be written to config.json
        let mut c = CaptureBuffer::new();
        c.on_event(down(Key::Ctrl));
        c.on_event(down(Key::Other(0x70)));
        assert_eq!(c.on_event(up(Key::Ctrl)), Capture::Invalid);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test hotkey_logic`
Expected: compile failure — `canonical`, `combo_names`, `CaptureBuffer`, `Capture` do not exist.

- [ ] **Step 3: Implement.** Update the module doc comment's `M3b` reference to `M3c`, then add the following to `src-tauri/src/hotkey_logic.rs`, above `pub struct HoldTracker`:

```rust
/// How long the app will listen for a new combo before giving up. While it
/// listens it swallows every key on the machine, so this is a hard ceiling,
/// not a preference.
pub const CAPTURE_TIMEOUT_MS: u64 = 6_000;

const VK_ESCAPE: u32 = 0x1B;

fn rank(k: &Key) -> u8 {
    match k {
        Key::Ctrl => 0,
        Key::Win => 1,
        Key::Alt => 2,
        Key::Shift => 3,
        Key::Other(_) => 4,
    }
}

/// One fixed order for every combo, so pressing Space-then-Ctrl and
/// Ctrl-then-Space save and display identically.
pub fn canonical(mut keys: Vec<Key>) -> Vec<Key> {
    keys.sort_by_key(rank);
    keys
}

fn name_of(k: &Key) -> Option<String> {
    Some(match k {
        Key::Ctrl => "ctrl".into(),
        Key::Win => "win".into(),
        Key::Alt => "alt".into(),
        Key::Shift => "shift".into(),
        Key::Other(0x20) => "space".into(),
        Key::Other(0x09) => "tab".into(),
        Key::Other(0x14) => "capslock".into(),
        Key::Other(vk) => {
            let c = char::from_u32(*vk)?;
            if c.is_ascii_uppercase() || c.is_ascii_digit() {
                c.to_ascii_lowercase().to_string()
            } else {
                return None;
            }
        }
    })
}

/// Inverse of key_from_name. None when any key has no config name (F-keys,
/// media keys): such a combo cannot be persisted, so it must be refused at
/// capture time rather than silently lost on the next launch.
pub fn combo_names(keys: &[Key]) -> Option<Vec<String>> {
    keys.iter().map(name_of).collect()
}

#[derive(Debug, Clone, PartialEq)]
pub enum Capture {
    /// Keys held so far — repaint the preview.
    Pending(Vec<Key>),
    /// A usable combo, in canonical order. Persist and apply it.
    Done(Vec<Key>),
    /// Two or more keys that cannot work as a hotkey. Keep the old one.
    Invalid,
    Cancelled,
}

/// Watches keys during a rebind. The combo is whatever is held down when the
/// user lets go of the first key.
pub struct CaptureBuffer {
    keys: Vec<Key>,
}

impl CaptureBuffer {
    pub fn new() -> Self {
        CaptureBuffer { keys: Vec::new() }
    }

    pub fn on_event(&mut self, ev: KeyEvent) -> Capture {
        match ev {
            KeyEvent::Down(Key::Other(VK_ESCAPE), _) => Capture::Cancelled,
            KeyEvent::Down(key, _) => {
                if !self.keys.contains(&key) {
                    self.keys.push(key);
                }
                Capture::Pending(canonical(self.keys.clone()))
            }
            KeyEvent::Up(_, _) => {
                if self.keys.len() < 2 {
                    // A stray tap of one key: forget it and keep listening.
                    self.keys.clear();
                    return Capture::Pending(Vec::new());
                }
                let keys = canonical(std::mem::take(&mut self.keys));
                match combo_names(&keys) {
                    Some(names) if parse_combo(&names).is_some() => Capture::Done(keys),
                    _ => Capture::Invalid,
                }
            }
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test` → expected **52 passed**.
Run: `cargo check` → zero warnings.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/hotkey_logic.rs
git commit -m "feat: canonical combo names and hotkey capture state machine"
```

---

### Task 2: Capture mode in the keyboard hook

**Files:**
- Modify: `src-tauri/src/hook.rs`

**Interfaces:**
- Produces: `hook::set_capture(bool)`.
- Consumes: nothing.

- [ ] **Step 1: Add the capture flag.** In `src-tauri/src/hook.rs`, change the atomics import to include `AtomicBool`:

```rust
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
```

Add below the existing statics:

```rust
// While true, every key is eaten. Only set by begin_hotkey_capture, which
// also arms a watchdog that clears it no matter what happens next.
static CAPTURE: AtomicBool = AtomicBool::new(false);
```

Add next to `set_suppression`:

```rust
/// Swallow every key while a new combo is being recorded, so pressing Win
/// cannot open the Start menu and the combo cannot leak into whatever app
/// is focused. Key events are still forwarded to the pipeline.
pub fn set_capture(on: bool) {
    CAPTURE.store(on, Ordering::SeqCst);
}
```

- [ ] **Step 2: Eat the keys.** In `hook_proc`, immediately after the `if let Some(tx) = SENDER.get() { ... }` block and **before** the `let target = SUPPRESS_VK.load(...)` line, insert:

```rust
            if CAPTURE.load(Ordering::SeqCst) {
                return LRESULT(1);
            }
```

(Both the press and the release are swallowed, so no app can be left holding a modifier it never saw released.)

- [ ] **Step 3: Verify**

Run: `cargo test` → still **52 passed**. `cargo check` → zero warnings.
(Behaviour is verified by Task 7 — nothing calls `set_capture` yet, so expect a dead-code warning ONLY if you stop here; Task 5 wires it up. If `cargo check` warns at this point, that is expected and resolves in Task 5 — do not add an allow attribute.)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/hook.rs
git commit -m "feat: keyboard hook capture mode swallows keys during rebind"
```

---

### Task 3: Shared handles for the live subsystems

**Files:**
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Produces: `AppState { config, key, audio, generation, capturing, capture_gen }`.
- Consumes: `audio::AudioEngine`.

The settings commands need to reach the running mic stream and the running key tracker. Everything they touch moves into `AppState`, including the dictation generation counter that currently lives inside the pipeline.

- [ ] **Step 1: Replace the body of `src-tauri/src/state.rs` below the doc comment with:**

```rust
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};

use crate::audio::AudioEngine;
use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Mutex<Config>>,
    pub key: Arc<Mutex<String>>,
    /// The running mic stream. Changing the device swaps it in place.
    pub audio: Arc<AudioEngine>,
    /// Bumped whenever a dictation is superseded; a stale worker checks this
    /// before it touches the pill.
    pub generation: Arc<AtomicU64>,
    /// True from "Change hotkey" until the combo lands, is refused, or times
    /// out. While true the machine's keyboard is being swallowed.
    pub capturing: Arc<AtomicBool>,
    /// Stops a watchdog from cancelling a capture session it did not start.
    pub capture_gen: Arc<AtomicU64>,
}

impl AppState {
    pub fn new(config: Config, key: String, audio: Arc<AudioEngine>) -> Self {
        AppState {
            config: Arc::new(Mutex::new(config)),
            key: Arc::new(Mutex::new(key)),
            audio,
            generation: Arc::new(AtomicU64::new(0)),
            capturing: Arc::new(AtomicBool::new(false)),
            capture_gen: Arc::new(AtomicU64::new(0)),
        }
    }
}
```

- [ ] **Step 2: Reorder setup in `src-tauri/src/lib.rs`** so the audio engine exists before the state that holds it. Replace the block from `let key = keys::load()` through `pipeline::start(...)` with:

```rust
            let key = keys::load().unwrap_or_default();
            let audio_engine =
                audio::AudioEngine::start(app.handle().clone(), cfg.input_device.clone());
            let app_state = state::AppState::new(cfg.clone(), key.clone(), audio_engine);
            app.manage(app_state.clone());

            if key.is_empty() {
                applog::log("pipeline-started-without-key");
            }
            pipeline::start(app.handle().clone(), app_state);
```

- [ ] **Step 3: Verify**

Run: `cargo check`. Expected: errors in `pipeline.rs` only (its `start` still takes three arguments and builds its own generation counter). That is Task 4's job — proceed.

- [ ] **Step 4: Commit** (after Task 4 compiles; state and pipeline change together)

Skip the commit here. Continue straight to Task 4 and commit both together.

---

### Task 4: Live combo swap + capture routing in the pipeline

**Files:**
- Modify: `src-tauri/src/pipeline.rs`

**Interfaces:**
- Produces: `pipeline::start(app, state)`; emits the `hotkey` event to the settings window.
- Consumes: `AppState`, `CaptureBuffer`, `combo_names`, `hook::set_capture`.

- [ ] **Step 1: Rework `pipeline::start`.** Replace everything from `pub fn start(` down to (but not including) the `/// The AI rewrite runs when EITHER toggle is on.` comment with:

```rust
/// The combo the tracker should be using right now, read from config.
fn combo_from_config(state: &crate::state::AppState) -> Vec<hotkey_logic::Key> {
    let cfg = state.config.lock().unwrap();
    hotkey_logic::parse_combo(&cfg.hotkey).unwrap_or_else(|| {
        applog::log("config-invalid-hotkey-using-default");
        hotkey_logic::parse_combo(&["ctrl".into(), "win".into()]).expect("default combo")
    })
}

fn apply_combo(combo: &[hotkey_logic::Key]) {
    hook::set_suppression(
        hotkey_logic::combo_other_vk(combo),
        &combo.iter().copied().filter(|k| k.is_modifier()).collect::<Vec<_>>(),
    );
}

/// Tell the settings window what the rebind is doing. `keys` that have no
/// config name (an F-key mid-press) come through as an empty preview; the
/// release then reports them as invalid.
fn emit_hotkey(app: &tauri::AppHandle, phase: &str, keys: &[hotkey_logic::Key]) {
    let names = hotkey_logic::combo_names(keys).unwrap_or_default();
    let _ = app.emit("hotkey", serde_json::json!({ "phase": phase, "keys": names }));
}

fn end_capture(state: &crate::state::AppState) {
    state.capturing.store(false, Ordering::SeqCst);
    state.capture_gen.fetch_add(1, Ordering::SeqCst);
    hook::set_capture(false);
}

pub fn start(app: tauri::AppHandle, state: crate::state::AppState) {
    let (tx, rx) = channel();
    hook::spawn(tx);

    let audio = state.audio.clone();
    let generation = state.generation.clone();
    let combo = combo_from_config(&state);
    apply_combo(&combo);

    std::thread::spawn(move || {
        let mut tracker = hotkey_logic::HoldTracker::new(combo);
        let mut capture = hotkey_logic::CaptureBuffer::new();
        let mut was_capturing = false;

        for ev in rx {
            if state.capturing.load(Ordering::SeqCst) {
                if !was_capturing {
                    capture = hotkey_logic::CaptureBuffer::new();
                    was_capturing = true;
                }
                match capture.on_event(ev) {
                    hotkey_logic::Capture::Pending(keys) => {
                        emit_hotkey(&app, "preview", &keys);
                    }
                    hotkey_logic::Capture::Done(keys) => {
                        let names =
                            hotkey_logic::combo_names(&keys).expect("validated at capture");
                        {
                            let mut cfg = state.config.lock().unwrap();
                            cfg.hotkey = names;
                            crate::config::save(&cfg);
                        }
                        apply_combo(&keys);
                        tracker = hotkey_logic::HoldTracker::new(keys.clone());
                        end_capture(&state);
                        was_capturing = false;
                        applog::log("hotkey-rebound");
                        emit_hotkey(&app, "set", &keys);
                    }
                    hotkey_logic::Capture::Invalid => {
                        end_capture(&state);
                        was_capturing = false;
                        applog::log("hotkey-capture-invalid");
                        emit_hotkey(&app, "invalid", &[]);
                    }
                    hotkey_logic::Capture::Cancelled => {
                        end_capture(&state);
                        was_capturing = false;
                        applog::log("hotkey-capture-cancelled");
                        emit_hotkey(&app, "cancelled", &[]);
                    }
                }
                continue;
            }

            if was_capturing {
                // Capture ended from outside (watchdog, or the window lost
                // focus). Rebuild the tracker so keys held during capture
                // cannot look like the start of a dictation.
                was_capturing = false;
                tracker = hotkey_logic::HoldTracker::new(combo_from_config(&state));
            }

            match tracker.on_event(ev) {
                hotkey_logic::Action::None => {}
                hotkey_logic::Action::Start => {
                    let my_gen = generation.fetch_add(1, Ordering::SeqCst) + 1;
                    let ui = Ui { app: app.clone(), generation: generation.clone() };
                    if !audio.is_healthy() {
                        applog::log("recording-refused-no-mic");
                        std::thread::spawn(move || ui.show_error(my_gen, "No mic detected"));
                        continue;
                    }
                    audio.start_recording();
                    ui.show(my_gen);
                    ui.emit(my_gen, "listening", "");
                    applog::log("recording-start");
                }
                hotkey_logic::Action::Cancel => {
                    let _ = audio.stop_recording();
                    let my_gen = generation.load(Ordering::SeqCst);
                    let ui = Ui { app: app.clone(), generation: generation.clone() };
                    std::thread::spawn(move || ui.fade_out_and_hide(my_gen));
                    applog::log("recording-cancel-short-tap");
                }
                hotkey_logic::Action::Finish { held_ms } => {
                    let (samples, rate) = audio.stop_recording();
                    applog::log(&format!(
                        "recording-finish held_ms={held_ms} samples={}",
                        samples.len()
                    ));
                    let my_gen = generation.load(Ordering::SeqCst);
                    let ui = Ui { app: app.clone(), generation: generation.clone() };

                    if dsp::is_effectively_silent(&samples) {
                        applog::log("silent-discarded");
                        std::thread::spawn(move || ui.fade_out_and_hide(my_gen));
                        continue;
                    }

                    ui.emit(my_gen, "processing", "");
                    let wav = dsp::encode_wav_mono16(
                        &dsp::resample_to_16k(&samples, rate),
                        16_000,
                    );
                    let state = state.clone();
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
                            Ok(_) if !ui.current(my_gen) => {
                                applog::log("result-discarded-stale");
                                // No UI touches: a newer dictation owns the pill.
                            }
                            Ok(text) if text.is_empty() => {
                                applog::log("empty-transcript");
                                ui.fade_out_and_hide(my_gen);
                            }
                            Ok(text) => {
                                let final_text = if wants_formatting(use_formatter, casual) {
                                    match client.format_text(&text, casual) {
                                        Ok(f) if !f.is_empty() => f,
                                        Ok(_) => text.clone(),
                                        Err(e) => {
                                            let (m, d) = overlay_state::describe_error(&e);
                                            applog::log(&format!(
                                                "formatter-failed-fallback-raw {m} {d}"
                                            ));
                                            text.clone()
                                        }
                                    }
                                } else {
                                    text.clone()
                                };
                                if paste(&final_text) {
                                    ui.emit(my_gen, "success", "");
                                    std::thread::sleep(Duration::from_millis(
                                        overlay_state::SUCCESS_HOLD_MS,
                                    ));
                                    ui.fade_out_and_hide(my_gen);
                                } else {
                                    ui.show_error(my_gen, "Couldn't paste safely");
                                }
                            }
                            Err(e) => {
                                let (message, detail) = overlay_state::describe_error(&e);
                                applog::log(&format!("transcribe-error {message} {detail}"));
                                ui.show_error(my_gen, message);
                            }
                        }
                    });
                }
            }
        }
    });
}
```

Also remove the now-unused `audio` import if the compiler flags it, and drop `use std::sync::Arc;` only if it becomes unused — `Ui` still holds an `Arc`, so it should stay.

- [ ] **Step 2: Verify**

Run: `cargo test` → **52 passed**. `cargo check` → zero warnings.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/state.rs src-tauri/src/lib.rs src-tauri/src/pipeline.rs
git commit -m "feat: shared live handles and in-place hotkey rebind"
```

---

### Task 5: Live microphone swap + the four new commands

**Files:**
- Modify: `src-tauri/src/audio.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Produces: `AudioEngine::switch_device`, commands `list_microphones`, `set_microphone`, `begin_hotkey_capture`, `cancel_hotkey_capture`.

The mic stream cannot be moved between threads, so it is built, held, and dropped on one dedicated thread. That thread currently parks forever; it now waits on a channel instead and rebuilds the stream whenever a new device name arrives.

- [ ] **Step 1: Rework the stream thread in `src-tauri/src/audio.rs`.**

Update the module doc comment: `Default input device only in M1 (device picker is M3).` becomes `The device is swappable at runtime (M3c) — see switch_device.`

Remove `#[allow(dead_code)]` from `list_input_devices` (Task 5 gives it a caller).

Add to the imports:

```rust
use std::sync::mpsc::{channel, Sender};
```

Add a field to `pub struct AudioEngine`:

```rust
    /// Carries the next device name to the stream thread. The stream object
    /// itself can only exist on that thread.
    device_tx: Mutex<Sender<Option<String>>>,
```

Replace the body of `pub fn start` from its first line down to (and including) the first `std::thread::spawn(...)` block with:

```rust
    pub fn start(app: tauri::AppHandle, preferred: Option<String>) -> Arc<AudioEngine> {
        let (device_tx, device_rx) = channel::<Option<String>>();
        let engine = Arc::new(AudioEngine {
            ring: Mutex::new(VecDeque::new()),
            recording: Mutex::new(None),
            rate: AtomicU32::new(16_000),
            peak: AtomicU16::new(0),
            healthy: AtomicBool::new(false),
            device_tx: Mutex::new(device_tx),
        });

        // The cpal stream is not Send: build it, hold it, and drop it all on
        // this one thread. It blocks here until a device change arrives.
        let e = engine.clone();
        std::thread::spawn(move || {
            let mut stream = open(&e, &preferred);
            for next in device_rx {
                drop(stream.take());
                e.reset_buffers();
                stream = open(&e, &next);
            }
        });
```

Add these methods inside `impl AudioEngine`, next to `is_healthy`:

```rust
    /// Change the capture device without restarting the app. The pre-roll
    /// buffer is thrown away because the new device may run at a different
    /// sample rate, and splicing the two would garble the first half-second.
    pub fn switch_device(&self, preferred: Option<String>) {
        applog::log("audio-switch-device-requested");
        let _ = self.device_tx.lock().unwrap().send(preferred);
    }

    fn reset_buffers(&self) {
        self.ring.lock().unwrap().clear();
        *self.recording.lock().unwrap() = None;
    }
```

Add this free function above `fn build_stream`:

```rust
/// Build and start a stream, keeping the healthy flag honest. A dictation
/// attempted while this is None gets the "No mic detected" pill.
fn open(engine: &Arc<AudioEngine>, preferred: &Option<String>) -> Option<cpal::Stream> {
    engine.healthy.store(false, Ordering::SeqCst);
    match build_stream(engine, preferred) {
        Ok(stream) => {
            if stream.play().is_ok() {
                engine.healthy.store(true, Ordering::SeqCst);
                applog::log("audio-stream-started");
                Some(stream)
            } else {
                applog::log("audio-stream-play-failed");
                None
            }
        }
        Err(msg) => {
            applog::log(&format!("audio-stream-error {msg}"));
            None
        }
    }
}
```

- [ ] **Step 2: Add the commands.** In `src-tauri/src/commands.rs`, extend the imports:

```rust
use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::{Emitter, Manager, State};

use crate::{applog, audio, autostart, config, groq, hook, hotkey_logic, keys, state::AppState};
```

Append these commands:

```rust
#[tauri::command]
pub fn list_microphones() -> Vec<String> {
    audio::list_input_devices()
}

#[tauri::command]
pub fn set_microphone(state: State<AppState>, value: Option<String>) {
    let value = value.filter(|v| !v.trim().is_empty());
    state.config.lock().unwrap().input_device = value.clone();
    persist(&state);
    state.audio.switch_device(value);
    applog::log("setting-microphone-changed");
}

/// Start listening for a new combo. Everything typed anywhere on the machine
/// is swallowed until this ends, so it always ends: on the first key release,
/// on Escape, when the window loses focus, or on the watchdog below.
#[tauri::command]
pub fn begin_hotkey_capture(app: tauri::AppHandle, state: State<AppState>) {
    // A dictation must not survive into capture mode.
    state.generation.fetch_add(1, Ordering::SeqCst);
    let _ = state.audio.stop_recording();
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.hide();
    }

    let my_gen = state.capture_gen.load(Ordering::SeqCst);
    state.capturing.store(true, Ordering::SeqCst);
    hook::set_capture(true);
    applog::log("hotkey-capture-begin");

    let capturing = state.capturing.clone();
    let capture_gen = state.capture_gen.clone();
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(hotkey_logic::CAPTURE_TIMEOUT_MS));
        if capture_gen.load(Ordering::SeqCst) == my_gen
            && capturing.swap(false, Ordering::SeqCst)
        {
            hook::set_capture(false);
            capture_gen.fetch_add(1, Ordering::SeqCst);
            applog::log("hotkey-capture-timeout");
            let _ = app.emit(
                "hotkey",
                serde_json::json!({ "phase": "cancelled", "keys": [] }),
            );
        }
    });
}

#[tauri::command]
pub fn cancel_hotkey_capture(app: tauri::AppHandle, state: State<AppState>) {
    if state.capturing.swap(false, Ordering::SeqCst) {
        hook::set_capture(false);
        state.capture_gen.fetch_add(1, Ordering::SeqCst);
        applog::log("hotkey-capture-cancelled-by-window");
        let _ = app.emit(
            "hotkey",
            serde_json::json!({ "phase": "cancelled", "keys": [] }),
        );
    }
}
```

- [ ] **Step 3: Register them.** In `src-tauri/src/lib.rs`, extend `tauri::generate_handler![...]` with:

```rust
            commands::list_microphones,
            commands::set_microphone,
            commands::begin_hotkey_capture,
            commands::cancel_hotkey_capture,
```

- [ ] **Step 4: Verify**

Run: `cargo test` → **52 passed**. `cargo check` → zero warnings.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/audio.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat: live microphone swap and hotkey capture commands"
```

---

### Task 6: Settings window — microphone dropdown and hotkey rebind

**Files:**
- Modify: `src/settings.html`
- Modify: `src/settings.js`

**Interfaces:**
- Consumes: the four new commands and the `hotkey` event.

- [ ] **Step 1: Markup.** In `src/settings.html`:

Replace the `.combo` block and the Change hotkey button with:

```html
    <div class="combo">
      <span>Hold</span>
      <span class="combo-keys" id="combo-keys"></span>
      <span>— speak — release</span>
    </div>
    <button class="btn change" id="change-hotkey">Change hotkey</button>
    <span class="hint" id="hotkey-hint"></span>
```

Replace the microphone row's control and description with:

```html
      <div>
        <div class="label">Microphone</div>
        <div class="desc">Input device used while dictating</div>
      </div>
      <select class="mic" id="mic"></select>
```

Replace the `.mic-static` rule with:

```css
  .combo-keys { display: flex; align-items: center; gap: 11px; }
  .hint { font-size: 12px; color: var(--muted); margin-left: 12px; }
  .hint.ok { color: #3fb970; }
  .hint.err { color: var(--accent); }
  body.capturing .key { border-color: var(--accent); }

  .mic {
    width: 232px; font-size: 13px; padding: 9px 12px; background: var(--surface);
    color: var(--text); border: 1px solid var(--divider);
    font-family: var(--font-body);
  }
```

- [ ] **Step 2: Wiring.** In `src/settings.js`:

Add to the top, under the existing destructuring:

```js
const { listen } = window.__TAURI__.event;
```

Replace the hotkey display lines in `load()` (`const keys = cfg.hotkey.map(...)` through `el("mic-name").textContent = ...`) with:

```js
  currentHotkey = cfg.hotkey;
  renderCombo(currentHotkey, false);
  await loadMics(cfg.input_device);
```

Add above `load()`:

```js
const KEY_LABELS = {
  ctrl: "Ctrl", win: "Win", alt: "Alt", shift: "Shift",
  space: "Space", tab: "Tab", capslock: "Caps Lock",
};
const keyLabel = (n) => KEY_LABELS[n] || n.toUpperCase();

let currentHotkey = ["ctrl", "win"];
let capturing = false;

function renderCombo(names, listening) {
  const box = el("combo-keys");
  box.innerHTML = "";
  if (!names.length) {
    const s = document.createElement("span");
    s.className = "key";
    s.textContent = listening ? "…" : "—";
    box.appendChild(s);
    return;
  }
  names.forEach((n, i) => {
    if (i) {
      const p = document.createElement("span");
      p.className = "plus";
      p.textContent = "+";
      box.appendChild(p);
    }
    const k = document.createElement("span");
    k.className = "key";
    k.textContent = keyLabel(n);
    box.appendChild(k);
  });
}

function setHint(text, kind) {
  const h = el("hotkey-hint");
  h.textContent = text;
  h.className = `hint ${kind || ""}`;
}

function setCapturing(on) {
  capturing = on;
  document.body.classList.toggle("capturing", on);
  el("change-hotkey").textContent = on ? "Listening — press your keys" : "Change hotkey";
}

async function loadMics(selected) {
  const mics = await invoke("list_microphones");
  const sel = el("mic");
  sel.innerHTML = "";
  const def = document.createElement("option");
  def.value = "";
  def.textContent = "System default";
  sel.appendChild(def);
  for (const name of mics) {
    const o = document.createElement("option");
    o.value = name;
    o.textContent = name;
    sel.appendChild(o);
  }
  // A saved mic that is unplugged right now still shows, so the setting
  // does not silently look like it was reset.
  if (selected && !mics.includes(selected)) {
    const o = document.createElement("option");
    o.value = selected;
    o.textContent = `${selected} (not connected)`;
    sel.appendChild(o);
  }
  sel.value = selected || "";
}
```

Add above `load();` at the bottom:

```js
// --- microphone ---
el("mic").onchange = async () => {
  const value = el("mic").value;
  await invoke("set_microphone", { value: value || null });
  el("status-text").textContent = "Microphone updated";
};

// --- hotkey rebind ---
el("change-hotkey").onclick = async () => {
  if (capturing) {
    await invoke("cancel_hotkey_capture");
    return;
  }
  setCapturing(true);
  renderCombo([], true);
  setHint("Hold the keys together, then let go. Esc cancels.", "");
  await invoke("begin_hotkey_capture");
};

listen("hotkey", ({ payload }) => {
  if (payload.phase === "preview") {
    renderCombo(payload.keys, true);
    return;
  }
  setCapturing(false);
  if (payload.phase === "set") {
    currentHotkey = payload.keys;
    renderCombo(currentHotkey, false);
    setHint("Hotkey updated", "ok");
    return;
  }
  renderCombo(currentHotkey, false);
  setHint(
    payload.phase === "invalid"
      ? "Needs a modifier and at most one other key"
      : "Hotkey unchanged",
    payload.phase === "invalid" ? "err" : ""
  );
});

// Clicking away mid-rebind must not leave the keyboard swallowed.
window.addEventListener("blur", () => {
  if (capturing) invoke("cancel_hotkey_capture");
});
```

- [ ] **Step 3: Verify**

Run: `npm run tauri dev`. Open settings from the tray. Confirm the hotkey chips read "Ctrl + Win", the Change hotkey button is enabled, and the microphone dropdown lists real device names with the correct one selected. Full behaviour is Task 7.

- [ ] **Step 4: Commit**

```bash
git add src/settings.html src/settings.js
git commit -m "feat: microphone dropdown and hotkey rebind in the settings window"
```

---

### Task 7: Human verification protocol

**Files:**
- Create: `docs/reports/milestone-3c-results.md`

- [ ] **Step 1: Report the protocol and STOP.** Post the checks below and wait for the human's PASS/FAIL. Do not write the report until they answer.

Set the API key first if it is not already saved, then `npm run tauri dev`.

**Microphone**
1. The dropdown lists the machine's real input devices, with "System default" first and selected.
2. Pick a different device, then dictate. Text still arrives. (Proves the stream rebuilt rather than dying.)
3. Pick a device that hears nothing — a muted or unplugged input, or a virtual one. Dictate normally. The pill fades and nothing is pasted. (Proves the *new* device is the one actually being recorded, not the old one.)
4. Set it back to a working mic, quit from the tray, relaunch. The dropdown still shows that mic and dictation works.

**Hotkey rebind**
5. Click "Change hotkey", hold Ctrl and Space together, release. The chips change to "Ctrl + Space" and the hint says "Hotkey updated".
6. Hold Ctrl+Space over a text field and speak. It dictates, **and no spaces are typed into the field** while holding.
7. Press Ctrl+Win. Nothing happens — the old combo is dead.
8. Type a normal space, and normal text, in any app. Everything works as usual.
9. Click "Change hotkey", press Escape. The chips stay on Ctrl + Space, hint says "Hotkey unchanged".
10. **Safety check — do this one carefully.** Click "Change hotkey", then do nothing at all for about 8 seconds. The button returns to "Change hotkey" on its own. Now type in any app: **the keyboard is completely normal.** If it is not, quit the app from the tray and report immediately.
11. Click "Change hotkey", then click somewhere else on the desktop without pressing any keys. The button resets and the keyboard is normal.
12. Click "Change hotkey" and hold A and B together. The hint says it needs a modifier; the hotkey is unchanged.
13. Quit from the tray, relaunch. The hotkey is still Ctrl + Space and it works.
14. Rebind back to Ctrl + Win and confirm it dictates.

If any check fails, include the exact hint text shown, the DevTools console error (right-click the settings window → Inspect), and the tail of `C:\Users\<you>\AppData\Roaming\WhisperOSS\log.txt`.

- [ ] **Step 2: Write `docs/reports/milestone-3c-results.md`** in the same shape as the earlier milestone reports: one row per check with PASS/FAIL and what was observed, the test count, any DEVIATIONS, and a GO / NO-GO verdict for Milestone 4.

- [ ] **Step 3: Commit**

```bash
git add docs/reports/milestone-3c-results.md
git commit -m "docs: milestone 3c results"
```
