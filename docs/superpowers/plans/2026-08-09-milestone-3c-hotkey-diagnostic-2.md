# Milestone 3c — Hotkey Rebind Diagnostic, Round 2

**What round 1 measured.** Full log at `%APPDATA%\WhisperOSS\log.txt`, 16 rebind attempts, single process (log `app-start` timestamp matches the only running `scaffold-tmp.exe`).

- `hotkey-capture-begin hook_flag=true` every time — the swallow flag reaches the keyboard hook module.
- **Zero `hook-swallow-down` / `hook-swallow-up` lines.** The hook never swallowed a key.
- **Zero `pipeline-capture-flag=true` and zero `pipeline-capture-event` lines.** The dictation loop never received a key event while a rebind was in progress.
- Every attempt ended `hotkey-capture-cancelled reason=blur` (11×) or `reason=button` (5×), between 0.37 s and 4.5 s after it began. Never a timeout, never invalid, never rebound.
- No `hotkey-capture-cancel-ignored` lines, so the flag was genuinely still set at each cancel.
- `recording-start` at 14:14:29 in the same log: the hook and the dictation loop are alive and working outside of rebind windows.
- Human-observed during a "rebind": Ctrl+F opened the WebView find bar, Ctrl+Esc opened the Start menu, Ctrl+Win still started a dictation, Space re-activated the still-focused "Change hotkey" button.

**Hypothesis under test (single):** the settings window loses focus shortly after "Change hotkey" is clicked, the window's blur handler calls `cancel_hotkey_capture`, and the rebind is over before any key is pressed. The likely focus disturbance is `begin_hotkey_capture` hiding the overlay window.

**Minimal test:** stop blur from cancelling — make it log only. Change one behaviour, nothing else. Then record what the hook sees for every key during the rebind window so the timeline is unambiguous.

**Safety:** the 6 s watchdog stays the guarantee that keys are never swallowed for long. Escape and first-key-release still end a rebind through the normal path. Do NOT raise the timeout.

## Global Constraints

- Windows-only. Repo root: `C:\projects (code)\15. WhisperOSS redefined` (quote paths). `cargo` runs from `src-tauri\`. Never touch `src-reference\`.
- All 52 tests stay green. Zero new compiler warnings.
- **Change nothing except what is written below.** No fixes, no cleanups, no reordering.
- Never log which key was pressed — event kind and flag values only.

---

### Task 1: Remove the blur cancel and record the hook's view of every key

**Files:**
- Modify: `src-tauri/src/hook.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src/settings.js`

- [ ] **Step 1: Bounded per-key logging in the hook.** In `src-tauri/src/hook.rs`, add beside `CAPTURE`:

```rust
// Diagnostic window: while true, every key transition is logged with the
// swallow flag's value. Bounded so the hook stays fast enough that Windows
// does not drop it.
static DIAG: AtomicBool = AtomicBool::new(false);
```

Add beside `set_capture`:

```rust
pub fn set_diag(on: bool) {
    DIAG.store(on, Ordering::SeqCst);
}
```

In `hook_proc`, replace the whole capture block:

```rust
            if CAPTURE.load(Ordering::SeqCst) {
                // Diagnostic only — no key identity, per the privacy rule.
                crate::applog::log(if down { "hook-swallow-down" } else { "hook-swallow-up" });
                return LRESULT(1);
            }
```

with:

```rust
            let capture_on = CAPTURE.load(Ordering::SeqCst);
            if DIAG.load(Ordering::SeqCst) {
                // Diagnostic only — no key identity, per the privacy rule.
                crate::applog::log(&format!(
                    "hook-key down={down} capture={capture_on}"
                ));
            }
            if capture_on {
                return LRESULT(1);
            }
```

- [ ] **Step 2: Open and close the diagnostic window.** In `src-tauri/src/commands.rs`, inside `begin_hotkey_capture`, add `hook::set_diag(true);` immediately after the existing `hook::set_capture(true);` line.

In the same function's watchdog thread, add `hook::set_diag(false);` as the **first** statement inside the `if` body that already calls `hook::set_capture(false)`.

Then, so the diagnostic window always closes even when the rebind ends normally, add `hook::set_diag(false);` immediately after **each** remaining `hook::set_capture(false);` call — one in `cancel_hotkey_capture` (`src-tauri/src/commands.rs`) and one in `end_capture` (`src-tauri/src/pipeline.rs`).

- [ ] **Step 3: Make blur log instead of cancel.** In `src-tauri/src/commands.rs`, add:

```rust
/// Diagnostic: records that the settings window lost focus during a rebind,
/// without ending it. Round 1 showed blur was cancelling every attempt.
#[tauri::command]
pub fn report_blur(state: State<AppState>) {
    applog::log(&format!(
        "settings-window-blurred capturing={}",
        state.capturing.load(Ordering::SeqCst)
    ));
}
```

Register it in `src-tauri/src/lib.rs`:

```rust
            commands::report_blur,
```

In `src/settings.js`, replace the blur listener with:

```js
// DIAGNOSTIC: blur no longer cancels — it only reports. The 6 s watchdog is
// the safety net for this run.
window.addEventListener("blur", () => {
  if (capturing) invoke("report_blur");
});
```

- [ ] **Step 4: Verify it builds**

Run: `cargo test` → **52 passed**. `cargo check` → zero warnings.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/hook.rs src-tauri/src/commands.rs src-tauri/src/pipeline.rs src-tauri/src/lib.rs src/settings.js
git commit -m "chore: round 2 instrumentation - blur reports instead of cancelling"
```

---

### Task 2: One measured run

- [ ] **Step 1: Report this protocol and STOP.**

Delete the log first:

```powershell
Remove-Item "$env:APPDATA\WhisperOSS\log.txt" -ErrorAction SilentlyContinue
```

`npm run tauri dev`, open settings from the tray, then — no dictation:

1. Click "Change hotkey".
2. Hold **Ctrl + Shift** together for about two seconds, then release both. Note what the chips show while holding, and what the hint says after releasing.
3. Wait five seconds.
4. Whatever the state is, click "Change hotkey" and hold **Ctrl + Alt** for two seconds, then release.
5. Wait eight seconds without touching anything.
6. Confirm typing works normally in any app.
7. Quit from the tray.

Paste the whole log.

- [ ] **Step 2: Do not fix anything from this plan.** Hand the log back.

**How the log will be read:**

| What appears | What it means |
|---|---|
| `hook-key down=true capture=true` while holding | keys reach the hook and the rebind is live — the earlier failure really was blur cancelling early |
| `hook-key ... capture=false` while holding | the swallow flag is being cleared by something not yet identified |
| no `hook-key` lines at all while holding | the hook is not being called during rebinds — the fault is in the hook installation, not the flag |
| `settings-window-blurred capturing=true` | the window really is losing focus mid-rebind; the cause of the focus move is the next thing to find |
| `hotkey-rebound` | removing the blur cancel fixed it outright — hypothesis confirmed |
