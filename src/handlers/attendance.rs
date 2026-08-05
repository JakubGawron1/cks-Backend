use axum::extract::{Path, Query, State};
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
    pub event_id: Option<String>,
    /// Filtr statusu, np. `pending_unauthorized`
    pub status: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RefreshSessionBody {
    pub label: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ApproveAttendanceBody {
    /// Wymagane, gdy skan nie miał powiązanego treningu (`event_id` null).
    pub event_id: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/attendance/session",
    responses(
        (status = 200, description = "Stała sesja QR klubu (bez rotacji)", body = AttendanceSession),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_session(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<AttendanceSession>> {
    ensure_roles(&auth, &[Role::Trener, Role::Admin])?;
    Ok(Json(state.db.ensure_attendance_session().await?))
}

#[utoipa::path(
    post,
    path = "/api/attendance/session",
    request_body = RefreshSessionBody,
    responses(
        (status = 200, description = "Odświeżono kod QR (nowy token)", body = AttendanceSession),
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
    let label = body
        .label
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            prev.as_ref()
                .map(|p| p.label.clone())
                .unwrap_or_else(|| "Trening klubowy".into())
        });
    let session = AttendanceSession {
        token: uuid::Uuid::new_v4().to_string(),
        label,
        created_at: prev
            .as_ref()
            .map(|p| p.created_at.clone())
            .unwrap_or_else(|| now.clone()),
        refreshed_at: now,
        event_id: None,
    };
    state.db.set_attendance_session(session.clone()).await?;
    state
        .db
        .append_log(
            LogLevel::Info,
            "attendance",
            "Odświeżono stały kod QR obecności",
            Some(&auth.user.id),
        )
        .await?;
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

    if let Some(ref event_id) = query.event_id {
        let _ = state.db.reconcile_attendance_for_event(event_id).await;
    } else {
        let _ = state
            .db
            .reconcile_past_training_attendance_since_days(62)
            .await;
    }

    let is_staff = !auth.is_previewing()
        && crate::models::role::has_any_role(auth.roles(), &[Role::Trener, Role::Admin]);
    let mut items = state.db.list_attendance_in_window().await?;

    if !is_staff {
        let uid = auth.effective_id();
        items.retain(|r| r.user_id == uid);
        // Zawodnik nie widzi pending_unauthorized / rejected
        items.retain(|r| r.status == "present" || r.status == "absent");
    } else if let Some(uid) = query.user_id {
        items.retain(|r| r.user_id == uid);
    }

    if let Some(day) = query.day {
        let events = state.db.list_events().await?;
        items.retain(|r| {
            if r.checked_at.starts_with(&day) {
                return true;
            }
            if r.status == "absent" || r.status == "pending_unauthorized" {
                if let Some(eid) = &r.event_id {
                    return events.iter().any(|e| e.id == *eid && e.date == day);
                }
            }
            false
        });
    }

    if let Some(event_id) = query.event_id {
        items.retain(|r| r.event_id.as_deref() == Some(event_id.as_str()));
    }

    if let Some(status) = query.status {
        items.retain(|r| r.status == status);
    }

    Ok(Json(items))
}

#[utoipa::path(
    post,
    path = "/api/attendance",
    request_body = CheckInBody,
    responses(
        (status = 200, description = "Zarejestrowano obecność", body = AttendanceRecord),
        (status = 400, description = "Nieprawidłowe dane / brak treningu w terminie", body = ErrorBody),
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
    let record = state
        .db
        .check_in_attendance(&auth.user.id, &auth.user.display_name, token)
        .await?;
    state
        .db
        .append_log(
            LogLevel::Info,
            "attendance",
            &format!("Check-in: {}", auth.user.email),
            Some(&auth.user.id),
        )
        .await?;
    Ok(Json(record))
}

#[utoipa::path(
    post,
    path = "/api/attendance/{id}/approve",
    params(("id" = String, Path, description = "ID rekordu obecności")),
    request_body = ApproveAttendanceBody,
    responses(
        (status = 200, description = "Zaakceptowano nieautoryzowany skan", body = AttendanceRecord),
        (status = 400, description = "Nieprawidłowe dane", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Nie znaleziono", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn approve_attendance(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<ApproveAttendanceBody>,
) -> AppResult<Json<AttendanceRecord>> {
    ensure_roles(&auth, &[Role::Trener, Role::Admin])?;
    let record = state
        .db
        .approve_unauthorized_attendance(&id, body.event_id.as_deref())
        .await?;
    state
        .db
        .append_log(
            LogLevel::Info,
            "attendance",
            &format!(
                "Zaakceptowano nieautoryzowaną obecność: {}",
                record.display_name
            ),
            Some(&auth.user.id),
        )
        .await?;
    Ok(Json(record))
}

#[utoipa::path(
    post,
    path = "/api/attendance/{id}/reject",
    params(("id" = String, Path, description = "ID rekordu obecności")),
    responses(
        (status = 200, description = "Odrzucono nieautoryzowany skan", body = AttendanceRecord),
        (status = 400, description = "Nieprawidłowe dane", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Nie znaleziono", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn reject_attendance(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<AttendanceRecord>> {
    ensure_roles(&auth, &[Role::Trener, Role::Admin])?;
    let record = state.db.reject_unauthorized_attendance(&id).await?;
    state
        .db
        .append_log(
            LogLevel::Info,
            "attendance",
            &format!(
                "Odrzucono nieautoryzowaną obecność: {}",
                record.display_name
            ),
            Some(&auth.user.id),
        )
        .await?;
    Ok(Json(record))
}
