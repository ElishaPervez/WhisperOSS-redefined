# Milestone 4b — First Run & Typography

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

## Context for a fresh session

WhisperOSS is a Windows dictation app: hold **Ctrl+Win**, speak, release, and the transcript is pasted where your cursor is. Tauri 2 + Rust, vanilla HTML/CSS/JS frontend, no bundler. The old Python version lives in `src-reference\`, which is git-ignored and **must never be touched**.

Milestones 0–4a are complete and human-verified: the click-through overlay pill, the dictation pipeline, the tray icon, the settings window, and a per-user NSIS installer with a real app icon. 42 tests pass with zero warnings.

**Two gaps remain before the app is presentable.**

1. **A fresh install looks broken.** The app stores its Groq API key in Windows Credential Manager. With no key saved, pressing Ctrl+Win does nothing visible — the only evidence is a line in a log file. There is no onboarding at all. The approved design already solves this: `docs/design/art3-firstrun.png` shows a two-step first-run card.
2. **The app is in the wrong typeface.** The design system specifies **Archivo** for everything (`--font-heading` and `--font-body` in the design export both resolve to `"Archivo", system-ui, sans-serif`). `src/design-tokens.css` currently substitutes Segoe UI, so the app does not look like the design. The design export loads Archivo from Google's servers; a desktop app must bundle it instead so it renders instantly and works offline.

**Goal:** ship the first-run flow, bundle Archivo, and finish the executable rename left over from 4a.

**Design reference:** open `docs/design/art3-firstrun.png` before starting Task 2. It shows four states — step 1 dark, step 1 light, step 2 light, step 2 dark with an invalid key. The window shell is the same 960×640 frameless card as the settings window.

## Global Constraints

- Windows-only. Repo root: `C:\projects (code)\15. WhisperOSS redefined` (quote paths in shell commands — the folder name has spaces and parentheses). `cargo` runs from `src-tauri\`.
- **Never touch `src-reference\`.**
- Shell: commands are written for PowerShell. Adapt freely if your shell differs — that is a HOW decision; report it in DEVIATIONS.
- All **42** tests must stay green. Zero compiler warnings. This milestone is almost entirely UI and window wiring, so it adds no new pure logic and therefore no new tests — that is expected, not an oversight.
- The plan's code, commands, and "Expected" values are the source of truth. No unrequested changes.
- Do not pause between tasks. Post a short report after each commit and continue. Stop only for: the human-only step (Task 6), a failed verification, or a mismatch that is not mechanical.
- Task 1 needs network access. If it is blocked, STOP and report — do not substitute a different typeface or fall back to a web-hosted font.
- Keep a running DEVIATIONS list.

---

### Task 1: Bundle the Archivo typeface

**Files:**
- Create: `src/fonts/archivo-latin.woff2`
- Create: `src/fonts/OFL.txt`
- Modify: `src/design-tokens.css`

- [ ] **Step 1: Download the font.** Archivo is licensed under the SIL Open Font License, so it can be redistributed inside the app. Fetch the **latin subset, variable weight** woff2 from Google Fonts and save it as `src/fonts/archivo-latin.woff2`.

The Google Fonts CSS API serves woff2 only to modern user agents, so the request needs a browser user-agent string. One way (adapt freely — this is a HOW decision):

```powershell
New-Item -ItemType Directory -Force -Path "src\fonts" | Out-Null
$ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36"
$css = Invoke-WebRequest -Uri "https://fonts.googleapis.com/css2?family=Archivo:wght@100..900" -UserAgent $ua -UseBasicParsing
$css.Content
```

The response contains several `@font-face` blocks, one per unicode subset. Pick the block whose `unicode-range` includes `U+0000-00FF` — that is the **latin** subset — and download the `.woff2` URL from that block only. Save it to `src\fonts\archivo-latin.woff2`.

Report the file's size in bytes. Expect roughly 20–40 KB. If it is under 5 KB or over 200 KB you have grabbed the wrong subset — stop and report.

- [ ] **Step 2: Include the licence.** Save the Open Font License text to `src/fonts/OFL.txt`, from:

```
https://raw.githubusercontent.com/google/fonts/main/ofl/archivo/OFL.txt
```

Redistributing the font without its licence file is not acceptable.

- [ ] **Step 3: Declare and use the font.** At the very top of `src/design-tokens.css`, above the existing comment, add:

```css
@font-face {
  font-family: "Archivo";
  src: url("fonts/archivo-latin.woff2") format("woff2");
  font-weight: 100 900;
  font-style: normal;
  font-display: block;
}
```

Then, in **all three** blocks that define them (`:root`, the `prefers-color-scheme: dark` block if it defines fonts, and any `[data-theme]` block that does), change the two font variables to put Archivo first:

```css
  --font-heading: "Archivo", "Segoe UI Variable Display", system-ui, sans-serif;
  --font-body: "Archivo", "Segoe UI Variable Text", system-ui, sans-serif;
