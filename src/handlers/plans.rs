use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::{AppError, AppResult};
use crate::models::club::{
    LogLevel, PlanExercise, PlanProgressEntry, TrainingPlan, TrainingPlanProgress,
};
use crate::models::role::Role;
use crate::models::user::{ErrorBody, OkResponse};
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct PlanBody {
    pub title: String,
    pub description: Option<String>,
    pub week_label: Option<String>,
    pub exercises: Vec<PlanExercise>,
    pub assigned_user_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ProgressBody {
    pub entries: Vec<PlanProgressEntry>,
}

fn is_plan_editor(auth: &AuthUser) -> bool {
    auth.roles().contains(&Role::Trener) || auth.roles().contains(&Role::Superadmin)
}

#[utoipa::path(
    get,
    path = "/api/plans",
    responses(
        (status = 200, description = "Lista planów treningowych", body = Vec<TrainingPlan>),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_plans(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<TrainingPlan>>> {
    ensure_roles(&auth, &[Role::Zawodnik, Role::Trener])?;
    // W podglądzie — perspektywa targetu (nie pełna lista trenera/SA).
    if !auth.is_previewing() && is_plan_editor(&auth) {
        return Ok(Json(state.db.list_plans().await?));
    }
    Ok(Json(state.db.plans_for_user(auth.effective_id()).await?))
}

#[utoipa::path(
    post,
    path = "/api/plans",
    request_body = PlanBody,
    responses(
        (status = 200, description = "Utworzono plan treningowy", body = TrainingPlan),
        (status = 400, description = "Nieprawidłowe dane wejściowe", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_plan(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<PlanBody>,
) -> AppResult<Json<TrainingPlan>> {
    ensure_roles(&auth, &[Role::Trener])?;
    if !is_plan_editor(&auth) {
        return Err(AppError::Forbidden("Brak uprawnień do edycji planów.".into()));
    }
    if body.title.trim().is_empty() {
        return Err(AppError::BadRequest("Podaj tytuł planu.".into()));
    }
    let now = chrono::Utc::now().to_rfc3339();
    let plan = TrainingPlan {
        id: uuid::Uuid::new_v4().to_string(),
        title: body.title.trim().to_string(),
        description: body.description,
        week_label: body.week_label,
        exercises: body.exercises,
        assigned_user_ids: body.assigned_user_ids.unwrap_or_default(),
        created_by: auth.user.id.clone(),
        created_at: now.clone(),
        updated_at: now,
    };
    state.db.upsert_plan(plan.clone()).await?;
    state.db.append_log(
        LogLevel::Info,
        "plans",
        &format!("Utworzono plan {}", plan.title),
        Some(&auth.user.id),
    ).await?;
    for uid in &plan.assigned_user_ids {
        crate::mail::notify_user(
            &state,
            uid,
            "Nowy plan treningowy",
            &format!("Przypisano Cię do planu: {}", plan.title),
            "training_plan",
            Some("/panel/plany"),
            crate::mail::EmailChannel::TrainingPlan,
        )
        .await;
    }
    Ok(Json(plan))
}

#[utoipa::path(
    patch,
    path = "/api/plans/{id}",
    params(("id" = String, Path, description = "ID planu")),
    request_body = PlanBody,
    responses(
        (status = 200, description = "Zaktualizowano plan treningowy", body = TrainingPlan),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Plan nie istnieje", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_plan(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<PlanBody>,
) -> AppResult<Json<TrainingPlan>> {
    ensure_roles(&auth, &[Role::Trener])?;
    if !is_plan_editor(&auth) {
        return Err(AppError::Forbidden("Brak uprawnień do edycji planów.".into()));
    }
    let existing = state
        .db
        .get_plan(&id).await?
        .ok_or_else(|| AppError::NotFound("Plan nie istnieje.".into()))?;
    let prev_assigned = existing.assigned_user_ids.clone();
    let plan = TrainingPlan {
        id: existing.id,
        title: body.title.trim().to_string(),
        description: body.description,
        week_label: body.week_label,
        exercises: body.exercises,
        assigned_user_ids: body.assigned_user_ids.unwrap_or_default(),
        created_by: existing.created_by,
        created_at: existing.created_at,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    state.db.upsert_plan(plan.clone()).await?;
    state.db.append_log(
        LogLevel::Info,
        "plans",
        &format!("Zaktualizowano plan {}", plan.title),
        Some(&auth.user.id),
    ).await?;
    for uid in &plan.assigned_user_ids {
        if !prev_assigned.contains(uid) {
            crate::mail::notify_user(
                &state,
                uid,
                "Nowy plan treningowy",
                &format!("Przypisano Cię do planu: {}", plan.title),
                "training_plan",
                Some("/panel/plany"),
                crate::mail::EmailChannel::TrainingPlan,
            )
            .await;
        }
    }
    Ok(Json(plan))
}

#[utoipa::path(
    delete,
    path = "/api/plans/{id}",
    params(("id" = String, Path, description = "ID planu")),
    responses(
        (status = 200, description = "Usunięto plan treningowy", body = OkResponse),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_plan(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<OkResponse>> {
    ensure_roles(&auth, &[Role::Trener])?;
    if !is_plan_editor(&auth) {
        return Err(AppError::Forbidden("Brak uprawnień do edycji planów.".into()));
    }
    state.db.delete_plan(&id).await?;
    state.db.append_log(
        LogLevel::Warn,
        "plans",
        &format!("Usunięto plan {id}"),
        Some(&auth.user.id),
    ).await?;
    Ok(Json(OkResponse { ok: true }))
}

#[utoipa::path(
    get,
    path = "/api/plans/{id}/progress",
    params(("id" = String, Path, description = "ID planu")),
    responses(
        (status = 200, description = "Postęp realizacji planu", body = TrainingPlanProgress),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_my_progress(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(plan_id): Path<String>,
) -> AppResult<Json<TrainingPlanProgress>> {
    ensure_roles(&auth, &[Role::Zawodnik, Role::Trener])?;
    let uid = auth.effective_id();
    if let Some(p) = state.db.get_plan_progress(&plan_id, uid).await? {
        return Ok(Json(p));
    }
    Ok(Json(TrainingPlanProgress {
        id: format!("{}:{}", plan_id, uid),
        plan_id,
        user_id: uid.to_string(),
        entries: vec![],
        updated_at: chrono::Utc::now().to_rfc3339(),
    }))
}

#[utoipa::path(
    put,
    path = "/api/plans/{id}/progress",
    params(("id" = String, Path, description = "ID planu")),
    request_body = ProgressBody,
    responses(
        (status = 200, description = "Zapisano postęp realizacji planu", body = TrainingPlanProgress),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Plan nie istnieje", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn save_progress(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(plan_id): Path<String>,
    Json(body): Json<ProgressBody>,
) -> AppResult<Json<TrainingPlanProgress>> {
    ensure_roles(&auth, &[Role::Zawodnik, Role::Trener])?;
    let plan = state
        .db
        .get_plan(&plan_id).await?
        .ok_or_else(|| AppError::NotFound("Plan nie istnieje.".into()))?;
    if !plan.assigned_user_ids.is_empty()
        && !plan.assigned_user_ids.contains(&auth.user.id)
        && !is_plan_editor(&auth)
    {
        return Err(AppError::Forbidden(
            "Plan nie jest do Ciebie przypisany.".into(),
        ));
    }

    let progress = TrainingPlanProgress {
        id: format!("{}:{}", plan_id, auth.user.id),
        plan_id,
        user_id: auth.user.id.clone(),
        entries: body.entries,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    state.db.upsert_plan_progress(progress.clone()).await?;
    Ok(Json(progress))
}
