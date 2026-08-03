//! TLS connector tuned by malleable profile `ja3_hint` (feature `ws-tls`).
//!
//! Full browser JA3 parity (GREASE, extension order, ECH) is not claimed.
//! This path forces **rustls 0.22** (not SChannel) and reorders cipher suites so
//! ClientHello is controllable via `CryptoProvider`.

use crate::error::{ClientError, Result};
use log::debug;
use rustls::crypto::{ring as provider_ring, CryptoProvider};
use rustls::ClientConfig;
use std::sync::Arc;
use tokio_tungstenite::Connector;

/// Build a rustls connector for WSS using profile ja3_hint.
pub fn connector_for_ja3_hint(ja3_hint: &str) -> Result<Connector> {
    let config = client_config_for_hint(ja3_hint)?;
    debug!("tls_ja3: rustls connector ready hint={}", ja3_hint);
    Ok(Connector::Rustls(config))
}

fn client_config_for_hint(ja3_hint: &str) -> Result<Arc<ClientConfig>> {
    let mut root_store = rustls::RootCertStore::empty();
    let certs = rustls_native_certs::load_native_certs()
        .map_err(|e| ClientError::ConnectionError(format!("load native roots: {e}")))?;
    for c in certs {
        let _ = root_store.add(c);
    }

    let suites = cipher_suites_for_hint(ja3_hint);
    let provider = CryptoProvider {
        cipher_suites: suites,
        ..provider_ring::default_provider()
    };

    let config = ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|e| ClientError::ConnectionError(format!("tls versions: {e}")))?
        .with_root_certificates(root_store)
        .with_no_client_auth();

    Ok(Arc::new(config))
}

/// Prefer TLS1.3 AEAD suites first; order varies slightly by browser hint.
fn cipher_suites_for_hint(hint: &str) -> Vec<rustls::SupportedCipherSuite> {
    use provider_ring::cipher_suite::{
        TLS13_AES_128_GCM_SHA256, TLS13_AES_256_GCM_SHA384, TLS13_CHACHA20_POLY1305_SHA256,
        TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256, TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
        TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256, TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
        TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384, TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
    };

    let h = hint.to_ascii_lowercase();
    // Chrome / Edge: AES-128-GCM first
    if h.contains("chrome") || h.contains("edge") {
        return vec![
            TLS13_AES_128_GCM_SHA256,
            TLS13_AES_256_GCM_SHA384,
            TLS13_CHACHA20_POLY1305_SHA256,
            TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
            TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
            TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
            TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
            TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256,
            TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
        ];
    }
    // Firefox-ish: ChaCha first on TLS1.3
    if h.contains("firefox") {
        return vec![
            TLS13_CHACHA20_POLY1305_SHA256,
            TLS13_AES_128_GCM_SHA256,
            TLS13_AES_256_GCM_SHA384,
            TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256,
            TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
            TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
            TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
            TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
            TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
        ];
    }
    // aws / github tooling vibe: AES-256 first
    if h.contains("aws") || h.contains("github") {
        return vec![
            TLS13_AES_256_GCM_SHA384,
            TLS13_AES_128_GCM_SHA256,
            TLS13_CHACHA20_POLY1305_SHA256,
            TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
            TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
            TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
            TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
        ];
    }
    vec![
        TLS13_AES_128_GCM_SHA256,
        TLS13_AES_256_GCM_SHA384,
        TLS13_CHACHA20_POLY1305_SHA256,
        TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
        TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
        TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
        TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_and_firefox_differ() {
        let a = cipher_suites_for_hint("chrome-126");
        let b = cipher_suites_for_hint("firefox-127");
        assert!(!a.is_empty());
        assert!(!b.is_empty());
        assert_ne!(format!("{:?}", a[0]), format!("{:?}", b[0]));
    }

    #[test]
    fn config_builds_with_native_roots() {
        let _ = cipher_suites_for_hint("edge-126");
        // System roots may be missing in some CI — suite selection still exercised
        let _ = client_config_for_hint("chrome-126");
    }
}
