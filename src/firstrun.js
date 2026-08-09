const { invoke } = window.__TAURI__.core;
const { getCurrentWindow } = window.__TAURI__.window;
const { openUrl } = window.__TAURI__.opener;

const win = getCurrentWindow();
const el = (id) => document.getElementById(id);

// Closing without a key is allowed — the app keeps running in the tray and
// asks again the next time the hotkey is pressed.
el("close").onclick = () => win.hide();

function showStep(n) {
  el("step1").classList.toggle("active", n === 1);
  el("step2").classList.toggle("active", n === 2);
  el("dot1").classList.toggle("on", n === 1);
  el("dot2").classList.toggle("on", n === 2);
  el("stepcount").textContent = `Step ${n} of 2`;
  if (n === 2) el("api-key").focus();
}

el("get-started").onclick = () => showStep(2);

el("toggle-key").onclick = () => {
  const input = el("api-key");
  input.type = input.type === "password" ? "text" : "password";
};

el("groq-link").onclick = (e) => {
  e.preventDefault();
  openUrl("https://console.groq.com/keys");
};

function setError(text) {
  el("key-error").classList.toggle("shown", Boolean(text));
  el("key-error-text").textContent = text || "";
  el("api-key").classList.toggle("invalid", Boolean(text));
}

async function validate() {
  const key = el("api-key").value.trim();
  if (!key) { setError("Enter a key"); return; }
  setError("");
  el("validate").disabled = true;
  el("validate").textContent = "Checking…";
  try {
    await invoke("save_api_key", { key });
    win.hide();
  } catch (msg) {
    setError(String(msg));
  } finally {
    el("validate").disabled = false;
    el("validate").textContent = "Validate & finish";
  }
}

el("validate").onclick = validate;
el("api-key").addEventListener("keydown", (e) => {
  if (e.key === "Enter") validate();
});
