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
use crate::weightlifting_categories::resolve_category_from_profile;
use chrono::{NaiveDate, Utc};

fn parse_event_date(raw: Option<&str>) -> AppResult<String> {
    let trimmed = raw
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::BadRequest("Podaj datę zawodów / treningu.".into()))?;
    let date = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d").map_err(|_| {
        AppError::BadRequest("Data musi być w formacie RRRR-MM-DD.".into())
    })?;
    Ok(date.format("%Y-%m-%d").to_string())
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateResultBody {
    /// Gdy brak — status bez zmian (sama edycja pól).
    pub status: Option<ResultStatus>,
    pub reviewer_note: Option<String>,
    pub event_name: Option<String>,
    /// YYYY-MM-DD
    pub event_date: Option<String>,
    pub snatch_kg: Option<f64>,
    pub clean_jerk_kg: Option<f64>,
    pub bodyweight_kg: Option<f64>,
    pub venue: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateResultBody {
    pub event_name: String,
    /// Data zawodów / treningu (YYYY-MM-DD)
    pub event_date: Option<String>,
    pub kind: Option<String>,
    pub snatch_kg: Option<f64>,
    pub clean_jerk_kg: Option<f64>,
    pub total_kg: Option<f64>,
    pub bodyweight_kg: Option<f64>,
    pub venue: Option<String>,
    /// Ignorowane dla zawodów — kategoria wyliczana z profilu (wiek + płeć) i masy ciała.
    pub category: Option<String>,
    pub athlete_name: Option<String>,
    /// Powiązanie z kontem zawodnika (staff może wpisywać za kogoś)
    pub user_id: Option<String>,
    /// Profil zawodnika (staff) — do wyliczenia kategorii gdy brak user_id
    pub profile_id: Option<String>,
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
        let uid = auth.effective_id();
        let filtered = all
            .into_iter()
            .filter(|r| r.user_id.as_deref() == Some(uid))
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

    let kind = body
        .kind
        .unwrap_or_else(|| "competition".into())
        .to_ascii_lowercase();
    if kind != "competition" && kind != "training" {
        return Err(AppError::BadRequest(
            "kind musi być 'competition' lub 'training'.".into(),
        ));
    }

    let event_name = {
        let trimmed = body.event_name.trim();
        if kind == "training" {
            if trimmed.is_empty() {
                "Trening".to_string()
            } else {
                trimmed.to_string()
            }
        } else if trimmed.is_empty() {
            return Err(AppError::BadRequest("Podaj nazwę zawodów.".into()));
        } else {
            trimmed.to_string()
        }
    };

    let event_date = parse_event_date(body.event_date.as_deref())?;

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

    let bodyweight_kg = body.bodyweight_kg;
    let mut linked_profile_id: Option<String> = None;
    let category = if kind == "competition" {
        let bw = bodyweight_kg.filter(|v| v.is_finite() && *v > 0.0).ok_or_else(|| {
            AppError::BadRequest("Podaj masę ciała na zawodach (kg).".into())
        })?;

        let profile = if let Some(pid) = body
            .profile_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(
                state
                    .db
                    .get_profile(pid)
                    .await?
                    .ok_or_else(|| {
                        AppError::BadRequest("Wybrany profil zawodnika nie istnieje.".into())
                    })?,
            )
        } else if let Some(uid) = user_id.as_deref() {
            state.db.find_profile_by_user_id(uid).await?
        } else {
            None
        };

        let profile = profile.ok_or_else(|| {
            AppError::BadRequest(
                "Brak profilu zawodnika — uzupełnij datę urodzenia i płeć w profilu.".into(),
            )
        })?;
        linked_profile_id = Some(profile.id.clone());

        Some(resolve_category_from_profile(
            &profile,
            bw,
            Utc::now().date_naive(),
        )?)
    } else {
        body
            .category
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    };

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
        event_name,
        event_date: Some(event_date),
        kind,
        snatch_kg: body.snatch_kg,
        clean_jerk_kg: body.clean_jerk_kg,
        total_kg: total,
        bodyweight_kg,
        venue: body.venue,
        category,
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

    if result.status == ResultStatus::Accepted {
        let synced = state
            .db
            .apply_accepted_competition_to_profile(
                &result,
                linked_profile_id.as_deref(),
            )
            .await?;
        if synced {
            state
                .db
                .append_log(
                    LogLevel::Info,
                    "results",
                    &format!(
                        "Zaktualizowano kategorię/masę w profilu po wyniku {} → {:?}",
                        result.event_name, result.category
                    ),
                    Some(&auth.user.id),
                )
                .await?;
        }
    }

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
        (status = 200, description = "Zaktualizowano / zweryfikowano wynik", body = CompetitionResult),
        (status = 400, description = "Nieprawidłowe dane", body = ErrorBody),
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
    ensure_roles(&auth, &[Role::Zawodnik, Role::Trener, Role::Admin])?;

    let mut result = state
        .db
        .get_result(&id)
        .await?
        .ok_or_else(|| AppError::NotFound("Wynik nie istnieje.".into()))?;

    let is_staff = has_any_role(auth.roles(), &[Role::Trener, Role::Admin]);
    let is_owner = result.user_id.as_deref() == Some(auth.effective_id());

    if !is_staff {
        if !is_owner {
            return Err(AppError::Forbidden(
                "Możesz edytować tylko własne wyniki.".into(),
            ));
        }
        match result.status {
            ResultStatus::Pending | ResultStatus::NeedsEdit | ResultStatus::Accepted => {}
            ResultStatus::Rejected => {
                return Err(AppError::Forbidden(
                    "Odrzuconego wyniku nie da się poprawić — zgłoś nowy.".into(),
                ));
            }
        }
        if let Some(status) = body.status {
            if status != ResultStatus::Pending && status != ResultStatus::NeedsEdit {
                return Err(AppError::Forbidden(
                    "Zawodnik nie może zmieniać statusu weryfikacji.".into(),
                ));
            }
        }
    } else {
        // Kadra: edycja accepted / pending / needs_edit (poprawki błędów).
        match result.status {
            ResultStatus::Accepted
            | ResultStatus::Pending
            | ResultStatus::NeedsEdit => {}
            ResultStatus::Rejected => {
                if body.event_name.is_some()
                    || body.event_date.is_some()
                    || body.snatch_kg.is_some()
                    || body.clean_jerk_kg.is_some()
                    || body.bodyweight_kg.is_some()
                    || body.venue.is_some()
                {
                    return Err(AppError::BadRequest(
                        "Odrzuconego wyniku nie edytuje się — utwórz nowy albo zmień status.".into(),
                    ));
                }
            }
        }
    }

    let fields_touched = body.event_name.is_some()
        || body.event_date.is_some()
        || body.snatch_kg.is_some()
        || body.clean_jerk_kg.is_some()
        || body.bodyweight_kg.is_some()
        || body.venue.is_some();

    if let Some(name) = body.event_name.as_deref() {
        let trimmed = name.trim();
        if result.kind.eq_ignore_ascii_case("training") {
            result.event_name = if trimmed.is_empty() {
                "Trening".into()
            } else {
                trimmed.to_string()
            };
        } else if trimmed.is_empty() {
            return Err(AppError::BadRequest("Podaj nazwę zawodów.".into()));
        } else {
            result.event_name = trimmed.to_string();
        }
    }

    if let Some(raw_date) = body.event_date.as_deref() {
        result.event_date = Some(parse_event_date(Some(raw_date))?);
    }

    if let Some(s) = body.snatch_kg {
        result.snatch_kg = Some(s);
    }
    if let Some(c) = body.clean_jerk_kg {
        result.clean_jerk_kg = Some(c);
    }
    if body.snatch_kg.is_some() || body.clean_jerk_kg.is_some() {
        result.total_kg = match (result.snatch_kg, result.clean_jerk_kg) {
            (Some(s), Some(c)) => Some(s + c),
            _ => result.total_kg,
        };
    }

    if let Some(bw) = body.bodyweight_kg {
        if !bw.is_finite() || bw <= 0.0 {
            return Err(AppError::BadRequest("Podaj poprawną masę ciała (kg).".into()));
        }
        result.bodyweight_kg = Some(bw);
        if result.kind.eq_ignore_ascii_case("competition") {
            let profile = if let Some(uid) = result.user_id.as_deref() {
                state.db.find_profile_by_user_id(uid).await?
            } else {
                let name = result.athlete_name.trim().to_lowercase();
                state
                    .db
                    .list_profiles()
                    .await?
                    .into_iter()
                    .find(|p| p.display_name.trim().to_lowercase() == name)
            };
            if let Some(profile) = profile {
                result.category = Some(resolve_category_from_profile(
                    &profile,
                    bw,
                    Utc::now().date_naive(),
                )?);
            }
        }
    }

    if let Some(venue) = body.venue {
        let t = venue.trim().to_string();
        result.venue = if t.is_empty() { None } else { Some(t) };
    }

    if let Some(note) = body.reviewer_note {
        result.reviewer_note = {
            let t = note.trim().to_string();
            if t.is_empty() { None } else { Some(t) }
        };
    }

    if let Some(status) = body.status {
        result.status = status;
    } else if !is_staff && fields_touched {
        // Po poprawce zawodnika (także wcześniej zaakceptowanego) wraca do weryfikacji.
        result.status = ResultStatus::Pending;
    }

    let became_pending_from_athlete = !is_staff
        && fields_touched
        && result.status == ResultStatus::Pending;

    result.updated_at = chrono::Utc::now().to_rfc3339();
    state.db.upsert_result(result.clone()).await?;

    if result.status == ResultStatus::Accepted
        && result.kind.eq_ignore_ascii_case("competition")
    {
        let synced = state
            .db
            .apply_accepted_competition_to_profile(&result, None)
            .await?;
        if synced {
            state
                .db
                .append_log(
                    LogLevel::Info,
                    "results",
                    &format!(
                        "Zaktualizowano kategorię/masę w profilu po edycji/weryfikacji {} → {:?}",
                        result.event_name, result.category
                    ),
                    Some(&auth.user.id),
                )
                .await?;
        }
    }

    state
        .db
        .append_log(
            LogLevel::Info,
            "results",
            &format!(
                "Aktualizacja wyniku {id} → {:?}{}",
                result.status,
                if fields_touched { " (pola)" } else { "" }
            ),
            Some(&auth.user.id),
        )
        .await?;

    if became_pending_from_athlete {
        let _ = state
            .db
            .notify_staff(
                "Wynik do ponownej weryfikacji",
                &format!(
                    "{} · {} ({}) — poprawiony przez zawodnika",
                    result.athlete_name, result.event_name, result.kind
                ),
                "result",
                Some("/klub/weryfikacja-wynikow"),
                Some(&auth.user.id),
            )
            .await;
    }

    if is_staff {
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
    }

    Ok(Json(result))
}
