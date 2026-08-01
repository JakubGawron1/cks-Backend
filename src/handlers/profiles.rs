use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::{AppError, AppResult};
use crate::models::club::{AthleteProfile, LogLevel};
use crate::models::role::Role;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ProfileBody {
    pub user_id: String,
    pub display_name: String,
    pub bodyweight_kg: Option<f64>,
    pub category: Option<String>,
    pub notes: Option<String>,
}

pub async fn list_profiles(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<AthleteProfile>>> {
    ensure_roles(&auth, &[Role::Trener, Role::Admin])?;
    Ok(Json(state.db.list_profiles()?))
}

pub async fn create_profile(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<ProfileBody>,
) -> AppResult<Json<AthleteProfile>> {
    ensure_roles(&auth, &[Role::Trener, Role::Admin])?;

    if body.display_name.trim().is_empty() {
        return Err(AppError::BadRequest("Podaj nazwę zawodnika.".into()));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let profile = AthleteProfile {
        id: uuid::Uuid::new_v4().to_string(),
        user_id: body.user_id,
        display_name: body.display_name.trim().to_string(),
        bodyweight_kg: body.bodyweight_kg,
        category: body.category,
        notes: body.notes,
        created_at: now.clone(),
        updated_at: now,
    };
    state.db.upsert_profile(profile.clone())?;
    state.db.append_log(
        LogLevel::Info,
        "profiles",
        &format!("Utworzono profil {}", profile.display_name),
        Some(&auth.user.id),
    )?;
    Ok(Json(profile))
}

pub async fn update_profile(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<ProfileBody>,
) -> AppResult<Json<AthleteProfile>> {
    ensure_roles(&auth, &[Role::Trener, Role::Admin])?;

    let existing = state
        .db
        .get_profile(&id)?
        .ok_or_else(|| AppError::NotFound("Profil nie istnieje.".into()))?;

    let profile = AthleteProfile {
        id: existing.id,
        user_id: body.user_id,
        display_name: body.display_name.trim().to_string(),
        bodyweight_kg: body.bodyweight_kg,
        category: body.category,
        notes: body.notes,
        created_at: existing.created_at,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    state.db.upsert_profile(profile.clone())?;
    state.db.append_log(
        LogLevel::Info,
        "profiles",
        &format!("Zaktualizowano profil {}", profile.display_name),
        Some(&auth.user.id),
    )?;
    Ok(Json(profile))
}

pub async fn delete_profile(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    ensure_roles(&auth, &[Role::Trener, Role::Admin])?;
    state.db.delete_profile(&id)?;
    state.db.append_log(
        LogLevel::Warn,
        "profiles",
        &format!("Usunięto profil {id}"),
        Some(&auth.user.id),
    )?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
