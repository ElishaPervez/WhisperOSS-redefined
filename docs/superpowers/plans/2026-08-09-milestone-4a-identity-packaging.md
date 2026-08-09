# Milestone 4a — Identity & Packaging

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

## Context for a fresh session

WhisperOSS is a Windows dictation app: hold **Ctrl+Win**, speak, release, and the transcript is pasted where your cursor is. It is a Tauri 2 + Rust rewrite of an older Python app kept in `src-reference\`, which is git-ignored and **must never be touched**. The frontend is vanilla HTML/CSS/JS with no bundler.

Milestones 0–3 are complete and verified by a human: the click-through overlay pill, the dictation pipeline (keyboard hook → record → Groq → privacy paste), the tray icon, and the settings window (API key, AI formatting, casual mode, microphone picker, theme, start-with-Windows). The hotkey is fixed at Ctrl+Win. 42 tests pass with zero warnings.

The app has never been packaged. It has only ever been run through `npm run tauri dev`, and it still carries the Tauri scaffold's identity: the Rust crate is called `scaffold-tmp`, so the executable is `scaffold-tmp.exe`, and the icons are the default Tauri artwork.

**Goal of this milestone:** give the app its own name and icon, and produce a real Windows installer. No user-facing behaviour changes.

## Global Constraints

- Windows-only. Repo root: `C:\projects (code)\15. WhisperOSS redefined` (quote paths in shell commands — the folder name contains spaces and parentheses). `cargo` runs from `src-tauri\`.
- **Never touch `src-reference\`.**
- Shell: commands are written for PowerShell. Adapt freely if your shell differs — that is a HOW decision; report it in DEVIATIONS.
- All **42** tests must stay green. Zero compiler warnings.
- The plan's code, commands, and "Expected" values are the source of truth. No unrequested changes.
- Do not pause between tasks. Post a short report after each commit and continue. Stop only for: the human-only step (Task 4), a failed verification, or a mismatch that is not mechanical.
- Tasks 3 and 4 need network access (Tauri downloads the NSIS installer toolchain on first bundle). If the sandbox blocks it, stop and report rather than working around it.
- Keep a running DEVIATIONS list.

---

### Task 1: Rename the crate off the Tauri scaffold

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/main.rs`
- Modify: `package.json`

The executable is named after the Rust package, which is still the scaffold's `scaffold-tmp`. That name reaches the user in three visible places: the process in Task Manager, the path written to the start-with-Windows registry entry, and the window's taskbar entry.

- [ ] **Step 1: Rename the package.** In `src-tauri/Cargo.toml`, change the `[package]` block's first lines to:

```toml
[package]
name = "whispeross"
version = "0.1.0"
description = "Hold Ctrl+Win, speak, release — your words are typed wherever the cursor is."
authors = ["Elisha Pervez"]
edition = "2021"
```

And in the `[lib]` block, change the library name:

```toml
name = "whispeross_lib"
```

Leave `crate-type` and every dependency exactly as they are.

- [ ] **Step 2: Update the entry point.** In `src-tauri/src/main.rs`, change the body of `fn main` to:

```rust
    whispeross_lib::run()
```

- [ ] **Step 3: Rename the npm package.** In `package.json`, change `"name": "scaffold-tmp"` to `"name": "whispeross"`. Change nothing else.

- [ ] **Step 4: Verify**

Run: `cargo test` → **42 passed**.
Run: `cargo check` → zero warnings.
Confirm the built binary is now named `whispeross.exe`:

```powershell
Get-ChildItem "src-tauri\target\debug\*.exe" | Select-Object Name
```

