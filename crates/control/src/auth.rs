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

use crate::{crypto, password, totp, AppState};
use std::time::{SystemTime, UNIX_EPOCH};
use sqlx::PgConnection;
use std::sync::atomic::Ordering;

/// Serialises first-user detection with user creation.
const REGISTRATION_LOCK: i64 = 7_300_001;
/// Lifetime of the intermediate session between the password and the second factor.
const MFA_STEP_TTL_SECS: i64 = 300;
const RECOVERY_CODE_COUNT: usize = 10;
const ISSUER: &str = "ZHeroDiZk";

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    Invalid(String),
    #[error("invalid credentials")]
    Unauthorized,
    #[error("{0}")]
    Forbidden(&'static str),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("{0}")]
    Unavailable(&'static str),
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
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            ApiError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
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
#[serde(untagged)]
enum LoginResponse {
    Session { token: String, expires_at: DateTime<Utc> },
    MfaRequired { mfa_required: bool, mfa_token: String, expires_at: DateTime<Utc> },
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/auth/register", post(register))
        .route("/v1/auth/login", post(login))
        .route("/v1/auth/logout", post(logout))
        .route("/v1/auth/me", get(me))
        .route("/v1/auth/login/mfa", post(login_mfa))
        .route("/v1/auth/mfa/enroll", post(mfa_enroll))
        .route("/v1/auth/mfa/confirm", post(mfa_confirm))
        .route("/v1/auth/mfa/disable", post(mfa_disable))
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
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => return Err(ApiError::Conflict("an account with this email already exists")),
        Err(other) => return Err(other.into()),
    }
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(UserOut { id, email })))
}

type LoginRow = (Uuid, String, i32, Option<DateTime<Utc>>, bool, bool);

pub fn unix_now(state: &AppState) -> u64 {
    let real = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    real.saturating_add_signed(state.settings.time_offset.load(Ordering::Relaxed))
}

/// Count a failed login step and lock the account when the limit is reached.
async fn register_failure(state: &AppState, user_id: Uuid) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE users SET failed_attempts = failed_attempts + 1,          locked_until = CASE WHEN failed_attempts + 1 >= $2                              THEN now() + make_interval(secs => $3) ELSE locked_until END          WHERE id = $1",
    )
    .bind(user_id)
    .bind(state.settings.max_failed_attempts)
    .bind(state.settings.lockout_secs as f64)
    .execute(&state.pool)
    .await?;
    Ok(())
}

