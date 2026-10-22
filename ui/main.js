import {
  pushSample, yScale, formatRate, formatBytes, formatDuration, formatLatency,
  latencyStats, monitorName, formatPeriod, secondWanLines,
  trimBefore, visiblePoints, nearestPoint, splitAtGaps, formatClock,
} from "./series.js";
import { applyTheme } from "./theme.js";

const MAX_POINTS = 150; // latency readings kept for the tooltip: 5 minutes at 2 s polling
// Live samples are kept for the longest live window. 1 hour at 2 s is 1800 points.
const LIVE_KEEP_MS = 3600_000;
const LIVE_MAX = 4000;
// Graph windows. Live ones draw the samples the widget collects; history ones come from
// the gateway's 5-minute and hourly reports (averages), refreshed every 5 minutes.
const VIEWS = {
  "5m": { span: 5 * 60_000, gap: 30_000 },
  "1h": { span: 3600_000, gap: 30_000 },
  "24h": { span: 86_400_000, gap: 15 * 60_000, history: "5-min avg" },
  "7d": { span: 7 * 86_400_000, gap: 3 * 3600_000, history: "hourly avg" },
};
const HISTORY_EVERY_MS = 5 * 60_000;
const $ = (id) => document.getElementById(id);
const canvas = $("graph");
const ctx = canvas.getContext("2d");
const css = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

/**
 * Sets a footer figure's tooltip (null removes it) and the same text, read in order after
 * the figure, for screen readers, which do not reliably read tooltips on plain text.
 */
function setDetail(id, text) {
  const el = $(id);
  if (text) {
    if (el.title !== text) el.title = text;
  } else {
    el.removeAttribute("title");
  }
  const sr = $(id + "-detail");
  const spoken = text ? ", " + text.replace(/\u2193/g, "download").replace(/\u2191/g, "upload").replace(/\s+\/\s+/g, ", ").replace(/\n/g, ". ") : "";
  if (sr.textContent !== spoken) sr.textContent = spoken;
}

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

// The selected window, remembered on this PC. Browser storage can be unavailable.
let view = "5m";
try {
  const saved = localStorage.getItem("graph-view");
  if (saved && VIEWS[saved]) view = saved;
} catch {}

// Gateway history per window: { points, at, error }.
const history = {};
// Where the pointer is over the graph, as a fraction of its width, or null.
let hoverX = null;

/** Points for the selected window, the window's end time, and the line-break gap. */
function viewData() {
  const v = VIEWS[view];
  const end = Date.now();
  const source = v.history ? (history[view] && history[view].points) || [] : samples;
  return { v, end, points: visiblePoints(source, end, v.span) };
}

