// SPDX-License-Identifier: AGPL-3.0-only
//! Per-address request limit for the endpoints an anonymous caller can reach (registration, login,
//! second factor, device enrolment). A fixed window per address; the limiter holds no secrets
//! and forgets old windows. Without a known client address a request is not limited: in the
//! running server the address is always known (see `main.rs`).

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{atomic::{AtomicI64, Ordering}, Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};

/// What the middleware needs; cloned into every request.
#[derive(Clone)]
pub struct LimitState {
    pub limiter: Arc<RateLimiter>,
    pub trust_forwarded_for: bool,
    /// Same clock offset as the account settings (0 in production).
    pub time_offset: Arc<AtomicI64>,
}

impl LimitState {
    fn now(&self) -> u64 {
        let real = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        real.saturating_add_signed(self.time_offset.load(Ordering::Relaxed))
    }
}

/// Entries kept before old windows are purged.
const MAX_TRACKED: usize = 10_000;

#[derive(Debug)]
pub struct RateLimiter {
    max: u32,
    window_secs: u64,
    windows: Mutex<HashMap<IpAddr, (u64, u32)>>,
}

impl RateLimiter {
    pub fn new(max: u32, window_secs: u64) -> Self {
        Self { max, window_secs: window_secs.max(1), windows: Mutex::new(HashMap::new()) }
    }

    /// `Ok` when the request may proceed, `Err(seconds)` with the time until the window ends.
    pub fn check(&self, address: IpAddr, now: u64) -> Result<(), u64> {
        let mut windows = self.windows.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if windows.len() >= MAX_TRACKED {
            let window = self.window_secs;
            windows.retain(|_, (start, _)| now < start.saturating_add(window));
        }
        let entry = windows.entry(address).or_insert((now, 0));
        if now >= entry.0.saturating_add(self.window_secs) {
            *entry = (now, 0);
        }
        if entry.1 >= self.max {
            return Err(entry.0.saturating_add(self.window_secs).saturating_sub(now).max(1));
        }
        entry.1 += 1;
        Ok(())
    }

    pub fn tracked(&self) -> usize {
        self.windows.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).len()
    }
}

/// The caller's address: the first `X-Forwarded-For` entry when the server sits behind a
/// trusted reverse proxy, otherwise the peer address of the connection.
pub fn client_address(request: &Request, trust_forwarded_for: bool) -> Option<IpAddr> {
    if trust_forwarded_for {
        if let Some(address) = request.headers().get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .and_then(|value| value.trim().parse().ok()) {
            return Some(address);
        }
    }
    request.extensions().get::<ConnectInfo<SocketAddr>>().map(|info| info.0.ip())
}

pub async fn limit_anonymous(State(state): State<LimitState>, request: Request, next: Next) -> Response {
    if let Some(address) = client_address(&request, state.trust_forwarded_for) {
        if let Err(retry_after) = state.limiter.check(address, state.now()) {
            let mut response = (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({ "error": "too many requests, try again later" })),
            )
                .into_response();
            if let Ok(value) = HeaderValue::from_str(&retry_after.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
            return response;
        }
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([10, 0, 0, last])
    }

    #[test]
    fn allows_up_to_the_limit_then_blocks_until_the_window_ends() {
        let limiter = RateLimiter::new(3, 60);
        for _ in 0..3 {
            assert_eq!(limiter.check(ip(1), 1000), Ok(()));
        }
        assert_eq!(limiter.check(ip(1), 1000), Err(60));
        assert_eq!(limiter.check(ip(1), 1030), Err(30));
        assert_eq!(limiter.check(ip(1), 1059), Err(1));
        assert_eq!(limiter.check(ip(1), 1060), Ok(()), "a new window starts");
    }

    #[test]
    fn addresses_are_independent() {
        let limiter = RateLimiter::new(1, 60);
        assert_eq!(limiter.check(ip(1), 0), Ok(()));
        assert!(limiter.check(ip(1), 0).is_err());
        assert_eq!(limiter.check(ip(2), 0), Ok(()));
    }

    #[test]
    fn old_windows_are_purged_when_the_table_is_full() {
        let limiter = RateLimiter::new(1, 10);
        for n in 0..MAX_TRACKED as u32 {
            let [a, b, c, d] = n.to_be_bytes();
            assert_eq!(limiter.check(IpAddr::from([a | 1, b, c, d]), 0), Ok(()));
        }
        assert!(limiter.tracked() >= MAX_TRACKED - 1);
        assert_eq!(limiter.check(ip(250), 100), Ok(()));
        assert!(limiter.tracked() < 10, "expired windows are dropped: {}", limiter.tracked());
    }

    #[test]
    fn forwarded_for_is_used_only_when_trusted() {
        let request = Request::builder().header("x-forwarded-for", "203.0.113.7, 10.0.0.1").body(axum::body::Body::empty()).unwrap();
        assert_eq!(client_address(&request, true), Some("203.0.113.7".parse().unwrap()));
        assert_eq!(client_address(&request, false), None, "ignored without a trusted proxy");
        let bad = Request::builder().header("x-forwarded-for", "not-an-ip").body(axum::body::Body::empty()).unwrap();
        assert_eq!(client_address(&bad, true), None);
    }

    #[test]
    fn missing_or_invalid_forwarded_address_falls_back_to_peer() {
        let peer: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        for forwarded in [None, Some("not-an-ip"), Some("")] {
            let mut request = Request::builder();
            if let Some(value) = forwarded {
                request = request.header("x-forwarded-for", value);
            }
            let mut request = request.body(axum::body::Body::empty()).unwrap();
            request.extensions_mut().insert(ConnectInfo(peer));
            assert_eq!(client_address(&request, true), Some(peer.ip()));
        }
    }
}
