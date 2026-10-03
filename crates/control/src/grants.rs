// SPDX-License-Identifier: AGPL-3.0-only
//! Session grants: after checking the operator's access rules, the server signs a short-lived
//! grant that the device agent verifies (see the `zherodizk-grant` crate). Stage 0.5.1.
//! A grant is only a statement of permission; nothing here connects anyone to a device.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{DateTime, Utc};
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zherodizk_grant::{key_id, sign, Claims, VERSION};

use crate::{
    audit,
    auth::{authenticate, unix_now, ApiError},
    groups::{device_in_org, effective_capabilities, normalize_capabilities},
    orgs::{require_role, Role},
    AppState,
};

/// Lifetime of an issued grant in seconds; devices accept at most 300.
pub const GRANT_TTL_SECS: u64 = 60;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/grants/public-key", get(public_key))
        .route("/v1/orgs/:org/devices/:device/grants", post(issue_grant))
        .route("/v1/orgs/:org/grants", get(list_grants))
        .route("/v1/orgs/:org/grants/:grant/revoke", post(revoke_grant))
}

fn signing_key(state: &AppState) -> Result<SigningKey, ApiError> {
    let seed = state
        .settings
        .grant_key
        .as_ref()
        .ok_or(ApiError::Unavailable("grant signing is not configured on this server"))?;
    Ok(SigningKey::from_bytes(seed))
}

#[derive(Serialize)]
struct PublicKeyOut {
    algorithm: &'static str,
    kid: String,
    public_key: String,
}

/// The key devices use to verify grants (they are given it when they enrol).
async fn public_key(State(state): State<AppState>) -> Result<Json<PublicKeyOut>, ApiError> {
    let verifying = signing_key(&state)?.verifying_key();
    Ok(Json(PublicKeyOut {
        algorithm: "ed25519",
        kid: key_id(&verifying),
        public_key: STANDARD.encode(verifying.to_bytes()),
    }))
}

#[derive(Deserialize)]
struct IssueBody {
    /// Subset of the caller's effective capabilities; all of them when omitted.
    capabilities: Option<Vec<String>>,
    /// `attended` (default) or `unattended`.
    mode: Option<String>,
}

#[derive(Serialize)]
struct GrantOut {
    grant_id: Uuid,
    token: String,
    capabilities: Vec<String>,
    mode: String,
    not_before: u64,
    expires_at: u64,
}

async fn issue_grant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((org, device)): Path<(Uuid, Uuid)>,
    Json(body): Json<IssueBody>,
) -> Result<(StatusCode, Json<GrantOut>), ApiError> {
    let who = authenticate(&state, &headers).await?;
    require_role(&state, who.user_id, org, Role::Member).await?;
    device_in_org(&state, org, device).await?;
    let key = signing_key(&state)?;

    let allowed = effective_capabilities(&state, who.user_id, org, device).await?;
    if allowed.is_empty() {
        return Err(ApiError::Forbidden("no access rule allows this operator to use this device"));
    }
    let capabilities = match &body.capabilities {
        Some(requested) => normalize_capabilities(requested)?,
        None => allowed.clone(),
    };
    if !capabilities.iter().all(|c| allowed.contains(c)) {
        return Err(ApiError::Forbidden("requested capabilities exceed what the access rules allow"));
    }
    let mode = body.mode.unwrap_or_else(|| "attended".to_owned());
    if mode != "attended" && mode != "unattended" {
        return Err(ApiError::Invalid("mode must be attended or unattended".into()));
    }

    let grant_id = Uuid::new_v4();
    let not_before = unix_now(&state);
    let expires_at = not_before + GRANT_TTL_SECS;
    let claims = Claims {
        v: VERSION,
        jti: grant_id.to_string(),
        org: org.to_string(),
        operator: who.user_id.to_string(),
        device: device.to_string(),
        caps: capabilities.clone(),
        mode: mode.clone(),
        nbf: not_before,
        exp: expires_at,
    };
    let token = sign(&claims, &key).map_err(|_| ApiError::Internal)?;

    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "INSERT INTO grants (id, org_id, operator_id, device_id, capabilities, mode, not_before, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, to_timestamp($7::double precision), to_timestamp($8::double precision))",
    )
    .bind(grant_id)
    .bind(org)
    .bind(who.user_id)
    .bind(device)
    .bind(&capabilities)
    .bind(&mode)
    .bind(not_before as f64)
    .bind(expires_at as f64)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut *tx,
        Some(org),
        Some(who.user_id),
        "grant.issued",
        Some(grant_id.to_string()),
        serde_json::json!({ "device_id": device, "capabilities": capabilities, "mode": mode }),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(GrantOut { grant_id, token, capabilities, mode, not_before, expires_at })))
}

