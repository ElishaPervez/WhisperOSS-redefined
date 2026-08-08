# Milestone 1 — Headless Dictation Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A working dictation app: hold Ctrl+Win anywhere in Windows, speak, release — the transcribed text is pasted into the focused app, with the Milestone-0 pill showing live bars while recording. Tray icon with Quit. No settings UI yet.

**Architecture:** A low-level keyboard hook feeds a pure hold-tracking state machine over a channel. On finish, the always-on audio engine (with 0.5 s pre-roll ring) hands back samples; pure DSP functions downsample to 16 kHz and encode WAV in memory; a blocking Groq client (15 s timeout, one retry) transcribes; a dedicated clipboard-owner thread stages the text with Windows' history-exclusion formats using **delayed rendering** — the OS tells us the moment the target app actually pastes, which is what makes clipboard restore sequenced instead of a timer guess.

**Tech Stack:** Existing Tauri 2 app from Milestone 0. New crates: `windows` (hook, clipboard, synthetic Ctrl+V), `cpal` (mic), `reqwest` blocking+multipart (Groq), `serde_json`, `keyring` (Credential Manager).

**Spec:** `docs/superpowers/specs/2026-08-08-whispeross-v2-design.md` §4–§6, milestone 1 in §8.

## Global Constraints

- Windows-only. Repo root: `C:\projects (code)\15. WhisperOSS redefined` (quote all paths — they contain spaces).
- Shell: `cd`, `git`, `npm`, `cargo` commands work in PowerShell and Git Bash as written.
- Never touch `src-reference\` (v1 Python, reference only, git-ignored).
- All `cargo` commands run from `src-tauri\` (`cd "C:\projects (code)\15. WhisperOSS redefined\src-tauri"`).
- Pinned behavior values (from spec): pre-roll **0.5 s** · min hold **150 ms** · request timeout **15 s**, **1 retry** · upload rate **16 kHz mono 16-bit WAV** · transcription model **`whisper-large-v3-turbo`**, language **`en`**, temperature **0** · hotkey fixed **Ctrl+Win** (configurability is Milestone 3).
- If a pinned crate version doesn't resolve, use the newest stable release on crates.io and log a deviation. Do not switch to a different crate.
- Privacy rule (hard): the transcript goes on the clipboard ONLY with the three history-exclusion formats set. If staging them fails, abort the paste. Never fall back to a plain clipboard write, never use a crate that does one (that's why no `arboard`).
- Not in this milestone (do not build): AI formatting/casual requests, settings UI, hotkey rebinding, mic picker, overlay processing/success/error visuals (Milestone 2 — errors are log-only for now), first-run flow.
- Known spec narrowing, accepted for M1 and revisited in M5 hardening: clipboard snapshot/restore covers plain text only (v1 snapshotted all formats; images/rich text on the user's clipboard are not restored — logged, not silently ignored).

---

### Task 1: Dependencies and the event log

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Create: `src-tauri/src/applog.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod applog;`)

**Interfaces:**
- Consumes: nothing.
- Produces: `applog::log(event: &str)` — appends a timestamped line to `%APPDATA%\WhisperOSS\log.txt`, size-capped; `applog::format_line(unix_ms: u64, event: &str) -> String`; `applog::over_cap(len_bytes: u64) -> bool`. Every later task calls `applog::log`.

- [ ] **Step 1: Add dependencies to `src-tauri/Cargo.toml`**

Change the `tauri` line to enable the tray feature and add the new crates under `[dependencies]`:

```toml
tauri = { version = "2", features = ["tray-icon"] }
cpal = "0.16"
reqwest = { version = "0.12", features = ["blocking", "multipart", "json"] }
serde_json = "1"
keyring = { version = "3", features = ["windows-native"] }
windows = { version = "0.61", features = [
  "Win32_Foundation",
  "Win32_System_DataExchange",
  "Win32_System_Memory",
  "Win32_System_LibraryLoader",
  "Win32_UI_Input_KeyboardAndMouse",
  "Win32_UI_WindowsAndMessaging",
] }
```

Run `cargo check` — expect it to download and compile cleanly (warnings about unused deps are fine at this point).

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/src/applog.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_has_timestamp_event_and_newline() {
        assert_eq!(format_line(1723118400123, "recording-start"),
                   "1723118400123 recording-start\n");
    }

    #[test]
    fn cap_is_one_megabyte() {
        assert!(!over_cap(1_000_000));
        assert!(over_cap(1_000_001));
    }
}
```

Add `mod applog;` at the top of `src-tauri/src/lib.rs`.

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test applog`
Expected: FAIL to compile — `format_line`, `over_cap` not found.

- [ ] **Step 4: Implement**

Add above the test module in `src-tauri/src/applog.rs`:

```rust
//! Append-only diagnostic log. Event names only — NEVER transcript text,
//! audio, or key contents (spec §6).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const CAP_BYTES: u64 = 1_000_000;

pub fn format_line(unix_ms: u64, event: &str) -> String {
    format!("{unix_ms} {event}\n")
}

pub fn over_cap(len_bytes: u64) -> bool {
    len_bytes > CAP_BYTES
}

fn log_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    let dir = PathBuf::from(base).join("WhisperOSS");
    fs::create_dir_all(&dir).ok()?;
    Some(dir.join("log.txt"))
}

/// Best-effort: logging must never crash or block the pipeline.
pub fn log(event: &str) {
    let Some(path) = log_path() else { return };
    if fs::metadata(&path).map(|m| over_cap(m.len())).unwrap_or(false) {
        let _ = fs::write(&path, b"");
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(format_line(now, event).as_bytes());
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test applog`
Expected: `2 passed`.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/applog.rs src-tauri/src/lib.rs
git commit -m "feat: M1 dependencies and capped diagnostic log"
```

---

### Task 2: API key resolution (Credential Manager + env bootstrap)

**Files:**
- Create: `src-tauri/src/keys.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod keys;`)

**Interfaces:**
- Consumes: `applog::log`.
- Produces: `keys::resolve(store_val: Option<String>, env_val: Option<String>) -> (Option<String>, bool)` (key, should-save-to-store); `keys::load() -> Option<String>` — Credential Manager first, else `WHISPEROSS_GROQ_KEY` env var (which then gets saved into the Credential Manager). Task 9 calls `keys::load()`.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/keys.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_wins_over_env() {
        assert_eq!(resolve(Some("sk_store".into()), Some("sk_env".into())),
                   (Some("sk_store".into()), false));
    }

    #[test]
    fn env_used_and_flagged_for_saving_when_store_empty() {
        assert_eq!(resolve(None, Some("  sk_env  ".into())),
                   (Some("sk_env".into()), true));
    }

    #[test]
    fn nothing_available() {
        assert_eq!(resolve(None, None), (None, false));
        assert_eq!(resolve(None, Some("   ".into())), (None, false));
    }
}
```

Add `mod keys;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test keys`
Expected: FAIL to compile — `resolve` not found.

- [ ] **Step 3: Implement**

Add above the test module in `src-tauri/src/keys.rs`:

```rust
//! Groq API key lookup. The key lives in Windows Credential Manager
//! (service "WhisperOSS", account "groq_api_key") and NEVER in a file.
//! Until the settings UI exists (M3), the WHISPEROSS_GROQ_KEY environment
//! variable bootstraps it: found once, it is saved into the vault.

