# WhisperOSS v2 — Design Spec

Date: 2026-08-08
Status: Approved scope + approved visual design; awaiting implementation plan.
Reference code: `src-reference/` (v1, Python/PyQt6 — git-ignored, local only).

## 1. Product

A Windows desktop dictation app built with Tauri + Rust. One feature:

> Hold the hotkey (default **Ctrl+Win**) anywhere in Windows, speak, release —
> the transcribed text is typed into whatever app had the cursor.

The app lives in the system tray. Goals, in order: exceptional GUI, reliability,
speed (startup and every interaction).

## 2. Scope

### In scope
- Hold-to-dictate pipeline with 0.5 s pre-roll (audio from before the key press
  is captured, so the first word is never clipped). Mic stream is always on.
- Privacy paste: transcript is placed on the clipboard flagged so it never
  enters Win+V clipboard history or the cloud clipboard; Ctrl+V is injected;
  the user's original clipboard is restored afterward.
- Optional AI cleanup, off by default: **AI formatting** toggle
  (punctuation/paragraphs) and **Casual mode** toggle (lowercase, light emoji).
- Configurable hotkey (default Ctrl+Win), rebindable from settings.
- Settings window with exactly seven controls (see §4).
- First-run flow: welcome → API key entry with live validation.
- System tray (Show settings / Quit); closing the window hides to tray.
- Start with Windows (per-user registry Run key), toggleable.
- Groq API key stored in Windows Credential Manager, never in a plain file.

### Out of scope (deliberately killed from v1)
Quick Answer mode, Visual Search mode, Gemini integration, translation,
screen snipping, answer cards, web search, model-picker dropdowns,
animation-FPS setting, always-listening toggle (now permanently on).

### Non-goals for v2.0 (future candidates)
macOS/Linux, local (offline) transcription, dictation history, per-app profiles.

## 3. Visual design — authoritative source

The approved design lives in this repo:

- `docs/design/claude-design-export/WhisperOSS.dc.html` — full design source
  (all three artboards; open in a browser). Uses
  `_ds/modernist-…/styles.css` (design tokens) and `support.js` (canvas viewer).
- `docs/design/art1-settings.png`, `art2-pill.png`, `art3-firstrun.png`,
  `whispeross-design-full.png` — screenshots of the approved state.
- Editable original: https://claude.ai/design/p/f27cb614-5084-4d3f-a48f-ddc27db44eed

Design system ("Modernist"): Archivo typeface, accent `#ec3013`, square corners
(`--radius-*: 0`), light surface `#f3f2f2` / near-black dark theme, hard
uppercase micro-labels. Both themes are specified for every surface.

**Build notes:**
- The mockup loads Archivo from Google Fonts. The app must **bundle the font
  locally** — no network dependency for UI rendering.
- The design tokens in `styles.css` (colors, spacing, shadows) are the palette
  the real windows use; `support.js` is only the mockup viewer, not app code.

### Surfaces

