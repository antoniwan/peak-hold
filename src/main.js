const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const RANGE_MIN = 20, RANGE_MAX = 120, HISTORY_MS = 30000;
const REFS = [
  [35, "a quiet room at night"], [50, "a library or quiet office"], [65, "normal conversation"],
  [75, "a busy restaurant"], [85, "heavy traffic"], [95, "a lawn mower"], [105, "a motorcycle"],
  [115, "a rock concert"], [Infinity, "a thunderclap, near the threshold of pain"]
];
const $ = id => document.getElementById(id);

// Levels arrive from Rust in dBFS; the calibration offset turns them into estimated dB SPL.
let offset = 94;
let peakFs = null, peakAt = null, lastPeakFs = null;
let smooth = null;
const history = []; // [time, dBFS]

const clampDb = db => Math.min(RANGE_MAX, Math.max(RANGE_MIN, db));
const pct = db => (clampDb(db) - RANGE_MIN) / (RANGE_MAX - RANGE_MIN) * 100;
const toDb = fs => Math.max(0, fs + offset);
const zoneOf = db => db < 70 ? "ok" : db < 85 ? "warn" : "danger";
const zoneText = { ok: "Comfortable", warn: "Loud", danger: "Hearing risk with long exposure" };
const describe = db => "About as loud as " + REFS.find(([max]) => db < max)[1] + ".";

function setStatus(text, state) { const s = $("status"); s.textContent = text; s.dataset.state = state; }

for (let db = RANGE_MIN; db <= RANGE_MAX; db += 20) {
  const s = document.createElement("span");
  s.textContent = db;
  s.style.bottom = pct(db) + "%";
  $("scale").appendChild(s);
}

try {
  const saved = parseFloat(localStorage.getItem("offset"));
  if (Number.isFinite(saved)) { offset = saved; $("offset").value = saved; }
} catch {}
invoke("set_offset", { offset });

$("offset").addEventListener("input", e => {
  const v = parseFloat(e.target.value);
  if (!Number.isFinite(v)) return;
  offset = v;
  try { localStorage.setItem("offset", String(v)); } catch {}
  invoke("set_offset", { offset });
  renderPeak();
});

const resetPeak = () => invoke("reset_peak");
$("reset").addEventListener("click", resetPeak);
document.addEventListener("keydown", e => {
  if ((e.key === "r" || e.key === "R") && !e.ctrlKey && !e.metaKey && e.target.tagName !== "INPUT") resetPeak();
});

function apply(snap) {
  if (snap.error) {
    setStatus("Mic unavailable", "error");
    $("gateMsg").textContent = snap.error;
    $("gate").hidden = false;
    return;
  }
  if (snap.device) {
    setStatus("Listening · " + snap.device, "live");
    $("gate").hidden = true;
  }
  peakFs = snap.peakFs ?? null;
  peakAt = snap.peakAt ?? null;
  if (peakFs !== lastPeakFs) {
    if (peakFs != null && (lastPeakFs == null || peakFs - lastPeakFs > 0.5)) bump();
    lastPeakFs = peakFs;
    renderPeak();
  }
  if (snap.levelFs != null) {
    history.push([performance.now(), snap.levelFs]);
    smooth = smooth == null ? snap.levelFs : smooth + (snap.levelFs - smooth) * 0.4;
  }
}

function renderPeak() {
  const has = peakFs != null;
  const db = has ? toDb(peakFs) : 0;
  $("peakNum").textContent = has ? db.toFixed(1) : "--.-";
  $("compare").textContent = has ? describe(db) : "Waiting for sound.";
  const zone = $("zone");
  zone.hidden = !has;
  if (has) { zone.dataset.zone = zoneOf(db); zone.textContent = zoneText[zoneOf(db)]; }
  $("hold").hidden = !has;
  if (has) $("hold").style.bottom = pct(db) + "%";
  renderWhen();
}

function renderWhen() {
  if (!peakAt) { $("when").textContent = ""; return; }
  const secs = Math.round((Date.now() - peakAt) / 1000);
  const ago = secs < 2 ? "just now" : secs < 60 ? secs + " s ago" : secs < 3600 ? Math.floor(secs / 60) + " min ago" : Math.floor(secs / 3600) + " h ago";
  $("when").textContent = "Captured " + new Date(peakAt).toLocaleTimeString() + " · " + ago;
}

