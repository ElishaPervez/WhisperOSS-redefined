# Milestone 3a results — settings engine

Date: 2026-08-09

| # | Check | Result | Notes |
|---|---|---|---|
| 1 | Baseline: unchanged config preserves the Milestone 2 dictation cycle | PASS | Bars → shimmer → check completed and dictation pasted successfully. |
| 2 | Formatting: punctuation and spoken math conversion | PASS | Formatting produced real punctuation and converted “x squared” to `x²`. |
| 3 | Casual mode: lowercase output and spoken emoji conversion | PASS | Output was lowercase and ended with three crying-face emoji. |
| 4 | Modifier-only rebind: Ctrl+Alt replaces Ctrl+Win | PASS | Ctrl+Alt started dictation; Ctrl+Win did nothing. |
| 5 | Regular-key rebind and swallowing: Ctrl+Space | PASS | Dictation worked in Notepad, no spaces appeared while Ctrl+Space was held, and plain Space typed normally afterward. |
| 6 | Invalid hotkey config falls back safely | PASS | `banana` was rejected, the default Ctrl+Win combo worked, and the fallback event appeared in the log. |
| 7 | Start-with-Windows registry reconciliation | PASS | The Run value existed when enabled and was removed after disabling and restarting. |
| 8 | Restore defaults and confirm baseline again | PASS | The default Ctrl+Win config was restored and dictation completed successfully. |

Automated tests: 40 passed.
Verdict: GO for Milestone 3b.
Deviations: during development, the Start-with-Windows Run entry points at the development executable named `scaffold-tmp.exe`. Milestone 4 will replace this with the installed, renamed executable through the installer and executable rename.
