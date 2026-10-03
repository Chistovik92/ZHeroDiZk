// SPDX-License-Identifier: AGPL-3.0-only
//! Append-only audit log. Entries hold identifiers and names only: never passwords, tokens,
//! one-time codes, keys or recovery codes.
//!
//! An entry is written right after the action it describes, inside the same transaction
//! where the handler has one. Handlers that use single statements write it just afterwards;
//! if that write fails the request answers 500 although the action itself has happened.

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgExecutor;
use uuid::Uuid;

use crate::{
    auth::{authenticate, ApiError},
    orgs::{require_role, Role},
    AppState,
};

pub const DEFAULT_LIMIT: i64 = 50;
pub const MAX_LIMIT: i64 = 200;

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/orgs/:org/audit", get(list_events))
}

pub async fn record<'e, E: PgExecutor<'e>>(
    executor: E,
    org: Option<Uuid>,
    actor: Option<Uuid>,
    action: &str,
    target: Option<String>,
    detail: Value,
) -> Result<(), ApiError> {
    sqlx::query("INSERT INTO audit_events (org_id, actor_user_id, action, target, detail) VALUES ($1, $2, $3, $4, $5)")
        .bind(org)
        .bind(actor)
        .bind(action)
        .bind(target)
        .bind(detail)
        .execute(executor)
        .await?;
    Ok(())
}

#[derive(Deserialize)]
struct ListQuery {
    limit: Option<i64>,
    /// Return only entries older than this id (cursor for the next page).
    before: Option<i64>,
}

#[derive(Serialize)]
struct EventOut {
    id: i64,
    actor_user_id: Option<Uuid>,
    action: String,
    target: Option<String>,
    detail: Value,
    created_at: DateTime<Utc>,
}

pub fn clamp_limit(requested: Option<i64>) -> i64 {
    requested.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

async fn list_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(org): Path<Uuid>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let who = authenticate(&state, &headers).await?;
    require_role(&state, who.user_id, org, Role::Admin).await?;
    let limit = clamp_limit(query.limit);
    let rows: Vec<(i64, Option<Uuid>, String, Option<String>, Value, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, actor_user_id, action, target, detail, created_at FROM audit_events \
         WHERE org_id = $1 AND ($2::bigint IS NULL OR id < $2) ORDER BY id DESC LIMIT $3",
    )
    .bind(org)
    .bind(query.before)
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;
    let events: Vec<EventOut> = rows
        .into_iter()
        .map(|(id, actor_user_id, action, target, detail, created_at)| EventOut { id, actor_user_id, action, target, detail, created_at })
        .collect();
    let next = if events.len() as i64 == limit { events.last().map(|e| e.id) } else { None };
    Ok(Json(json!({ "events": events, "next_before": next })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_are_clamped() {
        assert_eq!(clamp_limit(None), 50);
        assert_eq!(clamp_limit(Some(0)), 1);
        assert_eq!(clamp_limit(Some(-5)), 1);
        assert_eq!(clamp_limit(Some(10_000)), 200);
        assert_eq!(clamp_limit(Some(7)), 7);
    }
}
