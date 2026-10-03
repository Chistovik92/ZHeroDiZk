// SPDX-License-Identifier: AGPL-3.0-only
//! TOTP multi-factor authentication against a real PostgreSQL server.
//! Set ZHD_TEST_DATABASE_URL (CI provides one); without it the tests are skipped.

use std::sync::{atomic::Ordering, Arc};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use data_encoding::BASE32_NOPAD;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;
use zherodizk_control::{connect_and_migrate_in_schema, router, totp, AppState, AuthSettings};

const PASSWORD: &str = "correct horse battery staple";
const EMAIL: &str = "admin@example.org";

struct Harness {
    app: Router,
    pool: sqlx::PgPool,
    offset: Arc<std::sync::atomic::AtomicI64>,
}

async fn harness(mut settings: AuthSettings, with_key: bool) -> Option<Harness> {
    let url = std::env::var("ZHD_TEST_DATABASE_URL").ok()?;
    if with_key {
        settings.mfa_key = Some([9u8; 32]);
    }
    let offset = settings.time_offset.clone();
    let schema = format!("t_{}", Uuid::new_v4().simple());
    let pool = connect_and_migrate_in_schema(&url, &schema).await.expect("schema and migrations");
    let app = router(AppState { pool: pool.clone(), settings });
    Some(Harness { app, pool, offset })
}

