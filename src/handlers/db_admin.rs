use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use utoipa::ToSchema;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::AppResult;
use crate::models::club::LogLevel;
use crate::models::role::Role;
use crate::models::user::{ErrorBody, OkResponse};
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpsertRowBody {
    /// Dowolny obiekt JSON reprezentujący wiersz tabeli.
    #[schema(value_type = Object)]
    pub row: Value,
}

#[utoipa::path(
    get,
    path = "/api/admin/db/tables",
    responses(
        (status = 200, description = "Lista tabel zarządzanych przez panel admina", body = Vec<String>),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn db_list_tables(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<&'static str>>> {
    ensure_roles(&auth, &[Role::Superadmin])?;
    Ok(Json(state.db.db_list_tables()))
}

#[utoipa::path(
    get,
    path = "/api/admin/db/{table}",
    params(("table" = String, Path, description = "Nazwa tabeli")),
    responses(
        (status = 200, description = "Wiersze tabeli (dowolne obiekty JSON)", body = Vec<Value>),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn db_list_rows(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(table): Path<String>,
) -> AppResult<Json<Vec<Value>>> {
    ensure_roles(&auth, &[Role::Superadmin])?;
    Ok(Json(state.db.db_list_rows(&table).await?))
}

#[utoipa::path(
    post,
    path = "/api/admin/db/{table}",
    params(("table" = String, Path, description = "Nazwa tabeli")),
    request_body = UpsertRowBody,
    responses(
        (status = 200, description = "Zapisano wiersz", body = OkResponse),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn db_upsert_row(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(table): Path<String>,
    Json(body): Json<UpsertRowBody>,
) -> AppResult<Json<OkResponse>> {
    ensure_roles(&auth, &[Role::Superadmin])?;
    state.db.db_upsert_row(&table, body.row).await?;
    state.db.append_log(
        LogLevel::Warn,
        "db_admin",
        &format!("Upsert w tabeli {table}"),
        Some(&auth.user.id),
    ).await?;
    Ok(Json(OkResponse { ok: true }))
}

#[utoipa::path(
    delete,
    path = "/api/admin/db/{table}/{id}",
    params(
        ("table" = String, Path, description = "Nazwa tabeli"),
        ("id" = String, Path, description = "ID wiersza"),
    ),
    responses(
        (status = 200, description = "Usunięto wiersz", body = OkResponse),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn db_delete_row(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((table, id)): Path<(String, String)>,
) -> AppResult<Json<OkResponse>> {
    ensure_roles(&auth, &[Role::Superadmin])?;
    state.db.db_delete_row(&table, &id).await?;
    state.db.append_log(
        LogLevel::Warn,
        "db_admin",
        &format!("Delete {id} z tabeli {table}"),
        Some(&auth.user.id),
    ).await?;
    Ok(Json(OkResponse { ok: true }))
}
