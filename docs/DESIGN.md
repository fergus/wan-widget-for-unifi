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
| Latency | `GET .../stat/health`, `www` entry, `latency` (ms) | Measured by the gateway to the internet, not PC to gateway. Equals `wan.uptime_stats.WAN.latency_average` |
| Latency per target, availability | `stat/health`, `wan` entry, `uptime_stats.WAN.monitors[]` (`target`, `latency_average`) and `uptime_stats.WAN.availability` over `time_period` (24 h) | The three targets the UniFi app lists (Microsoft, Google, Cloudflare). `alerting_monitors` are separate and ignored. Recorded from a UCG Ultra on gateway firmware 5.1 |
| WAN online or offline (notifications) | `stat/device`, the gateway's `last_wan_status`, e.g. `{"WAN":"online"}` (one key per WAN), read with the uptime every 60 s | Confirmed on a UCG Ultra. "online" and "offline" are acted on, other values ignored. A gateway without it falls back to health's `www` status ("ok" up, "error" down). A first reading is never a change, so starting during an outage is quiet |
| Time since last WAN disconnect | `GET .../stat/device`, the device with a non-null `wan1` block (the gateway), its `uplink.uptime` in seconds, every 60 s | Confirmed on a UCG Ultra: `uplink.uptime` is the link's age, while the device's own `uptime` is time since boot and `wan1` has no uptime. Shown as "-" if absent |
| Console detection | `GET /api/system` (no API key) | A UniFi OS console answers with a `hardware` object (`shortname` "UDRULT" on a UCG Ultra). Used by Find gateway, which tries the .1 address of the PC's /24, then 192.168.1.1 and 192.168.0.1 |
| Sites | `GET /proxy/network/api/self/sites` | `name` is the id used in URLs ("default"), `desc` the display name |
| Legacy server detection | `GET /status` (no API key) | The legacy self-hosted Network Server answers with `meta.server_version`. It has no API keys, so Test connection explains instead of reporting a rejected key. On UniFi OS this path is an HTML page |
| Graph history | `POST .../stat/report/5minutes.site` (24 h) and `.../hourly.site` (7 d), same request and reply as the daily report | Confirmed on a UCG Ultra: 288 and 168 buckets with no gaps. Rate is bytes over the bucket length; the newest bucket is still filling, so it is divided by the time covered so far (at least a minute). Fetched when the window is chosen and every 5 minutes while shown. The 5 m and 1 h windows use the live samples (kept for an hour in memory). Peak heights therefore differ by window: averaging over 300 s or 3600 s spreads a burst across the bucket, so the same traffic peaks lower on 24 h than live, and lower again on 7 d (on a UCG Ultra over one day, the highest 5-minute average was 124 Mbit/s and the highest hourly 63 Mbit/s). The README's "Reading the graph" explains this to users |
| Second WAN | `stat/health`, `wan` entry, `uptime_stats.WAN2` | Assumed to have the same shape as `WAN` (one Home Assistant integration reads it this way). Not confirmed on dual-WAN hardware; whether `rx_bytes-r` covers both links is unknown |
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
| Away from home | After 3 polls with no answer at all (timeout or no connection) **and** a PC address different from the one at the last successful poll, stop polling until the address changes, the user clicks Check now, or settings are saved | Cheap and offline: the address comes from a UDP `connect` that sends nothing. The address check keeps a gateway reboot at home on the normal backoff path. Wrong key, wrong site and certificate changes never count as away |


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
