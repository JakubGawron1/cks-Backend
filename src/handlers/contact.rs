use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::{AppError, AppResult};
use crate::models::club::{ContactMessage, LogLevel};
use crate::models::role::Role;
use crate::models::user::{ErrorBody, OkResponse};
use crate::state::AppState;

const STAFF: &[Role] = &[Role::Trener, Role::Admin, Role::Superadmin];

#[derive(Debug, Deserialize, ToSchema)]
pub struct ContactMessageBody {
    pub name: String,
    pub email: String,
    pub phone: String,
    pub subject: String,
    pub body: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateContactMessageBody {
    pub read: bool,
}

#[utoipa::path(
    post,
    path = "/api/contact",
    request_body = ContactMessageBody,
    responses(
        (status = 200, description = "Wiadomość zapisana", body = ContactMessage),
        (status = 400, description = "Nieprawidłowe dane wejściowe", body = ErrorBody),
    ),
    tag = "contact"
)]
pub async fn submit_contact(
    State(state): State<AppState>,
    Json(body): Json<ContactMessageBody>,
) -> AppResult<Json<ContactMessage>> {
    let name = body.name.trim().to_string();
    let email = body.email.trim().to_lowercase();
    let phone = body.phone.trim().to_string();
    let subject = body.subject.trim().to_string();
    let message_body = body.body.trim().to_string();

    if name.len() < 2 {
        return Err(AppError::BadRequest(
            "Podaj imię i nazwisko (min. 2 znaki).".into(),
        ));
    }
    if name.len() > 120 {
        return Err(AppError::BadRequest("Imię i nazwisko jest za długie.".into()));
    }
    if !email.contains('@') || email.len() < 5 || email.len() > 200 {
        return Err(AppError::BadRequest("Podaj poprawny adres e-mail.".into()));
    }
    if phone.len() < 6 || phone.len() > 40 {
        return Err(AppError::BadRequest("Podaj poprawny numer telefonu.".into()));
    }
    if subject.is_empty() || subject.len() > 200 {
        return Err(AppError::BadRequest(
            "Podaj tytuł wiadomości (1–200 znaków).".into(),
        ));
    }
    if message_body.is_empty() || message_body.len() > 5000 {
        return Err(AppError::BadRequest(
            "Podaj treść wiadomości (1–5000 znaków).".into(),
        ));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let message = ContactMessage {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        email,
        phone,
        subject,
        body: message_body,
        read: false,
        created_at: now,
        read_at: None,
        read_by: None,
    };

    state.db.upsert_contact_message(message.clone()).await?;
    let _ = state
        .db
        .append_log(
            LogLevel::Info,
            "contact",
            &format!("Nowa wiadomość kontaktowa: {}", message.subject),
            None,
        )
        .await;

    crate::mail::notify_staff_email(
        &state,
        "Nowa wiadomość kontaktowa",
        &format!("{}: {}", message.name, message.subject),
        "contact",
        Some("/klub/wiadomosci"),
        None,
        crate::mail::EmailChannel::Contact,
    )
    .await;

    let (subject, html) =
        crate::mail::templates::contact_confirmation(&message.name, &message.subject);
    if state.db.is_flag_enabled("email_contact_confirmation").await {
        if let Err(err) = state.mailer.send(&message.email, &subject, &html).await {
            tracing::warn!(error = %err, "contact: confirmation email failed");
        }
    }

    Ok(Json(message))
}

#[utoipa::path(
    get,
    path = "/api/contact/messages",
    responses(
        (status = 200, description = "Skrzynka wiadomości kontaktowych", body = Vec<ContactMessage>),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "contact"
)]
pub async fn list_contact_messages(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<ContactMessage>>> {
    ensure_roles(&auth, STAFF)?;
    Ok(Json(state.db.list_contact_messages().await?))
}

#[utoipa::path(
    patch,
    path = "/api/contact/messages/{id}",
    params(("id" = String, Path, description = "ID wiadomości")),
    request_body = UpdateContactMessageBody,
    responses(
        (status = 200, description = "Zaktualizowano status odczytu", body = ContactMessage),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Wiadomość nie istnieje", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "contact"
)]
pub async fn update_contact_message(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateContactMessageBody>,
) -> AppResult<Json<ContactMessage>> {
    ensure_roles(&auth, STAFF)?;

    let existing = state
        .db
        .get_contact_message(&id)
        .await?
        .ok_or_else(|| AppError::NotFound("Wiadomość nie istnieje.".into()))?;

    let now = chrono::Utc::now().to_rfc3339();
    let message = ContactMessage {
        read: body.read,
        read_at: if body.read {
            Some(now)
        } else {
            None
        },
        read_by: if body.read {
            Some(auth.user.id.clone())
        } else {
            None
        },
        ..existing
    };

    state.db.upsert_contact_message(message.clone()).await?;
    Ok(Json(message))
}

#[utoipa::path(
    delete,
    path = "/api/contact/messages/{id}",
    params(("id" = String, Path, description = "ID wiadomości")),
    responses(
        (status = 200, description = "Usunięto wiadomość", body = OkResponse),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Wiadomość nie istnieje", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "contact"
)]
pub async fn delete_contact_message(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<OkResponse>> {
    ensure_roles(&auth, STAFF)?;

    let existing = state
        .db
        .get_contact_message(&id)
        .await?
        .ok_or_else(|| AppError::NotFound("Wiadomość nie istnieje.".into()))?;

    state.db.delete_contact_message(&id).await?;
    state
        .db
        .append_log(
            LogLevel::Warn,
            "contact",
            &format!("Usunięto wiadomość kontaktową: {}", existing.subject),
            Some(&auth.user.id),
        )
        .await?;

    Ok(Json(OkResponse { ok: true }))
}
