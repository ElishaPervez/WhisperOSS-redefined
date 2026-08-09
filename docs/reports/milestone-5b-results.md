# Milestone 5b results - final polish

Date: 2026-08-09

## Final verification

Verdict: **GO for release.** Every check the human ran passed. Rich-text
clipboard preservation was not run and remains explicitly untested; it is not
counted as a pass.

| # | Check | Result | What the human observed |
|---|---|---|---|
| 1 | Install over the existing app and launch | PASS | The new installer upgraded the existing installation without an uninstall. The app launched and the existing setup continued to work. |
| 2 | Dictate immediately after launch | PASS | The first Ctrl+Win dictation worked without a warm-up attempt. The fresh log measured 25 ms from process start to ready. |
| 3 | Chosen translucent pill material | PASS | Normal and error pills showed the human-approved translucent smoked-grey/red system material with clean 8 px rounded corners. Visible blur was not expected or required. |
| 4 | Content-sized network error | PASS | With Wi-Fi off, the full message "Couldn't reach Groq" appeared in a wider pill without clipping and stayed centered. |
| 5 | Clipboard keeps a screenshot | PASS | A Win+Shift+S capture survived a dictation and still pasted afterward. |
| 6 | Clipboard keeps copied files | PASS | Two copied files survived a dictation and both pasted into another folder. |
| 7 | Clipboard keeps rich text | SKIPPED / UNTESTED | The human did not run this setup. Rich-text formatting preservation is not claimed as manually verified. |
| 8 | Dictation stays out of clipboard history | PASS | Dictated text did not appear in Win+V history. |
| 9 | Regression sweep | PASS | Normal dictation pasted; one silent take disappeared quietly; AI formatting produced punctuated text; microphone disable/re-enable recovery and the live settings update continued to work. |
| 10 | Fresh-log performance and paste sequence | PASS | Thirteen dictations showed visible bars within 6–16 ms of recording start. Every completed paste was confirmed and followed by clipboard restoration. The Wi-Fi-off request produced the intended network error, and no API-key errors appeared. |

## Performance measurements

| User-visible interval | Observed | Target | Result |
|---|---:|---:|---|
| Launch until the app is ready | 25 ms | under 1,500 ms | PASS |
| Hold begins until listening bars are painted | 6–16 ms across 13 dictations | under 100 ms | PASS |
| Key release until the target accepts the paste | typically 0.5–0.7 s | network round-trip plus about 0.3 s | PASS |
| Key release until paste for a 13 s dictation with AI formatting | 1.8 s | network round-trip plus about 0.3 s | PASS |

The progress indicator therefore appears well inside its latency target, and
the upload/format/paste path completes without presenting a false finished
state. The longer formatted dictation includes both transcription and the
optional formatting request.

## Clipboard result

The app now preserves every clipboard format whose contents can be copied and
restored as a Windows byte block: Unicode text, DIB/DIBv5 images, copied-file
lists, and app-registered formats such as HTML and RTF. It limits the in-memory
copy to 16 MB; above that limit it keeps text only. Screenshot and copied-file
preservation passed the human check. Rich-text preservation was skipped and is
therefore untested in this release pass.

Every completed dictation in the supplied log reached paste confirmation and
then restored the prior clipboard. Dictated text remained absent from Win+V.

## Pill decision trail

1. The original configuration-driven acrylic effect produced a dark,
   unblurred surface on this Windows 11 build.
2. The supported Windows transient backdrop produced a translucent
   smoked-grey material with clean system-rounded corners, but no visible blur.
3. A legacy acrylic call with an explicit tint also failed to produce blur.
4. The diagnostic initially reverted to the old opaque pill because blur was
   the acceptance criterion at that time.
5. After reviewing the actual appearances, the human chose the supported
   transient backdrop from attempt A as the final design. It was reinstated as
   a deliberate translucent material, not as a claim of blur.
6. The final installed build passed the normal and error-state visual check.

The shipped pill therefore uses the Windows transient system backdrop, a
translucent smoked-grey normal tint, a translucent red error tint, and the 8 px
Windows corner clip. A future Windows version may blur the same backdrop; that
would remain compatible with the chosen design.

## Package artifacts

- Installer: `WhisperOSS_0.1.0_x64-setup.exe` - 2,621,485 bytes.
- Upgrade path: installed successfully over the existing app without an uninstall.
- Automated tests: 46 passed.
- `cargo check`: passed with zero compiler warnings.

## DEVIATIONS

- The original Task 3 expected visible acrylic blur. The configuration effect,
  the supported system backdrop, and the explicit-tint legacy attempt produced
  no visible blur on this Windows build. The human selected the supported
  system backdrop's translucent material as the shipped appearance.
- The bundled Windows desktop-control runtime was unavailable during the first
  development check. A local Windows input and screen-capture fallback was used
  for diagnosis; the human made every acceptance decision and completed the
  final installed-build pass.
- The first diagnostic restore command used repository-root file names while
  running one directory below the root. Git rejected the paths and changed
  nothing; the command was rerun from the repository root before verification.
- The first synthetic Task 4 shortcut arrived before keyboard monitoring was
  ready and produced no recording. The retry ran after the app reported ready
  and produced the 11–14 ms development measurements; the final human log later
  measured 6–16 ms across 13 dictations.
- `Cargo.toml` twice showed a line-ending-only dirty flag with no textual
  difference after diagnostic/build commands. The exact committed file was
  restored before each commit, and no unintended content entered the history.
- Enabling the final Windows backdrop features did not change `Cargo.lock`;
  the plan's staging command included it, but Git correctly committed only the
  three files with content changes.

## Release verdict

**GO.** The final installer upgrades an existing installation successfully;
startup and hold-to-visible latency are comfortably inside their targets; the
chosen translucent material passed in normal and error states; screenshot and
file clipboard contents survive dictation; privacy paste remains excluded from
history; every paste is confirmed and restores the prior clipboard; silence,
network failure, formatting, and microphone recovery behave correctly. Rich
text is the sole skipped and untested human check.
