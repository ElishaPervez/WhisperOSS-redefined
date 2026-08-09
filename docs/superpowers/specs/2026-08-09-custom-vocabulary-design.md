# Custom vocabulary (approved design, 2026-08-09)

Add a user-editable custom vocabulary to WhisperOSS. The words are sent to
Groq's Whisper endpoint as the `prompt` field on every transcription request,
which biases the model toward those exact spellings. This is a bias, not a
substitution: no post-processing of the transcript happens.

Audience: implementing agent (Codex). Everything needed is in this file plus
the referenced source files. The design was approved by the product owner on
2026-08-09; do not redesign, implement as written.

## 1. Product behavior

- The settings window gains a seventh row, label "Custom vocabulary",
  description "Names and jargon Whisper should spell correctly".
- The control is a tag-chip editor:
  - Existing words render as chips, each with a remove control (a small "x").
  - An inline text input sits after the chips. Enter commits the typed word
    as a new chip. Typing a comma also commits (commas can never be part of
    a word because the wire format is comma-joined).
  - Clicking a chip's "x" removes it.
  - Every add or remove persists immediately (same pattern as the toggles,
    no Save button).
- The list defaults to EMPTY. No built-in words. (Product owner explicitly
  chose empty over prefilled defaults.)
- With a non-empty list, the transcription request gains exactly one new
  multipart field: `prompt` = the words joined with `", "` in list order,
  e.g. `"Codex, Claude, OpenAI, Anthropic, Fable"`.
- With an empty list, the request must be byte-for-byte identical to today
  (no empty `prompt` field).
- The AI formatting / casual cleanup pass is untouched.
- Multi-word chips are allowed ("Claude Code" is one chip).
- Casing is preserved exactly as typed; casing is the point of the feature.
- Soft limit: when the list exceeds 50 words, the row description changes to
  a warning that Whisper only reads roughly the last 150 words. Never
  hard-block adding words.

## 2. Sanitization rules (single source of truth: the Rust command)

Applied in `set_vocabulary` before persisting, in this order:

1. Trim whitespace from each entry.
2. Drop entries that are empty after trimming.
3. Drop case-insensitive duplicates, keeping the FIRST occurrence (its
   casing and position win).

The frontend may also pre-trim for display niceness, but the Rust command is
the enforcement point, tested in Rust.

## 3. Changes by file

All paths relative to the repo root
`C:\projects (code)\15. WhisperOSS redefined`.

### src-tauri/src/config.rs

- Add field to `Config`:
  ```rust
  /// Custom vocabulary sent as the Whisper `prompt` field (spec: custom
  /// vocabulary, 2026-08-09). Empty = feature off, field omitted from the
  /// request.
  pub vocabulary: Vec<String>,
  ```
- `Default` impl: `vocabulary: Vec::new()`.
- The struct already has `#[serde(default)]`, so an old config.json without
  the key merges to empty. No migration code.
- Update tests: `defaults_match_spec` asserts empty vec;
  `partial_json_fills_missing_fields_with_defaults` asserts a JSON blob
  without the key yields empty vec; `roundtrip` includes a non-empty
  vocabulary.

### src-tauri/src/commands.rs

- New command, same shape as `set_formatter` / `set_theme`:
  ```rust
  #[tauri::command]
  pub fn set_vocabulary(state: State<AppState>, value: Vec<String>) {
      state.config.lock().unwrap().vocabulary = sanitize_vocabulary(value);
      persist(&state);
      applog::log("setting-vocabulary-changed");
  }
  ```
- `sanitize_vocabulary(Vec<String>) -> Vec<String>` implements section 2 as
  a pure, unit-tested function (private fn in commands.rs with `#[cfg(test)]`
  tests, matching how the module tests other pure logic; if commands.rs has
  no test module today, add one for this function only).
- Do NOT log the words themselves (applog is a diagnostics file; the log
  line above carries no user content, matching the other setting logs).

### src-tauri/src/lib.rs

- Register `commands::set_vocabulary` in the `generate_handler![]` list.

### src-tauri/src/groq.rs

- Change the transcription entry points to carry the prompt:
  ```rust
  pub fn transcribe(&self, wav: Vec<u8>, vocab_prompt: &str) -> Result<String, GroqError>
  fn attempt(&self, wav: Vec<u8>, vocab_prompt: &str) -> Result<String, GroqError>
  ```
- In `attempt`, after the existing `.text(...)` calls:
  ```rust
  let form = if vocab_prompt.is_empty() {
      form
  } else {
      form.text("prompt", vocab_prompt.to_string())
  };
  ```
- Retry discipline is unchanged (the prompt rides on both attempts).
- Existing tests updated to pass `""`.
- New tests (see section 5) verify field presence/absence on the wire.

### src-tauri/src/pipeline.rs

- In the worker thread closure, extend the existing single config lock:
  ```rust
  let (key, use_formatter, casual, vocab) = {
      let cfg = state.config.lock().unwrap();
      (
          state.key.lock().unwrap().clone(),
          cfg.use_formatter,
          cfg.casual_mode,
          cfg.vocabulary.join(", "),
      )
  };
  ```
