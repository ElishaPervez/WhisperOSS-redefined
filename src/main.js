const { listen } = window.__TAURI__.event;

// --- bars -------------------------------------------------------------
const pill = document.querySelector(".pill");
const BAR_COUNT = 12;
const bars = [];
for (let i = 0; i < BAR_COUNT; i++) {
  const b = document.createElement("div");
  b.className = "bar";
  pill.appendChild(b);
  bars.push(b);
}

// Centre-emphasis: middle bars react more than edge bars.
const weights = bars.map((_, i) => {
  const d = Math.abs(i - (BAR_COUNT - 1) / 2) / ((BAR_COUNT - 1) / 2);
  return 0.35 + 0.65 * Math.cos((d * Math.PI) / 2);
});

// --- events from Rust -------------------------------------------------
let target = 0; // latest level from Rust, 0..1
listen("level", (e) => { target = e.payload; });

// --- fps meter --------------------------------------------------------
const fpsEl = document.getElementById("fps");
let frames = 0, last = performance.now(), minFps = Infinity;
setInterval(() => {
  const now = performance.now();
  const fps = (frames * 1000) / (now - last);
  minFps = Math.min(minFps, fps);
  fpsEl.textContent = `${fps.toFixed(0)}/${minFps.toFixed(0)}`;
  console.log(`fps avg(1s)=${fps.toFixed(1)} min=${minFps.toFixed(1)}`);
  frames = 0; last = now;
}, 1000);

// --- 60 fps animation loop -------------------------------------------
let smoothed = 0, t = 0;
function frame() {
  frames++;
  t += 1 / 60;
  // fast attack, slow decay — matches how the real meter should feel
  smoothed = target > smoothed
    ? smoothed + (target - smoothed) * 0.5
    : smoothed + (target - smoothed) * 0.12;
  const MIN = 0.16; // resting scaleY (≈4px of the 24px bar)
  bars.forEach((b, i) => {
    const wobble = 0.85 + 0.15 * Math.sin(t * 7 + i * 1.7);
    const s = MIN + (1 - MIN) * smoothed * weights[i] * wobble;
    b.style.transform = `scaleY(${s.toFixed(3)})`;
  });
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