function bump() {
  const p = $("peak");
  p.classList.remove("bump");
  void p.offsetWidth;
  p.classList.add("bump");
}

let colors = {};
function readColors() {
  const cs = getComputedStyle(document.documentElement);
  for (const k of ["fg", "muted", "line", "accent", "font-num"]) colors[k] = cs.getPropertyValue("--" + k).trim();
}
readColors();
matchMedia("(prefers-color-scheme: dark)").addEventListener("change", readColors);

function frame(now) {
  while (history.length && now - history[0][0] > HISTORY_MS) history.shift();
  if (smooth != null) {
    const nowDb = toDb(smooth);
    $("nowNum").textContent = nowDb.toFixed(0);
    const fill = $("fill");
    fill.style.height = pct(nowDb) + "%";
    fill.style.background = "var(--" + zoneOf(nowDb) + ")";
  }
  renderWhen();
  draw(now);
  requestAnimationFrame(frame);
}

function draw(now) {
  const c = $("hist"), dpr = window.devicePixelRatio || 1;
  const w = c.clientWidth, h = c.clientHeight;
  if (c.width !== Math.round(w * dpr) || c.height !== Math.round(h * dpr)) {
    c.width = Math.round(w * dpr);
    c.height = Math.round(h * dpr);
  }
  const g = c.getContext("2d");
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.clearRect(0, 0, w, h);

  const padL = 30, padR = 6, padT = 8, padB = 20;
  const pw = w - padL - padR, ph = h - padT - padB;
  const x = t => padL + (1 - (now - t) / HISTORY_MS) * pw;
  const y = db => padT + (1 - (clampDb(db) - RANGE_MIN) / (RANGE_MAX - RANGE_MIN)) * ph;

  g.font = "11px " + colors["font-num"];
  g.textBaseline = "middle";
  g.lineWidth = 1;
  for (let db = RANGE_MIN; db <= RANGE_MAX; db += 20) {
    g.strokeStyle = colors.line;
    g.beginPath(); g.moveTo(padL, Math.round(y(db)) + .5); g.lineTo(w - padR, Math.round(y(db)) + .5); g.stroke();
    g.fillStyle = colors.muted; g.textAlign = "right";
    g.fillText(db, padL - 6, y(db));
  }
  g.textBaseline = "alphabetic";
  for (const s of [30, 20, 10, 0]) {
    g.textAlign = s === 30 ? "left" : s === 0 ? "right" : "center";
    g.fillText(s === 0 ? "now" : "−" + s + " s", padL + (1 - s / 30) * pw, h - 4);
  }

  if (history.length > 1) {
    g.beginPath();
    g.moveTo(x(history[0][0]), y(RANGE_MIN));
    for (const [t, fs] of history) g.lineTo(x(t), y(toDb(fs)));
    g.lineTo(x(history[history.length - 1][0]), y(RANGE_MIN));
    g.closePath();
    g.globalAlpha = .12; g.fillStyle = colors.fg; g.fill(); g.globalAlpha = 1;

    g.beginPath();
    history.forEach(([t, fs], i) => i ? g.lineTo(x(t), y(toDb(fs))) : g.moveTo(x(t), y(toDb(fs))));
    g.strokeStyle = colors.fg; g.lineWidth = 1.5; g.lineJoin = "round"; g.stroke();
  }

  if (peakFs != null) {
    const db = toDb(peakFs), py = y(db);
    g.setLineDash([6, 4]); g.strokeStyle = colors.accent; g.lineWidth = 2;
    g.beginPath(); g.moveTo(padL, py); g.lineTo(w - padR, py); g.stroke(); g.setLineDash([]);
    g.textAlign = "left"; g.fillStyle = colors.accent; g.font = "600 11px " + colors["font-num"];
    g.fillText("peak " + db.toFixed(1), padL + 6, py < padT + 16 ? py + 14 : py - 6);
  }
}

listen("meter", e => apply(e.payload));
invoke("get_snapshot").then(apply);
requestAnimationFrame(frame);