- Call `client.transcribe(wav, &vocab)`.
- The join lives here (it is trivial); sanitization already happened at
  save time.

### src-tauri/tauri.conf.json

- Settings window (`"label": "settings"`): height 640 -> 700. Width stays
  960. Do NOT touch the first-run window even though it has the same
  dimensions today.

### src/settings.html

- Add a seventh `.row` after the "Start with Windows" row, before the
  closing of `.rows`:
  - Left side: label "Custom vocabulary", desc with
    `<span id="vocab-note">` for the soft-limit warning.
  - Right side: `<div class="chips" id="vocab">` containing chips plus an
    inline `<input type="text" id="vocab-input">`.
- Styling MUST follow the existing design language: sharp rectangles, no
  border-radius anywhere in this app. Chips are small boxes:
  `border: 1px solid var(--divider); background: var(--surface);
  padding: 4px 10px; font-size: 13px;` with the "x" in `var(--muted)`,
  turning `var(--accent)` on hover. The inline input is borderless against
  the row background with a subtle placeholder ("add a word..."). The chip
  container wraps (`flex-wrap: wrap; gap: 8px; max-width: 460px;
  justify-content: flex-end;`) so a long list grows downward inside the row.
- The row grows with content; the other rows keep sharing the remaining
  height (they are all `flex: 1`). The +60 px window height absorbs the
  first two chip lines.

### src/settings.js

- On `load()`: render chips from `cfg.vocabulary`.
- Add on Enter or comma in `#vocab-input`; ignore an empty/whitespace commit;
  clear the input after commit.
- Remove on "x" click.
- After every mutation: re-render, then
  `await invoke("set_vocabulary", { value: words })` where `words` is the
  current chip list in order. Re-render from the frontend copy; do not
  round-trip through `get_settings` on every keystroke (matches the
  optimistic pattern used by the toggles).
- Duplicate handling in the UI: if the committed word case-insensitively
  matches an existing chip, do not add a chip (mirrors the Rust rule so the
  UI never shows a chip the backend dropped).
- `#vocab-note`: empty at <= 50 words; at > 50 words set text
  "Whisper reads only about the last 150 words" using the same
  muted/accent note styling as `#mic-note`.
- Status bar feedback on change: `el("status-text").textContent =
  "Vocabulary updated"` (matches the microphone row).
- `settings-shown` already calls `load()`, which re-reads config, so the
  chips stay fresh when the window reopens.

### docs/superpowers/specs/2026-08-08-whispeross-v2-design.md

Keep the v2 spec truthful:

- The "exactly six controls" line: six -> seven.
- The Config keys list (around line 130): add `vocabulary` (list of words,
  default empty).
- The Groq client bullet (around line 115): note that the transcription
  request includes the user's custom vocabulary as the `prompt` field when
  the list is non-empty.

## 4. Explicitly out of scope

- No vocabulary UI in the first-run window.
- No changes to the formatting/casual chat request.
- No hard cap on list length, no token counting.
- No per-word enable/disable, no import/export.

## 5. Tests (all must pass via `cargo test` in src-tauri)

Existing suite must stay green. New coverage:

1. config.rs: default empty; missing-key JSON merges to empty; roundtrip
   with a non-empty list.
2. commands.rs `sanitize_vocabulary`: trims; drops empties; drops
   case-insensitive duplicates keeping first casing/order
   (`["Claude", "claude ", "", "OpenAI"]` -> `["Claude", "OpenAI"]`).
3. groq.rs: with a non-empty vocab prompt, the multipart body contains a
   `prompt` part whose value is the joined string; with `""`, the body does
   NOT contain a `prompt` part. The existing `serve_once` helper discards
   the request; add a capturing variant that sends the raw request bytes
   back over an `mpsc` channel so the test can assert on the body. Note:
   the existing 64 KiB read buffer reads the request in one shot; keep the
   test WAV small (16 bytes like the other tests) so the whole multipart
   body fits.
4. Manual verification (documented in the results file, not automated):
   add Codex, Claude, OpenAI, Anthropic, Fable; dictate a sentence
   containing several of them; confirm the spellings land. Then remove all
   chips and confirm dictation still works.

## 6. Acceptance criteria

- [ ] Settings shows the seventh row; chips add on Enter/comma, remove on x,
      persist across app restart.
- [ ] config.json contains `"vocabulary": [...]` after a change; deleting
      the key from the file and restarting yields an empty list, not a crash.
- [ ] With words set, the transcription request carries
      `prompt: "<w1>, <w2>, ..."`; with no words, the request has no prompt
      field.
- [ ] Empty-list behavior is byte-identical to the current release
      (verified by the groq.rs absence test).
- [ ] Window is 700 px tall; all seven rows readable with 5 chips present.
- [ ] `cargo test` green; v2 spec doc updated.