**Settings window — 960×640, frameless, acrylic, the only real window.**
Header (brand + minimize/close) · hotkey hero ("Hold Ctrl + Win — speak —
release" + *Change hotkey* button) · then exactly seven controls:
1. Groq API key (masked field, show/hide, Save)
2. AI formatting (toggle)
3. Casual mode (toggle)
4. Microphone (dropdown + refresh)
5. Theme (Auto / Light / Dark segmented control)
6. Start with Windows (toggle)
7. *(Change hotkey counts as the seventh control, in the hero)*
Footer status line: "Groq connected · Microphone OK" + version.

**Overlay pill — 120×36, always on top, click-through, never takes focus.**
Bottom-center of the monitor the cursor is on, 26 px above the taskbar.
Five states with specified motion:
- **Listening** — 12 bars driven by live input level.
- **Processing** — "PROCESSING" with a shimmer sweeping left, 1.6 s loop.
- **Success** — check pops in, holds 400 ms, fades.
- **Error** — red pill with a 3–4 word message, shown 2 s, then fades.
- **Idle** — nothing on screen; exit fade is 240 ms.

**First-run — two steps in the same window shell.**
Step 1 welcome ("WhisperOSS — Speak anywhere." + Get started). Step 2 API key
entry: masked input, show/hide, "Validate & finish", link to
console.groq.com/keys, inline invalid-key state ("Groq rejected this key…").
Key is validated against the live API before the app proceeds.

## 4. Architecture

Two layers. Everything the user *feels* as latency is Rust; everything the user
*sees* is a webview window. The UI can crash or lag without affecting a
dictation in flight.

### Rust core (no UI)
- **Hotkey listener** — low-level keyboard hook (`SetWindowsHookEx` via
  `windows-rs`). Handles: hold detection, configurable combo, minimum hold of
  150 ms (shorter = accidental tap, ignored), release via real key-up events.
  No polling loops, no third-party hotkey library.
- **Audio** — `cpal` (WASAPI). Persistent capture stream at the device's native
  rate, mono, feeding a 0.5 s ring buffer. On dictation end: downsample to
  16 kHz locally, package as WAV in memory (no temp files). Device list
  refresh must not interrupt the live stream. If the chosen mic disappears,
  fall back to the Windows default device.
- **Groq client** — `reqwest` against Groq's REST API (no SDK).
  Transcription request: 15 s timeout, one automatic retry, cancellable.
  Starting a new dictation cancels any in-flight request. Optional second
  request for formatting/casual cleanup (same timeout rules).
- **Paste** — Win32 clipboard via `windows-rs`: snapshot current clipboard,
  set transcript with the three history-exclusion formats
  (`ExcludeClipboardContentFromMonitorProcessing`,
  `CanIncludeInClipboardHistory=0`, `CanUploadToCloudClipboard=0`),
  inject Ctrl+V, restore the snapshot after the paste is confirmed —
  sequenced, not on a timer. If the exclusion formats cannot be set, **abort
  the paste and show an error** (v1's rule: never leak into clipboard history).
- **Config** — JSON at `%APPDATA%\WhisperOSS\config.json`. Keys: hotkey
  (modifier + key names), `use_formatter`, `casual_mode`, `input_device`
  (stored by device *name*, not index — indexes shift between sessions),
  `theme`, `run_on_startup`. Unknown/missing keys merge onto defaults.
- **Secrets** — `keyring` crate → Windows Credential Manager, service
  "WhisperOSS". The API key never appears in `config.json`.
- **Autostart** — `HKCU\…\Run` value "WhisperOSS" (or `tauri-plugin-autostart`
  if it writes the same key). Reconciled with the toggle at startup.
- **Tray** — Tauri built-in. Menu: Show settings / Quit. Custom app icon
  (v1 shipped without one — v2 needs a real icon; not yet designed).

### Webview windows
- **Overlay** — transparent, always-on-top, click-through, skip-taskbar.
  Receives two event streams from Rust: state changes (listening / processing /
  success / error(message) / hidden) and audio level (~30 Hz while listening).
  Renders the pill per §3. Display-only: it sends nothing back and can never
  block the pipeline.
- **Settings** — the §3 settings window. Reads/writes config through Tauri
  commands. Hidden on close, shown from tray or when an invalid-key error
  demands attention.
- **First-run** — same window shell, shown only when no valid API key exists.

### Hardcoded (no UI, changed only via app releases)
- Transcription model: `whisper-large-v3-turbo`; language: English (as v1).
- Formatting model: Groq-hosted chat model, pinned in code at build time
  (v1 used `openai/gpt-oss-120b`; implementer verifies current best at build).
- Pre-roll 500 ms · min hold 150 ms · request timeout 15 s · 1 retry ·
  error display 2 s · overlay fade 240 ms.

## 5. Dictation pipeline

1. Hotkey held ≥150 ms → recording flag set (ring buffer already has the
   pre-roll) → overlay shows Listening with live bars.
2. Hotkey released → overlay shows Processing → audio downsampled, WAV
   packaged in memory.
3. If the recording is effectively silent (peak below a floor for its whole
   duration), drop it: overlay simply fades out. No upload, no error.
4. Transcribe via Groq. If formatting/casual is on, run the cleanup request.
5. Paste per §4 (snapshot → flagged set → Ctrl+V → sequenced restore).
6. Overlay shows Success (400 ms) → fades. Total feel target: text lands in
   ~1 s for a short sentence; the Groq round-trip is the only unavoidable wait.

Concurrency rule: one dictation at a time; a new hold cancels any in-flight
request and starts fresh. UI startup rule: windows become interactive before
any network call happens (v1 blocked launch on two network round-trips).

## 6. Error handling

Every failure surfaces as the red pill state for 2 s with a plain-language
message, then the app returns to idle, ready for the next dictation. No modal
dialogs, no permanently stuck states.

| Failure | Behavior |
|---|---|
| No microphone found | "No mic detected"; auto-fallback to Windows default when one appears |
| Network unreachable / timeout | one retry, then "Couldn't reach Groq" |
| API key rejected (401) | "Invalid API key" + settings window opens to the key field |
| Groq server error (5xx) | one retry, then "Groq error — try again" |
| Privacy clipboard flags can't be set | abort paste, "Couldn't paste safely" — never fall back to an unflagged paste |
| Mic vanishes mid-recording | finish with what was captured if >minimum, else error pill |

Diagnostics: a plain-text log (`%APPDATA%\WhisperOSS\log.txt`, size-capped,
no transcript contents — timestamps and event names only).

## 7. Testing

**Automated (Rust unit/integration):** config load/merge/migration, WAV
packaging + downsample correctness, silence detection, hotkey state machine
(hold/abort/rebind cases as pure logic), Groq client against a local mock
server (success, timeout, retry, 401, cancel).

**Manual release checklist** (Windows integrations where mocks would exceed
the code they test): paste into Notepad, a browser, Word, and a terminal;
hotkey recovery across lock screen and UAC prompts; overlay position and
crispness on multi-monitor mixed-DPI setups; mic hot-plug and disappearance;
close-to-tray, autostart, first-run flow end to end.

**Milestone 0 gate:** the click-through overlay prototype (below) must show
60 fps bars and correct click-through toggling before anything else is built.

## 8. Milestones

0. **Overlay prototype** — transparent, click-through, always-on-top Tauri
   window animating 12 bars at 60 fps from synthetic level data. The one real
   technical unknown; validates the architecture.
1. **Headless pipeline** — fixed Ctrl+Win, hotkey hook → record → transcribe →
   privacy paste, tray icon, no settings UI. The product works end to end.
2. **Overlay wired** — real states and live levels replace synthetic data.
3. **Settings** — window from the design export, config + Credential Manager,
   mic picker, theme, hotkey rebind, autostart toggle.
4. **First-run + polish** — onboarding flow, app icon, installer, acrylic.
5. **Hardening** — full error matrix, manual checklist pass, performance pass.

**Success criteria:** cold start to ready < 1.5 s · hold → visible bars
< 100 ms · release → pasted text bounded by the network round-trip + ~300 ms ·
no state in the app that requires killing the process to escape.
