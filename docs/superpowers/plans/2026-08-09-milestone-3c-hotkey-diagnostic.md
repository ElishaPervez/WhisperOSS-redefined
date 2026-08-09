# Milestone 3c — Hotkey Rebind Diagnostic

**Status:** Task 7 verification failed. Checks 1–4 (microphone) PASS. Check 5 (rebind) FAILS: no combo binds. Holding Ctrl+Space reverts the display to Ctrl+Win; every other combo produces no visible response at all.

**Why this is a diagnostic and not a fix:** the evidence is self-contradictory, so any fix now would be a guess.

- `%APPDATA%\WhisperOSS\log.txt` shows `hotkey-capture-begin` followed by `hotkey-capture-cancelled-by-window` on all five attempts — never `hotkey-rebound`, `hotkey-capture-invalid`, or `hotkey-capture-timeout`.
- Ctrl+Space reverting the display means Space activated the focused "Change hotkey" button, which fires the button's cancel path. That can only happen if the keyboard hook did **not** swallow Space — yet the log says capture had begun.
- `hotkey-capture-cancelled-by-window` is logged by BOTH the window-blur handler and the second button click, so the log cannot distinguish them. That is a defect in the instrumentation, not in the feature.
- Nothing logs whether key events reached the capture recorder at all, so the absence of a result proves nothing.

**Goal of this plan:** add logging at three boundaries, get one run, read the log. No behaviour changes. No fixes.

## Global Constraints

- Windows-only. Repo root: `C:\projects (code)\15. WhisperOSS redefined` (quote paths). `cargo` runs from `src-tauri\`. Never touch `src-reference\`.
- All 52 tests must stay green. Zero new compiler warnings.
- **Change nothing except what is written below.** Do not "improve" the rebind while you are in there, do not reorder existing statements, do not add a fix you think is obvious. A changed behaviour makes the measurement worthless.
- Privacy rule still holds: never log which key was pressed. Log counts and phases only.

---

### Task 1: Instrument the three boundaries

**Files:**
- Modify: `src-tauri/src/hook.rs`
- Modify: `src-tauri/src/pipeline.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src/settings.js`

- [ ] **Step 1: Prove whether the hook sees the capture flag.** In `src-tauri/src/hook.rs`, add next to `set_capture`:

```rust
/// Read back for diagnostics: proves whether the store reached this module.
pub fn is_capturing() -> bool {
    CAPTURE.load(Ordering::SeqCst)
}
```

In `hook_proc`, replace the capture block:

```rust
            if CAPTURE.load(Ordering::SeqCst) {
                return LRESULT(1);
            }
```

with:

```rust
            if CAPTURE.load(Ordering::SeqCst) {
                // Diagnostic only — no key identity, per the privacy rule.
                crate::applog::log(if down { "hook-swallow-down" } else { "hook-swallow-up" });
                return LRESULT(1);
            }
```

- [ ] **Step 2: Prove whether key events reach the capture recorder.** In `src-tauri/src/pipeline.rs`, inside the `if state.capturing.load(...)` branch, add a log immediately after the `if !was_capturing { ... }` block and before `match capture.on_event(ev)`:

```rust
                applog::log("pipeline-capture-event");
```

And in the `Capture::Pending(keys)` arm, replace the body with:

```rust
                    hotkey_logic::Capture::Pending(keys) => {
                        applog::log(&format!("hotkey-capture-pending n={}", keys.len()));
                        emit_hotkey(&app, "preview", &keys);
                    }
```

Also add, immediately after `for ev in rx {`:

```rust
            let capturing_now = state.capturing.load(Ordering::SeqCst);
            if capturing_now != was_capturing {
                applog::log(&format!("pipeline-capture-flag={capturing_now}"));
            }
```

(Leave the existing `if state.capturing.load(...)` line exactly as it is — this is an extra read for logging, not a replacement.)

- [ ] **Step 3: Tell the two cancel paths apart.** In `src-tauri/src/commands.rs`, change `cancel_hotkey_capture` to take a reason:

```rust
#[tauri::command]
pub fn cancel_hotkey_capture(app: tauri::AppHandle, state: State<AppState>, reason: String) {
    if state.capturing.swap(false, Ordering::SeqCst) {
        hook::set_capture(false);
        state.capture_gen.fetch_add(1, Ordering::SeqCst);
        applog::log(&format!("hotkey-capture-cancelled reason={reason}"));
        let _ = app.emit(
            "hotkey",
            serde_json::json!({ "phase": "cancelled", "keys": [] }),
        );
    } else {
        applog::log(&format!("hotkey-capture-cancel-ignored reason={reason}"));
    }
}
```

In `begin_hotkey_capture`, replace the line `applog::log("hotkey-capture-begin");` with:

```rust
    applog::log(&format!(
        "hotkey-capture-begin hook_flag={}",
        hook::is_capturing()
    ));
```

- [ ] **Step 4: Pass the reason from the window.** In `src/settings.js`, update the two call sites:

```js
el("change-hotkey").onclick = async () => {
  if (capturing) {
    await invoke("cancel_hotkey_capture", { reason: "button" });
    return;
  }
  setCapturing(true);
  renderCombo([], true);
  setHint("Hold the keys together, then let go. Esc cancels.", "");
  await invoke("begin_hotkey_capture");
};
```

```js
window.addEventListener("blur", () => {
  if (capturing) invoke("cancel_hotkey_capture", { reason: "blur" });
});
```

- [ ] **Step 5: Verify it builds**

Run: `cargo test` → **52 passed**. `cargo check` → zero warnings.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/hook.rs src-tauri/src/pipeline.rs src-tauri/src/commands.rs src/settings.js
git commit -m "chore: instrument hotkey capture boundaries for diagnosis"
```

---

### Task 2: One measured run

- [ ] **Step 1: Report this protocol and STOP.** The human runs it.

Delete the log first so the run is clean:

```powershell
Remove-Item "$env:APPDATA\WhisperOSS\log.txt" -ErrorAction SilentlyContinue
```

Then `npm run tauri dev`, open settings from the tray, and do exactly this — nothing else, no dictation:

1. Click "Change hotkey". Note whether the button text changes to "Listening — press your keys" and whether the chips become "…".
2. Hold **Ctrl + Shift** together for about two seconds, then release both.
3. Wait five seconds without touching anything.
4. Click "Change hotkey" again. Hold **Ctrl + Space** for about two seconds, then release.
5. Wait five seconds.
6. Quit from the tray.

Then paste the whole log:

```powershell
Get-Content "$env:APPDATA\WhisperOSS\log.txt"
```

- [ ] **Step 2: Do not propose a fix from this plan.** Hand the log back. The root cause is decided from the log, then a separate fix plan is written.

**How the log will be read:**

| What appears | What it means |
|---|---|
| `hotkey-capture-begin hook_flag=true` | the swallow flag reached the keyboard hook |
| `hotkey-capture-begin hook_flag=false` | the flag never reached the hook — the fault is there |
| `hook-swallow-down` lines while holding | keys really are being blocked system-wide |
| no `hook-swallow` lines | keys are leaking to whatever window has focus |
| `pipeline-capture-flag=true` | the dictation loop knows a rebind is in progress |
| `pipeline-capture-event` lines | key events are reaching the combo recorder |
| `hotkey-capture-pending n=1` then `n=2` | the recorder is accumulating the combo correctly |
| `reason=button` | Space (or Enter) is pressing the focused button |
| `reason=blur` | the settings window is losing focus |
