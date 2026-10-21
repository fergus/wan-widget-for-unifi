// Pure helpers for the live graph and number formatting. No DOM access.

/** Appends a sample and keeps at most `max` entries. Returns a new array. */
export function pushSample(buf, sample, max) {
  const next = buf.concat([sample]);
  return next.length > max ? next.slice(next.length - max) : next;
}

/**
 * The y axis for samples in bytes/s: the top is the peak rounded up to a multiple of 10
 * in the unit it is labelled in (Mbit/s from 1 Mbit/s up, else kbit/s), at least 10.
 * Returns the top in bytes/s and the labels for the top and the 50% line.
 */
export function yScale(buf) {
  let peak = 0;
  for (const s of buf) peak = Math.max(peak, s.rx, s.tx);
  const bits = peak * 8;
  const [unit, name] = bits >= 1e6 ? [1e6, "Mbit/s"] : [1e3, "kbit/s"];
  const top = Math.max(10, Math.ceil(bits / unit / 10 - 1e-9) * 10);
  return { top: (top * unit) / 8, topLabel: top + " " + name, halfLabel: top / 2 + " " + name };
}

/** Bytes per second to a human string in megabits/second, e.g. "4.1 Mbit/s". */
export function formatRate(bytesPerSec) {
  const bits = bytesPerSec * 8;
  if (bits >= 1e9) return (bits / 1e9).toFixed(2) + " Gbit/s";
  if (bits >= 1e6) return (bits / 1e6).toFixed(1) + " Mbit/s";
  if (bits >= 1e3) return (bits / 1e3).toFixed(0) + " kbit/s";
  return Math.round(bits) + " bit/s";
}

/** Decimal (SI) byte count, matching how ISPs bill. */
export function formatBytes(n) {
  if (n >= 1e12) return (n / 1e12).toFixed(2) + " TB";
  if (n >= 1e9) return (n / 1e9).toFixed(1) + " GB";
  if (n >= 1e6) return (n / 1e6).toFixed(0) + " MB";
  return Math.round(n / 1e3) + " kB";
}

/** Whole seconds as the two largest units, e.g. "12d 4h", "5h 12m", "3m". */
export function formatDuration(seconds) {
  const s = Math.max(0, Math.floor(seconds));
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return d + "d " + h + "h";
  if (h > 0) return h + "h " + m + "m";
  if (m > 0) return m + "m";
  return "<1m";
}

/** Latency in whole milliseconds, or a dash when the gateway did not report one. */
export function formatLatency(ms) {
  return typeof ms === "number" ? Math.round(ms) + " ms" : "-";
}

/** Average, min and max of the numbers in `values` (non-numbers ignored), or null if none. */
export function latencyStats(values) {
  const v = values.filter((n) => typeof n === "number");
  if (v.length === 0) return null;
  return {
    avg: Math.round(v.reduce((a, b) => a + b, 0) / v.length),
    min: Math.min(...v),
    max: Math.max(...v),
  };
}

/** The provider the UniFi app names for a monitor target, else the target itself. */
export function monitorName(target) {
  const t = String(target).toLowerCase();
  if (t.includes("microsoft")) return "Microsoft";
  if (t.includes("google")) return "Google";
  if (t === "1.1.1.1" || t.includes("cloudflare")) return "Cloudflare";
  return String(target);
}

/** A window length in seconds as "24 hours", "90 minutes" or "1 hour". */
export function formatPeriod(seconds) {
  if (seconds >= 3600) {
    const h = Math.round(seconds / 3600);
    return h + (h === 1 ? " hour" : " hours");
  }
  const m = Math.max(1, Math.round(seconds / 60));
  return m + (m === 1 ? " minute" : " minutes");
}

/**
 * Tooltip lines for a dual-WAN gateway's second WAN, or [] when there is none. Each
 * monitor becomes "Second WAN, Cloudflare: 22 ms".
 */
export function secondWanLines(wan2) {
  if (!wan2) return [];
  const lines = (wan2.monitors || []).map((m) => "Second WAN, " + monitorName(m.target) + ": " + formatLatency(m.latency));
  if (typeof wan2.availability_pct === "number") {
    const over = wan2.availability_period_s ? " over the last " + formatPeriod(wan2.availability_period_s) : "";
    lines.push("Second WAN availability " + wan2.availability_pct.toFixed(2) + "%" + over);
  }
  return lines.length ? lines : ["Second WAN: no monitor data"];
}

/** Drops points older than `cutoff` (ms) and keeps at most `max`, newest last. */
export function trimBefore(points, cutoff, max) {
  let i = 0;
  while (i < points.length && points[i].t < cutoff) i++;
  const kept = points.slice(i);
  return kept.length > max ? kept.slice(kept.length - max) : kept;
}

/**
 * Points (sorted by `t`) inside the window ending at `end` and `span` ms long, plus
 * the one just before it, so the line enters from the left edge instead of starting
 * part-way across.
 */
export function visiblePoints(points, end, span) {
  const start = end - span;
  let i = points.findIndex((p) => p.t >= start);
  if (i === -1) return points.length && points[points.length - 1].t < start ? [] : points.slice();
  if (i > 0) i -= 1;
  return points.slice(i).filter((p) => p.t <= end);
}

/** The point (sorted by `t`) closest in time to `t`, or null for none. */
export function nearestPoint(points, t) {
  if (points.length === 0) return null;
  let lo = 0;
  let hi = points.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (points[mid].t < t) lo = mid + 1;
    else hi = mid;
  }
  const prev = points[lo - 1];
  return prev && t - prev.t < points[lo].t - t ? prev : points[lo];
}

/** Splits points into runs wherever two neighbours are more than `maxGap` ms apart. */
export function splitAtGaps(points, maxGap) {
  const runs = [];
  let run = [];
  for (const p of points) {
    if (run.length && p.t - run[run.length - 1].t > maxGap) {
      runs.push(run);
      run = [];
    }
    run.push(p);
  }
  if (run.length) runs.push(run);
  return runs;
}

const DAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const pad = (n) => String(n).padStart(2, "0");

/** Local time for the hover readout: seconds for short windows, the weekday for a week. */
export function formatClock(ms, spanMs) {
  const d = new Date(ms);
  const hm = pad(d.getHours()) + ":" + pad(d.getMinutes());
  if (spanMs <= 15 * 60_000) return hm + ":" + pad(d.getSeconds());
  if (spanMs > 26 * 3600_000) return DAYS[d.getDay()] + " " + hm;
  return hm;
}
