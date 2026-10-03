// SPDX-License-Identifier: AGPL-3.0-only
//! Device groups, access rules (ACL) and personal address books.
//! Access is denied unless a rule grants it; being an owner or admin of the organisation
//! does not by itself allow connecting to a device.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{delete, get, post, put},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::{authenticate, ApiError},
    orgs::{require_role, validate_name, Role},
    AppState,
};

pub const CAPABILITIES: [&str; 4] = ["view", "input", "file_transfer", "clipboard"];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/orgs/:org/groups", post(create_group).get(list_groups))
        .route("/v1/orgs/:org/groups/:group/devices", post(add_group_device))
        .route("/v1/orgs/:org/groups/:group/devices/:device", delete(remove_group_device))
        .route("/v1/orgs/:org/acl", get(list_acl).put(put_acl))
        .route("/v1/orgs/:org/acl/:user/:group", delete(delete_acl))
        .route("/v1/orgs/:org/devices/:device/access", get(my_access))
        .route("/v1/orgs/:org/address-book", get(list_address_book))
        .route("/v1/orgs/:org/address-book/:device", put(put_address_book).delete(delete_address_book))
}

/// Sorted, de-duplicated, non-empty list of known capabilities.
pub fn normalize_capabilities(requested: &[String]) -> Result<Vec<String>, ApiError> {
    let mut out: Vec<String> = Vec::new();
    for item in requested {
        if !CAPABILITIES.contains(&item.as_str()) {
            return Err(ApiError::Invalid(format!("unknown capability: {item}")));
        }
        if !out.contains(item) {
            out.push(item.clone());
        }
    }
    if out.is_empty() {
        return Err(ApiError::Invalid("at least one capability is required".into()));
    }
    out.sort();
    Ok(out)
}

/// What `user` may do on `device`: the union of the capabilities of every rule whose group
/// contains the device. A revoked device yields nothing. Used later to build session grants.
pub async fn effective_capabilities(state: &AppState, user: Uuid, org: Uuid, device: Uuid) -> Result<Vec<String>, ApiError> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT c FROM acl_rules r \
         JOIN device_group_members m ON m.group_id = r.group_id AND m.org_id = r.org_id \
         JOIN devices d ON d.id = m.device_id AND d.org_id = m.org_id AND d.status = 'active' \
         CROSS JOIN LATERAL unnest(r.capabilities) AS c \
         WHERE r.user_id = $1 AND r.org_id = $2 AND m.device_id = $3 ORDER BY c",
    )
    .bind(user)
    .bind(org)
    .bind(device)
    .fetch_all(&state.pool)
    .await?;
    Ok(rows)
}

async fn group_in_org(state: &AppState, org: Uuid, group: Uuid) -> Result<(), ApiError> {
    let found: Option<i32> = sqlx::query_scalar("SELECT 1 FROM device_groups WHERE id = $1 AND org_id = $2")
        .bind(group)
        .bind(org)
        .fetch_optional(&state.pool)
        .await?;
    found.map(|_| ()).ok_or(ApiError::NotFound("group not found"))
}

async fn device_in_org(state: &AppState, org: Uuid, device: Uuid) -> Result<(), ApiError> {
    let found: Option<i32> = sqlx::query_scalar("SELECT 1 FROM devices WHERE id = $1 AND org_id = $2")
        .bind(device)
        .bind(org)
        .fetch_optional(&state.pool)
        .await?;
    found.map(|_| ()).ok_or(ApiError::NotFound("device not found"))
}

#[derive(Deserialize)]
struct NameBody {
    name: String,
}

#[derive(Serialize)]
struct GroupOut {
    id: Uuid,
    name: String,
    created_at: DateTime<Utc>,
}

async fn create_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(org): Path<Uuid>,
    Json(body): Json<NameBody>,
) -> Result<(StatusCode, Json<GroupOut>), ApiError> {
    let who = authenticate(&state, &headers).await?;
    require_role(&state, who.user_id, org, Role::Admin).await?;
    let name = validate_name(&body.name)?;
    let id = Uuid::new_v4();
    let inserted: Result<DateTime<Utc>, sqlx::Error> = sqlx::query_scalar(
        "INSERT INTO device_groups (id, org_id, name) VALUES ($1, $2, $3) RETURNING created_at",
    )
    .bind(id)
    .bind(org)
    .bind(&name)
    .fetch_one(&state.pool)
    .await;
    match inserted {
        Ok(created_at) => Ok((StatusCode::CREATED, Json(GroupOut { id, name, created_at }))),
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            Err(ApiError::Conflict("a group with this name already exists"))
        }
        Err(other) => Err(other.into()),
    }
}

