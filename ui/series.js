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
