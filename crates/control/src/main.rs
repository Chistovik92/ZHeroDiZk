// SPDX-License-Identifier: AGPL-3.0-only
use tracing_subscriber::EnvFilter;
use zherodizk_control::{config::Config, connect_and_migrate, router, AppState, AuthSettings};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    let config = Config::from_env()?;
    let pool = connect_and_migrate(&config.database_url).await?;
    let settings = AuthSettings {
        allow_registration: config.allow_registration,
        mfa_key: config.mfa_key,
        ..AuthSettings::default()
    };
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    tracing::info!(listen = %config.listen, "control server started");
    axum::serve(listener, router(AppState { pool, settings }))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
