# UniFi Widget: plan

## Goal

A small always-on-top Windows 11 widget showing:

1. Real-time line graph of WAN download and upload throughput
2. Data used in the current billing month
3. External (public) IP address

Data comes from the UniFi gateway at `192.168.1.1` (UCG Ultra, Network app 10.6.x, verified 2026-10-18).

## Decisions

| Area | Decision | Why |
|---|---|---|
| Stack | Tauri 2: Rust core + static web UI | Small installer, low idle RAM, transparent frameless window |
| Source | UniFi classic local API with `X-API-KEY` | Verified working; exposes live rates and daily history |
| Live rates | `GET /proxy/network/api/s/<site>/stat/health`, `wan` entry, `rx_bytes-r` / `tx_bytes-r` | Already bytes/s, no counter-delta maths |
| Monthly usage | `POST .../stat/report/daily.site` (`wan-rx_bytes`, `wan-tx_bytes`), summed from cycle start | Controller keeps history while the PC is off; newest bucket already holds today's partial total, so do not add live rates |
| External IP | Configurable public lookup (default `https://api.ipify.org`, ~5 min) plus gateway WAN IP and a CGNAT badge | The gateway reports `100.96.x.x` (CGNAT), not the public IP |
| Window | Frameless, transparent, always-on-top, draggable, remembers position, tray with click-through toggle | Simplest reliable behaviour on Win11 |
| Secrets | API key in Windows Credential Manager; entered at first run; never in the settings file | Requirement from the user |

## Findings that shaped the design

- `stat/health` `wan_ip` is in 100.64.0.0/10 (carrier-grade NAT). A lookup service is the only way to show the real address.
- Daily buckets start at local midnight in the controller's timezone (UTC+10 / UTC+11 here). The billing cycle start therefore uses a real IANA zone (`Australia/Sydney` default), not a fixed offset. The zone setting must match the controller's.
- The classic API is undocumented and may change between UniFi releases. All parsing is isolated in `widget-core::unifi` and tested against recorded fixtures.
- Gateway certificate is self-signed. `accept_invalid_certs` defaults to true; certificate pinning is a later hardening step.

## Architecture

```
ui/ (static JS, canvas graph)        <-- events: wan-rates, monthly-usage, ip-info, poll-status
        ^
src-tauri/ (Rust)
  poller   2s health | 5 min daily report | 5 min IP lookup
  config   settings.json + Credential Manager
        |
crates/widget-core (pure, tested on Linux)
  unifi    parse health / daily report, build report request
  usage    cycle_start(now, billing_day, tz), monthly_totals
  ip       is_cgnat, parse_lookup, ip_view
  settings struct, defaults, validate
```

The UI never calls the gateway; the key stays in the Rust process.

## Milestones

| # | Milestone | Status |
|---|---|---|
| M0 | Probe the API; confirm endpoints and fields | Done |
| M1 | `widget-core` with fixture-driven tests | Done (30 tests) |
| M1b | UI graph helpers and formatters with tests | Done (6 tests) |
| M2 | Tauri shell, poller, config, first-run settings screen (gateway URL, API key, billing day, timezone, poll interval, lookup URL) | Shell scaffolded, **uncompiled**; settings screen not started |
| M3 | Tray icon (quit, click-through, settings), remembered window position, autostart | Not started |
| M4 | NSIS installer build on Windows, icons | Not started |
| M5 | Hardening: certificate pinning, backoff on gateway errors, stale-data indicator | Not started |

## Risks and open items

- **Rust shell unverified.** `src-tauri` has not been compiled (no WebKit/WebView2 toolchain here). Expect small fixes on first Windows build.
- **No UI to enter the API key yet.** The `set_api_key` / `save_settings` commands exist; the settings screen that calls them is M2.
- **Polling cost.** 2 s polling of `stat/health` on the gateway is assumed cheap; measure CPU on the UCG Ultra during M2.
- **Timezone mismatch** between the setting and the controller shifts the cycle boundary by hours. Consider reading the site timezone from the controller later.
- **Monthly figure lags** if the controller updates the current-day bucket only hourly. Verify during M2 and note in the UI if so.
- **The probe API key was pasted into a Claude Code session.** Rotate it in the UniFi Network app.

## Testing approach

- `widget-core`: unit tests plus recorded, sanitized fixtures in `crates/widget-core/tests/fixtures/` (real shapes: float-encoded byte counts, multi-subsystem health, error envelope). Time maths is tested across DST, year and month-length boundaries.
- `ui`: pure functions tested with `node --test`; the page runs in a plain browser with synthetic data for layout checks.
- Poller and config (`src-tauri`): thin by design; the plan is to add a mock HTTP server test once the shell compiles.
