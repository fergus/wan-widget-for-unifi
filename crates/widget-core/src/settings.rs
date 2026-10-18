//! User-editable settings. The API key is NOT part of this struct: it lives in
//! the OS credential store and never touches the config file.

use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub gateway_url: String,
    pub site: String,
    pub poll_interval_ms: u64,
    pub billing_day: u32,
    pub timezone: String,
    /// Empty string disables the external-IP lookup.
    pub ip_lookup_url: String,
    /// UniFi gateways ship self-signed certificates.
    pub accept_invalid_certs: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            gateway_url: "https://192.168.1.1".into(),
            site: "default".into(),
            poll_interval_ms: 2000,
            billing_day: 1,
            timezone: "Australia/Sydney".into(),
            ip_lookup_url: "https://api.ipify.org".into(),
            accept_invalid_certs: true,
        }
    }
}

impl Settings {
    pub fn tz(&self) -> Option<Tz> {
        self.timezone.parse().ok()
    }

    /// Returns every problem found, so the settings UI can show them all at once.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errs = Vec::new();
        if !self.gateway_url.starts_with("https://") {
            errs.push("gateway_url must start with https://".into());
        }
        if self.site.is_empty() || self.site.contains('/') {
            errs.push("site must be non-empty and contain no '/'".into());
        }
        if !(1000..=60_000).contains(&self.poll_interval_ms) {
            errs.push("poll_interval_ms must be between 1000 and 60000".into());
        }
        if !(1..=31).contains(&self.billing_day) {
            errs.push("billing_day must be between 1 and 31".into());
        }
        if self.tz().is_none() {
            errs.push(format!("unknown timezone '{}'", self.timezone));
        }
        if !self.ip_lookup_url.is_empty() && !self.ip_lookup_url.starts_with("https://") {
            errs.push("ip_lookup_url must be empty or start with https://".into());
        }
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert_eq!(Settings::default().validate(), Ok(()));
    }

    #[test]
    fn partial_json_fills_in_defaults() {
        let s: Settings = serde_json::from_str(r#"{"billing_day":15}"#).unwrap();
        assert_eq!(s.billing_day, 15);
        assert_eq!(s.gateway_url, "https://192.168.1.1");
    }

    #[test]
    fn config_never_serializes_an_api_key() {
        let json = serde_json::to_string(&Settings::default()).unwrap();
        assert!(!json.to_lowercase().contains("key"));
    }

    #[test]
    fn collects_all_errors_at_once() {
        let s = Settings {
            gateway_url: "http://192.168.1.1".into(),
            poll_interval_ms: 10,
            billing_day: 0,
            timezone: "Mars/Olympus".into(),
            ..Settings::default()
        };
        assert_eq!(s.validate().unwrap_err().len(), 4);
    }

    #[test]
    fn empty_lookup_url_disables_lookup_and_is_valid() {
        let s = Settings { ip_lookup_url: String::new(), ..Settings::default() };
        assert_eq!(s.validate(), Ok(()));
    }

    #[test]
    fn poll_interval_bounds_are_inclusive() {
        for ms in [1000, 60_000] {
            let s = Settings { poll_interval_ms: ms, ..Settings::default() };
            assert_eq!(s.validate(), Ok(()));
        }
        let s = Settings { poll_interval_ms: 999, ..Settings::default() };
        assert!(s.validate().is_err());
    }
}