async fn list_groups(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(org): Path<Uuid>,
) -> Result<Json<Vec<GroupOut>>, ApiError> {
    let who = authenticate(&state, &headers).await?;
    require_role(&state, who.user_id, org, Role::Member).await?;
    let rows: Vec<(Uuid, String, DateTime<Utc>)> =
        sqlx::query_as("SELECT id, name, created_at FROM device_groups WHERE org_id = $1 ORDER BY name")
            .bind(org)
            .fetch_all(&state.pool)
            .await?;
    Ok(Json(rows.into_iter().map(|(id, name, created_at)| GroupOut { id, name, created_at }).collect()))
}

#[derive(Deserialize)]
struct DeviceBody {
    device_id: Uuid,
}

async fn add_group_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((org, group)): Path<(Uuid, Uuid)>,
    Json(body): Json<DeviceBody>,
) -> Result<StatusCode, ApiError> {
    let who = authenticate(&state, &headers).await?;
    require_role(&state, who.user_id, org, Role::Admin).await?;
    group_in_org(&state, org, group).await?;
    device_in_org(&state, org, body.device_id).await?;
    sqlx::query("INSERT INTO device_group_members (group_id, device_id, org_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
        .bind(group)
        .bind(body.device_id)
        .bind(org)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_group_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((org, group, device)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let who = authenticate(&state, &headers).await?;
    require_role(&state, who.user_id, org, Role::Admin).await?;
    let removed = sqlx::query("DELETE FROM device_group_members WHERE group_id = $1 AND device_id = $2 AND org_id = $3")
        .bind(group)
        .bind(device)
        .bind(org)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if removed == 1 {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound("membership not found"))
    }
}

#[derive(Deserialize)]
struct AclBody {
    user_id: Uuid,
    group_id: Uuid,
    capabilities: Vec<String>,
}

#[derive(Serialize)]
struct AclOut {
    user_id: Uuid,
    group_id: Uuid,
    capabilities: Vec<String>,
}

async fn put_acl(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(org): Path<Uuid>,
    Json(body): Json<AclBody>,
) -> Result<Json<AclOut>, ApiError> {
    let who = authenticate(&state, &headers).await?;
    require_role(&state, who.user_id, org, Role::Admin).await?;
    let capabilities = normalize_capabilities(&body.capabilities)?;
    group_in_org(&state, org, body.group_id).await?;
    let member: Option<i32> = sqlx::query_scalar("SELECT 1 FROM memberships WHERE org_id = $1 AND user_id = $2")
        .bind(org)
        .bind(body.user_id)
        .fetch_optional(&state.pool)
        .await?;
    member.ok_or(ApiError::NotFound("user is not a member of the organisation"))?;
    sqlx::query(
        "INSERT INTO acl_rules (org_id, user_id, group_id, capabilities) VALUES ($1, $2, $3, $4) \
         ON CONFLICT (user_id, group_id) DO UPDATE SET capabilities = EXCLUDED.capabilities, updated_at = now()",
    )
    .bind(org)
    .bind(body.user_id)
    .bind(body.group_id)
    .bind(&capabilities)
    .execute(&state.pool)
    .await?;
    Ok(Json(AclOut { user_id: body.user_id, group_id: body.group_id, capabilities }))
}

async fn list_acl(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(org): Path<Uuid>,
) -> Result<Json<Vec<AclOut>>, ApiError> {
    let who = authenticate(&state, &headers).await?;
    require_role(&state, who.user_id, org, Role::Admin).await?;
    let rows: Vec<(Uuid, Uuid, Vec<String>)> =
        sqlx::query_as("SELECT user_id, group_id, capabilities FROM acl_rules WHERE org_id = $1 ORDER BY user_id, group_id")
            .bind(org)
            .fetch_all(&state.pool)
            .await?;
    Ok(Json(rows.into_iter().map(|(user_id, group_id, capabilities)| AclOut { user_id, group_id, capabilities }).collect()))
}

async fn delete_acl(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((org, user, group)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let who = authenticate(&state, &headers).await?;
    require_role(&state, who.user_id, org, Role::Admin).await?;
    let removed = sqlx::query("DELETE FROM acl_rules WHERE org_id = $1 AND user_id = $2 AND group_id = $3")
        .bind(org)
        .bind(user)
        .bind(group)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if removed == 1 {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound("rule not found"))
    }
}

#[derive(Serialize)]
struct AccessOut {
    device_id: Uuid,
    capabilities: Vec<String>,
}

/// The caller's own effective capabilities on a device (empty means no access).
async fn my_access(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((org, device)): Path<(Uuid, Uuid)>,
) -> Result<Json<AccessOut>, ApiError> {
    let who = authenticate(&state, &headers).await?;
    require_role(&state, who.user_id, org, Role::Member).await?;
    device_in_org(&state, org, device).await?;
    let capabilities = effective_capabilities(&state, who.user_id, org, device).await?;
    Ok(Json(AccessOut { device_id: device, capabilities }))
}

#[derive(Deserialize)]
struct AliasBody {
    alias: String,
}

#[derive(Serialize)]
struct EntryOut {
    device_id: Uuid,
    alias: String,
    device_name: String,
    device_status: String,
}

async fn list_address_book(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(org): Path<Uuid>,
) -> Result<Json<Vec<EntryOut>>, ApiError> {
    let who = authenticate(&state, &headers).await?;
    require_role(&state, who.user_id, org, Role::Member).await?;
    let rows: Vec<(Uuid, String, String, String)> = sqlx::query_as(
        "SELECT e.device_id, e.alias, d.name, d.status FROM address_book_entries e \
         JOIN devices d ON d.id = e.device_id AND d.org_id = e.org_id \
         WHERE e.user_id = $1 AND e.org_id = $2 ORDER BY e.alias, e.device_id",
    )
    .bind(who.user_id)
    .bind(org)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(device_id, alias, device_name, device_status)| EntryOut { device_id, alias, device_name, device_status })
            .collect(),
    ))
}

