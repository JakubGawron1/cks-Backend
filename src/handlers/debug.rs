use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::{AppError, AppResult};
use crate::mail::templates;
use crate::models::club::LogLevel;
use crate::models::role::Role;
use crate::models::user::ErrorBody;
use crate::state::AppState;
use axum::extract::State;

#[derive(Debug, Deserialize, ToSchema)]
pub struct SendTestEmailBody {
    pub email: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SendTestEmailResponse {
    pub ok: bool,
    pub to: String,
    /// true gdy EMAIL_ENABLED i jest klucz — faktyczna próba Resend; false = tylko log.
    pub delivered: bool,
    pub message: String,
}

#[utoipa::path(
    post,
    path = "/api/admin/debug/send-test-email",
    request_body = SendTestEmailBody,
    responses(
        (status = 200, description = "Wysłano / zalogowano testowy e-mail", body = SendTestEmailResponse),
        (status = 400, description = "Nieprawidłowy e-mail", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 500, description = "Błąd wysyłki", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "admin"
)]
pub async fn send_test_email(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<SendTestEmailBody>,
) -> AppResult<Json<SendTestEmailResponse>> {
    ensure_roles(&auth, &[Role::Superadmin])?;

    if !state.db.is_flag_enabled("email_test").await {
        return Err(AppError::BadRequest(
            "Wysyłka testowych e-maili jest wyłączona (flaga email_test).".into(),
        ));
    }

    let to = body.email.trim().to_ascii_lowercase();
    if !to.contains('@') || to.len() < 5 || to.len() > 200 {
        return Err(AppError::BadRequest("Podaj poprawny adres e-mail.".into()));
    }

    let (subject, html) = templates::debug_test(&auth.user.email, &to);
    state.mailer.send(&to, &subject, &html).await?;

    let delivered = state.config.email_enabled && state.config.brevo_api_key.is_some();
    let message = if delivered {
        format!("Wysłano testowy e-mail na {to} (Brevo).")
    } else {
        format!(
            "EMAIL_ENABLED=false lub brak BREVO_API_KEY — treść zalogowana w backendzie (to={to})."
        )
    };

    state
        .db
        .append_log(
            LogLevel::Info,
            "debug",
            &format!(
                "Superadmin {} wysłał testowy e-mail na {}",
                auth.user.email, to
            ),
            Some(&auth.user.id),
        )
        .await?;

    Ok(Json(SendTestEmailResponse {
        ok: true,
        to,
        delivered,
        message,
    }))
}
