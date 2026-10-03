// SPDX-License-Identifier: AGPL-3.0-only
//! Endpoints the web panel relies on (0.7.7): member list, grant list, password change.
//! Set ZHD_TEST_DATABASE_URL (CI provides one); without it the tests are skipped.

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
const NEW_PASSWORD: &str = "another long passphrase 42";

async fn app() -> Option<Router> {
    let url = std::env::var("ZHD_TEST_DATABASE_URL").ok()?;
    let schema = format!("t_{}", Uuid::new_v4().simple());
    let pool = connect_and_migrate_in_schema(&url, &schema).await.expect("schema and migrations");
    let settings = AuthSettings {
        allow_registration: true,
        grant_key: Some([9u8; 32]),
        max_failed_attempts: 50,
        auth_rate_limit: 0,
        ..AuthSettings::default()
    };
    Some(router(AppState { pool, settings }))
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

async fn login(app: &Router, email: &str, password: &str) -> (StatusCode, String) {
    let (status, body) = call(app, "POST", "/v1/auth/login", None, Some(json!({ "email": email, "password": password }))).await;
    (status, body["token"].as_str().unwrap_or_default().to_owned())
}

async fn register_and_login(app: &Router, email: &str) -> String {
    let (status, _) = call(app, "POST", "/v1/auth/register", None, Some(json!({ "email": email, "password": PASSWORD }))).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, token) = login(app, email, PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
    token
}

#[tokio::test]
async fn members_are_listed_for_admins_only() {
    let Some(app) = app().await else { return };
    let owner = register_and_login(&app, "owner@example.org").await;
    let member = register_and_login(&app, "member@example.org").await;
    let org = call(&app, "POST", "/v1/orgs", Some(&owner), Some(json!({ "name": "Team" }))).await.1["id"].as_str().unwrap().to_owned();
    let (status, _) = call(&app, "POST", &format!("/v1/orgs/{org}/members"), Some(&owner), Some(json!({ "email": "member@example.org", "role": "member" }))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = call(&app, "GET", &format!("/v1/orgs/{org}/members"), Some(&owner), None).await;
    assert_eq!(status, StatusCode::OK);
    let list = body.as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0]["email"], "member@example.org");
    assert_eq!(list[1]["role"], "owner");

    let (status, _) = call(&app, "GET", &format!("/v1/orgs/{org}/members"), Some(&member), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let outsider = register_and_login_other(&app).await;
    let (status, _) = call(&app, "GET", &format!("/v1/orgs/{org}/members"), Some(&outsider), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&app, "GET", &format!("/v1/orgs/{org}/members"), None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

async fn register_and_login_other(app: &Router) -> String {
    register_and_login(app, "outsider@example.org").await
}

#[tokio::test]
async fn grants_list_is_empty_and_scoped() {
    let Some(app) = app().await else { return };
    let owner = register_and_login(&app, "owner@example.org").await;
    let org = call(&app, "POST", "/v1/orgs", Some(&owner), Some(json!({ "name": "Team" }))).await.1["id"].as_str().unwrap().to_owned();
    let (status, body) = call(&app, "GET", &format!("/v1/orgs/{org}/grants"), Some(&owner), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));
    let outsider = register_and_login(&app, "outsider@example.org").await;
    let (status, _) = call(&app, "GET", &format!("/v1/orgs/{org}/grants"), Some(&outsider), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn password_change_needs_the_current_password_and_ends_other_sessions() {
    let Some(app) = app().await else { return };
    let first = register_and_login(&app, "user@example.org").await;
    let (_, second) = login(&app, "user@example.org", PASSWORD).await;

    let (status, _) = call(&app, "POST", "/v1/auth/password", Some(&first), Some(json!({ "current_password": "wrong password!!", "new_password": NEW_PASSWORD }))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&app, "POST", "/v1/auth/password", Some(&first), Some(json!({ "current_password": PASSWORD, "new_password": "short" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(&app, "POST", "/v1/auth/password", None, Some(json!({ "current_password": PASSWORD, "new_password": NEW_PASSWORD }))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = call(&app, "POST", "/v1/auth/password", Some(&first), Some(json!({ "current_password": PASSWORD, "new_password": NEW_PASSWORD }))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // The session that changed the password stays valid; the other one ends.
    assert_eq!(call(&app, "GET", "/v1/auth/me", Some(&first), None).await.0, StatusCode::OK);
    assert_eq!(call(&app, "GET", "/v1/auth/me", Some(&second), None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(login(&app, "user@example.org", PASSWORD).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(login(&app, "user@example.org", NEW_PASSWORD).await.0, StatusCode::OK);
}