```

If a block does not currently define the font variables, leave it alone — they inherit from `:root`.

(`font-display: block` is correct here rather than `swap`: the file is local, so it loads in milliseconds, and blocking avoids a visible flash of the wrong typeface as the window opens.)

- [ ] **Step 4: Verify**

Run `npm run tauri dev`, open settings from the tray. The window must render in Archivo — visibly different from Segoe UI: look at the capital "G" in "Groq API key" and the overall letterforms, which are more geometric and tighter. Right-click → Inspect → Network tab, reload, and confirm `archivo-latin.woff2` loads from the app itself with **no request to fonts.gstatic.com**.

- [ ] **Step 5: Commit**

```bash
git add src/fonts src/design-tokens.css
git commit -m "feat: bundle the Archivo typeface the design specifies"
```

---

### Task 2: The first-run window shell

**Files:**
- Create: `src/firstrun.html`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/default.json`

- [ ] **Step 1: Register the window.** In `src-tauri/tauri.conf.json`, add a third entry to `app.windows`, after the `settings` entry:

```json
      {
        "label": "firstrun",
        "title": "Welcome to WhisperOSS",
        "url": "firstrun.html",
        "width": 960,
        "height": 640,
        "resizable": false,
        "maximizable": false,
        "minimizable": false,
        "decorations": false,
        "transparent": false,
        "center": true,
        "skipTaskbar": false,
        "visible": false
      }
```

- [ ] **Step 2: Grant it permissions.** In `src-tauri/capabilities/default.json`, add `"firstrun"` to the `windows` array so it reads `["overlay", "settings", "firstrun"]`, and add one permission to the `permissions` array:

```json
    "opener:allow-open-url"
```

(The window needs `hide` to close itself, which the existing `core:window:allow-hide` already covers. `opener:allow-open-url` is for the "Get one at console.groq.com/keys" link.)

- [ ] **Step 3: Build the markup.** Create `src/firstrun.html`. Both steps live in one document; only one is visible at a time. Layout, wording and states come from `docs/design/art3-firstrun.png` — read that image before writing this.

