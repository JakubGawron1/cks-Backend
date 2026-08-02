use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::{AppError, AppResult};
use crate::models::club::{AttendanceRecord, AttendanceSession, LogLevel};
use crate::models::role::Role;
use crate::models::user::ErrorBody;
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct CheckInBody {
    pub token: String,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct AttendanceQuery {
    pub user_id: Option<String>,
    pub day: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RefreshSessionBody {
    pub label: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/attendance/session",
    responses(
        (status = 200, description = "Aktualna sesja obecności (kod QR)", body = AttendanceSession),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Brak sesji obecności", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_session(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<AttendanceSession>> {
    ensure_roles(&auth, &[Role::Trener, Role::Admin])?;
    let session = state.db.get_attendance_session().await?.ok_or_else(|| {
        AppError::NotFound("Brak sesji obecności — odśwież kod QR.".into())
    })?;
    Ok(Json(session))
}

#[utoipa::path(
    post,
    path = "/api/attendance/session",
    request_body = RefreshSessionBody,
    responses(
        (status = 200, description = "Odświeżono kod QR obecności", body = AttendanceSession),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn refresh_session(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<RefreshSessionBody>,
) -> AppResult<Json<AttendanceSession>> {
    ensure_roles(&auth, &[Role::Trener, Role::Admin])?;
    let now = chrono::Utc::now().to_rfc3339();
    let prev = state.db.get_attendance_session().await?;
    let session = AttendanceSession {
        token: uuid::Uuid::new_v4().to_string(),
        label: body
            .label
            .or_else(|| prev.as_ref().map(|p| p.label.clone()))
            .unwrap_or_else(|| "Trening".into()),
        created_at: prev
            .as_ref()
            .map(|p| p.created_at.clone())
            .unwrap_or_else(|| now.clone()),
        refreshed_at: now,
    };
    state.db.set_attendance_session(session.clone()).await?;
    state.db.append_log(
        LogLevel::Info,
        "attendance",
        "Odświeżono kod QR obecności",
        Some(&auth.user.id),
    ).await?;
    Ok(Json(session))
}

#[utoipa::path(
    get,
    path = "/api/attendance",
    params(AttendanceQuery),
    responses(
        (status = 200, description = "Lista obecności", body = Vec<AttendanceRecord>),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_attendance(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<AttendanceQuery>,
) -> AppResult<Json<Vec<AttendanceRecord>>> {
    ensure_roles(&auth, &[Role::Trener, Role::Admin, Role::Zawodnik])?;
    let mut items = state.db.list_attendance_in_window().await?;

    // Zawodnik widzi tylko własne
    if !crate::models::role::has_any_role(auth.roles(), &[Role::Trener, Role::Admin]) {
        items.retain(|r| r.user_id == auth.user.id);
    } else if let Some(uid) = query.user_id {
        items.retain(|r| r.user_id == uid);
    }

    if let Some(day) = query.day {
        items.retain(|r| r.checked_at.starts_with(&day));
    }

    Ok(Json(items))
}

#[utoipa::path(
    post,
    path = "/api/attendance",
    request_body = CheckInBody,
    responses(
        (status = 200, description = "Zarejestrowano obecność", body = AttendanceRecord),
        (status = 400, description = "Nieprawidłowe dane wejściowe", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn check_in(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CheckInBody>,
) -> AppResult<Json<AttendanceRecord>> {
    ensure_roles(&auth, &[Role::Zawodnik, Role::Trener, Role::Admin])?;
    let token = body.token.trim();
    if token.is_empty() {
        return Err(AppError::BadRequest("Podaj kod z QR.".into()));
    }
    let record = state.db.check_in_attendance(
        &auth.user.id,
        &auth.user.display_name,
        token,
    ).await?;
    state.db.append_log(
        LogLevel::Info,
        "attendance",
        &format!("Check-in: {}", auth.user.email),
        Some(&auth.user.id),
    ).await?;
    Ok(Json(record))
}
