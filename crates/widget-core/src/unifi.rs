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

pub fn daily_report_url(gateway_url: &str, site: &str) -> String {
    format!("{}/stat/report/daily.site", site_base(gateway_url, site))
}

pub fn sysinfo_url(gateway_url: &str, site: &str) -> String {
    format!("{}/stat/sysinfo", site_base(gateway_url, site))
}

pub fn device_url(gateway_url: &str, site: &str) -> String {
    format!("{}/stat/device", site_base(gateway_url, site))
}

#[derive(Deserialize)]
struct DeviceEntry {
    /// Only the gateway has a `wan1` block (it is null on switches and access points).
    wan1: Option<Wan1>,
    uplink: Option<Uplink>,
}

#[derive(Deserialize)]
struct Wan1 {}

#[derive(Deserialize)]
struct Uplink {
    uptime: Option<f64>,
}

/// Seconds the gateway's WAN link has been up, i.e. the time since its last disconnect,
/// from `stat/device`. On the gateway (the device with a `wan1` block) this is
/// `uplink.uptime`; the device's own `uptime` is time since it booted, which is not the
/// same. None if the gateway does not report it.
pub fn parse_wan_uptime(body: &str) -> Result<Option<u64>, ParseError> {
    Ok(parse_envelope::<DeviceEntry>(body)?
        .into_iter()
        .find(|d| d.wan1.is_some())
        .and_then(|d| d.uplink)
        .and_then(|u| u.uptime)
        .map(|u| u.max(0.0) as u64))
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
}

#[derive(Deserialize)]
struct HealthEntry {
    subsystem: String,
    latency: Option<f64>,
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
    Ok(WanHealth {
        rx_bytes_per_s: wan.rx.unwrap_or(0.0).max(0.0) as u64,
        tx_bytes_per_s: wan.tx.unwrap_or(0.0).max(0.0) as u64,
        wan_ip: wan.wan_ip.and_then(|s| s.parse().ok()),
        isp_name: wan.isp_name,
        latency_ms,
    })
}

/// One day of WAN traffic from `stat/report/daily.site`.
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
    fn latency_is_absent_without_a_www_subsystem() {
        let body = r#"{"meta":{"rc":"ok"},"data":[{"subsystem":"wan"}]}"#;
        assert_eq!(parse_health(body).unwrap().latency_ms, None);
    }

    #[test]
    fn wan_uptime_comes_from_the_gateway_not_other_devices() {
        let body = include_str!("../tests/fixtures/device.json");
        // Recorded from a UCG Ultra: link up 106380 s, device up 3235463 s.
        assert_eq!(parse_wan_uptime(body), Ok(Some(106_380)));
        assert_eq!(
            device_url("https://192.168.1.1/", "default"),
            "https://192.168.1.1/proxy/network/api/s/default/stat/device"
        );
    }

    #[test]
    fn wan_uptime_is_none_when_no_device_reports_it() {
        // A switch's uplink.uptime must not be mistaken for the WAN link's.
        let body =
            r#"{"meta":{"rc":"ok"},"data":[{"type":"usw","wan1":null,"uplink":{"uptime":5}}]}"#;
        assert_eq!(parse_wan_uptime(body), Ok(None));
        let no_uptime = r#"{"meta":{"rc":"ok"},"data":[{"type":"udm","wan1":{},"uplink":{}}]}"#;
        assert_eq!(parse_wan_uptime(no_uptime), Ok(None));
        assert!(matches!(
            parse_wan_uptime(ERROR),
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
    fn request_body_is_valid_json() {
        let v: serde_json::Value = serde_json::from_str(&daily_report_request(10, 20)).unwrap();
        assert_eq!(v["start"], 10);
        assert_eq!(v["end"], 20);
    }
}