```html
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8" />
<link rel="stylesheet" href="design-tokens.css" />
<style>
  * { box-sizing: border-box; }
  html, body { margin: 0; height: 100vh; overflow: hidden; }
  body {
    background: var(--bg); color: var(--text);
    font-family: var(--font-body);
    display: flex; flex-direction: column;
    border: 1px solid var(--divider);
  }

  .titlebar {
    height: 44px; flex: none; display: flex; align-items: center;
    justify-content: flex-end;
  }
  .winbtn {
    width: 46px; height: 44px; display: flex; align-items: center;
    justify-content: center; color: var(--muted); background: none;
    border: none; cursor: pointer;
  }
  .winbtn:hover { background: var(--accent); color: #fff; }

  .stage { flex: 1; padding: 40px 64px 0; }
  .step { display: none; }
  .step.active { display: block; }

  .mark { width: 13px; height: 13px; background: var(--accent); margin-bottom: 30px; }
  .kicker {
    font-size: 11px; letter-spacing: 0.18em; font-weight: 700;
    color: var(--muted); margin-bottom: 16px;
  }
  h1 {
    font-family: var(--font-heading); font-weight: 700; font-size: 56px;
    letter-spacing: -0.03em; margin: 0; line-height: 1.05;
  }
  .subhead {
    font-family: var(--font-heading); font-weight: 700; font-size: 26px;
    color: var(--muted); margin-top: 6px; letter-spacing: -0.01em;
  }
  .rule { width: 420px; height: 1px; background: var(--divider); margin: 30px 0 26px; }
  .body-copy { font-size: 14px; line-height: 1.65; max-width: 460px; color: var(--text); }
  .body-copy strong { font-weight: 700; }

  .btn {
    font-family: var(--font-heading); font-weight: 700; font-size: 13px;
    cursor: pointer; border: 1px solid var(--divider); background: none;
    color: var(--text); padding: 11px 20px;
  }
  .btn:disabled { opacity: 0.5; cursor: default; }
  .btn-primary { background: var(--accent); color: #fff; border-color: var(--accent); }
  .cta { margin-top: 34px; }

  .keyrow { display: flex; align-items: center; gap: 8px; margin-top: 4px; }
  input[type="text"], input[type="password"] {
    width: 340px; font-size: 13px; padding: 11px 13px; background: var(--surface);
    color: var(--text); border: 1px solid var(--divider); font-family: var(--font-body);
  }
  input.invalid { border-color: var(--accent); }
  .icon-btn { padding: 11px 12px; }

  .error {
    display: none; align-items: center; gap: 7px; margin-top: 12px;
    font-size: 12px; color: var(--accent);
  }
  .error.shown { display: flex; }
  .getkey { margin-top: 12px; font-size: 12px; color: var(--muted); }
  .getkey a { color: var(--accent); font-weight: 700; }

  .footer {
    height: 46px; flex: none;
    border-top: 1px solid color-mix(in srgb, var(--text) 14%, transparent);
    display: flex; align-items: center; justify-content: space-between;
    padding: 0 64px; font-size: 12px; color: var(--muted);
  }
  .dots { display: flex; gap: 7px; }
  .dot { width: 17px; height: 3px; background: var(--divider); }
  .dot.on { background: var(--accent); }
</style>
</head>
<body>
  <div class="titlebar" data-tauri-drag-region>
    <button class="winbtn" id="close" title="Close">
      <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.4"><path d="M1.5 1.5l9 9M10.5 1.5l-9 9"/></svg>
    </button>
  </div>

  <div class="stage">
    <section class="step active" id="step1">
      <div class="mark"></div>
      <h1>WhisperOSS</h1>
      <div class="subhead">Speak anywhere.</div>
      <div class="rule"></div>
      <div class="body-copy">
        Hold <strong>Ctrl + Win</strong> in any app, say what you mean, and release.
        Your words are typed straight into whatever had the cursor.
      </div>
      <button class="btn btn-primary cta" id="get-started">Get started</button>
    </section>

    <section class="step" id="step2">
      <div class="kicker">CONNECT GROQ</div>
      <h1>Add your API key</h1>
      <div class="body-copy" style="margin-top:14px">
        Transcription runs on Groq. The key stays on this PC and is used for
        nothing else.
      </div>
      <div class="rule"></div>
      <div class="keyrow">
        <input type="password" id="api-key" placeholder="gsk_…" />
        <button class="btn icon-btn" id="toggle-key" title="Show key">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M2 12s3.6-6.5 10-6.5S22 12 22 12s-3.6 6.5-10 6.5S2 12 2 12z"/><circle cx="12" cy="12" r="2.6"/></svg>
        </button>
        <button class="btn btn-primary" id="validate">Validate &amp; finish</button>
      </div>
      <div class="error" id="key-error">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 3l9 16H3z"/><path d="M12 9v5"/><path d="M12 17h.01"/></svg>
        <span id="key-error-text"></span>
      </div>
      <div class="getkey">Get one at <a href="#" id="groq-link">console.groq.com/keys</a></div>
    </section>
  </div>

  <div class="footer">
    <div class="dots">
      <div class="dot on" id="dot1"></div>
      <div class="dot" id="dot2"></div>
    </div>
    <div id="stepcount">Step 1 of 2</div>
  </div>

  <script src="firstrun.js"></script>
</body>
</html>
```

- [ ] **Step 4: Verify**

Run `npm run tauri dev`. The window is registered but hidden, so nothing changes on screen yet — confirm only that the app still starts and the settings window still opens from the tray. `cargo check` → zero warnings.

- [ ] **Step 5: Commit**

```bash
git add src/firstrun.html src-tauri/tauri.conf.json src-tauri/capabilities/default.json
git commit -m "feat: first-run window shell and markup from the design"
```

---

### Task 3: Wire the first-run window

**Files:**
- Create: `src/firstrun.js`

