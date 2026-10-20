//! Plain-language messages for failed requests. The widget and Settings show these;
//! the raw technical error goes to the log.

use reqwest::StatusCode;
use widget_core::unifi::ParseError;

use crate::pin;

/// Who a request went to, so messages name the right thing.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Peer {
    Gateway,
    Lookup,
}

impl Peer {
    fn name(self) -> &'static str {
        match self {
            Peer::Gateway => "the gateway",
            Peer::Lookup => "the IP lookup service",
        }
    }
}

/// A failed request: the plain-language message plus whether the peer could not be
/// reached at all (timeout or no connection). The poller uses that to spot that the PC
/// has left the gateway's network; a wrong key, a bad site or a pin mismatch are not that.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub message: String,
    pub unreachable: bool,
}

impl Failure {
    pub fn other(message: String) -> Self {
        Self {
            message,
            unreachable: false,
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<Failure> for String {
    fn from(f: Failure) -> String {
        f.message
    }
}

/// Like `send`, but also records whether the peer was unreachable.
pub fn send_failure(e: &reqwest::Error, peer: Peer) -> Failure {
    Failure {
        message: send(e, peer),
        unreachable: peer == Peer::Gateway
            && !pin::mismatch()
            && (e.is_timeout() || e.is_connect()),
    }
}

/// A request that never got an HTTP reply: unreachable, timed out, or TLS refused.
pub fn send(e: &reqwest::Error, peer: Peer) -> String {
    log::debug!("request to {} failed: {e:?}", peer.name());
    if peer == Peer::Gateway && pin::mismatch() {
        return "The gateway's certificate has changed. If you expected that, press Forget \
                pinned certificate in Settings"
            .into();
    }
    if e.is_timeout() {
        return format!("No answer from {} within 8 seconds", peer.name());
    }
    if e.is_connect() && peer == Peer::Gateway {
        return "Can't reach the gateway. Check the address and that this PC is on the same \
                network"
            .into();
    }
    format!("Lost the connection to {}", peer.name())
}

/// An HTTP reply that is not a success. `body` tells apart the gateway's 401s: an
/// unknown site answers 401 with `api.err.NoSiteContext`, a bad key answers 401 without.
pub fn status(code: StatusCode, peer: Peer, body: &str) -> String {
    let n = code.as_u16();
    let no_site = body.contains("api.err.NoSiteContext");
    match (n, peer) {
        (401 | 404, Peer::Gateway) if no_site || n == 404 => {
            "The gateway has no site with that name. Check the site in \
                                 Settings (usually \"default\")"
                .into()
        }
        (401 | 403, Peer::Gateway) => {
            "The gateway rejected the API key. Check it in Settings".into()
        }
        (500..=599, _) => format!(
            "{} had an internal error (HTTP {n})",
            capitalise(peer.name())
        ),
        _ => format!("Unexpected reply from {} (HTTP {n})", peer.name()),
    }
}

/// A gateway reply that could not be read.
pub fn parse(e: &ParseError) -> String {
    log::debug!("unreadable gateway reply: {e}");
    match e {
        ParseError::Json(_) => "The gateway's reply could not be read. Its Network app \
                                version may not be supported yet"
            .into(),
        ParseError::Controller(rc) => format!("The gateway refused the request ({rc})"),
        ParseError::NoWan => "The gateway reports no internet (WAN) connection".into(),
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// Bytes per second as the widget shows it, e.g. "4.1 Mbit/s".
pub fn rate(bytes_per_s: u64) -> String {
    let bits = bytes_per_s as f64 * 8.0;
    if bits >= 1e9 {
        format!("{:.2} Gbit/s", bits / 1e9)
    } else if bits >= 1e6 {
        format!("{:.1} Mbit/s", bits / 1e6)
    } else if bits >= 1e3 {
        format!("{:.0} kbit/s", bits / 1e3)
    } else {
        format!("{bits:.0} bit/s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shapes of the 401 replies from a UCG Ultra (Network 10.6).
    const NO_SITE: &str = r#"{"meta":{"rc":"error","msg":"api.err.NoSiteContext"},"data":[]}"#;
    const BAD_KEY: &str = r#"{"error":{"code":401,"message":"Unauthorized"}}"#;

    #[test]
    fn bad_key_and_bad_site_are_told_apart() {
        let key = status(StatusCode::UNAUTHORIZED, Peer::Gateway, BAD_KEY);
        assert!(key.contains("API key"), "{key}");
        let site = status(StatusCode::UNAUTHORIZED, Peer::Gateway, NO_SITE);
        assert!(site.contains("no site"), "{site}");
        assert!(status(StatusCode::FORBIDDEN, Peer::Gateway, "").contains("API key"));
        assert!(status(StatusCode::NOT_FOUND, Peer::Gateway, "").contains("no site"));
    }

    #[test]
    fn lookup_statuses_do_not_blame_the_key() {
        let m = status(StatusCode::UNAUTHORIZED, Peer::Lookup, NO_SITE);
        assert_eq!(m, "Unexpected reply from the IP lookup service (HTTP 401)");
        assert_eq!(
            status(StatusCode::BAD_GATEWAY, Peer::Lookup, ""),
            "The IP lookup service had an internal error (HTTP 502)"
        );
    }

    #[test]
    fn parse_errors_are_plain() {
        assert!(parse(&ParseError::Json("expected value".into())).contains("could not be read"));
        assert!(parse(&ParseError::NoWan).contains("no internet"));
    }

    #[test]
    fn rates_match_the_widget() {
        assert_eq!(rate(0), "0 bit/s");
        assert_eq!(rate(512_000), "4.1 Mbit/s");
        assert_eq!(rate(43_000), "344 kbit/s");
        assert_eq!(rate(250_000_000), "2.00 Gbit/s");
    }
}
