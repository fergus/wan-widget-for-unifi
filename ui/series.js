// Pure helpers for the live graph and number formatting. No DOM access.

/** Appends a sample and keeps at most `max` entries. Returns a new array. */
export function pushSample(buf, sample, max) {
  const next = buf.concat([sample]);
  return next.length > max ? next.slice(next.length - max) : next;
}

/** Nice-ish upper bound for the y axis so the graph does not jitter every tick. */
export function yMax(buf) {
  let peak = 0;
  for (const s of buf) peak = Math.max(peak, s.rx, s.tx);
  if (peak <= 0) return 1;
  const pow = Math.pow(10, Math.floor(Math.log10(peak)));
  for (const step of [1, 2, 5, 10]) {
    if (peak <= step * pow) return step * pow;
  }
  return 10 * pow;
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
