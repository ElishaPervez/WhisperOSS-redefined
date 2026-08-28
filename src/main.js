const { listen } = window.__TAURI__.event;
const { invoke } = window.__TAURI__.core;

// --- faces ------------------------------------------------------------
const pill = document.getElementById("pill");
const faces = {
  listening: document.getElementById("face-listening"),
  streaming: document.getElementById("face-streaming"),
  processing: document.getElementById("face-processing"),
  success: document.getElementById("face-success"),
  error: document.getElementById("face-error"),
};
const errText = document.getElementById("err-text");
const streamTextWrap = document.getElementById("stream-text-wrap");
const streamText = document.getElementById("stream-text");

let previousWords = [];
function setStreamText(text) {
  if (!text) {
    streamText.innerHTML = "";
    previousWords = [];
    streamTextWrap.classList.remove("faded-left");
    return;
  }
  const words = text.trim().split(/\s+/);
  let commonCount = 0;
  while (
    commonCount < previousWords.length &&
    commonCount < words.length &&
    previousWords[commonCount] === words[commonCount]
  ) {
    commonCount++;
  }
  if (commonCount === previousWords.length && commonCount < words.length) {
    for (let i = commonCount; i < words.length; i++) {
      const span = document.createElement("span");
      span.className = "word";
      span.textContent = words[i];
      streamText.appendChild(span);
    }
  } else if (commonCount !== words.length || words.length !== previousWords.length) {
    streamText.innerHTML = "";
    words.forEach((w, i) => {
      const span = document.createElement("span");
      span.className = i >= commonCount ? "word" : "";
      span.textContent = w;
      streamText.appendChild(span);
    });
  }
  previousWords = words;
  streamTextWrap.scrollLeft = streamTextWrap.scrollWidth;
  streamTextWrap.classList.toggle("faded-left", streamTextWrap.scrollLeft > 4);
}

function setState(state, message) {
  pill.classList.toggle("faded", state === "hidden");
  pill.classList.toggle("error", state === "error");
  for (const [name, el] of Object.entries(faces)) {
    el.classList.toggle("on", name === state);
  }
  if (state === "error") errText.textContent = message || "Error";
  if (state === "streaming") {
    setStreamText(message);
  } else if (state === "listening") {
    setStreamText("");
    // After the next paint, so the log line means "bars are on screen".
    requestAnimationFrame(() => invoke("overlay_visible"));
  } else {
    setStreamText("");
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

// --- mini streaming waveform bars (docked on the left) ----------------
const MINI_BAR_COUNT = 4;
const miniBars = [];
const streamBarsContainer = document.getElementById("stream-bars");
for (let i = 0; i < MINI_BAR_COUNT; i++) {
  const b = document.createElement("div");
  b.className = "mini-bar";
  streamBarsContainer.appendChild(b);
  miniBars.push(b);
}

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
  miniBars.forEach((b, i) => {
    const wobble = 0.85 + 0.15 * Math.sin(t * 7 + i * 1.7);
    const s = 0.25 + 0.75 * smoothed * wobble;
    b.style.transform = `scaleY(${s.toFixed(3)})`;
  });
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
