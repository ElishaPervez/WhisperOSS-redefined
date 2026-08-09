# Milestone 3c results — microphone picker shipped, hotkey rebind removed

Date: 2026-08-09

## Final verification

| # | Check | Result | Notes |
|---|---|---|---|
| 1 | Settings shows the fixed shortcut without rebind controls | PASS | The tray opened settings; the hero showed “Hold Ctrl + Win — speak — release” with its caption below, no “Change hotkey” button, and no leftover gap or misalignment. |
| 2 | Dictation still works with Ctrl+Win | PASS | Holding Ctrl+Win over a text field, speaking, and releasing pasted the transcript at the cursor. |
| 3 | Normal keyboard input is not swallowed | PASS | Space, Esc, Ctrl+F, Ctrl+Shift, and ordinary typing behaved normally while the app was running. |
| 4 | Changing the microphone still permits dictation | PASS | Selecting another input device rebuilt the live stream; the next dictation still pasted text. |
| 5 | The microphone choice survives restart | PASS | After quitting from the tray and relaunching, the selected device remained selected and dictation still worked. |
| 6 | AI formatting and Start-with-Windows still work | PASS | AI formatting returned punctuated text. Disabling and enabling Start-with-Windows produced the expected registry state. |
| 7 | Post-removal sessions contain no removed-feature log markers | PASS | The log was not deleted before the run, so the retained 14:28:35 session contains `hotkey-capture-begin`, `hotkey-capture-timeout`, and `hotkey-capture-cancelled` lines from before removal. The four post-removal sessions starting at 14:42:50, 14:46:30, 14:59:33, and 15:00:40 contain zero `hotkey-capture-*`, `hook-swallow-*`, `hook-key`, `pipeline-capture-*`, or `settings-window-blurred` lines. This was verified by searching the complete log. |

## Microphone picker — original four checks

| # | Check | Result | Notes |
|---|---|---|---|
| 1 | Real devices appear with System default first | PASS | The dropdown listed the machine’s input devices, with System default first and initially selected. |
| 2 | Switching to another working device takes effect | PASS | Dictation still pasted text after another input device was selected. |
| 3 | A silent device proves the old stream is no longer used | PASS | With an input that heard nothing selected, the pill faded and no text was pasted. |
| 4 | A working device persists across restart | PASS | After restoring a working microphone, quitting, and relaunching, the same device remained selected and dictation worked. |

These four checks came from the [Milestone 3c live-devices plan](../superpowers/plans/2026-08-09-milestone-3c-live-devices.md) and passed during its original human verification.

## Hotkey rebind outcome

The hotkey rebind was built, but clicking “Change hotkey” started a capture that the settings window’s focus handling ended before a key combination could be recorded. Two diagnostic rounds measured the failure: [round 1](../superpowers/plans/2026-08-09-milestone-3c-hotkey-diagnostic.md) separated the capture boundaries and cancellation reasons; [round 2](../superpowers/plans/2026-08-09-milestone-3c-hotkey-diagnostic-2.md) isolated the focus-loss behavior. Product then decided to remove the unreliable rebind instead of continuing to debug it. Ctrl+Win is now fixed, while the microphone picker remains shipped.

Automated tests: 42 passed. `cargo check`: passed with zero warnings.

## DEVIATIONS

- The requested plan-execution helper was not installed, so the plan was executed directly without changing its repository scope.
- Task 1 predicted temporary compiler failures in the keyboard hook, pipeline, and commands. Only the pipeline and commands still referenced the deleted capture code; the later planned deletions removed those references.
- Task 1 says to keep eight original shortcut tests but names nine. All nine named tests were kept, which produced the required final total of 42.
- The Task 2 keyboard-hook description already matched the plan’s replacement text, so no duplicate edit was made.
- The Windows app-control runtime was unavailable. Task 5 used Windows UI Automation and the live WebView instead: the tray opened settings, the rendered layout was inspected, and a clean reload produced zero errors.
- The first Task 5 development launch could not replace the executable because an older project-local instance still held it open. That exact instance was stopped; the retry launched and passed.
- The Task 7 log was not deleted before verification. Session boundaries separated the pre-removal markers from the four post-removal sessions, all of which were clean. The handoff described five post-removal sessions, but the supplied log contains four post-removal `app-start` lines at the four listed times.

Verdict: **GO for Milestone 4.** The microphone picker passed its original and final checks, the fixed Ctrl+Win dictation path works, normal keyboard input is unaffected, all other settings still work, and every post-removal session is free of rebind logging.
