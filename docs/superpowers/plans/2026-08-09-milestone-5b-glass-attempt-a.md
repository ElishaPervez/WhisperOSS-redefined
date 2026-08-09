# Milestone 5b — The pill keeps Attempt A's material

**Status:** The glass diagnostic ended in a full revert because both attempts were judged "not blurred". The human has since reviewed the recordings and chosen **Attempt A's appearance as the final pill design**: the translucent smoked-grey material, blur or no blur. This plan reinstates Attempt A exactly, as a deliberate look rather than a failed blur experiment.

**What Attempt A is.** The DWM *system backdrop* (`DWMSBT_TRANSIENTWINDOW`) — the material Windows' own flyouts use — plus the rounded corner clip and the translucent CSS tint from the original Task 3. On this build the backdrop renders as translucent grey without blur; that is the approved appearance. If a future Windows build composites it fully, the same code gains real blur behind the same tint — acceptable and desirable.

## Global Constraints

- Same as the milestone plan. All **46** tests stay green, zero warnings. Never touch `src-reference\`.
- The `windowEffects` config option stays **out** of `tauri.conf.json` — it painted the backdrop black on this build.

---

### Task 1: Reinstate the material

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/index.html`

- [ ] **Step 1: Features.** In `src-tauri/Cargo.toml`, add to the `windows` crate feature list:

```toml
  "Win32_Graphics_Dwm",
  "Win32_UI_Controls",
```

- [ ] **Step 2: The backdrop.** In `src-tauri/src/lib.rs`, replace the overlay block in `setup`:

```rust
            // Overlay: hidden until a dictation starts, never clickable in M1.
            if let Some(w) = app.get_webview_window("overlay") {
                let _ = w.set_ignore_cursor_events(true);
                // The pill's material: Windows' transient backdrop, clipped
                // to rounded antialiased corners. On this build it renders as
                // translucent smoked grey rather than blurred acrylic — that
                // appearance IS the approved design (human decision, 5b). If
                // a later Windows build composites it fully, the same code
                // gains blur behind the same tint.
                if let Ok(handle) = w.hwnd() {
                    use windows::Win32::Graphics::Dwm::{
                        DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
                        DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE,
                        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
                    };
                    use windows::Win32::UI::Controls::MARGINS;
                    let hwnd = windows::Win32::Foundation::HWND(handle.0);
                    let pref = DWMWCP_ROUND;
                    let margins = MARGINS {
                        cxLeftWidth: -1,
                        cxRightWidth: -1,
                        cyTopHeight: -1,
                        cyBottomHeight: -1,
                    };
                    unsafe {
                        let _ = DwmSetWindowAttribute(
                            hwnd,
                            DWMWA_WINDOW_CORNER_PREFERENCE,
                            &pref as *const _ as *const core::ffi::c_void,
                            std::mem::size_of_val(&pref) as u32,
                        );
                        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
                        let backdrop = DWMSBT_TRANSIENTWINDOW;
                        let _ = DwmSetWindowAttribute(
                            hwnd,
                            DWMWA_SYSTEMBACKDROP_TYPE,
                            &backdrop as *const _ as *const core::ffi::c_void,
                            std::mem::size_of_val(&backdrop) as u32,
                        );
                    }
                }
            }
```

(If `handle.0`'s type does not match the `HWND` field directly, bridge with `as _` — mechanical, list as a deviation, don't stop.)

- [ ] **Step 3: The tint.** In `src/index.html`, change the `.pill` and `.pill.error` rules to the original Task 3 styling:

```css
  .pill {
    position: fixed; inset: 0;
    background: rgba(11, 10, 10, 0.72);
    border-radius: 8px;
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.06);
    display: flex; align-items: center; justify-content: center;
    opacity: 1;
    transition: opacity 240ms ease, background-color 160ms ease;
  }
  .pill.faded { opacity: 0; }
  .pill.error { background: rgba(236, 48, 19, 0.85); }
```

- [ ] **Step 4: Verify.** `cargo test` → **46 passed**. `cargo check` → zero warnings. Then `npm run tauri dev`, have the human dictate once over a colorful window, and ask: **does this match the look you chose?** Wait for the answer. Yes → continue. No → STOP and report.

- [ ] **Step 5: Commit.**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/lib.rs src/index.html
git commit -m "feat: translucent pill on the system backdrop material"
```

---

### Task 2: Rebuild and hand over to the final pass

- [ ] **Step 1:** `npm run tauri build` — report filename and size.

- [ ] **Step 2: Report and STOP.** The human runs the milestone plan's Task 5 Step 2 protocol against this new installer, with its step 3 reworded: *the pill shows the chosen translucent grey material with clean rounded corners in both normal and error states* (no blur expected).

Afterwards (on the human's go): the milestone plan's Task 5 Steps 4–6 as written, with the spec and report recording the design decision — blur is unavailable on this build; the shipped pill is the system backdrop's translucent material with the 8 px system corner clip, chosen by the human over the solid pill.
