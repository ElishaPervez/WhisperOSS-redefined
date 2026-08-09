# Milestone 5a — Reliability & Recovery

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

## Context for a fresh session

WhisperOSS is a Windows dictation app: hold **Ctrl+Win**, speak, release, and the transcript is pasted where your cursor is. Tauri 2 + Rust, vanilla HTML/CSS/JS frontend, no bundler. The old Python version lives in `src-reference\`, which is git-ignored and **must never be touched**.

Milestones 0–4 are complete and human-verified. The app installs, dictates, has a settings window, a first-run flow, its own icon and installer. 42 tests pass with zero warnings.

This milestone closes the gaps in **spec §6, the error table** — the cases where the app currently fails silently or fails permanently.

**What is already correct and must not be re-implemented:**
- Exactly one retry on network and server failures, and no retry on a rejected key — built and tested in `src-tauri/src/groq.rs`.
- Aborting the paste if the privacy clipboard flags cannot be set.
- Silent recordings being discarded without an error.

**The three real gaps:**

1. **A rejected API key is a dead end.** The pill says "Invalid API key" for two seconds and disappears. Spec §6 says it should also open the settings window at the key field. Right now the user is told what is wrong but given nowhere to fix it.
2. **A missing microphone is permanent.** The audio stream is opened once at startup. If no microphone exists then — or the device is unplugged, or Windows refuses to open it — the app never tries again. Every dictation shows "No mic detected" until the app is restarted. Spec §6 requires falling back to the Windows default when a device appears.
3. **A microphone that will not open says nothing.** This was seen in a real log during Milestone 3c: the user selected a device, Windows refused it with an unsupported-format error, and the settings window carried on displaying that device as if it were in use. Dictation then silently did nothing.

## Global Constraints

- Windows-only. Repo root: `C:\projects (code)\15. WhisperOSS redefined` (quote paths in shell commands — the folder name has spaces and parentheses). `cargo` runs from `src-tauri\`.
- **Never touch `src-reference\`.**
- Shell: commands are written for PowerShell. Adapt freely if your shell differs — that is a HOW decision; report it in DEVIATIONS.
- All **42** tests must stay green. Zero compiler warnings.
- The plan's code, commands, and "Expected" values are the source of truth. No unrequested changes.
- Do not pause between tasks. Post a short report after each commit and continue. Stop only for: the human-only step (Task 5), a failed verification, or a mismatch that is not mechanical.
- Keep a running DEVIATIONS list.

---

### Task 1: A rejected key opens settings at the key field

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/pipeline.rs`
- Modify: `src/settings.js`

The settings window already re-reads its values when it opens, driven by a `settings-shown` event. That event gains a payload saying whether the key field should be focused, so there is only ever one signal and no race between two of them.

- [ ] **Step 1: Split the show helper.** In `src-tauri/src/lib.rs`, replace the whole `show_settings` function with:

```rust
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
```

- [ ] **Step 2: Call it on a rejected key.** In `src-tauri/src/pipeline.rs`, replace the final `Err(e)` arm of the `match client.transcribe(wav)` block with:

```rust
                            Err(e) => {
                                let (message, detail) = overlay_state::describe_error(&e);
                                applog::log(&format!("transcribe-error {message} {detail}"));
                                if matches!(e, groq::GroqError::Unauthorized) {
                                    crate::show_settings_at_key(&app);
                                }
                                ui.show_error(my_gen, message);
                            }
```