use crate::applog;

const SERVICE: &str = "WhisperOSS";
const ACCOUNT: &str = "groq_api_key";
pub const ENV_VAR: &str = "WHISPEROSS_GROQ_KEY";

/// Pure decision: which key to use, and whether it should be persisted.
pub fn resolve(store_val: Option<String>, env_val: Option<String>) -> (Option<String>, bool) {
    if let Some(k) = store_val {
        if !k.trim().is_empty() {
            return (Some(k), false);
        }
    }
    match env_val {
        Some(k) if !k.trim().is_empty() => (Some(k.trim().to_string()), true),
        _ => (None, false),
    }
}

pub fn load() -> Option<String> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT).ok();
    let store_val = entry.as_ref().and_then(|e| e.get_password().ok());
    let env_val = std::env::var(ENV_VAR).ok();
    let (key, save) = resolve(store_val, env_val);
    match (&key, save, &entry) {
        (Some(k), true, Some(e)) => {
            let _ = e.set_password(k);
            applog::log("api-key-bootstrapped-from-env");
        }
        (Some(_), false, _) => applog::log("api-key-from-credential-manager"),
        (None, _, _) => applog::log("api-key-missing"),
        _ => {}
    }
    key
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test keys`
Expected: `3 passed`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/keys.rs src-tauri/src/lib.rs
git commit -m "feat: API key resolution via Credential Manager with env bootstrap"
```

---

### Task 3: DSP — downsample, WAV encode, silence check, level normalize (pure, TDD)

**Files:**
- Create: `src-tauri/src/dsp.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod dsp;`)

**Interfaces:**
- Consumes: nothing.
- Produces: `dsp::resample_to_16k(input: &[i16], src_rate: u32) -> Vec<i16>`; `dsp::encode_wav_mono16(samples: &[i16], sample_rate: u32) -> Vec<u8>`; `dsp::is_effectively_silent(samples: &[i16]) -> bool`; `dsp::normalize_level(peak: i16) -> f64` (0.0–1.0 for the visualizer). Tasks 6 and 9 consume these.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/dsp.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_16k_is_passthrough() {
        let s = vec![1i16, 2, 3, 4];
        assert_eq!(resample_to_16k(&s, 16_000), s);
    }

    #[test]
    fn resample_48k_thirds_the_length_and_keeps_amplitude() {
        let s = vec![900i16; 4800]; // 100 ms of constant signal at 48 kHz
        let out = resample_to_16k(&s, 48_000);
        assert_eq!(out.len(), 1600);
        assert!(out.iter().all(|&v| v == 900));
    }

    #[test]
    fn resample_empty_is_empty() {
        assert!(resample_to_16k(&[], 48_000).is_empty());
    }

    #[test]
    fn wav_header_is_valid_for_16k_mono() {
        let wav = encode_wav_mono16(&[0i16, 1000, -1000], 16_000);
        assert_eq!(wav.len(), 44 + 6);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1); // mono
        assert_eq!(u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]), 16_000);
        assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 16); // bits/sample
        assert_eq!(u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]), 6); // data bytes
        assert_eq!(i16::from_le_bytes([wav[46], wav[47]]), 1000);
    }

    #[test]
    fn silence_detection() {
        assert!(is_effectively_silent(&[]));
        assert!(is_effectively_silent(&vec![50i16; 16_000]));
        let mut speech = vec![50i16; 16_000];
        speech[8000] = 5000;
        assert!(!is_effectively_silent(&speech));
    }

    #[test]
    fn level_normalization_bounds() {
        assert_eq!(normalize_level(0), 0.0);
        assert!(normalize_level(700) > 0.1);
        assert!((normalize_level(i16::MAX) - 1.0).abs() < 1e-9);
    }
}
```

Add `mod dsp;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test dsp`
Expected: FAIL to compile — functions not found.

- [ ] **Step 3: Implement**

Add above the test module in `src-tauri/src/dsp.rs`:

```rust
//! Pure audio math. Upload is always 16 kHz mono 16-bit WAV (spec §4):
//! devices capture at their native rate and we downsample locally so the
//! upload is ~3x smaller than v1's.

/// Linear-interpolation downsample to 16 kHz. Adequate for speech-to-text;
/// no external DSP crate needed.
pub fn resample_to_16k(input: &[i16], src_rate: u32) -> Vec<i16> {
    const DST: f64 = 16_000.0;
    if src_rate == 16_000 || input.is_empty() {
        return input.to_vec();
    }
    let ratio = src_rate as f64 / DST;
    let out_len = (input.len() as f64 / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 * ratio;
        let i0 = pos.floor() as usize;
        let frac = pos - i0 as f64;
        let s0 = input[i0] as f64;
        let s1 = input[(i0 + 1).min(input.len() - 1)] as f64;
        out.push((s0 + (s1 - s0) * frac).round() as i16);
    }
    out
}

/// Standard 44-byte RIFF/WAVE header + PCM data, built in memory.
/// No temp files anywhere in the pipeline (spec §4).
pub fn encode_wav_mono16(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVE");
    b.extend_from_slice(b"fmt ");
    b.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&sample_rate.to_le_bytes());
    b.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    b.extend_from_slice(&2u16.to_le_bytes()); // block align
    b.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&s.to_le_bytes());
    }
    b
}

/// True when the whole recording never rises above ~1% of full scale —
/// the user held the keys but said nothing. Dropped without upload (spec §5).
pub fn is_effectively_silent(samples: &[i16]) -> bool {
    const FLOOR: u16 = 330;
    !samples.iter().any(|s| s.unsigned_abs() > FLOOR)
}

/// v1's tuned visualizer curve: peak → 0.0..=1.0 with a soft knee.
pub fn normalize_level(peak: i16) -> f64 {
    (((peak as f64) / 7000.0).min(1.0).powf(0.72) * 1.18).min(1.0)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test dsp`
Expected: `6 passed`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/dsp.rs src-tauri/src/lib.rs
git commit -m "feat: DSP module - 16k downsample, in-memory WAV, silence check (tested)"
```

---

### Task 4: Hold-to-dictate state machine (pure, TDD)

**Files:**
- Create: `src-tauri/src/hotkey_logic.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod hotkey_logic;`)

