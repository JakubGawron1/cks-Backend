use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::AppResult;
use crate::models::club::LogLevel;
use crate::models::role::Role;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct UpsertRowBody {
    pub row: Value,
}

pub async fn db_list_tables(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<&'static str>>> {
    ensure_roles(&auth, &[Role::Superadmin])?;
    Ok(Json(state.db.db_list_tables()))
}

pub async fn db_list_rows(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(table): Path<String>,
) -> AppResult<Json<Vec<Value>>> {
    ensure_roles(&auth, &[Role::Superadmin])?;
    Ok(Json(state.db.db_list_rows(&table)?))
}

pub async fn db_upsert_row(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(table): Path<String>,
    Json(body): Json<UpsertRowBody>,
) -> AppResult<Json<serde_json::Value>> {
    ensure_roles(&auth, &[Role::Superadmin])?;
    state.db.db_upsert_row(&table, body.row)?;
    state.db.append_log(
        LogLevel::Warn,
        "db_admin",
        &format!("Upsert w tabeli {table}"),
        Some(&auth.user.id),
    )?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn db_delete_row(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((table, id)): Path<(String, String)>,
) -> AppResult<Json<serde_json::Value>> {
    ensure_roles(&auth, &[Role::Superadmin])?;
    state.db.db_delete_row(&table, &id)?;
    state.db.append_log(
        LogLevel::Warn,
        "db_admin",
        &format!("Delete {id} z tabeli {table}"),
        Some(&auth.user.id),
    )?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
