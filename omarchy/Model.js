// Pure helpers for the Omarchy plugin: gateway reply parsing, billing-cycle sums,
// graph series and number formatting. Plain script (no imports) so QML can load it
// with `import "Model.js" as Model` and node can load it for the tests.
// Mirrors crates/widget-core (unifi.rs, history.rs, usage.rs) and ui/series.js.

function parseEnvelope(body) {
  try {
    var data = JSON.parse(String(body || ""))
    if (!data || !data.meta || data.meta.rc !== "ok" || !Array.isArray(data.data)) return null
    return data.data
  } catch (e) {
    return null
  }
}

function num(v) {
  return typeof v === "number" && isFinite(v) ? v : null
}

function monitors(list) {
  return (list || []).map(function(m) {
    var l = num(m.latency_average)
    return { target: String(m.target), latencyMs: l === null ? null : Math.max(0, Math.round(l)) }
  })
}

// stat/health: live rates in bytes per second, public-ish IP, latency and per-target stats.
function parseHealth(body) {
  var rows = parseEnvelope(body)
  if (!rows) return null
  var wan = null, www = null
  rows.forEach(function(r) {
    if (r.subsystem === "wan") wan = r
    else if (r.subsystem === "www") www = r
  })
  if (!wan) return null
  var stats = wan.uptime_stats || {}
  var w1 = stats.WAN || {}
  var w2 = stats.WAN2
  var latency = www ? num(www.latency) : null
  return {
    rx: Math.max(0, Math.floor(num(wan["rx_bytes-r"]) || 0)),
    tx: Math.max(0, Math.floor(num(wan["tx_bytes-r"]) || 0)),
    wanIp: wan.wan_ip || "",
    ispName: wan.isp_name || "",
    latencyMs: latency === null ? null : Math.max(0, Math.round(latency)),
    monitors: monitors(w1.monitors),
    availabilityPct: num(w1.availability),
    availabilityPeriodS: num(w1.time_period),
    wan2: w2 ? { monitors: monitors(w2.monitors), availabilityPct: num(w2.availability) } : null,
    internetStatus: www && www.status ? String(www.status) : ""
  }
}

// stat/device: the gateway is the device with a wan1 block.
function parseDevice(body) {
  var rows = parseEnvelope(body)
  if (!rows) return null
  var gw = null
  for (var i = 0; i < rows.length; i++) {
    if (rows[i].wan1) { gw = rows[i]; break }
  }
  if (!gw) return { wanS: null, deviceS: null }
  var secs = function(v) { v = num(v); return v === null ? null : Math.max(0, Math.floor(v)) }
  return { wanS: secs(gw.uplink && gw.uplink.uptime), deviceS: secs(gw.uptime) }
}

// stat/report/{5minutes,hourly,daily}.site buckets, oldest first.
function parseReport(body) {
  var rows = parseEnvelope(body)
  if (!rows) return null
  return rows.filter(function(r) { return num(r.time) !== null }).map(function(r) {
    return {
      startMs: r.time,
      rx: Math.max(0, Math.floor(num(r["wan-rx_bytes"]) || 0)),
      tx: Math.max(0, Math.floor(num(r["wan-tx_bytes"]) || 0))
    }
  }).sort(function(a, b) { return a.startMs - b.startMs })
}

function reportBody(startMs, endMs) {
  return JSON.stringify({ attrs: ["wan-rx_bytes", "wan-tx_bytes", "time"], start: startMs, end: endMs })
}

// Average rate per bucket, stamped at the bucket midpoint. The newest bucket is
// still filling, so divide by the time it has covered (at least a minute).
function bucketRates(buckets, bucketS, nowMs) {
  var fullMs = bucketS * 1000
  return buckets.filter(function(b) { return b.startMs <= nowMs }).map(function(b) {
    var covered = Math.min(fullMs, Math.max(60000, nowMs - b.startMs))
    var secs = covered / 1000
    return { t: b.startMs + covered / 2, rx: Math.round(b.rx / secs), tx: Math.round(b.tx / secs) }
  })
}

// Start of the billing cycle containing nowMs: local midnight on billingDay,
// clamped to the month's length. Uses the PC's timezone.
function cycleStart(nowMs, billingDay) {
  var now = new Date(nowMs)
  function at(y, m) {
    var last = new Date(y, m + 1, 0).getDate()
    return new Date(y, m, Math.min(Math.max(1, billingDay), last), 0, 0, 0, 0)
  }
  var start = at(now.getFullYear(), now.getMonth())
  if (now >= start) return start.getTime()
  return at(now.getFullYear(), now.getMonth() - 1).getTime()
}

