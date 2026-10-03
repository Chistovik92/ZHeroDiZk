// SPDX-License-Identifier: AGPL-3.0-only
//! Groups, ACL and address books against a real PostgreSQL server, including tenant isolation
//! enforced by the database itself. Set ZHD_TEST_DATABASE_URL (CI provides one).

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

struct Person {
    token: String,
    id: String,
}

async fn person(h: &Harness, email: &str) -> Person {
    let (status, body) = call(&h.app, "POST", "/v1/auth/register", None, Some(json!({ "email": email, "password": PASSWORD }))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (_, login) = call(&h.app, "POST", "/v1/auth/login", None, Some(json!({ "email": email, "password": PASSWORD }))).await;
    Person { token: login["token"].as_str().unwrap().to_owned(), id: body["id"].as_str().unwrap().to_owned() }
}

async fn org(h: &Harness, owner: &Person, name: &str) -> String {
    call(&h.app, "POST", "/v1/orgs", Some(&owner.token), Some(json!({ "name": name }))).await.1["id"].as_str().unwrap().to_owned()
}

async fn add_member(h: &Harness, owner: &Person, org: &str, email: &str, role: &str) {
    let (status, _) = call(
        &h.app,
        "POST",
        &format!("/v1/orgs/{org}/members"),
        Some(&owner.token),
        Some(json!({ "email": email, "role": role })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

/// Enrol a device through the real flow; returns its id.
async fn device(h: &Harness, owner: &Person, org: &str, name: &str) -> String {
    let token = call(&h.app, "POST", &format!("/v1/orgs/{org}/enrollment-tokens"), Some(&owner.token), None).await.1["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    let key = SigningKey::from_bytes(&seed);
    let public_key = STANDARD.encode(key.verifying_key().to_bytes());
    let signature = STANDARD.encode(key.sign(&enrollment_message(&token, name, &public_key)).to_bytes());
    let (status, body) = call(
        &h.app,
        "POST",
        "/v1/devices/enroll",
        None,
        Some(json!({ "token": token, "name": name, "platform": "windows", "public_key": public_key, "signature": signature })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["device_id"].as_str().unwrap().to_owned()
}

async fn group(h: &Harness, owner: &Person, org: &str, name: &str) -> String {
    let (status, body) = call(&h.app, "POST", &format!("/v1/orgs/{org}/groups"), Some(&owner.token), Some(json!({ "name": name }))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().unwrap().to_owned()
}

async fn put_acl(h: &Harness, owner: &Person, org: &str, user: &str, group: &str, caps: &[&str]) -> (StatusCode, Value) {
    call(
        &h.app,
        "PUT",
        &format!("/v1/orgs/{org}/acl"),
        Some(&owner.token),
        Some(json!({ "user_id": user, "group_id": group, "capabilities": caps })),
    )
    .await
}

async fn access(h: &Harness, who: &Person, org: &str, device: &str) -> Vec<String> {
    let (status, body) = call(&h.app, "GET", &format!("/v1/orgs/{org}/devices/{device}/access"), Some(&who.token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["capabilities"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_owned()).collect()
}

#[tokio::test]
async fn access_is_denied_by_default_even_for_owners() {
    let Some(h) = harness().await else { return };
    let owner = person(&h, "owner@example.org").await;
    let org_id = org(&h, &owner, "Team").await;
    let dev = device(&h, &owner, &org_id, "pc").await;
    assert!(access(&h, &owner, &org_id, &dev).await.is_empty(), "owner role is not an access right");
}

#[tokio::test]
async fn rules_grant_the_union_of_capabilities_per_group() {
    let Some(h) = harness().await else { return };
    let owner = person(&h, "owner@example.org").await;
    let worker = person(&h, "worker@example.org").await;
    let org_id = org(&h, &owner, "Team").await;
    add_member(&h, &owner, &org_id, "worker@example.org", "member").await;
    let pc = device(&h, &owner, &org_id, "pc").await;
    let other = device(&h, &owner, &org_id, "other").await;
    let g1 = group(&h, &owner, &org_id, "Office").await;
    let g2 = group(&h, &owner, &org_id, "Servers").await;
    for (g, d) in [(&g1, &pc), (&g2, &pc)] {
        let (status, _) = call(&h.app, "POST", &format!("/v1/orgs/{org_id}/groups/{g}/devices"), Some(&owner.token), Some(json!({ "device_id": d }))).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    assert_eq!(put_acl(&h, &owner, &org_id, &worker.id, &g1, &["view", "input", "view"]).await.0, StatusCode::OK);
    assert_eq!(put_acl(&h, &owner, &org_id, &worker.id, &g2, &["clipboard"]).await.0, StatusCode::OK);

    assert_eq!(access(&h, &worker, &org_id, &pc).await, vec!["clipboard", "input", "view"]);
    assert!(access(&h, &worker, &org_id, &other).await.is_empty(), "devices outside the groups stay closed");

    // Updating a rule replaces it; deleting removes it.
    assert_eq!(put_acl(&h, &owner, &org_id, &worker.id, &g1, &["view"]).await.0, StatusCode::OK);
    assert_eq!(access(&h, &worker, &org_id, &pc).await, vec!["clipboard", "view"]);
    let del = format!("/v1/orgs/{org_id}/acl/{}/{}", worker.id, g2);
    assert_eq!(call(&h.app, "DELETE", &del, Some(&owner.token), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(call(&h.app, "DELETE", &del, Some(&owner.token), None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(access(&h, &worker, &org_id, &pc).await, vec!["view"]);

    // Removing the device from the group closes the access again.
    let (status, _) = call(&h.app, "DELETE", &format!("/v1/orgs/{org_id}/groups/{g1}/devices/{pc}"), Some(&owner.token), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(access(&h, &worker, &org_id, &pc).await.is_empty());
}

#[tokio::test]
async fn revoked_devices_have_no_access() {
    let Some(h) = harness().await else { return };
    let owner = person(&h, "owner@example.org").await;
    let org_id = org(&h, &owner, "Team").await;
    let pc = device(&h, &owner, &org_id, "pc").await;
    let g = group(&h, &owner, &org_id, "All").await;
    call(&h.app, "POST", &format!("/v1/orgs/{org_id}/groups/{g}/devices"), Some(&owner.token), Some(json!({ "device_id": pc }))).await;
    assert_eq!(put_acl(&h, &owner, &org_id, &owner.id, &g, &["view", "input"]).await.0, StatusCode::OK);
    assert_eq!(access(&h, &owner, &org_id, &pc).await, vec!["input", "view"]);
    call(&h.app, "POST", &format!("/v1/orgs/{org_id}/devices/{pc}/revoke"), Some(&owner.token), None).await;
    assert!(access(&h, &owner, &org_id, &pc).await.is_empty());
}

#[tokio::test]
async fn acl_input_is_validated_and_role_gated() {
    let Some(h) = harness().await else { return };
    let owner = person(&h, "owner@example.org").await;
    let worker = person(&h, "worker@example.org").await;
    let stranger = person(&h, "stranger@example.org").await;
    let org_id = org(&h, &owner, "Team").await;
    add_member(&h, &owner, &org_id, "worker@example.org", "member").await;
    let g = group(&h, &owner, &org_id, "Office").await;

    assert_eq!(put_acl(&h, &owner, &org_id, &worker.id, &g, &["root"]).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(put_acl(&h, &owner, &org_id, &worker.id, &g, &[]).await.0, StatusCode::BAD_REQUEST);
    // A user outside the organisation cannot be given a rule.
    assert_eq!(put_acl(&h, &owner, &org_id, &stranger.id, &g, &["view"]).await.0, StatusCode::NOT_FOUND);
    // Unknown group.
    assert_eq!(put_acl(&h, &owner, &org_id, &worker.id, &Uuid::new_v4().to_string(), &["view"]).await.0, StatusCode::NOT_FOUND);
    // A plain member cannot manage groups or rules.
    let as_worker = call(&h.app, "POST", &format!("/v1/orgs/{org_id}/groups"), Some(&worker.token), Some(json!({ "name": "mine" }))).await;
    assert_eq!(as_worker.0, StatusCode::FORBIDDEN);
    let (status, _) = call(&h.app, "GET", &format!("/v1/orgs/{org_id}/acl"), Some(&worker.token), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // Duplicate group names conflict.
    let (status, _) = call(&h.app, "POST", &format!("/v1/orgs/{org_id}/groups"), Some(&owner.token), Some(json!({ "name": "Office" }))).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn tenants_are_isolated_in_the_api() {
    let Some(h) = harness().await else { return };
    let alice = person(&h, "alice@example.org").await;
    let bob = person(&h, "bob@example.org").await;
    let org_a = org(&h, &alice, "A").await;
    let org_b = org(&h, &bob, "B").await;
    let dev_a = device(&h, &alice, &org_a, "alice pc").await;
    let group_b = group(&h, &bob, &org_b, "Bob group").await;

    // Bob cannot put Alice's device into his group.
    let (status, _) = call(&h.app, "POST", &format!("/v1/orgs/{org_b}/groups/{group_b}/devices"), Some(&bob.token), Some(json!({ "device_id": dev_a }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // Nor read Alice's groups or ACL.
    assert_eq!(call(&h.app, "GET", &format!("/v1/orgs/{org_a}/groups"), Some(&bob.token), None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(call(&h.app, "GET", &format!("/v1/orgs/{org_a}/acl"), Some(&bob.token), None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        call(&h.app, "GET", &format!("/v1/orgs/{org_b}/devices/{dev_a}/access"), Some(&bob.token), None).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn the_database_itself_refuses_cross_tenant_links() {
    let Some(h) = harness().await else { return };
    let alice = person(&h, "alice@example.org").await;
    let bob = person(&h, "bob@example.org").await;
    let org_a: Uuid = org(&h, &alice, "A").await.parse().unwrap();
    let org_b: Uuid = org(&h, &bob, "B").await.parse().unwrap();
    let dev_a: Uuid = device(&h, &alice, &org_a.to_string(), "alice pc").await.parse().unwrap();
    let group_b: Uuid = group(&h, &bob, &org_b.to_string(), "Bob group").await.parse().unwrap();
    let alice_id: Uuid = alice.id.parse().unwrap();

    // Group of tenant B + device of tenant A, labelled with either tenant: both are rejected.
    for label in [org_a, org_b] {
        let linked = sqlx::query("INSERT INTO device_group_members (group_id, device_id, org_id) VALUES ($1, $2, $3)")
            .bind(group_b)
            .bind(dev_a)
            .bind(label)
            .execute(&h.pool)
            .await;
        assert!(linked.is_err(), "cross-tenant group membership must be impossible");
    }
    // A rule for a user who is not a member of that organisation.
    let rule = sqlx::query("INSERT INTO acl_rules (org_id, user_id, group_id, capabilities) VALUES ($1, $2, $3, ARRAY['view'])")
        .bind(org_b)
        .bind(alice_id)
        .bind(group_b)
        .execute(&h.pool)
        .await;
    assert!(rule.is_err(), "ACL rule for a non-member must be impossible");
    // Unknown capability names are rejected by the table itself.
    let bad = sqlx::query("INSERT INTO acl_rules (org_id, user_id, group_id, capabilities) VALUES ($1, $2, $3, ARRAY['root'])")
        .bind(org_b)
        .bind(bob.id.parse::<Uuid>().unwrap())
        .bind(group_b)
        .execute(&h.pool)
        .await;
    assert!(bad.is_err(), "unknown capability must be impossible");
}

#[tokio::test]
async fn address_book_is_personal_and_tenant_scoped() {
    let Some(h) = harness().await else { return };
    let owner = person(&h, "owner@example.org").await;
    let worker = person(&h, "worker@example.org").await;
    let outsider = person(&h, "outsider@example.org").await;
    let org_id = org(&h, &owner, "Team").await;
    add_member(&h, &owner, &org_id, "worker@example.org", "member").await;
    let pc = device(&h, &owner, &org_id, "Office PC").await;
    let path = format!("/v1/orgs/{org_id}/address-book/{pc}");

    assert_eq!(call(&h.app, "PUT", &path, Some(&owner.token), Some(json!({ "alias": "My desk" }))).await.0, StatusCode::NO_CONTENT);
    assert_eq!(call(&h.app, "PUT", &path, Some(&owner.token), Some(json!({ "alias": "Desk" }))).await.0, StatusCode::NO_CONTENT);
    let (_, mine) = call(&h.app, "GET", &format!("/v1/orgs/{org_id}/address-book"), Some(&owner.token), None).await;
    assert_eq!(mine.as_array().unwrap().len(), 1);
    assert_eq!(mine[0]["alias"], "Desk");
    assert_eq!(mine[0]["device_name"], "Office PC");
    // Another member sees an empty list; a non-member sees nothing at all.
    let (_, theirs) = call(&h.app, "GET", &format!("/v1/orgs/{org_id}/address-book"), Some(&worker.token), None).await;
    assert_eq!(theirs, json!([]));
    assert_eq!(call(&h.app, "GET", &format!("/v1/orgs/{org_id}/address-book"), Some(&outsider.token), None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(call(&h.app, "PUT", &path, Some(&outsider.token), Some(json!({ "alias": "x" }))).await.0, StatusCode::NOT_FOUND);
    // Unknown device and bad alias.
    let unknown = format!("/v1/orgs/{org_id}/address-book/{}", Uuid::new_v4());
    assert_eq!(call(&h.app, "PUT", &unknown, Some(&owner.token), Some(json!({ "alias": "x" }))).await.0, StatusCode::NOT_FOUND);
    assert_eq!(call(&h.app, "PUT", &path, Some(&owner.token), Some(json!({ "alias": "  " }))).await.0, StatusCode::BAD_REQUEST);

    assert_eq!(call(&h.app, "DELETE", &path, Some(&owner.token), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(call(&h.app, "DELETE", &path, Some(&owner.token), None).await.0, StatusCode::NOT_FOUND);
}
