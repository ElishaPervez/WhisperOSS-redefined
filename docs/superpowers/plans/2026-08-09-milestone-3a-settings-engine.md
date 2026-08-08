# Milestone 3a — Settings Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Everything the settings window will control exists and works first, driven by a config file: persisted settings, a rebindable hold-to-dictate combo (modifiers + optionally one regular key, with key-swallowing), the AI formatting / casual-mode request, microphone selection, and start-with-Windows. No new UI in this plan — apply-on-restart via `config.json`; Milestone 3b adds the window and live apply.

**Architecture:** A `Config` struct serialized to `%APPDATA%\WhisperOSS\config.json` (missing/corrupt fields fall back to defaults — the file can never brick the app). The hotkey layer generalizes: the hook forwards ALL key transitions as `(key, up/down, time)`; a combo-aware tracker starts when every combo key is held and finishes on the first release. If the combo contains a non-modifier key, the hook swallows exactly that key while the combo's modifiers are held, so it never types into the focused app. Formatting is a second Groq request (chat), applied after transcription when enabled, falling back to the raw transcript on failure.

**Tech Stack:** Existing app. One new `windows` crate feature (`Win32_System_Registry`). No other new dependencies (`serde` derive comes via a `serde_json`-companion addition of `serde` with derive).

**Spec:** `docs/superpowers/specs/2026-08-08-whispeross-v2-design.md` §2 (in-scope toggles), §4 (config keys, autostart, hardcoded models). Scope decisions from the user (2026-08-09): tray left-click opens settings (M3b); hotkey rebind allows modifiers + any single regular key.

## Global Constraints

- Windows-only. Repo root: `C:\projects (code)\15. WhisperOSS redefined` (quote paths). `cargo` runs from `src-tauri\`. Never touch `src-reference\`.
- Config file: `%APPDATA%\WhisperOSS\config.json`, pretty-printed JSON. The API key is NEVER in it (Credential Manager only, from M1).
- Pinned values: formatting model **`openai/gpt-oss-120b`**, temperature **0.3**, same 15 s timeout + 1 retry as transcription. Hotkey default **ctrl+win**, minimum 2 keys, at least 1 modifier, at most 1 non-modifier key.
- Formatting failure NEVER loses a dictation: fall back to pasting the raw transcript and log it.
- New compiler warnings from your changes: stop and fix. New warnings in untouched code: report in DEVIATIONS.
- Existing 29 tests stay green; this plan adds more (running totals stated per task).
- Not in this plan: any UI, live config apply (restart-to-apply is correct here), hotkey capture UX, tray changes, acrylic. All M3b.

---

### Task 1: Config module (TDD)

**Files:**
- Modify: `src-tauri/Cargo.toml` (add `serde`)
- Create: `src-tauri/src/config.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod config;`)

**Interfaces:**
- Consumes: nothing.
- Produces: `config::Config` (fields below, all `serde(default)`), `Config::default()`, `config::from_json(text: &str) -> Config` (corrupt-tolerant), `config::to_json(cfg: &Config) -> String`, `config::load() -> Config`, `config::save(cfg: &Config)` (writes `%APPDATA%\WhisperOSS\config.json`). Tasks 3, 5, 6, 7 consume.

- [ ] **Step 1: Add serde to `src-tauri/Cargo.toml` dependencies**

```toml
serde = { version = "1", features = ["derive"] }
```

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/src/config.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let c = Config::default();
        assert_eq!(c.hotkey, vec!["ctrl".to_string(), "win".to_string()]);
        assert!(!c.use_formatter);
        assert!(!c.casual_mode);
        assert_eq!(c.input_device, None);
        assert_eq!(c.theme, "auto");
        assert!(c.run_on_startup);
    }

    #[test]
    fn partial_json_fills_missing_fields_with_defaults() {
        let c = from_json(r#"{ "use_formatter": true }"#);
        assert!(c.use_formatter);
        assert_eq!(c.hotkey, vec!["ctrl".to_string(), "win".to_string()]);
        assert_eq!(c.theme, "auto");
    }

    #[test]
    fn corrupt_json_falls_back_to_defaults() {
        let c = from_json("{ not valid json !!");
        assert_eq!(c, Config::default());
    }

    #[test]
    fn roundtrip() {
        let mut c = Config::default();
        c.casual_mode = true;
        c.input_device = Some("Yeti Nano (WASAPI)".into());
        assert_eq!(from_json(&to_json(&c)), c);
    }
}
```

