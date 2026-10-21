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
    /// IANA zone for the billing cycle. Empty (the default) uses the gateway's zone.
    pub timezone: String,
    /// Empty string disables the external-IP lookup.
    pub ip_lookup_url: String,
    /// UniFi gateways ship self-signed certificates.
    pub accept_invalid_certs: bool,
    /// "system" (follow Windows), "dark" or "light".
    pub theme: String,
    /// Which Windows notifications to send.
    pub notify: Notify,
}

/// Windows notifications, each on by default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Notify {
    /// The internet connection (a WAN) goes down, and when it comes back.
    pub wan: bool,
    /// The public IP address changes.
    pub ip: bool,
    /// The gateway stops answering while the PC is still on the home network.
    pub gateway: bool,
    /// A new version is available (once per version each time the widget starts).
    pub update: bool,
}

impl Default for Notify {
    fn default() -> Self {
        Self {
            wan: true,
            ip: true,
            gateway: true,
            update: true,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            gateway_url: "https://192.168.1.1".into(),
            site: "default".into(),
            poll_interval_ms: 2000,
            billing_day: 1,
            timezone: String::new(),
            ip_lookup_url: "https://api.ipify.org".into(),
            accept_invalid_certs: true,
            theme: "system".into(),
            notify: Notify::default(),
        }
    }
}

impl Settings {
    /// The timezone override, or None when the setting is blank or unknown.
    pub fn tz(&self) -> Option<Tz> {
        self.timezone.parse().ok()
    }

    /// Zone for the billing cycle: the setting if given, else the gateway's, else the
    /// PC's. Daily buckets follow the gateway's zone, so that is the right default.
    pub fn resolve_tz(&self, gateway: Option<&str>, system: Option<&str>) -> Option<Tz> {
        self.tz()
            .or_else(|| gateway.and_then(|z| z.parse().ok()))
            .or_else(|| system.and_then(|z| z.parse().ok()))
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
        if !self.timezone.is_empty() && self.tz().is_none() {
            errs.push(format!("unknown timezone '{}'", self.timezone));
        }
        if !self.ip_lookup_url.is_empty() && !self.ip_lookup_url.starts_with("https://") {
            errs.push("ip_lookup_url must be empty or start with https://".into());
        }
        if !["system", "dark", "light"].contains(&self.theme.as_str()) {
            errs.push("theme must be system, dark or light".into());
        }
        if errs.is_empty() {
            Ok(())
        } else {
            Err(errs)
        }
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
    fn blank_timezone_is_valid_and_follows_the_gateway() {
        let s = Settings::default();
        assert_eq!(s.validate(), Ok(()));
        assert_eq!(
            s.resolve_tz(Some("Europe/London"), Some("Australia/Perth")),
            Some(chrono_tz::Europe::London)
        );
    }

    #[test]
    fn timezone_setting_overrides_the_gateway() {
        let s = Settings {
            timezone: "Pacific/Auckland".into(),
            ..Settings::default()
        };
        assert_eq!(
            s.resolve_tz(Some("Europe/London"), None),
            Some(chrono_tz::Pacific::Auckland)
        );
    }

    #[test]
    fn unknown_gateway_zone_falls_back_to_the_pc() {
        let s = Settings::default();
        assert_eq!(
            s.resolve_tz(Some("Not/AZone"), Some("Australia/Perth")),
            Some(chrono_tz::Australia::Perth)
        );
        assert_eq!(s.resolve_tz(None, None), None);
    }

    #[test]
    fn notifications_default_on_and_older_files_still_load() {
        let s: Settings = serde_json::from_str(r#"{"billing_day":15}"#).unwrap();
        assert_eq!(s.notify, Notify::default());
        assert!(s.notify.wan && s.notify.ip && s.notify.gateway && s.notify.update);
        let s: Settings = serde_json::from_str(r#"{"notify":{"ip":false}}"#).unwrap();
        assert!(!s.notify.ip && s.notify.wan);
    }

    #[test]
    fn theme_must_be_a_known_value() {
        for t in ["system", "dark", "light"] {
            let s = Settings {
                theme: t.into(),
                ..Settings::default()
            };
            assert_eq!(s.validate(), Ok(()));
        }
        let s = Settings {
            theme: "neon".into(),
            ..Settings::default()
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn empty_lookup_url_disables_lookup_and_is_valid() {
        let s = Settings {
            ip_lookup_url: String::new(),
            ..Settings::default()
        };
        assert_eq!(s.validate(), Ok(()));
    }

    #[test]
    fn poll_interval_bounds_are_inclusive() {
        for ms in [1000, 60_000] {
            let s = Settings {
                poll_interval_ms: ms,
                ..Settings::default()
            };
            assert_eq!(s.validate(), Ok(()));
        }
        let s = Settings {
            poll_interval_ms: 999,
            ..Settings::default()
        };
        assert!(s.validate().is_err());
    }
}