The key is validated against Groq and saved by the existing `save_api_key` command, which returns `Ok(())` or a short error string. Reuse it — do not add a new command.

- [ ] **Step 1: Create `src/firstrun.js`:**

```js
const { invoke } = window.__TAURI__.core;
const { getCurrentWindow } = window.__TAURI__.window;
const { openUrl } = window.__TAURI__.opener;

const win = getCurrentWindow();
const el = (id) => document.getElementById(id);

// Closing without a key is allowed — the app keeps running in the tray and
// asks again the next time the hotkey is pressed.
el("close").onclick = () => win.hide();

function showStep(n) {
  el("step1").classList.toggle("active", n === 1);
  el("step2").classList.toggle("active", n === 2);
  el("dot1").classList.toggle("on", n === 1);
  el("dot2").classList.toggle("on", n === 2);
  el("stepcount").textContent = `Step ${n} of 2`;
  if (n === 2) el("api-key").focus();
}

el("get-started").onclick = () => showStep(2);

el("toggle-key").onclick = () => {
  const input = el("api-key");
  input.type = input.type === "password" ? "text" : "password";
};

el("groq-link").onclick = (e) => {
  e.preventDefault();
  openUrl("https://console.groq.com/keys");
};

function setError(text) {
  el("key-error").classList.toggle("shown", Boolean(text));
  el("key-error-text").textContent = text || "";
  el("api-key").classList.toggle("invalid", Boolean(text));
}

async function validate() {
  const key = el("api-key").value.trim();
  if (!key) { setError("Enter a key"); return; }
  setError("");
  el("validate").disabled = true;
  el("validate").textContent = "Checking…";
  try {
    await invoke("save_api_key", { key });
    win.hide();
  } catch (msg) {
    setError(String(msg));
  } finally {
    el("validate").disabled = false;
    el("validate").textContent = "Validate & finish";
  }
}

el("validate").onclick = validate;
el("api-key").addEventListener("keydown", (e) => {
  if (e.key === "Enter") validate();
});
```

- [ ] **Step 2: Verify**

Temporarily make the window visible to test it: in `src-tauri/tauri.conf.json`, set the `firstrun` window's `"visible"` to `true`, run `npm run tauri dev`, and check:

- Step 1 renders as in the design, "Get started" advances to step 2, and the footer dots and "Step 2 of 2" update.
- A deliberately wrong key (e.g. `gsk_notarealkey`) shows the red border and the inline message.
- Clicking the console.groq.com link opens it in your browser.
- The ✕ closes the window and the app keeps running.

**Then set `"visible"` back to `false` before committing.** Task 4 is what decides when it appears.

- [ ] **Step 3: Commit**

```bash
git add src/firstrun.js
git commit -m "feat: first-run steps, key validation and inline errors"
```

---

### Task 4: Show first run when there is no key

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/pipeline.rs`

Two entry points. At startup with no key saved, the window opens by itself. And if the user closes it without adding a key, pressing the hotkey brings it back instead of silently doing nothing — which is exactly how the app behaves today, and it reads as broken.

- [ ] **Step 1: Add a helper in `src-tauri/src/lib.rs`,** next to the existing `pub(crate) fn show_settings`:

```rust
pub(crate) fn show_first_run(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("firstrun") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}
```

- [ ] **Step 2: Open it at startup.** In `src-tauri/src/lib.rs`, replace this block:

```rust
            if key.is_empty() {
                applog::log("pipeline-started-without-key");
            }
```

with:

```rust
            if key.is_empty() {
                applog::log("first-run-no-key");
                show_first_run(app.handle());
            }
```

- [ ] **Step 3: Reopen it from the hotkey.** In `src-tauri/src/pipeline.rs`, inside the `hotkey_logic::Action::Start` arm, insert this as the **first** thing in the arm — before the generation counter is bumped and before the mic health check:

```rust
                    if state.key.lock().unwrap().is_empty() {
                        applog::log("recording-refused-no-key");
                        crate::show_first_run(&app);
                        continue;
                    }
```

(Refusing here rather than at the end means no pointless recording and no wasted network round-trip that could only ever come back as "Invalid API key".)

- [ ] **Step 4: Verify**

Run: `cargo test` → **42 passed**. `cargo check` → zero warnings.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/pipeline.rs
git commit -m "feat: first run opens when no API key is saved"
```

