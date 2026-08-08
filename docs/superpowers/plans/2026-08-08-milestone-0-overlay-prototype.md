# Milestone 0 — Overlay Prototype Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove the riskiest assumption in the WhisperOSS v2 architecture: a transparent, always-on-top, click-through Tauri webview window can render the 12-bar audio pill at 60 fps from a 30 Hz Rust event stream, positioned bottom-center above the taskbar, with click-through toggling on and off correctly.

**Architecture:** Scaffold the real Tauri 2 app (this is not throwaway — later milestones build on it). A Rust thread emits synthetic audio levels at 30 Hz and flips click-through every 5 s; a single frameless webview window ("overlay") renders the pill with plain HTML/CSS/JS, no framework, no bundler. Position math is a pure Rust module with unit tests.

**Tech Stack:** Tauri 2.x (Rust stable, `windows` target), vanilla JS frontend via `withGlobalTauri` (no npm runtime deps, no build step), `cargo test` for Rust units.

**Spec:** `docs/superpowers/specs/2026-08-08-whispeross-v2-design.md` (§3 overlay, §8 milestone 0). Design reference: `docs/design/art2-pill.png`.

## Global Constraints

- Windows-only. Repo root: `C:\projects (code)\15. WhisperOSS redefined` (note the spaces — always quote paths).
- Shell: `cd`, `git`, `npm`, and `cargo` commands work as written in both PowerShell and Git Bash. Where file operations differ (Task 1 Step 2), both variants are given — use the one matching your shell.
- `src-reference/` is v1 Python. Never modify it, never import from it, never commit it.
- Pill: **120×36 logical px**, bottom-center, **26 logical px above the taskbar** (work area bottom), on the monitor containing the cursor. DPI-aware: physical = logical × scale factor.
- Overlay window: transparent, no decorations, no shadow, always-on-top, skip-taskbar, never takes focus.
- Colors (from `docs/design/claude-design-export/_ds/modernist-004a60a8-d2ba-445b-8715-9131d459e452/styles.css`): pill body near-black `#0b0a0a`, bars `#f3f2f2`, accent `#ec3013`, success green `#3fb970`.
- No network calls anywhere in this milestone. No extra crates beyond what the Tauri template generates.
- Frontend is plain HTML/CSS/JS. No React/Vue/bundler — the design export is plain HTML and later milestones drop it in directly.
- Gate criteria (Task 6): 1-second-average fps ≥ 58 with minimum ≥ 45 over a 60 s run, and click-through verified in both states.

---

### Task 1: Scaffold the Tauri app

**Files:**
- Create (via generator): `package.json`, `src/index.html`, `src/main.js`, `src/styles.css`, `src-tauri/` (Cargo project, `tauri.conf.json`, `capabilities/default.json`, `icons/`)
- Modify: `src-tauri/tauri.conf.json` (identifier + product name)

**Interfaces:**
- Consumes: nothing (first task).
- Produces: a running `npm run tauri dev` app; `src-tauri/src/lib.rs` with a `run()` function that later tasks extend; frontend served from `src/` with `withGlobalTauri` available.

- [ ] **Step 1: Generate the project into a temp folder**

```bash
cd "C:\projects (code)\15. WhisperOSS redefined"
npm create tauri-app@latest scaffold-tmp -- --template vanilla --manager npm --yes
```

- [ ] **Step 2: Move it into the repo root, keeping our .gitignore**

PowerShell:

```powershell
Remove-Item scaffold-tmp\.gitignore
Copy-Item -Path "scaffold-tmp\*" -Destination . -Recurse -Force
Remove-Item -Recurse -Force scaffold-tmp
```

Or in Git Bash: `rm scaffold-tmp/.gitignore && cp -r scaffold-tmp/. . && rm -rf scaffold-tmp`

The vanilla template serves the frontend straight from a source folder (`frontendDist` pointing at it in `tauri.conf.json`) with no build step. If the generated layout differs from the paths this plan uses (`src/index.html`, `src/main.js`), keep the template's layout and use its paths consistently in Tasks 3–5 — the file *contents* in this plan are what matters.

