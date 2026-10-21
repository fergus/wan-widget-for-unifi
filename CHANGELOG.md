# Changelog

All notable changes to WAN Widget for UniFi. Versions follow
[semantic versioning](https://semver.org/).

## Unreleased

- Richer graph: switch between the last 5 minutes, hour, 24 hours and 7 days with the
  buttons over the graph. 24 hours and 7 days come from the gateway's own 5-minute and
  hourly averages, so they are there straight after a restart. Hover the graph to read
  the time and both rates at that point; the top line is labelled with its value.
- **Find gateway** in Settings looks for UniFi consoles on your network, and runs by
  itself the first time. It tries your PC's own network and the common UniFi addresses,
  without sending your API key.
- The **Site** field lists the sites your API key can see.
- UniFi OS Server (self-hosted UniFi OS, usually on port 11443) is supported and
  documented. Test connection explains when an address is the legacy self-hosted
  UniFi Network Server, which has no API keys and cannot be used.
- Dual-WAN gateways: the second WAN's latency and availability show in the latency
  tooltip when the gateway reports them. Not yet confirmed on dual-WAN hardware.
- Fixed a run of spaces in the middle of Test connection's timezone warning.

## 0.2.0 - 2026-10-20

- Latency (gateway to internet, as measured by the gateway) and how long the WAN link
  has been up since its last disconnect. Hover them for detail: latency per monitor
  target (Microsoft, Google, Cloudflare), the recent range and 24-hour availability;
  uptime with your ISP, when the link came up and when the gateway last booted.
- Away mode: when the PC leaves the gateway's network, the widget says "Away", dims the
  last values and stops polling entirely, including the IP lookup. It resumes when the
  PC's network address changes, when you click the banner or choose **Check gateway
  now** in the tray, or when you save settings. A gateway that is down while the PC is
  still on the home network keeps the existing retry-with-backoff behaviour.

## 0.1.1 - 2026-10-20

- The log now records the result of each update check, so update problems can be
  diagnosed from the log file.
- First release delivered through auto-update. If you have 0.1.0, the tray and
  Settings offer it within a day of starting the widget.

## 0.1.0 - 2026-10-20

First public release.

- Live WAN download and upload graph from the gateway's local API, sharp at any
  display scaling.
- Data used in the current billing month, with a configurable billing day. The month
  starts at midnight in the gateway's own timezone unless you override it.
- Public IP address, with a CGNAT badge when the gateway's WAN address differs.
- Tray menu: show or hide, click-through, pin to desktop, Start with Windows, updates.
- API key in Windows Credential Manager; the gateway's self-signed certificate is
  pinned per host on first use.
- Plain-English errors, and a log file for bug reports (Settings > Open log folder).
- Signed auto-update from GitHub releases. Nothing installs until you choose it.
