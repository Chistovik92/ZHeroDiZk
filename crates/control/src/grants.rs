// SPDX-License-Identifier: AGPL-3.0-only
//! Session grants: after checking the operator's access rules, the server signs a short-lived
//! grant that the device agent verifies (see the `zherodizk-grant` crate). Stage 0.5.1.
//! A grant is only a statement of permission; nothing here connects anyone to a device.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
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
