//! Background polling. Emits events to the UI; the API key never leaves this process.

use std::net::{IpAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use widget_core::{ip, settings::Settings, unifi, usage};

use crate::errors::{self, Failure, Peer};
use crate::{config, pin};

const MONTHLY_EVERY: Duration = Duration::from_secs(300);
const MONTHLY_RETRY: Duration = Duration::from_secs(60);
const IP_EVERY: Duration = Duration::from_secs(300);
const UPTIME_EVERY: Duration = Duration::from_secs(60);
/// Consecutive unreachable polls, with the PC's address changed, before the widget decides
/// it has left the gateway's network and pauses.
const AWAY_AFTER: u32 = 3;
/// Longest wait between health polls while the gateway keeps failing.
const MAX_BACKOFF_MS: u64 = 60_000;

/// Set by the widget or the tray to leave the away state and poll again at once.
static RESUME: AtomicBool = AtomicBool::new(false);

pub fn resume() {
    RESUME.store(true, Ordering::Relaxed);
}

fn take_resume() -> bool {
    RESUME.swap(false, Ordering::Relaxed)
}

/// The PC's outgoing address. Connecting a UDP socket sends nothing; it only makes the OS
/// pick the route, so this is free and changes whenever the PC joins another network.
pub(crate) fn local_ip() -> Option<IpAddr> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.0.2.1:9").ok()?;
    sock.local_addr().ok().map(|a| a.ip())
}

/// Away means the gateway has gone quiet and the PC's address is not the one it had when
/// the gateway last answered. Same address, same network: the gateway is down, not us,
/// so keep retrying with backoff.
fn should_go_away(unreachable_streak: u32, home: Option<IpAddr>, now: Option<IpAddr>) -> bool {
    unreachable_streak >= AWAY_AFTER && (home.is_none() || home != now)
}

/// Poll interval after `failures` consecutive gateway errors: doubles each time, capped.
fn backoff_ms(base_ms: u64, failures: u32) -> u64 {
    base_ms
        .saturating_mul(1u64 << failures.min(6))
        .min(MAX_BACKOFF_MS)
}

#[derive(Serialize, Clone)]
struct Rates {
    rx: u64,
    tx: u64,
    latency: Option<u32>,
    monitors: Vec<MonitorOut>,
    availability_pct: Option<f64>,
    availability_period_s: Option<u64>,
    wan2: Option<SecondWanOut>,
}

/// A dual-WAN gateway's second WAN, for the latency tooltip.
#[derive(Serialize, Clone)]
struct SecondWanOut {
    monitors: Vec<MonitorOut>,
    availability_pct: Option<f64>,
    availability_period_s: Option<u64>,
}

#[derive(Serialize, Clone)]
struct MonitorOut {
    target: String,
    latency: Option<u32>,
}

/// Seconds since the gateway's WAN link last came up, and since the gateway booted, as of
/// when they were fetched.
#[derive(Serialize, Clone)]
pub struct Uptime {
    seconds: Option<u64>,
    device_seconds: Option<u64>,
    isp: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct Monthly {
    rx: u64,
    tx: u64,
}

#[derive(Serialize, Clone)]
pub struct IpInfo {
    external: Option<String>,
    wan: Option<String>,
    cgnat: bool,
}

/// Last monthly and IP values. These are emitted every 5 minutes, so a page that
/// loads between emits would otherwise show blanks until the next one.
#[derive(Default)]
pub struct Latest {
    monthly: Mutex<Option<Monthly>>,
    ip: Mutex<Option<IpInfo>>,
    uptime: Mutex<Option<Uptime>>,
    away: AtomicBool,
}

#[derive(Serialize)]
pub struct Snapshot {
    monthly: Option<Monthly>,
    ip: Option<IpInfo>,
    uptime: Option<Uptime>,
    away: bool,
}

impl Latest {
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            monthly: self.monthly.lock().ok().and_then(|g| g.clone()),
            ip: self.ip.lock().ok().and_then(|g| g.clone()),
            uptime: self.uptime.lock().ok().and_then(|g| g.clone()),
            away: self.away.load(Ordering::Relaxed),
        }
    }
}

