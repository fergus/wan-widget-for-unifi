import { test } from "node:test";
import assert from "node:assert/strict";
import { pushSample, yMax, formatRate, formatBytes } from "./series.js";

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

test("yMax of an empty or idle series is 1", () => {
  assert.equal(yMax([]), 1);
  assert.equal(yMax([{ rx: 0, tx: 0 }]), 1);
});

test("yMax rounds up to 1/2/5 x 10^n using the larger of rx and tx", () => {
  assert.equal(yMax([{ rx: 130, tx: 20 }]), 200);
  assert.equal(yMax([{ rx: 20, tx: 310 }]), 500);
  assert.equal(yMax([{ rx: 600, tx: 0 }]), 1000);
  assert.equal(yMax([{ rx: 500, tx: 0 }]), 500);
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
