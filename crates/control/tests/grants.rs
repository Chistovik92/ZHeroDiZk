// SPDX-License-Identifier: AGPL-3.0-only
//! Session grants end to end: the server issues a signed grant, and the device side
//! (`zherodizk-grant` + `zherodizk-access-policy`) verifies and authorises it.
//! Set ZHD_TEST_DATABASE_URL (CI provides one); without it the tests are skipped.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use http_body_util::BodyExt;
use rand::{rngs::OsRng, RngCore};
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;
use zherodizk_access_policy::Capability;
use zherodizk_control::{connect_and_migrate_in_schema, devices::enrollment_message, router, AppState, AuthSettings};
use zherodizk_grant::{authorize_connection, verify, ConnectionDenied, GrantError, LocalState, ReplayCache};

const PASSWORD: &str = "correct horse battery staple";

struct Harness {
    app: Router,
    pool: sqlx::PgPool,
}

async fn harness(with_key: bool) -> Option<Harness> {
    let url = std::env::var("ZHD_TEST_DATABASE_URL").ok()?;
    let schema = format!("t_{}", Uuid::new_v4().simple());
    let pool = connect_and_migrate_in_schema(&url, &schema).await.expect("schema and migrations");
    let settings = AuthSettings {
        allow_registration: true,
        grant_key: with_key.then_some([5u8; 32]),
        ..AuthSettings::default()
    };
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

struct Person {
    token: String,
    id: String,
    email: String,
}

async fn person(h: &Harness, email: &str) -> Person {
    let (_, body) = call(&h.app, "POST", "/v1/auth/register", None, Some(json!({ "email": email, "password": PASSWORD }))).await;
    let (_, login) = call(&h.app, "POST", "/v1/auth/login", None, Some(json!({ "email": email, "password": PASSWORD }))).await;
    Person {
        token: login["token"].as_str().unwrap().to_owned(),
        id: body["id"].as_str().unwrap().to_owned(),
        email: email.to_owned(),
    }
}

/// An organisation with a device in a group and a rule for `operator`; returns (org, device, group).
async fn world(h: &Harness, owner: &Person, operator: &Person, caps: &[&str]) -> (String, String, String) {
    let org = call(&h.app, "POST", "/v1/orgs", Some(&owner.token), Some(json!({ "name": "Team" }))).await.1["id"].as_str().unwrap().to_owned();
    if operator.id != owner.id {
        call(&h.app, "POST", &format!("/v1/orgs/{org}/members"), Some(&owner.token), Some(json!({ "email": operator.email, "role": "member" }))).await;
    }
    let enroll_token = call(&h.app, "POST", &format!("/v1/orgs/{org}/enrollment-tokens"), Some(&owner.token), None).await.1["token"].as_str().unwrap().to_owned();
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    let key = SigningKey::from_bytes(&seed);
    let public_key = STANDARD.encode(key.verifying_key().to_bytes());
    let signature = STANDARD.encode(key.sign(&enrollment_message(&enroll_token, "pc", &public_key)).to_bytes());
    let device = call(
        &h.app,
        "POST",
        "/v1/devices/enroll",
        None,
        Some(json!({ "token": enroll_token, "name": "pc", "platform": "linux", "public_key": public_key, "signature": signature })),
    )
    .await
    .1["device_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let group = call(&h.app, "POST", &format!("/v1/orgs/{org}/groups"), Some(&owner.token), Some(json!({ "name": "All" }))).await.1["id"].as_str().unwrap().to_owned();
    call(&h.app, "POST", &format!("/v1/orgs/{org}/groups/{group}/devices"), Some(&owner.token), Some(json!({ "device_id": device }))).await;
    if !caps.is_empty() {
        let (status, _) = call(
            &h.app,
            "PUT",
            &format!("/v1/orgs/{org}/acl"),
            Some(&owner.token),
            Some(json!({ "user_id": operator.id, "group_id": group, "capabilities": caps })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    (org, device, group)
}

async fn server_key(h: &Harness) -> VerifyingKey {
    let (status, body) = call(&h.app, "GET", "/v1/grants/public-key", None, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["algorithm"], "ed25519");
    let bytes: [u8; 32] = STANDARD.decode(body["public_key"].as_str().unwrap()).unwrap().try_into().unwrap();
    VerifyingKey::from_bytes(&bytes).unwrap()
}

async fn issue(h: &Harness, who: &Person, org: &str, device: &str, body: Value) -> (StatusCode, Value) {
    call(&h.app, "POST", &format!("/v1/orgs/{org}/devices/{device}/grants"), Some(&who.token), Some(body)).await
}

fn local<'a>(org: &'a str, device: &'a str, caps: &'a [Capability]) -> LocalState<'a> {
    LocalState {
        org_id: org,
        device_id: device,
        device_enabled: true,
        unattended_enabled: false,
        local_capabilities: caps,
        grant_revoked: false,
        session_consent: true,
    }
}

#[tokio::test]
async fn public_key_and_issuing_need_a_configured_key() {
    let Some(h) = harness(false).await else { return };
    let owner = person(&h, "operator@example.org").await;
    let (org, device, _) = world(&h, &owner, &owner, &["view"]).await;
    assert_eq!(call(&h.app, "GET", "/v1/grants/public-key", None, None).await.0, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(issue(&h, &owner, &org, &device, json!({})).await.0, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn issued_grant_verifies_on_the_device_and_is_authorised_by_the_policy() {
    let Some(h) = harness(true).await else { return };
    let operator = person(&h, "operator@example.org").await;
    let (org, device, _) = world(&h, &operator, &operator, &["view", "input"]).await;
    let key = server_key(&h).await;

    let (status, body) = issue(&h, &operator, &org, &device, json!({})).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let token = body["token"].as_str().unwrap();
    let not_before = body["not_before"].as_u64().unwrap();
    assert_eq!(body["expires_at"].as_u64().unwrap() - not_before, 60);

    // Device side.
    let claims = verify(token, &[key], not_before + 1, &device).expect("grant verifies");
    assert_eq!((claims.org.as_str(), claims.operator.as_str(), claims.device.as_str()), (org.as_str(), operator.id.as_str(), device.as_str()));
    assert_eq!(claims.caps, vec!["input", "view"]);
    assert_eq!(claims.mode, "attended");
    let all = [Capability::View, Capability::Input, Capability::FileTransfer, Capability::Clipboard];
    assert!(authorize_connection(&claims, &local(&org, &device, &all), &[Capability::View, Capability::Input], not_before + 1).is_ok());
    assert!(authorize_connection(&claims, &local(&org, &device, &all), &[Capability::FileTransfer], not_before + 1).is_err());

    // The token is not stored; the audit entry and the table describe the grant without it.
    let columns: Vec<String> = sqlx::query_scalar("SELECT column_name FROM information_schema.columns WHERE table_name = 'grants' AND table_schema = current_schema()")
        .fetch_all(&h.pool)
        .await
        .unwrap();
    assert!(!columns.iter().any(|c| c.contains("token")), "{columns:?}");
    let audit: String = sqlx::query_scalar("SELECT string_agg(detail::text || coalesce(target, ''), ' ') FROM audit_events WHERE action = 'grant.issued'")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert!(!audit.contains(token));
    assert!(audit.contains(&device));
}

#[tokio::test]
async fn a_grant_cannot_be_replayed_reused_on_another_device_or_used_after_expiry() {
    let Some(h) = harness(true).await else { return };
    let operator = person(&h, "operator@example.org").await;
    let (org, device, _) = world(&h, &operator, &operator, &["view"]).await;
    let key = server_key(&h).await;
    let body = issue(&h, &operator, &org, &device, json!({})).await.1;
    let token = body["token"].as_str().unwrap();
    let nbf = body["not_before"].as_u64().unwrap();

    let claims = verify(token, &[key], nbf, &device).unwrap();
    let mut cache = ReplayCache::new();
    assert_eq!(cache.accept(&claims, nbf), Ok(()));
    assert_eq!(cache.accept(&claims, nbf + 5), Err(GrantError::Replay));
    assert_eq!(verify(token, &[key], nbf, &Uuid::new_v4().to_string()), Err(GrantError::WrongDevice));
    assert_eq!(verify(token, &[key], nbf + 61, &device), Err(GrantError::Expired));
    // A different server key is not trusted.
    let mut other_seed = [0u8; 32];
    OsRng.fill_bytes(&mut other_seed);
    let other = SigningKey::from_bytes(&other_seed).verifying_key();
    assert_eq!(verify(token, &[other], nbf, &device), Err(GrantError::UnknownKey));
}

#[tokio::test]
async fn grants_never_exceed_the_access_rules() {
    let Some(h) = harness(true).await else { return };
    let operator = person(&h, "operator@example.org").await;
    let (org, device, _) = world(&h, &operator, &operator, &["view"]).await;
    // Subset and mode handling.
    assert_eq!(issue(&h, &operator, &org, &device, json!({ "capabilities": ["view"], "mode": "unattended" })).await.0, StatusCode::CREATED);
    assert_eq!(issue(&h, &operator, &org, &device, json!({ "capabilities": ["input"] })).await.0, StatusCode::FORBIDDEN);
    assert_eq!(issue(&h, &operator, &org, &device, json!({ "capabilities": ["view", "clipboard"] })).await.0, StatusCode::FORBIDDEN);
    assert_eq!(issue(&h, &operator, &org, &device, json!({ "capabilities": ["root"] })).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(issue(&h, &operator, &org, &device, json!({ "capabilities": [] })).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(issue(&h, &operator, &org, &device, json!({ "mode": "root" })).await.0, StatusCode::BAD_REQUEST);
    let issued: i64 = sqlx::query_scalar("SELECT count(*) FROM grants").fetch_one(&h.pool).await.unwrap();
    assert_eq!(issued, 1, "refused requests must not leave grants behind");
}

#[tokio::test]
async fn no_rule_revoked_device_or_foreign_tenant_means_no_grant() {
    let Some(h) = harness(true).await else { return };
    let alice = person(&h, "operator@example.org").await;
    let bob = person(&h, "bob@example.org").await;
    // Alice owns the organisation but has no rule: being an owner is not an access right.
    let (org, device, group) = world(&h, &alice, &alice, &[]).await;
    assert_eq!(issue(&h, &alice, &org, &device, json!({})).await.0, StatusCode::FORBIDDEN);

    // With a rule it works; after revoking the device it stops.
    call(&h.app, "PUT", &format!("/v1/orgs/{org}/acl"), Some(&alice.token), Some(json!({ "user_id": alice.id, "group_id": group, "capabilities": ["view"] }))).await;
    assert_eq!(issue(&h, &alice, &org, &device, json!({})).await.0, StatusCode::CREATED);
    call(&h.app, "POST", &format!("/v1/orgs/{org}/devices/{device}/revoke"), Some(&alice.token), None).await;
    assert_eq!(issue(&h, &alice, &org, &device, json!({})).await.0, StatusCode::FORBIDDEN);

    // Bob, in another organisation, learns nothing and gets nothing.
    assert_eq!(issue(&h, &bob, &org, &device, json!({})).await.0, StatusCode::NOT_FOUND);
    let own_org = call(&h.app, "POST", "/v1/orgs", Some(&bob.token), Some(json!({ "name": "B" }))).await.1["id"].as_str().unwrap().to_owned();
    assert_eq!(issue(&h, &bob, &own_org, &device, json!({})).await.0, StatusCode::NOT_FOUND);
    assert_eq!(call(&h.app, "POST", &format!("/v1/orgs/{org}/devices/{device}/grants"), None, Some(json!({}))).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn grants_can_be_revoked_by_their_operator_or_an_admin_only() {
    let Some(h) = harness(true).await else { return };
    let owner = person(&h, "owner@example.org").await;
    let operator = person(&h, "operator@example.org").await;
    let other = person(&h, "other@example.org").await;
    let (org, device, _) = world(&h, &owner, &operator, &["view"]).await;
    call(&h.app, "POST", &format!("/v1/orgs/{org}/members"), Some(&owner.token), Some(json!({ "email": "other@example.org", "role": "member" }))).await;

    let first = issue(&h, &operator, &org, &device, json!({})).await.1["grant_id"].as_str().unwrap().to_owned();
    let second = issue(&h, &operator, &org, &device, json!({})).await.1["grant_id"].as_str().unwrap().to_owned();
    // Another plain member cannot touch it (and cannot tell it exists).
    assert_eq!(call(&h.app, "POST", &format!("/v1/orgs/{org}/grants/{first}/revoke"), Some(&other.token), None).await.0, StatusCode::NOT_FOUND);
    // The operator can revoke their own grant, once.
    assert_eq!(call(&h.app, "POST", &format!("/v1/orgs/{org}/grants/{first}/revoke"), Some(&operator.token), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(call(&h.app, "POST", &format!("/v1/orgs/{org}/grants/{first}/revoke"), Some(&operator.token), None).await.0, StatusCode::NOT_FOUND);
    // An admin (the owner) can revoke someone else's grant.
    assert_eq!(call(&h.app, "POST", &format!("/v1/orgs/{org}/grants/{second}/revoke"), Some(&owner.token), None).await.0, StatusCode::NO_CONTENT);
    let revoked: i64 = sqlx::query_scalar("SELECT count(*) FROM grants WHERE revoked_at IS NOT NULL").fetch_one(&h.pool).await.unwrap();
    assert_eq!(revoked, 2);
    // Unknown grant and other organisations.
    assert_eq!(call(&h.app, "POST", &format!("/v1/orgs/{org}/grants/{}/revoke", Uuid::new_v4()), Some(&owner.token), None).await.0, StatusCode::NOT_FOUND);
    let actions: Vec<String> = sqlx::query_scalar("SELECT action FROM audit_events WHERE action LIKE 'grant.%' ORDER BY id")
        .fetch_all(&h.pool)
        .await
        .unwrap();
    assert_eq!(actions.iter().filter(|a| *a == "grant.issued").count(), 2);
    assert_eq!(actions.iter().filter(|a| *a == "grant.revoked").count(), 2);
}

#[tokio::test]
async fn the_database_refuses_grants_across_tenants() {
    let Some(h) = harness(true).await else { return };
    let alice = person(&h, "operator@example.org").await;
    let bob = person(&h, "bob@example.org").await;
    let (org_a, device_a, _) = world(&h, &alice, &alice, &["view"]).await;
    let org_b = call(&h.app, "POST", "/v1/orgs", Some(&bob.token), Some(json!({ "name": "B" }))).await.1["id"].as_str().unwrap().to_owned();
    let result = sqlx::query(
        "INSERT INTO grants (id, org_id, operator_id, device_id, capabilities, mode, not_before, expires_at) \
         VALUES ($1, $2, $3, $4, ARRAY['view'], 'attended', now(), now() + interval '1 minute')",
    )
    .bind(Uuid::new_v4())
    .bind(org_b.parse::<Uuid>().unwrap())
    .bind(bob.id.parse::<Uuid>().unwrap())
    .bind(device_a.parse::<Uuid>().unwrap())
    .execute(&h.pool)
    .await;
    assert!(result.is_err(), "a grant of organisation B for a device of organisation {org_a} must be impossible");
    let _ = ConnectionDenied::WrongDevice; // the policy-level counterpart is covered in the grant crate
}
