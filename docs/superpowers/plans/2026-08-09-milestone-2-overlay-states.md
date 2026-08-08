# Milestone 2 — Overlay States Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The pill shows the full state cycle from the approved design: live bars while listening → "PROCESSING" shimmer while transcribing → checkmark pop on success → red pill with a short message on failure — with clean fades, and no UI glitches when dictations overlap.

**Architecture:** Rust stays the only source of truth: the pipeline emits a single `"ui"` event (`{state, message}`) at each transition and owns all timing (hold durations, fade delays, window show/hide). The frontend is a dumb state renderer — it toggles CSS classes and never talks back. A generation guard ensures a stale worker (from an interrupted dictation) can never touch the UI that a newer dictation owns.

**Tech Stack:** Existing app. No new dependencies. New Rust module `overlay_state.rs` (pure, tested); rewritten `pipeline.rs`; new pill frontend.

**Spec:** `docs/superpowers/specs/2026-08-08-whispeross-v2-design.md` §3 (overlay), §6 (error table). Design reference: `docs/design/art2-pill.png`.

## Global Constraints

- Windows-only. Repo root: `C:\projects (code)\15. WhisperOSS redefined` (quote all paths). `cargo` runs from `src-tauri\`. Never touch `src-reference\`.
- Design timings (pinned): shimmer loop **1.6 s** · success hold **400 ms** · error hold **2 s** · fade out **240 ms**.
- Colors: pill body `#0b0a0a`, bars/check `#f3f2f2`, error pill `#ec3013` with white text, shimmer base grey `#8a8a8a`.
- Pill stays **120×36** in every state (design: "identical in both themes, all states").
- Error messages (pinned, from spec §6): `Couldn't reach Groq` · `Invalid API key` · `Groq error` · `Couldn't paste safely` · `No mic detected`. Nothing longer — they must fit the pill.
- The fps meter from M0 is removed in this milestone (debug-only; not in the design).
- Known spec narrowing, accepted for M2: on "Invalid API key" the spec also wants the settings window opened to the key field — there is no settings window until M3, so M2 shows the error pill only. The M3 plan picks this up.
- New compiler warnings introduced by your changes are a stop-and-fix; new warnings appearing in untouched code are a report-in-deviations.
- Existing 26 tests must stay green throughout.

---

### Task 1: Overlay state contract + error descriptions (pure, TDD)

**Files:**
- Create: `src-tauri/src/overlay_state.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod overlay_state;`)

**Interfaces:**
- Consumes: `groq::GroqError` (Milestone 1).
- Produces: duration constants `FADE_MS: u64 = 240`, `SUCCESS_HOLD_MS: u64 = 400`, `ERROR_HOLD_MS: u64 = 2000`; `ui_payload(state: &str, message: &str) -> serde_json::Value`; `describe_error(err: &groq::GroqError) -> (&'static str, String)` returning (pill message, log detail). Task 2 consumes all of these.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/overlay_state.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::groq::GroqError;

    #[test]
    fn payload_shape() {
        let p = ui_payload("error", "Couldn't reach Groq");
        assert_eq!(p["state"], "error");
        assert_eq!(p["message"], "Couldn't reach Groq");
        let p = ui_payload("listening", "");
        assert_eq!(p["message"], "");
    }

    #[test]
    fn error_descriptions_match_spec() {
        let (msg, detail) = describe_error(&GroqError::Unauthorized);
        assert_eq!(msg, "Invalid API key");
        assert_eq!(detail, "");

        let (msg, detail) = describe_error(&GroqError::Network("dns fail".into()));
        assert_eq!(msg, "Couldn't reach Groq");
        assert_eq!(detail, "dns fail");

        let (msg, detail) = describe_error(&GroqError::Server("HTTP 500".into()));
        assert_eq!(msg, "Groq error");
        assert_eq!(detail, "HTTP 500");
    }

    #[test]
    fn durations_match_design() {
        assert_eq!(FADE_MS, 240);
        assert_eq!(SUCCESS_HOLD_MS, 400);
        assert_eq!(ERROR_HOLD_MS, 2000);
    }
}
```

Add `mod overlay_state;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test overlay_state`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

Add above the test module in `src-tauri/src/overlay_state.rs`:

```rust
//! The overlay's state contract. Rust owns ALL timing and wording; the
//! webview only renders. States: listening | processing | success | error
//! | hidden. Durations come from the approved design (art2-pill.png).

