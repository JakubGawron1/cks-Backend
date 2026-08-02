use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::{AppError, AppResult};
use crate::models::club::{FeatureFlag, LogLevel};
use crate::models::role::Role;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct UpdateFlagBody {
    pub enabled: bool,
}

pub async fn list_flags(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<FeatureFlag>>> {
    ensure_roles(&auth, &[Role::Superadmin])?;
    Ok(Json(state.db.list_flags().await?))
}

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
