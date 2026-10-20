import { pushSample, yMax, formatRate, formatBytes, formatDuration, formatLatency } from "./series.js";
import { applyTheme } from "./theme.js";

const MAX_POINTS = 150; // 5 minutes at 2s polling
const $ = (id) => document.getElementById(id);
const canvas = $("graph");
const ctx = canvas.getContext("2d");
const css = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

let samples = [];

// Size the backing store to the screen's pixels so lines stay sharp at 125-200% scaling.
// Checked on every draw because the window can move to a monitor with other scaling.
function fitCanvas() {
  const dpr = window.devicePixelRatio || 1;
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  if (!w || !h) return null;
  const pw = Math.round(w * dpr);
  const ph = Math.round(h * dpr);
  if (canvas.width !== pw || canvas.height !== ph) {
    canvas.width = pw;
    canvas.height = ph;
  }
  ctx.setTransform(pw / w, 0, 0, ph / h, 0, 0);
  return { w, h };
}

function draw() {
  const size = fitCanvas();
  if (!size) return;
  const { w, h } = size;
  ctx.clearRect(0, 0, w, h);
  const top = yMax(samples);
  const step = w / (MAX_POINTS - 1);
  const x = (i) => w - (samples.length - 1 - i) * step;
  const y = (v) => h - 2 - (v / top) * (h - 6);

  ctx.strokeStyle = css("--muted");
  ctx.globalAlpha = 0.25;
  ctx.beginPath();
  ctx.moveTo(0, h - 0.5);
  ctx.lineTo(w, h - 0.5);
  ctx.stroke();
  ctx.globalAlpha = 1;

  for (const [key, color] of [["rx", "--down"], ["tx", "--up"]]) {
    if (samples.length < 2) break;
    ctx.strokeStyle = css(color);
    ctx.lineWidth = 1.5;
    // Curves through the midpoints between samples: smoother, and never overshoots a peak.
    const pts = samples.map((s, i) => [x(i), y(s[key])]);
    ctx.beginPath();
    ctx.moveTo(pts[0][0], pts[0][1]);
    for (let i = 1; i < pts.length - 1; i++) {
      const [cx, cy] = pts[i];
      const [nx, ny] = pts[i + 1];
      ctx.quadraticCurveTo(cx, cy, (cx + nx) / 2, (cy + ny) / 2);
    }
    const last = pts[pts.length - 1];
    ctx.lineTo(last[0], last[1]);
    ctx.stroke();
  }
}

// Rates arrive every poll interval (2 s); with backoff they can stretch to 60 s.
// Past STALE_MS with no rates, the widget dims and says so.
const STALE_MS = 15_000;
let lastRatesAt = 0;

function onRates({ rx, tx, latency }) {
  if (away) return;
  lastRatesAt = Date.now();
  samples = pushSample(samples, { rx, tx }, MAX_POINTS);
  $("rx").textContent = "\u2193 " + formatRate(rx);
  $("tx").textContent = "\u2191 " + formatRate(tx);
  $("latency").textContent = formatLatency(latency);
  draw();
}

// Seconds the WAN link had been up when last fetched (every 60 s), and when that was.
// The display counts on from there so it does not sit still between fetches.
let uptime = null;

function onUptime({ seconds }) {
  uptime = typeof seconds === "number" ? { seconds, at: Date.now() } : null;
  showUptime();
}

function showUptime(stale = false) {
  const live = uptime && !away && !stale;
  $("uptime").textContent = live ? formatDuration(uptime.seconds + (Date.now() - uptime.at) / 1000) : "-";
}

// Away: the gateway is not reachable from this network, so polling is paused. Keep the
// last month and IP (dimmed) but drop the live numbers, which would be old.
let away = false;

function onAway(isAway) {
  away = isAway;
  document.querySelector(".card").classList.toggle("away", away);
  $("away").hidden = !away;
  if (away) {
    samples = [];
    lastRatesAt = 0;
    $("rx").textContent = "Away";
    $("tx").textContent = "";
    $("latency").textContent = "-";
    $("status").hidden = true;
    draw();
  } else {
    // Back on a network we are polling again; show the usual "waiting" dashes until data lands.
    $("rx").textContent = "-";
    $("tx").textContent = "-";
  }
  showUptime();
}

