//! External-IP helpers. The gateway's WAN address is often CGNAT (100.64.0.0/10),
//! so the real public address has to come from an outside lookup service.

use std::net::IpAddr;

use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
#[error("lookup did not return an IP address")]
pub struct LookupError;

/// True for RFC 6598 shared address space (100.64.0.0/10), used by carrier-grade NAT.
pub fn is_cgnat(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 100 && (o[1] & 0b1100_0000) == 0b0100_0000
        }
        IpAddr::V6(_) => false,
    }
}

/// Parses a plain-text lookup response such as the body of `https://api.ipify.org`.
pub fn parse_lookup(body: &str) -> Result<IpAddr, LookupError> {
    body.trim().parse().map_err(|_| LookupError)
}

/// What the UI shows for the address row.
#[derive(Debug, Clone, PartialEq)]
pub struct IpView {
    pub external: Option<IpAddr>,
    pub wan: Option<IpAddr>,
    pub cgnat: bool,
}

pub fn ip_view(external: Option<IpAddr>, wan: Option<IpAddr>) -> IpView {
    let cgnat = wan.map(is_cgnat).unwrap_or(false)
        || (external.is_some() && wan.is_some() && external != wan);
    IpView { external, wan, cgnat }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn cgnat_range_boundaries() {
        assert!(!is_cgnat(ip("100.63.255.255")));
        assert!(is_cgnat(ip("100.64.0.0")));
        assert!(is_cgnat(ip("100.96.51.186")));
        assert!(is_cgnat(ip("100.127.255.255")));
        assert!(!is_cgnat(ip("100.128.0.0")));
        assert!(!is_cgnat(ip("10.0.0.1")));
        assert!(!is_cgnat(ip("2001:db8::1")));
    }

    #[test]
    fn lookup_trims_whitespace() {
        assert_eq!(parse_lookup(" 203.0.113.7\n"), Ok(ip("203.0.113.7")));
        assert_eq!(parse_lookup("2001:db8::1"), Ok(ip("2001:db8::1")));
    }

    #[test]
    fn lookup_rejects_html_and_empty() {
        assert_eq!(parse_lookup("<html>429</html>"), Err(LookupError));
        assert_eq!(parse_lookup(""), Err(LookupError));
    }

    #[test]
    fn view_flags_cgnat_from_wan_address() {
        let v = ip_view(Some(ip("203.0.113.7")), Some(ip("100.96.51.186")));
        assert!(v.cgnat);
    }

    #[test]
    fn view_flags_mismatch_even_without_cgnat_range() {
        let v = ip_view(Some(ip("203.0.113.7")), Some(ip("198.51.100.2")));
        assert!(v.cgnat);
    }

    #[test]
    fn view_clean_when_addresses_match() {
        let v = ip_view(Some(ip("203.0.113.7")), Some(ip("203.0.113.7")));
        assert!(!v.cgnat);
    }

    #[test]
    fn view_without_lookup_does_not_claim_a_mismatch() {
        assert!(!ip_view(None, Some(ip("203.0.113.7"))).cgnat);
    }
}
