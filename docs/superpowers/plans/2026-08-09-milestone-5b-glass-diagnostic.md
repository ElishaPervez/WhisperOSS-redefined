# Milestone 5b — Glass diagnostic: make the acrylic real or drop it

**Status:** Task 3 Step 4 failed exactly as its stop-condition anticipated: rounded corners applied, but the pill is uniformly dark over high-contrast backgrounds — no blur, and nothing behind the window shows through. Task 3 remains uncommitted. Tasks 1–2 are committed and untouched by this plan.

**Diagnosis.** The config option `windowEffects: ["acrylic"]` is implemented on Windows through an old, undocumented composition API. Recent Windows 11 builds (this machine runs the 26220 line) have been dropping support for that path — callers get a dark, unblurred backdrop, which is precisely what was observed. The supported replacement is the DWM *system backdrop* attribute (`DWMSBT_TRANSIENTWINDOW` — the acrylic used by the OS's own flyouts), which needs the frame extended into the client area to composite.

This plan tries the supported path first, the legacy path with an explicit tint second, and if neither produces blur, reverts to the solid pill. **A human looks at the pill after each attempt** — do not judge the blur yourself; ask, wait for the answer, then follow the branch.

## Global Constraints

- Same as the milestone plan. All **46** tests stay green, zero warnings. Never touch `src-reference\`.
- Start from the current uncommitted Task 3 state: keep its CSS changes (`src/index.html`) and the corner-preference block in `lib.rs` exactly as they are.

---

### Step 1: Remove the config effect

In `src-tauri/tauri.conf.json`, delete the line added by Task 3 Step 1:

```json
        "windowEffects": { "effects": ["acrylic"] }
```

It demonstrably does nothing on this build except darken the backdrop, and it would fight the attempts below.

### Step 2: Attempt A — the supported system backdrop

In `src-tauri/Cargo.toml`, add `"Win32_UI_Controls"` to the `windows` crate feature list (`MARGINS` lives there).

In `src-tauri/src/lib.rs`, inside the overlay block, extend the existing `if let Ok(handle) = w.hwnd()` body — after the corner-preference call, add:

```rust
                    use windows::Win32::Graphics::Dwm::{
                        DwmExtendFrameIntoClientArea, DWMWA_SYSTEMBACKDROP_TYPE,
                        DWMSBT_TRANSIENTWINDOW,
                    };
                    use windows::Win32::UI::Controls::MARGINS;
                    // The supported acrylic: the same backdrop Windows' own
                    // flyouts use. It only composites when the frame is
                    // extended across the whole client area.
                    let margins = MARGINS {
                        cxLeftWidth: -1,
                        cxRightWidth: -1,
                        cyTopHeight: -1,
                        cyBottomHeight: -1,
                    };
                    unsafe {
                        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
                        let backdrop = DWMSBT_TRANSIENTWINDOW;
                        let _ = DwmSetWindowAttribute(
                            hwnd,
                            DWMWA_SYSTEMBACKDROP_TYPE,
                            &backdrop as *const _ as *const core::ffi::c_void,
                            std::mem::size_of_val(&backdrop) as u32,
                        );
                    }
```

(Reuse the `hwnd` binding already constructed for the corner call; merge the `use` items into the existing import block if that is where imports live. Mechanical import/naming adjustments are fine — list them as deviations.)

Verify compilation: `cargo check` → zero warnings. Then `npm run tauri dev` and **ask the human** to hold Ctrl+Win over a colorful window and answer: *blurred, or still flat?*

- **Blurred** → go to Step 4.
- **Still flat** → undo ONLY this step's additions (the `MARGINS`/backdrop block and the `Win32_UI_Controls` feature), then go to Step 3.

### Step 3: Attempt B — the legacy accent with an explicit tint

Only reached if Attempt A failed and was undone.

Add to `src-tauri/Cargo.toml` dependencies:

```toml
window-vibrancy = "0.6"
```

In `src-tauri/src/lib.rs`, in the overlay block (after the corner-preference `if let`, at the same level as `set_ignore_cursor_events`):

```rust
                // Legacy acrylic, called directly with a non-zero tint — the
                // config path can produce a black backdrop when its tint is
                // fully transparent.
                let _ = window_vibrancy::apply_acrylic(&w, Some((11, 10, 10, 60)));
```

`cargo check` → zero warnings. `npm run tauri dev` and **ask the human again**: *blurred, or still flat?*

- **Blurred** → go to Step 4.
- **Still flat** → go to Step 5.

### Step 4: Glass works — commit and resume

`cargo test` → **46 passed**. `cargo check` → zero warnings. Commit everything that is part of the working variant (conf, Cargo.toml, Cargo.lock, lib.rs, index.html):

```bash
git add -A
git commit -m "feat: frosted-glass pill with system-rounded corners"
```

Then resume the milestone plan at **Task 4**.

### Step 5: Glass is not available — revert and move on

Only reached if both attempts failed. Discard every uncommitted change so the pill stays the shipped solid design (18 px radius, opaque backgrounds):

```bash
git checkout -- src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/src/lib.rs src/index.html
git checkout -- src-tauri/Cargo.lock
```

`cargo test` → **46 passed**. Record in the DEVIATIONS list that Task 3 was abandoned: both the supported system backdrop and the legacy accent produced no blur on this Windows build, so the solid pill stays. The milestone report (Task 5) must say so, and its protocol's step 3 (the glass verdict) is then skipped. Resume the milestone plan at **Task 4**.
