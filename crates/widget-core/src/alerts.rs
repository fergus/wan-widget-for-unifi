//! When to send Windows notifications, and what they say. No I/O: the app feeds in what
//! it saw and sends whatever this decides.

use std::collections::BTreeMap;

/// Online state per WAN ("WAN", "WAN2", ...). Taken from the gateway's own
/// `last_wan_status` (`"online"` or `"offline"`; anything else is left out as unknown).
/// A gateway that reports none falls back to health's `www` status as a single "WAN":
/// `"ok"` is online, `"error"` offline.
pub fn wan_online(
    last_wan_status: &BTreeMap<String, String>,
    www_status: Option<&str>,
) -> BTreeMap<String, bool> {
    let from_gateway: BTreeMap<String, bool> = last_wan_status
        .iter()
        .filter_map(|(name, s)| match s.as_str() {
            "online" => Some((name.clone(), true)),
            "offline" => Some((name.clone(), false)),
            _ => None,
        })
        .collect();
    if !last_wan_status.is_empty() {
        return from_gateway;
    }
    match www_status {
        Some("ok") => BTreeMap::from([("WAN".to_string(), true)]),
        Some("error") => BTreeMap::from([("WAN".to_string(), false)]),
        _ => BTreeMap::new(),
    }
}

/// A WAN that went offline or came back.
#[derive(Debug, Clone, PartialEq)]
pub struct WanChange {
    pub name: String,
    pub online: bool,
}

/// WANs whose state differs from the last reading. A WAN seen for the first time is not
/// a change, so starting the widget during an outage stays quiet.
pub fn wan_changes(prev: &BTreeMap<String, bool>, now: &BTreeMap<String, bool>) -> Vec<WanChange> {
    now.iter()
        .filter(|(name, online)| prev.get(*name).is_some_and(|p| p != *online))
        .map(|(name, online)| WanChange {
            name: name.clone(),
            online: *online,
        })
        .collect()
}

/// How a WAN is named in a notification.
pub fn wan_label(name: &str) -> String {
    match name {
        "WAN" => "Your internet connection".into(),
        "WAN2" => "Your second internet connection (WAN2)".into(),
        other => format!("Internet connection {other}"),
    }
}

/// True when the public IP is known both times and differs. A failed lookup (None) is
/// not a change.
pub fn ip_changed(prev: Option<&str>, now: Option<&str>) -> bool {
    matches!((prev, now), (Some(a), Some(b)) if a != b)
}

/// An outage's length for a notification: "45 seconds", "4 minutes", "2 hours 5 minutes".
pub fn format_outage(secs: u64) -> String {
    let plural = |n: u64, unit: &str| format!("{n} {unit}{}", if n == 1 { "" } else { "s" });
    if secs < 60 {
        return plural(secs, "second");
    }
    let mins = secs / 60;
    if mins < 60 {
        return plural(mins, "minute");
    }
    let (h, m) = (mins / 60, mins % 60);
    if m == 0 {
        plural(h, "hour")
    } else {
        format!("{} {}", plural(h, "hour"), plural(m, "minute"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn online(pairs: &[(&str, bool)]) -> BTreeMap<String, bool> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn gateway_status_is_used_when_reported() {
        // As a UCG Ultra reports it: {"WAN":"online"}.
        assert_eq!(
            wan_online(&status(&[("WAN", "online")]), Some("error")),
            online(&[("WAN", true)])
        );
        assert_eq!(
            wan_online(
                &status(&[("WAN", "offline"), ("WAN2", "online"), ("WAN3", "?")]),
                None
            ),
            online(&[("WAN", false), ("WAN2", true)])
        );
    }

    #[test]
    fn health_www_status_is_the_fallback() {
        let none = BTreeMap::new();
        assert_eq!(wan_online(&none, Some("ok")), online(&[("WAN", true)]));
        assert_eq!(wan_online(&none, Some("error")), online(&[("WAN", false)]));
        assert!(wan_online(&none, Some("warning")).is_empty());
        assert!(wan_online(&none, None).is_empty());
    }

    #[test]
    fn only_real_transitions_are_changes() {
        let up = online(&[("WAN", true)]);
        let down = online(&[("WAN", false)]);
        assert_eq!(
            wan_changes(&up, &down),
            vec![WanChange {
                name: "WAN".into(),
                online: false
            }]
        );
        assert_eq!(
            wan_changes(&down, &up),
            vec![WanChange {
                name: "WAN".into(),
                online: true
            }]
        );
        assert!(wan_changes(&up, &up).is_empty());
        // First reading, even if offline: no notification.
        assert!(wan_changes(&BTreeMap::new(), &down).is_empty());
        // A WAN that stops being reported is not "offline".
        assert!(wan_changes(&up, &BTreeMap::new()).is_empty());
    }

    #[test]
    fn second_wan_changes_on_its_own() {
        let before = online(&[("WAN", true), ("WAN2", true)]);
        let after = online(&[("WAN", true), ("WAN2", false)]);
        assert_eq!(
            wan_changes(&before, &after),
            vec![WanChange {
                name: "WAN2".into(),
                online: false
            }]
        );
        assert_eq!(wan_label("WAN2"), "Your second internet connection (WAN2)");
    }

    #[test]
    fn ip_change_needs_two_known_addresses() {
        assert!(ip_changed(Some("203.0.113.7"), Some("203.0.113.9")));
        assert!(!ip_changed(Some("203.0.113.7"), Some("203.0.113.7")));
        assert!(!ip_changed(None, Some("203.0.113.7")));
        assert!(!ip_changed(Some("203.0.113.7"), None));
    }

    #[test]
    fn outages_read_naturally() {
        assert_eq!(format_outage(1), "1 second");
        assert_eq!(format_outage(45), "45 seconds");
        assert_eq!(format_outage(60), "1 minute");
        assert_eq!(format_outage(299), "4 minutes");
        assert_eq!(format_outage(3600), "1 hour");
        assert_eq!(format_outage(7500), "2 hours 5 minutes");
    }
}
