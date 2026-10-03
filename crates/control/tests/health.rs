// SPDX-License-Identifier: AGPL-3.0-only
//! Needs a PostgreSQL server: set ZHD_TEST_DATABASE_URL (CI provides one).

use axum::{body::Body, http::Request, http::StatusCode};
use tower::ServiceExt;
use zherodizk_control::{connect_and_migrate, router, AppState, AuthSettings};

#[tokio::test]
async fn healthz_reports_database_ok_after_migration() {
    let Ok(url) = std::env::var("ZHD_TEST_DATABASE_URL") else {
        eprintln!("ZHD_TEST_DATABASE_URL is not set: skipping");
        return;
    };
    let pool = connect_and_migrate(&url).await.expect("connect and migrate");
    let row: (i64,) = sqlx::query_as("SELECT count(*) FROM schema_info")
        .fetch_one(&pool)
        .await
        .expect("schema_info exists");
    assert_eq!(row.0, 1);
    let app = router(AppState { pool, settings: AuthSettings::default() });
    let response = app
        .oneshot(Request::builder().uri("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn migrations_are_idempotent() {
    let Ok(url) = std::env::var("ZHD_TEST_DATABASE_URL") else {
        return;
    };
    connect_and_migrate(&url).await.expect("first run");
    connect_and_migrate(&url).await.expect("second run");
}
