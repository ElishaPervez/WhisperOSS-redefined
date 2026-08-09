# Milestone 4b results - first run and typography

Date: 2026-08-09

## Final verification

| # | Check | Result | Notes |
|---|---|---|---|
| 1 | Clean install launches the current application | PASS | After the previous installation was fully removed, `WhisperOSS_0.1.0_x64-setup.exe` installed successfully and WhisperOSS launched from the Start menu. |
| 2 | Welcome card appears when no key is saved | PASS | The welcome card opened by itself on launch after the saved Windows credential was removed. The application log recorded `api-key-missing -> first-run-no-key`. |
| 3 | Accepted key hands the user to settings | PASS | A valid Groq key was accepted, the welcome card closed, and the settings window opened immediately. The application log recorded `api-key-saved -> first-run-complete`; the app no longer appeared to vanish. |
| 4 | Saved-key state is current immediately | PASS | The API key row read "Saved" in green as soon as settings opened, without an application restart. |
| 5 | Dictation works immediately after setup | PASS | Dictation worked seconds after the key was accepted, without a restart. The application log recorded `recording-start -> pasted-confirmed`. |
| 6 | Reopened settings still shows current values | PASS | After settings was closed and reopened from the tray, the API key row still read "Saved". |
| 7 | Microphones added after startup appear | PASS | A microphone enabled while settings was closed appeared in the dropdown when settings reopened, without restarting the app. |
| 8 | AI formatting still works | PASS | With AI formatting enabled, dictation returned punctuated text. Formatting was then disabled again successfully. |

## Package artifacts

- Installer: `WhisperOSS_0.1.0_x64-setup.exe` - 2,620,239 bytes.
- Bundled Archivo font: 34,928 bytes.
- Clean-install executable: `C:\Users\PC\AppData\Local\WhisperOSS\WhisperOSS.exe`.
- Clean-install process name: `WhisperOSS`.

Automated tests: 42 passed. `cargo check`: passed with zero compiler warnings.

## Executable-name resolution

An upgrade over the existing installation continued to run `whispeross.exe`, so the new capitalization was not visible in Task Manager. A full uninstall followed by a clean install produced `C:\Users\PC\AppData\Local\WhisperOSS\WhisperOSS.exe`, and Task Manager showed the process as `WhisperOSS`.

The old lowercase name was an artifact left by upgrading the existing installation, not a current configuration problem. The clean installer already produces the intended name, so no fix was required.

## Plan gaps found during verification

Three gaps in the Milestone 4b plan were exposed by verification. They were plan omissions, not execution errors.

| Gap | What the user observed and why | Fix commit |
|---|---|---|
| Browser-opening capability was not registered | Clicking the Groq link did not open a browser because the application had not initialized the capability that handles external links. | `c2b39bb` - `fix: register the opener plugin so the Groq link opens a browser` |
| First-run hand-off was missing | After a valid key was accepted, the welcome card closed and no window replaced it, making the running tray application appear to have vanished. The success path hid the welcome card without opening settings. | `b4a7282` - `fix: first run hands off to settings instead of vanishing` |
| Settings values became stale | Settings could omit the green "Saved" marker and newly enabled microphones because it read values only once at application startup, while the window was hidden. It now re-reads current values every time it opens. | `79c3fc8` - `fix: settings re-reads its values each time the window opens` |

Human verification confirmed that all three fixes work in the installed build.

## DEVIATIONS

- None.

Verdict: **GO for Milestone 5.** The clean installer uses the intended executable name, first-time setup visibly hands the user into a current settings window, Archivo is bundled, dictation works immediately after setup, devices added after startup appear on reopen, and AI formatting remains functional.
