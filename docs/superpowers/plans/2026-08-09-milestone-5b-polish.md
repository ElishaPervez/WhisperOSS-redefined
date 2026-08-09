# Milestone 5b — Final polish: glass, sizing, speed, and a clipboard that keeps everything

## Context for a fresh session

WhisperOSS is a Windows dictation app: hold Ctrl+Win anywhere, speak, release — the words are typed where the cursor is. Tauri 2 + Rust, vanilla JS frontend (no bundler), Windows-only. Transcription via Groq; the API key lives in Windows Credential Manager.

- Repo root: `C:\projects (code)\15. WhisperOSS redefined` (quote paths in every command). `cargo` runs from `src-tauri\`. `npm run tauri build` / `npm run tauri dev` from the root.
- `src-reference\` is the old Python app, kept for reference. **Never touch it.**
- The backend map: `lib.rs` (setup, tray, window helpers), `pipeline.rs` (the dictation loop and overlay choreography — Rust owns all timing, wording, and now sizing; the webview only renders), `audio.rs` (always-on mic stream with pre-roll, auto-recovery, auto-reclaim), `clipboard.rs` (privacy paste via delayed rendering — `WM_RENDERFORMAT` from the OS is the paste confirmation), `hook.rs`/`hotkey_logic.rs` (low-level keyboard hook + hold tracker), `position.rs` (pill placement math, pure), `overlay_state.rs` (overlay state contract), `commands.rs` (settings IPC), `groq.rs`, `dsp.rs`, `config.rs`, `keys.rs`, `applog.rs` (timestamped log at `%APPDATA%\WhisperOSS\log.txt`).
- The frontend: `src\index.html` + `src\main.js` (the overlay pill), `src\settings.html/js`, `src\firstrun.html/js`, `src\design-tokens.css`.
- Current state: Milestone 5a shipped reliability (auto-recovery measured at 7 ms, auto-reclaim at 11 ms). **44 tests pass, zero compiler warnings.** Installer builds via NSIS at ~2.5 MiB.

**Execution rules.** This plan is the source of truth: its code, commands, and Expected values are decisions already made — change only what is written here. You have freedom over shell dialect and sandbox workarounds, none over design. Keep a DEVIATIONS list and report it. Do not pause between tasks; report after each commit and continue. Stop only for the human step in Task 5, a failed verification, or a mismatch that isn't mechanical. Purely mechanical adjustments (an import path, a type cast noted below) may be made and listed as deviations without stopping.

## What this milestone does

Four improvements and a final measured verification:

1. **The clipboard survives a dictation with everything intact.** Today only plain text is put back after a paste — if the user had a screenshot, copied files, or rich text on the clipboard, dictating destroys it. The snapshot/restore now carries every restorable format.
2. **The error pill sizes itself to its message** instead of being fixed at 120 px, so wording is no longer constrained to ~20 characters.
3. **The pill becomes frosted glass** (Windows acrylic) with system-rounded corners.
4. **Startup and latency get instrumented** so the spec's performance targets (cold start < 1.5 s, hold → visible bars < 100 ms) are measured from the log, not guessed.

## Global Constraints

- Test count goes from 44 to **46** (Tasks 1 and 2 add one each). Zero compiler warnings.
- All timing, wording, and sizing decisions live in Rust; the webview renders.

---

### Task 1: The clipboard keeps images, files, and rich text

**Files:**
- Modify: `src-tauri/src/clipboard.rs`
- Modify: `src-tauri/src/pipeline.rs`

Today `snapshot_text()` reads only plain text, and the module header documents the limitation. The replacement snapshots every format whose bytes can be copied and puts them all back on restore. Formats Windows synthesizes on demand (ANSI text from Unicode text, a bitmap handle from DIB pixels) are skipped and regenerate themselves; formats that are GDI handles rather than bytes cannot be carried and are skipped.

- [ ] **Step 1: Write the failing test.** Add to the existing `mod tests` in `src-tauri/src/clipboard.rs`:

```rust
    #[test]
    fn snapshot_format_filter() {
        // kept: the four standard formats whose bytes can be copied
        assert!(should_snapshot(13)); // Unicode text
        assert!(should_snapshot(8));  // DIB image (screenshots)
        assert!(should_snapshot(17)); // DIBv5 image
        assert!(should_snapshot(15)); // copied files (HDROP)
        // kept: everything app-registered (HTML, RTF, drop effects)
        assert!(should_snapshot(0xC000));
        assert!(should_snapshot(0xC123));
        // dropped: synthesized or handle-based formats
        assert!(!should_snapshot(1));      // ANSI text — synthesized from Unicode
        assert!(!should_snapshot(2));      // bitmap — a GDI handle, not bytes
        assert!(!should_snapshot(3));      // metafile
        assert!(!should_snapshot(14));     // enhanced metafile
        assert!(!should_snapshot(16));     // locale — synthesized
        assert!(!should_snapshot(0x0083)); // owner-display range
    }
