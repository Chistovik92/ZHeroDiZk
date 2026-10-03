// SPDX-License-Identifier: AGPL-3.0-only
use base64::{engine::general_purpose::STANDARD, Engine};
use rand::{rngs::OsRng, RngCore};
use tracing_subscriber::EnvFilter;
use zherodizk_control::{config::Config, connect_and_migrate, router, AppState, AuthSettings};

/// Prints two fresh random keys in the form the settings expect. Nothing is stored.
fn generate_keys() {
    let key = || {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        STANDARD.encode(bytes)
    };
    println!("ZHD_MFA_KEY={}", key());
    println!("ZHD_GRANT_KEY={}", key());
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if std::env::args().nth(1).as_deref() == Some("generate-keys") {
        generate_keys();
        return Ok(());
    }
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    let config = Config::from_env()?;
    let pool = connect_and_migrate(&config.database_url).await?;
    let settings = AuthSettings {
        allow_registration: config.allow_registration,
        mfa_key: config.mfa_key,
        grant_key: config.grant_key,
        auth_rate_limit: config.auth_rate_limit,
        trust_forwarded_for: config.trust_forwarded_for,
        ..AuthSettings::default()
    };
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    tracing::info!(listen = %config.listen, "control server started");
    // The peer address is needed by the rate limiter.
    axum::serve(
        listener,
        router(AppState { pool, settings }).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
