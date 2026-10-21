//! Parsing for the classic UniFi Network local API (`/proxy/network/api/s/<site>/...`).

use std::net::IpAddr;

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum ParseError {
    #[error("invalid JSON: {0}")]
    Json(String),
    #[error("controller returned rc={0}")]
    Controller(String),
    #[error("no wan subsystem in health response")]
    NoWan,
}

#[derive(Deserialize)]
struct Envelope<T> {
    meta: Meta,
    data: Vec<T>,
}

#[derive(Deserialize)]
struct Meta {
    rc: String,
}

fn parse_envelope<T: for<'de> Deserialize<'de>>(body: &str) -> Result<Vec<T>, ParseError> {
    let env: Envelope<T> =
        serde_json::from_str(body).map_err(|e| ParseError::Json(e.to_string()))?;
    if env.meta.rc != "ok" {
        return Err(ParseError::Controller(env.meta.rc));
    }
    Ok(env.data)
}

fn site_base(gateway_url: &str, site: &str) -> String {
    format!(
        "{}/proxy/network/api/s/{}",
        gateway_url.trim_end_matches('/'),
        site
    )
}

pub fn health_url(gateway_url: &str, site: &str) -> String {
    format!("{}/stat/health", site_base(gateway_url, site))
}

/// `stat/report/5minutes.site` or `stat/report/hourly.site`, for the graph's history.
/// Same request body and reply shape as the daily report.
pub fn history_report_url(gateway_url: &str, site: &str, scale: crate::history::Scale) -> String {
    format!(
        "{}/stat/report/{}.site",
        site_base(gateway_url, site),
        scale.report()
    )
}

pub fn daily_report_url(gateway_url: &str, site: &str) -> String {
    format!("{}/stat/report/daily.site", site_base(gateway_url, site))
}

pub fn sysinfo_url(gateway_url: &str, site: &str) -> String {
    format!("{}/stat/sysinfo", site_base(gateway_url, site))
}

/// Sites the API key can see. Not site-scoped, so no `site` argument.
pub fn sites_url(gateway_url: &str) -> String {
    format!(
        "{}/proxy/network/api/self/sites",
        gateway_url.trim_end_matches('/')
    )
}

/// UniFi OS console information. Answers without an API key, so discovery can use it.
pub fn console_url(gateway_url: &str) -> String {
    format!("{}/api/system", gateway_url.trim_end_matches('/'))
}

/// The legacy self-hosted Network Server's status page (no API key needed).
pub fn legacy_status_url(gateway_url: &str) -> String {
    format!("{}/status", gateway_url.trim_end_matches('/'))
}

#[derive(Deserialize)]
struct ConsoleInfo {
    hardware: ConsoleHardware,
}

#[derive(Deserialize)]
struct ConsoleHardware {
    shortname: Option<String>,
}

/// Some(model code, e.g. "UDRULT") when the reply is a UniFi OS console's `/api/system`.
/// Anything else, including other web servers, is None. The model may be empty.
pub fn parse_console(body: &str) -> Option<String> {
    serde_json::from_str::<ConsoleInfo>(body)
        .ok()
        .map(|c| c.hardware.shortname.unwrap_or_default())
}

#[derive(Deserialize)]
struct LegacyStatus {
    meta: LegacyMeta,
}

#[derive(Deserialize)]
struct LegacyMeta {
    server_version: Option<String>,
}

/// The server version when the reply is a legacy Network Server's `/status`. That
/// server has no API keys, so the widget cannot use it.
pub fn parse_legacy_status(body: &str) -> Option<String> {
    serde_json::from_str::<LegacyStatus>(body)
        .ok()
        .and_then(|s| s.meta.server_version)
}

/// A site the API key can see. `name` is the id used in URLs ("default"); `desc` is
/// the name shown in the UniFi app.
#[derive(Debug, Clone, PartialEq)]
pub struct Site {
    pub name: String,
    pub desc: String,
}

#[derive(Deserialize)]
struct SiteEntry {
    name: String,
    desc: Option<String>,
}

pub fn parse_sites(body: &str) -> Result<Vec<Site>, ParseError> {
    Ok(parse_envelope::<SiteEntry>(body)?
        .into_iter()
        .map(|e| Site {
            desc: e.desc.unwrap_or_else(|| e.name.clone()),
            name: e.name,
        })
        .collect())
}

pub fn device_url(gateway_url: &str, site: &str) -> String {
    format!("{}/stat/device", site_base(gateway_url, site))
}

