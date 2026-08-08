# Milestone 1 results — headless dictation pipeline

Date: 2026-08-09
Machine: 12th Gen Intel(R) Core(TM) i5-12400F / Windows default input microphone (device name is not recorded in M1) / Windows 11 Pro Insider Preview 10.0.26220 (build 26220)

| # | Check | Result | Notes |
|---|---|---|---|
| 1 | Happy path: speech → text in Notepad | PASS | Release to confirmed paste measured 0.61 s in the first logged run. |
| 2 | First word not clipped (pre-roll) | PASS | Human-observed first word present; the run ended in a confirmed paste. |
| 3 | Original clipboard restored | PASS | Human-observed `SENTINEL` restored; the log confirms restoration after the transcript paste. |
| 4 | Transcript absent from Win+V history | PASS | Human-observed Win+V result; privacy staging produced no failure event. |
| 5 | Short tap ignored | PASS | Four short taps were cancelled without a paste event. |
| 6 | Silence discarded without upload | PASS | The silent hold was discarded immediately after recording finished. |
| 7 | Network failure: logged, app alive, recovers | PASS | Network failure appeared about 24 s after release (15 s timeout plus one retry); the next dictation after reconnect ended in a confirmed paste. |
| 8 | Rapid re-dictation: no interleaved pastes | PASS | The first result was discarded as stale; only the second result produced a confirmed paste. |
| 9 | Tray quit clean | PASS | The tray Quit action was recorded and the app exited. |

Automated tests: 26 passed.
Verdict: GO for Milestone 2.
Deviations: the plan's Claude-specific execution helper was unavailable, so Codex followed the written plan directly; Windows crate 0.61 places the global-memory handle type in a different generated namespace, so Task 8 adapted the two handle-conversion call sites without changing clipboard behavior.