---

### Task 5: Name the executable WhisperOSS.exe

**Files:**
- Modify: `src-tauri/tauri.conf.json`

Milestone 4a renamed the crate to `whispeross`, so the binary is `whispeross.exe` — lowercase, visible in Task Manager. Tauri can name the binary independently of the crate.

- [ ] **Step 1:** In `src-tauri/tauri.conf.json`, add this key at the top level, immediately after `"identifier"`:

```json
  "mainBinaryName": "WhisperOSS",
```

- [ ] **Step 2: Verify**

Run `npm run tauri build`, then:

```powershell
Get-ChildItem "src-tauri\target\release\WhisperOSS.exe" | Select-Object Name, Length
Get-ChildItem "src-tauri\target\release\bundle\nsis\*.exe" | Select-Object Name, Length
```

Expected: `WhisperOSS.exe` exists, and a fresh installer is produced. Report both names and sizes.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tauri.conf.json
git commit -m "chore: name the executable WhisperOSS.exe"
```

---

### Task 6: Human verification

- [ ] **Step 1: Report this protocol and STOP.** The human runs it; wait for PASS/FAIL before writing the report.

This milestone must be tested on a machine state with **no API key saved**, which means temporarily removing the real one. It is restored in the last step.

**Before starting:** quit WhisperOSS from the tray and stop any `npm run tauri dev` session.

1. **Remove the saved key.** Open Windows Credential Manager (`control /name Microsoft.CredentialManager` → Windows Credentials), find the WhisperOSS entry, and note it exists before removing it. Report its exact display name.
2. **Install the new build** from `src-tauri\target\release\bundle\nsis\`, then launch WhisperOSS from the Start menu.
3. **First run appears by itself.** The welcome card opens, centred, showing the orange mark, "WhisperOSS", "Speak anywhere.", the Ctrl+Win explanation, and "Get started". The footer reads "Step 1 of 2" with the first dash highlighted.
4. **Typography.** The text is Archivo — geometric, tight, clearly not Segoe UI. Compare against `docs/design/art3-firstrun.png`; report any visible difference in size, spacing or weight.
5. **Step 2.** "Get started" moves to the key screen. Footer reads "Step 2 of 2" with the second dash highlighted.
6. **A bad key is refused.** Type `gsk_notarealkey` and click Validate & finish. The field turns red and an inline message appears. The window stays open.
7. **The link works.** "console.groq.com/keys" opens in your browser.
8. **A good key finishes.** Paste your real Groq key and click Validate & finish. The window closes on its own.
9. **The app is immediately usable.** Hold Ctrl+Win over a text field, speak, release. Text is pasted — with no restart.
10. **Settings agree.** Open settings from the tray; the API key row reads "Saved".
11. **Closing first run does not strand you.** Quit from the tray. Remove the key from Credential Manager again. Launch from the Start menu, and close the first-run card with ✕. Now press Ctrl+Win over a text field: the first-run card must come back instead of nothing happening.
12. **Restore.** Add your real key through the first-run card so the app is left working.
13. **Executable name.** With the app running:

```powershell
Get-Process | Where-Object { $_.ProcessName -like "*hisper*" } | Select-Object ProcessName, Id
```

Expected: `WhisperOSS`, not `whispeross`.

14. **Dark and light.** In settings, switch Theme to Light, then Dark. Reopen the first-run card if convenient (or trust the settings window) and confirm both themes look right — the design has both.

- [ ] **Step 2: Write `docs/reports/milestone-4b-results.md`** in the same shape as `docs/reports/milestone-4a-results.md`: one row per check with PASS/FAIL and what was observed, the bundled font's file size, the test count, the DEVIATIONS list, and a GO / NO-GO verdict for Milestone 5.

- [ ] **Step 3: Commit**

```bash
git add docs/reports/milestone-4b-results.md
git commit -m "docs: milestone 4b results"
```

---

## What comes next (not this plan)

**Milestone 5 — Hardening.** The full error matrix from spec §6 (including "invalid key opens settings to the key field", which is specified but not implemented), acrylic/blur on the overlay pill, a mic hot-plug pass, a performance pass against the spec's success criteria (cold start under 1.5 s, hold-to-bars under 100 ms), and a clipboard snapshot that preserves more than plain text.
