// SPDX-License-Identifier: AGPL-3.0-only
//! ZHeroDiZk control server: configuration, database, health check, local accounts with sessions
//! and MFA, organisations, device enrolment, groups, access rules, audit log and signed session
//! grants. Nothing here connects anyone to a device: a grant is only a statement of permission
//! that the device agent must verify.

pub mod audit;
pub mod auth;
pub mod config;
pub mod crypto;
pub mod devices;
pub mod grants;
pub mod groups;
pub mod limiter;
pub mod orgs;
pub mod password;
pub mod totp;

use std::sync::{atomic::AtomicI64, Arc};

use axum::{extract::State, http::StatusCode, routing::get, Json, Router};
use serde::Serialize;
use sqlx::{postgres::PgPoolOptions, Connection, PgConnection, PgPool};

/// Account-related limits. Defaults: registration closed after the first account,
/// 12-hour sessions, 5 failed logins lock the account for 15 minutes.
#[derive(Clone)]
pub struct AuthSettings {
    pub allow_registration: bool,
    pub session_ttl_secs: i64,
    pub max_failed_attempts: i32,
    pub lockout_secs: i64,
    /// Key that encrypts TOTP secrets at rest; without it MFA endpoints answer 503.
    pub mfa_key: Option<[u8; crypto::KEY_LEN]>,
    /// Seed of the Ed25519 key that signs session grants; without it grant endpoints answer 503.
    pub grant_key: Option<[u8; 32]>,
    /// Requests per minute and address allowed on anonymous endpoints (register, login, second
    /// factor, device enrolment); 0 turns the limit off.
    pub auth_rate_limit: u32,
    /// Take the client address from `X-Forwarded-For` (only behind a trusted reverse proxy).
    pub trust_forwarded_for: bool,
    /// Seconds added to the real clock when checking one-time codes. Always 0 in production;
    /// tests move it to step through TOTP periods without waiting.
    pub time_offset: Arc<AtomicI64>,
}

impl std::fmt::Debug for AuthSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthSettings")
            .field("allow_registration", &self.allow_registration)
            .field("session_ttl_secs", &self.session_ttl_secs)
            .field("max_failed_attempts", &self.max_failed_attempts)
            .field("lockout_secs", &self.lockout_secs)
            .field("mfa_key", &self.mfa_key.map(|_| "<redacted>"))
            .field("grant_key", &self.grant_key.map(|_| "<redacted>"))
            .finish()
    }
}

impl Default for AuthSettings {
    fn default() -> Self {
        Self {
            allow_registration: false,
            session_ttl_secs: 12 * 3600,
            max_failed_attempts: 5,
            lockout_secs: 15 * 60,
            mfa_key: None,
            grant_key: None,
            auth_rate_limit: 30,
            trust_forwarded_for: false,
            time_offset: Arc::new(AtomicI64::new(0)),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub settings: AuthSettings,
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    database: &'static str,
}

pub fn router(state: AppState) -> Router {
    let max = if state.settings.auth_rate_limit == 0 { u32::MAX } else { state.settings.auth_rate_limit };
    let limit = limiter::LimitState {
        limiter: Arc::new(limiter::RateLimiter::new(max, 60)),
        trust_forwarded_for: state.settings.trust_forwarded_for,
        time_offset: state.settings.time_offset.clone(),
    };
    let anonymous = auth::anonymous_routes()
        .merge(devices::anonymous_routes())
        .layer(axum::middleware::from_fn_with_state(limit, limiter::limit_anonymous));
    Router::new()
        .route("/healthz", get(healthz))
        .merge(anonymous)
        .merge(auth::routes())
        .merge(orgs::routes())
        .merge(devices::routes())
        .merge(groups::routes())
        .merge(audit::routes())
        .merge(grants::routes())
        .with_state(state)
}

async fn healthz(State(state): State<AppState>) -> (StatusCode, Json<Health>) {
    match sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&state.pool).await {
        Ok(_) => (StatusCode::OK, Json(Health { status: "ok", database: "ok" })),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(Health { status: "degraded", database: "error" }),
        ),
    }
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub async fn connect_and_migrate(database_url: &str) -> Result<PgPool, BoxError> {
    let pool = PgPoolOptions::new().max_connections(10).connect(database_url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

/// Same as [`connect_and_migrate`] but inside a dedicated schema; used by tests so that
/// every test works on an empty database.
pub async fn connect_and_migrate_in_schema(database_url: &str, schema: &str) -> Result<PgPool, BoxError> {
    if schema.is_empty() || !schema.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
        return Err("schema name must consist of [a-z0-9_]".into());
    }
    let mut connection = PgConnection::connect(database_url).await?;
    sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    let search_path = format!("SET search_path TO {schema}");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .after_connect(move |conn, _meta| {
            let statement = search_path.clone();
            Box::pin(async move {
                sqlx::query(&statement).execute(&mut *conn).await?;
                Ok(())
            })
        })
        .connect(database_url)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

/// Every route the server exposes as (method, OpenAPI path). `docs/api/openapi.yaml` must
/// describe exactly these; a test keeps the two in step.
pub const API_ROUTES: &[(&str, &str)] = &[
    ("GET", "/healthz"),
    ("POST", "/v1/auth/register"),
    ("POST", "/v1/auth/login"),
    ("POST", "/v1/auth/login/mfa"),
    ("POST", "/v1/auth/logout"),
    ("GET", "/v1/auth/me"),
    ("POST", "/v1/auth/password"),
    ("POST", "/v1/auth/mfa/enroll"),
    ("POST", "/v1/auth/mfa/confirm"),
    ("POST", "/v1/auth/mfa/disable"),
    ("POST", "/v1/orgs"),
    ("GET", "/v1/orgs"),
    ("GET", "/v1/orgs/{org}/members"),
    ("POST", "/v1/orgs/{org}/members"),
    ("POST", "/v1/orgs/{org}/enrollment-tokens"),
    ("GET", "/v1/orgs/{org}/audit"),
    ("GET", "/v1/grants/public-key"),
    ("GET", "/v1/orgs/{org}/grants"),
    ("POST", "/v1/orgs/{org}/devices/{device}/grants"),
    ("POST", "/v1/orgs/{org}/grants/{grant}/revoke"),
    ("POST", "/v1/devices/enroll"),
    ("GET", "/v1/orgs/{org}/devices"),
    ("POST", "/v1/orgs/{org}/devices/{device}/revoke"),
    ("GET", "/v1/orgs/{org}/devices/{device}/access"),
    ("POST", "/v1/orgs/{org}/groups"),
    ("GET", "/v1/orgs/{org}/groups"),
    ("POST", "/v1/orgs/{org}/groups/{group}/devices"),
    ("DELETE", "/v1/orgs/{org}/groups/{group}/devices/{device}"),
    ("GET", "/v1/orgs/{org}/acl"),
    ("PUT", "/v1/orgs/{org}/acl"),
    ("DELETE", "/v1/orgs/{org}/acl/{user}/{group}"),
    ("GET", "/v1/orgs/{org}/address-book"),
    ("PUT", "/v1/orgs/{org}/address-book/{device}"),
    ("DELETE", "/v1/orgs/{org}/address-book/{device}"),
];
