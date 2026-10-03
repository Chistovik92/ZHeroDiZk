// SPDX-License-Identifier: AGPL-3.0-only
//! Settings come from environment variables; nothing is read from files implicitly.

use std::net::SocketAddr;

pub const DEFAULT_LISTEN: &str = "127.0.0.1:21114";

#[derive(Debug, PartialEq, Eq)]
pub struct Config {
    pub listen: SocketAddr,
    pub database_url: String,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("ZHD_DATABASE_URL is required")]
    MissingDatabaseUrl,
    #[error("ZHD_LISTEN is not a valid socket address: {0}")]
    InvalidListen(String),
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
        Ok(Self { listen, database_url })
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
}
