// SPDX-License-Identifier: AGPL-3.0-only
//! Local accounts: registration, login with lockout, bearer sessions.
//! Stage 0.4.2. There are no organisations, MFA or device rights yet.

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Utc};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{password, AppState};

/// Serialises first-user detection with user creation.
const REGISTRATION_LOCK: i64 = 7_300_001;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    Invalid(String),
    #[error("invalid credentials")]
    Unauthorized,
    #[error("{0}")]
    Forbidden(&'static str),
    #[error("an account with this email already exists")]
    Conflict,
    #[error("internal error")]
    Internal,
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        tracing::error!(%error, "database error");
        ApiError::Internal
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self {
            ApiError::Invalid(_) => StatusCode::BAD_REQUEST,
            ApiError::Unauthorized => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden(_) => StatusCode::FORBIDDEN,
            ApiError::Conflict => StatusCode::CONFLICT,
            ApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(serde_json::json!({ "error": self.to_string() }))).into_response()
    }
}

#[derive(Deserialize)]
struct Credentials {
    email: String,
    password: String,
}

#[derive(Serialize)]
struct UserOut {
    id: Uuid,
    email: String,
}

#[derive(Serialize)]
struct LoginOut {
    token: String,
    expires_at: DateTime<Utc>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/auth/register", post(register))
        .route("/v1/auth/login", post(login))
        .route("/v1/auth/logout", post(logout))
        .route("/v1/auth/me", get(me))
}

/// Trim, lowercase and check the basic shape. This is not a full RFC 5322 parser:
/// the address is only an account name until mail delivery exists.
pub fn normalize_email(raw: &str) -> Result<String, ApiError> {
    let email = raw.trim().to_lowercase();
    let valid = (3..=254).contains(&email.len())
        && email.matches('@').count() == 1
        && !email.starts_with('@')
        && !email.ends_with('@')
        && !email.chars().any(|c| c.is_whitespace() || c.is_control());
    if valid {
        Ok(email)
    } else {
        Err(ApiError::Invalid("email address is not valid".into()))
    }
}

/// 256 random bits, URL-safe; only its SHA-256 is stored.
pub fn new_session_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn token_digest(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

async fn hash_blocking(plain: String) -> Result<String, ApiError> {
    tokio::task::spawn_blocking(move || password::hash(&plain))
        .await
        .map_err(|_| ApiError::Internal)?
        .map_err(|_| ApiError::Internal)
}

async fn verify_blocking(stored: Option<String>, plain: String) -> Result<bool, ApiError> {
    tokio::task::spawn_blocking(move || match stored {
        Some(hash) => password::verify(&hash, &plain),
        None => {
            password::verify_dummy(&plain);
            false
        }
    })
    .await
    .map_err(|_| ApiError::Internal)
}

async fn register(
    State(state): State<AppState>,
    Json(body): Json<Credentials>,
) -> Result<(StatusCode, Json<UserOut>), ApiError> {
    let email = normalize_email(&body.email)?;
    password::validate(&body.password).map_err(|e| ApiError::Invalid(e.to_string()))?;
    let hash = hash_blocking(body.password).await?;

    let mut tx = state.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(REGISTRATION_LOCK)
        .execute(&mut *tx)
        .await?;
    let existing: i64 = sqlx::query_scalar("SELECT count(*) FROM users").fetch_one(&mut *tx).await?;
    // The very first account (the administrator) may always be created.
    if existing > 0 && !state.settings.allow_registration {
        return Err(ApiError::Forbidden("registration is closed"));
    }
    let id = Uuid::new_v4();
    let inserted = sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(&email)
        .bind(&hash)
        .execute(&mut *tx)
        .await;
    match inserted {
        Ok(_) => {}
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => return Err(ApiError::Conflict),
        Err(other) => return Err(other.into()),
    }
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(UserOut { id, email })))
}

type LoginRow = (Uuid, String, i32, Option<DateTime<Utc>>, bool);

