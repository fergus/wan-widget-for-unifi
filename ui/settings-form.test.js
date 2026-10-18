import { test } from "node:test";
import assert from "node:assert/strict";
import { toForm, fromForm, errorList } from "./settings-form.js";

const backend = {
  gateway_url: "https://192.168.1.1",
  site: "default",
  poll_interval_ms: 2000,
  billing_day: 1,
  timezone: "Australia/Sydney",
  ip_lookup_url: "https://api.ipify.org",
  accept_invalid_certs: true,
};

test("toForm shows the poll interval in seconds and enables lookup when a URL is set", () => {
  const f = toForm(backend);
  assert.equal(f.pollSeconds, "2");
  assert.equal(f.lookupEnabled, true);
  assert.equal(f.ipLookupUrl, "https://api.ipify.org");
});

test("toForm marks lookup disabled for an empty URL", () => {
  assert.equal(toForm({ ...backend, ip_lookup_url: "" }).lookupEnabled, false);
});

test("form round-trips to the same backend settings", () => {
  const { settings, errors } = fromForm(toForm(backend));
  assert.deepEqual(errors, []);
  assert.deepEqual(settings, backend);
});

test("fromForm converts seconds to milliseconds, including fractions", () => {
  const f = { ...toForm(backend), pollSeconds: "1.5" };
  assert.equal(fromForm(f).settings.poll_interval_ms, 1500);
});

test("fromForm trims text fields", () => {
  const f = { ...toForm(backend), gatewayUrl: "  https://gw.lan  ", site: " default " };
  const s = fromForm(f).settings;
  assert.equal(s.gateway_url, "https://gw.lan");
  assert.equal(s.site, "default");
});

test("fromForm clears the lookup URL when lookup is switched off", () => {
  const f = { ...toForm(backend), lookupEnabled: false };
  assert.equal(fromForm(f).settings.ip_lookup_url, "");
});

test("fromForm rejects non-numeric and blank numbers, reporting both", () => {
  const f = { ...toForm(backend), pollSeconds: "fast", billingDay: "" };
  const { settings, errors } = fromForm(f);
  assert.equal(settings, null);
  assert.equal(errors.length, 2);
});

test("fromForm rejects a fractional billing day", () => {
  const { settings, errors } = fromForm({ ...toForm(backend), billingDay: "1.5" });
  assert.equal(settings, null);
  assert.equal(errors.length, 1);
});

test("fromForm requires a URL when lookup is enabled", () => {
  const f = { ...toForm(backend), ipLookupUrl: "   " };
  const { settings, errors } = fromForm(f);
  assert.equal(settings, null);
  assert.match(errors[0], /lookup URL/);
});

test("errorList normalises strings, arrays and Errors", () => {
  assert.deepEqual(errorList("boom"), ["boom"]);
  assert.deepEqual(errorList(["a", "b"]), ["a", "b"]);
  assert.deepEqual(errorList(new Error("bad")), ["bad"]);
});