(Opening settings before `show_error` matters: `show_error` sleeps for the pill's two-second hold, so doing it afterwards would delay the window by two seconds.)

- [ ] **Step 3: Handle the payload.** In `src/settings.js`, replace the `listen("settings-shown", ...)` line with:

```js
// The window is hidden rather than destroyed when closed, so the webview is
// only ever loaded once. Re-read on every open.
listen("settings-shown", async ({ payload }) => {
  await load();
  if (payload) {
    const input = el("api-key");
    input.value = "";
    input.type = "password";
    input.focus();
    setKeyFeedback("Groq rejected this key — paste a new one", "err");
  }
});
```

- [ ] **Step 4: Verify**

Run: `cargo test` → **42 passed**. `cargo check` → zero warnings.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/pipeline.rs src/settings.js
git commit -m "feat: a rejected key opens settings at the key field"
```

---

### Task 2: The microphone recovers by itself

**Files:**
- Modify: `src-tauri/src/audio.rs`

The audio stream lives on one dedicated thread, which currently blocks forever waiting for a device-change request. It gains a two-second timeout so that whenever there is no working stream it quietly tries again — which covers starting with no microphone at all, unplugging one, and Windows refusing a device that later becomes free.

- [ ] **Step 1: Mark the stream unhealthy when it fails mid-use.** In `src-tauri/src/audio.rs`, inside `build_stream`, replace the error-callback line:

```rust
    let err_fn = |err| applog::log(&format!("audio-callback-error {err}"));
```

with:

```rust
    // A device that dies mid-stream must stop counting as healthy, or the
    // retry loop below will never notice it needs to reopen.
    let e_err = engine.clone();
    let err_fn = move |err| {
        applog::log(&format!("audio-callback-error {err}"));
        e_err.healthy.store(false, Ordering::SeqCst);
    };
```

- [ ] **Step 2: Add the quiet retry path.** Add this function directly below `fn open`:

```rust
/// The retry path, run every two seconds while there is no working stream.
/// It stays silent while it keeps failing — otherwise a machine with no
/// microphone would write a log line every two seconds forever — and writes
/// exactly one line when a device finally appears.
fn reopen_quietly(engine: &Arc<AudioEngine>, preferred: &Option<String>) -> Option<cpal::Stream> {
    engine.healthy.store(false, Ordering::SeqCst);
    let stream = match build_stream(engine, preferred) {
        Ok(s) => s,
        Err(_) => {
            *engine.active_device.lock().unwrap() = None;
            return None;
        }
    };
    if stream.play().is_ok() {
        engine.healthy.store(true, Ordering::SeqCst);
        applog::log("audio-stream-recovered");
        Some(stream)
    } else {
        *engine.active_device.lock().unwrap() = None;
        None
    }
}
```

(`active_device` is added in Task 3. Write this now; the file will not compile until Task 3 adds the field. That is expected — Tasks 2 and 3 commit together.)

- [ ] **Step 3: Give the stream thread its timeout.** Replace the whole stream thread body — the `std::thread::spawn(move || { ... })` block that currently contains `let mut stream = open(&e, &preferred);` and the `for next in device_rx` loop — with:

```rust
        let e = engine.clone();
        std::thread::spawn(move || {
            use std::sync::mpsc::RecvTimeoutError;
            let mut wanted = preferred;
            let mut stream = open(&e, &wanted);
            loop {
                match device_rx.recv_timeout(Duration::from_secs(2)) {
                    Ok(next) => {
                        // Dropping on this thread is required: cpal streams
                        // are !Send.
                        drop(stream.take());
                        e.reset_buffers();
                        wanted = next;
                        stream = open(&e, &wanted);
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        // Nobody asked for a change. If there is no working
                        // stream — no mic at boot, one unplugged, or a device
                        // Windows refused — try again.
                        if !e.is_healthy() {
                            drop(stream.take());
                            e.reset_buffers();
                            stream = reopen_quietly(&e, &wanted);
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        });
```

- [ ] **Step 4: Commit** — skip. This does not compile until Task 3. Continue.

---

### Task 3: Settings shows when the chosen microphone is not the one in use

**Files:**
- Modify: `src-tauri/src/audio.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/settings.html`
- Modify: `src/settings.js`

The dropdown shows what the user *asked for*. It must also show what is *actually recording* when those differ, otherwise a device Windows refused looks selected and working while dictation silently produces nothing.

- [ ] **Step 1: Track the device actually in use.** In `src-tauri/src/audio.rs`, add a field to `pub struct AudioEngine`:

```rust
    /// The device the running stream actually opened — not necessarily the
    /// one the user picked, since a refused device falls back to the default.
    active_device: Mutex<Option<String>>,
```

Add its initialiser in `AudioEngine::start`'s `Arc::new(AudioEngine { ... })` block:

```rust
            active_device: Mutex::new(None),
```

Add this accessor inside `impl AudioEngine`, next to `is_healthy`:

```rust
    pub fn active_device(&self) -> Option<String> {
        self.active_device.lock().unwrap().clone()
    }
```

In `build_stream`, immediately after the `let device = pick_device(preferred).ok_or("no input device")?;` line, record the name:

```rust
    *engine.active_device.lock().unwrap() = device.name().ok();
```

And in `fn open`, clear it on both failure paths — set `*engine.active_device.lock().unwrap() = None;` immediately before each `None` return (the play-failed branch and the build-error branch).

- [ ] **Step 2: Expose it.** In `src-tauri/src/commands.rs`, append:

```rust
/// What the microphone is really doing, for the settings window. `active` is
/// the device the stream actually opened, which differs from the user's
/// choice when Windows refused that device.
#[derive(serde::Serialize)]
pub struct MicStatus {
    pub healthy: bool,
    pub active: Option<String>,
}

#[tauri::command]
pub fn microphone_status(state: State<AppState>) -> MicStatus {
    MicStatus {
        healthy: state.audio.is_healthy(),
        active: state.audio.active_device(),
    }
}
```

Register it in `src-tauri/src/lib.rs`'s `tauri::generate_handler![...]`:

```rust
            commands::microphone_status,
```

- [ ] **Step 3: Somewhere to show it.** In `src/settings.html`, change the microphone row's description line to:

```html
        <div class="desc">Input device used while dictating <span class="micnote" id="mic-note"></span></div>
```

And add this rule to the `<style>` block, next to `.mic`:

```css
  .micnote { font-size: 12px; color: var(--accent); margin-left: 4px; }
```

- [ ] **Step 4: Fill it in.** In `src/settings.js`, inside `load()`, immediately after the `await loadMics(cfg.input_device);` line, add:

```js
  const mic = await invoke("microphone_status");
  const note = el("mic-note");
  if (!mic.healthy) {
    note.textContent = "· no microphone available";
  } else if (cfg.input_device && mic.active && mic.active !== cfg.input_device) {
    note.textContent = `· unavailable, using ${mic.active}`;
  } else {
    note.textContent = "";
  }
```

- [ ] **Step 5: Verify**

Run: `cargo test` → **42 passed**. `cargo check` → zero warnings.
Run `npm run tauri dev`, open settings from the tray, and confirm the microphone row shows **no** note when the selected device is working.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/audio.rs src-tauri/src/commands.rs src-tauri/src/lib.rs src/settings.html src/settings.js
git commit -m "feat: microphone recovers by itself and settings reports what is really in use"
```

---

### Task 4: Make the spec's error table match what is built

**Files:**
- Modify: `docs/superpowers/specs/2026-08-08-whispeross-v2-design.md`

Two rows of the §6 table describe behaviour that was never built the way it is written. Bring the spec to the truth rather than leaving a promise nobody is keeping.

- [ ] **Step 1:** In the §6 error table, change the Groq server-error row's message from `"Groq error — try again"` to `"Groq error"`, and add this line directly under the table:

```markdown
The pill is 120 px wide, so messages are kept to roughly twenty characters.
"Groq error" rather than "Groq error — try again" for that reason, and because
the app has already retried once by the time the message appears — telling the
user to try again would be describing work it just did.
```

- [ ] **Step 2:** Change the "No microphone found" row's behaviour text to:

```markdown
"No mic detected"; the audio stream retries every 2 s, so a device that is plugged in, re-enabled, or freed by another app is picked up without a restart
```

- [ ] **Step 3:** Add a row to the table for the case Milestone 3c uncovered:

```markdown
| Selected mic can't be opened | falls back to the Windows default and the settings window shows "unavailable, using <device>" beside the picker |
```

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/specs/2026-08-08-whispeross-v2-design.md
git commit -m "docs: error table matches the built behaviour"
```

---

### Task 5: Human verification

- [ ] **Step 1: Build the installer, report this protocol, and STOP.**

```powershell
npm run tauri build
```

Report the installer filename and size. Then the human runs the checks below; wait for PASS/FAIL.

**Rejected key**
1. Install the new build and launch it. Open settings, and in the API key box type `gsk_notarealkey` — but **do not** click Save. Instead, close settings, then hold Ctrl+Win over a text field and speak. Nothing should change yet: the saved key is still good and dictation works. This is the control.
2. Now break it deliberately: ask the operator running this plan to overwrite the stored key with an invalid one (Credential Manager entry `groq_api_key.WhisperOSS`), or simply save `gsk_notarealkey` — the Save button will refuse it, which is expected, so use Credential Manager.
3. With the invalid key stored and the app restarted, hold Ctrl+Win and speak. Expected: the red pill reads "Invalid API key" **and the settings window opens with the cursor already in the key box**, showing "Groq rejected this key — paste a new one".
4. Paste the real key and click Save. Dictate again — it works, no restart.

**Microphone recovery**
5. With the app running and settings closed, disable your microphone in Windows (Settings → System → Sound → the input device → Don't allow / Disable). Hold Ctrl+Win and speak. Expected: the red pill reads "No mic detected".
6. Re-enable the microphone. Wait about five seconds, then hold Ctrl+Win and speak **without restarting the app**. Expected: it records and pastes normally. Check `%APPDATA%\WhisperOSS\log.txt` for a single `audio-stream-recovered` line — and confirm the log is **not** filled with repeated failure lines from the period when the mic was off.

**Microphone that will not open**
7. In settings, select a device that Windows will refuse — during Milestone 3c a virtual or stereo-mix input did this. Close and reopen settings from the tray. Expected: beside "Microphone" the row reads "· unavailable, using <the device actually in use>". If every device on the machine opens fine, say so and skip.
8. Select a working device again. Reopen settings — the note is gone.

**Regressions**
9. Dictation still works, AI formatting still works, and the first-run flow is untouched (only reachable with no saved key, so no need to re-test it here).

- [ ] **Step 2: Write `docs/reports/milestone-5a-results.md`** in the same shape as `docs/reports/milestone-4b-results.md`: one row per check with PASS/FAIL and what was observed, the test count, the DEVIATIONS list, and a GO / NO-GO verdict for Milestone 5b.

- [ ] **Step 3: Commit**

```bash
git add docs/reports/milestone-5a-results.md
git commit -m "docs: milestone 5a results"
```

---

## What comes next (not this plan)

**Milestone 5b — Polish & performance.** The frosted-glass effect on the pill; the pill sizing itself to its message instead of being fixed at 120 px; measuring against the two targets set before any code was written (ready to use within 1.5 s of launch, bars moving within 100 ms of the keypress); keeping more than plain text on the clipboard through a dictation; and a final end-to-end manual pass.