#[derive(Deserialize)]
struct DeviceEntry {
    /// Only the gateway has a `wan1` block (it is null on switches and access points).
    wan1: Option<Wan1>,
    uplink: Option<Uplink>,
    uptime: Option<f64>,
}

#[derive(Deserialize)]
struct Wan1 {}

#[derive(Deserialize)]
struct Uplink {
    uptime: Option<f64>,
}

/// Uptimes of the gateway, in seconds, from `stat/device`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GatewayUptime {
    /// How long the WAN link has been up: the time since its last disconnect.
    pub wan_s: Option<u64>,
    /// How long the gateway itself has run since it last booted.
    pub device_s: Option<u64>,
}

/// Finds the gateway (the device with a `wan1` block) and reads its link uptime
/// (`uplink.uptime`) and its own boot uptime (`uptime`). Both are None if no device is
/// a gateway or it does not report them.
pub fn parse_gateway_uptime(body: &str) -> Result<GatewayUptime, ParseError> {
    let secs = |v: f64| v.max(0.0) as u64;
    Ok(parse_envelope::<DeviceEntry>(body)?
        .into_iter()
        .find(|d| d.wan1.is_some())
        .map(|d| GatewayUptime {
            wan_s: d.uplink.and_then(|u| u.uptime).map(secs),
            device_s: d.uptime.map(secs),
        })
        .unwrap_or_default())
}

#[derive(Deserialize)]
struct SysinfoEntry {
    timezone: Option<String>,
}

/// The controller's IANA timezone from `stat/sysinfo`, if it reports one. Daily report
/// buckets start at local midnight in this zone.
pub fn parse_sysinfo_timezone(body: &str) -> Result<Option<String>, ParseError> {
    Ok(parse_envelope::<SysinfoEntry>(body)?
        .into_iter()
        .find_map(|e| e.timezone)
        .filter(|tz| !tz.is_empty()))
}

/// Live WAN throughput and address from `stat/health`.
#[derive(Debug, Clone, PartialEq)]
pub struct WanHealth {
    pub rx_bytes_per_s: u64,
    pub tx_bytes_per_s: u64,
    pub wan_ip: Option<IpAddr>,
    pub isp_name: Option<String>,
    /// Gateway-to-internet latency in ms, measured by the gateway (the `www` subsystem).
    pub latency_ms: Option<u32>,
    /// The gateway's WAN monitors (the targets the UniFi app lists), with their latency.
    pub monitors: Vec<Monitor>,
    /// WAN availability over `availability_period_s`, as a percentage.
    pub availability_pct: Option<f64>,
    pub availability_period_s: Option<u64>,
    /// The second WAN on a dual-WAN gateway, when the gateway reports one.
    pub wan2: Option<SecondWan>,
}

/// Monitors and availability of a dual-WAN gateway's second WAN (`uptime_stats.WAN2`).
/// Read defensively: the shape is assumed to match `WAN` and is not yet confirmed on a
/// real dual-WAN gateway.
#[derive(Debug, Clone, PartialEq)]
pub struct SecondWan {
    pub monitors: Vec<Monitor>,
    pub availability_pct: Option<f64>,
    pub availability_period_s: Option<u64>,
}

/// One WAN monitor target and its average latency.
#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    /// What the gateway pings or queries, e.g. "google.com" or "1.1.1.1".
    pub target: String,
    pub latency_ms: Option<u32>,
}

#[derive(Deserialize, Default)]
struct UptimeStats {
    #[serde(rename = "WAN")]
    wan: Option<WanStats>,
    #[serde(rename = "WAN2")]
    wan2: Option<WanStats>,
}

#[derive(Deserialize, Default)]
struct WanStats {
    #[serde(default)]
    monitors: Vec<MonitorEntry>,
    availability: Option<f64>,
    time_period: Option<u64>,
}

#[derive(Deserialize)]
struct MonitorEntry {
    target: String,
    latency_average: Option<f64>,
}

#[derive(Deserialize)]
struct HealthEntry {
    subsystem: String,
    latency: Option<f64>,
    uptime_stats: Option<UptimeStats>,
    #[serde(rename = "rx_bytes-r")]
    rx: Option<f64>,
    #[serde(rename = "tx_bytes-r")]
    tx: Option<f64>,
    wan_ip: Option<String>,
    isp_name: Option<String>,
}

