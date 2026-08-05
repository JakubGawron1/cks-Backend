use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::{AppError, AppResult};
use crate::models::club::{FeatureFlag, LogLevel, PublicFlag};
use crate::models::role::Role;
use crate::models::user::ErrorBody;
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateFlagBody {
    pub enabled: bool,
}

#[utoipa::path(
    get,
    path = "/api/admin/flags",
    responses(
        (status = 200, description = "Lista flag funkcjonalności", body = Vec<FeatureFlag>),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_flags(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<FeatureFlag>>> {
    ensure_roles(&auth, &[Role::Superadmin])?;
    Ok(Json(state.db.list_flags().await?))
}

#[utoipa::path(
    get,
    path = "/api/flags/public",
    responses(
        (status = 200, description = "Publiczne flagi funkcjonalności", body = Vec<PublicFlag>),
    )
)]
pub async fn list_public_flags(State(state): State<AppState>) -> AppResult<Json<Vec<PublicFlag>>> {
    /// Flagi czytane po stronie klienta (witryna + panele).
    const PUBLIC_FLAG_KEYS: &[&str] = &[
        "public_blog",
        "announcements_board",
        "public_calendar",
        "club_calendar",
        "athlete_calendar",
        "ui_toasts",
        "experimental_panel_themes",
        "experimental_notification_emails",
    ];

    let flags = state.db.list_flags().await?;
    let public: Vec<PublicFlag> = flags
        .into_iter()
        .filter(|f| PUBLIC_FLAG_KEYS.contains(&f.key.as_str()))
        .map(|f| PublicFlag {
            key: f.key,
            enabled: f.enabled,
        })
        .collect();
    Ok(Json(public))
}

#[utoipa::path(
    patch,
    path = "/api/admin/flags/{key}",
    params(("key" = String, Path, description = "Klucz flagi")),
    request_body = UpdateFlagBody,
    responses(
        (status = 200, description = "Zaktualizowano flagę", body = FeatureFlag),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Flaga nie istnieje", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_flag(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(key): Path<String>,
    Json(body): Json<UpdateFlagBody>,
) -> AppResult<Json<FeatureFlag>> {
    ensure_roles(&auth, &[Role::Superadmin])?;

    let mut flags = state.db.list_flags().await?;
    let flag = flags
        .iter_mut()
        .find(|f| f.key == key)
        .ok_or_else(|| AppError::NotFound("Flaga nie istnieje.".into()))?;

    flag.enabled = body.enabled;
    flag.updated_at = chrono::Utc::now().to_rfc3339();
    let updated = flag.clone();
    state.db.upsert_flag(updated.clone()).await?;
    state.db.append_log(
        LogLevel::Info,
        "flags",
        &format!(
            "Flaga {} = {}",
            updated.key,
            if updated.enabled { "on" } else { "off" }
        ),
        Some(&auth.user.id),
    ).await?;
    Ok(Json(updated))
}
