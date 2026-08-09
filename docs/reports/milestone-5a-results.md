# Milestone 5a results - reliability and recovery

Date: 2026-08-09

## Final verification

| # | Check | Result | Notes |
|---|---|---|---|
| 1 | Current installer launches with settings open for live checks | PASS | `WhisperOSS_0.1.0_x64-setup.exe` installed and launched successfully. Settings opened from the tray and remained open throughout the device-loss, return, and switching checks. |
| 2 | Losing the selected device updates settings live | PASS | Disabling the selected USB microphone moved recording to the Windows-default fallback in about 0.8 s. Without closing, reopening, or touching settings, the microphone row changed to "unavailable, using <other device>". |
| 3 | The app automatically returns to the selected device | PASS | Re-enabling the USB microphone triggered `audio-reclaiming-preferred-device`. The swap from the fallback to the returned USB device completed in 11 ms, the warning note cleared by itself, and subsequent dictation used the USB microphone. |
| 4 | A listed device that refuses to open does not churn | PASS | Selecting DroidCam produced exactly one reclaim attempt for that selection, then went quiet for the remainder of the wait. The app stayed on the working fallback and dictation continued to paste normally. |
| 5 | A manual selection clears refusal memory | PASS | Reselecting the USB microphone moved recording back to it, cleared the warning note, and showed "Microphone updated" in the status bar. |
| 6 | Dictation and silent-take behavior did not regress | PASS | Normal dictation pasted successfully and a genuinely silent take on a working device still faded without an error. "Check your mic" remains PASS from the preceding human round; this final round produced no log evidence for that sub-check, and the responsible path in `src-tauri/src/pipeline.rs` was untouched by the reclaim plan. |
| 7 | Broken-device periods do not fill the log | PASS | The log contained the expected reclaim event, at most one DroidCam reclaim attempt per selection, and no line repeating every two seconds. All completed dictations pasted successfully. |

Earlier verification also confirmed that a rejected Groq key shows "Invalid API key" and opens settings with the key box focused, replacing it with a good key restores dictation without a restart, and AI formatting still produces punctuated text.

## Package artifacts

- Installer: `WhisperOSS_0.1.0_x64-setup.exe` - 2,623,818 bytes.
- Automated tests: 44 passed.
- `cargo check`: passed with zero compiler warnings.

## Recovery measurements

- First human round: after a running microphone disappeared, the audio callback detected the loss and recovered onto the Windows-default fallback in 7 ms.
- Final human round: the complete observed loss-to-fallback transition took about 0.8 s.
- Final human round: after the selected USB microphone reappeared, the reclaim event fired and the stream moved from the fallback back to USB in 11 ms.

The 7 ms and 11 ms measurements cover the engine's two recovery operations: moving away from a lost device and moving back to the selected device after it returns.

## Plan gaps found by human verification

Four defects were exposed across three human-verification rounds. They were gaps in the milestone plans, not execution errors.

| Defect | What the user observed and why | Fix commits |
|---|---|---|
| A silent fallback looked like no response | Windows selected NVIDIA Broadcast as the fallback, but that virtual device produced silence while its companion app was not running. Real-length dictations were therefore discarded as silence and the pill faded without explaining why. The app now preserves quiet-user behavior on the selected device but shows "Check your mic" when silence comes from a fallback. | `0fc9db2` - `feat: a silent take on a fallback device says so instead of fading away` |
| A refusing microphone trapped the app | A device could remain listed in Windows while refusing to open. The app retried that same broken device indefinitely because fallback previously covered only devices whose names disappeared. It now tries the Windows default when the selected device refuses. | `5270664` - `fix: fall back to the default mic when the chosen one refuses to open` |
| An open settings window kept an old microphone warning | Device loss and recovery changed the recording stream, but an already-open settings window refreshed only when reopened. The audio engine now announces real transitions and settings refreshes only the microphone row. | `aa722bb` - `feat: the audio engine announces microphone changes`; `ac4fbd1` - `fix: settings updates the microphone row while the window is open` |
| The app never returned to the selected microphone | Once a working fallback opened, the stream counted as healthy and the two-second tick stopped checking whether the selected device had returned. The app now detects a reappearance, avoids switching mid-recording, and remembers one failed reclaim attempt so a refusing device cannot cause repeated teardown. | `05047cd` - `feat: pure decision logic for reclaiming the chosen microphone`; `fbe21df` - `fix: the app moves back to the chosen microphone when it reappears` |

## Startup race found during development verification

The first version of live microphone announcements also announced the initial stream opening during application setup. Settings reacted before its shared data had been registered, so DevTools showed one red startup error. This was caught during the required Task 2 console check, before the change was committed to the settings listener.

The initial stream opening is not a transition and now stays silent; later losses, recoveries, and device switches still announce. Commit `72b6401` (`fix: the startup stream open does not announce before the app is ready`) removed the race. A fresh development launch then showed zero console messages and no red errors.

## DEVIATIONS

- The plan's prescribed Superpowers execution helper was not available, so the written checklist was executed directly without expanding repository scope.
- The original rejected-key plan moved the recording loop's application handle into the first background task, so Rust would not compile. The plan was amended to use the background task's existing handle before execution resumed.
- The bundled desktop-control runtime was unavailable. The accepted Windows UI Automation substitution performed the required development-window, dropdown, and console checks.

Verdict: **GO for Milestone 5b.** Rejected keys now lead directly to repair, lost microphones recover without restarting, refusing devices fall back without log spam, fallback silence produces a useful error, settings reflects microphone transitions while it remains open, and the app automatically returns to the selected device when it reappears.
