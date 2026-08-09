const { invoke } = window.__TAURI__.core;
const { getCurrentWindow } = window.__TAURI__.window;
const { listen } = window.__TAURI__.event;

const win = getCurrentWindow();
document.getElementById("min").onclick = () => win.minimize();
document.getElementById("close").onclick = () => win.hide();

const el = (id) => document.getElementById(id);

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

let currentHotkey = ["ctrl", "win"];
let capturing = false;

function renderCombo(names, listening) {
  const box = el("combo-keys");
  box.innerHTML = "";
  if (!names.length) {
    const s = document.createElement("span");
    s.className = "key";
    s.textContent = listening ? "…" : "—";
    box.appendChild(s);
    return;
  }
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

function setHint(text, kind) {
  const h = el("hotkey-hint");
  h.textContent = text;
  h.className = `hint ${kind || ""}`;
}

function setCapturing(on) {
  capturing = on;
  document.body.classList.toggle("capturing", on);
  el("change-hotkey").textContent = on ? "Listening — press your keys" : "Change hotkey";
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

async function load() {
  const cfg = await invoke("get_settings");
  paintToggle(el("formatter"), cfg.use_formatter);
  paintToggle(el("casual"), cfg.casual_mode);
  paintToggle(el("autostart"), cfg.run_on_startup);
  applyTheme(cfg.theme);

  currentHotkey = cfg.hotkey;
  renderCombo(currentHotkey, false);
  await loadMics(cfg.input_device);

  const hasKey = await invoke("has_api_key");
  if (hasKey) {
    el("api-key").placeholder = "••••••••••••••••";
    setKeyFeedback("Saved", "ok");
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

// --- hotkey rebind ---
el("change-hotkey").onclick = async () => {
  if (capturing) {
    await invoke("cancel_hotkey_capture", { reason: "button" });
    return;
  }
  setCapturing(true);
  renderCombo([], true);
  setHint("Hold the keys together, then let go. Esc cancels.", "");
  await invoke("begin_hotkey_capture");
};

listen("hotkey", ({ payload }) => {
  if (payload.phase === "preview") {
    renderCombo(payload.keys, true);
    return;
  }
  setCapturing(false);
  if (payload.phase === "set") {
    currentHotkey = payload.keys;
    renderCombo(currentHotkey, false);
    setHint("Hotkey updated", "ok");
    return;
  }
  renderCombo(currentHotkey, false);
  setHint(
    payload.phase === "invalid"
      ? "Needs a modifier and at most one other key"
      : "Hotkey unchanged",
    payload.phase === "invalid" ? "err" : ""
  );
});

// DIAGNOSTIC: blur no longer cancels — it only reports. The 6 s watchdog is
// the safety net for this run.
window.addEventListener("blur", () => {
  if (capturing) invoke("report_blur");
});

load();