pub fn parse_health(body: &str) -> Result<WanHealth, ParseError> {
    let entries = parse_envelope::<HealthEntry>(body)?;
    let latency_ms = entries
        .iter()
        .find(|e| e.subsystem == "www")
        .and_then(|e| e.latency)
        .map(|l| l.max(0.0).round() as u32);
    let wan = entries
        .into_iter()
        .find(|e| e.subsystem == "wan")
        .ok_or(ParseError::NoWan)?;
    let uptime_stats = wan.uptime_stats.unwrap_or_default();
    let stats = uptime_stats.wan.unwrap_or_default();
    let monitors = |list: Vec<MonitorEntry>| -> Vec<Monitor> {
        list.into_iter()
            .map(|m| Monitor {
                target: m.target,
                latency_ms: m.latency_average.map(|l| l.max(0.0).round() as u32),
            })
            .collect()
    };
    let wan2 = uptime_stats.wan2.map(|w| SecondWan {
        monitors: monitors(w.monitors),
        availability_pct: w.availability,
        availability_period_s: w.time_period,
    });
    Ok(WanHealth {
        rx_bytes_per_s: wan.rx.unwrap_or(0.0).max(0.0) as u64,
        tx_bytes_per_s: wan.tx.unwrap_or(0.0).max(0.0) as u64,
        wan_ip: wan.wan_ip.and_then(|s| s.parse().ok()),
        isp_name: wan.isp_name,
        latency_ms,
        monitors: monitors(stats.monitors),
        availability_pct: stats.availability,
        availability_period_s: stats.time_period,
        wan2,
    })
}

/// One bucket of WAN traffic from a `stat/report/*.site` report (daily, hourly or
/// 5-minute; they share a shape).
/// `start_ms` is the bucket start (local midnight in the controller's timezone).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DayBucket {
    pub start_ms: i64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Deserialize)]
struct DayEntry {
    time: i64,
    #[serde(rename = "wan-rx_bytes", default)]
    rx: f64,
    #[serde(rename = "wan-tx_bytes", default)]
    tx: f64,
}

/// Returns buckets sorted by start time.
pub fn parse_daily_report(body: &str) -> Result<Vec<DayBucket>, ParseError> {
    let mut out: Vec<DayBucket> = parse_envelope::<DayEntry>(body)?
        .into_iter()
        .map(|e| DayBucket {
            start_ms: e.time,
            rx_bytes: e.rx.max(0.0) as u64,
            tx_bytes: e.tx.max(0.0) as u64,
        })
        .collect();
    out.sort_by_key(|b| b.start_ms);
    Ok(out)
}

