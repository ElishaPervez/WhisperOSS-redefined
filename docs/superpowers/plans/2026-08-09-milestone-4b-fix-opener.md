# Milestone 4b — Fix: register the opener plugin

**Status:** Task 3 verification failed. Clicking "console.groq.com/keys" in the first-run window does nothing, and the WebView console reports `plugin opener not found`.

**Root cause (confirmed by reading the code):** `tauri-plugin-opener` is declared in `src-tauri/Cargo.toml` and `opener:default` / `opener:allow-open-url` are granted in `src-tauri/capabilities/default.json`, but the plugin is never registered on the Tauri builder — `src-tauri/src/lib.rs` has no `.plugin(...)` call at all. A permission grants access to a plugin that was never loaded, so the frontend's `window.__TAURI__.opener` namespace does not exist at runtime.

This is a gap in the Milestone 4b plan, not an execution error. Nothing else in Task 3 is wrong: step navigation and bad-key validation both verified.

## Global Constraints

- Windows-only. Repo root: `C:\projects (code)\15. WhisperOSS redefined` (quote paths). `cargo` runs from `src-tauri\`. Never touch `src-reference\`.
- All **42** tests stay green. Zero compiler warnings.
- Change only what is written below.

---

### Task 1: Register the plugin and finish Task 3

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Register it.** In `src-tauri/src/lib.rs`, insert the plugin registration immediately after `tauri::Builder::default()` and before `.invoke_handler(...)`, so the chain begins:

```rust
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
```

- [ ] **Step 2: Verify**

Run: `cargo test` → **42 passed**. `cargo check` → zero warnings.

Then re-run the Task 3 Step 2 protocol from `docs/superpowers/plans/2026-08-09-milestone-4b-first-run-typography.md`: temporarily set the `firstrun` window's `"visible"` to `true` in `src-tauri/tauri.conf.json`, run `npm run tauri dev`, and confirm all four items:

- Step 1 renders as in the design; "Get started" advances to step 2; the footer dots and "Step 2 of 2" update.
- A deliberately wrong key (`gsk_notarealkey`) shows the red border and the inline message.
- **Clicking "console.groq.com/keys" opens it in the default browser** — this is the item that failed.
- The ✕ closes the window and the app keeps running.

**Set `"visible"` back to `false` before committing.**

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/lib.rs src/firstrun.js
git commit -m "fix: register the opener plugin so the Groq link opens a browser"
```

(`src/firstrun.js` is currently untracked — Task 3's commit never happened because verification failed. It commits here with the fix.)

- [ ] **Step 4: Continue.** Resume the Milestone 4b plan at **Task 4** and run it through to Task 6 as originally written.
