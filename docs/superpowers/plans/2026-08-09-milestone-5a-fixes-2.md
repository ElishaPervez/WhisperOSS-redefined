# Milestone 5a — Fix: the settings window doesn't notice a microphone coming back

**Status:** Task 3 verification passed except for one thing. Confirmed working: normal dictation, a genuinely silent take still fading away with no error, "Check your mic" when a fallback device hears nothing, the settings note naming the device actually recording, and a refusing device no longer trapping the app.

**Defect.** The microphone warning beside the picker is written once, when the settings window opens. If the window is already open when the microphone situation changes — a device re-enabled, a stream recovered, a switch completing — the note keeps showing the old state until the user closes and reopens the window or triggers another dictation.

**Root cause.** `src/settings.js` only re-reads its values on the `settings-shown` event, which fires when the window is shown. Nothing tells an already-open window that the audio engine's state changed underneath it. The engine knows the moment it happens — it just never says so.

**Fix.** The audio engine announces a change; the settings window listens and refreshes just the microphone row. Announcements are limited to real transitions — device lost, device recovered, device switched — so the two-second retry loop cannot spam an event every two seconds while a device stays broken.

## Global Constraints

- Windows-only. Repo root: `C:\projects (code)\15. WhisperOSS redefined` (quote paths). `cargo` runs from `src-tauri\`. Never touch `src-reference\`.
- All **43** tests stay green. Zero compiler warnings. This is event wiring with no new pure logic, so it adds no tests.
- Change only what is written below.
- Do not pause between tasks. Stop only for the human step in Task 3.

---

### Task 1: The audio engine announces changes

**Files:**
- Modify: `src-tauri/src/audio.rs`

- [ ] **Step 1: Give the engine a handle to speak through.** Add a field to `pub struct AudioEngine`:

```rust
    /// Kept so the engine can tell the settings window when the microphone
    /// situation changes while that window is already open.
    app: tauri::AppHandle,
```

- [ ] **Step 2: Fill it in.** In `AudioEngine::start`, the `app` parameter is currently moved into the level-emitter thread. Clone it for the struct instead. Add this as the first line of the function body:

```rust
        let app_for_engine = app.clone();
```

and add this initialiser inside the `Arc::new(AudioEngine { ... })` block:

```rust
            app: app_for_engine,
```

- [ ] **Step 3: Add the announcement.** Inside `impl AudioEngine`, next to `active_device`:

```rust
    /// Only called on real transitions — lost, recovered, switched — so an
    /// already-open settings window refreshes without the two-second retry
    /// loop firing an event every two seconds while a device stays broken.
    fn announce_change(&self) {
        let _ = self.app.emit("mic-changed", ());
    }
```

- [ ] **Step 4: Announce at the three transitions.**

In `fn open`, add `engine.announce_change();` as the **last** statement before each of the three returns — after `Some(stream)`'s healthy store and log, and on both failure paths. Concretely, each `Some(stream)` / `None` return in that function is preceded by the announcement.

In `fn reopen_quietly`, add `engine.announce_change();` **only** in the success branch, directly after `applog::log("audio-stream-recovered");`. Do not announce on its failure paths — that function runs every two seconds while a device stays broken, and the loss was already announced by the error callback below.

In `build_stream`'s error callback closure, add the announcement after the healthy store, so it reads:

```rust
    let e_err = engine.clone();
    let err_fn = move |err| {
        applog::log(&format!("audio-callback-error {err}"));
        e_err.healthy.store(false, Ordering::SeqCst);
        e_err.announce_change();
    };
```

- [ ] **Step 5: Verify**

Run: `cargo test` → **43 passed**. `cargo check` → zero warnings.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/audio.rs
git commit -m "feat: the audio engine announces microphone changes"
```

---

### Task 1b: The startup open stays silent

**Status note (2026-08-09):** Task 1 (`aa722bb`) verification of Task 2 found a
startup race. The engine's very first `open` runs during app setup, before the
application has finished registering its managed state. Its announcement
reached the already-loaded settings page, which called `get_settings` and was
rejected — one red console error at every launch.

The first open at boot is **not a transition** — nothing was lost, recovered,
or switched, and the settings window re-reads everything on `settings-shown`
anyway. So the initial open must not announce. Every later `open` call (device
switches) and every other announcement site (recovery, error callback) runs
long after startup, when the app is fully ready.

**Files:**
- Modify: `src-tauri/src/audio.rs`

- [ ] **Step 1: Give `open` an announce flag.** Change its signature to:

```rust
fn open(
    engine: &Arc<AudioEngine>,
    preferred: &Option<String>,
    announce: bool,
) -> Option<cpal::Stream> {
```

and wrap each of the three `engine.announce_change();` calls inside it as:

```rust
                if announce {
                    engine.announce_change();
                }
```

(matching each site's existing indentation).

- [ ] **Step 2: Update the two call sites** in the stream thread inside
`AudioEngine::start`:

The initial open, before the loop — this is startup, stay silent:

```rust
            let mut stream = open(&e, &wanted, false);
```

The device-switch open, in the `Ok(next)` branch — a real transition, announce:

```rust
                        stream = open(&e, &wanted, true);
```

`reopen_quietly`, the error callback, and everything else stay exactly as
Task 1 left them.

- [ ] **Step 3: Verify**

Run: `cargo test` → **43 passed**. `cargo check` → zero warnings.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/audio.rs
git commit -m "fix: the startup stream open does not announce before the app is ready"
```

---

### Task 2: The settings window listens

**Files:**
- Modify: `src/settings.js`

The microphone row is refreshed on its own rather than re-running the whole window, so a change arriving while the user is typing in the API key box cannot disturb what they are doing.

- [ ] **Step 1: Extract the microphone refresh.** Add this function above `load()`:

```js
async function refreshMic() {
  const cfg = await invoke("get_settings");
  await loadMics(cfg.input_device);
  const mic = await invoke("microphone_status");
  const note = el("mic-note");
  if (!mic.healthy) {
    note.textContent = "· no microphone available";
  } else if (cfg.input_device && mic.active && mic.active !== cfg.input_device) {
    note.textContent = `· unavailable, using ${mic.active}`;
  } else {
    note.textContent = "";
  }
}
```

- [ ] **Step 2: Use it in `load()`.** Replace these lines in `load()`:

```js
  await loadMics(cfg.input_device);
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

with:

```js
  await refreshMic();
```

- [ ] **Step 3: Listen for changes.** Add this next to the existing `listen("settings-shown", ...)` handler:

```js
// The engine can change microphone underneath an already-open window.
listen("mic-changed", () => refreshMic());
```

- [ ] **Step 4: Verify**

Run `npm run tauri dev`, open settings from the tray, and confirm the microphone row still shows no note with a working device selected, and that the dropdown still lists devices correctly. Right-click → Inspect → Console must be free of red errors.

- [ ] **Step 5: Commit**

```bash
git add src/settings.js
git commit -m "fix: settings updates the microphone row while the window is open"
```

---

### Task 3: Rebuild, re-verify, and finish the milestone

- [ ] **Step 1: Build the installer.**

```powershell
npm run tauri build
```

Report the filename and size.

- [ ] **Step 2: Report this protocol and STOP.** The human runs it. Ask for `%APPDATA%\WhisperOSS\log.txt` to be deleted first so the run is clean.

The point of every check below is that **the settings window stays open the whole time**. Nothing may be closed, reopened, or dictated into to force a refresh.

1. **Install the new build**, launch it, and open settings from the tray. Leave it open for all of the following.
2. **Losing a device updates live.** With your USB microphone selected, disable it in Windows (Sound → Recording → the device → Disable). Watch the settings window without touching it. Within a couple of seconds the Microphone row must show **"· unavailable, using \<other device\>"**.
3. **Getting it back updates live.** Re-enable the device in Windows. Select it again in the dropdown if it does not reselect itself. The note must clear **on its own**, with no reopening and no dictating.
4. **Switching updates live.** Change the dropdown to another working device. The row stays clean and the status bar reads "Microphone updated".
5. **Nothing regressed.** Dictate normally — text pastes. Record two seconds of silence on a working device — the pill fades with no error. Disable the selected device and dictate — the red pill reads "Check your mic".
6. **The log is not noisy.** Paste `%APPDATA%\WhisperOSS\log.txt`. There must be no long runs of repeated audio errors from the period when the device was disabled.

- [ ] **Step 3: Write `docs/reports/milestone-5a-results.md`** in the same shape as `docs/reports/milestone-4b-results.md`: one row per check with PASS/FAIL and what was observed, the test count (43), the DEVIATIONS list, and a GO / NO-GO verdict for Milestone 5b.

Record all of it: mid-session recovery measured at 7 ms; the three defects human testing found across the two rounds (a deaf fallback device fading silently, a refusing device retried forever, and a settings window that did not notice changes while open); and their fix commits.

- [ ] **Step 4: Commit**

```bash
git add docs/reports/milestone-5a-results.md
git commit -m "docs: milestone 5a results"
```