/// Gateway client. With accept_invalid_certs the gateway's self-signed certificate is
/// checked against the pin in pin_path instead of the public CA roots. `record` saves the
/// pin for a host seen for the first time; Test connection passes false.
fn client(s: &Settings, pin_path: &Path, record: bool) -> Result<reqwest::Client, String> {
    let builder = reqwest::Client::builder().timeout(Duration::from_secs(8));
    let builder = if s.accept_invalid_certs {
        let tls = pin::tls_config(pin_path.to_path_buf(), record).map_err(|e| e.to_string())?;
        builder.use_preconfigured_tls(tls)
    } else {
        builder
    };
    builder.build().map_err(|e| e.to_string())
}

/// Client for public hosts such as the IP lookup. Must not use the gateway pin.
fn plain_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap_or_default()
}

async fn get_text(c: &reqwest::Client, url: &str, key: Option<&str>) -> Result<String, Failure> {
    let mut req = c.get(url);
    if let Some(k) = key {
        req = req.header("X-API-KEY", k);
    }
    // Only gateway requests carry the key.
    let peer = if key.is_some() {
        Peer::Gateway
    } else {
        Peer::Lookup
    };
    let resp = req
        .send()
        .await
        .map_err(|e| errors::send_failure(&e, peer))?;
    let code = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| errors::send_failure(&e, peer))?;
    if !code.is_success() {
        return Err(Failure::other(errors::status(code, peer, &body)));
    }
    Ok(body)
}

/// The gateway's IANA timezone from stat/sysinfo, or None if it does not report one.
async fn fetch_gateway_tz(
    c: &reqwest::Client,
    s: &Settings,
    key: &str,
) -> Result<Option<String>, String> {
    let body = get_text(c, &unifi::sysinfo_url(&s.gateway_url, &s.site), Some(key)).await?;
    unifi::parse_sysinfo_timezone(&body).map_err(|e| errors::parse(&e))
}

/// The gateway's WAN link and boot uptimes, as far as it reports them.
async fn fetch_uptime(
    c: &reqwest::Client,
    s: &Settings,
    key: &str,
) -> Result<unifi::GatewayUptime, String> {
    let body = get_text(c, &unifi::device_url(&s.gateway_url, &s.site), Some(key)).await?;
    unifi::parse_gateway_uptime(&body).map_err(|e| errors::parse(&e))
}

/// POSTs a `stat/report/*.site` request for WAN bytes between two times and returns
/// the buckets. Daily, hourly and 5-minute reports share the request and reply.
async fn fetch_report(
    c: &reqwest::Client,
    url: String,
    key: &str,
    start_ms: i64,
    end_ms: i64,
) -> Result<Vec<unifi::DayBucket>, String> {
    let body = unifi::daily_report_request(start_ms, end_ms);
    let resp = c
        .post(url)
        .header("X-API-KEY", key)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| errors::send(&e, Peer::Gateway))?;
    let code = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| errors::send(&e, Peer::Gateway))?;
    if !code.is_success() {
        return Err(errors::status(code, Peer::Gateway, &text));
    }
    unifi::parse_daily_report(&text).map_err(|e| errors::parse(&e))
}

async fn fetch_monthly(
    c: &reqwest::Client,
    s: &Settings,
    key: &str,
    tz: widget_core::Tz,
) -> Result<Monthly, String> {
    let now = Utc::now();
    let start = usage::cycle_start(now, s.billing_day, tz);
    let url = unifi::daily_report_url(&s.gateway_url, &s.site);
    let buckets = fetch_report(
        c,
        url,
        key,
        start.timestamp_millis(),
        now.timestamp_millis(),
    )
    .await?;
    let t = usage::monthly_totals(&buckets, start);
    Ok(Monthly {
        rx: t.rx_bytes,
        tx: t.tx_bytes,
    })
}

/// One point of the graph's history: average bytes per second over a gateway bucket.
#[derive(Serialize)]
pub struct HistoryPoint {
    t: i64,
    rx: u64,
    tx: u64,
}