- [ ] **Step 3: Set identity in `src-tauri/tauri.conf.json`**

Edit these two fields (leave the rest as generated):

```json
{
  "productName": "WhisperOSS",
  "identifier": "com.whispeross.app"
}
```

Also ensure `"app"` contains `"withGlobalTauri": true` (add it if the template didn't):

```json
"app": {
  "withGlobalTauri": true
}
```

- [ ] **Step 4: Verify the dev app runs**

```bash
npm install
npm run tauri dev
```

Expected: first run compiles Rust for several minutes, then the template's default window opens and renders. Close it.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: scaffold Tauri 2 app (vanilla frontend, no bundler)"
```

---

### Task 2: Pill position math (pure Rust, TDD)

**Files:**
- Create: `src-tauri/src/position.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod position;`)

**Interfaces:**
- Consumes: nothing.
- Produces: `position::Rect { x: i32, y: i32, width: u32, height: u32 }`, `position::pill_position(work_area: Rect, scale: f64) -> (i32, i32)` (top-left in physical px), `position::contains(r: Rect, px: f64, py: f64) -> bool`. Task 3 calls all three.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/position.rs` containing ONLY the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centres_pill_at_100_percent_scale() {
        // 1920x1040 work area (1080 minus a 40px taskbar):
        // pill 120x36, margin 26 → x=(1920-120)/2=900, y=1040-36-26=978
        let wa = Rect { x: 0, y: 0, width: 1920, height: 1040 };
        assert_eq!(pill_position(wa, 1.0), (900, 978));
    }

    #[test]
    fn scales_pill_and_margin_at_150_percent() {
        // pill 180x54, margin 39 → x=(2560-180)/2=1190, y=1352-54-39=1259
        let wa = Rect { x: 0, y: 0, width: 2560, height: 1352 };
        assert_eq!(pill_position(wa, 1.5), (1190, 1259));
    }

    #[test]
    fn respects_monitor_origin_offset() {
        // second monitor to the right and slightly lower
        let wa = Rect { x: 1920, y: 200, width: 1920, height: 1040 };
        assert_eq!(pill_position(wa, 1.0), (2820, 1178));
    }

    #[test]
    fn contains_point_inside_and_outside() {
        let r = Rect { x: 10, y: 10, width: 100, height: 50 };
        assert!(contains(r, 10.0, 10.0));
        assert!(contains(r, 109.9, 59.9));
        assert!(!contains(r, 110.0, 60.0));
        assert!(!contains(r, 5.0, 30.0));
    }
}
```

And add to `src-tauri/src/lib.rs`, above the existing code:

```rust
mod position;
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd "C:\projects (code)\15. WhisperOSS redefined\src-tauri"
cargo test position
```

Expected: FAIL to compile — `Rect`, `pill_position`, `contains` not found.

- [ ] **Step 3: Implement the module**

Add above the test module in `src-tauri/src/position.rs`:

```rust
//! Pill placement math. All inputs/outputs are PHYSICAL pixels; the spec's
//! sizes (pill 120x36, margin 26) are logical and get multiplied by the
//! monitor scale factor here.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub const PILL_LOGICAL_W: f64 = 120.0;
pub const PILL_LOGICAL_H: f64 = 36.0;
pub const TASKBAR_MARGIN_LOGICAL: f64 = 26.0;

/// Top-left position for the pill: horizontally centred in `work_area`,
/// bottom edge `26 * scale` px above the work-area bottom (the work area
/// already excludes the taskbar).
pub fn pill_position(work_area: Rect, scale: f64) -> (i32, i32) {
    let pw = (PILL_LOGICAL_W * scale).round() as i32;
    let ph = (PILL_LOGICAL_H * scale).round() as i32;
    let margin = (TASKBAR_MARGIN_LOGICAL * scale).round() as i32;
    let x = work_area.x + (work_area.width as i32 - pw) / 2;
    let y = work_area.y + work_area.height as i32 - ph - margin;
    (x, y)
}

