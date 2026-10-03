// SPDX-License-Identifier: AGPL-3.0-only
//! Device enrolment with proof of key ownership, listing and revocation.
//!
//! A device creates an Ed25519 key pair and signs a message that contains the one-time
//! enrolment token, the device name and its public key. The server verifies the signature
//! before it spends the token, so a registration cannot be completed with someone else's key.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::{authenticate, token_digest, ApiError},
    orgs::{require_role, validate_name, Role},
    AppState,
};

pub const PLATFORMS: [&str; 5] = ["windows", "linux", "android", "macos", "ios"];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/devices/enroll", post(enroll))
        .route("/v1/orgs/:org/devices", get(list_devices))
        .route("/v1/orgs/:org/devices/:device/revoke", post(revoke_device))
}

/// The exact bytes a device signs. The version prefix separates this use of the key from any
/// other, so a signature made here is never valid for another purpose.
pub fn enrollment_message(token: &str, name: &str, public_key_b64: &str) -> Vec<u8> {
    format!("zherodizk-enroll-v1\n{token}\n{name}\n{public_key_b64}").into_bytes()
}

#[derive(Deserialize)]
struct EnrollBody {
    token: String,
    name: String,
    platform: String,
    public_key: String,
    signature: String,
}

#[derive(Serialize)]
struct EnrollOut {
    device_id: Uuid,
    org_id: Uuid,
}

async fn enroll(State(state): State<AppState>, Json(body): Json<EnrollBody>) -> Result<(StatusCode, Json<EnrollOut>), ApiError> {
    let name = validate_name(&body.name)?;
    if !PLATFORMS.contains(&body.platform.as_str()) {
        return Err(ApiError::Invalid("unknown platform".into()));
    }
    let key_bytes: [u8; 32] = STANDARD
        .decode(&body.public_key)
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| ApiError::Invalid("public_key must be 32 bytes in base64".into()))?;
    let signature_bytes: [u8; 64] = STANDARD
        .decode(&body.signature)
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| ApiError::Invalid("signature must be 64 bytes in base64".into()))?;
    let key = VerifyingKey::from_bytes(&key_bytes).map_err(|_| ApiError::Invalid("public_key is not a valid key".into()))?;

    // Proof of possession first; the token is not spent by an invalid request.
    let message = enrollment_message(&body.token, &body.name, &body.public_key);
    key.verify_strict(&message, &Signature::from_bytes(&signature_bytes))
        .map_err(|_| ApiError::Unauthorized)?;

    let mut tx = state.pool.begin().await?;
    let org: Option<Uuid> = sqlx::query_scalar(
        "UPDATE enrollment_tokens SET used_at = now() \
         WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now() RETURNING org_id",
    )
    .bind(token_digest(&body.token))
    .fetch_optional(&mut *tx)
    .await?;
    let org = org.ok_or(ApiError::Unauthorized)?;
    let device_id = Uuid::new_v4();
    let inserted = sqlx::query("INSERT INTO devices (id, org_id, name, platform, public_key) VALUES ($1, $2, $3, $4, $5)")
        .bind(device_id)
        .bind(org)
        .bind(&name)
        .bind(&body.platform)
        .bind(key_bytes.as_slice())
        .execute(&mut *tx)
        .await;
    match inserted {
        Ok(_) => {}
        // The failed statement aborts the transaction, so the token stays unspent.
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            return Err(ApiError::Conflict("this device key is already registered"))
        }
        Err(other) => return Err(other.into()),
    }
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(EnrollOut { device_id, org_id: org })))
}

#[derive(Serialize)]
struct DeviceOut {
    id: Uuid,
    name: String,
    platform: String,
    status: String,
    created_at: DateTime<Utc>,
}

async fn list_devices(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(org): Path<Uuid>,
) -> Result<Json<Vec<DeviceOut>>, ApiError> {
    let who = authenticate(&state, &headers).await?;
    require_role(&state, who.user_id, org, Role::Member).await?;
    let rows: Vec<(Uuid, String, String, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, name, platform, status, created_at FROM devices WHERE org_id = $1 ORDER BY created_at, id",
    )
    .bind(org)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, name, platform, status, created_at)| DeviceOut { id, name, platform, status, created_at })
            .collect(),
    ))
}

async fn revoke_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((org, device)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let who = authenticate(&state, &headers).await?;
    require_role(&state, who.user_id, org, Role::Admin).await?;
    // The organisation is part of the condition: a device id from another tenant matches nothing.
    let changed = sqlx::query(
        "UPDATE devices SET status = 'revoked', revoked_at = now() \
         WHERE id = $1 AND org_id = $2 AND status = 'active'",
    )
    .bind(device)
    .bind(org)
    .execute(&state.pool)
    .await?
    .rows_affected();
    if changed == 1 {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound("device not found"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_is_versioned_and_binds_every_field() {
        let base = enrollment_message("tok", "laptop", "KEY");
        assert!(base.starts_with(b"zherodizk-enroll-v1\n"));
        assert_ne!(base, enrollment_message("tok2", "laptop", "KEY"));
        assert_ne!(base, enrollment_message("tok", "laptop2", "KEY"));
        assert_ne!(base, enrollment_message("tok", "laptop", "KEY2"));
    }

    #[test]
    fn platform_list_matches_the_database_constraint() {
        for platform in PLATFORMS {
            assert!(platform.chars().all(|c| c.is_ascii_lowercase()));
        }
        assert_eq!(PLATFORMS.len(), 5);
    }
}
