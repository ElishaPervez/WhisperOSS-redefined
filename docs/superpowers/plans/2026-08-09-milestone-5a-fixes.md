# Milestone 5a — Fixes: deaf fallback, and a device that refuses forever

**Status:** Task 5 human verification found two defects. The rest passed: a rejected key shows "Invalid API key" and opens settings with the key box focused; pasting a good key works with no restart; a device Windows refuses outright produces "No mic detected"; AI formatting still works.

**What the log proves already works.** When the selected microphone was disabled mid-session:

```
audio-callback-error The requested device is no longer available...
audio-preferred-device-missing-using-default
audio-stream-recovered
```

Detection and recovery took **7 milliseconds**. That part of Milestone 5a is correct and must not be changed.

---

**Defect 1 — a silent recording on a fallback device looks like saying nothing.**

After falling back, the machine's Windows default was *NVIDIA Broadcast*, a virtual device that produces no audio unless its own app is running. So every dictation captured pure silence, which the app discards on purpose with the pill fading away. From the user's side: they spoke, and nothing happened, with no explanation.

Log evidence — four dictations, all real audio-length captures, all silently thrown away:

```
recording-finish held_ms=890  samples=66720   silent-discarded
recording-finish held_ms=1922 samples=116160  silent-discarded
recording-finish held_ms=922  samples=68640   silent-discarded
recording-finish held_ms=2188 samples=128640  silent-discarded
```

Silently discarding a genuinely silent recording is correct and stays. But when the device actually recording is **not the one the user chose**, silence almost certainly means a broken microphone rather than a quiet user, and that case deserves to be said out loud.

---

**Defect 2 — a device that refuses to open is retried forever.**

`pick_device` in `src-tauri/src/audio.rs` falls back to the Windows default only when the chosen device's **name cannot be found**. A device that exists but refuses to open — held by another app, or offering a format we can't use — is returned every time, so the two-second retry loop tries the same broken device indefinitely and the app stays deaf until the user changes it by hand.

Log evidence: after the DroidCam stream failed at 1786277027218 the app sat unusable for twenty seconds until the device was changed manually.

```
audio-stream-error A backend-specific error has occurred: 0x88890008
recording-refused-no-mic
```

Spec §6 requires falling back to the Windows default. It currently only does so for a *missing* device, not a *refusing* one.

## Global Constraints

- Windows-only. Repo root: `C:\projects (code)\15. WhisperOSS redefined` (quote paths). `cargo` runs from `src-tauri\`. Never touch `src-reference\`.
- Test count goes from 42 to **43** (Task 1 adds one). Zero compiler warnings.
- Change only what is written below.
- Do not pause between tasks. Stop only for the human step in Task 3.

---

### Task 1: Say something when a silent take came from a fallback device

**Files:**
- Modify: `src-tauri/src/pipeline.rs`

- [ ] **Step 1: Write the failing test.** Add to the existing `mod tests` at the bottom of `src-tauri/src/pipeline.rs`:

```rust
    use super::on_fallback_device;

    #[test]
    fn fallback_detection() {
        let usb = Some("USB PnP Audio Device".to_string());
        let nvidia = Some("NVIDIA Broadcast".to_string());
        // user picked a device and it is the one recording
        assert!(!on_fallback_device(&usb, &usb));
        // user picked a device and something else is recording
        assert!(on_fallback_device(&usb, &nvidia));
        // user picked a device and nothing is recording at all
        assert!(on_fallback_device(&usb, &None));
        // user picked "system default": whatever is recording is correct
        assert!(!on_fallback_device(&None, &nvidia));
        assert!(!on_fallback_device(&None, &None));
    }
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test fallback_detection`
Expected: compile failure — `on_fallback_device` does not exist.

- [ ] **Step 3: Add the helper.** In `src-tauri/src/pipeline.rs`, directly above `fn wants_formatting`:

```rust
/// True when the user picked a specific device but something else is actually
/// recording. In that state a silent take almost certainly means a broken
/// microphone rather than a quiet user, so it is worth saying out loud.
fn on_fallback_device(wanted: &Option<String>, active: &Option<String>) -> bool {
    match (wanted, active) {
        (Some(w), Some(a)) => w != a,
        (Some(_), None) => true,
        _ => false,
    }
}
```

- [ ] **Step 4: Use it.** In the `hotkey_logic::Action::Finish` arm, replace the silence block:

```rust
                    if dsp::is_effectively_silent(&samples) {
                        applog::log("silent-discarded");
                        std::thread::spawn(move || ui.fade_out_and_hide(my_gen));
                        continue;
                    }