/// Insert a session and return the one-time token and its expiry.
async fn create_session(
    conn: &mut PgConnection,
    user_id: Uuid,
    kind: &str,
    ttl_secs: i64,
) -> Result<(String, DateTime<Utc>), ApiError> {
    let token = new_session_token();
    let expires_at: DateTime<Utc> = sqlx::query_scalar(
        "INSERT INTO sessions (id, user_id, token_hash, expires_at, kind)          VALUES ($1, $2, $3, now() + make_interval(secs => $4), $5) RETURNING expires_at",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(token_digest(&token))
    .bind(ttl_secs as f64)
    .bind(kind)
    .fetch_one(conn)
    .await?;
    Ok((token, expires_at))
}

async fn login(
    State(state): State<AppState>,
    Json(body): Json<Credentials>,
) -> Result<Json<LoginResponse>, ApiError> {
    // Every failure, including a malformed email, is the same 401.
    let Ok(email) = normalize_email(&body.email) else {
        verify_blocking(None, body.password).await?;
        return Err(ApiError::Unauthorized);
    };
    let row: Option<LoginRow> = sqlx::query_as(
        "SELECT id, password_hash, failed_attempts, locked_until, is_active, mfa_enabled          FROM users WHERE email = $1",
    )
    .bind(&email)
    .fetch_optional(&state.pool)
    .await?;

    let Some((user_id, stored_hash, _failed, locked_until, active, mfa_enabled)) = row else {
        verify_blocking(None, body.password).await?;
        return Err(ApiError::Unauthorized);
    };
    let locked = locked_until.is_some_and(|until| until > Utc::now());
    if locked || !active {
        verify_blocking(None, body.password).await?;
        return Err(ApiError::Unauthorized);
    }
    if !verify_blocking(Some(stored_hash), body.password).await? {
        register_failure(&state, user_id).await?;
        return Err(ApiError::Unauthorized);
    }

    let mut tx = state.pool.begin().await?;
    if mfa_enabled {
        // The failure counter is reset only after the second step succeeds.
        let (mfa_token, expires_at) = create_session(&mut tx, user_id, "mfa_pending", MFA_STEP_TTL_SECS).await?;
        tx.commit().await?;
        return Ok(Json(LoginResponse::MfaRequired { mfa_required: true, mfa_token, expires_at }));
    }
    sqlx::query("UPDATE users SET failed_attempts = 0, locked_until = NULL WHERE id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let (token, expires_at) = create_session(&mut tx, user_id, "full", state.settings.session_ttl_secs).await?;
    tx.commit().await?;
    Ok(Json(LoginResponse::Session { token, expires_at }))
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
         WHERE s.token_hash = $1 AND s.kind = 'full' AND s.revoked_at IS NULL \n         AND s.expires_at > now() AND u.is_active",
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

fn mfa_key(state: &AppState) -> Result<&[u8; crypto::KEY_LEN], ApiError> {
    state
        .settings
        .mfa_key
        .as_ref()
        .ok_or(ApiError::Unavailable("multi-factor authentication is not configured on this server"))
}

fn recovery_digest(code: &str) -> Vec<u8> {
    let normalised: String = code
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    Sha256::digest(normalised.as_bytes()).to_vec()
}

/// `xxxxx-xxxxx` codes from an alphabet without look-alike characters (rejection sampling,
/// so every character is uniformly distributed).
pub fn new_recovery_code() -> String {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let limit = 256 - (256 % ALPHABET.len());
    let mut code = String::with_capacity(11);
    let mut buffer = [0u8; 1];
    while code.len() < 11 {
        if code.len() == 5 {
            code.push('-');
            continue;
        }
        OsRng.fill_bytes(&mut buffer);
        if (buffer[0] as usize) < limit {
            code.push(ALPHABET[buffer[0] as usize % ALPHABET.len()] as char);
        }
    }
    code
}

#[derive(Serialize)]
struct EnrollOut {
    secret: String,
    otpauth_uri: String,
}

#[derive(Deserialize)]
struct CodeBody {
    code: String,
}

#[derive(Serialize)]
struct RecoveryOut {
    recovery_codes: Vec<String>,
}

/// Start enrolment: a new secret is stored (encrypted) but not yet active.
async fn mfa_enroll(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<EnrollOut>, ApiError> {
    let who = authenticate(&state, &headers).await?;
    let key = mfa_key(&state)?;
    let enabled: bool = sqlx::query_scalar("SELECT mfa_enabled FROM users WHERE id = $1")
        .bind(who.user_id)
        .fetch_one(&state.pool)
        .await?;
    if enabled {
        return Err(ApiError::Conflict("multi-factor authentication is already enabled"));
    }
    let secret = totp::generate_secret();
    let sealed = crypto::seal(key, &secret).map_err(|_| ApiError::Internal)?;
    sqlx::query("UPDATE users SET mfa_secret_enc = $2 WHERE id = $1")
        .bind(who.user_id)
        .bind(sealed)
        .execute(&state.pool)
        .await?;
    Ok(Json(EnrollOut {
        secret: totp::secret_to_base32(&secret),
        otpauth_uri: totp::otpauth_uri(ISSUER, &who.email, &secret),
    }))
}

/// Finish enrolment with a valid code; returns the recovery codes once.
async fn mfa_confirm(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CodeBody>,
) -> Result<Json<RecoveryOut>, ApiError> {
    let who = authenticate(&state, &headers).await?;
    let key = mfa_key(&state)?;
    let row: (Option<Vec<u8>>, bool) =
        sqlx::query_as("SELECT mfa_secret_enc, mfa_enabled FROM users WHERE id = $1")
            .bind(who.user_id)
            .fetch_one(&state.pool)
            .await?;
    if row.1 {
        return Err(ApiError::Conflict("multi-factor authentication is already enabled"));
    }
    let sealed = row.0.ok_or_else(|| ApiError::Invalid("start enrolment first".into()))?;
    let secret = crypto::open(key, &sealed).map_err(|_| ApiError::Internal)?;
    let step = totp::verify(&secret, &body.code, unix_now(&state), 1)
        .ok_or_else(|| ApiError::Invalid("code is not valid".into()))?;

    let codes: Vec<String> = (0..RECOVERY_CODE_COUNT).map(|_| new_recovery_code()).collect();
    let mut tx = state.pool.begin().await?;
    sqlx::query("UPDATE users SET mfa_enabled = true, mfa_last_step = $2 WHERE id = $1")
        .bind(who.user_id)
        .bind(step as i64)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM recovery_codes WHERE user_id = $1")
        .bind(who.user_id)
        .execute(&mut *tx)
        .await?;
    for code in &codes {
        sqlx::query("INSERT INTO recovery_codes (id, user_id, code_hash) VALUES ($1, $2, $3)")
            .bind(Uuid::new_v4())
            .bind(who.user_id)
            .bind(recovery_digest(code))
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(RecoveryOut { recovery_codes: codes }))
}

#[derive(Deserialize)]
struct MfaLogin {
    mfa_token: String,
    code: Option<String>,
    recovery_code: Option<String>,
}

/// Check a TOTP code and mark its step as used (a step is accepted only once, and never
/// an older one than the last accepted).
async fn consume_totp(state: &AppState, user_id: Uuid, sealed: &[u8], code: &str) -> Result<bool, ApiError> {
    let key = mfa_key(state)?;
    let secret = crypto::open(key, sealed).map_err(|_| ApiError::Internal)?;
    let Some(step) = totp::verify(&secret, code, unix_now(state), 1) else {
        return Ok(false);
    };
    let updated = sqlx::query("UPDATE users SET mfa_last_step = $2 WHERE id = $1 AND mfa_last_step < $2")
        .bind(user_id)
        .bind(step as i64)
        .execute(&state.pool)
        .await?
        .rows_affected();
    Ok(updated == 1)
}

async fn login_mfa(State(state): State<AppState>, Json(body): Json<MfaLogin>) -> Result<Json<LoginResponse>, ApiError> {
    type Pending = (Uuid, Uuid, Option<Vec<u8>>, Option<DateTime<Utc>>);
    let row: Option<Pending> = sqlx::query_as(
        "SELECT s.id, u.id, u.mfa_secret_enc, u.locked_until FROM sessions s JOIN users u ON u.id = s.user_id \
         WHERE s.token_hash = $1 AND s.kind = 'mfa_pending' AND s.revoked_at IS NULL \
         AND s.expires_at > now() AND u.is_active AND u.mfa_enabled",
    )
    .bind(token_digest(&body.mfa_token))
    .fetch_optional(&state.pool)
    .await?;
    let (pending_id, user_id, sealed, locked_until) = row.ok_or(ApiError::Unauthorized)?;
    if locked_until.is_some_and(|until| until > Utc::now()) {
        return Err(ApiError::Unauthorized);
    }

    let accepted = match (&body.code, &body.recovery_code) {
        (Some(code), None) => {
            let sealed = sealed.ok_or(ApiError::Internal)?;
            consume_totp(&state, user_id, &sealed, code).await?
        }
        (None, Some(recovery)) => {
            sqlx::query("UPDATE recovery_codes SET used_at = now() WHERE user_id = $1 AND code_hash = $2 AND used_at IS NULL")
                .bind(user_id)
                .bind(recovery_digest(recovery))
                .execute(&state.pool)
                .await?
                .rows_affected()
                == 1
        }
        _ => return Err(ApiError::Invalid("send either code or recovery_code".into())),
    };
    if !accepted {
        register_failure(&state, user_id).await?;
        return Err(ApiError::Unauthorized);
    }

    let mut tx = state.pool.begin().await?;
    sqlx::query("UPDATE sessions SET revoked_at = now() WHERE id = $1")
        .bind(pending_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE users SET failed_attempts = 0, locked_until = NULL WHERE id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let (token, expires_at) = create_session(&mut tx, user_id, "full", state.settings.session_ttl_secs).await?;
    tx.commit().await?;
    Ok(Json(LoginResponse::Session { token, expires_at }))
}

#[derive(Deserialize)]
struct DisableBody {
    password: String,
    code: String,
}

/// Turning MFA off needs the password and a fresh code, not just a session.
async fn mfa_disable(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<DisableBody>,
) -> Result<StatusCode, ApiError> {
    let who = authenticate(&state, &headers).await?;
    let row: (String, Option<Vec<u8>>, bool) =
        sqlx::query_as("SELECT password_hash, mfa_secret_enc, mfa_enabled FROM users WHERE id = $1")
            .bind(who.user_id)
            .fetch_one(&state.pool)
            .await?;
    let (hash, sealed, enabled) = row;
    if !enabled {
        return Err(ApiError::Conflict("multi-factor authentication is not enabled"));
    }
    let sealed = sealed.ok_or(ApiError::Internal)?;
    let password_ok = verify_blocking(Some(hash), body.password).await?;
    let code_ok = consume_totp(&state, who.user_id, &sealed, &body.code).await?;
    if !(password_ok && code_ok) {
        register_failure(&state, who.user_id).await?;
        return Err(ApiError::Unauthorized);
    }
    let mut tx = state.pool.begin().await?;
    sqlx::query("UPDATE users SET mfa_enabled = false, mfa_secret_enc = NULL, mfa_last_step = 0 WHERE id = $1")
        .bind(who.user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM recovery_codes WHERE user_id = $1")
        .bind(who.user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_codes_have_the_expected_shape() {
        let a = new_recovery_code();
        let b = new_recovery_code();
        assert_ne!(a, b);
        assert_eq!(a.len(), 11);
        assert_eq!(a.as_bytes()[5], b'-');
        assert!(a.chars().filter(|c| *c != '-').all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
        assert!(!a.contains(['0', '1', 'i', 'l', 'o']));
    }

    #[test]
    fn recovery_digest_ignores_case_and_separators() {
        assert_eq!(recovery_digest("ABCDE-23456"), recovery_digest("abcde 23456"));
        assert_ne!(recovery_digest("abcde-23456"), recovery_digest("abcde-23457"));
    }

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
