// SPDX-License-Identifier: AGPL-3.0-only
//! ZHeroDiZk control server. Stage 0.4.2: configuration, database, health check and local
//! accounts with sessions. There are no organisations, devices or remote-access rights yet.

pub mod auth;
pub mod config;
pub mod crypto;
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
    Router::new()
        .route("/healthz", get(healthz))
        .merge(auth::routes())
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
