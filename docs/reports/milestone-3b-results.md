# Milestone 3b results — settings window

Date: 2026-08-09

| # | Check | Result | Notes |
|---|---|---|---|
| 1 | Open, close-to-tray, and reopen from the tray | PASS | This originally failed because the window lacked permission to hide itself. After `f9f3f87`, ✕ hid the window without ending the app and a tray left-click reopened it. |
| 2 | Settings reflect the saved configuration and hotkey | PASS | The switches matched the saved configuration and the hotkey display showed the configured combination. |
| 3 | AI formatting applies immediately without a restart | PASS | Turning formatting on produced punctuated text on the next dictation; turning it off returned raw text. |
| 4 | Casual mode applies immediately without AI formatting | PASS | This originally failed because Casual mode only ran when AI formatting was also enabled. After `dffda3f`, Casual mode alone produced lowercase text ending in 😭😭😭. |
| 5 | Light, Dark, and Auto themes apply and persist | PASS | Each theme recolored the window immediately; Auto followed Windows and the selected theme survived closing and reopening. |
| 6 | Start-with-Windows setting updates the registry | PASS | Turning it off removed the WhisperOSS Run value and turning it on restored the value. |
| 7 | Valid API key saves and works immediately | PASS | The window showed “Checking…” followed by “Saved,” cleared the field to dots, and the next dictation succeeded. |
| 8 | Invalid API key is rejected without replacing the valid key | PASS | `gsk_wrong` produced the red rejection message and later dictation continued using the previously saved key. |
| 9 | Settings persist after fully quitting and relaunching | PASS | Every switch and the selected theme returned in the state left before quitting. |
| 10 | Re-verification: drag, minimize, close-to-tray, and tray reopen | PASS | Dragging moved the window, – minimized it, ✕ hid it while the app kept running, and the tray icon reopened it. |
| 11 | Re-verification: formatting OFF and Casual ON | PASS | Dictation was rewritten in lowercase casual style and ended in 😭😭😭 instead of returning raw text. |
| 12 | Re-verification: formatting ON and Casual OFF | PASS | A messy sentence returned properly punctuated and capitalized. |
| 13 | Re-verification: formatting ON and Casual ON | PASS | Casual style won when both switches were enabled, producing lowercase casual output with emoji. |
| 14 | Re-verification: autostart OFF | PASS | The WhisperOSS Run value was not found after turning Start-with-Windows off. |
| 15 | Re-verification: autostart ON | PASS | The WhisperOSS Run value was present after turning Start-with-Windows on. |

Automated tests: 42 passed.
Compiler warnings: none.
Verdict: GO for Milestone 3c.

Deviations:

- The execution skill named by the plans was not installed, so the plans were followed directly.
- The key-saving function was unused until its settings command was added; its temporary unused-code warning was suppressed so each task ended with no new warnings.
- A previously running development process initially prevented Windows from replacing the executable. After its repo path and command were verified, it was stopped and the required launch succeeded.
- The interactive computer-control runtime was unavailable. Automated desktop checks used the real WhisperOSS process and Windows window state; the human verification subsequently confirmed the visible behavior.
- Task 4 requested a working HTML close button before Task 5 supplied its JavaScript. Task 4 therefore verified the exact static design, and button behavior was verified after wiring and after the permission fix.
- Two visual-verification screenshots remained in the Windows temporary directory because command policy rejected deleting them; neither file is inside the repository.