Expected: `whispeross.exe` present. (An old `scaffold-tmp.exe` may still be sitting in `target\debug` from a previous build. That is stale output, not a failure — note it and leave it; `target\` is git-ignored.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/main.rs package.json
git commit -m "chore: rename crate from the Tauri scaffold to whispeross"
```

---

### Task 2: A real app icon

**Files:**
- Create: `docs/design/app-icon-source.png`
- Replace: everything in `src-tauri/icons/`

The current icons are the stock Tauri artwork. The design system's brand mark is a solid accent square — the same mark that sits in the settings window's title bar and at the top of the first-run card. The icon is that mark: an accent square centred on the dark surface colour.

Colours come from `src/design-tokens.css` and are not negotiable: background `#191817`, mark `#FF563C`.

- [ ] **Step 1: Generate the source image.** Create a 1024×1024 PNG at `docs/design/app-icon-source.png`: the whole canvas filled with `#191817`, and a `#FF563C` square exactly 340×340 px centred on it (top-left corner at 342, 342). No rounding, no gradient, no border — the design language is flat.

Any tool is acceptable; this is a HOW decision. On Windows with no extra dependencies, `System.Drawing` works:

```powershell
Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap 1024, 1024
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.Clear([System.Drawing.ColorTranslator]::FromHtml("#191817"))
$brush = New-Object System.Drawing.SolidBrush([System.Drawing.ColorTranslator]::FromHtml("#FF563C"))
$g.FillRectangle($brush, 342, 342, 340, 340)
$g.Dispose()
$bmp.Save("docs\design\app-icon-source.png", [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
```

- [ ] **Step 2: Generate every icon size from it.**

```powershell
npx @tauri-apps/cli icon "docs\design\app-icon-source.png"
```

This overwrites `src-tauri/icons/` with the full set (`32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.ico`, `icon.icns`, the Square*Logo.png set, and `icon.png`). Leave `tauri.conf.json`'s existing `bundle.icon` list alone — those paths are unchanged.

- [ ] **Step 3: Verify**

- `src-tauri/icons/icon.ico` and `src-tauri/icons/32x32.png` exist and have a recent modified time.
- Open `src-tauri/icons/32x32.png` and confirm it is a dark square with an orange square inside — not the Tauri logo.
- Run `cargo check` → still zero warnings.

- [ ] **Step 4: Commit**

```bash
git add docs/design/app-icon-source.png src-tauri/icons
git commit -m "feat: app icon from the brand mark, replacing the Tauri scaffold artwork"
```

---

### Task 3: Build a Windows installer

**Files:**
- Modify: `src-tauri/tauri.conf.json`

- [ ] **Step 1: Configure the bundle.** In `src-tauri/tauri.conf.json`, replace the `"bundle"` object with:

```json
  "bundle": {
    "active": true,
    "targets": ["nsis"],
    "publisher": "Elisha Pervez",
    "shortDescription": "Hold Ctrl+Win, speak, release.",
    "longDescription": "Dictation for Windows. Hold Ctrl+Win in any app, speak, and release — your words are typed wherever the cursor already was. Transcription runs on Groq; the API key stays on this PC.",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
```

(`nsis` alone, not `all`: it produces one friendly `setup.exe` that installs per-user with no administrator prompt, which matches an app whose autostart is also per-user. Building `all` would additionally produce an MSI that nobody needs.)

- [ ] **Step 2: Build the installer.**

```powershell
npm run tauri build
```

This compiles in release mode and downloads the NSIS toolchain on first run, so expect several minutes. If the download is blocked, STOP and report — do not substitute a different target.

- [ ] **Step 3: Verify the output exists**

```powershell
Get-ChildItem "src-tauri\target\release\bundle\nsis\*.exe" | Select-Object Name, Length
```

Expected: a single file named `WhisperOSS_0.1.0_x64-setup.exe` (or the same shape with the platform suffix your toolchain emits). Report the exact filename and size.

Also confirm the release binary is correctly named:

```powershell
Get-ChildItem "src-tauri\target\release\WhisperOSS.exe","src-tauri\target\release\whispeross.exe" -ErrorAction SilentlyContinue | Select-Object Name
```

Report which name is present — Tauri renames the binary to `productName` during bundling, so either is acceptable; the human check in Task 4 is what decides.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/tauri.conf.json
git commit -m "feat: per-user NSIS installer with real app metadata"
```

(`target/` is git-ignored, so the installer itself is not committed.)

---

### Task 4: Human verification

- [ ] **Step 1: Report this protocol and STOP.** The human runs it; wait for PASS/FAIL before writing the report.

**Before starting:** quit any running WhisperOSS from the tray, and stop any `npm run tauri dev` session. Two copies running at once will confuse every check below.

Record the current autostart value so the change is visible:

```powershell
reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v WhisperOSS
```

Then run the installer from `src-tauri\target\release\bundle\nsis\`.

1. **The installer looks like a real app.** It shows the WhisperOSS name and the orange-square icon, and installs without an administrator prompt.
2. **It launches.** WhisperOSS appears in the Start menu with the orange-square icon and starts from there.
3. **The tray icon is the new one.** The system tray shows the orange square, not the Tauri logo.
4. **Settings still work.** Left-click the tray icon. The window opens, still shows your saved API key as "Saved", and your microphone is still selected. (Settings and the API key are stored per-user outside the app folder, so the installed copy inherits them from the dev builds.)
5. **Dictation still works.** Hold Ctrl+Win over a text field, speak, release. The text is pasted.
6. **The process has the right name.** With the app running, check Task Manager, or:

```powershell
Get-Process | Where-Object { $_.ProcessName -like "*hisper*" -or $_.ProcessName -like "*scaffold*" } | Select-Object ProcessName, Id
```

Expected: a WhisperOSS-named process, and **no** `scaffold-tmp`.

7. **Autostart now points at the installed app.** In settings, toggle Start with Windows off, then on. Then:

```powershell
reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v WhisperOSS
```

Expected: the path points into the installed location (under `%LOCALAPPDATA%\Programs\` or wherever the installer placed it), **not** into `src-tauri\target\debug`.

8. **It survives a reboot-equivalent.** Quit from the tray, launch again from the Start menu, and confirm dictation still works.
9. **Uninstall is clean.** Uninstall via Settings → Apps. WhisperOSS disappears from the Start menu. Note whether the tray icon and the autostart registry entry are gone afterwards, and report what you observe either way.

Then reinstall it so the app is available for the next milestone.

- [ ] **Step 2: Write `docs/reports/milestone-4a-results.md`** in the same shape as `docs/reports/milestone-3c-results.md`: one row per check with PASS/FAIL and what was observed, the installer's exact filename and size, the test count, the DEVIATIONS list, and a GO / NO-GO verdict for Milestone 4b.

- [ ] **Step 3: Commit**

```bash
git add docs/reports/milestone-4a-results.md
git commit -m "docs: milestone 4a results"
```

---

## What comes next (not this plan)

**Milestone 4b — First run + typography.** The onboarding window from `docs/design/art3-firstrun.png` (two steps: welcome, then API key with inline validation), shown when no key is saved. Right now a fresh install looks broken — the hotkey silently does nothing and the log is the only clue. Plus bundling the **Archivo** typeface the design actually specifies; the app currently substitutes Segoe UI, so it does not yet look like the design.
