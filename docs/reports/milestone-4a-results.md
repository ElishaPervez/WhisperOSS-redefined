# Milestone 4a results — identity and Windows packaging

Date: 2026-08-09

## Final verification

| # | Check | Result | Notes |
|---|---|---|---|
| 1 | Installer presents WhisperOSS and installs per-user | PASS | `WhisperOSS_0.1.0_x64-setup.exe` showed the WhisperOSS name and installed per-user without an administrator prompt. |
| 2 | Start-menu identity and launch work | PASS | WhisperOSS appeared in the Start menu with the orange-square icon and launched successfully from there. |
| 3 | System-tray icon uses the brand mark | PASS | The tray showed the orange-square mark instead of the Tauri scaffold artwork. |
| 4 | Settings and saved credentials survive installation | PASS | The installed build inherited the saved API key, showed it as “Saved,” and preserved the selected microphone because those values live outside the application folder. The app window also displayed the orange-square icon. |
| 5 | Dictation works from the installed build | PASS | Holding Ctrl+Win over a text field, speaking, and releasing pasted the transcript at the cursor. |
| 6 | Installed process has the product identity | PASS | The running process was `whispeross.exe`; no `scaffold-tmp` process existed. |
| 7 | Autostart points to the installed executable | PASS | After Start with Windows was toggled off and on, the Run value was `"C:\Users\PC\AppData\Local\WhisperOSS\whispeross.exe"`, not a development path under `src-tauri\target\debug`. |
| 8 | Start-menu relaunch preserves dictation | PASS | After quitting from the tray and launching again from the Start menu, dictation still worked. |
| 9 | Uninstall removes the installed application cleanly | PASS | Uninstall left no application files, autostart entry, running process, Start-menu entry, or Windows Apps-list entry. `%APPDATA%\WhisperOSS` and the Credential Manager key intentionally survived so user settings and the saved API key remain available after reinstall. The application was reinstalled afterward for Milestone 4b. |

## Package artifacts

- Installer: `WhisperOSS_0.1.0_x64-setup.exe` — 2,499,596 bytes.
- Install location: `C:\Users\PC\AppData\Local\WhisperOSS`.
- Installed footprint: approximately 11 MB, consisting of `whispeross.exe` at 11,253,760 bytes plus `uninstall.exe`.
- Installation mode: per-user, with no administrator prompt.

The installer is smaller than the installed application because NSIS compresses the executable and installer payload into the 2,499,596-byte setup file.

Automated tests: 42 passed. `cargo check`: passed with zero compiler warnings.

## Milestone 4b follow-up

The installed process is `whispeross.exe`, using the lowercase Rust package name. This is cosmetic and visible in Task Manager; it does not affect installation, launching, autostart, settings, or dictation. Milestone 4b should set `mainBinaryName` in `tauri.conf.json` so the executable is `WhisperOSS.exe`. The change was explicitly deferred to Milestone 4b.

## DEVIATIONS

- The requested plan-execution helper was not installed, so the written plan was executed directly without expanding its repository scope.
- The old `scaffold-tmp.exe` remains only as ignored debug output. The required `whispeross.exe` was built, and human verification confirmed that no `scaffold-tmp` process runs after installation.
- The icon source was generated with the plan’s deterministic `System.Drawing` method so the 1024×1024 canvas, 340×340 mark, coordinates, and colors are exact.
- The first NSIS build invocation was terminated after one second by an incorrect command timeout. The unchanged build command was rerun with sufficient time and completed successfully.
- Tauri emitted a packaging advisory because the existing bundle identifier ends in `.app`. The plan did not authorize changing that identifier, so it remains unchanged.
- The release folder contains both `WhisperOSS.exe` and `whispeross.exe` with identical size and modification time. The installed process uses `whispeross.exe`; the capitalization change is recorded above for Milestone 4b.

Verdict: **GO for Milestone 4b.** The app now has its own identity, a verified brand icon across Windows surfaces, a working per-user NSIS installer, a clean installed process and autostart path, preserved user settings, functioning dictation, and clean uninstall behavior.
