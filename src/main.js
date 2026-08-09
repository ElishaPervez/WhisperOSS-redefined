const { listen } = window.__TAURI__.event;
const { invoke } = window.__TAURI__.core;

// --- faces ------------------------------------------------------------
const pill = document.getElementById("pill");
const faces = {
  listening: document.getElementById("face-listening"),
  processing: document.getElementById("face-processing"),
  success: document.getElementById("face-success"),
  error: document.getElementById("face-error"),
};
const errText = document.getElementById("err-text");

function setState(state, message) {
  pill.classList.toggle("faded", state === "hidden");
  pill.classList.toggle("error", state === "error");
  for (const [name, el] of Object.entries(faces)) {
    el.classList.toggle("on", name === state);
  }
  if (state === "error") errText.textContent = message || "Error";
  if (state === "listening") {
    // After the next paint, so the log line means "bars are on screen".
    requestAnimationFrame(() => invoke("overlay_visible"));
  }
}

listen("ui", (e) => setState(e.payload.state, e.payload.message));

// --- listening bars (unchanged behavior from M0/M1) -------------------
const BAR_COUNT = 12;
const bars = [];
for (let i = 0; i < BAR_COUNT; i++) {
  const b = document.createElement("div");
  b.className = "bar";
  faces.listening.appendChild(b);
  bars.push(b);
}
const weights = bars.map((_, i) => {
  const d = Math.abs(i - (BAR_COUNT - 1) / 2) / ((BAR_COUNT - 1) / 2);
  return 0.35 + 0.65 * Math.cos((d * Math.PI) / 2);
});

let target = 0;
listen("level", (e) => { target = e.payload; });

let smoothed = 0, t = 0;
function frame() {
  t += 1 / 60;
  smoothed = target > smoothed
    ? smoothed + (target - smoothed) * 0.5
    : smoothed + (target - smoothed) * 0.12;
  const MIN = 0.16;
  bars.forEach((b, i) => {
    const wobble = 0.85 + 0.15 * Math.sin(t * 7 + i * 1.7);
    const s = MIN + (1 - MIN) * smoothed * weights[i] * wobble;
    b.style.transform = `scaleY(${s.toFixed(3)})`;
  });
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
