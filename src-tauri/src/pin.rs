//! Trust-on-first-use pinning for the gateway's self-signed certificate.
//!
//! The first successful TLS handshake records the SHA-256 of the gateway's leaf
//! certificate in gateway-pin.json (app config dir). Later handshakes must present
//! the same certificate; a different one is refused and flagged, so the widget can
//! tell the user. Forgetting the pin (Settings) accepts the next certificate.

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

#[derive(Serialize, Deserialize)]
struct Stored {
    sha256: String,
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

fn read_pin(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str::<Stored>(&text).ok().map(|s| s.sha256)
}

fn write_pin(path: &Path, sha256: &str) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string(&Stored { sha256: sha256.to_string() }) {
        let _ = fs::write(path, json);
    }
}

/// Removes the stored pin so the next certificate is trusted on first use.
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
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let fp = fingerprint(end_entity.as_ref());
        match read_pin(&self.path) {
            Some(pinned) if pinned == fp => {
                MISMATCH.store(false, Ordering::Relaxed);
                Ok(ServerCertVerified::assertion())
            }
            Some(_) => {
                MISMATCH.store(true, Ordering::Relaxed);
                Err(Error::General("gateway certificate does not match the pinned one".into()))
            }
            None => {
                write_pin(&self.path, &fp);
                MISMATCH.store(false, Ordering::Relaxed);
                Ok(ServerCertVerified::assertion())
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
/// the public CA roots. Handshake signatures are still verified.
pub fn tls_config(path: PathBuf) -> Result<ClientConfig, Error> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = PinVerifier { path, provider: provider.clone() };
    Ok(ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth())
}
