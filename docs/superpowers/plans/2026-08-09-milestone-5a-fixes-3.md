# Milestone 5a — Fix: the app never returns to the chosen microphone

**Status:** The live-update protocol found a new defect. Losing the device works: disabling the USB microphone put the app on the Windows default within about a second, and the open settings window showed "· unavailable, using …" on its own. But re-enabling the device did nothing — the note never cleared and the app kept recording on the fallback forever.

**Log evidence.** The fallback is announced and then the log goes silent — the device was re-enabled and the app never reacted:

```
audio-callback-error The requested device is no longer available...
audio-preferred-device-missing-using-default
audio-stream-recovered
(nothing further)
```

**Root cause.** The two-second tick in the stream thread has exactly one trigger: *there is no working stream*. Once the app has fallen back to a working default, everything is "healthy" by its own definition, so it stops looking. Nothing anywhere asks "did the device the user picked come back?" — the only path back is the user manually reselecting it in the dropdown.

**Fix.** While the app is running on a device other than the one the user picked, the same two-second tick also checks whether the picked device has reappeared in Windows, and moves back to it the moment it has. Guards:

- Never mid-recording — swapping streams would cut the take in half.
- A device that is *listed* but *refuses to open* (DroidCam) is attempted once per appearance and then remembered, so it cannot cause a stream teardown and a log line every two seconds. The memory is cleared when the device vanishes again (a fresh appearance deserves a fresh try) or when the user picks anything in the dropdown.
- In the steady state — on the right device — the tick does nothing and does not enumerate devices at all.

A successful move back goes through the existing `open`, which already announces, so the open settings window clears its note live with no new wiring.

## Global Constraints

- Windows-only. Repo root: `C:\projects (code)\15. WhisperOSS redefined` (quote paths). `cargo` runs from `src-tauri\`. Never touch `src-reference\`.
- Test count goes from 43 to **44** (Task 1 adds one). Zero compiler warnings.
- Change only what is written below.
- Do not pause between tasks. Stop only for the human step in Task 3.

---

### Task 1: The decision, as pure logic

**Files:**
- Modify: `src-tauri/src/audio.rs`

- [ ] **Step 1: Write the failing test.** Add a `mod tests` at the bottom of `src-tauri/src/audio.rs` (there is none yet):

```rust
#[cfg(test)]
mod tests {
    use super::{reclaim_step, Reclaim};

