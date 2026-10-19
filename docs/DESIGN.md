# Design notes

How the widget gets its numbers, why it is built the way it is, and what is still open.

## Data sources

All gateway calls use the classic UniFi Network local API on UniFi OS
(`/proxy/network/api/s/<site>/...`) with an `X-API-KEY` header.

| What | Endpoint | Notes |
|---|---|---|
| Live rates | `GET .../stat/health`, `wan` entry, `rx_bytes-r` / `tx_bytes-r` | Already bytes per second, so no counter-delta maths |
| Billing timezone | `GET .../stat/sysinfo`, `timezone` | Used unless the timezone setting overrides it, falling back to the PC's zone |
| Monthly usage | `POST .../stat/report/daily.site` with `wan-rx_bytes`, `wan-tx_bytes`, summed from the cycle start | The controller keeps history while the PC is off. The newest bucket already holds today's partial total, so live rates are not added on top |
| External IP | A configurable public lookup (default `https://api.ipify.org`, every 5 minutes), plus the gateway's WAN IP from `stat/health` | Behind carrier-grade NAT the gateway's WAN address is not the public one |

The classic API is undocumented and may change between UniFi releases. All parsing is
isolated in `widget-core::unifi` and tested against recorded, sanitised fixtures.

## Findings that shaped the design

- **CGNAT.** On some ISPs `stat/health` reports a `wan_ip` in 100.64.0.0/10. A lookup
  service is then the only way to show the real public address, and the widget shows a
  CGNAT badge.
- **Daily buckets start at local midnight in the controller's timezone.** The billing
  cycle start therefore uses a real IANA zone, not a fixed offset, and is tested across
  DST, month-length and year boundaries. The zone comes from the controller
  (`stat/sysinfo`), so it matches the buckets unless the user overrides it.
- **Self-signed certificates.** UniFi gateways ship one, so public CA validation is
  replaced by trust-on-first-use pinning of the leaf certificate's SHA-256.

## Decisions

| Area | Decision | Why |
|---|---|---|
| Stack | Tauri 2: Rust core and a static web UI | Small installer (about 3 MB), low idle memory, transparent frameless window |
| Window | Frameless, transparent, always on top or pinned to the desktop, draggable, remembers its position | Simplest reliable behaviour on Windows 11 |
| Tray and autostart | Tray menu plus a Settings checkbox, both reading the real registry state. Autostart is off until you opt in. Click-through is never saved, so it cannot lock you out | Avoids a saved flag drifting from the OS |
| Secrets | API key in Windows Credential Manager, entered at first run, never in the settings file | Keeps the key out of plain-text config and backups |
| Key isolation | The UI never calls the gateway; only the Rust process holds the key | The web view cannot leak it |
| Gateway errors | Health polling backs off up to 60 s; a failed monthly fetch retries after 60 s; the widget dims and says "No recent data" after 15 s without rates | Stays quiet when the gateway is down and recovers on its own |

## Architecture

```
ui/ (static JS, canvas graph)        <-- events: wan-rates, monthly-usage, ip-info, poll-status, theme
        ^
src-tauri/ (Rust)
  poller   health every poll interval (2 s default) | daily report every 5 min | IP lookup every 5 min
  config   settings.json + Credential Manager
  pin      trust-on-first-use certificate pin
  tray, window_state
        |
crates/widget-core (pure, no I/O, tested on Linux)
  unifi    parse health and daily report, build report request
  usage    cycle_start(now, billing_day, tz), monthly_totals
  ip       is_cgnat, parse_lookup, ip_view
  settings struct, defaults, validate
```

## Testing

- `widget-core`: unit tests and recorded fixtures in `crates/widget-core/tests/fixtures/`
  (real response shapes: float-encoded byte counts, multi-subsystem health, the error
  envelope).
- `ui`: pure functions tested with `node --test`. The pages open in a plain browser with
  synthetic data for layout checks.
- `src-tauri`: kept thin. A mock HTTP server test for the poller is still to do.

## Open risks

- **Trust on first use.** The first connection decides which certificate is trusted.
  If the gateway's certificate is regenerated, the widget refuses it until you press
  Forget pinned certificate.
- **Timezone override.** A timezone setting that differs from the controller's shifts
  the cycle boundary by hours. Test connection warns when they differ.
- **Monthly figure lag.** If the controller updates the current day's bucket only
  hourly, the monthly total lags by up to an hour.
- **Polling cost.** Polling `stat/health` every 2 s is assumed cheap for the gateway but
  has not been measured.
- **API changes.** A UniFi release can change or remove the classic endpoints. The
  official Network integration API is the likely long-term replacement.
