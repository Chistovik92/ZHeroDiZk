// SPDX-License-Identifier: AGPL-3.0-only
//! Organisations (tenants), memberships with roles and device enrolment tokens.
//! Every query below is scoped by organisation; a non-member gets 404, never a hint that
//! the organisation exists.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::{authenticate, new_session_token, token_digest, ApiError},
    AppState,
};

/// Lifetime of an enrolment token.
pub const ENROLLMENT_TTL_SECS: i64 = 24 * 3600;

/// Ordered from least to most powerful, so roles compare with `>=`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Member,
    Admin,
    Owner,
}

impl Role {
    pub fn parse(text: &str) -> Option<Role> {
        match text {
            "member" => Some(Role::Member),
            "admin" => Some(Role::Admin),
            "owner" => Some(Role::Owner),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Member => "member",
            Role::Admin => "admin",
            Role::Owner => "owner",
        }
    }
}

/// The caller's role in the organisation, or 404 when they are not a member, or 403 when the
/// role is lower than `minimum`.
pub async fn require_role(state: &AppState, user_id: Uuid, org_id: Uuid, minimum: Role) -> Result<Role, ApiError> {
    let found: Option<String> = sqlx::query_scalar("SELECT role FROM memberships WHERE org_id = $1 AND user_id = $2")
        .bind(org_id)
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await?;
    let role = found
        .as_deref()
        .and_then(Role::parse)
        .ok_or(ApiError::NotFound("organisation not found"))?;
    if role < minimum {
        return Err(ApiError::Forbidden("your role does not allow this"));
    }
    Ok(role)
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/orgs", post(create_org).get(list_orgs))
        .route("/v1/orgs/:org/members", post(add_member))
        .route("/v1/orgs/:org/enrollment-tokens", post(create_enrollment_token))
}

pub fn validate_name(raw: &str) -> Result<String, ApiError> {
    let name = raw.trim().to_owned();
    if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
        return Err(ApiError::Invalid("name must be 1-100 characters without control characters".into()));
    }
    Ok(name)
}

#[derive(Deserialize)]
struct NameBody {
    name: String,
}

#[derive(Serialize)]
struct OrgOut {
    id: Uuid,
    name: String,
    role: String,
}

async fn create_org(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<NameBody>,
) -> Result<(StatusCode, Json<OrgOut>), ApiError> {
    let who = authenticate(&state, &headers).await?;
    let name = validate_name(&body.name)?;
    let id = Uuid::new_v4();
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, $2)")
        .bind(id)
        .bind(&name)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO memberships (org_id, user_id, role) VALUES ($1, $2, 'owner')")
        .bind(id)
        .bind(who.user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(OrgOut { id, name, role: Role::Owner.as_str().into() })))
}

async fn list_orgs(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Vec<OrgOut>>, ApiError> {
    let who = authenticate(&state, &headers).await?;
    let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT o.id, o.name, m.role FROM memberships m JOIN organizations o ON o.id = m.org_id \
         WHERE m.user_id = $1 ORDER BY o.created_at, o.id",
    )
    .bind(who.user_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows.into_iter().map(|(id, name, role)| OrgOut { id, name, role }).collect()))
}

#[derive(Deserialize)]
struct MemberBody {
    email: String,
    role: String,
}

/// Admins may add members; only an owner may add admins. Owners are created with the
/// organisation and cannot be added here.
async fn add_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(org): Path<Uuid>,
    Json(body): Json<MemberBody>,
) -> Result<StatusCode, ApiError> {
    let who = authenticate(&state, &headers).await?;
    let caller = require_role(&state, who.user_id, org, Role::Admin).await?;
    let wanted = match Role::parse(&body.role) {
        Some(role @ (Role::Member | Role::Admin)) => role,
        _ => return Err(ApiError::Invalid("role must be member or admin".into())),
    };
    if wanted == Role::Admin && caller != Role::Owner {
        return Err(ApiError::Forbidden("only an owner can add administrators"));
    }
    let email = crate::auth::normalize_email(&body.email)?;
    let user: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE email = $1 AND is_active")
        .bind(&email)
        .fetch_optional(&state.pool)
        .await?;
    let user = user.ok_or(ApiError::NotFound("user not found"))?;
    let inserted = sqlx::query("INSERT INTO memberships (org_id, user_id, role) VALUES ($1, $2, $3)")
        .bind(org)
        .bind(user)
        .bind(wanted.as_str())
        .execute(&state.pool)
        .await;
    match inserted {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            Err(ApiError::Conflict("the user is already a member"))
        }
        Err(other) => Err(other.into()),
    }
}

#[derive(Serialize)]
struct TokenOut {
    token: String,
    expires_at: DateTime<Utc>,
}

async fn create_enrollment_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(org): Path<Uuid>,
) -> Result<(StatusCode, Json<TokenOut>), ApiError> {
    let who = authenticate(&state, &headers).await?;
    require_role(&state, who.user_id, org, Role::Admin).await?;
    let token = new_session_token();
    let expires_at: DateTime<Utc> = sqlx::query_scalar(
        "INSERT INTO enrollment_tokens (id, org_id, token_hash, created_by, expires_at) \
         VALUES ($1, $2, $3, $4, now() + make_interval(secs => $5)) RETURNING expires_at",
    )
    .bind(Uuid::new_v4())
    .bind(org)
    .bind(token_digest(&token))
    .bind(who.user_id)
    .bind(ENROLLMENT_TTL_SECS as f64)
    .fetch_one(&state.pool)
    .await?;
    Ok((StatusCode::CREATED, Json(TokenOut { token, expires_at })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_order_and_round_trip() {
        assert!(Role::Owner > Role::Admin && Role::Admin > Role::Member);
        for role in [Role::Member, Role::Admin, Role::Owner] {
            assert_eq!(Role::parse(role.as_str()), Some(role));
        }
        assert_eq!(Role::parse("root"), None);
        assert_eq!(Role::parse("Owner"), None);
    }

    #[test]
    fn names_are_trimmed_and_limited() {
        assert_eq!(validate_name("  Team A ").unwrap(), "Team A");
        assert!(validate_name("   ").is_err());
        assert!(validate_name(&"x".repeat(101)).is_err());
        assert!(validate_name("bad\nname").is_err());
        assert!(validate_name(&"я".repeat(100)).is_ok());
    }
}