async fn call(app: &Router, method: &str, uri: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let request = builder
        .body(body.map(|v| Body::from(v.to_string())).unwrap_or_else(Body::empty))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// The code an authenticator app would show `skew_secs` seconds after the server's "now".
fn code_for(h: &Harness, secret: &[u8], skew_secs: u64) -> String {
    let real = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let server_now = real.saturating_add_signed(h.offset.load(Ordering::Relaxed));
    totp::code_at(secret, server_now + skew_secs)
}

async fn password_login(h: &Harness) -> (StatusCode, Value) {
    call(&h.app, "POST", "/v1/auth/login", None, Some(json!({ "email": EMAIL, "password": PASSWORD }))).await
}

async fn second_step(h: &Harness, mfa_token: &str, field: &str, value: &str) -> (StatusCode, Value) {
    call(&h.app, "POST", "/v1/auth/login/mfa", None, Some(json!({ "mfa_token": mfa_token, field: value }))).await
}

/// Register, log in and enable MFA; returns (secret, recovery codes, full token).
async fn enrolled(h: &Harness) -> (Vec<u8>, Vec<String>, String) {
    call(&h.app, "POST", "/v1/auth/register", None, Some(json!({ "email": EMAIL, "password": PASSWORD }))).await;
    let token = password_login(h).await.1["token"].as_str().unwrap().to_owned();
    let (status, enroll) = call(&h.app, "POST", "/v1/auth/mfa/enroll", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{enroll}");
    let secret = BASE32_NOPAD.decode(enroll["secret"].as_str().unwrap().as_bytes()).unwrap();
    let (status, confirmed) = call(
        &h.app,
        "POST",
        "/v1/auth/mfa/confirm",
        Some(&token),
        Some(json!({ "code": code_for(h, &secret, 0) })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{confirmed}");
    let codes = confirmed["recovery_codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_owned())
        .collect();
    (secret, codes, token)
}

#[tokio::test]
async fn endpoints_answer_503_when_no_key_is_configured() {
    let Some(h) = harness(AuthSettings::default(), false).await else { return };
    call(&h.app, "POST", "/v1/auth/register", None, Some(json!({ "email": EMAIL, "password": PASSWORD }))).await;
    let token = password_login(&h).await.1["token"].as_str().unwrap().to_owned();
    let (status, _) = call(&h.app, "POST", "/v1/auth/mfa/enroll", Some(&token), None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn enrolment_secret_is_encrypted_and_wrong_code_is_refused() {
    let Some(h) = harness(AuthSettings::default(), true).await else { return };
    call(&h.app, "POST", "/v1/auth/register", None, Some(json!({ "email": EMAIL, "password": PASSWORD }))).await;
    let token = password_login(&h).await.1["token"].as_str().unwrap().to_owned();
    let (_, enroll) = call(&h.app, "POST", "/v1/auth/mfa/enroll", Some(&token), None).await;
    let secret = BASE32_NOPAD.decode(enroll["secret"].as_str().unwrap().as_bytes()).unwrap();
    assert!(enroll["otpauth_uri"].as_str().unwrap().starts_with("otpauth://totp/ZHeroDiZk:admin%40example.org?secret="));

    let stored: Vec<u8> = sqlx::query_scalar("SELECT mfa_secret_enc FROM users").fetch_one(&h.pool).await.unwrap();
    assert!(!stored.windows(secret.len()).any(|w| w == secret.as_slice()), "secret must not be stored in the clear");
    let enabled: bool = sqlx::query_scalar("SELECT mfa_enabled FROM users").fetch_one(&h.pool).await.unwrap();
    assert!(!enabled, "enrolment is not active before a code is confirmed");

    let (status, _) = call(&h.app, "POST", "/v1/auth/mfa/confirm", Some(&token), Some(json!({ "code": "000000" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // Without MFA enabled the password alone still gives a full session.
    assert!(password_login(&h).await.1["token"].is_string());
}

#[tokio::test]
async fn second_factor_login_replay_and_recovery_codes() {
    let Some(h) = harness(AuthSettings::default(), true).await else { return };
    let (secret, codes, _token) = enrolled(&h).await;
    assert_eq!(codes.len(), 10);

    let (status, first) = password_login(&h).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["mfa_required"], true);
    assert!(first.get("token").is_none(), "no full session before the second factor");
    let pending = first["mfa_token"].as_str().unwrap().to_owned();
    assert_eq!(call(&h.app, "GET", "/v1/auth/me", Some(&pending), None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(call(&h.app, "POST", "/v1/auth/mfa/enroll", Some(&pending), None).await.0, StatusCode::UNAUTHORIZED);

    assert_eq!(second_step(&h, &pending, "code", "000000").await.0, StatusCode::UNAUTHORIZED);

    // The enrolment code used step S; the next period's code is a fresh step.
    h.offset.store(30, Ordering::Relaxed);
    let code = code_for(&h, &secret, 0);
    let (status, done) = second_step(&h, &pending, "code", &code).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    let full = done["token"].as_str().unwrap().to_owned();
    assert_eq!(call(&h.app, "GET", "/v1/auth/me", Some(&full), None).await.0, StatusCode::OK);
    assert_eq!(second_step(&h, &pending, "code", &code).await.0, StatusCode::UNAUTHORIZED, "pending session is single use");

    // The same code must not work again, even with a fresh pending session.
    let again = password_login(&h).await.1["mfa_token"].as_str().unwrap().to_owned();
    assert_eq!(second_step(&h, &again, "code", &code).await.0, StatusCode::UNAUTHORIZED, "replay");

    // A recovery code works once.
    let (status, _) = second_step(&h, &again, "recovery_code", &codes[0]).await;
    assert_eq!(status, StatusCode::OK);
    let third = password_login(&h).await.1["mfa_token"].as_str().unwrap().to_owned();
    assert_eq!(second_step(&h, &third, "recovery_code", &codes[0]).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(second_step(&h, &third, "recovery_code", &codes[1].to_uppercase()).await.0, StatusCode::OK);
}

#[tokio::test]
async fn sending_both_or_neither_factor_is_a_bad_request() {
    let Some(h) = harness(AuthSettings::default(), true).await else { return };
    enrolled(&h).await;
    let pending = password_login(&h).await.1["mfa_token"].as_str().unwrap().to_owned();
    let (status, _) = call(&h.app, "POST", "/v1/auth/login/mfa", None, Some(json!({ "mfa_token": pending }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        &h.app,
        "POST",
        "/v1/auth/login/mfa",
        None,
        Some(json!({ "mfa_token": pending, "code": "123456", "recovery_code": "abcde-23456" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn wrong_second_factors_lock_the_account() {
    let settings = AuthSettings { max_failed_attempts: 3, ..AuthSettings::default() };
    let Some(h) = harness(settings, true).await else { return };
    let (secret, _codes, _token) = enrolled(&h).await;
    let pending = password_login(&h).await.1["mfa_token"].as_str().unwrap().to_owned();
    for _ in 0..3 {
        assert_eq!(second_step(&h, &pending, "code", "000000").await.0, StatusCode::UNAUTHORIZED);
    }
    h.offset.store(30, Ordering::Relaxed);
    let good = code_for(&h, &secret, 0);
    assert_eq!(second_step(&h, &pending, "code", &good).await.0, StatusCode::UNAUTHORIZED, "locked");
    assert_eq!(password_login(&h).await.0, StatusCode::UNAUTHORIZED, "password login is locked too");
}

#[tokio::test]
async fn disabling_needs_password_and_a_fresh_code() {
    let Some(h) = harness(AuthSettings::default(), true).await else { return };
    let (secret, _codes, _token) = enrolled(&h).await;
    h.offset.store(30, Ordering::Relaxed);
    let pending = password_login(&h).await.1["mfa_token"].as_str().unwrap().to_owned();
    let (status, done) = second_step(&h, &pending, "code", &code_for(&h, &secret, 0)).await;
    assert_eq!(status, StatusCode::OK);
    let full = done["token"].as_str().unwrap().to_owned();

    h.offset.store(60, Ordering::Relaxed);
    let wrong_password = json!({ "password": "wrong wrong wrong", "code": code_for(&h, &secret, 0) });
    assert_eq!(call(&h.app, "POST", "/v1/auth/mfa/disable", Some(&full), Some(wrong_password)).await.0, StatusCode::UNAUTHORIZED);

    h.offset.store(90, Ordering::Relaxed);
    let right = json!({ "password": PASSWORD, "code": code_for(&h, &secret, 0) });
    assert_eq!(call(&h.app, "POST", "/v1/auth/mfa/disable", Some(&full), Some(right)).await.0, StatusCode::NO_CONTENT);
    assert!(password_login(&h).await.1["token"].is_string(), "password alone works again");
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM recovery_codes").fetch_one(&h.pool).await.unwrap();
    assert_eq!(left, 0);
}
