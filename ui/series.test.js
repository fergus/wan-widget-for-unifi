import { test } from "node:test";
import assert from "node:assert/strict";
import { pushSample, yScale, formatRate, formatBytes, formatDuration, formatLatency, latencyStats, monitorName, formatPeriod, secondWanLines,
  trimBefore, visiblePoints, nearestPoint, splitAtGaps, formatClock } from "./series.js";

test("pushSample caps the buffer and drops the oldest", () => {
  let buf = [];
  for (let i = 0; i < 5; i++) buf = pushSample(buf, { rx: i, tx: 0 }, 3);
  assert.deepEqual(buf.map((s) => s.rx), [2, 3, 4]);
});

test("pushSample does not mutate its input", () => {
  const a = [{ rx: 1, tx: 1 }];
  pushSample(a, { rx: 2, tx: 2 }, 10);
  assert.equal(a.length, 1);
});

const mbit = (n) => (n * 1e6) / 8; // Mbit/s in bytes/s

test("yScale rounds the peak up to a multiple of 10 Mbit/s, with a 50% label", () => {
  assert.deepEqual(yScale([{ rx: mbit(124), tx: mbit(3) }]), { top: mbit(130), topLabel: "130 Mbit/s", halfLabel: "65 Mbit/s" });
  assert.deepEqual(yScale([{ rx: mbit(1), tx: mbit(3) }]), { top: mbit(10), topLabel: "10 Mbit/s", halfLabel: "5 Mbit/s" });
  assert.equal(yScale([{ rx: mbit(1210), tx: 0 }]).topLabel, "1210 Mbit/s");
});

test("yScale keeps an exact multiple of 10 as the top", () => {
  assert.equal(yScale([{ rx: mbit(20), tx: 0 }]).topLabel, "20 Mbit/s");
  assert.equal(yScale([{ rx: mbit(20.01), tx: 0 }]).topLabel, "30 Mbit/s");
});

test("yScale uses kbit/s below 1 Mbit/s and at least 10 when idle", () => {
  assert.deepEqual(yScale([{ rx: 950_000 / 8, tx: 0 }]), { top: 950_000 / 8, topLabel: "950 kbit/s", halfLabel: "475 kbit/s" });
  assert.deepEqual(yScale([]), { top: 10_000 / 8, topLabel: "10 kbit/s", halfLabel: "5 kbit/s" });
  assert.equal(yScale([{ rx: 0, tx: 0 }]).topLabel, "10 kbit/s");
});

test("formatRate converts bytes/s to bits", () => {
  assert.equal(formatRate(513102), "4.1 Mbit/s");
  assert.equal(formatRate(125_000_000), "1.00 Gbit/s");
  assert.equal(formatRate(1000), "8 kbit/s");
  assert.equal(formatRate(0), "0 bit/s");
});

test("formatBytes uses decimal units", () => {
  assert.equal(formatBytes(1.234e12), "1.23 TB");
  assert.equal(formatBytes(69.6e9), "69.6 GB");
  assert.equal(formatBytes(5e6), "5 MB");
  assert.equal(formatBytes(500), "1 kB");
});

test("formatDuration shows the two largest units", () => {
  assert.equal(formatDuration(0), "<1m");
  assert.equal(formatDuration(59), "<1m");
  assert.equal(formatDuration(180), "3m");
  assert.equal(formatDuration(5 * 3600 + 12 * 60 + 40), "5h 12m");
  assert.equal(formatDuration(12 * 86400 + 4 * 3600 + 59 * 60), "12d 4h");
  assert.equal(formatDuration(-5), "<1m");
});

test("formatLatency rounds and falls back to a dash", () => {
  assert.equal(formatLatency(9), "9 ms");
  assert.equal(formatLatency(9.6), "10 ms");
  assert.equal(formatLatency(0), "0 ms");
  assert.equal(formatLatency(null), "-");
  assert.equal(formatLatency(undefined), "-");
});

