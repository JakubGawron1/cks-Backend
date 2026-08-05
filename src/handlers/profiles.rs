use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::{AppError, AppResult};
use crate::models::club::{AthleteProfile, LogLevel};
use crate::models::role::Role;
use crate::models::user::{ErrorBody, OkResponse};
use crate::state::AppState;
use crate::weightlifting_categories::resolve_category;
use chrono::Utc;

fn normalize_photo_url(raw: Option<String>) -> Option<String> {
    raw.and_then(|s| {
        let t = s.trim().to_string();
        if t.is_empty() { None } else { Some(t) }
    })
}

/// Kategoria z masy + wieku/płci; gdy brak danych — zostaw ręczną z body (lub None).
fn resolve_profile_category(
    bodyweight_kg: Option<f64>,
    birth_date: &Option<String>,
    sex: &Option<String>,
    fallback: Option<String>,
) -> Option<String> {
    let bw = bodyweight_kg.filter(|v| v.is_finite() && *v > 0.0)?;
    let birth = birth_date.as_deref().map(str::trim).filter(|s| !s.is_empty())?;
    let sex_raw = sex.as_deref().map(str::trim).filter(|s| !s.is_empty())?;
    resolve_category(birth, sex_raw, bw, Utc::now().date_naive())
        .ok()
        .or(fallback)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ProfileBody {
    pub user_id: String,
    pub display_name: String,
    pub bodyweight_kg: Option<f64>,
    pub category: Option<String>,
    pub notes: Option<String>,
    pub photo_url: Option<String>,
    pub birth_date: Option<String>,
    pub sex: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/profiles",
    responses(
        (status = 200, description = "Lista profili zawodników", body = Vec<AthleteProfile>),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_profiles(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<AthleteProfile>>> {
    ensure_roles(&auth, &[Role::Trener, Role::Admin])?;
    Ok(Json(state.db.list_profiles().await?))
}

#[utoipa::path(
    get,
    path = "/api/public/profiles",
    responses(
        (status = 200, description = "Publiczna lista profili zawodników", body = Vec<AthleteProfile>),
    ),
    tag = "public"
)]
pub async fn list_public_profiles(
    State(state): State<AppState>,
) -> AppResult<Json<Vec<AthleteProfile>>> {
    Ok(Json(state.db.list_profiles().await?))
}

async fn resolve_profile_user_id(
    state: &AppState,
    user_id: &str,
) -> AppResult<String> {
    let trimmed = user_id.trim();
    if trimmed.is_empty() || trimmed == "manual" {
        return Ok("manual".into());
    }

    let user = state
        .db
        .find_user_by_id(trimmed)
        .await?
        .ok_or_else(|| AppError::BadRequest("Wybrane konto nie istnieje.".into()))?;

    if !user.roles.contains(&Role::Zawodnik) {
        return Err(AppError::BadRequest(
            "Profil można powiązać tylko z kontem o roli zawodnik.".into(),
        ));
    }

    Ok(user.id)
}

#[utoipa::path(
    post,
    path = "/api/profiles",
    request_body = ProfileBody,
    responses(
        (status = 200, description = "Utworzono profil", body = AthleteProfile),
        (status = 400, description = "Nieprawidłowe dane wejściowe", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_profile(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<ProfileBody>,
) -> AppResult<Json<AthleteProfile>> {
    ensure_roles(&auth, &[Role::Trener, Role::Admin])?;

    if body.display_name.trim().is_empty() {
        return Err(AppError::BadRequest("Podaj nazwę zawodnika.".into()));
    }

    let user_id = resolve_profile_user_id(&state, &body.user_id).await?;

    if let Some(ref sex) = body.sex {
        let s = sex.trim().to_ascii_lowercase();
        if s != "male" && s != "female" {
            return Err(AppError::BadRequest(
                "sex musi być 'male' lub 'female'.".into(),
            ));
        }
    }

    let mut photo_url = normalize_photo_url(body.photo_url);
    // Jeśli brak URL na profilu, a konto już ma zdjęcie — użyj zdjęcia konta.
    if photo_url.is_none() && user_id != "manual" {
        if let Some(user) = state.db.find_user_by_id(&user_id).await? {
            photo_url = user.photo_url;
        }
    }

    let now = chrono::Utc::now().to_rfc3339();
    let sex = body.sex.map(|s| s.trim().to_ascii_lowercase());
    let birth_date = body.birth_date;
    let bodyweight_kg = body.bodyweight_kg;
    let category = resolve_profile_category(
        bodyweight_kg,
        &birth_date,
        &sex,
        body.category,
    );
    let profile = AthleteProfile {
        id: uuid::Uuid::new_v4().to_string(),
        user_id: user_id.clone(),
        display_name: body.display_name.trim().to_string(),
        bodyweight_kg,
        category,
        notes: body.notes,
        photo_url: photo_url.clone(),
        birth_date,
        sex,
        created_at: now.clone(),
        updated_at: now,
    };
    state.db.upsert_profile(profile.clone()).await?;
    state
        .db
        .sync_photo_profile_to_user(&user_id, &photo_url)
        .await?;
    state.db.append_log(
        LogLevel::Info,
        "profiles",
        &format!("Utworzono profil {}", profile.display_name),
        Some(&auth.user.id),
    ).await?;
    Ok(Json(profile))
}

#[utoipa::path(
    patch,
    path = "/api/profiles/{id}",
    params(("id" = String, Path, description = "ID profilu")),
    request_body = ProfileBody,
    responses(
        (status = 200, description = "Zaktualizowano profil", body = AthleteProfile),
        (status = 400, description = "Nieprawidłowe dane wejściowe", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Profil nie istnieje", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_profile(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<ProfileBody>,
) -> AppResult<Json<AthleteProfile>> {
    ensure_roles(&auth, &[Role::Trener, Role::Admin])?;

    let existing = state
        .db
        .get_profile(&id).await?
        .ok_or_else(|| AppError::NotFound("Profil nie istnieje.".into()))?;

    let user_id = resolve_profile_user_id(&state, &body.user_id).await?;

    if let Some(ref sex) = body.sex {
        let s = sex.trim().to_ascii_lowercase();
        if s != "male" && s != "female" {
            return Err(AppError::BadRequest(
                "sex musi być 'male' lub 'female'.".into(),
            ));
        }
    }

    let photo_url = normalize_photo_url(body.photo_url);
    let sex = body.sex.map(|s| s.trim().to_ascii_lowercase());
    let birth_date = body.birth_date;
    let bodyweight_kg = body.bodyweight_kg;
    let category = resolve_profile_category(
        bodyweight_kg,
        &birth_date,
        &sex,
        body.category,
    );

    let profile = AthleteProfile {
        id: existing.id,
        user_id: user_id.clone(),
        display_name: body.display_name.trim().to_string(),
        bodyweight_kg,
        category,
        notes: body.notes,
        photo_url: photo_url.clone(),
        birth_date,
        sex,
        created_at: existing.created_at,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    state.db.upsert_profile(profile.clone()).await?;
    state
        .db
        .sync_photo_profile_to_user(&user_id, &photo_url)
        .await?;
    state.db.append_log(
        LogLevel::Info,
        "profiles",
        &format!("Zaktualizowano profil {}", profile.display_name),
        Some(&auth.user.id),
    ).await?;
    Ok(Json(profile))
}

#[utoipa::path(
    delete,
    path = "/api/profiles/{id}",
    params(("id" = String, Path, description = "ID profilu")),
    responses(
        (status = 200, description = "Usunięto profil", body = OkResponse),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_profile(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<OkResponse>> {
    ensure_roles(&auth, &[Role::Trener, Role::Admin])?;
    state.db.delete_profile(&id).await?;
    state.db.append_log(
        LogLevel::Warn,
        "profiles",
        &format!("Usunięto profil {id}"),
        Some(&auth.user.id),
    ).await?;
    Ok(Json(OkResponse { ok: true }))
}
