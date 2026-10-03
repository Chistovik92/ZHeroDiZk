// SPDX-License-Identifier: AGPL-3.0-only
//! Settings come from environment variables; nothing is read from files implicitly.

use std::net::SocketAddr;

pub const DEFAULT_LISTEN: &str = "127.0.0.1:21114";

#[derive(Debug, PartialEq, Eq)]
pub struct Config {
    pub listen: SocketAddr,
    pub database_url: String,
    /// Whether accounts other than the very first one may be created through the API.
    pub allow_registration: bool,
    /// 32-byte key (base64) used to encrypt TOTP secrets; MFA is unavailable without it.
    pub mfa_key: Option<[u8; 32]>,
    /// 32-byte seed (base64) of the Ed25519 key that signs session grants.
    pub grant_key: Option<[u8; 32]>,
    /// Anonymous requests per minute and address (0 = no limit).
    pub auth_rate_limit: u32,
    /// Trust `X-Forwarded-For` for the client address (only behind a trusted reverse proxy).
    pub trust_forwarded_for: bool,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("ZHD_DATABASE_URL is required")]
    MissingDatabaseUrl,
    #[error("ZHD_LISTEN is not a valid socket address: {0}")]
    InvalidListen(String),
    #[error("ZHD_ALLOW_REGISTRATION must be true or false, got: {0}")]
    InvalidFlag(String),
    #[error("ZHD_MFA_KEY must be 32 bytes encoded as standard base64")]
    InvalidMfaKey,
    #[error("ZHD_GRANT_KEY must be 32 bytes encoded as standard base64")]
    InvalidGrantKey,
    #[error("ZHD_AUTH_RATE_LIMIT must be a whole number of requests per minute (0 = off), got: {0}")]
    InvalidRateLimit(String),
}

/// An optional 32-byte key in standard base64; empty or missing means "not configured".
fn parse_key(value: Option<String>) -> Result<Option<[u8; 32]>, ()> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    match value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(text) => {
            let bytes = STANDARD.decode(text).map_err(|_| ())?;
            Ok(Some(<[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| ())?))
        }
    }
}

fn flag(value: Option<String>, name: &str) -> Result<bool, ConfigError> {
    match value.map(|v| v.trim().to_lowercase()).as_deref() {
        None | Some("") | Some("false") | Some("0") => Ok(false),
        Some("true") | Some("1") => Ok(true),
        Some(other) => Err(ConfigError::InvalidFlag(format!("{name}={other}"))),
    }
}

