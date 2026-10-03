// SPDX-License-Identifier: AGPL-3.0-only
//! Audit log behaviour and the API description. Set ZHD_TEST_DATABASE_URL (CI provides one).

use std::collections::BTreeSet;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signer, SigningKey};
use http_body_util::BodyExt;
use rand::{rngs::OsRng, RngCore};
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;
use zherodizk_control::{
    connect_and_migrate_in_schema, devices::enrollment_message, router, AppState, AuthSettings, API_ROUTES,
};

const PASSWORD: &str = "correct horse battery staple";
const OPENAPI: &str = include_str!("../../../docs/api/openapi.yaml");

/// (METHOD, path) pairs described in the OpenAPI file, read from its fixed layout.
fn documented_routes() -> BTreeSet<(String, String)> {
    let mut found = BTreeSet::new();
    let mut in_paths = false;
    let mut current: Option<String> = None;
    for line in OPENAPI.lines() {
        if line == "paths:" {
            in_paths = true;
            continue;
        }
        if in_paths && !line.starts_with(' ') && !line.is_empty() {
            break;
        }
        if !in_paths {
            continue;
        }
        if let Some(path) = line.strip_prefix("  /").and_then(|rest| rest.strip_suffix(':')) {
            current = Some(format!("/{path}"));
        } else if let Some(method) = line.strip_prefix("    ").and_then(|rest| rest.strip_suffix(':')) {
            if ["get", "post", "put", "delete"].contains(&method) {
                found.insert((method.to_uppercase(), current.clone().expect("method before path")));
            }
        }
    }
    found
}

#[test]
fn openapi_describes_exactly_the_routes_the_server_has() {
    let declared: BTreeSet<(String, String)> =
        API_ROUTES.iter().map(|(m, p)| (m.to_string(), p.to_string())).collect();
    assert_eq!(documented_routes(), declared);
}

struct Harness {
    app: Router,
    pool: sqlx::PgPool,
}

