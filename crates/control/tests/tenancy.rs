// SPDX-License-Identifier: AGPL-3.0-only
//! Organisations, roles and device enrolment against a real PostgreSQL server, including the
//! negative cases: other tenants, low roles, forged proofs, reused and expired tokens.
//! Set ZHD_TEST_DATABASE_URL (CI provides one); without it the tests are skipped.

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
use zherodizk_control::{connect_and_migrate_in_schema, devices::enrollment_message, router, AppState, AuthSettings};

const PASSWORD: &str = "correct horse battery staple";

struct Harness {
    app: Router,
    pool: sqlx::PgPool,
}

async fn harness() -> Option<Harness> {
    let url = std::env::var("ZHD_TEST_DATABASE_URL").ok()?;
    let schema = format!("t_{}", Uuid::new_v4().simple());
    let pool = connect_and_migrate_in_schema(&url, &schema).await.expect("schema and migrations");
    let settings = AuthSettings { allow_registration: true, ..AuthSettings::default() };
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

/// Register and log in; returns the session token.
async fn user(h: &Harness, email: &str) -> String {
    let (status, body) = call(&h.app, "POST", "/v1/auth/register", None, Some(json!({ "email": email, "password": PASSWORD }))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (_, login) = call(&h.app, "POST", "/v1/auth/login", None, Some(json!({ "email": email, "password": PASSWORD }))).await;
    login["token"].as_str().unwrap().to_owned()
}

async fn org(h: &Harness, token: &str, name: &str) -> String {
    let (status, body) = call(&h.app, "POST", "/v1/orgs", Some(token), Some(json!({ "name": name }))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().unwrap().to_owned()
}

async fn enrol_token(h: &Harness, token: &str, org: &str) -> String {
    let (status, body) = call(&h.app, "POST", &format!("/v1/orgs/{org}/enrollment-tokens"), Some(token), None).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["token"].as_str().unwrap().to_owned()
}

fn new_key() -> SigningKey {
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    SigningKey::from_bytes(&seed)
}

/// A correct enrolment request body for the given key.
fn enroll_body(token: &str, name: &str, key: &SigningKey) -> Value {
    let public_key = STANDARD.encode(key.verifying_key().to_bytes());
    let signature = key.sign(&enrollment_message(token, name, &public_key));
    json!({
        "token": token,
        "name": name,
        "platform": "linux",
        "public_key": public_key,
        "signature": STANDARD.encode(signature.to_bytes()),
    })
}

async fn enroll(h: &Harness, body: Value) -> (StatusCode, Value) {
    call(&h.app, "POST", "/v1/devices/enroll", None, Some(body)).await
}

#[tokio::test]
async fn creating_and_listing_organisations() {
    let Some(h) = harness().await else { return };
    let alice = user(&h, "alice@example.org").await;
    let bob = user(&h, "bob@example.org").await;
    let org_a = org(&h, &alice, "  Team A  ").await;
    org(&h, &bob, "Team B").await;

    let (_, mine) = call(&h.app, "GET", "/v1/orgs", Some(&alice), None).await;
    assert_eq!(mine.as_array().unwrap().len(), 1, "alice sees only her organisation");
    assert_eq!(mine[0]["id"], org_a);
    assert_eq!(mine[0]["name"], "Team A");
    assert_eq!(mine[0]["role"], "owner");

    let (status, _) = call(&h.app, "POST", "/v1/orgs", Some(&alice), Some(json!({ "name": "   " }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(call(&h.app, "POST", "/v1/orgs", None, Some(json!({ "name": "x" }))).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn roles_gate_member_management_and_tokens() {
    let Some(h) = harness().await else { return };
    let owner = user(&h, "owner@example.org").await;
    let admin = user(&h, "admin@example.org").await;
    let member = user(&h, "member@example.org").await;
    let outsider = user(&h, "outsider@example.org").await;
    let org_id = org(&h, &owner, "Team").await;
    let members = format!("/v1/orgs/{org_id}/members");

    let add = |email: &str, role: &str| Some(json!({ "email": email, "role": role }));
    assert_eq!(call(&h.app, "POST", &members, Some(&owner), add("admin@example.org", "admin")).await.0, StatusCode::NO_CONTENT);
    assert_eq!(call(&h.app, "POST", &members, Some(&admin), add("member@example.org", "member")).await.0, StatusCode::NO_CONTENT);
    // Duplicates, unknown users and owner promotion are refused.
    assert_eq!(call(&h.app, "POST", &members, Some(&admin), add("member@example.org", "member")).await.0, StatusCode::CONFLICT);
    assert_eq!(call(&h.app, "POST", &members, Some(&owner), add("nobody@example.org", "member")).await.0, StatusCode::NOT_FOUND);
    assert_eq!(call(&h.app, "POST", &members, Some(&owner), add("outsider@example.org", "owner")).await.0, StatusCode::BAD_REQUEST);
    // An admin cannot create other admins; a plain member cannot add anyone.
    assert_eq!(call(&h.app, "POST", &members, Some(&admin), add("outsider@example.org", "admin")).await.0, StatusCode::FORBIDDEN);
    assert_eq!(call(&h.app, "POST", &members, Some(&member), add("outsider@example.org", "member")).await.0, StatusCode::FORBIDDEN);
    // A non-member learns nothing about the organisation.
    assert_eq!(call(&h.app, "POST", &members, Some(&outsider), add("outsider@example.org", "member")).await.0, StatusCode::NOT_FOUND);

    let tokens = format!("/v1/orgs/{org_id}/enrollment-tokens");
    assert_eq!(call(&h.app, "POST", &tokens, Some(&admin), None).await.0, StatusCode::CREATED);
    assert_eq!(call(&h.app, "POST", &tokens, Some(&member), None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(call(&h.app, "POST", &tokens, Some(&outsider), None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn device_enrolment_happy_path_stores_only_the_public_key() {
    let Some(h) = harness().await else { return };
    let owner = user(&h, "owner@example.org").await;
    let org_id = org(&h, &owner, "Team").await;
    let token = enrol_token(&h, &owner, &org_id).await;

    let key = new_key();
    let (status, body) = enroll(&h, enroll_body(&token, "Office PC", &key)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["org_id"], org_id);

    let (status, list) = call(&h.app, "GET", &format!("/v1/orgs/{org_id}/devices"), Some(&owner), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["name"], "Office PC");
    assert_eq!(list[0]["status"], "active");

    let stored: Vec<u8> = sqlx::query_scalar("SELECT public_key FROM devices").fetch_one(&h.pool).await.unwrap();
    assert_eq!(stored, key.verifying_key().to_bytes());
    let digest: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM enrollment_tokens").fetch_one(&h.pool).await.unwrap();
    assert_ne!(digest, token.as_bytes(), "only a digest of the token is stored");
}

#[tokio::test]
async fn token_is_single_use() {
    let Some(h) = harness().await else { return };
    let owner = user(&h, "owner@example.org").await;
    let org_id = org(&h, &owner, "Team").await;
    let token = enrol_token(&h, &owner, &org_id).await;
    assert_eq!(enroll(&h, enroll_body(&token, "first", &new_key())).await.0, StatusCode::CREATED);
    assert_eq!(enroll(&h, enroll_body(&token, "second", &new_key())).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn forged_proofs_are_refused_and_do_not_spend_the_token() {
    let Some(h) = harness().await else { return };
    let owner = user(&h, "owner@example.org").await;
    let org_id = org(&h, &owner, "Team").await;
    let token = enrol_token(&h, &owner, &org_id).await;
    let key = new_key();

    // Signature made with another key.
    let mut forged = enroll_body(&token, "pc", &key);
    forged["signature"] = enroll_body(&token, "pc", &new_key())["signature"].clone();
    assert_eq!(enroll(&h, forged).await.0, StatusCode::UNAUTHORIZED);
    // A valid signature for a different name (the message is bound to every field).
    let mut renamed = enroll_body(&token, "pc", &key);
    renamed["name"] = json!("other name");
    assert_eq!(enroll(&h, renamed).await.0, StatusCode::UNAUTHORIZED);
    // Someone else's public key with our signature.
    let mut swapped = enroll_body(&token, "pc", &key);
    swapped["public_key"] = enroll_body(&token, "pc", &new_key())["public_key"].clone();
    assert_eq!(enroll(&h, swapped).await.0, StatusCode::UNAUTHORIZED);
    // Malformed encodings.
    let mut short_key = enroll_body(&token, "pc", &key);
    short_key["public_key"] = json!("AAAA");
    assert_eq!(enroll(&h, short_key).await.0, StatusCode::BAD_REQUEST);
    let mut bad_platform = enroll_body(&token, "pc", &key);
    bad_platform["platform"] = json!("amiga");
    assert_eq!(enroll(&h, bad_platform).await.0, StatusCode::BAD_REQUEST);

    // The token survived all of that.
    assert_eq!(enroll(&h, enroll_body(&token, "pc", &key)).await.0, StatusCode::CREATED);
}

#[tokio::test]
async fn unknown_and_expired_tokens_are_refused() {
    let Some(h) = harness().await else { return };
    let owner = user(&h, "owner@example.org").await;
    let org_id = org(&h, &owner, "Team").await;
    assert_eq!(enroll(&h, enroll_body("not-a-real-token", "pc", &new_key())).await.0, StatusCode::UNAUTHORIZED);

    let token = enrol_token(&h, &owner, &org_id).await;
    sqlx::query("UPDATE enrollment_tokens SET expires_at = now() - interval '1 second'")
        .execute(&h.pool)
        .await
        .unwrap();
    assert_eq!(enroll(&h, enroll_body(&token, "pc", &new_key())).await.0, StatusCode::UNAUTHORIZED);
    let devices: i64 = sqlx::query_scalar("SELECT count(*) FROM devices").fetch_one(&h.pool).await.unwrap();
    assert_eq!(devices, 0);
}

#[tokio::test]
async fn one_key_cannot_enrol_twice_and_the_token_is_kept() {
    let Some(h) = harness().await else { return };
    let owner = user(&h, "owner@example.org").await;
    let org_id = org(&h, &owner, "Team").await;
    let key = new_key();
    let first = enrol_token(&h, &owner, &org_id).await;
    assert_eq!(enroll(&h, enroll_body(&first, "pc", &key)).await.0, StatusCode::CREATED);
    let second = enrol_token(&h, &owner, &org_id).await;
    assert_eq!(enroll(&h, enroll_body(&second, "pc again", &key)).await.0, StatusCode::CONFLICT);
    let unused: i64 = sqlx::query_scalar("SELECT count(*) FROM enrollment_tokens WHERE used_at IS NULL")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(unused, 1, "a failed enrolment must not spend its token");
}

#[tokio::test]
async fn other_tenants_cannot_see_or_touch_devices() {
    let Some(h) = harness().await else { return };
    let alice = user(&h, "alice@example.org").await;
    let bob = user(&h, "bob@example.org").await;
    let org_a = org(&h, &alice, "Team A").await;
    let org_b = org(&h, &bob, "Team B").await;
    let token = enrol_token(&h, &alice, &org_a).await;
    let (_, enrolled) = enroll(&h, enroll_body(&token, "alice pc", &new_key())).await;
    let device = enrolled["device_id"].as_str().unwrap().to_owned();

    // Bob cannot list, mint tokens for, or revoke anything in Alice's organisation.
    assert_eq!(call(&h.app, "GET", &format!("/v1/orgs/{org_a}/devices"), Some(&bob), None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(call(&h.app, "POST", &format!("/v1/orgs/{org_a}/enrollment-tokens"), Some(&bob), None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        call(&h.app, "POST", &format!("/v1/orgs/{org_a}/devices/{device}/revoke"), Some(&bob), None).await.0,
        StatusCode::NOT_FOUND
    );
    // Using his own organisation id with Alice's device id matches nothing either.
    assert_eq!(
        call(&h.app, "POST", &format!("/v1/orgs/{org_b}/devices/{device}/revoke"), Some(&bob), None).await.0,
        StatusCode::NOT_FOUND
    );
    let status: String = sqlx::query_scalar("SELECT status FROM devices").fetch_one(&h.pool).await.unwrap();
    assert_eq!(status, "active");
    assert_eq!(call(&h.app, "GET", &format!("/v1/orgs/{org_b}/devices"), Some(&bob), None).await.1, json!([]));
}

#[tokio::test]
async fn revocation_needs_an_admin_and_is_not_repeatable() {
    let Some(h) = harness().await else { return };
    let owner = user(&h, "owner@example.org").await;
    let member = user(&h, "member@example.org").await;
    let org_id = org(&h, &owner, "Team").await;
    call(
        &h.app,
        "POST",
        &format!("/v1/orgs/{org_id}/members"),
        Some(&owner),
        Some(json!({ "email": "member@example.org", "role": "member" })),
    )
    .await;
    let token = enrol_token(&h, &owner, &org_id).await;
    let (_, enrolled) = enroll(&h, enroll_body(&token, "pc", &new_key())).await;
    let device = enrolled["device_id"].as_str().unwrap();
    let revoke = format!("/v1/orgs/{org_id}/devices/{device}/revoke");

    // A plain member can read the list but not revoke.
    assert_eq!(call(&h.app, "GET", &format!("/v1/orgs/{org_id}/devices"), Some(&member), None).await.0, StatusCode::OK);
    assert_eq!(call(&h.app, "POST", &revoke, Some(&member), None).await.0, StatusCode::FORBIDDEN);

    assert_eq!(call(&h.app, "POST", &revoke, Some(&owner), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(call(&h.app, "POST", &revoke, Some(&owner), None).await.0, StatusCode::NOT_FOUND);
    let (_, list) = call(&h.app, "GET", &format!("/v1/orgs/{org_id}/devices"), Some(&owner), None).await;
    assert_eq!(list[0]["status"], "revoked");
}