**Interfaces:**
- Consumes: nothing.
- Produces: `hotkey_logic::KeyEvent { CtrlDown(u64), CtrlUp(u64), WinDown(u64), WinUp(u64) }` (timestamps in ms); `hotkey_logic::Action { None, Start, Finish { held_ms: u64 }, Cancel }`; `hotkey_logic::HoldTracker` with `new()` and `on_event(&mut self, ev: KeyEvent) -> Action`. Task 5 feeds it from the OS hook; Task 9 acts on the Actions.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/hotkey_logic.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_both_then_release_finishes() {
        let mut t = HoldTracker::new();
        assert_eq!(t.on_event(KeyEvent::CtrlDown(0)), Action::None);
        assert_eq!(t.on_event(KeyEvent::WinDown(10)), Action::Start);
        assert_eq!(t.on_event(KeyEvent::CtrlUp(400)),
                   Action::Finish { held_ms: 390 });
    }

    #[test]
    fn reverse_press_order_also_works() {
        let mut t = HoldTracker::new();
        assert_eq!(t.on_event(KeyEvent::WinDown(0)), Action::None);
        assert_eq!(t.on_event(KeyEvent::CtrlDown(5)), Action::Start);
        assert_eq!(t.on_event(KeyEvent::WinUp(300)),
                   Action::Finish { held_ms: 295 });
    }

    #[test]
    fn tap_shorter_than_150ms_cancels() {
        let mut t = HoldTracker::new();
        t.on_event(KeyEvent::CtrlDown(0));
        assert_eq!(t.on_event(KeyEvent::WinDown(20)), Action::Start);
        assert_eq!(t.on_event(KeyEvent::WinUp(100)), Action::Cancel);
    }

    #[test]
    fn key_repeat_does_not_double_start() {
        let mut t = HoldTracker::new();
        t.on_event(KeyEvent::CtrlDown(0));
        assert_eq!(t.on_event(KeyEvent::WinDown(10)), Action::Start);
        // Windows auto-repeats key-down while held:
        assert_eq!(t.on_event(KeyEvent::CtrlDown(50)), Action::None);
        assert_eq!(t.on_event(KeyEvent::WinDown(60)), Action::None);
        assert_eq!(t.on_event(KeyEvent::CtrlUp(500)),
                   Action::Finish { held_ms: 490 });
    }

    #[test]
    fn single_key_never_starts() {
        let mut t = HoldTracker::new();
        assert_eq!(t.on_event(KeyEvent::WinDown(0)), Action::None);
        assert_eq!(t.on_event(KeyEvent::WinUp(500)), Action::None);
    }

    #[test]
    fn second_release_after_finish_is_ignored() {
        let mut t = HoldTracker::new();
        t.on_event(KeyEvent::CtrlDown(0));
        t.on_event(KeyEvent::WinDown(10));
        assert!(matches!(t.on_event(KeyEvent::CtrlUp(400)), Action::Finish { .. }));
        assert_eq!(t.on_event(KeyEvent::WinUp(450)), Action::None);
    }
}
```

Add `mod hotkey_logic;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test hotkey_logic`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

Add above the test module in `src-tauri/src/hotkey_logic.rs`:

```rust
//! Pure hold-to-dictate logic, driven by timestamped key events from the
//! OS hook (Task 5). Recording starts the instant both keys are down — the
//! 0.5 s pre-roll covers anything earlier. A release before MIN_HOLD_MS
//! counts as an accidental tap and cancels (spec §5).

