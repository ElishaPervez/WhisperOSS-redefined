const { invoke } = window.__TAURI__.core;
const { getCurrentWindow } = window.__TAURI__.window;

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

async function load() {
  const cfg = await invoke("get_settings");
  paintToggle(el("formatter"), cfg.use_formatter);
  paintToggle(el("casual"), cfg.casual_mode);
  paintToggle(el("autostart"), cfg.run_on_startup);
  applyTheme(cfg.theme);

  // Hotkey display (rebind is a later update).
  const keys = cfg.hotkey.map((k) => k.charAt(0).toUpperCase() + k.slice(1));
  el("key1").textContent = keys[0] || "Ctrl";
  el("key2").textContent = keys[1] || "Win";

  el("mic-name").textContent = cfg.input_device || "System default";

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

load();
