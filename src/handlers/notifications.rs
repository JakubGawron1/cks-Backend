use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::auth::extractor::AuthUser;
use crate::error::{AppError, AppResult};
use crate::models::club::{Notification, UnreadCountResponse};
use crate::models::user::{ErrorBody, OkResponse};
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateNotificationBody {
    pub read: bool,
}

#[utoipa::path(
    get,
    path = "/api/notifications",
    responses(
        (status = 200, description = "Skrzynka powiadomień użytkownika", body = Vec<Notification>),
        (status = 401, description = "Unauthorized", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "notifications"
)]
pub async fn list_notifications(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<Notification>>> {
    Ok(Json(
        state
            .db
            .list_notifications_for_user(auth.effective_id())
            .await?,
    ))
}

#[utoipa::path(
    get,
    path = "/api/notifications/unread-count",
    responses(
        (status = 200, description = "Liczba nieprzeczytanych", body = UnreadCountResponse),
        (status = 401, description = "Unauthorized", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "notifications"
)]
pub async fn unread_count(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<UnreadCountResponse>> {
    let count = state
        .db
        .unread_notification_count(auth.effective_id())
        .await?;
    Ok(Json(UnreadCountResponse { count }))
}

#[utoipa::path(
    patch,
    path = "/api/notifications/{id}",
    params(("id" = String, Path, description = "ID powiadomienia")),
    request_body = UpdateNotificationBody,
    responses(
        (status = 200, description = "Zaktualizowano status odczytu", body = Notification),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Powiadomienie nie istnieje", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "notifications"
)]
pub async fn update_notification(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateNotificationBody>,
) -> AppResult<Json<Notification>> {
    let existing = state
        .db
        .get_notification(&id)
        .await?
        .ok_or_else(|| AppError::NotFound("Powiadomienie nie istnieje.".into()))?;

    if existing.user_id != auth.user.id {
        return Err(AppError::Forbidden(
            "Nie możesz zmieniać cudzych powiadomień.".into(),
        ));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let notification = Notification {
        read: body.read,
        read_at: if body.read { Some(now) } else { None },
        ..existing
    };

    state.db.upsert_notification(notification.clone()).await?;
    Ok(Json(notification))
}

#[utoipa::path(
    delete,
    path = "/api/notifications/{id}",
    params(("id" = String, Path, description = "ID powiadomienia")),
    responses(
        (status = 200, description = "Usunięto powiadomienie", body = OkResponse),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Powiadomienie nie istnieje", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "notifications"
)]
pub async fn delete_notification(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<OkResponse>> {
    let existing = state
        .db
        .get_notification(&id)
        .await?
        .ok_or_else(|| AppError::NotFound("Powiadomienie nie istnieje.".into()))?;

    if existing.user_id != auth.user.id {
        return Err(AppError::Forbidden(
            "Nie możesz usuwać cudzych powiadomień.".into(),
        ));
    }

    state.db.delete_notification(&id).await?;
    Ok(Json(OkResponse { ok: true }))
}

#[utoipa::path(
    post,
    path = "/api/notifications/mark-all-read",
    responses(
        (status = 200, description = "Oznaczono wszystkie jako przeczytane", body = OkResponse),
        (status = 401, description = "Unauthorized", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "notifications"
)]
pub async fn mark_all_read(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<OkResponse>> {
    state
        .db
        .mark_all_notifications_read(&auth.user.id)
        .await?;
    Ok(Json(OkResponse { ok: true }))
}
