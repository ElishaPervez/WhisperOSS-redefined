# Milestone 4b — Fixes: hand-off after first run, and stale settings

**Status:** Task 6 human verification found two defects. Everything else passed: the welcome card opens by itself, Archivo renders, step navigation works, a bad key is refused inline, the Groq link opens a browser, a good key is accepted, and dictation works immediately afterwards with no restart.

**Defect 1 — the app appears to vanish after setup.** When the key is accepted, `src/firstrun.js` calls `win.hide()` and nothing replaces the window. The app is running in the tray, but a first-time user has no way to know that: the window they were using disappears and no other surface appears. It should hand off to the settings window.

**Defect 2 — the settings window shows stale values.** `src/settings.js` calls `load()` exactly once, when the webview first loads. That happens at app startup while the window is still hidden. Showing the window later does not re-run it, so anything that changed in between is not reflected. The human hit this as a missing green "Saved" marker beside the API key — the key really was saved, but the window was still drawing the state it read at startup. The same staleness applies to the microphone list (a device plugged in after startup never appears) and to any config change made outside the window.

Both are gaps in the Milestone 4b plan, not execution errors.

## Global Constraints

- Windows-only. Repo root: `C:\projects (code)\15. WhisperOSS redefined` (quote paths). `cargo` runs from `src-tauri\`. Never touch `src-reference\`.
- All **42** tests stay green. Zero compiler warnings.
- Change only what is written below.
- Do not pause between tasks. Stop only for the human step in Task 3.

---

### Task 1: Hand off from first run to settings

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/firstrun.js`

- [ ] **Step 1: Add the command.** In `src-tauri/src/commands.rs`, append:

```rust
/// The key was accepted: close the welcome card and hand the user to the
/// settings window. Without this the window simply disappears and a
/// first-time user cannot tell the app is still running.
#[tauri::command]
pub fn finish_first_run(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("firstrun") {
        let _ = w.hide();
    }
    crate::show_settings(&app);
    applog::log("first-run-complete");
}
```

`get_webview_window` needs the `Manager` trait, which was removed from this file in Milestone 3c. Change the Tauri import line to:

```rust
use tauri::{Manager, State};
```

- [ ] **Step 2: Register it.** In `src-tauri/src/lib.rs`, add to `tauri::generate_handler![...]`:

```rust
            commands::finish_first_run,
```

- [ ] **Step 3: Call it.** In `src/firstrun.js`, inside `validate()`, replace the success line `win.hide();` with:

```js
    await invoke("finish_first_run");
```

Leave the ✕ handler alone — closing without a key still just hides the window.

- [ ] **Step 4: Verify**

Run: `cargo test` → **42 passed**. `cargo check` → zero warnings.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src/firstrun.js
git commit -m "fix: first run hands off to settings instead of vanishing"
```

---

### Task 2: Refresh the settings window every time it opens

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/settings.js`

- [ ] **Step 1: Announce that the window was shown.** In `src-tauri/src/lib.rs`, change `show_settings` to:

```rust
pub(crate) fn show_settings(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
        // The webview loaded once, at startup, while this window was hidden.
        // Tell it to re-read everything so it never shows stale values.
        let _ = w.emit("settings-shown", ());
    }
}
```

Add `Emitter` to the Tauri import at the top of the file, so it reads:

```rust
use tauri::{Emitter, Manager, PhysicalPosition, WindowEvent};
```

- [ ] **Step 2: Re-read on that signal.** In `src/settings.js`, add the event import back at the top, under the existing destructuring lines:

```js
const { listen } = window.__TAURI__.event;
```

And add this immediately above the final `load();` call at the bottom of the file:

```js
// The window is hidden rather than destroyed when closed, so the webview is
// only ever loaded once. Re-read on every open.
listen("settings-shown", () => load());
```

- [ ] **Step 3: Make a repeat load correct.** `load()` currently only ever *sets* the saved-key marker, so a stale "Saved" could survive a key being removed. In `src/settings.js`, replace this block at the end of `load()`:

```js
  const hasKey = await invoke("has_api_key");
  if (hasKey) {
    el("api-key").placeholder = "••••••••••••••••";
    setKeyFeedback("Saved", "ok");
  }
```

with:

```js
  const hasKey = await invoke("has_api_key");
  if (hasKey) {
    el("api-key").placeholder = "••••••••••••••••";
    setKeyFeedback("Saved", "ok");
  } else {
    el("api-key").placeholder = "gsk_…";
    setKeyFeedback("", "");
  }
```

- [ ] **Step 4: Verify**

Run: `cargo test` → **42 passed**. `cargo check` → zero warnings.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src/settings.js
git commit -m "fix: settings re-reads its values each time the window opens"
```

---

### Task 3: Rebuild, re-verify, and finish the milestone

- [ ] **Step 1: Rebuild the installer.**

```powershell
npm run tauri build
```

Report the installer's filename and size.

- [ ] **Step 2: Report this protocol and STOP.** The human runs it.

The saved key must be removed again first so the welcome card appears — say so, and wait rather than doing it yourself.

1. **Install the new build** and launch WhisperOSS from the Start menu.
2. **Welcome card appears by itself.** Step through to page two and enter a valid Groq key.
3. **The hand-off works.** When the key is accepted, the welcome card closes **and the settings window opens** — the app does not just disappear.
4. **The saved marker is there.** The API key row reads "Saved" in green, immediately, with no restart.
5. **Dictation works** with no restart: hold Ctrl+Win over a text field, speak, release.
6. **Reopening is still correct.** Close settings with ✕, then left-click the tray icon. The window reopens and still reads "Saved".
7. **A device added later appears.** With settings closed, plug in or enable another microphone, then reopen settings from the tray. The new device is in the dropdown without restarting the app. (Skip and say so if you have no second device to add.)
8. **Regression check.** Toggle AI formatting on and dictate; the text comes back punctuated. Toggle it off again.

- [ ] **Step 3: Write `docs/reports/milestone-4b-results.md`** in the same shape as `docs/reports/milestone-4a-results.md`: one row per check with PASS/FAIL and what was observed, the bundled font size (34,928 bytes), the installer size, the test count, the DEVIATIONS list, and a GO / NO-GO verdict for Milestone 5.

Record in the report that the opener plugin registration and these two defects were plan gaps found during verification, with their fix commits.

- [ ] **Step 4: Commit**

```bash
git add docs/reports/milestone-4b-results.md
git commit -m "docs: milestone 4b results"
```