pub const MIN_HOLD_MS: u64 = 150;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyEvent {
    CtrlDown(u64),
    CtrlUp(u64),
    WinDown(u64),
    WinUp(u64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    None,
    Start,
    Finish { held_ms: u64 },
    Cancel,
}

pub struct HoldTracker {
    ctrl: bool,
    win: bool,
    started_at: Option<u64>,
}

impl HoldTracker {
    pub fn new() -> Self {
        Self { ctrl: false, win: false, started_at: None }
    }

    pub fn on_event(&mut self, ev: KeyEvent) -> Action {
        match ev {
            KeyEvent::CtrlDown(t) => {
                self.ctrl = true;
                self.maybe_start(t)
            }
            KeyEvent::WinDown(t) => {
                self.win = true;
                self.maybe_start(t)
            }
            KeyEvent::CtrlUp(t) => {
                self.ctrl = false;
                self.finish_if_active(t)
            }
            KeyEvent::WinUp(t) => {
                self.win = false;
                self.finish_if_active(t)
            }
        }
    }

    fn maybe_start(&mut self, t: u64) -> Action {
        if self.ctrl && self.win && self.started_at.is_none() {
            self.started_at = Some(t);
            Action::Start
        } else {
            Action::None
        }
    }

    fn finish_if_active(&mut self, t: u64) -> Action {
        match self.started_at.take() {
            None => Action::None,
            Some(s) => {
                let held_ms = t.saturating_sub(s);
                if held_ms >= MIN_HOLD_MS {
                    Action::Finish { held_ms }
                } else {
                    Action::Cancel
                }
            }
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test hotkey_logic`
Expected: `6 passed`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/hotkey_logic.rs src-tauri/src/lib.rs
git commit -m "feat: hold-to-dictate state machine with 150ms tap rejection (tested)"
```

---

### Task 5: Low-level keyboard hook

**Files:**
- Create: `src-tauri/src/hook.rs`
- Modify: `src-tauri/src/lib.rs` (spawn the hook, log actions for verification)

**Interfaces:**
- Consumes: `hotkey_logic::KeyEvent`.
- Produces: `hook::spawn(tx: std::sync::mpsc::Sender<hotkey_logic::KeyEvent>)` — installs a global `WH_KEYBOARD_LL` hook on its own thread and forwards Ctrl/Win transitions with the OS timestamp. Task 9 owns the receiving end.

- [ ] **Step 1: Implement the hook**

Create `src-tauri/src/hook.rs`:

```rust
//! Global low-level keyboard hook (spec §4): real key-down/key-up events,
//! no polling, no third-party hotkey library. The hook runs on a dedicated
//! thread with its own message loop, does minimal work, and never blocks —
//! Windows silently removes hooks that are slow.

use std::sync::mpsc::Sender;
use std::sync::OnceLock;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW,
    TranslateMessage, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::hotkey_logic::KeyEvent;

static SENDER: OnceLock<Sender<KeyEvent>> = OnceLock::new();

// Virtual-key codes: generic/left/right Ctrl, left/right Win.
const VK_CONTROL: u32 = 0x11;
const VK_LCONTROL: u32 = 0xA2;
const VK_RCONTROL: u32 = 0xA3;
const VK_LWIN: u32 = 0x5B;
const VK_RWIN: u32 = 0x5C;

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let t = kb.time as u64;
        let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        let up = matches!(wparam.0 as u32, WM_KEYUP | WM_SYSKEYUP);
        let ev = match kb.vkCode {
            VK_CONTROL | VK_LCONTROL | VK_RCONTROL if down => Some(KeyEvent::CtrlDown(t)),
            VK_CONTROL | VK_LCONTROL | VK_RCONTROL if up => Some(KeyEvent::CtrlUp(t)),
            VK_LWIN | VK_RWIN if down => Some(KeyEvent::WinDown(t)),
            VK_LWIN | VK_RWIN if up => Some(KeyEvent::WinUp(t)),
            _ => None,
        };
        if let (Some(ev), Some(tx)) = (ev, SENDER.get()) {
            let _ = tx.send(ev);
        }
    }
    // Never swallow keys — Ctrl and Win must keep working normally.
    CallNextHookEx(None, code, wparam, lparam)
}

pub fn spawn(tx: Sender<KeyEvent>) {
    SENDER.set(tx).expect("hook::spawn called twice");
    std::thread::spawn(|| unsafe {
        let _hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0)
            .expect("failed to install keyboard hook");
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}
```

If the `windows` crate's function signatures differ in the resolved version (e.g. `Option` wrapping on handles), adapt the call sites only — the logic and constants stay exactly as written. Log a deviation.

- [ ] **Step 2: Wire a temporary verification into `src-tauri/src/lib.rs`**

Add `mod hook;` and, inside the `setup` closure (after the existing `demo::spawn_demo` line), add:

```rust
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
```

- [ ] **Step 3: Verify manually**

Run: `npm run tauri dev` (from the repo root). Then:
1. Hold Ctrl+Win for ~1 s, release. 2. Tap Ctrl+Win as fast as you can. 3. Press and release Win alone (Start menu should open normally — we must not break it).

Open `%APPDATA%\WhisperOSS\log.txt`. Expected lines, in order: `hook-test-start` then `hook-test-finish held_ms=<≈1000>`; then `hook-test-start` + `hook-test-cancel`; and nothing for the Win-alone press. All 22 existing tests still pass (`cargo test`).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/hook.rs src-tauri/src/lib.rs
git commit -m "feat: global low-level keyboard hook feeding the hold tracker"
```

---

### Task 6: Always-on audio engine with pre-roll

**Files:**
- Create: `src-tauri/src/audio.rs`
- Modify: `src-tauri/src/lib.rs` (start engine; keep demo for now)
- Modify: `src/index.html`, `src/main.js` (remove the click-through dot; keep bars + fps)

**Interfaces:**
- Consumes: `dsp::normalize_level`, `applog::log`.
- Produces: `audio::AudioEngine` with `start(app: tauri::AppHandle) -> std::sync::Arc<AudioEngine>`, `start_recording(&self)`, `stop_recording(&self) -> (Vec<i16>, u32)` (samples + capture rate), `is_healthy(&self) -> bool`. Emits `"level"` events (f64) at ~30 Hz whenever recording. Task 9 drives it.

- [ ] **Step 1: Implement the engine**

Create `src-tauri/src/audio.rs`:

```rust
//! Always-on microphone capture (spec §4). The stream never stops: it feeds
//! a 0.5 s pre-roll ring so recording start is instant and the first word
//! is never clipped. Mono i16 at the device's native rate; downsampling to
//! 16 kHz happens at upload time (dsp.rs). Default input device only in M1
//! (device picker is M3).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tauri::Emitter;

use crate::{applog, dsp};

const PRE_ROLL_SECS: f64 = 0.5;

pub struct AudioEngine {
    ring: Mutex<VecDeque<i16>>,
    recording: Mutex<Option<Vec<i16>>>,
    rate: AtomicU32,
    peak: AtomicU16,
    healthy: AtomicBool,
}

impl AudioEngine {
    pub fn start(app: tauri::AppHandle) -> Arc<AudioEngine> {
        let engine = Arc::new(AudioEngine {
            ring: Mutex::new(VecDeque::new()),
            recording: Mutex::new(None),
            rate: AtomicU32::new(16_000),
            peak: AtomicU16::new(0),
            healthy: AtomicBool::new(false),
        });

        // The cpal stream is not Send: build it on a dedicated thread and
        // park that thread forever to keep the stream alive.
        let e = engine.clone();
        std::thread::spawn(move || {
            match build_stream(&e) {
                Ok(stream) => {
                    if stream.play().is_ok() {
                        e.healthy.store(true, Ordering::SeqCst);
                        applog::log("audio-stream-started");
                    } else {
                        applog::log("audio-stream-play-failed");
                    }
                    loop {
                        std::thread::park();
                    }
                }
                Err(msg) => applog::log(&format!("audio-stream-error {msg}")),
            }
        });

        // Level emitter for the visualizer: ~30 Hz while recording.
        let e = engine.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(33));
            if e.recording.lock().unwrap().is_some() {
                let p = e.peak.swap(0, Ordering::SeqCst);
                let _ = app.emit("level", dsp::normalize_level(p as i16));
            }
        });

        engine
    }

    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::SeqCst)
    }

    /// Instant start: seed the take with the pre-roll ring contents.
    pub fn start_recording(&self) {
        let seed: Vec<i16> = self.ring.lock().unwrap().iter().copied().collect();
        *self.recording.lock().unwrap() = Some(seed);
    }

    pub fn stop_recording(&self) -> (Vec<i16>, u32) {
        let samples = self.recording.lock().unwrap().take().unwrap_or_default();
        (samples, self.rate.load(Ordering::SeqCst))
    }

    fn ingest(&self, mono: &[i16], ring_cap: usize) {
        {
            let mut ring = self.ring.lock().unwrap();
            for &s in mono {
                if ring.len() == ring_cap {
                    ring.pop_front();
                }
                ring.push_back(s);
            }
        }
        if let Some(buf) = self.recording.lock().unwrap().as_mut() {
            buf.extend_from_slice(mono);
        }
        let peak = mono.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
        self.peak.fetch_max(peak, Ordering::SeqCst);
    }
}