#[derive(Deserialize)]
struct ListQuery {
    limit: Option<i64>,
}

#[derive(Serialize)]
struct GrantListItem {
    grant_id: Uuid,
    operator_id: Uuid,
    operator_email: String,
    device_id: Uuid,
    device_name: String,
    capabilities: Vec<String>,
    mode: String,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    /// `active` (not expired, not revoked), `expired` or `revoked`.
    status: &'static str,
}

type GrantRow = (Uuid, Uuid, String, Uuid, String, Vec<String>, String, DateTime<Utc>, DateTime<Utc>, Option<DateTime<Utc>>);

/// Grants issued in the organisation, newest first. Admins see everyone's; a member sees only
/// their own. The signed token is never stored, so it cannot be listed.
async fn list_grants(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(org): Path<Uuid>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<GrantListItem>>, ApiError> {
    let who = authenticate(&state, &headers).await?;
    let role = require_role(&state, who.user_id, org, Role::Member).await?;
    let only_operator: Option<Uuid> = (role < Role::Admin).then_some(who.user_id);
    let limit = query.limit.unwrap_or(100).clamp(1, 200);
    let rows: Vec<GrantRow> = sqlx::query_as(
        "SELECT g.id, g.operator_id, u.email, g.device_id, d.name, g.capabilities, g.mode, \
                g.issued_at, g.expires_at, g.revoked_at \
         FROM grants g JOIN users u ON u.id = g.operator_id JOIN devices d ON d.id = g.device_id \
         WHERE g.org_id = $1 AND ($2::uuid IS NULL OR g.operator_id = $2) \
         ORDER BY g.issued_at DESC, g.id LIMIT $3",
    )
    .bind(org)
    .bind(only_operator)
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;
    let now = Utc::now();
    Ok(Json(
        rows.into_iter()
            .map(|(grant_id, operator_id, operator_email, device_id, device_name, capabilities, mode, issued_at, expires_at, revoked_at)| {
                let status = if revoked_at.is_some() {
                    "revoked"
                } else if expires_at <= now {
                    "expired"
                } else {
                    "active"
                };
                GrantListItem { grant_id, operator_id, operator_email, device_id, device_name, capabilities, mode, issued_at, expires_at, revoked_at, status }
            })
            .collect(),
    ))
}

/// The operator who received a grant, or an admin, can revoke it. Anyone else is told 404.
async fn revoke_grant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((org, grant)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let who = authenticate(&state, &headers).await?;
    let role = require_role(&state, who.user_id, org, Role::Member).await?;
    let owner: Option<Uuid> = sqlx::query_scalar("SELECT operator_id FROM grants WHERE id = $1 AND org_id = $2")
        .bind(grant)
        .bind(org)
        .fetch_optional(&state.pool)
        .await?;
    let owner = owner.ok_or(ApiError::NotFound("grant not found"))?;
    if owner != who.user_id && role < Role::Admin {
        return Err(ApiError::NotFound("grant not found"));
    }
    let changed = sqlx::query("UPDATE grants SET revoked_at = now() WHERE id = $1 AND org_id = $2 AND revoked_at IS NULL")
        .bind(grant)
        .bind(org)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if changed != 1 {
        return Err(ApiError::NotFound("grant not found"));
    }
    audit::record(&state.pool, Some(org), Some(who.user_id), "grant.revoked", Some(grant.to_string()), serde_json::json!({})).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_ttl_is_within_what_devices_accept() {
        let (ttl, max) = (std::hint::black_box(GRANT_TTL_SECS), zherodizk_grant::MAX_LIFETIME_SECS);
        assert!(ttl > 0 && ttl <= max);
    }
}
