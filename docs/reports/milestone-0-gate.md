# Milestone 0 gate — overlay prototype

Date: 2026-08-08
Machine: 12th Gen Intel(R) Core(TM) i5-12400F / NVIDIA GeForce RTX 5060 Ti / Microsoft Windows 11 Pro Insider Preview 10.0.26220 (build 26220) / two 1920×1080 monitors at 100% scale; primary at (0,0), secondary at (-1920,3)

| Check | Target | Measured | Pass |
|---|---|---|---|
| fps, idle desktop, 60 s | avg ≥ 58, min ≥ 45 | avg 60, min 56 | yes |
| fps, video playing behind, 60 s | avg ≥ 58, min ≥ 45 | avg 60, min 56 | yes |
| Click-through ON: clicks reach app behind | yes | yes | yes |
| Click-through OFF: pill receives clicks | yes | yes | yes |
| Position: bottom-center, 26px above taskbar | yes | yes | yes |
| Transparent corners (no white/black box) | yes | yes | yes |

Verdict: GO for Milestone 1.
Notes: No flicker, focus steals, or positioning drift observed. The pill remained bottom-center above the taskbar with fully transparent corners.