    #[test]
    fn reclaim_decision() {
        use Reclaim::*;
        let usb = Some("USB PnP Audio Device");
        let nvidia = Some("NVIDIA Broadcast");
        // user picked "system default": there is nothing to go back to
        assert_eq!(reclaim_step(None, nvidia, false, false, None), Stay);
        // already on the picked device
        assert_eq!(reclaim_step(usb, usb, false, true, None), Stay);
        // picked device still absent from Windows: wait, and forget any refusal
        assert_eq!(reclaim_step(usb, nvidia, false, false, None), ForgetRefusal);
        assert_eq!(reclaim_step(usb, nvidia, false, false, usb), ForgetRefusal);
        // picked device is back: move to it
        assert_eq!(reclaim_step(usb, nvidia, false, true, None), Attempt);
        assert_eq!(reclaim_step(usb, None, false, true, None), Attempt);
        // back, but a recording is running: not in the middle of a take
        assert_eq!(reclaim_step(usb, nvidia, true, true, None), Stay);
        // listed but it refused to open last time: do not churn every two seconds
        assert_eq!(reclaim_step(usb, nvidia, false, true, usb), Stay);
    }
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test reclaim_decision`
Expected: compile failure — `reclaim_step` does not exist.

- [ ] **Step 3: Add the logic.** In `src-tauri/src/audio.rs`, directly above `fn build_with_fallback`:

```rust
/// What the two-second tick should do about getting back to the device the
/// user actually picked. Pure so it can be tested.
#[derive(Debug, PartialEq)]
enum Reclaim {
    /// Nothing to do — no picked device, on the right one already, mid-take,
    /// or the device is listed but refused to open last time.
    Stay,
    /// The picked device vanished from Windows again: forget a remembered
    /// refusal so its next appearance gets a fresh attempt.
    ForgetRefusal,
    /// The picked device is back in Windows — move to it.
    Attempt,
}

fn reclaim_step(
    wanted: Option<&str>,
    active: Option<&str>,
    recording: bool,
    listed: bool,
    refused: Option<&str>,
) -> Reclaim {
    let Some(name) = wanted else { return Reclaim::Stay };
    if active == Some(name) {
        return Reclaim::Stay;
    }
    if !listed {
        return Reclaim::ForgetRefusal;
    }
    if recording || refused == Some(name) {
        return Reclaim::Stay;
    }
    Reclaim::Attempt
}
```

- [ ] **Step 4: Verify**

Run: `cargo test` → **44 passed**. `cargo check` → zero warnings (the enum and function are not referenced yet; if dead-code warnings appear here, proceed straight into Task 2, which uses them, and verify zero warnings there).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/audio.rs
git commit -m "feat: pure decision logic for reclaiming the chosen microphone"
```

---

### Task 2: Wire it into the two-second tick

**Files:**
- Modify: `src-tauri/src/audio.rs`

- [ ] **Step 1: Remember a refusal.** Add a field to `pub struct AudioEngine`, next to `active_device`:

```rust
    /// The picked device we last tried to move back to and could not open.
    /// Checked so a device that is listed but permanently refusing cannot
    /// cause a reopen attempt every two seconds.
    refused_reclaim: Mutex<Option<String>>,
```

and its initialiser inside the `Arc::new(AudioEngine { ... })` block:

```rust
            refused_reclaim: Mutex::new(None),
```

- [ ] **Step 2: A manual pick clears the memory.** In `switch_device`, after the log line:

```rust
        *self.refused_reclaim.lock().unwrap() = None;
```

- [ ] **Step 3: Add the check method.** Inside `impl AudioEngine`, next to `announce_change`:

```rust
    /// The two-second tick's decision about moving back to the picked device.
    /// The steady state — already on the picked device, or no pick at all —
    /// returns before enumerating devices, so the tick stays free.
    fn reclaim_check(&self, wanted: &Option<String>) -> Reclaim {
        let Some(name) = wanted.as_deref() else { return Reclaim::Stay };
        let active = self.active_device();
        if active.as_deref() == Some(name) {
            return Reclaim::Stay;
        }
        let recording = self.recording.lock().unwrap().is_some();
        let listed = list_input_devices().iter().any(|d| d == name);
        let refused = self.refused_reclaim.lock().unwrap().clone();
        reclaim_step(Some(name), active.as_deref(), recording, listed, refused.as_deref())
    }
```

- [ ] **Step 4: Act on it.** In the stream thread inside `AudioEngine::start`, replace the timeout branch:

```rust
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
```

with:

```rust
                    Err(RecvTimeoutError::Timeout) => {
                        // Nobody asked for a change. If there is no working
                        // stream — no mic at boot, one unplugged, or a device
                        // Windows refused — try again.
                        if !e.is_healthy() {
                            drop(stream.take());
                            e.reset_buffers();
                            stream = reopen_quietly(&e, &wanted);
                        } else {
                            // Healthy, but possibly on a fallback. If the
                            // device the user picked is back, move to it.
                            match e.reclaim_check(&wanted) {
                                Reclaim::Attempt => {
                                    applog::log("audio-reclaiming-preferred-device");
                                    drop(stream.take());
                                    e.reset_buffers();
                                    stream = open(&e, &wanted, true);
                                    if e.active_device().as_deref() != wanted.as_deref() {
                                        // Listed but would not open: remember,
                                        // or this repeats every two seconds.
                                        *e.refused_reclaim.lock().unwrap() = wanted.clone();
                                    }
                                }
                                Reclaim::ForgetRefusal => {
                                    *e.refused_reclaim.lock().unwrap() = None;
                                }
                                Reclaim::Stay => {}
                            }
                        }
                    }
```

- [ ] **Step 5: Verify**

Run: `cargo test` → **44 passed**. `cargo check` → zero warnings.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/audio.rs
git commit -m "fix: the app moves back to the chosen microphone when it reappears"
```

---

### Task 3: Rebuild, re-verify, and finish the milestone

This replaces the previous plan's Task 3 entirely — same build, updated protocol, and the milestone report now covers this defect too.

- [ ] **Step 1: Build the installer.**

```powershell
npm run tauri build
```

Report the filename and size.

- [ ] **Step 2: Report this protocol and STOP.** The human runs it. Ask for `%APPDATA%\WhisperOSS\log.txt` to be deleted first so the run is clean.

**The settings window stays open the whole time.** Nothing may be closed, reopened, or dictated into to force a refresh.

1. **Install the new build**, launch it, open settings from the tray, and leave it open.
2. **Losing a device updates live.** With the USB microphone selected, disable it in Windows (Sound → Recording → the device → Disable). Within a couple of seconds the Microphone row shows **"· unavailable, using \<other device\>"** on its own.
3. **Getting it back is automatic now.** Re-enable the device in Windows and touch nothing — not the dropdown, not the window. Within a few seconds the app must move back to the USB microphone **by itself**: the note clears on its own, and a dictation afterwards comes from the USB device.
4. **A refusing device does not churn.** Select DroidCam in the dropdown. The row shows the "unavailable, using …" note. Wait at least twenty seconds doing nothing. Dictation into the real microphone still works throughout.
5. **Recovering from that is still manual and still works.** Reselect the USB device. The note clears and the status bar reads "Microphone updated".
6. **Nothing regressed.** Dictate normally — text pastes. Record two seconds of silence on a working device — the pill fades with no error. Disable the selected device and dictate — the red pill reads "Check your mic". Re-enable it afterwards.
7. **The log is quiet.** Paste `%APPDATA%\WhisperOSS\log.txt`. It must show `audio-reclaiming-preferred-device` for step 3, at most one reclaim attempt for DroidCam in step 4, and no line repeating every two seconds anywhere.

- [ ] **Step 3: Write `docs/reports/milestone-5a-results.md`** in the same shape as `docs/reports/milestone-4b-results.md`: one row per check with PASS/FAIL and what was observed, the test count (44), the DEVIATIONS list from all rounds, and a GO / NO-GO verdict for Milestone 5b.

Record all of it: mid-session loss-to-fallback recovery measured at 7 ms in the first round; the **four** defects human testing found across the three rounds — a deaf fallback device fading silently, a refusing device retried forever, a settings window that did not notice changes while open, and an app that never returned to the chosen device after it came back — and their fix commits, plus the startup-announcement race caught during Task 2 verification of the previous plan.

- [ ] **Step 4: Commit**

```bash
git add docs/reports/milestone-5a-results.md
git commit -m "docs: milestone 5a results"
```