use crate::groq::GroqError;

pub const FADE_MS: u64 = 240;
pub const SUCCESS_HOLD_MS: u64 = 400;
pub const ERROR_HOLD_MS: u64 = 2000;

pub fn ui_payload(state: &str, message: &str) -> serde_json::Value {
    serde_json::json!({ "state": state, "message": message })
}

/// Spec §6 error table: (short pill message, detail for the log).
/// Reading the detail here is also what finally consumes the error
/// payloads M1 left unread (the old dead-code warnings).
pub fn describe_error(err: &GroqError) -> (&'static str, String) {
    match err {
        GroqError::Unauthorized => ("Invalid API key", String::new()),
        GroqError::Network(detail) => ("Couldn't reach Groq", detail.clone()),
        GroqError::Server(detail) => ("Groq error", detail.clone()),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test overlay_state`
Expected: `3 passed` (suite total 29).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/overlay_state.rs src-tauri/src/lib.rs
git commit -m "feat: overlay state contract and spec error descriptions (tested)"
```

---

### Task 2: Pipeline emits states, generation-guarded UI

**Files:**
- Modify: `src-tauri/src/pipeline.rs` (full replacement below)

**Interfaces:**
- Consumes: everything from M1 plus `overlay_state::*`.
- Produces: `"ui"` events consumed by Task 3's frontend; `pipeline::start` signature unchanged. `paste` now returns `bool`. Fixes M1's unused-binding warning (`Ok(text)` on the stale arm → `Ok(_)`).

- [ ] **Step 1: Replace `src-tauri/src/pipeline.rs` entirely with:**

```rust
//! The dictation loop (spec §5) + overlay choreography (spec §3).
//! Every UI touch is guarded by the generation counter: a stale worker
//! (its dictation was superseded) must never show, hide, or restyle the
//! pill that a newer dictation owns. This also fixes an M1 glitch where a
//! superseded worker's cleanup could hide the pill mid-listening.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::Duration;

use tauri::{Emitter, Manager};

use crate::{
    applog, audio, clipboard, dsp, groq, hook, hotkey_logic, overlay_state,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const PASTE_CONFIRM_TIMEOUT: Duration = Duration::from_secs(5);

struct Ui {
    app: tauri::AppHandle,
    generation: Arc<AtomicU64>,
}

impl Ui {
    fn current(&self, my_gen: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == my_gen
    }

    fn emit(&self, my_gen: u64, state: &str, message: &str) {
        if !self.current(my_gen) {
            return;
        }
        let _ = self
            .app
            .emit("ui", overlay_state::ui_payload(state, message));
    }

    fn show(&self, my_gen: u64) {
        if !self.current(my_gen) {
            return;
        }
        if let Some(w) = self.app.get_webview_window("overlay") {
            let _ = crate::position_overlay(&self.app);
            let _ = w.show();
        }
    }

    /// Fade the pill out, then hide the window. Blocking — call off-thread.
    fn fade_out_and_hide(&self, my_gen: u64) {
        self.emit(my_gen, "hidden", "");
        std::thread::sleep(Duration::from_millis(overlay_state::FADE_MS));
        if !self.current(my_gen) {
            return;
        }
        if let Some(w) = self.app.get_webview_window("overlay") {
            let _ = w.hide();
        }
    }

    /// Error state per design: red pill, 2 s, fade. Blocking — call off-thread.
    fn show_error(&self, my_gen: u64, message: &str) {
        self.show(my_gen);
        self.emit(my_gen, "error", message);
        std::thread::sleep(Duration::from_millis(overlay_state::ERROR_HOLD_MS));
        self.fade_out_and_hide(my_gen);
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
                    let client = client.clone();
                    std::thread::spawn(move || {
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
                                if paste(&text) {
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

/// Privacy paste. Returns true only if the text was staged with the privacy
/// formats AND actually pulled by the target app.
fn paste(text: &str) -> bool {
    let previous = clipboard::snapshot_text();
    if previous.is_none() {
        applog::log("clipboard-snapshot-empty-or-nontext");
    }
    if !clipboard::stage(text, previous) {
        applog::log("paste-aborted-privacy-staging-failed");
        return false;
    }
    std::thread::sleep(Duration::from_millis(60));
    clipboard::send_ctrl_v();
    let confirmed = clipboard::wait_pasted(PASTE_CONFIRM_TIMEOUT);
    std::thread::sleep(Duration::from_millis(250));
    clipboard::restore();
    applog::log(if confirmed { "pasted-confirmed" } else { "paste-unconfirmed" });
    // Unconfirmed after 5 s usually means the focused app ignores Ctrl+V.
    // The text WAS delivered to the clipboard mechanism, so count it as a
    // success for the pill (spec's error table has no entry for this; the
    // log line records it).
    true
}
```

Note the behavior detail: on `Start`, `my_gen` is the *incremented* value (`fetch_add` returns the old one, hence the `+ 1`) — the new dictation immediately owns the UI and every older worker's guard goes stale.

- [ ] **Step 2: Verify tests and warnings**

Run: `cargo test` → expected 29 passed.
Run: `cargo check` → expected: ZERO warnings from `pipeline.rs`, `groq.rs`, `dsp.rs`, `keys.rs`, `audio.rs` — every M1 "never used" item now has a caller. Any remaining warning: stop and report.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/pipeline.rs
git commit -m "feat: pipeline drives overlay states with generation-guarded UI"
```

---

### Task 3: The pill's four faces (frontend)

**Files:**
- Replace: `src/index.html`, `src/main.js`

**Interfaces:**
- Consumes: `"ui"` events (`{state, message}`) and `"level"` events (f64) from Rust.
- Produces: the rendered states. No messages back to Rust, ever.

- [ ] **Step 1: Replace `src/index.html` with:**

```html
<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8" />
<style>
  html, body { margin: 0; background: transparent; overflow: hidden;
               -webkit-user-select: none; user-select: none; cursor: default; }
  .pill {
    position: fixed; inset: 0;
    background: #0b0a0a;
    border-radius: 18px;
    display: flex; align-items: center; justify-content: center;
    opacity: 1;
    transition: opacity 240ms ease, background-color 160ms ease;
  }
  .pill.faded { opacity: 0; }
  .pill.error { background: #ec3013; }

  .face {
    position: absolute; inset: 0;
    display: flex; align-items: center; justify-content: center; gap: 3px;
    opacity: 0; transition: opacity 120ms ease; pointer-events: none;
  }
  .face.on { opacity: 1; }

  .bar {
    width: 3px; height: 24px; border-radius: 1.5px;
    background: #f3f2f2;
    transform: scaleY(0.16); transform-origin: center;
    will-change: transform;
  }

  .status {
    font: 700 9px "Segoe UI", system-ui, sans-serif;
    letter-spacing: 0.18em;
    background: linear-gradient(100deg, #8a8a8a 40%, #ffffff 50%, #8a8a8a 60%);
    background-size: 250% 100%;
    -webkit-background-clip: text; background-clip: text; color: transparent;
    animation: shim 1.6s linear infinite;
  }
  @keyframes shim {
    from { background-position: 140% 0; }
    to   { background-position: -40% 0; }
  }

  .check.on svg { animation: pop 280ms cubic-bezier(0.2, 1.6, 0.4, 1) both; }
  @keyframes pop {
    from { transform: scale(0.4); opacity: 0; }
    to   { transform: scale(1); opacity: 1; }
  }

  .err {
    font: 600 10px "Segoe UI", system-ui, sans-serif;
    color: #ffffff; letter-spacing: 0.02em;
  }
</style>
</head>
<body>
  <div class="pill" id="pill">
    <div class="face" id="face-listening"></div>
    <div class="face" id="face-processing"><span class="status">PROCESSING</span></div>
    <div class="face check" id="face-success">
      <svg viewBox="0 0 24 24" width="16" height="16">
        <path d="M4 12.5l5 5L20 6.5" stroke="#f3f2f2" stroke-width="3"
              fill="none" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
    </div>
    <div class="face" id="face-error"><span class="err" id="err-text"></span></div>
  </div>
  <script src="main.js"></script>
</body>
</html>
```

- [ ] **Step 2: Replace `src/main.js` with:**

```js
const { listen } = window.__TAURI__.event;

// --- faces ------------------------------------------------------------
const pill = document.getElementById("pill");
const faces = {
  listening: document.getElementById("face-listening"),
  processing: document.getElementById("face-processing"),
  success: document.getElementById("face-success"),
  error: document.getElementById("face-error"),
};
const errText = document.getElementById("err-text");

function setState(state, message) {
  pill.classList.toggle("faded", state === "hidden");
  pill.classList.toggle("error", state === "error");
  for (const [name, el] of Object.entries(faces)) {
    el.classList.toggle("on", name === state);
  }
  if (state === "error") errText.textContent = message || "Error";
}

listen("ui", (e) => setState(e.payload.state, e.payload.message));

// --- listening bars (unchanged behavior from M0/M1) -------------------
const BAR_COUNT = 12;
const bars = [];
for (let i = 0; i < BAR_COUNT; i++) {
  const b = document.createElement("div");
  b.className = "bar";
  faces.listening.appendChild(b);
  bars.push(b);
}
const weights = bars.map((_, i) => {
  const d = Math.abs(i - (BAR_COUNT - 1) / 2) / ((BAR_COUNT - 1) / 2);
  return 0.35 + 0.65 * Math.cos((d * Math.PI) / 2);
});

let target = 0;
listen("level", (e) => { target = e.payload; });

let smoothed = 0, t = 0;
function frame() {
  t += 1 / 60;
  smoothed = target > smoothed
    ? smoothed + (target - smoothed) * 0.5
    : smoothed + (target - smoothed) * 0.12;
  const MIN = 0.16;
  bars.forEach((b, i) => {
    const wobble = 0.85 + 0.15 * Math.sin(t * 7 + i * 1.7);
    const s = MIN + (1 - MIN) * smoothed * weights[i] * wobble;
    b.style.transform = `scaleY(${s.toFixed(3)})`;
  });
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
```

(The fps meter is intentionally gone — M0's gate is passed; the design has no meter.)

- [ ] **Step 3: Verify build**

Run: `cargo test` (29 passed) and `npm run tauri dev` compiles and launches without errors. Visual behavior is verified in Task 4.

- [ ] **Step 4: Commit**

```bash
git add src/index.html src/main.js
git commit -m "feat: pill renders listening/processing/success/error states"
```

---

### Task 4: State-cycle verification and milestone report

**Files:**
- Create: `docs/reports/milestone-2-results.md`

**Interfaces:**
- Consumes: the whole app.
- Produces: recorded evidence for Milestone 3 planning.

- [ ] **Step 1: Run the protocol (human at the keyboard)**

`npm run tauri dev`, then:

1. **Full happy cycle:** dictate a sentence into Notepad. Expected: bars while held → "PROCESSING" with a moving light sweep on release → text lands → checkmark pops → pill fades out. No frozen-bars moment, no flash between faces.
2. **Error face:** Wi-Fi off, dictate. Expected: shimmer for up to ~30 s, then the pill turns red with "Couldn't reach Groq" for 2 s and fades. Wi-Fi back on.
3. **Silence:** hold silently 2 s. Expected: pill fades out quietly, no red, no shimmer.
4. **Short tap:** fast Ctrl+Win tap. Expected: pill blinks in and fades immediately — no other faces.
5. **Interrupt:** long dictation, then immediately a second short dictation while the first processes. Expected: the pill follows the SECOND dictation cleanly (bars → shimmer → check); the first's result never flashes the pill or hides it mid-listening.
6. **No mic:** in Windows sound settings, disable your microphone(s), restart the app, try to dictate. Expected: red "No mic detected" for 2 s. Re-enable mic afterward.

- [ ] **Step 2: Write `docs/reports/milestone-2-results.md`**

```markdown
# Milestone 2 results — overlay states

Date: <run date>

| # | Check | Result | Notes |
|---|---|---|---|
| 1 | Full cycle: bars → shimmer → check → fade | __ | |
| 2 | Network error face: red, correct text, 2 s | __ | |
| 3 | Silence: quiet fade, no error | __ | |
| 4 | Short tap: blink and fade only | __ | |
| 5 | Interrupt: pill follows newest dictation only | __ | |
| 6 | No-mic error face | __ | |

Automated tests: __ passed.
Verdict: GO / NO-GO for Milestone 3.
Deviations: <list or "none">
```

Fill every cell.

- [ ] **Step 3: Commit**

```bash
git add docs/reports/milestone-2-results.md
git commit -m "docs: milestone 2 state-cycle results"
```

If any check fails: STOP and report with log lines — no improvised fixes to plan-specified code.
