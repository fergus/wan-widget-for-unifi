//! Trust-on-first-use pinning for the gateway's self-signed certificate.
//!
//! The first successful TLS handshake with a host records the SHA-256 of its leaf
//! certificate in gateway-pin.json (app config dir), keyed by host name or IP. Later
//! handshakes with that host must present the same certificate; a different one is
//! refused and flagged, so the widget can tell the user. Forgetting the pins (Settings)
//! accepts the next certificate.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error, SignatureScheme};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

/// Set when the last handshake presented a certificate other than the pinned one.
static MISMATCH: AtomicBool = AtomicBool::new(false);
/// Set by forget(), so the poller retries now instead of waiting out its backoff.
static RESET: AtomicBool = AtomicBool::new(false);

/// True once after forget(). The poller uses it to cut a backoff wait short.
pub fn take_reset() -> bool {
    RESET.swap(false, Ordering::Relaxed)
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
struct Stored {
    /// Host name or IP address to the SHA-256 of its leaf certificate.
    #[serde(default)]
    hosts: BTreeMap<String, String>,
    /// The single pin written before pins were kept per host. It is taken as the pin
    /// of the first host that matches it, then dropped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
}

#[derive(Debug, PartialEq)]
enum Verdict {
    Trusted,
    Mismatch,
}

/// Whether `fp` is trusted for `host`, and the pins to save if they change.
/// `record` saves a first-seen host; without it an unknown host is trusted unsaved.
fn check(stored: &Stored, host: &str, fp: &str, record: bool) -> (Verdict, Option<Stored>) {
    match stored.hosts.get(host).or(stored.sha256.as_ref()) {
        Some(pinned) if pinned != fp => (Verdict::Mismatch, None),
        Some(_) if stored.hosts.contains_key(host) => (Verdict::Trusted, None),
        _ if !record => (Verdict::Trusted, None),
        // A new host, or the legacy pin matched: save it under this host.
        _ => {
            let mut next = stored.clone();
            next.sha256 = None;
            next.hosts.insert(host.to_string(), fp.to_string());
            (Verdict::Trusted, Some(next))
        }
    }
}

fn host_key(name: &ServerName<'_>) -> String {
    match name {
        ServerName::DnsName(d) => d.as_ref().to_ascii_lowercase(),
        ServerName::IpAddress(ip) => std::net::IpAddr::from(*ip).to_string(),
        _ => String::new(),
    }
}

pub fn path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("gateway-pin.json"))
}

pub fn mismatch() -> bool {
    MISMATCH.load(Ordering::Relaxed)
}

fn fingerprint(der: &[u8]) -> String {
    Sha256::digest(der).iter().map(|b| format!("{b:02x}")).collect()
}

fn read_pins(path: &Path) -> Stored {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_pins(path: &Path, stored: &Stored) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(stored) {
        let _ = fs::write(path, json);
    }
}

/// Removes every stored pin so the next certificate is trusted on first use.
pub fn forget(app: &AppHandle) {
    if let Some(p) = path(app) {
        let _ = fs::remove_file(p);
    }
    MISMATCH.store(false, Ordering::Relaxed);
    RESET.store(true, Ordering::Relaxed);
}

#[derive(Debug)]
struct PinVerifier {
    path: PathBuf,
    record: bool,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let fp = fingerprint(end_entity.as_ref());
        let (verdict, save) = check(&read_pins(&self.path), &host_key(server_name), &fp, self.record);
        if let Some(next) = save {
            write_pins(&self.path, &next);
        }
        match verdict {
            Verdict::Trusted => {
                MISMATCH.store(false, Ordering::Relaxed);
                Ok(ServerCertVerified::assertion())
            }
            Verdict::Mismatch => {
                MISMATCH.store(true, Ordering::Relaxed);
                Err(Error::General("gateway certificate does not match the pinned one".into()))
            }
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// TLS config that checks the gateway certificate against the stored pin instead of
/// the public CA roots. Handshake signatures are still verified. `record` saves the pin
/// of a host seen for the first time.
pub fn tls_config(path: PathBuf, record: bool) -> Result<ClientConfig, Error> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = PinVerifier { path, record, provider: provider.clone() };
    Ok(ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pins(hosts: &[(&str, &str)]) -> Stored {
        Stored {
            hosts: hosts.iter().map(|(h, f)| (h.to_string(), f.to_string())).collect(),
            sha256: None,
        }
    }

    #[test]
    fn first_host_is_pinned_when_recording() {
        let (v, save) = check(&Stored::default(), "192.168.1.1", "aa", true);
        assert_eq!(v, Verdict::Trusted);
        assert_eq!(save, Some(pins(&[("192.168.1.1", "aa")])));
    }

    #[test]
    fn unknown_host_is_trusted_but_not_saved_without_recording() {
        let (v, save) = check(&pins(&[("192.168.1.1", "aa")]), "192.168.0.1", "bb", false);
        assert_eq!(v, Verdict::Trusted);
        assert_eq!(save, None);
    }

    #[test]
    fn changed_certificate_is_refused_even_when_testing() {
        for record in [true, false] {
            let (v, save) = check(&pins(&[("192.168.1.1", "aa")]), "192.168.1.1", "bb", record);
            assert_eq!(v, Verdict::Mismatch);
            assert_eq!(save, None);
        }
    }

    #[test]
    fn each_host_keeps_its_own_pin() {
        let stored = pins(&[("192.168.1.1", "aa")]);
        let (v, save) = check(&stored, "gateway.lan", "bb", true);
        assert_eq!(v, Verdict::Trusted);
        assert_eq!(save, Some(pins(&[("192.168.1.1", "aa"), ("gateway.lan", "bb")])));
        assert_eq!(check(&stored, "192.168.1.1", "aa", true), (Verdict::Trusted, None));
    }

    #[test]
    fn legacy_pin_moves_to_the_first_matching_host() {
        let legacy: Stored = serde_json::from_str(r#"{"sha256":"aa"}"#).unwrap();
        let (v, save) = check(&legacy, "192.168.1.1", "aa", true);
        assert_eq!(v, Verdict::Trusted);
        assert_eq!(save, Some(pins(&[("192.168.1.1", "aa")])));
        assert_eq!(check(&legacy, "192.168.1.1", "bb", true).0, Verdict::Mismatch);
    }
}