```

with:

```rust
                    if dsp::is_effectively_silent(&samples) {
                        let wanted = state.config.lock().unwrap().input_device.clone();
                        if on_fallback_device(&wanted, &audio.active_device()) {
                            applog::log("silent-on-fallback-device");
                            std::thread::spawn(move || ui.show_error(my_gen, "Check your mic"));
                        } else {
                            applog::log("silent-discarded");
                            std::thread::spawn(move || ui.fade_out_and_hide(my_gen));
                        }
                        continue;
                    }
```

("Check your mic" is 14 characters — the pill is 120 px wide, so messages have to stay near twenty.)

- [ ] **Step 5: Verify**

Run: `cargo test` → **43 passed**. `cargo check` → zero warnings.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/pipeline.rs
git commit -m "feat: a silent take on a fallback device says so instead of fading away"
```

---

### Task 2: Fall back to the default when a device refuses to open

**Files:**
- Modify: `src-tauri/src/audio.rs`

- [ ] **Step 1: Add the fallback wrapper.** In `src-tauri/src/audio.rs`, add directly above `fn open`:

```rust
/// Try the user's chosen device, then the Windows default if it refuses.
/// `pick_device` already handles a device whose *name* has vanished; this
/// handles one that is still listed but will not open — held by another app,
/// or offering a format we cannot use. Without this the retry loop would
/// reopen the same broken device every two seconds forever.
fn build_with_fallback(
    engine: &Arc<AudioEngine>,
    preferred: &Option<String>,
    log_failure: bool,
) -> Result<cpal::Stream, String> {
    match build_stream(engine, preferred) {
        Ok(s) => Ok(s),
        Err(first) if preferred.is_some() => {
            if log_failure {
                applog::log(&format!("audio-preferred-device-refused {first}"));
            }
            build_stream(engine, &None)
        }
        Err(e) => Err(e),
    }
}
```

- [ ] **Step 2: Use it in both open paths.**

In `fn open`, change `match build_stream(engine, preferred) {` to:

```rust
    match build_with_fallback(engine, preferred, true) {
```

In `fn reopen_quietly`, change `let stream = match build_stream(engine, preferred) {` to:

```rust
    let stream = match build_with_fallback(engine, preferred, false) {
```

(The retry path passes `false` because it runs every two seconds; logging there would fill the file while a device stays broken. The settings window is what tells the user, and `open` logs the first occurrence.)

- [ ] **Step 3: Verify**

Run: `cargo test` → **43 passed**. `cargo check` → zero warnings.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/audio.rs
git commit -m "fix: fall back to the default mic when the chosen one refuses to open"
```

---

### Task 3: Rebuild, re-verify, and finish the milestone

- [ ] **Step 1: Build the installer.**

```powershell
npm run tauri build
```

Report the filename and size.

- [ ] **Step 2: Report this protocol and STOP.** The human runs it.

Install the new build and launch it. Delete `%APPDATA%\WhisperOSS\log.txt` first so the run is clean.

1. **Normal dictation still works.** Hold Ctrl+Win over a text field, speak, release. Text is pasted.
2. **A genuinely silent take is still silent.** With a working microphone selected, hold Ctrl+Win for two seconds and say nothing. Expected: the pill fades away with no error — this behaviour is deliberate and must not change.
3. **A broken microphone now speaks up.** Select your USB device in settings, then disable it in Windows (Sound → Recording → the device → Disable). Hold Ctrl+Win and speak. Expected: the red pill reads **"Check your mic"** rather than fading silently.
4. **Settings agrees.** Open settings from the tray. Beside "Microphone" the row reads **"· unavailable, using \<the device actually recording\>"**.
5. **A refusing device no longer traps the app.** Select DroidCam (the device that failed with `0x88890008`). Wait about five seconds, then hold Ctrl+Win and speak into your real microphone. Expected: it records and pastes — the app fell back to the Windows default instead of retrying DroidCam forever. Settings should show the "unavailable, using …" note.
6. **Recovery still works.** Re-enable the USB device and select it in settings. Dictate — it works, and the note in settings is gone.
7. **The log is not noisy.** Paste `%APPDATA%\WhisperOSS\log.txt`. There must be no long runs of repeated audio errors from the periods when a device was broken.

- [ ] **Step 3: Write `docs/reports/milestone-5a-results.md`** in the same shape as `docs/reports/milestone-4b-results.md`: one row per check with PASS/FAIL and what was observed, the test count (43), the DEVIATIONS list, and a GO / NO-GO verdict for Milestone 5b.

Record that mid-session recovery was measured at 7 ms in the first verification round, and that these two defects were plan gaps found by human testing.

- [ ] **Step 4: Commit**

```bash
git add docs/reports/milestone-5a-results.md
git commit -m "docs: milestone 5a results"
```