function draw() {
  const size = fitCanvas();
  if (!size) return;
  const { w, h } = size;
  ctx.clearRect(0, 0, w, h);
  const { v, end, points } = viewData();
  const { top, topLabel, halfLabel } = yScale(points);
  const x = (t) => w - ((end - t) / v.span) * w;
  const y = (val) => h - 2 - (val / top) * (h - 14);

  // Solid baseline; dashed grid lines at the top of the scale and at 50%, labelled.
  ctx.strokeStyle = css("--muted");
  ctx.lineWidth = 1;
  ctx.globalAlpha = 0.25;
  ctx.beginPath();
  ctx.moveTo(0, h - 0.5);
  ctx.lineTo(w, h - 0.5);
  ctx.stroke();
  ctx.setLineDash([2, 3]);
  ctx.beginPath();
  for (const val of [top, top / 2]) {
    ctx.moveTo(0, Math.round(y(val)) + 0.5);
    ctx.lineTo(w, Math.round(y(val)) + 0.5);
  }
  ctx.stroke();
  ctx.setLineDash([]);
  ctx.globalAlpha = 1;
  for (const [key, color] of [["rx", "--down"], ["tx", "--up"]]) {
    ctx.strokeStyle = css(color);
    ctx.lineWidth = 1.5;
    for (const run of splitAtGaps(points, v.gap)) {
      if (run.length < 2) continue;
      // Curves through the midpoints between samples: smoother, and never overshoots a peak.
      const pts = run.map((s) => [x(s.t), y(s[key])]);
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

  if (points.length) {
    ctx.fillStyle = css("--muted");
    ctx.font = "10px 'Segoe UI', system-ui, sans-serif";
    ctx.textBaseline = "bottom";
    // Drawn over the lines, with a halo in the card colour, so they stay readable.
    ctx.strokeStyle = css("--bg");
    ctx.lineWidth = 3;
    ctx.lineJoin = "round";
    const label = (text, ly) => {
      ctx.strokeText(text, 1, ly);
      ctx.fillText(text, 1, ly);
    };
    // The hover readout sits over the top label, so that one steps aside while hovering.
    if (hoverX === null) label(topLabel, Math.round(y(top)) - 1);
    label(halfLabel, Math.round(y(top / 2)) - 1);
  }

  // Hover: a crosshair on the nearest point and its values in the readout.
  const p = hoverX === null ? null : nearestPoint(points, end - (1 - hoverX) * v.span);
  if (p) {
    const px = Math.round(x(p.t)) + 0.5;
    ctx.strokeStyle = css("--muted");
    ctx.lineWidth = 1;
    ctx.globalAlpha = 0.6;
    ctx.beginPath();
    ctx.moveTo(px, 0);
    ctx.lineTo(px, h);
    ctx.stroke();
    ctx.globalAlpha = 1;
    for (const [key, color] of [["rx", "--down"], ["tx", "--up"]]) {
      ctx.fillStyle = css(color);
      ctx.beginPath();
      ctx.arc(px, y(p[key]), 2.5, 0, Math.PI * 2);
      ctx.fill();
    }
  }
  showReadout(p, v);
  describeGraph(points, v, topLabel);
}

const WINDOW_NAMES = { "5m": "last 5 minutes, live", "1h": "last hour, live", "24h": "last 24 hours, 5-minute averages", "7d": "last 7 days, hourly averages" };

/** The graph's accessible name: the window and the highest and latest rates in it. */
function describeGraph(points, v, topLabel) {
  let text = "Throughput graph, " + WINDOW_NAMES[view] + ".";
  if (points.length) {
    const last = points[points.length - 1];
    const peak = (k) => Math.max(...points.map((p) => p[k]));
    text +=
      " Download peak " + formatRate(peak("rx")) + ", latest " + formatRate(last.rx) + "." +
      " Upload peak " + formatRate(peak("tx")) + ", latest " + formatRate(last.tx) + "." +
      " Scale up to " + topLabel + ".";
  } else {
    text += " No data yet.";
  }
  if (canvas.getAttribute("aria-label") !== text) canvas.setAttribute("aria-label", text);
}

/** The readout over the graph: hovered values, else loading or error text for history. */
function showReadout(p, v) {
  const el = $("readout");
  let text = "";
  if (p) {
    text = formatClock(p.t, v.span) + "  \u2193 " + formatRate(p.rx) + "  \u2191 " + formatRate(p.tx);
    if (v.history) text += "  (" + v.history + ")";
  } else if (v.history) {
    const hist = history[view];
    if (!hist) text = "Loading...";
    else if (hist.error) text = "History unavailable: " + hist.error;
    else if (!hist.points.length) text = "No history from the gateway yet";
  }
  el.hidden = !text;
  if (el.textContent !== text) el.textContent = text;
}

/** Fetches the gateway history for the selected window if it is missing or old. */
function refreshHistory(force) {
  const v = VIEWS[view];
  if (!v.history || !tauri || !tauri.core) return;
  const name = view;
  const hist = history[name];
  if (!force && hist && Date.now() - hist.at < HISTORY_EVERY_MS) return;
  if (hist && hist.loading) return;
  history[name] = { points: hist ? hist.points : [], at: hist ? hist.at : 0, loading: true };
  tauri.core
    .invoke("history", { window: name })
    .then((points) => (history[name] = { points, at: Date.now() }))
    .catch((err) => (history[name] = { points: [], at: Date.now(), error: String(err) }))
    .finally(draw);
  draw();
}

function selectView(name) {
  view = name;
  try {
    localStorage.setItem("graph-view", name);
  } catch {}
  for (const b of document.querySelectorAll("#views button")) {
    b.setAttribute("aria-pressed", String(b.dataset.view === name));
  }
  refreshHistory(false);
  draw();
}

// Rates arrive every poll interval (2 s); with backoff they can stretch to 60 s.
// Past STALE_MS with no rates, the widget dims and says so.
const STALE_MS = 15_000;
let lastRatesAt = 0;

// Recent latency readings, for the tooltip's min / average / max.
let latencies = [];

function onRates({ rx, tx, latency, monitors, availability_pct: availability, availability_period_s: period, wan2 }) {
  if (away) return;
  lastRatesAt = Date.now();
  const now = Date.now();
  samples = trimBefore(samples.concat([{ t: now, rx, tx }]), now - LIVE_KEEP_MS, LIVE_MAX);
  latencies = pushSample(latencies, { v: typeof latency === "number" ? latency : null, t: Date.now() }, MAX_POINTS);
  $("rx").textContent = formatRate(rx);
  $("tx").textContent = formatRate(tx);
  $("latency").textContent = formatLatency(latency);
  showLatencyTip(monitors || [], availability, period, wan2);
  draw();
}

/** Per-target latency, the recent range and availability. Only rewritten when it changes. */
function showLatencyTip(monitors, availability, period, wan2) {
  const lines = ["Latency from the gateway to the internet, measured by the gateway"];
  for (const m of monitors) lines.push(monitorName(m.target) + ": " + formatLatency(m.latency));
  const st = latencyStats(latencies.map((l) => l.v));
  if (st) {
    const mins = Math.max(1, Math.round((Date.now() - latencies[0].t) / 60_000));
    lines.push("Last " + mins + " min: avg " + st.avg + ", min " + st.min + ", max " + st.max + " ms");
  }
  if (typeof availability === "number") {
    lines.push("Availability " + availability.toFixed(2) + "%" + (period ? " over the last " + formatPeriod(period) : ""));
  }
  lines.push(...secondWanLines(wan2));
  const tip = lines.join("\n");
  setDetail("latency", tip);
}

// Seconds the WAN link had been up when last fetched (every 60 s), and when that was.
// The display counts on from there so it does not sit still between fetches.
let uptime = null;

function onUptime({ seconds, device_seconds: deviceSeconds, isp }) {
  const num = (n) => (typeof n === "number" ? n : null);
  // Keep the record if either figure is known: a gateway can report its own uptime while
  // its WAN link uptime is missing (for example while the link is down).
  const wan = num(seconds);
  const device = num(deviceSeconds);
  uptime = wan === null && device === null ? null : { seconds: wan, deviceSeconds: device, isp: isp || null, at: Date.now() };
  showUptime();
}

const WHEN = { dateStyle: "medium", timeStyle: "short" };

/** "WAN link up for 29h 33m (since 19 Oct 2026, 6:12 am)", or null if unknown. */
function since(label, seconds, elapsed) {
  if (seconds === null) return null;
  const s = seconds + elapsed;
  return label + " " + formatDuration(s) + " (since " + new Date(Date.now() - s * 1000).toLocaleString([], WHEN) + ")";
}

function showUptime(stale = false) {
  const el = $("uptime");
  if (!uptime || away || stale) {
    el.textContent = "-";
    setDetail("uptime", null); // fall back to the field's general tooltip
    return;
  }
  const elapsed = (Date.now() - uptime.at) / 1000;
  // The figure is the WAN link's uptime; with no WAN figure the tooltip still has the rest.
  const text = uptime.seconds === null ? "-" : formatDuration(uptime.seconds + elapsed);
  const tip = [
    uptime.isp && "ISP: " + uptime.isp,
    uptime.seconds === null ? "WAN link uptime not reported" : since("WAN link up for", uptime.seconds, elapsed),
    since("Gateway up for", uptime.deviceSeconds, elapsed),
  ].filter(Boolean).join("\n");
  // Only written when changed (about once a minute), so an open tooltip does not flicker.
  if (el.textContent !== text) el.textContent = text;
  setDetail("uptime", tip);
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
    latencies = [];
    $("latency").textContent = "-";
    setDetail("latency", null); // fall back to the field's general tooltip
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
  setDetail("month", "\u2193 " + formatBytes(rx) + "  /  \u2191 " + formatBytes(tx));
}

function onIp({ external, wan, cgnat }) {
  $("ip").textContent = external || wan || "-";
  setDetail("ip", [external && "Public IP " + external, wan && "Gateway WAN " + wan].filter(Boolean).join("  /  "));
  $("cgnat").hidden = !cgnat;
  $("cgnat").title = wan ? "Gateway WAN address is " + wan : "";
}

function onStatus(message) {
  if (away) return;
  $("status").hidden = !message;
  $("status").textContent = message || "";
}

const tauri = window.__TAURI__;

for (const b of document.querySelectorAll("#views button")) {
  b.addEventListener("click", () => selectView(b.dataset.view));
}
canvas.addEventListener("mousemove", (e) => {
  const r = canvas.getBoundingClientRect();
  hoverX = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
  draw();
});
canvas.addEventListener("mouseleave", () => {
  hoverX = null;
  draw();
});
// Keyboard: arrows move the readout along the graph (Shift for bigger steps), Home and
// End jump to the ends, Escape stops. The readout is a live region, so it is announced.
canvas.addEventListener("keydown", (e) => {
  const step = e.shiftKey ? 0.1 : 0.02;
  const at = hoverX === null ? 1 : hoverX;
  const moves = { ArrowLeft: at - step, ArrowRight: at + step, Home: 0, End: 1 };
  if (e.key in moves) hoverX = Math.min(1, Math.max(0, moves[e.key]));
  else if (e.key === "Escape") hoverX = null;
  else return;
  e.preventDefault();
  draw();
});
canvas.addEventListener("blur", () => {
  hoverX = null;
  draw();
});
// History windows refresh while shown; live ones redraw on every sample anyway.
setInterval(() => {
  if (VIEWS[view].history) {
    refreshHistory(false);
    draw();
  }
}, 60_000);

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
  // Preview options in the page URL: ?away for the away state, ?theme=dark or light,
  // ?nowan for a gateway that reports no WAN link uptime.
  const query = new URLSearchParams(location.search);
  onUptime({ seconds: query.has("nowan") ? null : 106_380, device_seconds: 3_235_463, isp: "ExampleISP" });
  if (query.has("theme")) applyTheme(query.get("theme"));
  if (query.has("away")) onAway(true);
  // Synthetic gateway history: a daily cycle with some noise, at the report's bucket size.
  const fake = (span, step) => {
    const out = [];
    for (let t = Date.now() - span; t <= Date.now(); t += step) {
      const day = Math.sin(((t / 3600_000) % 24) / 24 * Math.PI * 2 - 1.5);
      out.push({ t, rx: Math.max(0, 900_000 + 700_000 * day + Math.random() * 300_000), tx: Math.max(0, 120_000 + 80_000 * day + Math.random() * 40_000) });
    }
    return out;
  };
  history["24h"] = { points: fake(86_400_000, 300_000), at: Date.now() };
  history["7d"] = { points: fake(7 * 86_400_000, 3600_000), at: Date.now() };
  if (query.has("view") && VIEWS[query.get("view")]) selectView(query.get("view"));
  let t = 0;
  setInterval(() => {
    t += 1;
    onRates({
      rx: Math.max(0, 600_000 + 400_000 * Math.sin(t / 45) + Math.random() * 150_000),
      tx: Math.max(0, 150_000 + 90_000 * Math.sin(t / 30) + Math.random() * 40_000),
      latency: 9 + Math.round(Math.random() * 6),
      monitors: [
        { target: "www.microsoft.com", latency: 10 },
        { target: "google.com", latency: 12 },
        { target: "1.1.1.1", latency: 7 },
      ],
      availability_pct: 99.7222,
      availability_period_s: 86400,
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

selectView(view);
