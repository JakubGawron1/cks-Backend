use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::{AppError, AppResult};
use crate::models::club::{CompetitionResult, LogLevel, ResultStatus};
use crate::models::role::{has_any_role, Role};
use crate::models::user::ErrorBody;
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateResultBody {
    pub status: ResultStatus,
    pub reviewer_note: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateResultBody {
    pub event_name: String,
    pub kind: Option<String>,
    pub snatch_kg: Option<f64>,
    pub clean_jerk_kg: Option<f64>,
    pub total_kg: Option<f64>,
    pub bodyweight_kg: Option<f64>,
    pub venue: Option<String>,
    pub category: Option<String>,
    pub athlete_name: Option<String>,
    /// Powiązanie z kontem zawodnika (staff może wpisywać za kogoś)
    pub user_id: Option<String>,
    /// Trener/admin: wynik od razu Accepted (bez kolejki weryfikacji)
    pub auto_accept: Option<bool>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct ResultsQuery {
    pub mine: Option<bool>,
}

#[utoipa::path(
    get,
    path = "/api/results",
    params(ResultsQuery),
    responses(
        (status = 200, description = "Lista wyników", body = Vec<CompetitionResult>),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
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

#[utoipa::path(
    get,
    path = "/api/public/results",
    responses(
        (status = 200, description = "Publiczne wyniki zawodów (zaakceptowane)", body = Vec<CompetitionResult>),
    ),
    tag = "public"
)]
pub async fn list_public_results(
    State(state): State<AppState>,
) -> AppResult<Json<Vec<CompetitionResult>>> {
    let all = state.db.list_results().await?;
    let public: Vec<CompetitionResult> = all
        .into_iter()
        .filter(|r| {
            r.status == ResultStatus::Accepted
                && r.kind.eq_ignore_ascii_case("competition")
        })
        .collect();
    Ok(Json(public))
}

#[utoipa::path(
    post,
    path = "/api/results",
    request_body = CreateResultBody,
    responses(
        (status = 200, description = "Zgłoszono wynik", body = CompetitionResult),
        (status = 400, description = "Nieprawidłowe dane wejściowe", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
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

    let is_staff = has_any_role(auth.roles(), &[Role::Trener, Role::Admin]);
    let auto_accept = body.auto_accept.unwrap_or(false);
    if auto_accept && !is_staff {
        return Err(AppError::Forbidden(
            "Tylko trener/admin może od razu akceptować wynik.".into(),
        ));
    }

    let (athlete_name, user_id) = if is_staff {
        let name = body
            .athlete_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| auth.user.display_name.clone());

        let uid = match body.user_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some("manual") => None,
            Some(id) => {
                let user = state
                    .db
                    .find_user_by_id(id)
                    .await?
                    .ok_or_else(|| AppError::BadRequest("Wybrane konto nie istnieje.".into()))?;
                if !user.roles.contains(&Role::Zawodnik) {
                    return Err(AppError::BadRequest(
                        "Wynik można powiązać tylko z kontem zawodnika.".into(),
                    ));
                }
                Some(user.id)
            }
            None if auto_accept => None,
            None => Some(auth.user.id.clone()),
        };
        (name, uid)
    } else {
        (
            auth.user.display_name.clone(),
            Some(auth.user.id.clone()),
        )
    };

    let total = body.total_kg.or_else(|| match (body.snatch_kg, body.clean_jerk_kg) {
        (Some(s), Some(c)) => Some(s + c),
        _ => None,
    });

    let now = chrono::Utc::now().to_rfc3339();
    let status = if auto_accept {
        ResultStatus::Accepted
    } else {
        ResultStatus::Pending
    };

    let result = CompetitionResult {
        id: uuid::Uuid::new_v4().to_string(),
        athlete_name,
        user_id,
        event_name: body.event_name.trim().to_string(),
        kind,
        snatch_kg: body.snatch_kg,
        clean_jerk_kg: body.clean_jerk_kg,
        total_kg: total,
        bodyweight_kg: body.bodyweight_kg,
        venue: body.venue,
        category: body.category,
        status,
        reviewer_note: if auto_accept {
            Some("Wpisane przez kadrę".into())
        } else {
            None
        },
        submitted_at: now.clone(),
        updated_at: now,
    };
    state.db.upsert_result(result.clone()).await?;
    state.db.append_log(
        LogLevel::Info,
        "results",
        &format!(
            "{} wynik {} ({}) przez {} → {:?}",
            if auto_accept {
                "Wpisano (auto-accept)"
            } else {
                "Zgłoszono"
            },
            result.event_name,
            result.kind,
            auth.user.email,
            result.status
        ),
        Some(&auth.user.id),
    )
    .await?;

    if result.status == ResultStatus::Pending {
        let _ = state
            .db
            .notify_staff(
                "Nowy wynik do weryfikacji",
                &format!(
                    "{} · {} ({})",
                    result.athlete_name, result.event_name, result.kind
                ),
                "result",
                Some("/klub/weryfikacja-wynikow"),
                Some(&auth.user.id),
            )
            .await;
    }

    Ok(Json(result))
}

#[utoipa::path(
    patch,
    path = "/api/results/{id}",
    params(("id" = String, Path, description = "ID wyniku")),
    request_body = UpdateResultBody,
    responses(
        (status = 200, description = "Zweryfikowano wynik", body = CompetitionResult),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Wynik nie istnieje", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
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

    if let Some(uid) = result.user_id.as_deref() {
        let status_label = match result.status {
            ResultStatus::Accepted => "zaakceptowany",
            ResultStatus::Rejected => "odrzucony",
            ResultStatus::NeedsEdit => "wymaga poprawy",
            ResultStatus::Pending => "oczekuje",
        };
        let note = result
            .reviewer_note
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| format!(" Notatka: {s}"))
            .unwrap_or_default();
        crate::mail::notify_user(
            &state,
            uid,
            "Aktualizacja wyniku",
            &format!(
                "Wynik z „{}” został {}.{note}",
                result.event_name, status_label
            ),
            "result",
            Some("/panel/wyniki"),
            crate::mail::EmailChannel::None,
        )
        .await;
    }

    Ok(Json(result))
}
