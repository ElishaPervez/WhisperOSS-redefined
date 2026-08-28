# WhisperOSS

Dictation for Windows. Hold Ctrl+Win in any app, speak, and release. Your words are typed wherever the cursor is.

## How it works

- Hold Ctrl+Win and a small pill appears at the bottom of the screen with live audio bars.
- Speak, then release. The recording is transcribed by the provider selected in Settings and pasted at your cursor. When using Google Gemini Live, live text preview streams directly into the overlay as you speak.
- Optional AI formatting cleans punctuation and paragraph breaks. Casual mode gives lowercase with light emoji.

## Privacy

- Google and Groq API keys are stored in separate Windows Credential Manager entries, never in a file. Only the selected provider receives a recording; failures are shown without sending it to the other provider.
- Dictated text is excluded from the Windows clipboard history (Win+V) and the cloud clipboard. If those protections cannot be applied, the paste is aborted instead of falling back.
- Whatever was on your clipboard before a dictation is put back afterwards, including images, copied files, and rich text.
- Key events are never logged. The diagnostic log contains event names only, never transcripts.

## Reliability

- The microphone stream is always on with a half-second pre-roll, so the first word is never clipped.
- If the selected microphone disappears, the app falls back to the Windows default within a second, then returns to your device automatically when it comes back.
- Cold start to ready is around 25 ms. Holding the hotkey to visible bars is under 20 ms.

## Install

Download the installer from the [releases page](https://github.com/ElishaPervez/WhisperOSS-redefined/releases) and run it. No admin rights needed. On first run the app asks for a Groq API key, which you can create at [console.groq.com/keys](https://console.groq.com/keys).
You can switch to Google Gemini Live and save a Google AI Studio key in Settings.

## Build from source

Requirements: Windows 11, Rust (stable), Node.js 20+.

```
npm ci
npm run tauri build
```

The installer lands in `src-tauri/target/release/bundle/nsis/`.

Tests:

```
cd src-tauri
cargo test
```

## Settings

| Setting | What it does |
| --- | --- |
| Transcription provider | Chooses Google Gemini Live or Groq Whisper; there is no automatic fallback |
| Provider API key | Stored separately for Google and Groq on this PC |
| Transcription model | Stored separately for each provider |
| Live preview | Streams real-time speech preview into the overlay (Gemini Live) |
| AI formatting | Cleans punctuation and paragraph breaks |
| Casual mode | Lowercase, light emoji |
| Microphone | Input device used while dictating |
| Theme | Auto, light, or dark |
| Start with Windows | Runs quietly in the tray |

The hotkey is fixed at Ctrl+Win.