```

Run `cargo test snapshot_format_filter` and watch it fail to compile — `should_snapshot` does not exist.

- [ ] **Step 2: The snapshot type and filter.** In `src-tauri/src/clipboard.rs`, below the `to_utf16z` function:

```rust
const CF_DIB: u32 = 8;
const CF_HDROP: u32 = 15;
const CF_DIBV5: u32 = 17;
/// A clipboard bigger than this is not cloned into memory; text survives,
/// the rest is let go. Stops a copied video from doubling its RAM.
const MAX_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;

/// Everything on the user's clipboard that can be put back after our paste.
pub struct Snapshot {
    /// (format id, raw bytes) — only formats whose data lives in an HGLOBAL.
    entries: Vec<(u32, Vec<u8>)>,
}

/// Formats worth carrying across a paste. The four standard ones are byte
/// buffers; 0xC000 and up are app-registered names (HTML, RTF, drop effects)
/// and are byte buffers by convention. Everything else is either synthesized
/// by Windows from a kept format or is a handle that cannot be copied.
fn should_snapshot(format: u32) -> bool {
    matches!(format, CF_UNICODETEXT | CF_DIB | CF_HDROP | CF_DIBV5) || format >= 0xC000
}
```

- [ ] **Step 3: Generalise writing.** Replace `set_unicode_text` with a bytes-based writer plus a thin text wrapper:

```rust
/// Copy bytes into an HGLOBAL and put it on the (already open) clipboard.
unsafe fn write_format(format: u32, bytes: &[u8]) -> bool {
    let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) else { return false };
    let ptr = GlobalLock(hmem);
    if ptr.is_null() {
        return false;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
    let _ = GlobalUnlock(hmem);
    SetClipboardData(format, Some(HANDLE(hmem.0))).is_ok()
}

/// Copy text into an HGLOBAL and put it on the (already open) clipboard.
unsafe fn set_unicode_text(text: &[u16]) -> bool {
    let bytes = std::slice::from_raw_parts(text.as_ptr() as *const u8, text.len() * 2);
    write_format(CF_UNICODETEXT, bytes)
}
```

- [ ] **Step 4: Replace the text snapshot with the full one.** Delete `snapshot_text` and add:

```rust
/// Bytes of one HGLOBAL-backed clipboard format. The clipboard must be open.
unsafe fn read_format(format: u32) -> Option<Vec<u8>> {
    let handle = GetClipboardData(format).ok()?;
    let hglobal = HGLOBAL(handle.0);
    let size = GlobalSize(hglobal);
    if size == 0 {
        return None;
    }
    let ptr = GlobalLock(hglobal) as *const u8;
    if ptr.is_null() {
        return None;
    }
    let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
    let _ = GlobalUnlock(hglobal);
    Some(bytes)
}