Add `mod config;` to `src-tauri/src/lib.rs`.

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test config`
Expected: FAIL to compile.

- [ ] **Step 4: Implement**

Add above the test module in `src-tauri/src/config.rs`:

```rust
//! Persisted settings (spec §4). Every field has a default and the file is
//! corrupt-tolerant: a bad or missing config.json can never brick the app.
//! The API key is NOT here — it lives in Windows Credential Manager (M1).

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Hold-to-dictate combo: lowercase key names, e.g. ["ctrl","win"] or
    /// ["ctrl","space"]. Validated by hotkey_logic::parse_combo.
    pub hotkey: Vec<String>,
    pub use_formatter: bool,
    pub casual_mode: bool,
    /// Input device by NAME (indexes shift between sessions). None = default.
    pub input_device: Option<String>,
    /// "auto" | "light" | "dark" (consumed by the settings window in M3b).
    pub theme: String,
    pub run_on_startup: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            hotkey: vec!["ctrl".into(), "win".into()],
            use_formatter: false,
            casual_mode: false,
            input_device: None,
            theme: "auto".into(),
            run_on_startup: true,
        }
    }
}

pub fn from_json(text: &str) -> Config {
    serde_json::from_str(text).unwrap_or_default()
}

pub fn to_json(cfg: &Config) -> String {
    serde_json::to_string_pretty(cfg).expect("config serializes")
}

fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    let dir = PathBuf::from(base).join("WhisperOSS");
    fs::create_dir_all(&dir).ok()?;
    Some(dir.join("config.json"))
}

pub fn load() -> Config {
    config_path()
        .and_then(|p| fs::read_to_string(p).ok())
        .map(|t| from_json(&t))
        .unwrap_or_default()
}

