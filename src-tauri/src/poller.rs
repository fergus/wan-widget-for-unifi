//! Background polling. Emits events to the UI; the API key never leaves this process.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use widget_core::{ip, settings::Settings, unifi, usage};

use crate::config;

const MONTHLY_EVERY: Duration = Duration::from_secs(300);
const IP_EVERY: Duration = Duration::from_secs(300);

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

fn client(s: &Settings) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(s.accept_invalid_certs)
        .timeout(Duration::from_secs(8))
        .build()
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
pub async fn test_connection(s: &Settings, key: &str) -> Result<String, String> {
    s.validate().map_err(|errs| errs.join("; "))?;
    let c = client(s).map_err(|e| e.to_string())?;
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
    let mut last_theme: Option<String> = None;
    let mut last_monthly: Option<std::time::Instant> = None;
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
        let Ok(c) = client(&s) else {
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        };

        let health_url = unifi::health_url(&s.gateway_url, &s.site);
        match get_text(&c, &health_url, Some(&key)).await.and_then(|b| {
            unifi::parse_health(&b).map_err(|e| e.to_string())
        }) {
            Ok(h) => {
                wan_ip = h.wan_ip;
                let _ = app.emit("poll-status", "");
                let _ = app.emit("wan-rates", Rates { rx: h.rx_bytes_per_s, tx: h.tx_bytes_per_s });
            }
            Err(e) => {
                let _ = app.emit("poll-status", format!("Gateway: {e}"));
            }
        }

        if last_monthly.map_or(true, |t| t.elapsed() >= MONTHLY_EVERY) {
            match fetch_monthly(&c, &s, &key).await {
                Ok(m) => {
                    last_monthly = Some(std::time::Instant::now());
                    if let Ok(mut g) = latest.monthly.lock() {
                        *g = Some(m.clone());
                    }
                    let _ = app.emit("monthly-usage", m);
                }
                Err(e) => eprintln!("monthly usage: {e}"),
            }
        }

        if last_ip.map_or(true, |t| t.elapsed() >= IP_EVERY) {
            let external = if s.ip_lookup_url.is_empty() {
                None
            } else {
                match get_text(&c, &s.ip_lookup_url, None).await {
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

        tokio::time::sleep(Duration::from_millis(s.poll_interval_ms)).await;
    }
}