/// Half-open containment: the right/bottom edges are outside.
pub fn contains(r: Rect, px: f64, py: f64) -> bool {
    px >= r.x as f64
        && py >= r.y as f64
        && px < r.x as f64 + r.width as f64
        && py < r.y as f64 + r.height as f64
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test position
```

Expected: `4 passed`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/position.rs src-tauri/src/lib.rs
git commit -m "feat: pill position math with DPI scaling (tested)"
```

---

### Task 3: Overlay window — config and runtime placement

**Files:**
- Modify: `src-tauri/tauri.conf.json` (replace the windows array)
- Modify: `src-tauri/capabilities/default.json` (window label)
- Modify: `src-tauri/src/lib.rs` (position the window at startup)

**Interfaces:**
- Consumes: `position::{Rect, pill_position, contains}` from Task 2.
- Produces: a window labeled `"overlay"` that Tasks 4–5 target via `app.get_webview_window("overlay")`; a `position_overlay(app: &tauri::AppHandle) -> tauri::Result<()>` function in `lib.rs`.

- [ ] **Step 1: Replace the windows array in `src-tauri/tauri.conf.json`**

Inside `"app"`, replace the existing `"windows"` entry entirely with:

```json
"windows": [
  {
    "label": "overlay",
    "title": "WhisperOSS Overlay",
    "width": 120,
    "height": 36,
    "resizable": false,
    "maximizable": false,
    "minimizable": false,
    "decorations": false,
    "transparent": true,
    "shadow": false,
    "alwaysOnTop": true,
    "skipTaskbar": true,
    "focus": false,
    "visible": true
  }
]
```

- [ ] **Step 2: Point the capability file at the new label**

In `src-tauri/capabilities/default.json`, set the windows list to the new label (the generated `"core:default"` permission set already allows event listening — leave permissions as generated):

```json
"windows": ["overlay"]
```

- [ ] **Step 3: Position the window at startup in `src-tauri/src/lib.rs`**

Replace the file's contents with (keeping `mod position;` from Task 2):

```rust
mod position;

use tauri::{Manager, PhysicalPosition};

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            position_overlay(app.handle())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Move the overlay to bottom-center of the monitor the cursor is on,
/// 26 logical px above the taskbar.
fn position_overlay(app: &tauri::AppHandle) -> tauri::Result<()> {
    let window = app
        .get_webview_window("overlay")
        .expect("overlay window missing from tauri.conf.json");

    let cursor = app.cursor_position()?;
    let monitor = app
        .available_monitors()?
        .into_iter()
        .find(|m| {
            let pos = m.position();
            let size = m.size();
            let bounds = position::Rect {
                x: pos.x,
                y: pos.y,
                width: size.width,
                height: size.height,
            };
            position::contains(bounds, cursor.x, cursor.y)
        })
        .or_else(|| app.primary_monitor().ok().flatten())
        .expect("no monitor found");

    let wa = monitor.work_area();
    let work_area = position::Rect {
        x: wa.position.x,
        y: wa.position.y,
        width: wa.size.width,
        height: wa.size.height,
    };
    let (x, y) = position::pill_position(work_area, monitor.scale_factor());
    window.set_position(PhysicalPosition::new(x, y))?;
    Ok(())
}
```

Note: `Monitor::work_area()` exists on tauri ≥ 2.2 (the default `tauri = "2"` dependency resolves well past that). If the compiler disagrees with the field access shape, check `Monitor` on docs.rs for the resolved version and adapt only the three lines reading `wa` — the math stays in `position.rs`.

- [ ] **Step 4: Verify by running**

```bash
cd "C:\projects (code)\15. WhisperOSS redefined"
npm run tauri dev
```

Expected: a small 120×36 window appears bottom-center of your current monitor, just above the taskbar, with no title bar, no taskbar entry, and stays above other windows. (It still shows template content — that's Task 4.) `cargo test` still passes.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/tauri.conf.json src-tauri/capabilities/default.json src-tauri/src/lib.rs
git commit -m "feat: frameless transparent overlay window placed above taskbar"
```

---

### Task 4: Pill UI — 12 bars at 60 fps from a 30 Hz Rust level stream

**Files:**
- Create: `src-tauri/src/demo.rs`
- Modify: `src-tauri/src/lib.rs` (spawn the demo thread)
- Replace: `src/index.html`, `src/main.js`
- Delete: `src/styles.css` and any template assets (e.g. `src/assets/`)

**Interfaces:**
- Consumes: the `"overlay"` window from Task 3.
- Produces: Rust events `"level"` (payload `f64`, 0.0–1.0, 30 Hz) consumed by the frontend; `demo::spawn_demo(app: tauri::AppHandle)` called from `lib.rs`; `demo::synth_level(t: f64, jitter: f64) -> f64` (pure, tested). Task 5 extends `demo.rs`.

- [ ] **Step 1: Write the failing test**

Create `src-tauri/src/demo.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synth_level_stays_in_unit_range() {
        let mut rng = 1u32;
        for i in 0..10_000 {
            let j = lcg_next(&mut rng);
            let v = synth_level(i as f64 * 0.033, j);
            assert!((0.0..=1.0).contains(&v), "out of range at {i}: {v}");
        }
    }
}
```

Add `mod demo;` to the top of `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run test to verify it fails**

```bash
cd "C:\projects (code)\15. WhisperOSS redefined\src-tauri"
cargo test demo
```

Expected: FAIL to compile — `lcg_next`, `synth_level` not found.

- [ ] **Step 3: Implement the synthetic level driver**

Add above the test module in `src-tauri/src/demo.rs`:

```rust
//! Milestone-0 synthetic driver: emits fake mic levels so the overlay can be
//! validated without any audio code. Replaced by the real recorder in M2.

use std::{thread, time::Duration};
use tauri::{AppHandle, Emitter};

/// Deterministic pseudo-random 0.0..1.0 (no rand dependency).
fn lcg_next(state: &mut u32) -> f64 {
    *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*state >> 8) as f64 / 16_777_216.0
}

/// Synthetic mic level: slow sine envelope plus jitter, clamped to 0..=1.
pub fn synth_level(t: f64, jitter: f64) -> f64 {
    let envelope = 0.35 + 0.35 * (t * 1.7).sin() + 0.2 * (t * 0.4).sin();
    (envelope + 0.25 * (jitter - 0.5)).clamp(0.0, 1.0)
}

/// Emit `"level"` events at 30 Hz forever.
pub fn spawn_demo(app: AppHandle) {
    thread::spawn(move || {
        let mut t = 0.0_f64;
        let mut rng = 0x2026_0808_u32;
        loop {
            t += 1.0 / 30.0;
            let level = synth_level(t, lcg_next(&mut rng));
            let _ = app.emit("level", level);
            thread::sleep(Duration::from_millis(33));
        }
    });
}
```

In `src-tauri/src/lib.rs`, extend the setup closure:

```rust
        .setup(|app| {
            position_overlay(app.handle())?;
            demo::spawn_demo(app.handle().clone());
            Ok(())
        })
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test demo
```

Expected: `1 passed`.

- [ ] **Step 5: Replace the frontend with the pill**

Delete `src/styles.css` and any `src/assets/` folder. Replace `src/index.html` with:

```html
<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8" />
<style>
  html, body { margin: 0; background: transparent; overflow: hidden;
               -webkit-user-select: none; user-select: none; cursor: default; }
  .pill {
    position: fixed; inset: 0;
    background: #0b0a0a;
    border-radius: 18px;
    display: flex; align-items: center; justify-content: center; gap: 3px;
  }
  .bar {
    width: 3px; height: 24px; border-radius: 1.5px;
    background: #f3f2f2;
    transform: scaleY(0.16); transform-origin: center;
    will-change: transform;
  }
  .dot {
    position: absolute; right: 6px; top: 6px;
    width: 4px; height: 4px; border-radius: 50%;
    background: #3fb970;
  }
  .fps {
    position: absolute; left: 6px; top: 3px;
    font: 8px monospace; color: #7d7979;
  }
</style>
</head>
<body>
  <div class="pill">
    <div class="fps" id="fps"></div>
    <div class="dot" id="dot"></div>
  </div>
  <script src="main.js"></script>
</body>
</html>
```

Replace `src/main.js` with:

```js
const { listen } = window.__TAURI__.event;

// --- bars -------------------------------------------------------------
const pill = document.querySelector(".pill");
const BAR_COUNT = 12;
const bars = [];
for (let i = 0; i < BAR_COUNT; i++) {
  const b = document.createElement("div");
  b.className = "bar";
  pill.appendChild(b);
  bars.push(b);
}

// Centre-emphasis: middle bars react more than edge bars.
const weights = bars.map((_, i) => {
  const d = Math.abs(i - (BAR_COUNT - 1) / 2) / ((BAR_COUNT - 1) / 2);
  return 0.35 + 0.65 * Math.cos((d * Math.PI) / 2);
});

// --- events from Rust -------------------------------------------------
let target = 0; // latest level from Rust, 0..1
listen("level", (e) => { target = e.payload; });

const dot = document.getElementById("dot");
listen("clickthrough", (e) => {
  // green = clicks pass through the pill, red = pill catches clicks
  dot.style.background = e.payload ? "#3fb970" : "#ec3013";
});
document.addEventListener("click", () =>
  console.log("PILL CLICKED — click-through is OFF"));

// --- fps meter --------------------------------------------------------
const fpsEl = document.getElementById("fps");
let frames = 0, last = performance.now(), minFps = Infinity;
setInterval(() => {
  const now = performance.now();
  const fps = (frames * 1000) / (now - last);
  minFps = Math.min(minFps, fps);
  fpsEl.textContent = `${fps.toFixed(0)}/${minFps.toFixed(0)}`;
  console.log(`fps avg(1s)=${fps.toFixed(1)} min=${minFps.toFixed(1)}`);
  frames = 0; last = now;
}, 1000);

// --- 60 fps animation loop -------------------------------------------
let smoothed = 0, t = 0;
function frame() {
  frames++;
  t += 1 / 60;
  // fast attack, slow decay — matches how the real meter should feel
  smoothed = target > smoothed
    ? smoothed + (target - smoothed) * 0.5
    : smoothed + (target - smoothed) * 0.12;
  const MIN = 0.16; // resting scaleY (≈4px of the 24px bar)
  bars.forEach((b, i) => {
    const wobble = 0.85 + 0.15 * Math.sin(t * 7 + i * 1.7);
    const s = MIN + (1 - MIN) * smoothed * weights[i] * wobble;
    b.style.transform = `scaleY(${s.toFixed(3)})`;
  });
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
```

- [ ] **Step 6: Verify by running**

```bash
cd "C:\projects (code)\15. WhisperOSS redefined"
npm run tauri dev
```

Expected: the black pill bottom-center with 12 white bars dancing smoothly (center bars taller), rounded corners with the desktop visible around them, a tiny fps readout in the pill's corner showing ~60, and `fps avg(1s)=…` lines in the webview devtools console (right-click pill → Inspect if click-through is off; devtools open with the window in dev builds).

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: 12-bar pill at 60fps driven by 30Hz Rust level events"
```

---

### Task 5: Click-through auto-toggle

**Files:**
- Modify: `src-tauri/src/demo.rs` (toggle click-through inside the demo loop)

**Interfaces:**
- Consumes: the `"overlay"` window (Task 3); the frontend `"clickthrough"` listener and dot (Task 4).
- Produces: Rust event `"clickthrough"` (payload `bool`, every 5 s); the OS-level input passthrough state actually flipping.

- [ ] **Step 1: Extend the demo loop**

Replace `spawn_demo` in `src-tauri/src/demo.rs` with (imports change too — full header shown):

```rust
use std::{thread, time::Duration};
use tauri::{AppHandle, Emitter, Manager};
```

```rust
/// Emit `"level"` at 30 Hz forever, and every 5 s flip the overlay between
/// click-through (clicks land on the window behind) and clickable.
pub fn spawn_demo(app: AppHandle) {
    thread::spawn(move || {
        let mut t = 0.0_f64;
        let mut rng = 0x2026_0808_u32;
        let mut click_through = true;
        let mut ticks: u64 = 0;

        if let Some(w) = app.get_webview_window("overlay") {
            let _ = w.set_ignore_cursor_events(click_through);
        }
        let _ = app.emit("clickthrough", click_through);

        loop {
            t += 1.0 / 30.0;
            ticks += 1;
            let level = synth_level(t, lcg_next(&mut rng));
            let _ = app.emit("level", level);

            if ticks % 150 == 0 {
                // 150 ticks at 30 Hz = 5 s
                click_through = !click_through;
                if let Some(w) = app.get_webview_window("overlay") {
                    let _ = w.set_ignore_cursor_events(click_through);
                }
                let _ = app.emit("clickthrough", click_through);
            }
            thread::sleep(Duration::from_millis(33));
        }
    });
}
```

- [ ] **Step 2: Verify the tests still pass**

```bash
cd "C:\projects (code)\15. WhisperOSS redefined\src-tauri"
cargo test
```

Expected: `5 passed` (4 position + 1 demo).

- [ ] **Step 3: Manual click-through verification**

```bash
cd "C:\projects (code)\15. WhisperOSS redefined"
npm run tauri dev
```

Protocol — do both phases:
1. Open Notepad maximized behind the pill. Put the cursor over the pill.
2. **Green dot phase** (click-through ON): click on the pill. Expected: the click lands in Notepad (caret moves/Notepad focuses); no "PILL CLICKED" console line.
3. **Red dot phase** (click-through OFF, within 5 s): click on the pill. Expected: "PILL CLICKED — click-through is OFF" appears in the pill's devtools console; Notepad does NOT receive the click.
4. Watch two full toggles to confirm it flips both ways repeatedly.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/demo.rs
git commit -m "feat: click-through auto-toggle every 5s with visual indicator"
```

---

### Task 6: Gate measurement and report

**Files:**
- Create: `docs/reports/milestone-0-gate.md`

**Interfaces:**
- Consumes: everything above.
- Produces: the recorded go/no-go decision the Milestone 1 plan depends on.

- [ ] **Step 1: Run the 60-second measurement**

```bash
cd "C:\projects (code)\15. WhisperOSS redefined"
npm run tauri dev
```

Let it run 60 s undisturbed with the bars animating, spanning at least 10 click-through toggles. Read the final `fps avg(1s)=… min=…` console line (min accumulates for the whole session). Then repeat once with a YouTube video playing behind the pill (load simulation) and note the numbers.

- [ ] **Step 2: Write the gate report**

Create `docs/reports/milestone-0-gate.md`:

```markdown
# Milestone 0 gate — overlay prototype

Date: <run date>
Machine: <CPU / GPU / Windows version / monitor setup incl. scale factors>

| Check | Target | Measured | Pass |
|---|---|---|---|
| fps, idle desktop, 60 s | avg ≥ 58, min ≥ 45 | avg __, min __ | __ |
| fps, video playing behind, 60 s | avg ≥ 58, min ≥ 45 | avg __, min __ | __ |
| Click-through ON: clicks reach app behind | yes | __ | __ |
| Click-through OFF: pill receives clicks | yes | __ | __ |
| Position: bottom-center, 26px above taskbar | yes | __ | __ |
| Transparent corners (no white/black box) | yes | __ | __ |

Verdict: GO / NO-GO for Milestone 1.
Notes: <anything observed — flicker, focus steals, positioning drift>
```

Fill every cell from the actual runs — no cell left as `__`.

- [ ] **Step 3: Commit**

```bash
git add docs/reports/milestone-0-gate.md
git commit -m "docs: milestone 0 gate results"
```

If any check fails, STOP — do not proceed to a Milestone 1 plan. Report the failure; fallback options (native-drawn overlay, different window flags) are an architecture conversation, not a workaround to improvise.
