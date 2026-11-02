const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const M = require("./Model.js");

const fixture = (n) =>
  fs.readFileSync(path.join(__dirname, "../crates/widget-core/tests/fixtures", n), "utf8");

test("parseHealth reads rates, IP, latency and monitors", () => {
  const h = M.parseHealth(fixture("health.json"));
  assert.equal(h.rx, 513102);
  assert.equal(h.tx, 356527);
  assert.equal(h.wanIp, "100.64.10.20");
  assert.equal(h.ispName, "ExampleISP");
  assert.ok(h.monitors.length > 0);
  assert.ok(h.availabilityPct > 99);
});

test("parseHealth rejects errors and replies without a wan entry", () => {
  assert.equal(M.parseHealth(fixture("error.json")), null);
  assert.equal(M.parseHealth("not json"), null);
  assert.equal(M.parseHealth('{"meta":{"rc":"ok"},"data":[{"subsystem":"wlan"}]}'), null);
});

test("parseDevice finds the gateway's uptimes", () => {
  const d = M.parseDevice(fixture("device.json"));
  assert.equal(typeof d.wanS, "number");
  assert.equal(typeof d.deviceS, "number");
});

test("parseReport sorts buckets and bucketRates divides the newest by time covered", () => {
  const body = JSON.stringify({
    meta: { rc: "ok" },
    data: [
      { time: 300000, "wan-rx_bytes": 30000, "wan-tx_bytes": 0 },
      { time: 0, "wan-rx_bytes": 300000, "wan-tx_bytes": 30000 },
    ],
  });
  const b = M.parseReport(body);
  assert.deepEqual(b.map((x) => x.startMs), [0, 300000]);
  const r = M.bucketRates(b, 300, 360000);
  assert.deepEqual(r[0], { t: 150000, rx: 1000, tx: 100 });
  assert.deepEqual(r[1], { t: 330000, rx: 500, tx: 0 }); // 60 s covered
});

test("cycleStart clamps the billing day and rolls back before it", () => {
  const feb = new Date(2026, 1, 15, 12).getTime();
  assert.equal(M.cycleStart(feb, 31), new Date(2026, 0, 31).getTime());
  const mar = new Date(2026, 2, 5, 12).getTime();
  assert.equal(M.cycleStart(mar, 10), new Date(2026, 1, 10).getTime());
  assert.equal(M.cycleStart(mar, 1), new Date(2026, 2, 1).getTime());
});

test("monthlyTotals sums buckets from the cycle start", () => {
  const t = M.monthlyTotals(
    [{ startMs: 0, rx: 5, tx: 1 }, { startMs: 10, rx: 7, tx: 2 }, { startMs: 20, rx: 1, tx: 1 }], 10);
  assert.deepEqual(t, { rx: 8, tx: 3 });
});

test("isCgnat covers 100.64.0.0/10 only", () => {
  assert.ok(M.isCgnat("100.64.0.1"));
  assert.ok(M.isCgnat("100.127.255.254"));
  assert.ok(!M.isCgnat("100.128.0.1"));
  assert.ok(!M.isCgnat("203.0.113.5"));
  assert.ok(!M.isCgnat(""));
});

test("yScale rounds up to multiples of 10", () => {
  assert.equal(M.yScale([]).topLabel, "10 kbit/s");
  const s = M.yScale([{ rx: 1_500_000, tx: 0 }]); // 12 Mbit/s
  assert.equal(s.topLabel, "20 Mbit/s");
  assert.equal(s.halfLabel, "10 Mbit/s");
  assert.equal(s.top, 2_500_000);
});

test("formatting", () => {
  assert.equal(M.formatRate(1_000_000), "8.0 Mbit/s");
  assert.equal(M.formatRateShort(1_000_000), "8.0M");
  assert.equal(M.formatRateShort(64_000), "512k");
  assert.equal(M.formatBytes(463e9), "463.0 GB");
  assert.equal(M.formatDuration(29 * 3600), "1d 5h");
  assert.equal(M.formatLatency(null), "-");
  assert.equal(M.monitorName("1.1.1.1"), "Cloudflare");
});

test("pushSample caps the buffer", () => {
  assert.deepEqual(M.pushSample([1, 2, 3], 4, 3), [2, 3, 4]);
});
