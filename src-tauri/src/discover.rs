//! Finds UniFi OS consoles on the local network for the Settings window.
//!
//! Probes a few likely addresses with `GET /api/system`, which a UniFi OS console
//! answers without an API key. No key is sent, nothing is pinned, and any certificate
//! is accepted, since nothing secret is exchanged and the user still confirms the
//! address before the widget uses it.

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use serde::Serialize;
use widget_core::unifi;

/// Addresses UniFi gateways commonly use, tried after the PC's own network.
const COMMON: [Ipv4Addr; 2] = [Ipv4Addr::new(192, 168, 1, 1), Ipv4Addr::new(192, 168, 0, 1)];

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Found {
    /// Gateway URL to put in Settings, e.g. "https://192.168.1.1".
    pub url: String,
    /// The console's model code from `/api/system`, e.g. "UDRULT". May be empty.
    pub model: String,
}

/// Candidate addresses, most likely first: the .1 address of the PC's own /24 network
/// (where UniFi puts the gateway by default), then the common defaults. No duplicates.
fn candidates(local: Option<IpAddr>) -> Vec<Ipv4Addr> {
    let mut out = Vec::new();
    if let Some(IpAddr::V4(ip)) = local {
        let [a, b, c, _] = ip.octets();
        if ip.is_private() {
            out.push(Ipv4Addr::new(a, b, c, 1));
        }
    }
    for ip in COMMON {
        if !out.contains(&ip) {
            out.push(ip);
        }
    }
    out
}

async fn probe(client: reqwest::Client, ip: Ipv4Addr) -> Option<Found> {
    let url = format!("https://{ip}");
    let body = client
        .get(unifi::console_url(&url))
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
    unifi::parse_console(&body).map(|model| Found { url, model })
}

/// Probes the candidates in parallel and returns the consoles that answered, in
/// candidate order. Takes about 3 seconds at most.
pub async fn find(local: Option<IpAddr>) -> Vec<Found> {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .danger_accept_invalid_certs(true)
        .build()
    else {
        return Vec::new();
    };
    let tasks: Vec<_> = candidates(local)
        .into_iter()
        .map(|ip| tauri::async_runtime::spawn(probe(client.clone(), ip)))
        .collect();
    let mut found = Vec::new();
    for t in tasks {
        if let Ok(Some(f)) = t.await {
            found.push(f);
        }
    }
    log::info!("gateway discovery found {}", found.len());
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> Option<IpAddr> {
        s.parse().ok()
    }

    #[test]
    fn own_network_comes_first() {
        assert_eq!(
            candidates(ip("10.0.20.57")),
            vec![
                Ipv4Addr::new(10, 0, 20, 1),
                Ipv4Addr::new(192, 168, 1, 1),
                Ipv4Addr::new(192, 168, 0, 1)
            ]
        );
    }

    #[test]
    fn no_duplicates_on_a_default_network() {
        assert_eq!(
            candidates(ip("192.168.1.42")),
            vec![Ipv4Addr::new(192, 168, 1, 1), Ipv4Addr::new(192, 168, 0, 1)]
        );
    }

    #[test]
    fn public_or_missing_addresses_only_try_the_defaults() {
        let defaults = vec![Ipv4Addr::new(192, 168, 1, 1), Ipv4Addr::new(192, 168, 0, 1)];
        assert_eq!(candidates(ip("203.0.113.9")), defaults);
        assert_eq!(candidates(None), defaults);
        assert_eq!(candidates(ip("fe80::1")), defaults);
    }
}
