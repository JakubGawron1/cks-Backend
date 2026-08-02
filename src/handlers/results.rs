use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::{AppError, AppResult};
use crate::models::club::{CompetitionResult, LogLevel, ResultStatus};
use crate::models::role::Role;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct UpdateResultBody {
    pub status: ResultStatus,
    pub reviewer_note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateResultBody {
    pub event_name: String,
    pub kind: Option<String>,
    pub snatch_kg: Option<f64>,
    pub clean_jerk_kg: Option<f64>,
    pub total_kg: Option<f64>,
    pub athlete_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ResultsQuery {
    pub mine: Option<bool>,
}

pub async fn list_results(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<ResultsQuery>,
) -> AppResult<Json<Vec<CompetitionResult>>> {
    let mine = query.mine.unwrap_or(false);
    if mine {
        ensure_roles(&auth, &[Role::Zawodnik, Role::Trener, Role::Admin])?;
        let all = state.db.list_results().await?;
        let filtered = all
            .into_iter()
            .filter(|r| r.user_id.as_deref() == Some(auth.user.id.as_str()))
            .collect();
        return Ok(Json(filtered));
    }
    ensure_roles(&auth, &[Role::Trener, Role::Admin])?;
    Ok(Json(state.db.list_results().await?))
}

pub async fn create_result(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateResultBody>,
) -> AppResult<Json<CompetitionResult>> {
    ensure_roles(&auth, &[Role::Zawodnik, Role::Trener, Role::Admin])?;

    if body.event_name.trim().is_empty() {
        return Err(AppError::BadRequest("Podaj nazwę zawodów / treningu.".into()));
    }

    let kind = body
        .kind
        .unwrap_or_else(|| "competition".into())
        .to_ascii_lowercase();
    if kind != "competition" && kind != "training" {
        return Err(AppError::BadRequest(
            "kind musi być 'competition' lub 'training'.".into(),
        ));
    }

    let total = body.total_kg.or_else(|| match (body.snatch_kg, body.clean_jerk_kg) {
        (Some(s), Some(c)) => Some(s + c),
        _ => None,
    });

    let now = chrono::Utc::now().to_rfc3339();
    let result = CompetitionResult {
        id: uuid::Uuid::new_v4().to_string(),
        athlete_name: body
            .athlete_name
            .unwrap_or_else(|| auth.user.display_name.clone()),
        user_id: Some(auth.user.id.clone()),
        event_name: body.event_name.trim().to_string(),
        kind,
        snatch_kg: body.snatch_kg,
        clean_jerk_kg: body.clean_jerk_kg,
        total_kg: total,
        status: ResultStatus::Pending,
        reviewer_note: None,
        submitted_at: now.clone(),
        updated_at: now,
    };
    state.db.upsert_result(result.clone()).await?;
    state.db.append_log(
        LogLevel::Info,
        "results",
        &format!(
            "Zgłoszono wynik {} ({}) przez {}",
            result.event_name, result.kind, auth.user.email
        ),
        Some(&auth.user.id),
    ).await?;
    Ok(Json(result))
}

pub async fn update_result(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateResultBody>,
) -> AppResult<Json<CompetitionResult>> {
    ensure_roles(&auth, &[Role::Trener, Role::Admin])?;

    let mut result = state
        .db
        .get_result(&id).await?
        .ok_or_else(|| AppError::NotFound("Wynik nie istnieje.".into()))?;

    result.status = body.status;
    result.reviewer_note = body.reviewer_note;
    result.updated_at = chrono::Utc::now().to_rfc3339();
    state.db.upsert_result(result.clone()).await?;
    state.db.append_log(
        LogLevel::Info,
        "results",
        &format!("Weryfikacja wyniku {id} → {:?}", result.status),
        Some(&auth.user.id),
    ).await?;
    Ok(Json(result))
}