/// Throughput history for the graph's "24h" (5-minute averages) or "7d" (hourly
/// averages) window, from the gateway's own reports, with the saved settings and key.
pub async fn history(
    window: &str,
    latest: &Latest,
    pin_path: &Path,
    app: &AppHandle,
) -> Result<Vec<HistoryPoint>, String> {
    let scale = widget_core::history::Scale::for_window(window)
        .ok_or_else(|| format!("unknown history window '{window}'"))?;
    // Away means the gateway is out of reach and polling is paused; do not try.
    if latest.away.load(Ordering::Relaxed) {
        return Err("Away from your home network".into());
    }
    let s = config::load_settings(app);
    let key = config::load_api_key().ok_or("No API key set")?;
    let c = client(&s, pin_path, false)?;
    let now = Utc::now().timestamp_millis();
    let url = unifi::history_report_url(&s.gateway_url, &s.site, scale);
    let buckets = fetch_report(&c, url, &key, now - scale.span_s() * 1000, now).await?;
    Ok(widget_core::history::bucket_rates(&buckets, scale, now)
        .into_iter()
        .map(|p| HistoryPoint {
            t: p.t_ms,
            rx: p.rx,
            tx: p.tx,
        })
        .collect())
}

#[derive(Serialize)]
pub struct SiteOut {
    name: String,
    desc: String,
}

/// Sites the key can see, for the site picker. Uses unsaved settings like Test.
pub async fn list_sites(s: &Settings, key: &str, pin_path: &Path) -> Result<Vec<SiteOut>, String> {
    let c = client(s, pin_path, false)?;
    let body = get_text(&c, &unifi::sites_url(&s.gateway_url), Some(key)).await?;
    let sites = unifi::parse_sites(&body).map_err(|e| errors::parse(&e))?;
    Ok(sites
        .into_iter()
        .map(|x| SiteOut {
            name: x.name,
            desc: x.desc,
        })
        .collect())
}

/// When a connection fails, checks whether the address is a legacy self-hosted Network
/// Server, which has no API keys, so the user is told why instead of "rejected the key".
async fn legacy_server_note(c: &reqwest::Client, s: &Settings) -> Option<String> {
    let body = get_text(c, &unifi::legacy_status_url(&s.gateway_url), None)
        .await
        .ok()?;
    unifi::parse_legacy_status(&body).map(|v| {
        format!(
            "This is a legacy self-hosted UniFi Network Server ({v}). It does not support \
             API keys, so the widget cannot use it. The widget works with UniFi OS consoles \
             and UniFi OS Server"
        )
    })
}

/// One-off health request with the given (possibly unsaved) settings and key. It checks
/// an existing pin but never records one, so testing a mistyped address pins nothing.
pub async fn test_connection(s: &Settings, key: &str, pin_path: &Path) -> Result<String, String> {
    s.validate().map_err(|errs| errs.join("; "))?;
    let c = client(s, pin_path, false)?;
    let body = match get_text(&c, &unifi::health_url(&s.gateway_url, &s.site), Some(key)).await {
        Ok(b) => b,
        Err(e) => return Err(legacy_server_note(&c, s).await.unwrap_or(e.into())),
    };
    let h = unifi::parse_health(&body).map_err(|e| errors::parse(&e))?;
    // Daily buckets follow the gateway's zone, so say which one it is and flag an
    // override that does not match it.
    let tz_note = match (fetch_gateway_tz(&c, s, key).await.ok().flatten(), s.tz()) {
        (Some(g), Some(set)) if set.name() != g => format!(
            ". The timezone setting ({}) differs from the gateway's ({g}), so the month may start at the wrong hour. Leave it blank to use the gateway's",
            set.name()
        ),
        (Some(g), _) => format!(". Gateway timezone: {g}"),
        (None, _) => String::new(),
    };
    Ok(format!(
        "Connected{}: {} down, {} up{}{tz_note}",
        h.isp_name.map(|n| format!(" to {n}")).unwrap_or_default(),
        errors::rate(h.rx_bytes_per_s),
        errors::rate(h.tx_bytes_per_s),
        h.latency_ms
            .map(|l| format!(", {l} ms latency"))
            .unwrap_or_default()
    ))
}

/// Away from the gateway's network: stop all polling (nothing is sent to the gateway or
/// the IP lookup) until the PC's address changes, the user presses Check now, or settings
/// are saved.
async fn wait_until_back(app: &AppHandle, latest: &Latest) {
    let from = local_ip();
    log::info!("gateway unreachable from a new network ({from:?}): paused until it changes");
    latest.away.store(true, Ordering::Relaxed);
    let _ = app.emit("poll-status", "");
    let _ = app.emit("away", true);
    take_resume();
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        crate::tray::sync_icon(app);
        if take_resume() || config::peek_changed() || pin::take_reset() {
            log::info!("resuming: asked to check again");
            break;
        }
        if local_ip() != from {
            log::info!("resuming: network address changed");
            break;
        }
    }
    latest.away.store(false, Ordering::Relaxed);
    let _ = app.emit("away", false);
}