fn build_stream(engine: &Arc<AudioEngine>) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or("no input device")?;
    let config = device.default_input_config().map_err(|e| e.to_string())?;
    let rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    engine.rate.store(rate, Ordering::SeqCst);
    let ring_cap = (rate as f64 * PRE_ROLL_SECS) as usize;

    let e = engine.clone();
    let err_fn = |err| applog::log(&format!("audio-callback-error {err}"));

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device
            .build_input_stream(
                &config.into(),
                move |data: &[f32], _| {
                    let mono: Vec<i16> = data
                        .chunks(channels)
                        .map(|frame| {
                            let avg = frame.iter().sum::<f32>() / frame.len() as f32;
                            (avg.clamp(-1.0, 1.0) * 32_767.0) as i16
                        })
                        .collect();
                    e.ingest(&mono, ring_cap);
                },
                err_fn,
                None,
            )
            .map_err(|e| e.to_string())?,
        cpal::SampleFormat::I16 => device
            .build_input_stream(
                &config.into(),
                move |data: &[i16], _| {
                    let mono: Vec<i16> = data
                        .chunks(channels)
                        .map(|frame| {
                            (frame.iter().map(|&s| s as i32).sum::<i32>()
                                / frame.len() as i32) as i16
                        })
                        .collect();
                    e.ingest(&mono, ring_cap);
                },
                err_fn,
                None,
            )
            .map_err(|e| e.to_string())?,
        other => return Err(format!("unsupported sample format {other:?}")),
    };
    Ok(stream)
}
```

- [ ] **Step 2: Start the engine and switch the pill to real audio**

In `src-tauri/src/lib.rs`: add `mod audio;`, and in `setup` REPLACE the `demo::spawn_demo(...)` line with:

```rust
            let audio_engine = audio::AudioEngine::start(app.handle().clone());
            // TEMPORARY (removed in Task 9): record continuously so the pill
            // shows live mic levels for this task's verification.
            audio_engine.start_recording();
```

Delete `src-tauri/src/demo.rs` and remove `mod demo;` from `lib.rs`.

In `src/index.html`: delete the `<div class="dot" id="dot"></div>` line and the `.dot` CSS block.
In `src/main.js`: delete the `dot` constant, the `listen("clickthrough", ...)` block, and the `document.addEventListener("click", ...)` line.

- [ ] **Step 3: Verify manually**

Run: `npm run tauri dev`. Expected: the pill's bars now respond to YOUR VOICE — flat-ish when silent, dancing when you speak, louder speech = taller bars, fps still ~60. `%APPDATA%\WhisperOSS\log.txt` gains `audio-stream-started`. `cargo test` still passes (21 tests — the demo test is gone with `demo.rs`).

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: always-on mic capture with 0.5s pre-roll driving the pill"
```

---

### Task 7: Groq transcription client (TDD against a local mock server)

**Files:**
- Create: `src-tauri/src/groq.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod groq;`)

**Interfaces:**
- Consumes: nothing.
- Produces: `groq::GroqClient::new(key: String, base_url: String, timeout: Duration) -> GroqClient`; `transcribe(&self, wav: Vec<u8>) -> Result<String, GroqError>`; `groq::GroqError { Unauthorized, Network(String), Server(String) }`; `groq::PROD_BASE_URL` (`"https://api.groq.com"`). Behavior: one retry on network/server errors, none on 401. Task 9 consumes.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/groq.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    /// One-shot HTTP server: accepts a single connection, reads the request,
    /// writes `response` verbatim. Close-delimited bodies (no content-length).
    fn serve_once(response: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 65536];
            let _ = sock.read(&mut buf);
            let _ = sock.write_all(response.as_bytes());
        });
        format!("http://{addr}")
    }

    fn client(base: String) -> GroqClient {
        GroqClient::new("test-key".into(), base, Duration::from_secs(2))
    }

    #[test]
    fn parses_transcription_text() {
        let base = serve_once(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n{\"text\": \" hello world \"}",
        );
        assert_eq!(client(base).transcribe(vec![0u8; 16]).unwrap(), "hello world");
    }

    #[test]
    fn unauthorized_maps_and_does_not_retry() {
        // serve_once accepts exactly one connection; a retry would fail with
        // a network error instead of Unauthorized — so this also proves no retry.
        let base = serve_once("HTTP/1.1 401 Unauthorized\r\nconnection: close\r\n\r\n{}");
        assert!(matches!(client(base).transcribe(vec![0u8; 16]),
                         Err(GroqError::Unauthorized)));
    }

    #[test]
    fn retries_once_after_dropped_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            drop(sock); // first attempt: connection dies
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 65536];
            let _ = sock.read(&mut buf);
            let _ = sock.write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n{\"text\": \"second try\"}",
            );
        });
        let c = client(format!("http://{addr}"));
        assert_eq!(c.transcribe(vec![0u8; 16]).unwrap(), "second try");
    }

    #[test]
    fn server_error_after_retries_maps_to_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            for _ in 0..2 {
                let (mut sock, _) = listener.accept().unwrap();
                let mut buf = [0u8; 65536];
                let _ = sock.read(&mut buf);
                let _ = sock.write_all(b"HTTP/1.1 500 Oops\r\nconnection: close\r\n\r\n");
            }
        });
        let c = client(format!("http://{addr}"));
        assert!(matches!(c.transcribe(vec![0u8; 16]), Err(GroqError::Server(_))));
    }
}
```

Add `mod groq;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test groq`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

Add above the test module in `src-tauri/src/groq.rs`:

```rust
//! Groq speech-to-text over plain REST (no SDK — spec §4). Hard rules:
//! 15 s timeout, exactly one retry on network/server failures, no retry on
//! a rejected key, cancellation handled by the caller via generation check.

use std::time::Duration;

pub const PROD_BASE_URL: &str = "https://api.groq.com";
const MODEL: &str = "whisper-large-v3-turbo";

#[derive(Debug)]
pub enum GroqError {
    Unauthorized,
    Network(String),
    Server(String),
}

pub struct GroqClient {
    http: reqwest::blocking::Client,
    base: String,
    key: String,
}

impl GroqClient {
    pub fn new(key: String, base_url: String, timeout: Duration) -> GroqClient {
        let http = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .expect("http client");
        GroqClient { http, base: base_url, key }
    }

    pub fn transcribe(&self, wav: Vec<u8>) -> Result<String, GroqError> {
        let mut last = None;
        for _ in 0..2 {
            match self.attempt(wav.clone()) {
                Ok(text) => return Ok(text),
                Err(GroqError::Unauthorized) => return Err(GroqError::Unauthorized),
                Err(e) => last = Some(e),
            }
        }
        Err(last.expect("at least one attempt ran"))
    }