/// JSON body for the POST to `stat/report/daily.site`.
pub fn daily_report_request(start_ms: i64, end_ms: i64) -> String {
    format!(
        r#"{{"attrs":["wan-rx_bytes","wan-tx_bytes","time"],"start":{start_ms},"end":{end_ms}}}"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEALTH: &str = include_str!("../tests/fixtures/health.json");
    const DAILY: &str = include_str!("../tests/fixtures/daily_report.json");
    const ERROR: &str = include_str!("../tests/fixtures/error.json");
    const SYSINFO: &str = include_str!("../tests/fixtures/sysinfo.json");

    #[test]
    fn sysinfo_reports_the_controller_timezone() {
        assert_eq!(
            parse_sysinfo_timezone(SYSINFO),
            Ok(Some("Australia/Sydney".into()))
        );
        assert_eq!(
            sysinfo_url("https://192.168.1.1/", "default"),
            "https://192.168.1.1/proxy/network/api/s/default/stat/sysinfo"
        );
    }

    #[test]
    fn console_info_identifies_a_unifi_os_console() {
        // Shape of /api/system on a UCG Ultra (other fields trimmed).
        let body =
            r#"{"hardware":{"shortname":"UDRULT","name":""},"name":"Gateway","hasInternet":true}"#;
        assert_eq!(parse_console(body), Some("UDRULT".into()));
        assert_eq!(parse_console(r#"{"hardware":{}}"#), Some(String::new()));
        assert_eq!(parse_console("<html>router login</html>"), None);
        assert_eq!(parse_console(r#"{"error":{"code":401}}"#), None);
        assert_eq!(
            console_url("https://192.168.1.1/"),
            "https://192.168.1.1/api/system"
        );
    }

    #[test]
    fn legacy_server_status_is_recognised() {
        let body = r#"{"meta":{"rc":"ok","up":true,"server_version":"8.6.9"},"data":[]}"#;
        assert_eq!(parse_legacy_status(body), Some("8.6.9".into()));
        assert_eq!(parse_legacy_status("<html></html>"), None);
        assert_eq!(parse_legacy_status(r#"{"meta":{"rc":"ok"}}"#), None);
    }

    #[test]
    fn sites_list_names_and_descriptions() {
        let body = r#"{"meta":{"rc":"ok"},"data":[{"name":"default","desc":"Default","role":"admin"},{"name":"x7k2p9q1"}]}"#;
        assert_eq!(
            parse_sites(body),
            Ok(vec![
                Site {
                    name: "default".into(),
                    desc: "Default".into()
                },
                Site {
                    name: "x7k2p9q1".into(),
                    desc: "x7k2p9q1".into()
                },
            ])
        );
        assert_eq!(
            sites_url("https://192.168.1.1:11443/"),
            "https://192.168.1.1:11443/proxy/network/api/self/sites"
        );
    }

    #[test]
    fn single_wan_has_no_second_wan() {
        assert_eq!(parse_health(HEALTH).unwrap().wan2, None);
    }

    #[test]
    fn second_wan_is_read_when_reported() {
        // Constructed: the WAN2 shape is assumed to match WAN (see SecondWan).
        let body = r#"{"meta":{"rc":"ok"},"data":[{"subsystem":"wan","rx_bytes-r":1,"tx_bytes-r":2,
            "uptime_stats":{"WAN":{"availability":100.0,"time_period":86400,"monitors":[]},
            "WAN2":{"availability":97.5,"time_period":86400,
              "monitors":[{"target":"1.1.1.1","latency_average":21.6},{"target":"ping.ui.com"}]}}}]}"#;
        let w2 = parse_health(body).unwrap().wan2.unwrap();
        assert_eq!(w2.availability_pct, Some(97.5));
        assert_eq!(w2.availability_period_s, Some(86400));
        assert_eq!(
            w2.monitors,
            vec![
                Monitor {
                    target: "1.1.1.1".into(),
                    latency_ms: Some(22)
                },
                Monitor {
                    target: "ping.ui.com".into(),
                    latency_ms: None
                },
            ]
        );
    }

    #[test]
    fn sysinfo_without_timezone_is_none() {
        let body = r#"{"meta":{"rc":"ok"},"data":[{"uptime":1},{"timezone":""}]}"#;
        assert_eq!(parse_sysinfo_timezone(body), Ok(None));
        assert!(matches!(
            parse_sysinfo_timezone(ERROR),
            Err(ParseError::Controller(_))
        ));
    }

    #[test]
    fn health_picks_wan_subsystem_not_wlan() {
        let h = parse_health(HEALTH).unwrap();
        assert_eq!(h.rx_bytes_per_s, 513_102);
        assert_eq!(h.tx_bytes_per_s, 356_527);
        assert_eq!(h.wan_ip, Some("100.64.10.20".parse().unwrap()));
        assert_eq!(h.isp_name.as_deref(), Some("ExampleISP"));
        assert_eq!(h.latency_ms, Some(9));
    }

    #[test]
    fn health_lists_the_wan_monitors_and_availability() {
        let h = parse_health(HEALTH).unwrap();
        let seen: Vec<_> = h
            .monitors
            .iter()
            .map(|m| (m.target.as_str(), m.latency_ms))
            .collect();
        // The three the UniFi app shows, not the separate alerting monitors.
        assert_eq!(
            seen,
            [
                ("www.microsoft.com", Some(10)),
                ("google.com", Some(12)),
                ("1.1.1.1", Some(7))
            ]
        );
        assert!((h.availability_pct.unwrap() - 99.7222).abs() < 0.001);
        assert_eq!(h.availability_period_s, Some(86_400));
    }

    #[test]
    fn missing_uptime_stats_means_no_monitors() {
        let body = r#"{"meta":{"rc":"ok"},"data":[{"subsystem":"wan"}]}"#;
        let h = parse_health(body).unwrap();
        assert!(h.monitors.is_empty());
        assert_eq!((h.availability_pct, h.availability_period_s), (None, None));
    }

    #[test]
    fn latency_is_absent_without_a_www_subsystem() {
        let body = r#"{"meta":{"rc":"ok"},"data":[{"subsystem":"wan"}]}"#;
        assert_eq!(parse_health(body).unwrap().latency_ms, None);
    }

    #[test]
    fn wan_uptime_comes_from_the_gateway_not_other_devices() {
        let body = include_str!("../tests/fixtures/device.json");
        // Recorded from a UCG Ultra: link up 106380 s, device up 3235463 s.
        assert_eq!(
            parse_gateway_uptime(body),
            Ok(GatewayUptime {
                wan_s: Some(106_380),
                device_s: Some(3_235_463)
            })
        );
        assert_eq!(
            device_url("https://192.168.1.1/", "default"),
            "https://192.168.1.1/proxy/network/api/s/default/stat/device"
        );
    }

    #[test]
    fn wan_uptime_is_none_when_no_device_reports_it() {
        // A switch's uplink.uptime must not be mistaken for the WAN link's.
        let body = r#"{"meta":{"rc":"ok"},"data":[{"type":"usw","uptime":9,"wan1":null,"uplink":{"uptime":5}}]}"#;
        assert_eq!(parse_gateway_uptime(body), Ok(GatewayUptime::default()));
        let no_uptime = r#"{"meta":{"rc":"ok"},"data":[{"type":"udm","wan1":{},"uplink":{}}]}"#;
        assert_eq!(
            parse_gateway_uptime(no_uptime),
            Ok(GatewayUptime::default())
        );
        assert!(matches!(
            parse_gateway_uptime(ERROR),
            Err(ParseError::Controller(_))
        ));
    }

    #[test]
    fn health_without_wan_is_an_error() {
        let body = r#"{"meta":{"rc":"ok"},"data":[{"subsystem":"lan"}]}"#;
        assert_eq!(parse_health(body), Err(ParseError::NoWan));
    }

    #[test]
    fn wan_missing_rates_default_to_zero() {
        let body = r#"{"meta":{"rc":"ok"},"data":[{"subsystem":"wan"}]}"#;
        let h = parse_health(body).unwrap();
        assert_eq!((h.rx_bytes_per_s, h.tx_bytes_per_s, h.wan_ip), (0, 0, None));
    }

    #[test]
    fn controller_error_is_surfaced() {
        assert_eq!(
            parse_health(ERROR),
            Err(ParseError::Controller("error".into()))
        );
        assert_eq!(
            parse_daily_report(ERROR),
            Err(ParseError::Controller("error".into()))
        );
    }

    #[test]
    fn garbage_is_a_json_error() {
        assert!(matches!(parse_health("<html>"), Err(ParseError::Json(_))));
    }

    #[test]
    fn daily_report_parses_float_encoded_bytes_and_sorts() {
        let b = parse_daily_report(DAILY).unwrap();
        assert_eq!(b.len(), 18);
        assert!(b.windows(2).all(|w| w[0].start_ms < w[1].start_ms));
        assert!(b[0].rx_bytes > 1_000_000_000);
    }

    #[test]
    fn daily_report_sorts_unordered_input() {
        let body = r#"{"meta":{"rc":"ok"},"data":[
            {"time":2000,"wan-rx_bytes":2.0E3,"wan-tx_bytes":1.0},
            {"time":1000,"wan-rx_bytes":1.0E3}]}"#;
        let b = parse_daily_report(body).unwrap();
        assert_eq!(b[0].start_ms, 1000);
        assert_eq!(b[0].tx_bytes, 0);
        assert_eq!(b[1].rx_bytes, 2000);
    }

    #[test]
    fn urls_tolerate_trailing_slashes() {
        assert_eq!(
            health_url("https://192.168.1.1", "default"),
            "https://192.168.1.1/proxy/network/api/s/default/stat/health"
        );
        assert_eq!(
            health_url("https://192.168.1.1///", "default"),
            health_url("https://192.168.1.1", "default")
        );
        assert_eq!(
            daily_report_url("https://gw.lan:8443/", "abc"),
            "https://gw.lan:8443/proxy/network/api/s/abc/stat/report/daily.site"
        );
    }

    #[test]
    fn history_reports_use_the_5minute_and_hourly_names() {
        use crate::history::Scale;
        assert_eq!(
            history_report_url("https://192.168.1.1/", "default", Scale::FiveMinutes),
            "https://192.168.1.1/proxy/network/api/s/default/stat/report/5minutes.site"
        );
        assert_eq!(
            history_report_url("https://192.168.1.1", "default", Scale::Hourly),
            "https://192.168.1.1/proxy/network/api/s/default/stat/report/hourly.site"
        );
    }

    #[test]
    fn request_body_is_valid_json() {
        let v: serde_json::Value = serde_json::from_str(&daily_report_request(10, 20)).unwrap();
        assert_eq!(v["start"], 10);
        assert_eq!(v["end"], 20);
    }
}