test("latencyStats summarises numbers and ignores gaps", () => {
  assert.equal(latencyStats([]), null);
  assert.equal(latencyStats([null, undefined]), null);
  assert.deepEqual(latencyStats([8, null, 14, 11]), { avg: 11, min: 8, max: 14 });
});

test("monitorName uses the names the UniFi app shows", () => {
  assert.equal(monitorName("www.microsoft.com"), "Microsoft");
  assert.equal(monitorName("google.com"), "Google");
  assert.equal(monitorName("1.1.1.1"), "Cloudflare");
  assert.equal(monitorName("ping.ui.com"), "ping.ui.com");
});

test("formatPeriod picks hours or minutes", () => {
  assert.equal(formatPeriod(86400), "24 hours");
  assert.equal(formatPeriod(3600), "1 hour");
  assert.equal(formatPeriod(90 * 60), "2 hours");
  assert.equal(formatPeriod(300), "5 minutes");
});

test("secondWanLines is empty for a single-WAN gateway", () => {
  assert.deepEqual(secondWanLines(null), []);
  assert.deepEqual(secondWanLines(undefined), []);
});

test("secondWanLines lists the second WAN's monitors and availability", () => {
  const lines = secondWanLines({
    monitors: [{ target: "1.1.1.1", latency: 22 }, { target: "ping.ui.com", latency: null }],
    availability_pct: 97.5,
    availability_period_s: 86400,
  });
  assert.deepEqual(lines, [
    "Second WAN, Cloudflare: 22 ms",
    "Second WAN, ping.ui.com: -",
    "Second WAN availability 97.50% over the last 24 hours",
  ]);
});

test("secondWanLines says so when a second WAN reports nothing", () => {
  assert.deepEqual(secondWanLines({ monitors: [] }), ["Second WAN: no monitor data"]);
});

const pts = (...ts) => ts.map((t) => ({ t, rx: t, tx: 0 }));

test("trimBefore drops old points and caps the count", () => {
  assert.deepEqual(trimBefore(pts(1, 2, 3, 4), 3, 10).map((p) => p.t), [3, 4]);
  assert.deepEqual(trimBefore(pts(1, 2, 3, 4), 0, 2).map((p) => p.t), [3, 4]);
});

test("visiblePoints keeps the window plus one point before it", () => {
  const p = pts(0, 100, 200, 300, 400);
  assert.deepEqual(visiblePoints(p, 400, 150).map((x) => x.t), [200, 300, 400]);
  assert.deepEqual(visiblePoints(p, 400, 1000).map((x) => x.t), [0, 100, 200, 300, 400]);
  assert.deepEqual(visiblePoints(p, 1000, 100), []);
  assert.deepEqual(visiblePoints([], 1000, 100), []);
});

test("nearestPoint finds the closest time", () => {
  const p = pts(0, 100, 200);
  assert.equal(nearestPoint(p, 40).t, 0);
  assert.equal(nearestPoint(p, 60).t, 100);
  assert.equal(nearestPoint(p, 999).t, 200);
  assert.equal(nearestPoint(p, -5).t, 0);
  assert.equal(nearestPoint([], 5), null);
});

test("splitAtGaps breaks the line where data is missing", () => {
  const runs = splitAtGaps(pts(0, 2, 4, 40, 42), 10);
  assert.deepEqual(runs.map((r) => r.map((x) => x.t)), [[0, 2, 4], [40, 42]]);
  assert.deepEqual(splitAtGaps([], 10), []);
});

test("formatClock adds seconds for short windows and the day for a week", () => {
  const t = new Date(2026, 9, 21, 14, 5, 32).getTime(); // a Wednesday, local time
  assert.equal(formatClock(t, 5 * 60_000), "14:05:32");
  assert.equal(formatClock(t, 3600_000), "14:05");
  assert.equal(formatClock(t, 86_400_000), "14:05");
  assert.equal(formatClock(t, 7 * 86_400_000), "Wed 14:05");
});