function onMonthly({ rx, tx }) {
  $("month").textContent = formatBytes(rx + tx);
  $("month").title = "\u2193 " + formatBytes(rx) + "  /  \u2191 " + formatBytes(tx);
}

function onIp({ external, wan, cgnat }) {
  $("ip").textContent = external || wan || "-";
  $("ip").title = [external && "Public IP " + external, wan && "Gateway WAN " + wan].filter(Boolean).join("  /  ");
  $("cgnat").hidden = !cgnat;
  $("cgnat").title = wan ? "Gateway WAN address is " + wan : "";
}

function onStatus(message) {
  if (away) return;
  $("status").hidden = !message;
  $("status").textContent = message || "";
}

const tauri = window.__TAURI__;
const openSettings = () => tauri && tauri.core && tauri.core.invoke("open_settings");
$("gear").addEventListener("click", openSettings);
$("status").addEventListener("click", openSettings);
const resumePolling = () => tauri && tauri.core && tauri.core.invoke("resume_polling");
$("away").addEventListener("click", resumePolling);
$("away").addEventListener("keydown", (e) => (e.key === "Enter" || e.key === " ") && resumePolling());

if (tauri && tauri.event) {
  // First run: no key stored yet, so go straight to the settings window.
  tauri.core.invoke("has_api_key").then((stored) => stored || openSettings());
  tauri.event.listen("wan-rates", (e) => onRates(e.payload));
  tauri.event.listen("monthly-usage", (e) => onMonthly(e.payload));
  tauri.event.listen("ip-info", (e) => onIp(e.payload));
  tauri.event.listen("poll-status", (e) => onStatus(e.payload));
  tauri.event.listen("wan-uptime", (e) => onUptime(e.payload));
  tauri.event.listen("away", (e) => onAway(e.payload));
  tauri.event.listen("theme", (e) => applyTheme(e.payload));
  tauri.core.invoke("get_settings").then((s) => applyTheme(s.theme));
  // Monthly and IP are only emitted every 5 minutes, so also fetch the latest values.
  tauri.core.invoke("latest_snapshot").then(({ monthly, ip, uptime: up, away: isAway }) => {
    if (monthly) onMonthly(monthly);
    if (ip) onIp(ip);
    if (up) onUptime(up);
    if (isAway) onAway(true);
  });
} else {
  // Plain browser preview: synthetic data so the layout can be checked without a gateway.
  onMonthly({ rx: 412e9, tx: 51e9 });
  onIp({ external: "203.0.113.7", wan: "100.64.10.20", cgnat: true });
  onUptime({ seconds: 1_049_270 });
  // Add ?away to the page URL to preview the away state.
  if (location.search.includes("away")) onAway(true);
  let t = 0;
  setInterval(() => {
    t += 1;
    onRates({
      rx: Math.max(0, 600_000 + 400_000 * Math.sin(t / 6) + Math.random() * 150_000),
      tx: Math.max(0, 150_000 + 90_000 * Math.sin(t / 4) + Math.random() * 40_000),
      latency: 9 + Math.round(Math.random() * 6),
    });
  }, 500);
}

setInterval(() => {
  const stale = !away && Date.now() - lastRatesAt > STALE_MS;
  document.querySelector(".card").classList.toggle("stale", stale);
  $("stale").hidden = !stale;
  showUptime(stale);
}, 1000);

// Keep the window exactly as tall as the card, so no transparent strip shows below it.
if (tauri && tauri.window && typeof ResizeObserver === "function") {
  const win = tauri.window.getCurrentWindow();
  const card = document.querySelector(".card");
  new ResizeObserver(() => {
    const height = Math.ceil(card.getBoundingClientRect().height);
    win.setSize(new tauri.window.LogicalSize(card.offsetWidth, height)).catch(() => {});
  }).observe(card);
}
