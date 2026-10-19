//! Background polling. Emits events to the UI; the API key never leaves this process.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use widget_core::{ip, settings::Settings, unifi, usage};

use crate::{config, pin};

const MONTHLY_EVERY: Duration = Duration::from_secs(300);
const MONTHLY_RETRY: Duration = Duration::from_secs(60);
const IP_EVERY: Duration = Duration::from_secs(300);
/// Longest wait between health polls while the gateway keeps failing.
const MAX_BACKOFF_MS: u64 = 60_000;

/// Poll interval after `failures` consecutive gateway errors: doubles each time, capped.
fn backoff_ms(base_ms: u64, failures: u32) -> u64 {
    base_ms.saturating_mul(1u64 << failures.min(6)).min(MAX_BACKOFF_MS)
}

#[derive(Serialize, Clone)]
struct Rates {
    rx: u64,
    tx: u64,
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
}

#[derive(Serialize)]
pub struct Snapshot {
    monthly: Option<Monthly>,
    ip: Option<IpInfo>,
}

impl Latest {
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            monthly: self.monthly.lock().ok().and_then(|g| g.clone()),
            ip: self.ip.lock().ok().and_then(|g| g.clone()),
        }
    }
}

/// Gateway client. With accept_invalid_certs the gateway's self-signed certificate is
/// checked against the pin in pin_path instead of the public CA roots.
fn client(s: &Settings, pin_path: &Path) -> Result<reqwest::Client, String> {
    let builder = reqwest::Client::builder().timeout(Duration::from_secs(8));
    let builder = if s.accept_invalid_certs {
        builder.use_preconfigured_tls(pin::tls_config(pin_path.to_path_buf()).map_err(|e| e.to_string())?)
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

async fn get_text(c: &reqwest::Client, url: &str, key: Option<&str>) -> Result<String, String> {
    let mut req = c.get(url);
    if let Some(k) = key {
        req = req.header("X-API-KEY", k);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.text().await.map_err(|e| e.to_string())
}

async fn fetch_monthly(c: &reqwest::Client, s: &Settings, key: &str) -> Result<Monthly, String> {
    let tz = s.tz().ok_or("bad timezone")?;
    let now = Utc::now();
    let start = usage::cycle_start(now, s.billing_day, tz);
    let url = unifi::daily_report_url(&s.gateway_url, &s.site);
    let body = unifi::daily_report_request(start.timestamp_millis(), now.timestamp_millis());
    let resp = c
        .post(url)
        .header("X-API-KEY", key)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let text = resp.text().await.map_err(|e| e.to_string())?;
    let buckets = unifi::parse_daily_report(&text).map_err(|e| e.to_string())?;
    let t = usage::monthly_totals(&buckets, start);
    Ok(Monthly { rx: t.rx_bytes, tx: t.tx_bytes })
}

/// One-off health request with the given (possibly unsaved) settings and key.
pub async fn test_connection(s: &Settings, key: &str, pin_path: &Path) -> Result<String, String> {
    s.validate().map_err(|errs| errs.join("; "))?;
    let c = client(s, pin_path)?;
    let body = get_text(&c, &unifi::health_url(&s.gateway_url, &s.site), Some(key)).await?;
    let h = unifi::parse_health(&body).map_err(|e| e.to_string())?;
    Ok(format!(
        "Connected{}: {} B/s down, {} B/s up",
        h.isp_name.map(|n| format!(" ({n})")).unwrap_or_default(),
        h.rx_bytes_per_s,
        h.tx_bytes_per_s
    ))
}

pub async fn run(app: AppHandle) {
    let latest: Arc<Latest> = (*app.state::<Arc<Latest>>()).clone();
    let pin_path: PathBuf = pin::path(&app).unwrap_or_default();
    let mut last_theme: Option<String> = None;
    let mut failures: u32 = 0;
    let mut monthly_due = std::time::Instant::now();
    let mut last_ip: Option<std::time::Instant> = None;
    let mut wan_ip: Option<std::net::IpAddr> = None;

    loop {
        // Re-read each cycle so settings and key changes apply without a restart.
        let s = config::load_settings(&app);
        if last_theme.as_deref() != Some(s.theme.as_str()) {
            let _ = app.emit("theme", s.theme.clone());
            last_theme = Some(s.theme.clone());
        }
        crate::tray::sync_icon(&app);
        let Some(key) = config::load_api_key() else {
            let _ = app.emit("poll-status", "No API key set");
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        };
        let c = match client(&s, &pin_path) {
            Ok(c) => c,
            Err(e) => {
                let _ = app.emit("poll-status", format!("Gateway: {e}"));
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let health_url = unifi::health_url(&s.gateway_url, &s.site);
        match get_text(&c, &health_url, Some(&key)).await.and_then(|b| {
            unifi::parse_health(&b).map_err(|e| e.to_string())
        }) {
            Ok(h) => {
                failures = 0;
                wan_ip = h.wan_ip;
                let _ = app.emit("poll-status", "");
                let _ = app.emit("wan-rates", Rates { rx: h.rx_bytes_per_s, tx: h.tx_bytes_per_s });
            }
            Err(e) => {
                failures = failures.saturating_add(1);
                let msg = if pin::mismatch() {
                    "Gateway certificate changed. Forget the pin in Settings if that is expected".to_string()
                } else {
                    format!("Gateway: {e}")
                };
                let _ = app.emit("poll-status", msg);
            }
        }

        let now = std::time::Instant::now();
        if now >= monthly_due {
            match fetch_monthly(&c, &s, &key).await {
                Ok(m) => {
                    monthly_due = std::time::Instant::now() + MONTHLY_EVERY;
                    if let Ok(mut g) = latest.monthly.lock() {
                        *g = Some(m.clone());
                    }
                    let _ = app.emit("monthly-usage", m);
                }
                Err(e) => {
                    monthly_due = std::time::Instant::now() + MONTHLY_RETRY;
                    eprintln!("monthly usage: {e}");
                }
            }
        }

        if last_ip.map_or(true, |t| t.elapsed() >= IP_EVERY) {
            let external = if s.ip_lookup_url.is_empty() {
                None
            } else {
                match get_text(&plain_client(), &s.ip_lookup_url, None).await {
                    Ok(b) => ip::parse_lookup(&b)
                        .map_err(|e| eprintln!("ip lookup parse: {e}"))
                        .ok(),
                    Err(e) => {
                        eprintln!("ip lookup: {e}");
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

        let wait_ms = if failures == 0 { s.poll_interval_ms } else { backoff_ms(s.poll_interval_ms, failures) };
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
    use super::backoff_ms;

    #[test]
    fn backoff_doubles_then_caps_at_a_minute() {
        assert_eq!(backoff_ms(2000, 1), 4000);
        assert_eq!(backoff_ms(2000, 2), 8000);
        assert_eq!(backoff_ms(2000, 5), 60_000);
        assert_eq!(backoff_ms(2000, u32::MAX), 60_000);
    }
}