function monthlyTotals(buckets, startMs) {
  var t = { rx: 0, tx: 0 }
  buckets.forEach(function(b) {
    if (b.startMs >= startMs) { t.rx += b.rx; t.tx += b.tx }
  })
  return t
}

// 100.64.0.0/10: carrier-grade NAT.
function isCgnat(ip) {
  var m = /^(\d+)\.(\d+)\.\d+\.\d+$/.exec(String(ip || ""))
  return !!m && +m[1] === 100 && +m[2] >= 64 && +m[2] <= 127
}

// Keeps at most max samples, newest last.
function pushSample(buf, sample, max) {
  var next = buf.concat([sample])
  return next.length > max ? next.slice(next.length - max) : next
}

// Y axis for samples in bytes/s: peak rounded up to a multiple of 10 in the unit
// it is labelled in (Mbit/s from 1 Mbit/s up, else kbit/s), at least 10.
function yScale(samples) {
  var peak = 0
  samples.forEach(function(s) { peak = Math.max(peak, s.rx, s.tx) })
  var bits = peak * 8
  var big = bits >= 1e6
  var unit = big ? 1e6 : 1e3
  var name = big ? "Mbit/s" : "kbit/s"
  var top = Math.max(10, Math.ceil(bits / unit / 10 - 1e-9) * 10)
  return { top: top * unit / 8, topLabel: top + " " + name, halfLabel: top / 2 + " " + name }
}

function formatRate(bytesPerSec) {
  var bits = bytesPerSec * 8
  if (bits >= 1e9) return (bits / 1e9).toFixed(2) + " Gbit/s"
  if (bits >= 1e6) return (bits / 1e6).toFixed(1) + " Mbit/s"
  if (bits >= 1e3) return (bits / 1e3).toFixed(0) + " kbit/s"
  return Math.round(bits) + " bit/s"
}

// Compact for the bar: "8.0M", "512k".
function formatRateShort(bytesPerSec) {
  var bits = bytesPerSec * 8
  if (bits >= 1e9) return (bits / 1e9).toFixed(1) + "G"
  if (bits >= 1e6) return (bits / 1e6).toFixed(bits >= 1e8 ? 0 : 1) + "M"
  if (bits >= 1e3) return Math.round(bits / 1e3) + "k"
  return Math.round(bits) + "b"
}

function formatBytes(n) {
  if (n >= 1e12) return (n / 1e12).toFixed(2) + " TB"
  if (n >= 1e9) return (n / 1e9).toFixed(1) + " GB"
  if (n >= 1e6) return (n / 1e6).toFixed(0) + " MB"
  return Math.round(n / 1e3) + " kB"
}

function formatDuration(seconds) {
  var s = Math.max(0, Math.floor(seconds))
  var d = Math.floor(s / 86400)
  var h = Math.floor((s % 86400) / 3600)
  var m = Math.floor((s % 3600) / 60)
  if (d > 0) return d + "d " + h + "h"
  if (h > 0) return h + "h " + m + "m"
  if (m > 0) return m + "m"
  return "<1m"
}

function formatLatency(ms) {
  return typeof ms === "number" ? Math.round(ms) + " ms" : "-"
}

function monitorName(target) {
  var t = String(target).toLowerCase()
  if (t.indexOf("microsoft") >= 0) return "Microsoft"
  if (t.indexOf("google") >= 0) return "Google"
  if (t === "1.1.1.1" || t.indexOf("cloudflare") >= 0) return "Cloudflare"
  return String(target)
}

if (typeof module !== "undefined" && module.exports) {
  module.exports = {
    parseHealth: parseHealth, parseDevice: parseDevice, parseReport: parseReport,
    reportBody: reportBody, bucketRates: bucketRates, cycleStart: cycleStart,
    monthlyTotals: monthlyTotals, isCgnat: isCgnat, pushSample: pushSample,
    yScale: yScale, formatRate: formatRate, formatRateShort: formatRateShort,
    formatBytes: formatBytes, formatDuration: formatDuration,
    formatLatency: formatLatency, monitorName: monitorName
  }
}