impl Config {
    /// `get` returns the value of an environment variable, so tests need no process state.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let database_url = get("ZHD_DATABASE_URL")
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
            .ok_or(ConfigError::MissingDatabaseUrl)?;
        let listen_text = get("ZHD_LISTEN")
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| DEFAULT_LISTEN.to_owned());
        let listen = listen_text
            .parse()
            .map_err(|_| ConfigError::InvalidListen(listen_text))?;
        let allow_registration = match get("ZHD_ALLOW_REGISTRATION").map(|v| v.trim().to_lowercase()) {
            None => false,
            Some(v) if v.is_empty() || v == "false" || v == "0" => false,
            Some(v) if v == "true" || v == "1" => true,
            Some(other) => return Err(ConfigError::InvalidFlag(other)),
        };
        let mfa_key = parse_key(get("ZHD_MFA_KEY")).map_err(|_| ConfigError::InvalidMfaKey)?;
        let grant_key = parse_key(get("ZHD_GRANT_KEY")).map_err(|_| ConfigError::InvalidGrantKey)?;
        let auth_rate_limit = match get("ZHD_AUTH_RATE_LIMIT").map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()) {
            None => 30,
            Some(text) => text.parse().map_err(|_| ConfigError::InvalidRateLimit(text))?,
        };
        let trust_forwarded_for = flag(get("ZHD_TRUST_FORWARDED_FOR"), "ZHD_TRUST_FORWARDED_FOR")?;
        Ok(Self { listen, database_url, allow_registration, mfa_key, grant_key, auth_rate_limit, trust_forwarded_for })
    }

    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> =
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn database_url_is_required() {
        assert_eq!(Config::from_lookup(lookup(&[])), Err(ConfigError::MissingDatabaseUrl));
        assert_eq!(
            Config::from_lookup(lookup(&[("ZHD_DATABASE_URL", "   ")])),
            Err(ConfigError::MissingDatabaseUrl)
        );
    }

    #[test]
    fn listens_on_loopback_by_default() {
        let config = Config::from_lookup(lookup(&[("ZHD_DATABASE_URL", "postgres://x")])).unwrap();
        assert_eq!(config.listen, DEFAULT_LISTEN.parse().unwrap());
        assert!(config.listen.ip().is_loopback());
    }

    #[test]
    fn custom_listen_address_is_validated() {
        let ok = Config::from_lookup(lookup(&[
            ("ZHD_DATABASE_URL", "postgres://x"),
            ("ZHD_LISTEN", "0.0.0.0:8443"),
        ]))
        .unwrap();
        assert_eq!(ok.listen.port(), 8443);
        let bad = Config::from_lookup(lookup(&[
            ("ZHD_DATABASE_URL", "postgres://x"),
            ("ZHD_LISTEN", "not-an-address"),
        ]));
        assert_eq!(bad, Err(ConfigError::InvalidListen("not-an-address".into())));
    }

    #[test]
    fn mfa_key_must_be_32_bytes_of_base64() {
        let base = ("ZHD_DATABASE_URL", "postgres://x");
        assert_eq!(Config::from_lookup(lookup(&[base])).unwrap().mfa_key, None);
        // 32 bytes of 0x01
        let good = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
        assert_eq!(Config::from_lookup(lookup(&[base, ("ZHD_MFA_KEY", good)])).unwrap().mfa_key, Some([1u8; 32]));
        for bad in ["not base64!", "AQEB", "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE="] {
            assert_eq!(Config::from_lookup(lookup(&[base, ("ZHD_MFA_KEY", bad)])), Err(ConfigError::InvalidMfaKey), "{bad}");
        }
    }

    #[test]
    fn rate_limit_and_proxy_settings() {
        let base = ("ZHD_DATABASE_URL", "postgres://x");
        let defaults = Config::from_lookup(lookup(&[base])).unwrap();
        assert_eq!((defaults.auth_rate_limit, defaults.trust_forwarded_for), (30, false));
        let set = Config::from_lookup(lookup(&[base, ("ZHD_AUTH_RATE_LIMIT", "5"), ("ZHD_TRUST_FORWARDED_FOR", "true")])).unwrap();
        assert_eq!((set.auth_rate_limit, set.trust_forwarded_for), (5, true));
        assert_eq!(Config::from_lookup(lookup(&[base, ("ZHD_AUTH_RATE_LIMIT", "0")])).unwrap().auth_rate_limit, 0);
        assert!(Config::from_lookup(lookup(&[base, ("ZHD_AUTH_RATE_LIMIT", "-1")])).is_err());
        assert!(Config::from_lookup(lookup(&[base, ("ZHD_TRUST_FORWARDED_FOR", "maybe")])).is_err());
    }

    #[test]
    fn grant_key_follows_the_same_rules() {
        let base = ("ZHD_DATABASE_URL", "postgres://x");
        assert_eq!(Config::from_lookup(lookup(&[base])).unwrap().grant_key, None);
        let good = "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=";
        assert_eq!(Config::from_lookup(lookup(&[base, ("ZHD_GRANT_KEY", good)])).unwrap().grant_key, Some([2u8; 32]));
        assert_eq!(Config::from_lookup(lookup(&[base, ("ZHD_GRANT_KEY", "AQEB")])), Err(ConfigError::InvalidGrantKey));
    }

    #[test]
    fn registration_flag_is_strict_and_closed_by_default() {
        let base = [("ZHD_DATABASE_URL", "postgres://x")];
        assert!(!Config::from_lookup(lookup(&base)).unwrap().allow_registration);
        for (text, expected) in [("true", true), ("1", true), ("TRUE", true), ("false", false), ("0", false), ("", false)] {
            let config = Config::from_lookup(lookup(&[base[0], ("ZHD_ALLOW_REGISTRATION", text)])).unwrap();
            assert_eq!(config.allow_registration, expected, "{text:?}");
        }
        let bad = Config::from_lookup(lookup(&[base[0], ("ZHD_ALLOW_REGISTRATION", "yes")]));
        assert_eq!(bad, Err(ConfigError::InvalidFlag("yes".into())));
    }
}
