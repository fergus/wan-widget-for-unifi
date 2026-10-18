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
    ctx.beginPath();
    samples.forEach((s, i) => (i ? ctx.lineTo(x(i), y(s[key])) : ctx.moveTo(x(i), y(s[key]))));
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
  $("month").textContent = formatBytes(rx + tx) + "  (\u2193 " + formatBytes(rx) + " / \u2191 " + formatBytes(tx) + ")";
}

function onIp({ external, wan, cgnat }) {
  $("ip").textContent = external || wan || "-";
  $("cgnat").hidden = !cgnat;
  $("cgnat").title = wan ? "Gateway WAN address is " + wan : "";
}

function onStatus(message) {
  $("status").hidden = !message;
  $("status").textContent = message || "";
}

const tauri = window.__TAURI__;
if (tauri && tauri.event) {
  tauri.event.listen("wan-rates", (e) => onRates(e.payload));
  tauri.event.listen("monthly-usage", (e) => onMonthly(e.payload));
  tauri.event.listen("ip-info", (e) => onIp(e.payload));
  tauri.event.listen("poll-status", (e) => onStatus(e.payload));
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
