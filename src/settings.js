const { invoke } = window.__TAURI__.core;
const { getCurrentWindow } = window.__TAURI__.window;
const { listen } = window.__TAURI__.event;

const win = getCurrentWindow();
document.getElementById("min").onclick = () => win.minimize();
document.getElementById("close").onclick = () => win.hide();

const el = (id) => document.getElementById(id);
let vocabularyWords = [];

function paintToggle(node, on) {
  node.classList.toggle("on", on);
}

function applyTheme(theme) {
  if (theme === "auto") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = theme;
  for (const b of el("theme").children) {
    b.classList.toggle("active", b.dataset.theme === theme);
  }
}

const KEY_LABELS = {
  ctrl: "Ctrl", win: "Win", alt: "Alt", shift: "Shift",
  space: "Space", tab: "Tab", capslock: "Caps Lock",
};
const keyLabel = (n) => KEY_LABELS[n] || n.toUpperCase();

function renderCombo(names) {
  const box = el("combo-keys");
  box.innerHTML = "";
  names.forEach((n, i) => {
    if (i) {
      const p = document.createElement("span");
      p.className = "plus";
      p.textContent = "+";
      box.appendChild(p);
    }
    const k = document.createElement("span");
    k.className = "key";
    k.textContent = keyLabel(n);
    box.appendChild(k);
  });
}

function renderVocabulary() {
  const box = el("vocab");
  const input = el("vocab-input");
  box.replaceChildren();

  vocabularyWords.forEach((word, index) => {
    const chip = document.createElement("span");
    chip.className = "chip";

    const label = document.createElement("span");
    label.textContent = word;
    chip.appendChild(label);

    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "chip-remove";
    remove.textContent = "x";
    remove.setAttribute("aria-label", `Remove ${word}`);
    remove.onclick = async () => {
      vocabularyWords.splice(index, 1);
      await persistVocabulary();
    };
    chip.appendChild(remove);
    box.appendChild(chip);
  });

  box.appendChild(input);
  el("vocab-note").textContent = vocabularyWords.length > 50
    ? "Whisper reads only about the last 150 words"
    : "";
}

async function persistVocabulary() {
  renderVocabulary();
  await invoke("set_vocabulary", { value: vocabularyWords });
  el("status-text").textContent = "Vocabulary updated";
}

async function commitVocabulary() {
  const input = el("vocab-input");
  const word = input.value.trim();
  input.value = "";
  if (!word) return;
  if (vocabularyWords.some((existing) => existing.toLowerCase() === word.toLowerCase())) return;

  vocabularyWords.push(word);
  await persistVocabulary();
}

async function loadMics(selected) {
  const mics = await invoke("list_microphones");
  const sel = el("mic");
  sel.innerHTML = "";
  const def = document.createElement("option");
  def.value = "";
  def.textContent = "System default";
  sel.appendChild(def);
  for (const name of mics) {
    const o = document.createElement("option");
    o.value = name;
    o.textContent = name;
    sel.appendChild(o);
  }
  // A saved mic that is unplugged right now still shows, so the setting
  // does not silently look like it was reset.
  if (selected && !mics.includes(selected)) {
    const o = document.createElement("option");
    o.value = selected;
    o.textContent = `${selected} (not connected)`;
    sel.appendChild(o);
  }
  sel.value = selected || "";
}

async function refreshMic() {
  const cfg = await invoke("get_settings");
  await loadMics(cfg.input_device);
  const mic = await invoke("microphone_status");
  const note = el("mic-note");
  if (!mic.healthy) {
    note.textContent = "· no microphone available";
  } else if (cfg.input_device && mic.active && mic.active !== cfg.input_device) {
    note.textContent = `· unavailable, using ${mic.active}`;
  } else {
    note.textContent = "";
  }
}

async function load() {
  const cfg = await invoke("get_settings");
  paintToggle(el("formatter"), cfg.use_formatter);
  paintToggle(el("casual"), cfg.casual_mode);
  paintToggle(el("autostart"), cfg.run_on_startup);
  applyTheme(cfg.theme);
  vocabularyWords = [...cfg.vocabulary];
  renderVocabulary();

  renderCombo(cfg.hotkey);
  await refreshMic();

  const hasKey = await invoke("has_api_key");
  if (hasKey) {
    el("api-key").placeholder = "••••••••••••••••";
    setKeyFeedback("Saved", "ok");
  } else {
    el("api-key").placeholder = "gsk_…";
    setKeyFeedback("", "");
  }
}

function setKeyFeedback(text, kind) {
  const f = el("key-feedback");
  f.textContent = text ? `· ${text}` : "";
  f.className = `keyfeedback ${kind || ""}`;
}

// --- toggles: optimistic paint, then persist ---
function wireToggle(id, command) {
  const node = el(id);
  node.onclick = async () => {
    const next = !node.classList.contains("on");
    paintToggle(node, next);
    await invoke(command, { value: next });
  };
}
wireToggle("formatter", "set_formatter");
wireToggle("casual", "set_casual");
wireToggle("autostart", "set_autostart");

// --- custom vocabulary ---
el("vocab-input").onkeydown = async (event) => {
  if (event.key !== "Enter" && event.key !== ",") return;
  event.preventDefault();
  await commitVocabulary();
};

// --- theme ---
for (const b of el("theme").children) {
  b.onclick = async () => {
    applyTheme(b.dataset.theme);
    await invoke("set_theme", { value: b.dataset.theme });
  };
}

// --- api key ---
el("toggle-key").onclick = () => {
  const input = el("api-key");
  input.type = input.type === "password" ? "text" : "password";
};
el("save-key").onclick = async () => {
  const key = el("api-key").value.trim();
  if (!key) { setKeyFeedback("Enter a key", "err"); return; }
  setKeyFeedback("Checking…", "");
  el("save-key").disabled = true;
  try {
    await invoke("save_api_key", { key });
    setKeyFeedback("Saved", "ok");
    el("api-key").value = "";
    el("api-key").placeholder = "••••••••••••••••";
  } catch (msg) {
    setKeyFeedback(String(msg), "err");
  } finally {
    el("save-key").disabled = false;
  }
};

// --- microphone ---
el("mic").onchange = async () => {
  const value = el("mic").value;
  await invoke("set_microphone", { value: value || null });
  el("status-text").textContent = "Microphone updated";
};

// The window is hidden rather than destroyed when closed, so the webview is
// only ever loaded once. Re-read on every open.
listen("settings-shown", async ({ payload }) => {
  await load();
  if (payload) {
    const input = el("api-key");
    input.value = "";
    input.type = "password";
    input.focus();
    setKeyFeedback("Groq rejected this key, paste a new one", "err");
  }
});

// The engine can change microphone underneath an already-open window.
listen("mic-changed", () => refreshMic());

load();