/// Copy every restorable format off the clipboard. None means there is
/// nothing we can put back. An oversized clipboard keeps only its text.
pub fn snapshot() -> Option<Snapshot> {
    unsafe {
        let hwnd = HWND(OWNER_HWND.load(Ordering::SeqCst) as *mut _);
        if !open_clipboard_retrying(hwnd) {
            return None;
        }
        let mut entries: Vec<(u32, Vec<u8>)> = Vec::new();
        let mut total = 0usize;
        let mut oversized = false;
        let mut format = EnumClipboardFormats(0);
        while format != 0 {
            if should_snapshot(format) {
                if let Some(bytes) = read_format(format) {
                    total += bytes.len();
                    if total > MAX_SNAPSHOT_BYTES {
                        oversized = true;
                    } else {
                        entries.push((format, bytes));
                    }
                }
            }
            format = EnumClipboardFormats(format);
        }
        let _ = CloseClipboard();
        if oversized {
            applog::log("clipboard-snapshot-oversized-keeping-text-only");
            entries.retain(|(f, _)| *f == CF_UNICODETEXT);
        }
        if entries.is_empty() {
            None
        } else {
            Some(Snapshot { entries })
        }
    }
}
```

Add `EnumClipboardFormats` to the `DataExchange` import list and `GlobalSize` to the `Memory` import list.

- [ ] **Step 5: Restore all of it.** Change the static and the two places that use it:

```rust
static RESTORE_TO: Mutex<Option<Snapshot>> = Mutex::new(None);
```

`stage` becomes:

```rust
/// Stage `text` for a privacy paste. Returns false if the privacy formats
/// could not be set — the caller MUST abort the paste in that case.
pub fn stage(text: &str, restore_to: Option<Snapshot>) -> bool {
    *PENDING.lock().unwrap() = Some(to_utf16z(text));
    *RESTORE_TO.lock().unwrap() = restore_to;
```

(the rest of `stage` is unchanged). In the `WM_RESTORE` arm, replace the restore block:

```rust
            if open_clipboard_retrying(hwnd) {
                let _ = EmptyClipboard();
                if let Some(snap) = RESTORE_TO.lock().unwrap().take() {
                    for (format, bytes) in &snap.entries {
                        let _ = write_format(*format, bytes);
                    }
                }
                let _ = CloseClipboard();
                applog::log("clipboard-restored");
            }
```

Update the module header's "M1 limitation" paragraph to say the snapshot now carries every HGLOBAL-backed format (text, images, copied files, rich text) and that oversized clipboards fall back to text only.

- [ ] **Step 6: The caller.** In `src-tauri/src/pipeline.rs`, `fn paste`, replace:

```rust
    let previous = clipboard::snapshot_text();
    if previous.is_none() {
        applog::log("clipboard-snapshot-empty-or-nontext");
    }
```

with:

```rust
    let previous = clipboard::snapshot();
    if previous.is_none() {
        applog::log("clipboard-snapshot-empty");
    }
```

- [ ] **Step 7: Verify.** `cargo test` → **45 passed**. `cargo check` → zero warnings.

- [ ] **Step 8: Commit.**

```bash
git add src-tauri/src/clipboard.rs src-tauri/src/pipeline.rs
git commit -m "feat: the clipboard keeps images, files, and rich text across a dictation"
```

---

### Task 2: The error pill sizes itself to its message

**Files:**
- Modify: `src-tauri/src/position.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/pipeline.rs`
- Modify: `src/index.html`

Rust owns sizing (it owns the wording, so it must own the width). The states with no text keep the resting 120 px; an error grows to fit.

- [ ] **Step 1: Write the failing test.** Add to `mod tests` in `src-tauri/src/position.rs`:

```rust
    #[test]
    fn error_width_grows_with_message() {
        // no text → the resting width
        assert_eq!(pill_width_for(""), PILL_LOGICAL_W);
        // a real message → wide enough, growing per character
        assert_eq!(pill_width_for("Invalid API key"), 44.0 + 15.0 * 6.5);
        // absurd length → capped
        assert_eq!(pill_width_for(&"x".repeat(80)), PILL_MAX_LOGICAL_W);
    }
```

Run `cargo test error_width_grows` — compile failure, `pill_width_for` does not exist.

- [ ] **Step 2: The math.** In `src-tauri/src/position.rs`, next to the existing constants:

```rust
pub const PILL_MAX_LOGICAL_W: f64 = 340.0;

/// Logical pill width for an error message. 6.5 px per character is generous
/// for the 10 px error font, so text is never clipped; the excess is
/// symmetric and reads as padding. Textless states use the resting width.
pub fn pill_width_for(message: &str) -> f64 {
    (44.0 + message.chars().count() as f64 * 6.5).clamp(PILL_LOGICAL_W, PILL_MAX_LOGICAL_W)
}
```

And give `pill_position` a width parameter:

```rust
pub fn pill_position(work_area: Rect, scale: f64, logical_w: f64) -> (i32, i32) {
    let pw = (logical_w * scale).round() as i32;
```

(only that one line changes inside; `PILL_LOGICAL_W` is no longer read there). Update the three existing position tests to pass `PILL_LOGICAL_W` as the third argument — their expected values do not change.

- [ ] **Step 3: Thread the width through.** In `src-tauri/src/lib.rs`, `position_overlay` takes the width and passes it on:

```rust
pub(crate) fn position_overlay(app: &tauri::AppHandle, logical_w: f64) -> tauri::Result<()> {
```

```rust
    let (x, y) = position::pill_position(work_area, monitor.scale_factor(), logical_w);
```

- [ ] **Step 4: Size on show.** In `src-tauri/src/pipeline.rs`, add `position` to the `use crate::{...}` list, and change `Ui::show` and `Ui::show_error`:

```rust
    fn show(&self, my_gen: u64, logical_w: f64) {
        if !self.current(my_gen) {
            return;
        }
        if let Some(w) = self.app.get_webview_window("overlay") {
            let _ = w.set_size(tauri::LogicalSize::new(logical_w, position::PILL_LOGICAL_H));
            let _ = crate::position_overlay(&self.app, logical_w);
            let _ = w.show();
        }
    }
```

```rust
    /// Error state per design: red pill sized to its message, 2 s, fade.
    /// Blocking — call off-thread.
    fn show_error(&self, my_gen: u64, message: &str) {
        self.show(my_gen, position::pill_width_for(message));
        self.emit(my_gen, "error", message);
        std::thread::sleep(Duration::from_millis(overlay_state::ERROR_HOLD_MS));
        self.fade_out_and_hide(my_gen);
    }
```

The one other `show` call site (the `Start` arm) becomes:

```rust
                    ui.show(my_gen, position::PILL_LOGICAL_W);
```

Every show now sets the size, so a wide error pill left hidden cannot leak its width into the next listening pill.

- [ ] **Step 5: Never wrap.** In `src/index.html`, add `white-space: nowrap;` to the `.err` rule.

- [ ] **Step 6: Verify.** `cargo test` → **46 passed**. `cargo check` → zero warnings.

- [ ] **Step 7: Commit.**

```bash
git add src-tauri/src/position.rs src-tauri/src/lib.rs src-tauri/src/pipeline.rs src/index.html
git commit -m "feat: the error pill sizes itself to its message"
```

---

### Task 3: Frosted glass

**Files:**
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/index.html`

The pill becomes Windows acrylic: the desktop behind it shows through, blurred. Two facts drive the implementation:

- The acrylic backdrop covers the whole window rectangle — CSS corners cannot clip it, so square blurred corners would poke out past an 18 px CSS radius. Windows itself can clip the window (backdrop included) to rounded, antialiased corners, but at the system radius (~8 px). So **the pill's radius changes from 18 to 8** to line up with the system clip. This is a deliberate design trade: real glass at 8 px beats fake glass at 18 px. The human verdict in Task 5 decides whether it stays.
- The tint must live in CSS, not in the effect, because the error state changes color and the effect's tint cannot change at runtime.

- [ ] **Step 1: The effect.** In `src-tauri/tauri.conf.json`, add to the **overlay** window object (after `"visible": false`):

```json
        "windowEffects": { "effects": ["acrylic"] }
```

- [ ] **Step 2: The corner clip.** In `src-tauri/Cargo.toml`, add `"Win32_Graphics_Dwm"` to the `windows` crate feature list. In `src-tauri/src/lib.rs`, inside `setup`, extend the existing overlay block:

```rust
            // Overlay: hidden until a dictation starts, never clickable in M1.
            if let Some(w) = app.get_webview_window("overlay") {
                let _ = w.set_ignore_cursor_events(true);
                // Windows clips the window — acrylic backdrop included — to
                // rounded, antialiased corners. Without this the blur would
                // poke out past the CSS radius as square corners.
                if let Ok(handle) = w.hwnd() {
                    use windows::Win32::Graphics::Dwm::{
                        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
                    };
                    let pref = DWMWCP_ROUND;
                    unsafe {
                        let _ = DwmSetWindowAttribute(
                            windows::Win32::Foundation::HWND(handle.0),
                            DWMWA_WINDOW_CORNER_PREFERENCE,
                            &pref as *const _ as *const core::ffi::c_void,
                            std::mem::size_of_val(&pref) as u32,
                        );
                    }
                }
            }
```

(If `handle.0`'s type does not match our `windows` crate's `HWND` field directly, bridge with `as _` — a mechanical cast, list it as a deviation, don't stop.)

- [ ] **Step 3: The glass face.** In `src/index.html`, change the `.pill` and `.pill.error` rules:

```css
  .pill {
    position: fixed; inset: 0;
    background: rgba(11, 10, 10, 0.72);
    border-radius: 8px;
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.06);
    display: flex; align-items: center; justify-content: center;
    opacity: 1;
    transition: opacity 240ms ease, background-color 160ms ease;
  }
  .pill.faded { opacity: 0; }
  .pill.error { background: rgba(236, 48, 19, 0.85); }
```

(Everything else in the stylesheet stays.)

- [ ] **Step 4: Verify.** `cargo test` → **46 passed**. `cargo check` → zero warnings. Then `npm run tauri dev` and hold Ctrl+Win over a colorful window: the pill must show a blurred version of what is behind it, with rounded corners and no square blur bleeding past them. If the effect fails to apply at all (solid or fully transparent pill), STOP and report what you see.

- [ ] **Step 5: Commit.**

```bash
git add src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/src/lib.rs src/index.html
git commit -m "feat: frosted-glass pill with system-rounded corners"
```

---

### Task 4: Measure what the spec promises

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src/main.js`

Two log lines make the spec's performance targets readable straight from `%APPDATA%\WhisperOSS\log.txt`: `app-start → app-ready` for startup, `recording-start → overlay-listening-visible` for hold-to-bars latency.

- [ ] **Step 1: Startup.** In `src-tauri/src/lib.rs`, add as the last line of the `setup` closure body, directly before `Ok(())`:

```rust
            applog::log("app-ready");
```

- [ ] **Step 2: The paint report.** In `src-tauri/src/commands.rs`, append:

```rust
/// The overlay reports the moment the listening bars are actually painted,
/// so hold-to-visible latency can be read straight from the log.
#[tauri::command]
pub fn overlay_visible() {
    applog::log("overlay-listening-visible");
}
```

Register it in `src-tauri/src/lib.rs`'s `generate_handler![...]`:

```rust
            commands::overlay_visible,
```

- [ ] **Step 3: Report after paint, not on message arrival.** In `src/main.js`, add under the existing destructuring at the top:

```js
const { invoke } = window.__TAURI__.core;
```

and add to the end of `setState`:

```js
  if (state === "listening") {
    // After the next paint, so the log line means "bars are on screen".
    requestAnimationFrame(() => invoke("overlay_visible"));
  }
```

- [ ] **Step 4: Verify.** `cargo test` → **46 passed**. `cargo check` → zero warnings. `npm run tauri dev`, dictate once, and confirm the log now shows `app-ready` after startup and `overlay-listening-visible` after `recording-start`.

- [ ] **Step 5: Commit.**

```bash
git add src-tauri/src/lib.rs src-tauri/src/commands.rs src/main.js
git commit -m "feat: instrument startup and hold-to-visible latency"
```

---

### Task 5: Build, the full pass, and the milestone report

- [ ] **Step 1: Build the installer.**

```powershell
npm run tauri build
```

Report the filename and size.

- [ ] **Step 2: Report this protocol and STOP.** The human runs it. Ask for `%APPDATA%\WhisperOSS\log.txt` to be deleted first.

1. **Install the new build** and launch it.
2. **Speed, felt.** Immediately after launching, hold Ctrl+Win and speak — it must just work, no warm-up.
3. **The glass.** Dictate over a colorful window (a busy webpage works). The pill shows a blur of what is behind it; the corners are round with no square blur poking past them. Then disable the selected microphone and dictate: the red pill is glass too. **Verdict question: does this look better than the old solid pill?** The radius is smaller than before (8 px vs 18 px) — that trade is yours to accept or reject; rejecting reverts one commit.
4. **The wide pill.** Turn off Wi-Fi and dictate. The pill must read "Couldn't reach Groq" in full — wider than the normal pill, text uncut, still centered on screen. Turn Wi-Fi back on.
5. **The clipboard keeps a screenshot.** Win+Shift+S, snip anything. Dictate a sentence into Notepad. Then Ctrl+V into Paint — the snip must still paste.
6. **The clipboard keeps files.** Copy two files in Explorer. Dictate. Ctrl+V into another folder — both files must paste.
7. **The clipboard keeps rich text.** Copy a formatted paragraph from a webpage. Dictate. Paste into WordPad — formatting preserved. (Skip any of 5–7 you can't set up, and say so.)
8. **Privacy still holds.** Open Win+V — the dictated sentences must NOT appear in clipboard history.
9. **Regression sweep.** Normal dictation pastes; two seconds of silence fades with no error; AI formatting punctuates; disabling then re-enabling the selected mic auto-recovers and auto-returns (settings row updates live).
10. **Paste the log.** The numbers come from it: `app-start → app-ready` must be under 1500 ms, and `recording-start → overlay-listening-visible` under 100 ms.

- [ ] **Step 3: If the human rejects the glass** (step 3 verdict): `git revert` the Task 3 commit, rebuild, and have the human re-check step 3's dictation once. Record the decision in the report. Otherwise skip this step.

- [ ] **Step 4: Update the spec.** In `docs/superpowers/specs/2026-08-08-whispeross-v2-design.md`, update the overlay section to record the shipped visuals per the human verdict (acrylic + 8 px system corners + content-sized error pill, or solid pill if reverted), and the clipboard section to record that snapshot/restore now carries all HGLOBAL-backed formats with a 16 MB cap.

- [ ] **Step 5: Write `docs/reports/milestone-5b-results.md`** in the same shape as `docs/reports/milestone-5a-results.md`: one row per check with PASS/FAIL and what was observed; the measured numbers against the spec targets (cold start < 1.5 s, hold → bars < 100 ms) plus the release → paste round-trip time visible in the log; the test count (46); the DEVIATIONS list; the glass verdict; and a GO / NO-GO verdict for release.

- [ ] **Step 6: Commit.**

```bash
git add docs/superpowers/specs/2026-08-08-whispeross-v2-design.md docs/reports/milestone-5b-results.md
git commit -m "docs: milestone 5b results and spec alignment"
```
