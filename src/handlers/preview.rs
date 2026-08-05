use axum::Json;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::{AppError, AppResult};
use crate::models::club::LogLevel;
use crate::models::role::Role;
use crate::models::user::{ErrorBody, OkResponse, PublicUser};
use crate::state::AppState;
use axum::extract::State;

#[derive(Debug, Deserialize, ToSchema)]
pub struct PreviewStartBody {
    pub user_id: String,
}

#[utoipa::path(
    post,
    path = "/api/admin/preview/start",
    request_body = PreviewStartBody,
    responses(
        (status = 200, description = "Rozpoczęto podgląd konta", body = PublicUser),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Użytkownik nie istnieje", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
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

    if !target.is_active {
        return Err(AppError::Forbidden(
            "Konto podglądane jest nieaktywne.".into(),
        ));
    }
    if target.id == auth.user.id {
        return Err(AppError::BadRequest(
            "Nie można podglądać własnego konta.".into(),
        ));
    }

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

#[utoipa::path(
    post,
    path = "/api/admin/preview/stop",
    responses(
        (status = 200, description = "Zakończono podgląd", body = OkResponse),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn preview_stop(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<OkResponse>> {
    ensure_roles(&auth, &[Role::Superadmin])?;
    state.db.append_log(
        LogLevel::Info,
        "preview",
        &format!("Superadmin {} zakończył podgląd", auth.user.email),
        Some(&auth.user.id),
    ).await?;
    Ok(Json(OkResponse { ok: true }))
}
