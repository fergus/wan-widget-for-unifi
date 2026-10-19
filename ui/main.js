import { pushSample, yMax, formatRate, formatBytes } from "./series.js";

const MAX_POINTS = 150; // 5 minutes at 2s polling
const $ = (id) => document.getElementById(id);
const canvas = $("graph");
const ctx = canvas.getContext("2d");
const css = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

let samples = [];

function draw() {
  const { width: w, height: h } = canvas;
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

function onRates({ rx, tx }) {
  samples = pushSample(samples, { rx, tx }, MAX_POINTS);
  $("rx").textContent = "\u2193 " + formatRate(rx);
  $("tx").textContent = "\u2191 " + formatRate(tx);
  draw();
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
  $("status").hidden = !message;
  $("status").textContent = message || "";
}

const tauri = window.__TAURI__;
const openSettings = () => tauri && tauri.core && tauri.core.invoke("open_settings");
$("gear").addEventListener("click", openSettings);
$("status").addEventListener("click", openSettings);

if (tauri && tauri.event) {
  // First run: no key stored yet, so go straight to the settings window.
  tauri.core.invoke("has_api_key").then((stored) => stored || openSettings());
  tauri.event.listen("wan-rates", (e) => onRates(e.payload));
  tauri.event.listen("monthly-usage", (e) => onMonthly(e.payload));
  tauri.event.listen("ip-info", (e) => onIp(e.payload));
  tauri.event.listen("poll-status", (e) => onStatus(e.payload));
  // Monthly and IP are only emitted every 5 minutes, so also fetch the latest values.
  tauri.core.invoke("latest_snapshot").then(({ monthly, ip }) => {
    if (monthly) onMonthly(monthly);
    if (ip) onIp(ip);
  });
} else {
  // Plain browser preview: synthetic data so the layout can be checked without a gateway.
  onMonthly({ rx: 412e9, tx: 51e9 });
  onIp({ external: "203.0.113.7", wan: "100.64.10.20", cgnat: true });
  let t = 0;
  setInterval(() => {
    t += 1;
    onRates({
      rx: Math.max(0, 600_000 + 400_000 * Math.sin(t / 6) + Math.random() * 150_000),
      tx: Math.max(0, 150_000 + 90_000 * Math.sin(t / 4) + Math.random() * 40_000),
    });
  }, 500);
}

// Keep the window exactly as tall as the card, so no transparent strip shows below it.
if (tauri && tauri.window && typeof ResizeObserver === "function") {
  const win = tauri.window.getCurrentWindow();
  const card = document.querySelector(".card");
  new ResizeObserver(() => {
    const height = Math.ceil(card.getBoundingClientRect().height);
    win.setSize(new tauri.window.LogicalSize(card.offsetWidth, height)).catch(() => {});
  }).observe(card);
}