async fn harness() -> Option<Harness> {
    let url = std::env::var("ZHD_TEST_DATABASE_URL").ok()?;
    let schema = format!("t_{}", Uuid::new_v4().simple());
    let pool = connect_and_migrate_in_schema(&url, &schema).await.expect("schema and migrations");
    let settings = AuthSettings { allow_registration: true, max_failed_attempts: 50, ..AuthSettings::default() };
    Some(Harness { app: router(AppState { pool: pool.clone(), settings }), pool })
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

#[tokio::test]
async fn every_declared_route_is_registered_with_its_method() {
    let Some(h) = harness().await else { return };
    let id = Uuid::new_v4();
    for (method, path) in API_ROUTES {
        let uri = path.replace("{org}", &id.to_string()).replace("{device}", &id.to_string())
            .replace("{group}", &id.to_string()).replace("{user}", &id.to_string());
        let body = (*method != "GET" && *method != "DELETE").then(|| json!({}));
        let (status, _) = call(&h.app, method, &uri, None, body).await;
        assert_ne!(status, StatusCode::NOT_FOUND, "{method} {path} is not routed");
        assert_ne!(status, StatusCode::METHOD_NOT_ALLOWED, "{method} {path} has the wrong method");
    }
}

async fn token_for(h: &Harness, email: &str) -> (String, String) {
    let (_, reg) = call(&h.app, "POST", "/v1/auth/register", None, Some(json!({ "email": email, "password": PASSWORD }))).await;
    let (_, login) = call(&h.app, "POST", "/v1/auth/login", None, Some(json!({ "email": email, "password": PASSWORD }))).await;
    (login["token"].as_str().unwrap().to_owned(), reg["id"].as_str().unwrap().to_owned())
}

async fn events(h: &Harness, token: &str, org: &str, query: &str) -> Value {
    let (status, body) = call(&h.app, "GET", &format!("/v1/orgs/{org}/audit{query}"), Some(token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

fn actions(body: &Value) -> Vec<String> {
    body["events"].as_array().unwrap().iter().map(|e| e["action"].as_str().unwrap().to_owned()).collect()
}

#[tokio::test]
async fn actions_are_recorded_without_secrets() {
    let Some(h) = harness().await else { return };
    let (owner, owner_id) = token_for(&h, "owner@example.org").await;
    let (_member, _) = token_for(&h, "member@example.org").await;
    let org = call(&h.app, "POST", "/v1/orgs", Some(&owner), Some(json!({ "name": "Team" }))).await.1["id"].as_str().unwrap().to_owned();
    call(&h.app, "POST", &format!("/v1/orgs/{org}/members"), Some(&owner), Some(json!({ "email": "member@example.org", "role": "member" }))).await;
    let token = call(&h.app, "POST", &format!("/v1/orgs/{org}/enrollment-tokens"), Some(&owner), None).await.1["token"].as_str().unwrap().to_owned();

    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    let key = SigningKey::from_bytes(&seed);
    let public_key = STANDARD.encode(key.verifying_key().to_bytes());
    let signature = STANDARD.encode(key.sign(&enrollment_message(&token, "pc", &public_key)).to_bytes());
    let device = call(
        &h.app,
        "POST",
        "/v1/devices/enroll",
        None,
        Some(json!({ "token": token, "name": "pc", "platform": "linux", "public_key": public_key, "signature": signature })),
    )
    .await
    .1["device_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let group = call(&h.app, "POST", &format!("/v1/orgs/{org}/groups"), Some(&owner), Some(json!({ "name": "All" }))).await.1["id"].as_str().unwrap().to_owned();
    call(&h.app, "POST", &format!("/v1/orgs/{org}/groups/{group}/devices"), Some(&owner), Some(json!({ "device_id": device }))).await;
    call(&h.app, "PUT", &format!("/v1/orgs/{org}/acl"), Some(&owner), Some(json!({ "user_id": owner_id, "group_id": group, "capabilities": ["view"] }))).await;
    call(&h.app, "POST", &format!("/v1/orgs/{org}/devices/{device}/revoke"), Some(&owner), None).await;

    let log = events(&h, &owner, &org, "").await;
    let seen = actions(&log);
    for expected in [
        "org.created", "member.added", "enrollment_token.created", "device.enrolled",
        "group.created", "group.device_added", "acl.set", "device.revoked",
    ] {
        assert!(seen.contains(&expected.to_owned()), "missing {expected} in {seen:?}");
    }
    // Newest first, and nothing sensitive anywhere in the stored text.
    assert_eq!(seen[0], "device.revoked");
    let dump = log.to_string();
    for secret in [token.as_str(), PASSWORD, public_key.as_str(), signature.as_str()] {
        assert!(!dump.contains(secret), "audit log must not contain {secret}");
    }
    let stored: String = sqlx::query_scalar("SELECT string_agg(detail::text || coalesce(target, ''), ' ') FROM audit_events")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert!(!stored.contains(&token) && !stored.contains(PASSWORD));

    // Account events (no organisation) are stored too: registrations, logins, failures.
    call(&h.app, "POST", "/v1/auth/login", None, Some(json!({ "email": "owner@example.org", "password": "wrong wrong wrong" }))).await;
    let account_actions: Vec<String> = sqlx::query_scalar("SELECT action FROM audit_events WHERE org_id IS NULL ORDER BY id")
        .fetch_all(&h.pool)
        .await
        .unwrap();
    for expected in ["user.registered", "auth.login", "auth.login_failed"] {
        assert!(account_actions.contains(&expected.to_owned()), "missing {expected}");
    }
}

#[tokio::test]
async fn only_admins_of_the_organisation_can_read_its_log() {
    let Some(h) = harness().await else { return };
    let (owner, _) = token_for(&h, "owner@example.org").await;
    let (member, _) = token_for(&h, "member@example.org").await;
    let (outsider, _) = token_for(&h, "outsider@example.org").await;
    let org = call(&h.app, "POST", "/v1/orgs", Some(&owner), Some(json!({ "name": "Team" }))).await.1["id"].as_str().unwrap().to_owned();
    call(&h.app, "POST", &format!("/v1/orgs/{org}/members"), Some(&owner), Some(json!({ "email": "member@example.org", "role": "member" }))).await;
    let path = format!("/v1/orgs/{org}/audit");
    assert_eq!(call(&h.app, "GET", &path, Some(&member), None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(call(&h.app, "GET", &path, Some(&outsider), None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(call(&h.app, "GET", &path, None, None).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn entries_of_one_organisation_never_appear_in_another() {
    let Some(h) = harness().await else { return };
    let (alice, _) = token_for(&h, "alice@example.org").await;
    let (bob, _) = token_for(&h, "bob@example.org").await;
    let org_a = call(&h.app, "POST", "/v1/orgs", Some(&alice), Some(json!({ "name": "Alice secret project" }))).await.1["id"].as_str().unwrap().to_owned();
    let org_b = call(&h.app, "POST", "/v1/orgs", Some(&bob), Some(json!({ "name": "B" }))).await.1["id"].as_str().unwrap().to_owned();
    let log_b = events(&h, &bob, &org_b, "").await;
    assert_eq!(actions(&log_b), vec!["org.created"]);
    assert!(!log_b.to_string().contains("Alice secret project"));
    assert_eq!(call(&h.app, "GET", &format!("/v1/orgs/{org_a}/audit"), Some(&bob), None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn paging_uses_a_cursor() {
    let Some(h) = harness().await else { return };
    let (owner, _) = token_for(&h, "owner@example.org").await;
    let org = call(&h.app, "POST", "/v1/orgs", Some(&owner), Some(json!({ "name": "Team" }))).await.1["id"].as_str().unwrap().to_owned();
    for index in 0..4 {
        call(&h.app, "POST", &format!("/v1/orgs/{org}/groups"), Some(&owner), Some(json!({ "name": format!("g{index}") }))).await;
    }
    let first = events(&h, &owner, &org, "?limit=2").await;
    assert_eq!(first["events"].as_array().unwrap().len(), 2);
    let cursor = first["next_before"].as_i64().expect("a full page has a cursor");
    let second = events(&h, &owner, &org, &format!("?limit=2&before={cursor}")).await;
    let ids: Vec<i64> = first["events"].as_array().unwrap().iter().chain(second["events"].as_array().unwrap()).map(|e| e["id"].as_i64().unwrap()).collect();
    assert_eq!(ids.len(), 4);
    assert!(ids.windows(2).all(|w| w[0] > w[1]), "strictly newest first with no repeats: {ids:?}");
    let rest = events(&h, &owner, &org, "?limit=200").await;
    assert_eq!(rest["events"].as_array().unwrap().len(), 5, "organisation + four groups");
    assert!(rest["next_before"].is_null());
}

#[tokio::test]
async fn the_log_is_append_only_in_the_database() {
    let Some(h) = harness().await else { return };
    let (owner, _) = token_for(&h, "owner@example.org").await;
    call(&h.app, "POST", "/v1/orgs", Some(&owner), Some(json!({ "name": "Team" }))).await;
    assert!(sqlx::query("UPDATE audit_events SET action = 'x'").execute(&h.pool).await.is_err());
    assert!(sqlx::query("DELETE FROM audit_events").execute(&h.pool).await.is_err());
    assert!(sqlx::query("TRUNCATE audit_events").execute(&h.pool).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events").fetch_one(&h.pool).await.unwrap();
    assert!(count >= 2);
}
