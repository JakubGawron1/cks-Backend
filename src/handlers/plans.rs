use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::{AppError, AppResult};
use crate::models::club::{
    LogLevel, PlanExercise, PlanProgressEntry, TrainingPlan, TrainingPlanProgress,
};
use crate::models::role::Role;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct PlanBody {
    pub title: String,
    pub description: Option<String>,
    pub week_label: Option<String>,
    pub exercises: Vec<PlanExercise>,
    pub assigned_user_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct ProgressBody {
    pub entries: Vec<PlanProgressEntry>,
}

fn is_plan_editor(auth: &AuthUser) -> bool {
    auth.roles().contains(&Role::Trener) || auth.roles().contains(&Role::Superadmin)
}

pub async fn list_plans(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<TrainingPlan>>> {
    ensure_roles(&auth, &[Role::Zawodnik, Role::Trener])?;
    if is_plan_editor(&auth) {
        return Ok(Json(state.db.list_plans()?));
    }
    Ok(Json(state.db.plans_for_user(&auth.user.id)?))
}

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
    state.db.upsert_plan(plan.clone())?;
    state.db.append_log(
        LogLevel::Info,
        "plans",
        &format!("Utworzono plan {}", plan.title),
        Some(&auth.user.id),
    )?;
    Ok(Json(plan))
}

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
        .get_plan(&id)?
        .ok_or_else(|| AppError::NotFound("Plan nie istnieje.".into()))?;
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
    state.db.upsert_plan(plan.clone())?;
    state.db.append_log(
        LogLevel::Info,
        "plans",
        &format!("Zaktualizowano plan {}", plan.title),
        Some(&auth.user.id),
    )?;
    Ok(Json(plan))
}

pub async fn delete_plan(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    ensure_roles(&auth, &[Role::Trener])?;
    if !is_plan_editor(&auth) {
        return Err(AppError::Forbidden("Brak uprawnień do edycji planów.".into()));
    }
    state.db.delete_plan(&id)?;
    state.db.append_log(
        LogLevel::Warn,
        "plans",
        &format!("Usunięto plan {id}"),
        Some(&auth.user.id),
    )?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn get_my_progress(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(plan_id): Path<String>,
) -> AppResult<Json<TrainingPlanProgress>> {
    ensure_roles(&auth, &[Role::Zawodnik, Role::Trener])?;
    if let Some(p) = state.db.get_plan_progress(&plan_id, &auth.user.id)? {
        return Ok(Json(p));
    }
    Ok(Json(TrainingPlanProgress {
        id: format!("{}:{}", plan_id, auth.user.id),
        plan_id,
        user_id: auth.user.id.clone(),
        entries: vec![],
        updated_at: chrono::Utc::now().to_rfc3339(),
    }))
}

pub async fn save_progress(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(plan_id): Path<String>,
    Json(body): Json<ProgressBody>,
) -> AppResult<Json<TrainingPlanProgress>> {
    ensure_roles(&auth, &[Role::Zawodnik, Role::Trener])?;
    let plan = state
        .db
        .get_plan(&plan_id)?
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
    state.db.upsert_plan_progress(progress.clone())?;
    Ok(Json(progress))
}