pub fn save(cfg: &Config) {
    if let Some(p) = config_path() {
        let _ = fs::write(p, to_json(cfg));
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test config`
Expected: `4 passed` (suite total 33).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/config.rs src-tauri/src/lib.rs
git commit -m "feat: corrupt-tolerant persisted config (tested)"
```

---

### Task 2: Formatting request + key validation (TDD)

**Files:**
- Create: `src-tauri/src/prompts.rs`
- Modify: `src-tauri/src/groq.rs` (add `format_text` and `validate_key`)
- Modify: `src-tauri/src/lib.rs` (add `mod prompts;`)

**Interfaces:**
- Consumes: existing `GroqClient` internals.
- Produces: `prompts::FORMAT_PROMPT: &str`, `prompts::CASUAL_PROMPT: &str`; `GroqClient::format_text(&self, text: &str, casual: bool) -> Result<String, GroqError>` (same retry rules as transcribe); `GroqClient::validate_key(&self) -> Result<(), GroqError>` (used by M3b's Save button; built now so the client is complete). Task 6 consumes `format_text`.

- [ ] **Step 1: Create `src-tauri/src/prompts.rs`**

```rust
//! System prompts for the optional AI cleanup pass (spec §2). Wording is
//! part of the product spec — do not tune without a spec change.

pub const FORMAT_PROMPT: &str = "You are a dictation formatter. Rewrite the \
user's raw speech transcript with correct punctuation, capitalization, and \
paragraph breaks. Preserve every word and the speaker's meaning; do not \
add content, summarize, or answer questions found in the text. Convert \
spoken math and units to symbols (for example 'x squared' becomes x\u{00b2}, \
'45 degrees' becomes 45\u{00b0}). Output plain text only - no markdown, no \
quotes around the result, no commentary.";

pub const CASUAL_PROMPT: &str = "You are a dictation formatter for casual \
chat. Rewrite the transcript in all lowercase with minimal punctuation: no \
sentence-ending periods, no commas unless needed for clarity. Keep slang \
and phrasing exactly as spoken. Convert spoken emoji names into the actual \
emoji, honoring counts ('three crying emojis' becomes three of that emoji). \
Preserve meaning; add nothing. Output plain text only - no markdown, no \
commentary.";
```

Add `mod prompts;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Write the failing tests (append inside the existing `tests` module in `src-tauri/src/groq.rs`)**

```rust
    #[test]
    fn format_text_sends_chat_and_returns_content() {
        let base = serve_once(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n{\"choices\":[{\"message\":{\"content\":\" Hello, world. \"}}]}",
        );
        let c = client(base);
        assert_eq!(c.format_text("hello world", false).unwrap(), "Hello, world.");
    }

    #[test]
    fn format_text_unauthorized_maps() {
        let base = serve_once("HTTP/1.1 401 Unauthorized\r\nconnection: close\r\n\r\n{}");
        assert!(matches!(client(base).format_text("x", true),
                         Err(GroqError::Unauthorized)));
    }

    #[test]
    fn validate_key_ok_and_unauthorized() {
        let base = serve_once(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n{\"data\":[]}",
        );
        assert!(client(base).validate_key().is_ok());
        let base = serve_once("HTTP/1.1 401 Unauthorized\r\nconnection: close\r\n\r\n{}");
        assert!(matches!(client(base).validate_key(),
                         Err(GroqError::Unauthorized)));
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test groq`
Expected: FAIL to compile.

- [ ] **Step 4: Implement (add to the `impl GroqClient` block in `src-tauri/src/groq.rs`)**

Also add at the top of the file, next to `MODEL`:

```rust
const FORMAT_MODEL: &str = "openai/gpt-oss-120b";
```

And in the impl block:

```rust
    /// Optional cleanup pass (spec §2). Same retry discipline as transcribe.
    pub fn format_text(&self, text: &str, casual: bool) -> Result<String, GroqError> {
        let mut last = None;
        for _ in 0..2 {
            match self.format_attempt(text, casual) {
                Ok(t) => return Ok(t),
                Err(GroqError::Unauthorized) => return Err(GroqError::Unauthorized),
                Err(e) => last = Some(e),
            }
        }
        Err(last.expect("at least one attempt ran"))
    }

    fn format_attempt(&self, text: &str, casual: bool) -> Result<String, GroqError> {
        let prompt = if casual {
            crate::prompts::CASUAL_PROMPT
        } else {
            crate::prompts::FORMAT_PROMPT
        };
        let body = serde_json::json!({
            "model": FORMAT_MODEL,
            "temperature": 0.3,
            "messages": [
                { "role": "system", "content": prompt },
                { "role": "user", "content": text },
            ],
        });
        let resp = self
            .http
            .post(format!("{}/openai/v1/chat/completions", self.base))
            .bearer_auth(&self.key)
            .json(&body)
            .send()
            .map_err(|e| GroqError::Network(e.to_string()))?;
        match resp.status().as_u16() {
            200 => {
                let v: serde_json::Value =
                    resp.json().map_err(|e| GroqError::Network(e.to_string()))?;
                Ok(v["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .to_string())
            }
            401 | 403 => Err(GroqError::Unauthorized),
            s => Err(GroqError::Server(format!("HTTP {s}"))),
        }
    }

    /// Cheap key check for the settings Save button (M3b).
    pub fn validate_key(&self) -> Result<(), GroqError> {
        let resp = self
            .http
            .get(format!("{}/openai/v1/models", self.base))
            .bearer_auth(&self.key)
            .send()
            .map_err(|e| GroqError::Network(e.to_string()))?;
        match resp.status().as_u16() {
            200 => Ok(()),
            401 | 403 => Err(GroqError::Unauthorized),
            s => Err(GroqError::Server(format!("HTTP {s}"))),
        }
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test groq`
Expected: `7 passed` in the groq module (suite total 36).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/prompts.rs src-tauri/src/groq.rs src-tauri/src/lib.rs
git commit -m "feat: AI formatting request, casual prompt, key validation (tested)"
```

---

### Task 3: Combo-aware hotkey logic (TDD)

**Files:**
- Modify: `src-tauri/src/hotkey_logic.rs` (full replacement below — the old Ctrl/Win-only tracker generalizes)

**Interfaces:**
- Consumes: nothing.
- Produces (Task 4 and 6 consume):
  `Key { Ctrl, Win, Alt, Shift, Other(u32) }`;
  `KeyEvent { Down(Key, u64), Up(Key, u64) }`;
  `Action { None, Start, Finish { held_ms: u64 }, Cancel }` (unchanged);
  `key_from_vk(vk: u32) -> Key`;
  `parse_combo(names: &[String]) -> Option<Vec<Key>>` (None = invalid per the rules: ≥2 keys, ≥1 modifier, ≤1 non-modifier);
  `combo_other_vk(combo: &[Key]) -> Option<u32>` (the vk the hook must swallow, if any);
  `HoldTracker::new(combo: Vec<Key>)`, `on_event(&mut self, ev: KeyEvent) -> Action`.

- [ ] **Step 1: Replace `src-tauri/src/hotkey_logic.rs` entirely with the tests first** (file contains ONLY this until Step 3):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl_win() -> Vec<Key> {
        parse_combo(&["ctrl".into(), "win".into()]).unwrap()
    }

    #[test]
    fn parse_combo_accepts_and_rejects_per_rules() {
        assert!(parse_combo(&["ctrl".into(), "win".into()]).is_some());
        assert!(parse_combo(&["ctrl".into(), "alt".into(), "shift".into()]).is_some());
        assert!(parse_combo(&["ctrl".into(), "space".into()]).is_some());
        assert!(parse_combo(&["alt".into(), "d".into()]).is_some());
        // rejected: single key, no modifier, two non-modifiers, unknown name
        assert!(parse_combo(&["ctrl".into()]).is_none());
        assert!(parse_combo(&["a".into(), "b".into()]).is_none());
        assert!(parse_combo(&["ctrl".into(), "a".into(), "b".into()]).is_none());
        assert!(parse_combo(&["ctrl".into(), "banana".into()]).is_none());
    }

    #[test]
    fn key_names_map_to_expected_keys() {
        assert_eq!(parse_combo(&["ctrl".into(), "space".into()]).unwrap()[1],
                   Key::Other(0x20));
        assert_eq!(parse_combo(&["win".into(), "d".into()]).unwrap()[1],
                   Key::Other(0x44));
    }

    #[test]
    fn vk_variants_collapse_to_one_key() {
        assert_eq!(key_from_vk(0x11), Key::Ctrl);
        assert_eq!(key_from_vk(0xA2), Key::Ctrl);
        assert_eq!(key_from_vk(0xA3), Key::Ctrl);
        assert_eq!(key_from_vk(0x5B), Key::Win);
        assert_eq!(key_from_vk(0x5C), Key::Win);
        assert_eq!(key_from_vk(0x12), Key::Alt);
        assert_eq!(key_from_vk(0xA0), Key::Shift);
        assert_eq!(key_from_vk(0x20), Key::Other(0x20));
    }

    #[test]
    fn combo_other_vk_extraction() {
        assert_eq!(combo_other_vk(&ctrl_win()), None);
        let c = parse_combo(&["ctrl".into(), "space".into()]).unwrap();
        assert_eq!(combo_other_vk(&c), Some(0x20));
    }

    #[test]
    fn default_combo_full_cycle() {
        let mut t = HoldTracker::new(ctrl_win());
        assert_eq!(t.on_event(KeyEvent::Down(Key::Ctrl, 0)), Action::None);
        assert_eq!(t.on_event(KeyEvent::Down(Key::Win, 10)), Action::Start);
        assert_eq!(t.on_event(KeyEvent::Up(Key::Ctrl, 400)),
                   Action::Finish { held_ms: 390 });
        // release of the second key after finish: ignored
        assert_eq!(t.on_event(KeyEvent::Up(Key::Win, 450)), Action::None);
    }

    #[test]
    fn short_tap_cancels() {
        let mut t = HoldTracker::new(ctrl_win());
        t.on_event(KeyEvent::Down(Key::Win, 0));
        assert_eq!(t.on_event(KeyEvent::Down(Key::Ctrl, 20)), Action::Start);
        assert_eq!(t.on_event(KeyEvent::Up(Key::Ctrl, 100)), Action::Cancel);
    }

    #[test]
    fn key_repeat_does_not_double_start() {
        let mut t = HoldTracker::new(ctrl_win());
        t.on_event(KeyEvent::Down(Key::Ctrl, 0));
        assert_eq!(t.on_event(KeyEvent::Down(Key::Win, 10)), Action::Start);
        assert_eq!(t.on_event(KeyEvent::Down(Key::Ctrl, 50)), Action::None);
        assert!(matches!(t.on_event(KeyEvent::Up(Key::Win, 500)),
                         Action::Finish { .. }));
    }

    #[test]
    fn unrelated_keys_do_not_disturb_hold() {
        let mut t = HoldTracker::new(ctrl_win());
        t.on_event(KeyEvent::Down(Key::Ctrl, 0));
        t.on_event(KeyEvent::Down(Key::Win, 10));
        assert_eq!(t.on_event(KeyEvent::Down(Key::Other(0x41), 50)), Action::None);
        assert_eq!(t.on_event(KeyEvent::Up(Key::Other(0x41), 90)), Action::None);
        assert!(matches!(t.on_event(KeyEvent::Up(Key::Ctrl, 400)),
                         Action::Finish { .. }));
    }

    #[test]
    fn three_key_combo_requires_all_three() {
        let c = parse_combo(&["ctrl".into(), "alt".into(), "space".into()]).unwrap();
        let mut t = HoldTracker::new(c);
        t.on_event(KeyEvent::Down(Key::Ctrl, 0));
        assert_eq!(t.on_event(KeyEvent::Down(Key::Other(0x20), 5)), Action::None);
        assert_eq!(t.on_event(KeyEvent::Down(Key::Alt, 10)), Action::Start);
        assert!(matches!(t.on_event(KeyEvent::Up(Key::Other(0x20), 400)),
                         Action::Finish { .. }));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test hotkey_logic`
Expected: FAIL to compile.

- [ ] **Step 3: Implement (add above the test module)**

```rust
//! Combo-aware hold-to-dictate logic. The combo comes from config
//! ("ctrl"+"win" by default; user-rebindable in M3b). Rules: at least two
//! keys, at least one modifier, at most one non-modifier — enforced by
//! parse_combo so an invalid config can never produce a broken tracker.
//! Recording starts when EVERY combo key is down; the first release of any
//! combo key finishes (or cancels under MIN_HOLD_MS — accidental tap).

pub const MIN_HOLD_MS: u64 = 150;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Ctrl,
    Win,
    Alt,
    Shift,
    Other(u32),
}

impl Key {
    pub fn is_modifier(&self) -> bool {
        !matches!(self, Key::Other(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyEvent {
    Down(Key, u64),
    Up(Key, u64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    None,
    Start,
    Finish { held_ms: u64 },
    Cancel,
}

/// Collapse left/right/generic virtual-key variants into one logical key.
pub fn key_from_vk(vk: u32) -> Key {
    match vk {
        0x11 | 0xA2 | 0xA3 => Key::Ctrl,
        0x5B | 0x5C => Key::Win,
        0x12 | 0xA4 | 0xA5 => Key::Alt,
        0x10 | 0xA0 | 0xA1 => Key::Shift,
        other => Key::Other(other),
    }
}

fn key_from_name(name: &str) -> Option<Key> {
    match name {
        "ctrl" => Some(Key::Ctrl),
        "win" => Some(Key::Win),
        "alt" => Some(Key::Alt),
        "shift" => Some(Key::Shift),
        "space" => Some(Key::Other(0x20)),
        "tab" => Some(Key::Other(0x09)),
        "capslock" => Some(Key::Other(0x14)),
        s if s.len() == 1 => {
            let c = s.chars().next().unwrap().to_ascii_uppercase();
            if c.is_ascii_uppercase() || c.is_ascii_digit() {
                Some(Key::Other(c as u32))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Validate and parse a combo from config names. None = invalid combo.
pub fn parse_combo(names: &[String]) -> Option<Vec<Key>> {
    let keys: Option<Vec<Key>> = names.iter().map(|n| key_from_name(n)).collect();
    let keys = keys?;
    let modifiers = keys.iter().filter(|k| k.is_modifier()).count();
    let others = keys.len() - modifiers;
    if keys.len() >= 2 && modifiers >= 1 && others <= 1 {
        Some(keys)
    } else {
        None
    }
}

/// The regular key the OS hook must swallow while the combo is held, if any.
pub fn combo_other_vk(combo: &[Key]) -> Option<u32> {
    combo.iter().find_map(|k| match k {
        Key::Other(vk) => Some(*vk),
        _ => None,
    })
}

pub struct HoldTracker {
    combo: Vec<Key>,
    down: Vec<bool>,
    started_at: Option<u64>,
}

impl HoldTracker {
    pub fn new(combo: Vec<Key>) -> Self {
        let n = combo.len();
        Self { combo, down: vec![false; n], started_at: None }
    }

    fn index_of(&self, key: Key) -> Option<usize> {
        self.combo.iter().position(|&k| k == key)
    }

    pub fn on_event(&mut self, ev: KeyEvent) -> Action {
        match ev {
            KeyEvent::Down(key, t) => {
                let Some(i) = self.index_of(key) else { return Action::None };
                self.down[i] = true;
                if self.down.iter().all(|&d| d) && self.started_at.is_none() {
                    self.started_at = Some(t);
                    Action::Start
                } else {
                    Action::None
                }
            }
            KeyEvent::Up(key, t) => {
                let Some(i) = self.index_of(key) else { return Action::None };
                self.down[i] = false;
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
    }
}
```

- [ ] **Step 4: Run tests — the whole suite**

Run: `cargo test`
Expected: `hotkey_logic` shows `9 passed`. The OLD hotkey tests are gone (replaced by these). Compile of `hook.rs`/`pipeline.rs` FAILS at this point — they still use the old `CtrlDown/WinUp` shapes. That is expected mid-task; Task 4 fixes the hook and Task 6 fixes the pipeline. To verify just this module compiles, run: `cargo test --lib hotkey_logic` and confirm the failures are ONLY unresolved names in `hook.rs`/`pipeline.rs`.

**Because the tree doesn't build, Tasks 3, 4, and 6 commit together at the end of Task 6.** (Deviation from one-commit-per-task, declared here by design: the combo change is one atomic refactor across three files.)

---

### Task 4: Hook forwards all keys + swallows the combo's regular key

**Files:**
- Modify: `src-tauri/src/hook.rs` (full replacement below)

**Interfaces:**
- Consumes: `hotkey_logic::{Key, KeyEvent, key_from_vk}`.
- Produces: `hook::spawn(tx: Sender<KeyEvent>)` (now forwards EVERY key transition); `hook::set_suppression(other_vk: Option<u32>, required_modifiers: &[Key])` — called by the pipeline when the active combo contains a regular key, so the hook swallows exactly that key while all the combo's modifiers are physically held.

- [ ] **Step 1: Replace `src-tauri/src/hook.rs` entirely with:**

```rust
//! Global low-level keyboard hook. Forwards every key transition (as
//! logical Keys) to the pipeline. If the active combo contains one regular
//! key (e.g. Space in Ctrl+Space), that key is SWALLOWED while all the
//! combo's modifier keys are held — otherwise holding the combo would type
//! into the focused app. Modifier keys are never swallowed.
//! PRIVACY: key events are never logged — only forwarded in memory.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::OnceLock;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW,
    TranslateMessage, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::hotkey_logic::{key_from_vk, Key, KeyEvent};

static SENDER: OnceLock<Sender<KeyEvent>> = OnceLock::new();

// Suppression state, written by the pipeline, read synchronously in the
// hook. SUPPRESS_VK == 0 means "suppress nothing".
static SUPPRESS_VK: AtomicU32 = AtomicU32::new(0);
static REQUIRED_MODS: AtomicU32 = AtomicU32::new(0);
static MODS_DOWN: AtomicU32 = AtomicU32::new(0);

const CTRL_BIT: u32 = 1;
const WIN_BIT: u32 = 2;
const ALT_BIT: u32 = 4;
const SHIFT_BIT: u32 = 8;

fn modifier_bit(key: Key) -> u32 {
    match key {
        Key::Ctrl => CTRL_BIT,
        Key::Win => WIN_BIT,
        Key::Alt => ALT_BIT,
        Key::Shift => SHIFT_BIT,
        Key::Other(_) => 0,
    }
}

pub fn set_suppression(other_vk: Option<u32>, required_modifiers: &[Key]) {
    let mask = required_modifiers.iter().map(|&k| modifier_bit(k)).fold(0, |a, b| a | b);
    REQUIRED_MODS.store(mask, Ordering::SeqCst);
    SUPPRESS_VK.store(other_vk.unwrap_or(0), Ordering::SeqCst);
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let t = kb.time as u64;
        let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        let up = matches!(wparam.0 as u32, WM_KEYUP | WM_SYSKEYUP);
        if down || up {
            let key = key_from_vk(kb.vkCode);

            // Track which modifiers are physically held.
            let bit = modifier_bit(key);
            if bit != 0 {
                if down {
                    MODS_DOWN.fetch_or(bit, Ordering::SeqCst);
                } else {
                    MODS_DOWN.fetch_and(!bit, Ordering::SeqCst);
                }
            }

            let ev = if down {
                KeyEvent::Down(key, t)
            } else {
                KeyEvent::Up(key, t)
            };
            if let Some(tx) = SENDER.get() {
                let _ = tx.send(ev);
            }

            // Swallow the combo's regular key while its modifiers are held.
            let target = SUPPRESS_VK.load(Ordering::SeqCst);
            if target != 0 && kb.vkCode == target {
                let required = REQUIRED_MODS.load(Ordering::SeqCst);
                if MODS_DOWN.load(Ordering::SeqCst) & required == required {
                    return LRESULT(1);
                }
            }
        }
    }
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

Same signature-adaptation allowance as M1: call-site shapes only, logic identical, log a deviation.

- [ ] **Step 2: Check compile scope**

Run: `cargo check`
Expected: `hook.rs` itself has no errors; remaining errors are ONLY in `pipeline.rs` (fixed in Task 6). No commit yet (see Task 3).

---

### Task 5: Autostart + audio device selection

**Files:**
- Modify: `src-tauri/Cargo.toml` (add `Win32_System_Registry` to the windows features list)
- Create: `src-tauri/src/autostart.rs`
- Modify: `src-tauri/src/audio.rs` (device-by-name selection + listing)
- Modify: `src-tauri/src/lib.rs` (add `mod autostart;`)

**Interfaces:**
- Consumes: `config::Config` fields.
- Produces: `autostart::reconcile(enabled: bool)` (write/remove `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\WhisperOSS` = quoted current-exe path); `autostart::is_enabled() -> bool`; `audio::list_input_devices() -> Vec<String>`; `audio::AudioEngine::start(app, preferred: Option<String>)` (changed signature — picks the named device, falls back to default when absent). M3b's commands consume the listing.

- [ ] **Step 1: Add the registry feature**

In `src-tauri/Cargo.toml`, add `"Win32_System_Registry"` to the `windows` crate's feature list.

- [ ] **Step 2: Write the failing test**

Create `src-tauri/src/autostart.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Integration test against the real per-user registry, using a
    /// throwaway value name so the real "WhisperOSS" entry is untouched.
    #[test]
    fn set_query_remove_roundtrip() {
        const TEST_NAME: &str = "WhisperOSSAutostartTest";
        set_run_value(TEST_NAME, "\"C:\\test\\fake.exe\"");
        assert_eq!(query_run_value(TEST_NAME).as_deref(), Some("\"C:\\test\\fake.exe\""));
        remove_run_value(TEST_NAME);
        assert_eq!(query_run_value(TEST_NAME), None);
    }
}
```

Add `mod autostart;` to `src-tauri/src/lib.rs`. Run `cargo test autostart` → expected: FAIL to compile.

- [ ] **Step 3: Implement `src-tauri/src/autostart.rs`**

```rust
//! Start-with-Windows via the per-user Run key (spec §4). Per-user only —
//! no admin rights involved. reconcile() runs at startup and after the
//! toggle changes (M3b) so the registry always matches the config.

use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW,
    RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE,
    REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
};

use crate::applog;
use crate::clipboard::to_utf16z;

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "WhisperOSS";

fn open_run_key(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Option<HKEY> {
    unsafe {
        let mut hkey = HKEY::default();
        let path = to_utf16z(RUN_KEY);
        let ok = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            access,
            None,
            &mut hkey,
            None,
        );
        if ok.is_ok() { Some(hkey) } else { None }
    }
}

pub(crate) fn set_run_value(name: &str, command: &str) {
    unsafe {
        if let Some(hkey) = open_run_key(KEY_SET_VALUE) {
            let wname = to_utf16z(name);
            let wval = to_utf16z(command);
            let bytes = std::slice::from_raw_parts(
                wval.as_ptr() as *const u8,
                wval.len() * 2,
            );
            let _ = RegSetValueExW(hkey, PCWSTR(wname.as_ptr()), None, REG_SZ, Some(bytes));
            let _ = RegCloseKey(hkey);
        }
    }
}

pub(crate) fn query_run_value(name: &str) -> Option<String> {
    unsafe {
        let path = to_utf16z(RUN_KEY);
        let wname = to_utf16z(name);
        let mut len: u32 = 0;
        let probe = RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            PCWSTR(wname.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut len),
        );
        if probe.is_err() || len == 0 {
            return None;
        }
        let mut buf = vec![0u16; (len as usize) / 2 + 1];
        let mut len2 = len;
        let ok = RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            PCWSTR(wname.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut len2),
        );
        if ok.is_err() {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }
}

pub(crate) fn remove_run_value(name: &str) {
    unsafe {
        if let Some(hkey) = open_run_key(KEY_SET_VALUE) {
            let wname = to_utf16z(name);
            let _ = RegDeleteValueW(hkey, PCWSTR(wname.as_ptr()));
            let _ = RegCloseKey(hkey);
        }
    }
}

pub fn is_enabled() -> bool {
    query_run_value(VALUE_NAME).is_some()
}

/// Make the registry match the config. Called at startup and on toggle.
pub fn reconcile(enabled: bool) {
    if enabled {
        if let Ok(exe) = std::env::current_exe() {
            set_run_value(VALUE_NAME, &format!("\"{}\"", exe.display()));
            applog::log("autostart-enabled");
        }
    } else if is_enabled() {
        remove_run_value(VALUE_NAME);
        applog::log("autostart-disabled");
    }
    let _ = KEY_READ; // feature-used marker; remove if unused warning appears
}
```

(If that last marker line triggers a warning itself, delete it and the `KEY_READ` import — it exists only in case the resolved crate signatures don't otherwise use `KEY_READ`.)

Run `cargo test autostart` → expected `1 passed`.

- [ ] **Step 4: Audio device selection in `src-tauri/src/audio.rs`**

Add near the top (below the imports):

```rust
/// Names of all available input devices, for the settings picker (M3b).
pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    let Ok(devices) = host.input_devices() else { return Vec::new() };
    devices.filter_map(|d| d.name().ok()).collect()
}

fn pick_device(preferred: &Option<String>) -> Option<cpal::Device> {
    let host = cpal::default_host();
    if let Some(name) = preferred {
        if let Ok(mut devices) = host.input_devices() {
            if let Some(d) = devices.find(|d| d.name().ok().as_deref() == Some(name)) {
                return Some(d);
            }
        }
        crate::applog::log("audio-preferred-device-missing-using-default");
    }
    host.default_input_device()
}
```

Change `AudioEngine::start` to accept the preference and pass it through:

```rust
    pub fn start(app: tauri::AppHandle, preferred: Option<String>) -> Arc<AudioEngine> {
```

…and inside the stream thread closure, change `build_stream(&e)` to `build_stream(&e, &preferred)`; change `build_stream`'s signature and device line to:

```rust
fn build_stream(
    engine: &Arc<AudioEngine>,
    preferred: &Option<String>,
) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = pick_device(preferred).ok_or("no input device")?;
    let _ = host; // host only needed by pick_device/list; drop if warned
```

(Adapt the two marker lines away if they cause warnings — they guard against signature drift only.)

- [ ] **Step 5: Check compile scope and commit**

Run: `cargo check` → remaining errors ONLY in `pipeline.rs`/`lib.rs` call sites (old `AudioEngine::start(app)` arity and old hotkey shapes) — fixed in Task 6.
Run: `cargo test autostart config groq` → all pass.

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/autostart.rs src-tauri/src/audio.rs src-tauri/src/lib.rs
git commit -m "feat: autostart registry control and mic selection by name"
```

---

### Task 6: Wire config through the pipeline

**Files:**
- Modify: `src-tauri/src/pipeline.rs` (combo from config, formatter step, suppression)
- Modify: `src-tauri/src/lib.rs` (load config, pass everything through)

**Interfaces:**
- Consumes: everything above.
- Produces: `pipeline::start(app, audio, api_key, cfg: config::Config)` (changed signature). Dictation applies `use_formatter`/`casual_mode`; combo comes from `cfg.hotkey`; autostart reconciled at boot.

- [ ] **Step 1: Update `src-tauri/src/pipeline.rs`**

Change the signature and the tracker/suppression setup (the `Ui` struct and all state choreography from M2 stay exactly as they are):

```rust
pub fn start(
    app: tauri::AppHandle,
    audio: Arc<audio::AudioEngine>,
    api_key: String,
    cfg: crate::config::Config,
) {
    let (tx, rx) = channel();
    hook::spawn(tx);

    let combo = hotkey_logic::parse_combo(&cfg.hotkey).unwrap_or_else(|| {
        applog::log("config-invalid-hotkey-using-default");
        hotkey_logic::parse_combo(&["ctrl".into(), "win".into()]).expect("default combo")
    });
    hook::set_suppression(hotkey_logic::combo_other_vk(&combo),
                          &combo.iter().copied().filter(|k| k.is_modifier()).collect::<Vec<_>>());

    let use_formatter = cfg.use_formatter;
    let casual = cfg.casual_mode;
    // ... existing client/generation setup unchanged ...
```

Replace `let mut tracker = hotkey_logic::HoldTracker::new();` with:

```rust
        let mut tracker = hotkey_logic::HoldTracker::new(combo);
```

In the `Finish` worker, replace the `Ok(text) =>` paste arm with the formatter step:

```rust
                            Ok(text) => {
                                let final_text = if use_formatter {
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
```

- [ ] **Step 2: Update `src-tauri/src/lib.rs` setup**

Replace the audio/pipeline lines in `setup` with:

```rust
            let cfg = config::load();
            config::save(&cfg); // materialize the file with defaults on first run
            autostart::reconcile(cfg.run_on_startup);

            let audio_engine =
                audio::AudioEngine::start(app.handle().clone(), cfg.input_device.clone());

            match keys::load() {
                Some(key) => pipeline::start(
                    app.handle().clone(),
                    audio_engine,
                    key,
                    cfg,
                ),
                None => applog::log("pipeline-not-started-no-key"),
            }
```

- [ ] **Step 3: Full suite + zero warnings**

Run: `cargo test` → expected **38 passed** total (33 config-era + hotkey_logic went 6→9 (+3) + groq +3 − 0 + autostart +1 = 38; if your count differs, list per-module counts in the report).
Run: `cargo check` → zero warnings (delete the two guard-marker lines from Task 5 if they warn).

- [ ] **Step 4: Commit (covers Tasks 3, 4, and 6 — the atomic combo refactor)**

```bash
git add -A
git commit -m "feat: config-driven pipeline - rebindable combo, AI formatting, device pick"
```

---

### Task 7: Engine verification and report

**Files:**
- Create: `docs/reports/milestone-3a-results.md`

- [ ] **Step 1: Protocol (human at the keyboard, `npm run tauri dev` between changes)**

Config edits happen in `%APPDATA%\WhisperOSS\config.json` with the app STOPPED; restart applies them.

1. **Baseline:** unchanged config → dictation works exactly as in M2 (bars → shimmer → check).
2. **Formatting:** set `"use_formatter": true`, restart. Dictate a rambling run-on sentence with "x squared" in it. Expected: pasted text has real punctuation and `x²`.
3. **Casual:** also set `"casual_mode": true`, restart. Dictate "hey what's up, three crying emojis". Expected: lowercase output ending in 😭😭😭.
4. **Rebind (modifiers):** set `"hotkey": ["ctrl","alt"]`, restart. Expected: Ctrl+Alt dictates; Ctrl+Win does nothing.
5. **Rebind (regular key + swallowing):** set `"hotkey": ["ctrl","space"]`, restart, focus Notepad, hold Ctrl+Space and speak. Expected: dictation works AND no spaces appear in Notepad while holding. Plain Space (no Ctrl) still types spaces normally.
6. **Invalid combo:** set `"hotkey": ["banana"]`, restart. Expected: Ctrl+Win works (default fallback); log shows `config-invalid-hotkey-using-default`.
7. **Autostart:** with `"run_on_startup": true` (default) after any run, check `HKCU\...\Run` in Registry Editor → `WhisperOSS` value exists pointing at the dev exe. Set `"run_on_startup": false`, restart app → value gone.
8. **Restore config** to defaults (`hotkey ["ctrl","win"]`, formatter as you prefer) and confirm dictation once more.

- [ ] **Step 2: Write `docs/reports/milestone-3a-results.md`** — same table format as previous milestone reports, one row per check above, plus automated-test count and verdict: GO / NO-GO for Milestone 3b.

- [ ] **Step 3: Commit**

```bash
git add docs/reports/milestone-3a-results.md
git commit -m "docs: milestone 3a engine results"
```

If any check fails: STOP and report with log lines.