pub async fn run(app: AppHandle) {
    let latest: Arc<Latest> = (*app.state::<Arc<Latest>>()).clone();
    let pin_path: PathBuf = pin::path(&app).unwrap_or_default();
    let mut last_theme: Option<String> = None;
    let mut failures: u32 = 0;
    // Consecutive polls where the gateway did not answer at all, and this PC's address when
    // it last did. Together they tell "gateway down" from "PC left the network".
    let mut unreachable_streak: u32 = 0;
    let mut home_ip: Option<IpAddr> = None;
    let mut uptime_due = std::time::Instant::now();
    let mut monthly_due = std::time::Instant::now();
    let mut last_ip: Option<std::time::Instant> = None;
    let mut wan_ip: Option<std::net::IpAddr> = None;
    let lookup = plain_client();
    let mut s = Settings::default();
    let mut api_key: Option<String> = None;
    // Kept between polls so connections to the gateway are reused, not re-handshaken.
    let mut gateway: Option<reqwest::Client> = None;
    // The gateway's own timezone, used when the timezone setting is blank.
    let mut gateway_tz: Option<String> = None;

    loop {
        // Settings saves set the flag, so changes apply without a restart.
        if config::take_changed() {
            s = config::load_settings(&app);
            api_key = config::load_api_key();
            gateway = None;
            gateway_tz = None;
            // Billing day or timezone may have changed, so recompute the month now.
            monthly_due = std::time::Instant::now();
            uptime_due = std::time::Instant::now();
            if last_theme.as_deref() != Some(s.theme.as_str()) {
                let _ = app.emit("theme", s.theme.clone());
                last_theme = Some(s.theme.clone());
            }
        }
        crate::tray::sync_icon(&app);
        let Some(key) = api_key.clone() else {
            let _ = app.emit("poll-status", "No API key set");
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        };
        let c = match &gateway {
            Some(c) => c.clone(),
            None => match client(&s, &pin_path, true) {
                Ok(c) => {
                    gateway = Some(c.clone());
                    c
                }
                Err(e) => {
                    log::warn!("gateway client: {e}");
                    let _ = app.emit("poll-status", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            },
        };

        let health_url = unifi::health_url(&s.gateway_url, &s.site);
        match get_text(&c, &health_url, Some(&key))
            .await
            .and_then(|b| unifi::parse_health(&b).map_err(|e| Failure::other(errors::parse(&e))))
        {
            Ok(h) => {
                if failures > 0 {
                    log::info!("gateway reachable again after {failures} failed polls");
                }
                failures = 0;
                unreachable_streak = 0;
                home_ip = local_ip();
                wan_ip = h.wan_ip;
                let _ = app.emit("poll-status", "");
                let _ = app.emit(
                    "wan-rates",
                    Rates {
                        rx: h.rx_bytes_per_s,
                        tx: h.tx_bytes_per_s,
                        latency: h.latency_ms,
                        monitors: h
                            .monitors
                            .iter()
                            .map(|m| MonitorOut {
                                target: m.target.clone(),
                                latency: m.latency_ms,
                            })
                            .collect(),
                        availability_pct: h.availability_pct,
                        availability_period_s: h.availability_period_s,
                        wan2: h.wan2.as_ref().map(|w| SecondWanOut {
                            monitors: w
                                .monitors
                                .iter()
                                .map(|m| MonitorOut {
                                    target: m.target.clone(),
                                    latency: m.latency_ms,
                                })
                                .collect(),
                            availability_pct: w.availability_pct,
                            availability_period_s: w.availability_period_s,
                        }),
                    },
                );
                if std::time::Instant::now() >= uptime_due {
                    uptime_due = std::time::Instant::now() + UPTIME_EVERY;
                    match fetch_uptime(&c, &s, &key).await {
                        Ok(g) => {
                            let u = Uptime {
                                seconds: g.wan_s,
                                device_seconds: g.device_s,
                                isp: h.isp_name.clone(),
                            };
                            if let Ok(mut g) = latest.uptime.lock() {
                                *g = Some(u.clone());
                            }
                            let _ = app.emit("wan-uptime", u);
                        }
                        Err(e) => log::warn!("wan uptime: {e}"),
                    }
                }
            }
            Err(e) => {
                failures = failures.saturating_add(1);
                // Log the start of an outage, not every retry.
                if failures == 1 {
                    log::warn!("live rates: {e}");
                }
                unreachable_streak = if e.unreachable {
                    unreachable_streak.saturating_add(1)
                } else {
                    0
                };
                if should_go_away(unreachable_streak, home_ip, local_ip()) {
                    wait_until_back(&app, &latest).await;
                    // Connections and cached values are from the other network.
                    failures = 0;
                    unreachable_streak = 0;
                    gateway = None;
                    monthly_due = std::time::Instant::now();
                    uptime_due = std::time::Instant::now();
                    last_ip = None;
                    continue;
                }
                let _ = app.emit("poll-status", e.message);
            }
        }

        let now = std::time::Instant::now();
        if now >= monthly_due {
            if s.tz().is_none() && gateway_tz.is_none() {
                match fetch_gateway_tz(&c, &s, &key).await {
                    Ok(z) => gateway_tz = z,
                    Err(e) => log::warn!("gateway timezone: {e}"),
                }
            }
            let system = iana_time_zone::get_timezone().ok();
            let result = match s.resolve_tz(gateway_tz.as_deref(), system.as_deref()) {
                Some(tz) => fetch_monthly(&c, &s, &key, tz).await,
                None => Err("no timezone from the setting, the gateway or Windows".into()),
            };
            match result {
                Ok(m) => {
                    monthly_due = std::time::Instant::now() + MONTHLY_EVERY;
                    if let Ok(mut g) = latest.monthly.lock() {
                        *g = Some(m.clone());
                    }
                    let _ = app.emit("monthly-usage", m);
                }
                Err(e) => {
                    monthly_due = std::time::Instant::now() + MONTHLY_RETRY;
                    log::warn!("monthly usage: {e}");
                }
            }
        }

        if last_ip.is_none_or(|t| t.elapsed() >= IP_EVERY) {
            let external = if s.ip_lookup_url.is_empty() {
                None
            } else {
                match get_text(&lookup, &s.ip_lookup_url, None).await {
                    Ok(b) => ip::parse_lookup(&b)
                        .map_err(|e| log::warn!("ip lookup parse: {e}"))
                        .ok(),
                    Err(e) => {
                        log::warn!("ip lookup: {e}");
                        None
                    }
                }
            };
            last_ip = Some(std::time::Instant::now());
            let v = ip::ip_view(external, wan_ip);
            let info = IpInfo {
                external: v.external.map(|i| i.to_string()),
                wan: v.wan.map(|i| i.to_string()),
                cgnat: v.cgnat,
            };
            if let Ok(mut g) = latest.ip.lock() {
                *g = Some(info.clone());
            }
            let _ = app.emit("ip-info", info);
        }

        let wait_ms = if failures == 0 {
            s.poll_interval_ms
        } else {
            backoff_ms(s.poll_interval_ms, failures)
        };
        // Sleep in short steps so a pin reset (Forget in Settings) retries straight away.
        let until = std::time::Instant::now() + Duration::from_millis(wait_ms);
        while std::time::Instant::now() < until {
            if pin::take_reset() {
                failures = 0;
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{backoff_ms, should_go_away};
    use std::net::IpAddr;

    fn ip(s: &str) -> Option<IpAddr> {
        Some(s.parse().unwrap())
    }

    #[test]
    fn away_needs_a_run_of_failures_and_a_changed_address() {
        let home = ip("192.168.1.20");
        // Gateway down at home: same address, keep backing off.
        assert!(!should_go_away(10, home, home));
        // Left the network, but not yet sure it is not a blip.
        assert!(!should_go_away(2, home, ip("10.0.0.5")));
        assert!(should_go_away(3, home, ip("10.0.0.5")));
        // No network at all counts as a change.
        assert!(should_go_away(3, home, None));
    }

    #[test]
    fn away_when_the_gateway_never_answered_this_run() {
        assert!(should_go_away(3, None, ip("10.0.0.5")));
        assert!(!should_go_away(2, None, ip("10.0.0.5")));
    }

    #[test]
    fn backoff_doubles_then_caps_at_a_minute() {
        assert_eq!(backoff_ms(2000, 1), 4000);
        assert_eq!(backoff_ms(2000, 2), 8000);
        assert_eq!(backoff_ms(2000, 5), 60_000);
        assert_eq!(backoff_ms(2000, u32::MAX), 60_000);
    }
}