async fn put_address_book(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((org, device)): Path<(Uuid, Uuid)>,
    Json(body): Json<AliasBody>,
) -> Result<StatusCode, ApiError> {
    let who = authenticate(&state, &headers).await?;
    require_role(&state, who.user_id, org, Role::Member).await?;
    let alias = validate_name(&body.alias)?;
    device_in_org(&state, org, device).await?;
    sqlx::query(
        "INSERT INTO address_book_entries (user_id, device_id, org_id, alias) VALUES ($1, $2, $3, $4) \
         ON CONFLICT (user_id, device_id) DO UPDATE SET alias = EXCLUDED.alias",
    )
    .bind(who.user_id)
    .bind(device)
    .bind(org)
    .bind(alias)
    .execute(&state.pool)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_address_book(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((org, device)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let who = authenticate(&state, &headers).await?;
    require_role(&state, who.user_id, org, Role::Member).await?;
    let removed = sqlx::query("DELETE FROM address_book_entries WHERE user_id = $1 AND device_id = $2 AND org_id = $3")
        .bind(who.user_id)
        .bind(device)
        .bind(org)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if removed == 1 {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound("entry not found"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn capabilities_are_validated_sorted_and_deduplicated() {
        assert_eq!(
            normalize_capabilities(&caps(&["view", "input", "view"])).unwrap(),
            caps(&["input", "view"])
        );
        assert!(normalize_capabilities(&[]).is_err());
        assert!(normalize_capabilities(&caps(&["view", "root"])).is_err());
        assert!(normalize_capabilities(&caps(&["VIEW"])).is_err());
    }

    #[test]
    fn capability_names_match_the_database_constraint() {
        let migration = include_str!("../migrations/0005_groups_acl.sql");
        for capability in CAPABILITIES {
            assert!(migration.contains(&format!("'{capability}'")), "{capability}");
        }
    }
}
