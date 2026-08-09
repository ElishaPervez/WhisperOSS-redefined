# Milestone 3c (revised) — Remove Hotkey Rebind, Keep the Microphone Picker

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

## Context for a fresh session

WhisperOSS is a Windows dictation app: hold **Ctrl+Win**, speak, release, and the transcript is pasted where your cursor is. It is a Tauri 2 + Rust rewrite of an older Python app (kept in `src-reference\`, which is git-ignored and **must never be touched**). The frontend is vanilla HTML/CSS/JS with no bundler.

Milestone 3c added two things: a **microphone picker** and a **hotkey rebind**. The microphone picker works and was verified by a human. The hotkey rebind never worked — clicking "Change hotkey" started a capture that was cancelled within a fraction of a second by the settings window's own focus-loss handler, so no key was ever recorded. Two rounds of diagnostics are written up in `docs/superpowers/plans/2026-08-09-milestone-3c-hotkey-diagnostic.md` and `-diagnostic-2.md`.

**The product owner has decided to drop the rebind feature entirely rather than continue debugging it.** The hotkey is now fixed at Ctrl+Win.

**Goal of this plan:** remove every trace of the rebind — the capture state machine, the keyboard-hook capture mode, the capture commands, the shared capture flags, the round-1 diagnostic logging, and the settings-window UI — while leaving the microphone picker and everything else working. Then update the spec and write the milestone report.

This is a deletion plan. If a step tells you to delete something and you find it already absent, note it in DEVIATIONS and move on.

## Global Constraints

- Windows-only. Repo root: `C:\projects (code)\15. WhisperOSS redefined` (quote paths in shell commands — the folder name contains spaces and parentheses). `cargo` runs from `src-tauri\`.
- **Never touch `src-reference\`.**
- Shell: commands below are written for PowerShell. Adapt freely if your shell differs — that is a HOW decision; report it in DEVIATIONS.
- The test count goes from **52 down to 42** (10 capture tests are deleted with the code they test). Zero compiler warnings when finished.
- The plan's code blocks are the source of truth. Do not add improvements, do not rename things, do not "while I'm here" refactor.
- Do not pause between tasks. Post a short report after each commit and continue. Stop only for: the human-only step (Task 7), a failed verification, or a mismatch that is not mechanical.
- Keep a running DEVIATIONS list and include it in your final report.

---

### Task 1: Delete the capture state machine

**Files:**
- Modify: `src-tauri/src/hotkey_logic.rs`

- [ ] **Step 1: Delete the capture code.** Remove everything that was added above `pub struct HoldTracker` — that is, the whole run from the comment `/// How long the app will listen for a new combo before giving up.` down to and including the closing brace of `impl CaptureBuffer`. Specifically these items go: `CAPTURE_TIMEOUT_MS`, `VK_ESCAPE`, `rank`, `canonical`, `name_of`, `combo_names`, `enum Capture`, `struct CaptureBuffer`, `impl CaptureBuffer`.

Everything else in the file stays: `Key`, `KeyEvent`, `Action`, `key_from_vk`, `key_from_name`, `parse_combo`, `combo_other_vk`, `HoldTracker`.

- [ ] **Step 2: Delete their tests.** In the `mod tests` block at the bottom, delete the helper functions `down` and `up`, and these ten tests:

```
canonical_order_is_press_order_independent
combo_names_maps_every_supported_key
combo_names_rejects_keys_with_no_config_name
combo_names_round_trips_through_parse_combo
capture_completes_on_first_release
capture_is_press_order_independent
capture_ignores_key_repeat
escape_cancels_capture
stray_single_tap_keeps_listening
unusable_combos_are_rejected
```

Keep the eight original tests (`parse_combo_accepts_and_rejects_per_rules`, `key_names_map_to_expected_keys`, `vk_variants_collapse_to_one_key`, `combo_other_vk_extraction`, `default_combo_full_cycle`, `short_tap_cancels`, `key_repeat_does_not_double_start`, `unrelated_keys_do_not_disturb_hold`, `three_key_combo_requires_all_three`).

- [ ] **Step 3: Correct the module doc comment.** The header currently claims the combo is user-rebindable. Replace the first two sentences of the `//!` block with:

```rust
//! Combo-aware hold-to-dictate logic. The combo is fixed at Ctrl+Win and read
//! from config at startup; there is no rebind UI. Rules: at least two keys, at
//! least one modifier, at most one non-modifier — enforced by parse_combo so an
//! invalid config can never produce a broken tracker.
```

- [ ] **Step 4: Verify**

Run: `cargo check`. Expected: errors in `hook.rs`, `pipeline.rs`, and `commands.rs` referencing the deleted items. That is correct at this stage — Tasks 2–4 remove those callers. Do not fix them here.

- [ ] **Step 5: Commit** — skip. This does not compile alone; commit after Task 4.

---

### Task 2: Remove capture mode from the keyboard hook

**Files:**
- Modify: `src-tauri/src/hook.rs`

- [ ] **Step 1: Delete the capture flag, the diagnostic flag, and their accessors.** Remove:

- the `static CAPTURE: AtomicBool = AtomicBool::new(false);` line and the comment above it
- the `static DIAG: AtomicBool = AtomicBool::new(false);` line and the comment above it
- the whole `pub fn set_capture` function and its doc comment
- the whole `pub fn is_capturing` function and its doc comment
- the whole `pub fn set_diag` function

- [ ] **Step 2: Delete the swallow block from `hook_proc`.** Remove these ten lines entirely:

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

The next block down — the one that swallows the combo's regular key while its modifiers are held (`SUPPRESS_VK` / `REQUIRED_MODS`) — **stays**. That is the mechanism that stops a combo like Ctrl+Space typing into the focused app, and it is still needed.

- [ ] **Step 3: Fix the imports.** Change the atomics import back to:

```rust
use std::sync::atomic::{AtomicU32, Ordering};
```

- [ ] **Step 4: Correct the module doc comment.** The header's second sentence promises a capture mode. Replace the whole `//!` block with:

```rust
//! Global low-level keyboard hook. Forwards every key transition (as
//! logical Keys) to the pipeline. If the active combo contains one regular
//! key (e.g. Space in Ctrl+Space), that key is SWALLOWED while all the
//! combo's modifier keys are held — otherwise holding the combo would type
//! into the focused app. Modifier keys are never swallowed.
//! PRIVACY: key events are never logged — only forwarded in memory.
```

- [ ] **Step 5: Commit** — skip. Continue to Task 3.

---

### Task 3: Remove capture routing from the pipeline

**Files:**
- Modify: `src-tauri/src/pipeline.rs`

- [ ] **Step 1: Delete the two capture helpers.** Remove the whole `fn emit_hotkey` (with its doc comment) and the whole `fn end_capture`. Keep `combo_from_config` and `apply_combo`.

- [ ] **Step 2: Simplify the event loop.** Inside `pub fn start`, replace the spawned thread's opening — from `std::thread::spawn(move || {` down to the line `match tracker.on_event(ev) {` — with exactly this:

```rust
    std::thread::spawn(move || {
        let mut tracker = hotkey_logic::HoldTracker::new(combo);

        for ev in rx {
            match tracker.on_event(ev) {
```

That deletes: the `capture` and `was_capturing` locals, the `pipeline-capture-flag` diagnostic log, the whole `if state.capturing.load(...) { ... continue; }` branch, and the `if was_capturing { ... }` tracker rebuild. Everything from `hotkey_logic::Action::None => {}` onward is unchanged.

- [ ] **Step 3: Check for orphaned imports.** `serde_json` was only used by `emit_hotkey`. `tauri::Emitter` is still needed by `Ui::emit` — keep it. Let `cargo check` tell you; remove only what it flags.

- [ ] **Step 4: Commit** — skip. Continue to Task 4.

---

### Task 4: Remove the capture commands and shared flags

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Delete the commands.** In `src-tauri/src/commands.rs`, remove the whole `begin_hotkey_capture` function (including its doc comment and the watchdog thread inside it), the whole `cancel_hotkey_capture` function, and the whole `report_blur` function. The `hook::set_diag(...)` calls live inside these functions and go with them.

Keep everything else, including `list_microphones` and `set_microphone`.

- [ ] **Step 2: Fix the imports.** Replace the import block at the top of `commands.rs` with:

```rust
use tauri::State;

use crate::{applog, audio, autostart, config, groq, keys, state::AppState};
```

(`std::sync::atomic::Ordering`, `std::time::Duration`, `tauri::Emitter`, `tauri::Manager`, `hook`, and `hotkey_logic` were all only used by the deleted commands. `save_api_key` already spells out `std::time::Duration::from_secs(15)` in full, so it needs no import.)

- [ ] **Step 3: Shrink the shared state.** In `src-tauri/src/state.rs`, delete the `capturing` and `capture_gen` fields from `struct AppState` (with their doc comments) and their initialisers in `AppState::new`. Change the atomics import to:

```rust
use std::sync::atomic::AtomicU64;
```

Keep `config`, `key`, `audio`, and `generation`.

- [ ] **Step 4: Unregister the commands.** In `src-tauri/src/lib.rs`, delete these lines from `tauri::generate_handler![...]`:

```rust
            commands::begin_hotkey_capture,
            commands::cancel_hotkey_capture,
            commands::report_blur,
```

- [ ] **Step 5: Verify**

Run: `cargo test` → expected **42 passed**.
Run: `cargo check` → zero warnings.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/hotkey_logic.rs src-tauri/src/hook.rs src-tauri/src/pipeline.rs src-tauri/src/commands.rs src-tauri/src/state.rs src-tauri/src/lib.rs
git commit -m "refactor: remove hotkey rebind, fix the shortcut at Ctrl+Win"
```

---

### Task 5: Remove the rebind UI from the settings window

**Files:**
- Modify: `src/settings.html`
- Modify: `src/settings.js`

- [ ] **Step 1: Replace the hero block** in `src/settings.html`. The current hero ends with a "Change hotkey" button and a hint span. Replace the whole `<div class="hero"> … </div>` with:

```html
  <div class="hero">
    <div class="kicker">GLOBAL HOTKEY</div>
    <div class="combo">
      <span>Hold</span>
      <span class="combo-keys" id="combo-keys"></span>
      <span>— speak — release</span>
    </div>
    <div class="hero-note">Works in any app — the text lands wherever your cursor is.</div>
  </div>
```

- [ ] **Step 2: Update the styles.** In the `<style>` block, delete these rules:

```css
  .change { margin-top: 20px; font-size: 12px; padding: 7px 13px; }
```
```css
  .hint { font-size: 12px; color: var(--muted); margin-left: 12px; }
  .hint.ok { color: #3fb970; }
  .hint.err { color: var(--accent); }
  body.capturing .key { border-color: var(--accent); }
```

and add, next to the other `.hero` rules:

```css
  .hero-note { margin-top: 20px; font-size: 12px; color: var(--muted); }
```

Keep `.combo-keys`, `.key`, `.plus`, `.btn`, `.btn-primary`, and `.icon-btn` — they are still used by the key row and the chips.

- [ ] **Step 3: Strip the rebind wiring** from `src/settings.js`:

- Delete the line `const { listen } = window.__TAURI__.event;`
- Delete the `let currentHotkey = ["ctrl", "win"];` and `let capturing = false;` declarations
- Delete the whole `setHint` function and the whole `setCapturing` function
- Delete the whole `el("change-hotkey").onclick = …` handler
- Delete the whole `listen("hotkey", …)` handler
- Delete the whole `window.addEventListener("blur", …)` handler

- [ ] **Step 4: Simplify the chip renderer.** Replace `renderCombo` with:

```js
function renderCombo(names) {
  const box = el("combo-keys");
  box.innerHTML = "";
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
```

And in `load()`, replace these two lines:

```js
  currentHotkey = cfg.hotkey;
  renderCombo(currentHotkey, false);
```

with:

```js
  renderCombo(cfg.hotkey);
```

(`KEY_LABELS` and `keyLabel` stay — the chips still come from config, so the window always shows the shortcut the app is actually listening for.)

- [ ] **Step 5: Verify**

Run: `npm run tauri dev`. Open settings from the tray. The hero reads "Hold Ctrl + Win — speak — release" with the caption underneath and no button. Right-click the window → Inspect → Console must be free of red errors.

- [ ] **Step 6: Commit**

```bash
git add src/settings.html src/settings.js
git commit -m "feat: settings shows the fixed Ctrl+Win shortcut, rebind UI removed"
```

---

### Task 6: Update the spec

**Files:**
- Modify: `docs/superpowers/specs/2026-08-08-whispeross-v2-design.md`

The spec still promises a rebindable hotkey. Bring it in line with reality — the spec is what later milestones are built against, so a stale promise here becomes a bug later.

- [ ] **Step 1:** In the scope list (around line 27), replace the bullet `- Configurable hotkey (default Ctrl+Win), rebindable from settings.` with:

```markdown
- Fixed hotkey: **Ctrl+Win**. Read from config at startup and validated, but there
  is no rebind UI — the capture-based rebind was built in Milestone 3c and removed
  after it proved unreliable (it was cancelled by the settings window's own focus
  handling before any key could be recorded).
```

- [ ] **Step 2:** In the settings-surface description (around lines 66–74), remove the *Change hotkey* button from the header/hero sentence and delete the list item that reads `*(Change hotkey counts as the seventh control, in the hero)*`. The hero is now display-only. Adjust the surrounding wording so the control count is correct — six controls: API key, AI formatting, casual mode, microphone, theme, start-with-Windows.

- [ ] **Step 3:** In the testing section (around line 187), change `hotkey state machine (hold/abort/rebind cases as pure logic)` to `hotkey state machine (hold/abort cases as pure logic)`.

- [ ] **Step 4:** In the milestone list (around line 208), remove `hotkey rebind` from the Milestone 3 description.

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/specs/2026-08-08-whispeross-v2-design.md
git commit -m "docs: spec records the hotkey as fixed, rebind dropped"
```

---

### Task 7: Human verification + milestone report

**Files:**
- Create: `docs/reports/milestone-3c-results.md`

- [ ] **Step 1: Report this protocol and STOP.** The human runs it; wait for PASS/FAIL before writing the report.

Delete the log first so the run is clean:

```powershell
Remove-Item "$env:APPDATA\WhisperOSS\log.txt" -ErrorAction SilentlyContinue
```

Then `npm run tauri dev` and check:

1. **Settings looks right.** Open from the tray. The hero shows "Hold Ctrl + Win — speak — release" with the caption below it and no "Change hotkey" button. Nothing looks misaligned where the button used to be.
2. **Dictation still works.** Hold Ctrl+Win over a text field, speak, release. Text is pasted.
3. **Nothing swallows keys.** Type normally in another app, including Space, Esc, Ctrl+F, Ctrl+Shift. Everything behaves exactly as it did before the app was running.
4. **Microphone picker still works.** Change the device in the dropdown, then dictate. Text still arrives.
5. **Microphone choice survives a restart.** Quit from the tray, relaunch, reopen settings — the same device is still selected and dictation works.
6. **Other settings still work.** Toggle AI formatting on, dictate, confirm the text comes back punctuated. Toggle Start-with-Windows off and on and confirm with:

```powershell
reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v WhisperOSS
```

7. **The log is clean.** Paste the contents of `%APPDATA%\WhisperOSS\log.txt`. There must be no `hotkey-capture-*`, `hook-swallow-*`, `hook-key`, `pipeline-capture-*`, or `settings-window-blurred` lines — those all belong to the removed feature.

- [ ] **Step 2: Write `docs/reports/milestone-3c-results.md`.** Same shape as `docs/reports/milestone-3a-results.md`: one row per check with PASS/FAIL and what was observed. Record that the microphone picker passed its original four checks, that the hotkey rebind was built and then removed by product decision after two diagnostic rounds (cite the two diagnostic plan files), the final test count (42), the DEVIATIONS list, and a GO / NO-GO verdict for Milestone 4.

- [ ] **Step 3: Commit**

```bash
git add docs/reports/milestone-3c-results.md
git commit -m "docs: milestone 3c results - mic picker shipped, rebind removed"
```