    fn attempt(&self, wav: Vec<u8>) -> Result<String, GroqError> {
        let part = reqwest::blocking::multipart::Part::bytes(wav)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| GroqError::Network(e.to_string()))?;
        let form = reqwest::blocking::multipart::Form::new()
            .part("file", part)
            .text("model", MODEL)
            .text("language", "en")
            .text("temperature", "0")
            .text("response_format", "json");

        let resp = self
            .http
            .post(format!("{}/openai/v1/audio/transcriptions", self.base))
            .bearer_auth(&self.key)
            .multipart(form)
            .send()
            .map_err(|e| GroqError::Network(e.to_string()))?;

        match resp.status().as_u16() {
            200 => {
                let v: serde_json::Value =
                    resp.json().map_err(|e| GroqError::Network(e.to_string()))?;
                Ok(v["text"].as_str().unwrap_or_default().trim().to_string())
            }
            401 | 403 => Err(GroqError::Unauthorized),
            s => Err(GroqError::Server(format!("HTTP {s}"))),
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test groq`
Expected: `4 passed` (total suite now 25).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/groq.rs src-tauri/src/lib.rs
git commit -m "feat: Groq transcription client with timeout and single retry (tested)"
```

---

### Task 8: Privacy clipboard and paste

**Files:**
- Create: `src-tauri/src/clipboard.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod clipboard;` and call `clipboard::init()` in setup)

**Interfaces:**
- Consumes: `applog::log`.
- Produces: `clipboard::init()` (spawns the owner-window thread; call once);
  `clipboard::snapshot_text() -> Option<String>`;
  `clipboard::stage(text: &str, restore_to: Option<String>) -> bool` (false = privacy staging failed → caller must abort);
  `clipboard::send_ctrl_v()`;
  `clipboard::wait_pasted(timeout: Duration) -> bool` (true = target app actually pulled our text);
  `clipboard::restore()` (restores snapshot ONLY if we still own the clipboard);
  `clipboard::to_utf16z(s: &str) -> Vec<u16>` (pure, tested). Task 9 sequences these.

- [ ] **Step 1: Write the failing test**

Create `src-tauri/src/clipboard.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16z_is_null_terminated_utf16() {
        let v = to_utf16z("Hi ✓");
        assert_eq!(v.last(), Some(&0u16));
        assert_eq!(String::from_utf16_lossy(&v[..v.len() - 1]), "Hi ✓");
    }
}
```

Add `mod clipboard;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test clipboard`
Expected: FAIL to compile — `to_utf16z` not found.

- [ ] **Step 3: Implement**

Add above the test module in `src-tauri/src/clipboard.rs`:

```rust
//! Privacy paste (spec §4). The transcript is staged with DELAYED RENDERING:
//! we put a promise (not the text) on the clipboard, plus three formats that
//! keep it out of Win+V history and the cloud clipboard. When the target app
//! pastes, Windows asks us to render (WM_RENDERFORMAT) — that message IS the
//! paste confirmation, which makes restore sequenced instead of a timer.
//! Restore only happens if we still own the clipboard, so a user copy in
//! between is never clobbered (fixes v1's bug).
//!
//! M1 limitation (documented in the plan header): snapshot/restore is plain
//! text only. A non-text clipboard (image, files) is logged and not restored.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardOwner,
    OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
    PostMessageW, RegisterClassW, HWND_MESSAGE, MSG, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WM_RENDERFORMAT, WNDCLASSW,
};

use crate::applog;

const CF_UNICODETEXT: u32 = 13;
const WM_STAGE: u32 = WM_APP + 1;
const WM_RESTORE: u32 = WM_APP + 2;

static PENDING: Mutex<Option<Vec<u16>>> = Mutex::new(None);
static RESTORE_TO: Mutex<Option<Vec<u16>>> = Mutex::new(None);
static STAGE_OK: AtomicBool = AtomicBool::new(false);
static STAGE_DONE: AtomicBool = AtomicBool::new(false);
static RENDERED: AtomicBool = AtomicBool::new(false);
static OWNER_HWND: AtomicIsize = AtomicIsize::new(0);

pub fn to_utf16z(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Copy text into an HGLOBAL and put it on the (already open) clipboard.
unsafe fn set_unicode_text(text: &[u16]) -> bool {
    let bytes = text.len() * 2;
    let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, bytes) else { return false };
    let ptr = GlobalLock(hmem);
    if ptr.is_null() {
        return false;
    }
    std::ptr::copy_nonoverlapping(text.as_ptr() as *const u8, ptr as *mut u8, bytes);
    let _ = GlobalUnlock(hmem);
    SetClipboardData(CF_UNICODETEXT, Some(HANDLE(hmem.0))).is_ok()
}

unsafe fn set_privacy_formats() -> bool {
    // Format name → DWORD value. Presence of the first alone excludes the
    // entry from clipboard monitors; the other two must be 0 (spec §4 / v1).
    let formats: [(&str, u32); 3] = [
        ("ExcludeClipboardContentFromMonitorProcessing", 0),
        ("CanIncludeInClipboardHistory", 0),
        ("CanUploadToCloudClipboard", 0),
    ];
    for (name, value) in formats {
        let wname = to_utf16z(name);
        let id = RegisterClipboardFormatW(PCWSTR(wname.as_ptr()));
        if id == 0 {
            return false;
        }
        let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, 4) else { return false };
        let ptr = GlobalLock(hmem);
        if ptr.is_null() {
            return false;
        }
        std::ptr::copy_nonoverlapping(value.to_le_bytes().as_ptr(), ptr as *mut u8, 4);
        let _ = GlobalUnlock(hmem);
        if SetClipboardData(id, Some(HANDLE(hmem.0))).is_err() {
            return false;
        }
    }
    true
}

unsafe fn open_clipboard_retrying(hwnd: HWND) -> bool {
    // Another app may hold the clipboard briefly; retry 60 x 10 ms (as v1).
    for _ in 0..60 {
        if OpenClipboard(Some(hwnd)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_RENDERFORMAT => {
            // The target app is pasting RIGHT NOW. The clipboard is already
            // open for us here — SetClipboardData directly, no OpenClipboard.
            if let Some(text) = PENDING.lock().unwrap().clone() {
                let _ = set_unicode_text(&text);
            }
            RENDERED.store(true, Ordering::SeqCst);
            LRESULT(0)
        }
        WM_STAGE => {
            let ok = 'stage: {
                if !open_clipboard_retrying(hwnd) {
                    break 'stage false;
                }
                let ok = EmptyClipboard().is_ok()
                    // NULL handle = delayed rendering. The call reports an
                    // error for NULL by design — ignore its return value.
                    && { let _ = SetClipboardData(CF_UNICODETEXT, None); true }
                    && set_privacy_formats();
                let _ = CloseClipboard();
                ok
            };
            STAGE_OK.store(ok, Ordering::SeqCst);
            STAGE_DONE.store(true, Ordering::SeqCst);
            LRESULT(0)
        }
        WM_RESTORE => {
            let owner = GetClipboardOwner().unwrap_or_default();
            if owner.0 as isize != OWNER_HWND.load(Ordering::SeqCst) {
                // Someone else owns the clipboard now (user copied something
                // since our paste) — leave it alone.
                applog::log("clipboard-restore-skipped-not-owner");
                return LRESULT(0);
            }
            if open_clipboard_retrying(hwnd) {
                let _ = EmptyClipboard();
                if let Some(prev) = RESTORE_TO.lock().unwrap().take() {
                    let _ = set_unicode_text(&prev);
                }
                let _ = CloseClipboard();
                applog::log("clipboard-restored");
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Spawn the hidden clipboard-owner window and its message loop. Call once.
pub fn init() {
    std::thread::spawn(|| unsafe {
        let class_name = to_utf16z("WhisperOSSClipboard");
        let hinstance = GetModuleHandleW(None).expect("module handle");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(class_name.as_ptr()),
            WINDOW_STYLE(0),
            0, 0, 0, 0,
            Some(HWND_MESSAGE), // message-only window: invisible, no taskbar
            None,
            Some(hinstance.into()),
            None,
        )
        .expect("clipboard window");
        OWNER_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    });
    // Give the window a moment to exist before first use.
    for _ in 0..100 {
        if OWNER_HWND.load(Ordering::SeqCst) != 0 {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn post(msg: u32) {
    let hwnd = HWND(OWNER_HWND.load(Ordering::SeqCst) as *mut _);
    unsafe {
        let _ = PostMessageW(Some(hwnd), msg, WPARAM(0), LPARAM(0));
    }
}

/// Read current clipboard text (None if empty or non-text).
pub fn snapshot_text() -> Option<String> {
    unsafe {
        let hwnd = HWND(OWNER_HWND.load(Ordering::SeqCst) as *mut _);
        if !open_clipboard_retrying(hwnd) {
            return None;
        }
        let result = GetClipboardData(CF_UNICODETEXT).ok().and_then(|h| {
            let ptr = GlobalLock(windows::Win32::System::Memory::HGLOBAL(h.0)) as *const u16;
            if ptr.is_null() {
                return None;
            }
            let mut len = 0usize;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
            let _ = GlobalUnlock(windows::Win32::System::Memory::HGLOBAL(h.0));
            Some(s)
        });
        let _ = CloseClipboard();
        result
    }
}

/// Stage `text` for a privacy paste. Returns false if the privacy formats
/// could not be set — the caller MUST abort the paste in that case.
pub fn stage(text: &str, restore_to: Option<String>) -> bool {
    *PENDING.lock().unwrap() = Some(to_utf16z(text));
    *RESTORE_TO.lock().unwrap() = restore_to.map(|s| to_utf16z(&s));
    RENDERED.store(false, Ordering::SeqCst);
    STAGE_DONE.store(false, Ordering::SeqCst);
    post(WM_STAGE);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !STAGE_DONE.load(Ordering::SeqCst) {
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    STAGE_OK.load(Ordering::SeqCst)
}

/// True once the target app has actually pulled our text (WM_RENDERFORMAT).
pub fn wait_pasted(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if RENDERED.load(Ordering::SeqCst) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

pub fn restore() {
    post(WM_RESTORE);
}

/// Synthetic Ctrl+V into the focused app.
pub fn send_ctrl_v() {
    fn key(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }
    let inputs = [
        key(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
        key(VK_V, KEYBD_EVENT_FLAGS(0)),
        key(VK_V, KEYEVENTF_KEYUP),
        key(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}
```

As with Task 5: if the resolved `windows` crate version wraps these signatures differently (`Option` on handles, `HGLOBAL` vs `HANDLE` conversions), adapt call sites only, keep the logic identical, log a deviation.

- [ ] **Step 4: Run tests, then build**

Run: `cargo test clipboard` → expected `1 passed` (suite total 26).
Run: `cargo check` → expected clean compile.

Add `clipboard::init();` as the first line inside the `setup` closure in `src-tauri/src/lib.rs`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/clipboard.rs src-tauri/src/lib.rs
git commit -m "feat: privacy clipboard with delayed-render paste confirmation"
```

---

### Task 9: Pipeline orchestration + tray

**Files:**
- Create: `src-tauri/src/pipeline.rs`
- Modify: `src-tauri/src/lib.rs` (final wiring: remove ALL temporary blocks)

**Interfaces:**
- Consumes: everything from Tasks 1–8 plus `position::*` and `position_overlay` from Milestone 0.
- Produces: `pipeline::start(app: tauri::AppHandle, audio: std::sync::Arc<audio::AudioEngine>, api_key: String)` — the complete dictation loop. The overlay window shows during recording, hides otherwise.

- [ ] **Step 1: Implement the pipeline**

Create `src-tauri/src/pipeline.rs`:

```rust
//! The dictation loop (spec §5): hook events → hold tracker → record →
//! silence gate → transcribe (stale results discarded by generation) →
//! privacy paste → sequenced clipboard restore. One dictation at a time;
//! a new hold makes any in-flight result stale. Errors are logged only
//! in M1 — the overlay error state arrives in M2.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::Duration;

use tauri::Manager;

use crate::{applog, audio, clipboard, dsp, groq, hook, hotkey_logic};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const PASTE_CONFIRM_TIMEOUT: Duration = Duration::from_secs(5);

fn set_overlay_visible(app: &tauri::AppHandle, visible: bool) {
    if let Some(w) = app.get_webview_window("overlay") {
        if visible {
            // Re-anchor to the monitor the cursor is on right now.
            let _ = crate::position_overlay(app);
            let _ = w.show();
        } else {
            let _ = w.hide();
        }
    }
}

pub fn start(app: tauri::AppHandle, audio: Arc<audio::AudioEngine>, api_key: String) {
    let (tx, rx) = channel();
    hook::spawn(tx);

    let client = Arc::new(groq::GroqClient::new(
        api_key,
        groq::PROD_BASE_URL.to_string(),
        REQUEST_TIMEOUT,
    ));
    let generation = Arc::new(AtomicU64::new(0));

    std::thread::spawn(move || {
        let mut tracker = hotkey_logic::HoldTracker::new();
        for ev in rx {
            match tracker.on_event(ev) {
                hotkey_logic::Action::None => {}
                hotkey_logic::Action::Start => {
                    generation.fetch_add(1, Ordering::SeqCst);
                    if !audio.is_healthy() {
                        applog::log("recording-refused-no-mic");
                        continue;
                    }
                    audio.start_recording();
                    set_overlay_visible(&app, true);
                    applog::log("recording-start");
                }
                hotkey_logic::Action::Cancel => {
                    let _ = audio.stop_recording();
                    set_overlay_visible(&app, false);
                    applog::log("recording-cancel-short-tap");
                }
                hotkey_logic::Action::Finish { held_ms } => {
                    let (samples, rate) = audio.stop_recording();
                    applog::log(&format!(
                        "recording-finish held_ms={held_ms} samples={}",
                        samples.len()
                    ));
                    if dsp::is_effectively_silent(&samples) {
                        set_overlay_visible(&app, false);
                        applog::log("silent-discarded");
                        continue;
                    }
                    let wav = dsp::encode_wav_mono16(
                        &dsp::resample_to_16k(&samples, rate),
                        16_000,
                    );
                    let my_gen = generation.load(Ordering::SeqCst);
                    let client = client.clone();
                    let generation = generation.clone();
                    let app = app.clone();
                    std::thread::spawn(move || {
                        let result = client.transcribe(wav);
                        let stale = generation.load(Ordering::SeqCst) != my_gen;
                        match result {
                            Ok(text) if stale => applog::log("result-discarded-stale"),
                            Ok(text) if text.is_empty() => applog::log("empty-transcript"),
                            Ok(text) => paste(&text),
                            Err(e) => applog::log(&format!("transcribe-error {e:?}")),
                        }
                        set_overlay_visible(&app, false);
                    });
                }
            }
        }
    });
}

fn paste(text: &str) {
    let previous = clipboard::snapshot_text();
    if previous.is_none() {
        applog::log("clipboard-snapshot-empty-or-nontext");
    }
    if !clipboard::stage(text, previous) {
        // Privacy formats could not be set: NEVER paste unprotected (spec §6).
        applog::log("paste-aborted-privacy-staging-failed");
        return;
    }
    std::thread::sleep(Duration::from_millis(60)); // let the clipboard settle
    clipboard::send_ctrl_v();
    let confirmed = clipboard::wait_pasted(PASTE_CONFIRM_TIMEOUT);
    std::thread::sleep(Duration::from_millis(250));
    clipboard::restore();
    applog::log(if confirmed { "pasted-confirmed" } else { "paste-unconfirmed" });
}
```

- [ ] **Step 2: Final `src-tauri/src/lib.rs`**

Replace the whole file with:

```rust
mod applog;
mod audio;
mod clipboard;
mod dsp;
mod groq;
mod hook;
mod hotkey_logic;
mod keys;
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
```

In `src-tauri/tauri.conf.json`, change the overlay window's `"visible": true` to `"visible": false` (hidden until dictation).

- [ ] **Step 3: Verify build and tests**

Run: `cargo test` → expected: 26 passed across suites.
Run: `cargo check` → clean.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: full dictation pipeline wired - hotkey to privacy paste, tray quit"
```

---

### Task 10: End-to-end verification and milestone report

**Files:**
- Create: `docs/reports/milestone-1-results.md`

**Interfaces:**
- Consumes: the whole app.
- Produces: the recorded evidence Milestone 2 planning depends on.

- [ ] **Step 1: One-time key setup (human provides the key)**

In the shell that will run the app, set the env var with the user's real Groq key (value never goes in any file or commit):

PowerShell: `$env:WHISPEROSS_GROQ_KEY = "<real key>"`
Git Bash: `export WHISPEROSS_GROQ_KEY="<real key>"`

First run logs `api-key-bootstrapped-from-env`; every later run (even without the env var) logs `api-key-from-credential-manager`.

- [ ] **Step 2: Run the E2E protocol (human at the keyboard)**

Run `npm run tauri dev`, then walk through, checking `%APPDATA%\WhisperOSS\log.txt` after each:

1. **Happy path:** focus Notepad, hold Ctrl+Win, say "hello world, this is a test", release. Expected: pill appears with live bars while held, disappears after; the spoken words appear in Notepad within ~1–2 s; log shows `recording-start` → `recording-finish` → `pasted-confirmed`.
2. **First-word check:** speak IMMEDIATELY as the keys go down ("testing one two three" with no pause). Expected: "testing" is not clipped (the pre-roll working).
3. **Clipboard preservation:** copy the text `SENTINEL` in Notepad, dictate a sentence, then press Ctrl+V manually. Expected: `SENTINEL` pastes — the transcript did not stay on the clipboard; log shows `clipboard-restored`.
4. **Win+V hygiene:** open the Win+V clipboard history panel. Expected: the dictated sentence is NOT in the history list.
5. **Short tap:** tap Ctrl+Win for an instant. Expected: nothing pastes; log shows `recording-cancel-short-tap`.
6. **Silence:** hold Ctrl+Win for 2 s saying nothing, release. Expected: nothing pastes, no network request; log shows `silent-discarded`.
7. **Network failure:** disconnect Wi-Fi, dictate a sentence. Expected: nothing pastes, app stays alive, pill hides; log shows `transcribe-error Network`. Reconnect, dictate again — works.
8. **Rapid re-dictation:** dictate a long sentence and immediately start a second short dictation before the first can finish. Expected: only the second dictation's text pastes (or the first is logged `result-discarded-stale`) — never both interleaved.
9. **Tray:** right-click the tray icon → Quit. Expected: app exits cleanly.

- [ ] **Step 3: Write the results report**

Create `docs/reports/milestone-1-results.md`:

```markdown
# Milestone 1 results — headless dictation pipeline

Date: <run date>
Machine: <CPU / mic / Windows version>

| # | Check | Result | Notes |
|---|---|---|---|
| 1 | Happy path: speech → text in Notepad | __ | latency felt: __ s |
| 2 | First word not clipped (pre-roll) | __ | |
| 3 | Original clipboard restored | __ | |
| 4 | Transcript absent from Win+V history | __ | |
| 5 | Short tap ignored | __ | |
| 6 | Silence discarded without upload | __ | |
| 7 | Network failure: logged, app alive, recovers | __ | |
| 8 | Rapid re-dictation: no interleaved pastes | __ | |
| 9 | Tray quit clean | __ | |

Automated tests: __ passed.
Verdict: GO / NO-GO for Milestone 2.
Deviations: <list or "none">
```

Fill every cell from the actual runs.

- [ ] **Step 4: Commit**

```bash
git add docs/reports/milestone-1-results.md
git commit -m "docs: milestone 1 end-to-end results"
```

If any check fails: STOP, report with the relevant log lines. Fixes to plan-specified code are an architecture conversation, not an improvisation.