async fn login(
    State(state): State<AppState>,
    Json(body): Json<Credentials>,
) -> Result<Json<LoginOut>, ApiError> {
    // Every failure, including a malformed email, is the same 401.
    let Ok(email) = normalize_email(&body.email) else {
        verify_blocking(None, body.password).await?;
        return Err(ApiError::Unauthorized);
    };
    let row: Option<LoginRow> = sqlx::query_as(
        "SELECT id, password_hash, failed_attempts, locked_until, is_active FROM users WHERE email = $1",
    )
    .bind(&email)
    .fetch_optional(&state.pool)
    .await?;

    let Some((user_id, stored_hash, _failed, locked_until, active)) = row else {
        verify_blocking(None, body.password).await?;
        return Err(ApiError::Unauthorized);
    };
    let locked = locked_until.is_some_and(|until| until > Utc::now());
    if locked || !active {
        verify_blocking(None, body.password).await?;
        return Err(ApiError::Unauthorized);
    }
    if !verify_blocking(Some(stored_hash), body.password).await? {
        sqlx::query(
            "UPDATE users SET failed_attempts = failed_attempts + 1, \
             locked_until = CASE WHEN failed_attempts + 1 >= $2 \
                                 THEN now() + make_interval(secs => $3) ELSE locked_until END \
             WHERE id = $1",
        )
        .bind(user_id)
        .bind(state.settings.max_failed_attempts)
        .bind(state.settings.lockout_secs as f64)
        .execute(&state.pool)
        .await?;
        return Err(ApiError::Unauthorized);
    }

    let token = new_session_token();
    let mut tx = state.pool.begin().await?;
    sqlx::query("UPDATE users SET failed_attempts = 0, locked_until = NULL WHERE id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let expires_at: DateTime<Utc> = sqlx::query_scalar(
        "INSERT INTO sessions (id, user_id, token_hash, expires_at) \
         VALUES ($1, $2, $3, now() + make_interval(secs => $4)) RETURNING expires_at",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(token_digest(&token))
    .bind(state.settings.session_ttl_secs as f64)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(LoginOut { token, expires_at }))
}

pub struct Authenticated {
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub email: String,
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ")?.trim();
    (!token.is_empty()).then_some(token)
}

/// Resolve the bearer token to a live session of an active user.
pub async fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<Authenticated, ApiError> {
    let token = bearer_token(headers).ok_or(ApiError::Unauthorized)?;
    let row: Option<(Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT s.id, u.id, u.email FROM sessions s JOIN users u ON u.id = s.user_id \
         WHERE s.token_hash = $1 AND s.revoked_at IS NULL AND s.expires_at > now() AND u.is_active",
    )
    .bind(token_digest(token))
    .fetch_optional(&state.pool)
    .await?;
    let (session_id, user_id, email) = row.ok_or(ApiError::Unauthorized)?;
    Ok(Authenticated { session_id, user_id, email })
}

async fn me(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<UserOut>, ApiError> {
    let who = authenticate(&state, &headers).await?;
    Ok(Json(UserOut { id: who.user_id, email: who.email }))
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<StatusCode, ApiError> {
    let who = authenticate(&state, &headers).await?;
    sqlx::query("UPDATE sessions SET revoked_at = now() WHERE id = $1")
        .bind(who.session_id)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_is_normalised_and_checked() {
        assert_eq!(normalize_email("  User@Example.ORG ").unwrap(), "user@example.org");
        for bad in ["", "a", "no-at-sign", "two@@signs", "@lead.com", "trail@", "sp ace@x.org", "a@b\n.c"] {
            assert!(normalize_email(bad).is_err(), "{bad:?}");
        }
        assert!(normalize_email(&format!("{}@x.org", "a".repeat(250))).is_err());
    }

    #[test]
    fn tokens_are_long_unique_and_hashed_to_32_bytes() {
        let a = new_session_token();
        let b = new_session_token();
        assert_ne!(a, b);
        assert!(a.len() >= 43);
        assert_eq!(token_digest(&a).len(), 32);
        assert_ne!(token_digest(&a), token_digest(&b));
    }

    #[test]
    fn bearer_header_parsing() {
        let mut headers = HeaderMap::new();
        assert!(bearer_token(&headers).is_none());
        headers.insert(header::AUTHORIZATION, "Bearer abc".parse().unwrap());
        assert_eq!(bearer_token(&headers), Some("abc"));
        headers.insert(header::AUTHORIZATION, "Basic abc".parse().unwrap());
        assert!(bearer_token(&headers).is_none());
        headers.insert(header::AUTHORIZATION, "Bearer   ".parse().unwrap());
        assert!(bearer_token(&headers).is_none());
    }
}
