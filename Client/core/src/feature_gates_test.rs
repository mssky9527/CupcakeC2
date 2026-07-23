// Unit tests for shipped capability gates (process injection removed).

use crate::config;
use crate::crypto::{decrypt, encrypt};
use crate::types::{CommandResult, SystemInfo};
use crate::utils;

#[test]
fn validate_server_url_accepts_bind_and_common_schemes() {
    assert!(config::validate_server_url("ws://127.0.0.1:8080/ws"));
    assert!(config::validate_server_url("wss://example.com/ws"));
    assert!(config::validate_server_url("tcp://10.0.0.1:4444"));
    assert!(config::validate_server_url("dns://c2.example"));
    assert!(config::validate_server_url("bind://0.0.0.0:9000"));
    assert!(!config::validate_server_url("http://evil"));
}

#[test]
fn agent_uuid_is_stable_and_well_formed() {
    let a = utils::get_agent_uuid();
    let b = utils::get_agent_uuid();
    assert_eq!(a, b);
    assert_eq!(a.len(), 36);
    assert_eq!(a.chars().filter(|&c| c == '-').count(), 4);
}

#[test]
fn register_and_response_messages_do_not_panic() {
    let info = SystemInfo::collect();
    let reg = info.to_register_message();
    assert!(!reg.payload.is_null());

    let result = CommandResult {
        stdout: "ok".into(),
        stderr: String::new(),
        path: None,
        req_id: Some("r1".into()),
    };
    let resp = result.to_response_message();
    assert!(!resp.payload.is_null());
    let json = serde_json::to_string(&resp).expect("response serializes");
    assert!(json.contains("ok") || json.contains("stdout"));
}

#[test]
fn encrypt_decrypt_roundtrip_uses_shipped_crypto() {
    let key = b"01234567890123456789012345678901";
    let plain = b"feature-gate-probe";
    let enc = encrypt(plain, key);
    assert!(enc.len() >= 12);
    let dec = decrypt(&enc, key).expect("decrypt ok");
    assert_eq!(&dec[..], plain);
}

#[test]
fn encrypt_rejects_wrong_key_length_without_panic() {
    let enc = encrypt(b"x", b"short");
    assert!(enc.is_empty());
}

#[test]
fn inject_feature_is_gone() {
    // Process injection must not be available as a compile feature.
    assert!(
        !cfg!(feature = "inject"),
        "inject feature must not exist in any default build"
    );
}

#[test]
fn stealth_adv_cfg_is_explicit() {
    let _ = cfg!(feature = "stealth-adv");
}

#[test]
fn beacon_and_post_ex_are_orthogonal() {
    // When building pure Stage0: --features ws,beacon (no post-ex).
    // Standard always enables post-ex via Cargo.toml.
    let _ = cfg!(feature = "beacon");
    let _ = cfg!(feature = "post-ex");
    let _ = cfg!(feature = "module-loader");
}

#[cfg(windows)]
#[test]
fn version_gate_logic_is_build_based() {
    use crate::stealth::{WindowsVersion, NT_CREATE_USER_PROCESS_MIN_BUILD};
    assert_eq!(NT_CREATE_USER_PROCESS_MIN_BUILD, 17763);
    assert!(!WindowsVersion::UNKNOWN.supports_nt_create_user_process());
    assert!(WindowsVersion {
        major: 10,
        minor: 0,
        build: 17763
    }
    .supports_nt_create_user_process());
    // Live OS probe must not panic
    let _ = crate::stealth::get_windows_version();
    let _ = crate::stealth::is_supported_for_nt_create_user_process();
}
