# Milestone 3b — Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Fix two defects found in Milestone 3b verification: (1) the settings window's minimize/close/drag do nothing, and (2) Casual mode produces raw text unless AI formatting is also on.

**Root causes (confirmed by reading the code):**
1. `src-tauri/capabilities/default.json` grants only `core:default` + `opener:default`. The JS calls `win.minimize()`, `win.hide()`, and uses `data-tauri-drag-region` — all **core window commands** that are permission-gated and were never granted. (App-defined commands like the toggles are NOT gated, which is why they worked.)
2. `src-tauri/src/pipeline.rs` line ~156 gates ALL formatting behind `if use_formatter`, so Casual-only falls through to the raw-text branch.

**Tech Stack:** Existing app. No new dependencies.

## Global Constraints

- Windows-only. Repo root: `C:\projects (code)\15. WhisperOSS redefined` (quote paths). `cargo` runs from `src-tauri\`. Never touch `src-reference\`.
- Existing 41 tests must stay green; Task 2 adds one. Zero new compiler warnings.

---

### Task 1: Grant the window-control permissions

**Files:**
- Modify: `src-tauri/capabilities/default.json`

**Interfaces:**
- Consumes: nothing.
- Produces: the settings window's minimize/hide/drag JS calls are permitted.

- [ ] **Step 1: Replace the `permissions` array in `src-tauri/capabilities/default.json` with:**

```json
  "permissions": [
    "core:default",
    "opener:default",
    "core:window:allow-minimize",
    "core:window:allow-hide",
    "core:window:allow-show",
    "core:window:allow-set-focus",
    "core:window:allow-start-dragging"
  ]
```

(`allow-start-dragging` powers `data-tauri-drag-region`; `allow-minimize`/`allow-hide` power the titlebar buttons; `allow-show`/`allow-set-focus` are used by the tray path and are granted for completeness.)

- [ ] **Step 2: Verify**

Run: `npm run tauri dev`. Open settings (tray left-click). Confirm ALL of:
- Dragging the top bar moves the window.
- The ✕ button hides the window (app keeps running; tray reopens it).
- The – button minimizes it.

If any still fails, open the settings window's devtools console (right-click → Inspect) and report the exact red error text — do not guess.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/capabilities/default.json
git commit -m "fix: grant window minimize/hide/drag permissions to settings window"
```

---

### Task 2: Casual mode triggers formatting on its own

**Files:**
- Modify: `src-tauri/src/pipeline.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: pure helper `pipeline::wants_formatting(use_formatter: bool, casual: bool) -> bool`, tested; the dictation worker uses it.

- [ ] **Step 1: Write the failing test — add this test module at the very bottom of `src-tauri/src/pipeline.rs`:**

```rust
#[cfg(test)]
mod tests {
    use super::wants_formatting;

    #[test]
    fn formatting_truth_table() {
        assert!(!wants_formatting(false, false)); // neither → raw
        assert!(wants_formatting(true, false));   // formal only
        assert!(wants_formatting(false, true));   // casual only → still formats
        assert!(wants_formatting(true, true));    // both → formats (casual wins in format_text)
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p scaffold-tmp wants_formatting` (or `cargo test wants_formatting`)
Expected: FAIL to compile — `wants_formatting` not found.

- [ ] **Step 3: Add the helper and use it.** Add this free function just above `fn paste(` near the bottom of `src-tauri/src/pipeline.rs`:

```rust
/// The AI rewrite runs when EITHER toggle is on. Casual is its own trigger,
/// not a sub-option of formatting; when casual is on, format_text picks the
/// casual prompt (so casual wins if both are on).
fn wants_formatting(use_formatter: bool, casual: bool) -> bool {
    use_formatter || casual
}
```

Then change the formatter condition in the dictation worker from:

```rust
                                let final_text = if use_formatter {
```

to:

```rust
                                let final_text = if wants_formatting(use_formatter, casual) {
```

(Everything else in that arm stays identical — `format_text(&text, casual)` already selects the casual prompt when `casual` is true.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test` → expected **42 passed**.
Run: `cargo check` → zero new warnings (note: `use_formatter` is still read, so no unused-variable warning).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/pipeline.rs
git commit -m "fix: casual mode triggers AI rewrite independently of formatting"
```

---

### Task 3: Re-verify the affected checks + update the report

**Files:**
- Modify: `docs/reports/milestone-3b-results.md`

- [ ] **Step 1: Re-run the human protocol for the three affected checks**

`npm run tauri dev`, then:
1. **Window controls (was check 1):** drag moves the window; – minimizes; ✕ hides to tray; tray left-click reopens. PASS/FAIL.
2. **Casual alone (was check 4):** in the settings window, turn AI formatting OFF and Casual mode ON. Dictate "hey what's up three crying emojis". Expected: lowercase text ending in 😭😭😭 (not raw, not capitalized). PASS/FAIL.
3. **Formatter alone (regression guard):** Casual OFF, AI formatting ON. Dictate a messy sentence. Expected: properly punctuated/capitalized. PASS/FAIL.
4. **Both on:** AI formatting ON and Casual ON. Dictate. Expected: casual style wins (lowercase/emoji). PASS/FAIL.
5. **Autostart both directions (re-confirm check 6):** toggle Start-with-Windows OFF → `reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v WhisperOSS` shows not-found; toggle ON → the same query shows the value present. PASS/FAIL for each direction. If ON does not produce the value, capture the tail of `%APPDATA%\WhisperOSS\log.txt` and STOP.

- [ ] **Step 2: Update `docs/reports/milestone-3b-results.md`** — change the two failed rows to their new results, add rows for the extra formatter-combination and autostart-direction checks, and set the final verdict GO / NO-GO for Milestone 3c.

- [ ] **Step 3: Commit**

```bash
git add docs/reports/milestone-3b-results.md
git commit -m "docs: milestone 3b re-verification after fixes"
```

If any check still fails: STOP and report with the devtools console error and/or the app log lines.
