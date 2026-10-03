// SPDX-License-Identifier: AGPL-3.0-only
//! Account and session behaviour against a real PostgreSQL server.
//! Set ZHD_TEST_DATABASE_URL (CI provides one); without it the tests are skipped.
//! Every test works in its own schema, so each starts with an empty database.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;
use zherodizk_control::{connect_and_migrate_in_schema, router, AppState, AuthSettings};

const PASSWORD: &str = "correct horse battery staple";

struct Harness {
    app: Router,
    pool: sqlx::PgPool,
}

async fn harness(settings: AuthSettings) -> Option<Harness> {
    let url = std::env::var("ZHD_TEST_DATABASE_URL").ok()?;
    let schema = format!("t_{}", Uuid::new_v4().simple());
    let pool = connect_and_migrate_in_schema(&url, &schema).await.expect("schema and migrations");
    let app = router(AppState { pool: pool.clone(), settings });
    Some(Harness { app, pool })
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
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn register(app: &Router, email: &str, password: &str) -> (StatusCode, Value) {
    call(app, "POST", "/v1/auth/register", None, Some(json!({ "email": email, "password": password }))).await
}

async fn login(app: &Router, email: &str, password: &str) -> (StatusCode, Value) {
    call(app, "POST", "/v1/auth/login", None, Some(json!({ "email": email, "password": password }))).await
}

#[tokio::test]
async fn first_account_then_registration_is_closed() {
    let Some(h) = harness(AuthSettings::default()).await else { return };
    let (status, body) = register(&h.app, "Admin@Example.org", PASSWORD).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["email"], "admin@example.org");
    let (status, body) = register(&h.app, "second@example.org", PASSWORD).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
async fn open_registration_allows_more_accounts_but_not_duplicates() {
    let settings = AuthSettings { allow_registration: true, ..AuthSettings::default() };
    let Some(h) = harness(settings).await else { return };
    assert_eq!(register(&h.app, "a@example.org", PASSWORD).await.0, StatusCode::CREATED);
    assert_eq!(register(&h.app, "b@example.org", PASSWORD).await.0, StatusCode::CREATED);
    assert_eq!(register(&h.app, "A@example.org ", PASSWORD).await.0, StatusCode::CONFLICT);
}

#[tokio::test]
async fn weak_passwords_and_bad_emails_are_rejected() {
    let Some(h) = harness(AuthSettings::default()).await else { return };
    assert_eq!(register(&h.app, "a@example.org", "short").await.0, StatusCode::BAD_REQUEST);
    assert_eq!(register(&h.app, "not-an-email", PASSWORD).await.0, StatusCode::BAD_REQUEST);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users").fetch_one(&h.pool).await.unwrap();
    assert_eq!(count, 0, "rejected requests must not create accounts");
}

#[tokio::test]
async fn password_is_stored_as_argon2id_only() {
    let Some(h) = harness(AuthSettings::default()).await else { return };
    register(&h.app, "a@example.org", PASSWORD).await;
    let stored: String = sqlx::query_scalar("SELECT password_hash FROM users").fetch_one(&h.pool).await.unwrap();
    assert!(stored.starts_with("$argon2id$"));
    assert!(!stored.contains(PASSWORD));
}

#[tokio::test]
async fn login_me_logout_cycle() {
    let Some(h) = harness(AuthSettings::default()).await else { return };
    register(&h.app, "a@example.org", PASSWORD).await;
    let (status, body) = login(&h.app, "A@example.org", PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
    let token = body["token"].as_str().unwrap().to_owned();
    assert!(token.len() >= 43);

    let (status, me) = call(&h.app, "GET", "/v1/auth/me", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["email"], "a@example.org");

    // Only the digest of the token is stored.
    let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM sessions").fetch_one(&h.pool).await.unwrap();
    assert_eq!(stored.len(), 32);
    assert_ne!(stored, token.as_bytes());

    assert_eq!(call(&h.app, "POST", "/v1/auth/logout", Some(&token), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(call(&h.app, "GET", "/v1/auth/me", Some(&token), None).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn wrong_password_and_unknown_user_look_the_same() {
    let Some(h) = harness(AuthSettings::default()).await else { return };
    register(&h.app, "a@example.org", PASSWORD).await;
    let wrong = login(&h.app, "a@example.org", "definitely not the password").await;
    let unknown = login(&h.app, "nobody@example.org", "definitely not the password").await;
    let malformed = login(&h.app, "???", "definitely not the password").await;
    assert_eq!(wrong.0, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong, unknown);
    assert_eq!(wrong, malformed);
}

#[tokio::test]
async fn account_locks_after_repeated_failures_even_for_the_right_password() {
    let settings = AuthSettings { max_failed_attempts: 3, ..AuthSettings::default() };
    let Some(h) = harness(settings).await else { return };
    register(&h.app, "a@example.org", PASSWORD).await;
    for _ in 0..3 {
        assert_eq!(login(&h.app, "a@example.org", "wrong wrong wrong").await.0, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(login(&h.app, "a@example.org", PASSWORD).await.0, StatusCode::UNAUTHORIZED);
    // Lockout is time-limited: move it into the past and the account works again.
    sqlx::query("UPDATE users SET locked_until = now() - interval '1 second'").execute(&h.pool).await.unwrap();
    assert_eq!(login(&h.app, "a@example.org", PASSWORD).await.0, StatusCode::OK);
    let failed: i32 = sqlx::query_scalar("SELECT failed_attempts FROM users").fetch_one(&h.pool).await.unwrap();
    assert_eq!(failed, 0, "a successful login resets the counter");
}

#[tokio::test]
async fn expired_revoked_and_garbage_tokens_are_rejected() {
    let Some(h) = harness(AuthSettings::default()).await else { return };
    register(&h.app, "a@example.org", PASSWORD).await;
    let token = login(&h.app, "a@example.org", PASSWORD).await.1["token"].as_str().unwrap().to_owned();
    assert_eq!(call(&h.app, "GET", "/v1/auth/me", Some(&token), None).await.0, StatusCode::OK);
    assert_eq!(call(&h.app, "GET", "/v1/auth/me", Some("garbage"), None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(call(&h.app, "GET", "/v1/auth/me", None, None).await.0, StatusCode::UNAUTHORIZED);
    sqlx::query("UPDATE sessions SET expires_at = now() - interval '1 second'").execute(&h.pool).await.unwrap();
    assert_eq!(call(&h.app, "GET", "/v1/auth/me", Some(&token), None).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn deactivated_user_loses_access_immediately() {
    let Some(h) = harness(AuthSettings::default()).await else { return };
    register(&h.app, "a@example.org", PASSWORD).await;
    let token = login(&h.app, "a@example.org", PASSWORD).await.1["token"].as_str().unwrap().to_owned();
    sqlx::query("UPDATE users SET is_active = false").execute(&h.pool).await.unwrap();
    assert_eq!(call(&h.app, "GET", "/v1/auth/me", Some(&token), None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(login(&h.app, "a@example.org", PASSWORD).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn concurrent_first_registrations_create_exactly_one_account() {
    let Some(h) = harness(AuthSettings::default()).await else { return };
    let mut tasks = Vec::new();
    for index in 0..4 {
        let app = h.app.clone();
        tasks.push(tokio::spawn(async move {
            register(&app, &format!("user{index}@example.org"), PASSWORD).await.0
        }));
    }
    let mut created = 0;
    for task in tasks {
        if task.await.unwrap() == StatusCode::CREATED {
            created += 1;
        }
    }
    assert_eq!(created, 1);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users").fetch_one(&h.pool).await.unwrap();
    assert_eq!(count, 1);
}
