// SPDX-License-Identifier: AGPL-3.0-only
//! Request limits on anonymous endpoints. Set ZHD_TEST_DATABASE_URL (CI provides one).

use std::net::SocketAddr;
use std::sync::atomic::Ordering;

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;
use zherodizk_control::{connect_and_migrate_in_schema, router, AppState, AuthSettings};

struct Harness {
    app: Router,
    offset: std::sync::Arc<std::sync::atomic::AtomicI64>,
}

async fn harness(limit: u32, trust_forwarded_for: bool) -> Option<Harness> {
    let url = std::env::var("ZHD_TEST_DATABASE_URL").ok()?;
    let schema = format!("t_{}", Uuid::new_v4().simple());
    let pool = connect_and_migrate_in_schema(&url, &schema).await.expect("schema and migrations");
    let settings = AuthSettings { auth_rate_limit: limit, trust_forwarded_for, ..AuthSettings::default() };
    let offset = settings.time_offset.clone();
    Some(Harness { app: router(AppState { pool, settings }), offset })
}

/// A login attempt as seen from `peer` (and optionally with a forwarded address).
async fn attempt(h: &Harness, uri: &str, peer: Option<&str>, forwarded: Option<&str>) -> (StatusCode, Option<String>) {
    let mut builder = Request::builder().method("POST").uri(uri).header("content-type", "application/json");
    if let Some(value) = forwarded {
        builder = builder.header("x-forwarded-for", value);
    }
    let mut request = builder
        .body(Body::from(json!({ "email": "nobody@example.org", "password": "wrong password here" }).to_string()))
        .unwrap();
    if let Some(peer) = peer {
        let address: SocketAddr = format!("{peer}:40000").parse().unwrap();
        request.extensions_mut().insert(ConnectInfo(address));
    }
    let response = h.app.clone().oneshot(request).await.unwrap();
    let retry = response.headers().get("retry-after").map(|v| v.to_str().unwrap().to_owned());
    let status = response.status();
    let _ = response.into_body().collect().await.unwrap();
    (status, retry)
}

#[tokio::test]
async fn anonymous_endpoints_are_limited_per_address() {
    let Some(h) = harness(3, false).await else { return };
    for _ in 0..3 {
        assert_eq!(attempt(&h, "/v1/auth/login", Some("198.51.100.1"), None).await.0, StatusCode::UNAUTHORIZED);
    }
    let (status, retry) = attempt(&h, "/v1/auth/login", Some("198.51.100.1"), None).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(retry.unwrap().parse::<u64>().unwrap() >= 1);
    // The limit is shared by all anonymous endpoints of one address, but not across addresses.
    assert_eq!(attempt(&h, "/v1/auth/register", Some("198.51.100.1"), None).await.0, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(attempt(&h, "/v1/devices/enroll", Some("198.51.100.1"), None).await.0, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(attempt(&h, "/v1/auth/login", Some("198.51.100.2"), None).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_window_reopens_after_a_minute() {
    let Some(h) = harness(1, false).await else { return };
    assert_eq!(attempt(&h, "/v1/auth/login", Some("198.51.100.1"), None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(attempt(&h, "/v1/auth/login", Some("198.51.100.1"), None).await.0, StatusCode::TOO_MANY_REQUESTS);
    h.offset.store(61, Ordering::Relaxed);
    assert_eq!(attempt(&h, "/v1/auth/login", Some("198.51.100.1"), None).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn other_routes_and_unknown_addresses_are_not_limited() {
    let Some(h) = harness(1, false).await else { return };
    // No peer information at all (only possible in tests): not limited.
    for _ in 0..5 {
        assert_eq!(attempt(&h, "/v1/auth/login", None, None).await.0, StatusCode::UNAUTHORIZED);
    }
    // Health checks and authenticated routes are outside the anonymous limit.
    for _ in 0..5 {
        let mut request = Request::builder().uri("/healthz").body(Body::empty()).unwrap();
        request.extensions_mut().insert(ConnectInfo("198.51.100.9:1".parse::<SocketAddr>().unwrap()));
        assert_eq!(h.app.clone().oneshot(request).await.unwrap().status(), StatusCode::OK);
        let mut request = Request::builder().uri("/v1/auth/me").body(Body::empty()).unwrap();
        request.extensions_mut().insert(ConnectInfo("198.51.100.9:1".parse::<SocketAddr>().unwrap()));
        assert_eq!(h.app.clone().oneshot(request).await.unwrap().status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn forwarded_for_is_honoured_only_when_trusted() {
    let Some(trusting) = harness(2, true).await else { return };
    for _ in 0..2 {
        attempt(&trusting, "/v1/auth/login", Some("10.0.0.1"), Some("203.0.113.7")).await;
    }
    assert_eq!(attempt(&trusting, "/v1/auth/login", Some("10.0.0.1"), Some("203.0.113.7")).await.0, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(attempt(&trusting, "/v1/auth/login", Some("10.0.0.1"), Some("203.0.113.8")).await.0, StatusCode::UNAUTHORIZED);

    // Without trust the header is ignored: a caller cannot dodge the limit by changing it.
    let Some(plain) = harness(2, false).await else { return };
    for _ in 0..2 {
        attempt(&plain, "/v1/auth/login", Some("10.0.0.1"), Some("203.0.113.7")).await;
    }
    assert_eq!(attempt(&plain, "/v1/auth/login", Some("10.0.0.1"), Some("203.0.113.99")).await.0, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn a_limit_of_zero_turns_it_off() {
    let Some(h) = harness(0, false).await else { return };
    for _ in 0..40 {
        assert_eq!(attempt(&h, "/v1/auth/login", Some("198.51.100.1"), None).await.0, StatusCode::UNAUTHORIZED);
    }
}
