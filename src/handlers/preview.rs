use axum::Json;
use serde::Deserialize;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::{AppError, AppResult};
use crate::models::club::LogLevel;
use crate::models::role::Role;
use crate::models::user::PublicUser;
use crate::state::AppState;
use axum::extract::State;

#[derive(Debug, Deserialize)]
pub struct PreviewStartBody {
    pub user_id: String,
}

pub async fn preview_start(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<PreviewStartBody>,
) -> AppResult<Json<PublicUser>> {
    ensure_roles(&auth, &[Role::Superadmin])?;

    let target = state
        .db
        .find_user_by_id(&body.user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Użytkownik nie istnieje.".into()))?;

    state.db.append_log(
        LogLevel::Info,
        "preview",
        &format!(
            "Superadmin {} rozpoczął podgląd konta {}",
            auth.user.email, target.email
        ),
        Some(&auth.user.id),
    ).await?;

    Ok(Json(PublicUser::from(&target)))
}

pub async fn preview_stop(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<serde_json::Value>> {
    ensure_roles(&auth, &[Role::Superadmin])?;
    state.db.append_log(
        LogLevel::Info,
        "preview",
        &format!("Superadmin {} zakończył podgląd", auth.user.email),
        Some(&auth.user.id),
    ).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
