use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::{AppError, AppResult};
use crate::models::club::{
    AssignedAthleteBrief, AthleteCalendarEvent, AthleteProfile, AttendanceRecord, CalendarEvent,
    EventWithdrawal, LogLevel, PublicCalendarEvent, TrainingScheduleDefaults, WithdrawalStatus,
};
use crate::models::role::Role;
use crate::models::user::{ErrorBody, OkResponse};
use crate::state::AppState;

#[derive(Debug, Deserialize, IntoParams)]
pub struct EventsQuery {
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EventBody {
    pub title: String,
    pub event_type: String,
    pub date: String,
    /// Koniec (włącznie). Puste / brak = jednodniowe.
    #[serde(default)]
    pub end_date: Option<String>,
    pub time: Option<String>,
    pub location: Option<String>,
    pub description: Option<String>,
    pub assigned_athlete_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct WithdrawBody {
    pub reason: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RestoreBody {
    /// Wymuś przywrócenie mimo kolizji treningu tego samego dnia
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RestoreConflictResponse {
    pub warning: String,
    pub conflicting_event_ids: Vec<String>,
}

use serde::Serialize;

fn staff_roles() -> [Role; 2] {
    [Role::Trener, Role::Admin]
}

fn validate_and_normalize(body: &EventBody) -> AppResult<(bool, bool, Vec<String>, Option<String>)> {
    let title = body.title.trim();
    if title.is_empty() {
        return Err(AppError::BadRequest("Podaj tytuł wydarzenia.".into()));
    }
    let date = body.date.trim();
    if date.len() != 10 {
        return Err(AppError::BadRequest("Podaj datę w formacie YYYY-MM-DD.".into()));
    }
    let event_type = body.event_type.trim();
    if event_type != "zawody" && event_type != "trening" {
        return Err(AppError::BadRequest(
            "Typ wydarzenia: zawody lub trening.".into(),
        ));
    }
    let all_athletes = event_type == "trening";
    let club_assigned = true;
    let mut assigned = body.assigned_athlete_ids.clone().unwrap_or_default();
    if all_athletes {
        assigned.clear();
    }

    // Treningi są jednodniowe; zawody mogą mieć zakres.
    let end_date = if all_athletes {
        None
    } else {
        let raw = body
            .end_date
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        match raw {
            None => None,
            Some(end) => {
                if end.len() != 10 {
                    return Err(AppError::BadRequest(
                        "Podaj datę końcową w formacie YYYY-MM-DD.".into(),
                    ));
                }
                if end < date {
                    return Err(AppError::BadRequest(
                        "Data końcowa nie może być wcześniejsza niż data rozpoczęcia.".into(),
                    ));
                }
                if end == date {
                    None
                } else {
                    Some(end.to_string())
                }
            }
        }
    };

    Ok((all_athletes, club_assigned, assigned, end_date))
}

async fn ensure_athlete_ids(state: &AppState, ids: &[String]) -> AppResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let profiles = state.db.list_profiles().await?;
    for id in ids {
        if !profiles.iter().any(|p| p.id == *id) {
            return Err(AppError::BadRequest(format!(
                "Nieznany profil zawodnika: {id}"
            )));
        }
    }
    Ok(())
}

fn to_public(e: &CalendarEvent) -> PublicCalendarEvent {
    PublicCalendarEvent {
        id: e.id.clone(),
        title: e.title.clone(),
        event_type: e.event_type.clone(),
        date: e.date.clone(),
        end_date: e.end_date.clone(),
        time: e.time.clone(),
        location: e.location.clone(),
        description: e.description.clone(),
        status: e.status.clone(),
        cancellation_note: e.cancellation_note.clone(),
    }
}

fn to_athlete_view(
    state: &AppState,
    e: &CalendarEvent,
    user_id: &str,
    my_ids: &[String],
    profiles: &[AthleteProfile],
    attendance: &[AttendanceRecord],
) -> AthleteCalendarEvent {
    let i_am_assigned = state
        .db
        .athlete_is_effectively_assigned(e, my_ids, user_id);

    let my_withdrawal_status = e
        .withdrawals
        .iter()
        .filter(|w| my_ids.contains(&w.athlete_id) || w.user_id.as_deref() == Some(user_id))
        .max_by_key(|w| w.at.clone())
        .map(|w| match w.status {
            WithdrawalStatus::Pending => "pending".to_string(),
            WithdrawalStatus::Accepted => "accepted".to_string(),
            WithdrawalStatus::Rejected => "rejected".to_string(),
        });

    // Treningi all_athletes — bez pełnej listy w DTO (UI i tak nie pokazuje składu).
    // Zawody — tylko ogłoszony skład.
    let assigned_athletes = if e.event_type == "zawody" && !e.all_athletes {
        e.assigned_athlete_ids
            .iter()
            .filter_map(|id| profiles.iter().find(|p| p.id == *id))
            .map(|p| AssignedAthleteBrief {
                id: p.id.clone(),
                display_name: p.display_name.clone(),
            })
            .collect()
    } else {
        Vec::new()
    };

    let roster_announced =
        e.event_type != "zawody" || !e.assigned_athlete_ids.is_empty() || e.all_athletes;

    let attendance_status = if e.event_type == "trening" {
        attendance
            .iter()
            .find(|r| {
                r.user_id == user_id
                    && r.event_id.as_deref() == Some(e.id.as_str())
                    && (r.status == "present" || r.status == "absent")
            })
            .map(|r| r.status.clone())
            .or_else(|| {
                if my_withdrawal_status.as_deref() == Some("accepted") {
                    Some("withdrawn".into())
                } else {
                    None
                }
            })
    } else {
        None
    };

    AthleteCalendarEvent {
        id: e.id.clone(),
        title: e.title.clone(),
        event_type: e.event_type.clone(),
        date: e.date.clone(),
        end_date: e.end_date.clone(),
        time: e.time.clone(),
        location: e.location.clone(),
        description: e.description.clone(),
        status: e.status.clone(),
        cancellation_note: e.cancellation_note.clone(),
        club_assigned: e.club_assigned,
        all_athletes: e.all_athletes,
        assigned_athletes,
        i_am_assigned,
        roster_announced,
        my_withdrawal_status,
        attendance_status,
    }
}

#[utoipa::path(
    get,
    path = "/api/events",
    params(EventsQuery),
    responses(
        (status = 200, description = "Lista wydarzeń (kadra)", body = Vec<CalendarEvent>),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_events(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<EventsQuery>,
) -> AppResult<Json<Vec<CalendarEvent>>> {
    ensure_roles(&auth, &staff_roles())?;
    Ok(Json(
        state
            .db
            .list_events_in_range(query.from.as_deref(), query.to.as_deref())
            .await?,
    ))
}

#[utoipa::path(
    get,
    path = "/api/public/events",
    params(EventsQuery),
    responses(
        (status = 200, description = "Publiczne wydarzenia klubowe", body = Vec<PublicCalendarEvent>),
    ),
    tag = "public"
)]
pub async fn list_public_events(
    State(state): State<AppState>,
    Query(query): Query<EventsQuery>,
) -> AppResult<Json<Vec<PublicCalendarEvent>>> {
    let items = state
        .db
        .list_public_events(query.from.as_deref(), query.to.as_deref())
        .await?;
    Ok(Json(items.iter().map(to_public).collect()))
}

#[utoipa::path(
    get,
    path = "/api/events/mine",
    params(EventsQuery),
    responses(
        (status = 200, description = "Kalendarz zawodnika", body = Vec<AthleteCalendarEvent>),
        (status = 401, description = "Unauthorized", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_my_events(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<EventsQuery>,
) -> AppResult<Json<Vec<AthleteCalendarEvent>>> {
    ensure_roles(
        &auth,
        &[Role::Zawodnik, Role::Trener, Role::Admin],
    )?;
    // Tylko niedawne treningi — pełne historyczne reconcile nie blokuje kalendarza.
    let _ = state
        .db
        .reconcile_past_training_attendance_since_days(21)
        .await;

    let profiles = state.db.list_profiles().await?;
    let uid = auth.effective_id();
    let my_ids: Vec<String> = profiles
        .iter()
        .filter(|p| p.user_id == uid)
        .map(|p| p.id.clone())
        .collect();
    let attendance = state.db.list_attendance_raw().await?;

    let events = state
        .db
        .list_events_in_range(query.from.as_deref(), query.to.as_deref())
        .await?;

    let out: Vec<AthleteCalendarEvent> = events
        .into_iter()
        .filter(|e| {
            e.club_assigned
                || e.all_athletes
                || my_ids
                    .iter()
                    .any(|id| e.assigned_athlete_ids.contains(id))
        })
        .map(|e| {
            to_athlete_view(
                &state,
                &e,
                uid,
                &my_ids,
                &profiles,
                &attendance,
            )
        })
        .collect();
    Ok(Json(out))
}

#[utoipa::path(
    post,
    path = "/api/events",
    request_body = EventBody,
    responses(
        (status = 200, description = "Utworzono wydarzenie", body = CalendarEvent),
        (status = 400, description = "Nieprawidłowe dane", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_event(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<EventBody>,
) -> AppResult<Json<CalendarEvent>> {
    ensure_roles(&auth, &staff_roles())?;
    let (all_athletes, club_assigned, assigned, end_date) = validate_and_normalize(&body)?;
    ensure_athlete_ids(&state, &assigned).await?;
    let now = chrono::Utc::now().to_rfc3339();
    let event = CalendarEvent {
        id: uuid::Uuid::new_v4().to_string(),
        title: body.title.trim().to_string(),
        event_type: body.event_type.trim().to_string(),
        date: body.date.trim().to_string(),
        end_date,
        time: body.time.filter(|t| !t.trim().is_empty()),
        location: body.location.filter(|t| !t.trim().is_empty()),
        description: body.description.filter(|t| !t.trim().is_empty()),
        status: "scheduled".into(),
        cancellation_note: None,
        club_assigned,
        source: "manual".into(),
        locked: true,
        all_athletes,
        assigned_athlete_ids: assigned,
        withdrawals: vec![],
        created_by: auth.user.id.clone(),
        created_at: now.clone(),
        updated_at: now,
    };
    state.db.upsert_event(event.clone()).await?;
    state
        .db
        .append_log(
            LogLevel::Info,
            "calendar",
            &format!("Utworzono wydarzenie: {}", event.title),
            Some(&auth.user.id),
        )
        .await?;
    Ok(Json(event))
}

#[utoipa::path(
    patch,
    path = "/api/events/{id}",
    request_body = EventBody,
    params(("id" = String, Path, description = "ID wydarzenia")),
    responses(
        (status = 200, description = "Zaktualizowano wydarzenie", body = CalendarEvent),
        (status = 400, description = "Nieprawidłowe dane", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Nie znaleziono", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_event(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<EventBody>,
) -> AppResult<Json<CalendarEvent>> {
    ensure_roles(&auth, &staff_roles())?;
    let mut event = state
        .db
        .get_event(&id)
        .await?
        .ok_or_else(|| AppError::NotFound("Nie znaleziono wydarzenia.".into()))?;

    let (all_athletes, club_assigned, assigned, end_date) = validate_and_normalize(&body)?;
    ensure_athlete_ids(&state, &assigned).await?;

    let date_changed = event.date != body.date.trim() || event.end_date != end_date;
    let time_changed = event.time.as_deref() != body.time.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty());
    let prev_assigned = event.assigned_athlete_ids.clone();

    event.title = body.title.trim().to_string();
    event.event_type = body.event_type.trim().to_string();
    event.date = body.date.trim().to_string();
    event.end_date = end_date;
    event.time = body.time.filter(|t| !t.trim().is_empty());
    event.location = body.location.filter(|t| !t.trim().is_empty());
    event.description = body.description.filter(|t| !t.trim().is_empty());
    event.club_assigned = club_assigned;
    event.all_athletes = all_athletes;
    event.assigned_athlete_ids = assigned.clone();
    event.locked = true;
    event.updated_at = chrono::Utc::now().to_rfc3339();

    state.db.upsert_event(event.clone()).await?;

    if date_changed || time_changed {
        let when = format!(
            "{}{}",
            event.date,
            event
                .time
                .as_ref()
                .map(|t| format!(" {t}"))
                .unwrap_or_default()
        );
        let _ = state
            .db
            .notify_staff(
                "Przeniesiono wydarzenie",
                &format!("{} → {}", event.title, when),
                "calendar",
                Some("/klub/kalendarz"),
                Some(&auth.user.id),
            )
            .await;
        // notify assigned athletes
        let profiles = state.db.list_profiles().await?;
        for pid in &event.assigned_athlete_ids {
            if let Some(p) = profiles.iter().find(|x| x.id == *pid) {
                if p.user_id != "manual" {
                    crate::mail::notify_user(
                        &state,
                        &p.user_id,
                        "Przeniesiono wydarzenie",
                        &format!("{} → {}", event.title, when),
                        "calendar",
                        Some("/panel/kalendarz"),
                        crate::mail::EmailChannel::None,
                    )
                    .await;
                }
            }
        }
    }

    // newly assigned / removed (e-mail tylko dla zawodów)
    let is_zawody = event.event_type == "zawody";
    let profiles = state.db.list_profiles().await.unwrap_or_default();
    for pid in &assigned {
        if !prev_assigned.contains(pid) {
            if let Some(p) = profiles.iter().find(|x| x.id == *pid) {
                if p.user_id != "manual" {
                    let email_ch = if is_zawody {
                        crate::mail::EmailChannel::Squad
                    } else {
                        crate::mail::EmailChannel::None
                    };
                    crate::mail::notify_user(
                        &state,
                        &p.user_id,
                        "Dopisano do składu",
                        &format!("Jesteś na składzie: {}", event.title),
                        "calendar",
                        Some("/panel/kalendarz"),
                        email_ch,
                    )
                    .await;
                }
            }
        }
    }
    if is_zawody {
        for pid in &prev_assigned {
            if !assigned.contains(pid) {
                if let Some(p) = profiles.iter().find(|x| x.id == *pid) {
                    if p.user_id != "manual" {
                        crate::mail::notify_user(
                            &state,
                            &p.user_id,
                            "Wypisano ze składu",
                            &format!("Usunięto Cię ze składu: {}", event.title),
                            "calendar",
                            Some("/panel/kalendarz"),
                            crate::mail::EmailChannel::Squad,
                        )
                        .await;
                    }
                }
            }
        }
    }

    Ok(Json(event))
}

#[utoipa::path(
    delete,
    path = "/api/events/{id}",
    params(("id" = String, Path, description = "ID wydarzenia")),
    responses(
        (status = 200, description = "Usunięto", body = OkResponse),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Nie znaleziono", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_event(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<OkResponse>> {
    ensure_roles(&auth, &staff_roles())?;
    let event = state
        .db
        .get_event(&id)
        .await?
        .ok_or_else(|| AppError::NotFound("Nie znaleziono wydarzenia.".into()))?;
    if event.event_type == "trening" {
        state.db.add_training_skip_date(&event.date).await?;
    }
    state.db.delete_event(&id).await?;
    Ok(Json(OkResponse { ok: true }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CancelBody {
    pub cancellation_note: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/events/{id}/cancel",
    params(("id" = String, Path, description = "ID wydarzenia")),
    request_body = CancelBody,
    responses(
        (status = 200, description = "Odwołano", body = CalendarEvent),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Nie znaleziono", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn cancel_event(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<CancelBody>,
) -> AppResult<Json<CalendarEvent>> {
    ensure_roles(&auth, &staff_roles())?;
    let mut event = state
        .db
        .get_event(&id)
        .await?
        .ok_or_else(|| AppError::NotFound("Nie znaleziono wydarzenia.".into()))?;
    event.status = "cancelled".into();
    event.cancellation_note = body
        .cancellation_note
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    event.locked = true;
    event.updated_at = chrono::Utc::now().to_rfc3339();
    state.db.upsert_event(event.clone()).await?;

    let note = event
        .cancellation_note
        .as_deref()
        .unwrap_or("bez podanego powodu");
    let _ = state
        .db
        .notify_staff(
            "Odwołano wydarzenie",
            &format!("{} — {}", event.title, note),
            "calendar",
            Some("/klub/kalendarz"),
            Some(&auth.user.id),
        )
        .await;

    Ok(Json(event))
}

#[utoipa::path(
    post,
    path = "/api/events/{id}/restore",
    params(("id" = String, Path, description = "ID wydarzenia")),
    request_body = RestoreBody,
    responses(
        (status = 200, description = "Przywrócono", body = CalendarEvent),
        (status = 409, description = "Kolizja treningu", body = RestoreConflictResponse),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Nie znaleziono", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn restore_event(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<RestoreBody>,
) -> AppResult<Json<CalendarEvent>> {
    ensure_roles(&auth, &staff_roles())?;
    let mut event = state
        .db
        .get_event(&id)
        .await?
        .ok_or_else(|| AppError::NotFound("Nie znaleziono wydarzenia.".into()))?;

    if event.event_type == "trening" && !body.force {
        let conflicts: Vec<String> = state
            .db
            .list_events()
            .await?
            .into_iter()
            .filter(|e| {
                e.id != event.id
                    && e.event_type == "trening"
                    && e.status == "scheduled"
                    && e.date == event.date
            })
            .map(|e| e.id)
            .collect();
        if !conflicts.is_empty() {
            return Err(AppError::BadRequest(format!(
                "Na ten dzień jest już inny trening. Wyślij ponownie z force=true. Konflikty: {}",
                conflicts.join(", ")
            )));
        }
    }

    event.status = "scheduled".into();
    event.cancellation_note = None;
    event.updated_at = chrono::Utc::now().to_rfc3339();
    state.db.upsert_event(event.clone()).await?;
    Ok(Json(event))
}

#[utoipa::path(
    post,
    path = "/api/events/{id}/withdraw",
    params(("id" = String, Path, description = "ID wydarzenia")),
    request_body = WithdrawBody,
    responses(
        (status = 200, description = "Złożono rezygnację", body = CalendarEvent),
        (status = 400, description = "Nieprawidłowe dane", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn withdraw_from_event(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<WithdrawBody>,
) -> AppResult<Json<CalendarEvent>> {
    ensure_roles(
        &auth,
        &[Role::Zawodnik, Role::Trener, Role::Admin],
    )?;
    let reason = body.reason.trim();
    if reason.is_empty() {
        return Err(AppError::BadRequest("Podaj powód rezygnacji.".into()));
    }

    let mut event = state
        .db
        .get_event(&id)
        .await?
        .ok_or_else(|| AppError::NotFound("Nie znaleziono wydarzenia.".into()))?;

    if event.status != "scheduled" {
        return Err(AppError::BadRequest(
            "Nie można rezygnować z odwołanego wydarzenia.".into(),
        ));
    }

    let profiles = state.db.list_profiles().await?;
    let my_profile = profiles
        .iter()
        .find(|p| p.user_id == auth.user.id)
        .ok_or_else(|| AppError::BadRequest("Brak profilu zawodnika.".into()))?;

    let my_ids = vec![my_profile.id.clone()];
    if !state
        .db
        .athlete_is_effectively_assigned(&event, &my_ids, &auth.user.id)
        && !event.all_athletes
        && !event.assigned_athlete_ids.contains(&my_profile.id)
    {
        // For zawody pending check — allow if on roster even if... actually need on roster
        if event.event_type == "zawody" && !event.assigned_athlete_ids.contains(&my_profile.id) {
            return Err(AppError::BadRequest(
                "Nie jesteś na składzie tych zawodów.".into(),
            ));
        }
        if event.event_type == "trening"
            && state
                .db
                .athlete_has_accepted_withdrawal(&event, &my_ids, &auth.user.id)
        {
            return Err(AppError::BadRequest("Już zrezygnowałeś z tego treningu.".into()));
        }
    }

    if event.event_type == "zawody" && !event.assigned_athlete_ids.contains(&my_profile.id) {
        return Err(AppError::BadRequest(
            "Nie jesteś na składzie tych zawodów.".into(),
        ));
    }

    let now = chrono::Utc::now().to_rfc3339();

    if event.event_type == "zawody" {
        if event.withdrawals.iter().any(|w| {
            w.athlete_id == my_profile.id && w.status == WithdrawalStatus::Pending
        }) {
            return Err(AppError::BadRequest(
                "Prośba o rezygnację już oczekuje na akceptację.".into(),
            ));
        }
        event.withdrawals.push(EventWithdrawal {
            athlete_id: my_profile.id.clone(),
            user_id: Some(auth.user.id.clone()),
            reason: reason.to_string(),
            at: now.clone(),
            status: WithdrawalStatus::Pending,
        });
        event.updated_at = now;
        state.db.upsert_event(event.clone()).await?;
        let _ = state
            .db
            .notify_staff(
                "Prośba o rezygnację z zawodów",
                &format!(
                    "{} chce zrezygnować z „{}”: {}",
                    auth.user.display_name, event.title, reason
                ),
                "calendar",
                Some("/klub/kalendarz"),
                Some(&auth.user.id),
            )
            .await;
    } else {
        // trening — accepted od razu
        event.withdrawals.retain(|w| w.athlete_id != my_profile.id);
        event.withdrawals.push(EventWithdrawal {
            athlete_id: my_profile.id.clone(),
            user_id: Some(auth.user.id.clone()),
            reason: reason.to_string(),
            at: now.clone(),
            status: WithdrawalStatus::Accepted,
        });
        event.updated_at = now;
        state.db.upsert_event(event.clone()).await?;
        let _ = state
            .db
            .notify_staff(
                "Rezygnacja z treningu",
                &format!(
                    "{} nie będzie na „{}”: {}",
                    auth.user.display_name, event.title, reason
                ),
                "calendar",
                Some("/klub/kalendarz"),
                Some(&auth.user.id),
            )
            .await;
    }

    Ok(Json(event))
}

#[utoipa::path(
    post,
    path = "/api/events/{id}/withdrawals/{athlete_id}/accept",
    params(
        ("id" = String, Path, description = "ID wydarzenia"),
        ("athlete_id" = String, Path, description = "ID profilu"),
    ),
    responses(
        (status = 200, description = "Zaakceptowano rezygnację", body = CalendarEvent),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Nie znaleziono", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn accept_withdrawal(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, athlete_id)): Path<(String, String)>,
) -> AppResult<Json<CalendarEvent>> {
    ensure_roles(&auth, &staff_roles())?;
    let mut event = state
        .db
        .get_event(&id)
        .await?
        .ok_or_else(|| AppError::NotFound("Nie znaleziono wydarzenia.".into()))?;
    if event.event_type != "zawody" {
        return Err(AppError::BadRequest(
            "Akceptacja dotyczy tylko zawodów.".into(),
        ));
    }
    let w = event
        .withdrawals
        .iter_mut()
        .find(|w| w.athlete_id == athlete_id && w.status == WithdrawalStatus::Pending)
        .ok_or_else(|| AppError::NotFound("Brak oczekującej rezygnacji.".into()))?;
    w.status = WithdrawalStatus::Accepted;
    let user_id = w.user_id.clone();
    event
        .assigned_athlete_ids
        .retain(|x| x != &athlete_id);
    event.updated_at = chrono::Utc::now().to_rfc3339();
    state.db.upsert_event(event.clone()).await?;
    if let Some(uid) = user_id {
        crate::mail::notify_user(
            &state,
            &uid,
            "Rezygnacja zaakceptowana",
            &format!("Wypisano Cię ze składu: {}", event.title),
            "calendar",
            Some("/panel/kalendarz"),
            crate::mail::EmailChannel::Squad,
        )
        .await;
    }
    Ok(Json(event))
}

#[utoipa::path(
    post,
    path = "/api/events/{id}/withdrawals/{athlete_id}/reject",
    params(
        ("id" = String, Path, description = "ID wydarzenia"),
        ("athlete_id" = String, Path, description = "ID profilu"),
    ),
    responses(
        (status = 200, description = "Odrzucono rezygnację", body = CalendarEvent),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Nie znaleziono", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn reject_withdrawal(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, athlete_id)): Path<(String, String)>,
) -> AppResult<Json<CalendarEvent>> {
    ensure_roles(&auth, &staff_roles())?;
    let mut event = state
        .db
        .get_event(&id)
        .await?
        .ok_or_else(|| AppError::NotFound("Nie znaleziono wydarzenia.".into()))?;
    let w = event
        .withdrawals
        .iter_mut()
        .find(|w| w.athlete_id == athlete_id && w.status == WithdrawalStatus::Pending)
        .ok_or_else(|| AppError::NotFound("Brak oczekującej rezygnacji.".into()))?;
    w.status = WithdrawalStatus::Rejected;
    let user_id = w.user_id.clone();
    event.updated_at = chrono::Utc::now().to_rfc3339();
    state.db.upsert_event(event.clone()).await?;
    if let Some(uid) = user_id {
        crate::mail::notify_user(
            &state,
            &uid,
            "Rezygnacja odrzucona",
            &format!("Pozostajesz na składzie: {}", event.title),
            "calendar",
            Some("/panel/kalendarz"),
            crate::mail::EmailChannel::Squad,
        )
        .await;
    }
    Ok(Json(event))
}

#[utoipa::path(
    post,
    path = "/api/events/{id}/withdrawals/{athlete_id}/clear",
    params(
        ("id" = String, Path, description = "ID wydarzenia"),
        ("athlete_id" = String, Path, description = "ID profilu"),
    ),
    responses(
        (status = 200, description = "Cofnięto rezygnację z treningu", body = CalendarEvent),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn clear_withdrawal(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, athlete_id)): Path<(String, String)>,
) -> AppResult<Json<CalendarEvent>> {
    ensure_roles(&auth, &staff_roles())?;
    let mut event = state
        .db
        .get_event(&id)
        .await?
        .ok_or_else(|| AppError::NotFound("Nie znaleziono wydarzenia.".into()))?;
    event.withdrawals.retain(|w| w.athlete_id != athlete_id);
    event.updated_at = chrono::Utc::now().to_rfc3339();
    state.db.upsert_event(event.clone()).await?;
    Ok(Json(event))
}

#[utoipa::path(
    get,
    path = "/api/events/schedule",
    responses(
        (status = 200, description = "Terminarz treningów", body = TrainingScheduleDefaults),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_schedule(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<TrainingScheduleDefaults>> {
    ensure_roles(&auth, &staff_roles())?;
    Ok(Json(state.db.get_training_schedule_defaults().await?))
}

#[utoipa::path(
    patch,
    path = "/api/events/schedule",
    request_body = TrainingScheduleDefaults,
    responses(
        (status = 200, description = "Zapisano terminarz", body = TrainingScheduleDefaults),
        (status = 400, description = "Nieprawidłowe dane", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_schedule(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<TrainingScheduleDefaults>,
) -> AppResult<Json<TrainingScheduleDefaults>> {
    ensure_roles(&auth, &staff_roles())?;
    if body.weekdays.is_empty() {
        return Err(AppError::BadRequest(
            "Wybierz co najmniej jeden dzień tygodnia.".into(),
        ));
    }
    for d in &body.weekdays {
        if !(1..=7).contains(d) {
            return Err(AppError::BadRequest(
                "Dni tygodnia: liczby 1–7 (pon–niedz).".into(),
            ));
        }
    }
    state.db.set_training_schedule_defaults(&body).await?;
    state
        .db
        .prune_seeded_trainings_outside_schedule(&body)
        .await?;
    state.db.seed_training_events().await?;
    Ok(Json(body))
}
