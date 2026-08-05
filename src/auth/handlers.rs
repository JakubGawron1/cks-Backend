use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::auth::extractor::AuthUser;
use crate::auth::jwt::issue_token;
use crate::auth::password::{hash_password, verify_password};
use crate::auth::tokens::{hash_token, new_token};
use crate::db::{EmailToken, EmailTokenPurpose};
use crate::error::{AppError, AppResult};
use crate::mail::{is_dev_email, templates, Mailer};
use crate::models::club::LogLevel;
use crate::models::user::{normalize_ui_theme, ErrorBody, NotificationPrefs, OkResponse, PublicUser};
use crate::state::AppState;
use axum::extract::State;

#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LoginResponse {
    pub token: String,
    pub token_type: String,
    pub expires_in_hours: i64,
    pub user: PublicUser,
}

#[utoipa::path(
    post,
    path = "/api/auth/login",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Zalogowano", body = LoginResponse),
        (status = 400, description = "Nieprawidłowe dane wejściowe", body = ErrorBody),
        (status = 401, description = "Nieprawidłowy e-mail lub hasło", body = ErrorBody),
    ),
    tag = "auth"
)]
pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> AppResult<Json<LoginResponse>> {
    let email = body.email.trim();
    if email.is_empty() || body.password.is_empty() {
        tracing::warn!(email_empty = email.is_empty(), "login: brak e-maila lub hasła");
        return Err(AppError::BadRequest("Podaj e-mail i hasło.".into()));
    }

    tracing::info!(email = %email, "login: próba");
    let user = state.db.authenticate(email, &body.password).await?;
    let token = issue_token(
        &user,
        &state.config.jwt_secret,
        state.config.jwt_expiry_hours,
    )?;

    tracing::info!(
        email = %user.email,
        user_id = %user.id,
        roles = ?user.roles,
        "login: OK"
    );

    Ok(Json(LoginResponse {
        token,
        token_type: "Bearer".into(),
        expires_in_hours: state.config.jwt_expiry_hours,
        user: PublicUser::from(&user),
    }))
}

#[utoipa::path(
    get,
    path = "/api/auth/me",
    responses(
        (status = 200, description = "Dane zalogowanego użytkownika", body = PublicUser),
        (status = 401, description = "Unauthorized", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "auth"
)]
pub async fn me(auth: AuthUser) -> AppResult<Json<PublicUser>> {
    Ok(Json(auth.public()))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateMeBody {
    pub display_name: Option<String>,
    pub current_password: Option<String>,
    pub new_password: Option<String>,
    /// Motyw paneli (stable + experimental; lista w `ALLOWED_UI_THEMES`)
    pub ui_theme: Option<String>,
    /// Zdjęcie konta (URL — po uploadzie lub ręczny fallback)
    pub photo_url: Option<String>,
    pub notification_prefs: Option<NotificationPrefs>,
}

/// Aktualizacja własnego konta (nazwa / hasło / motyw / prefs) — dostępne dla zalogowanego użytkownika.
#[utoipa::path(
    patch,
    path = "/api/auth/me",
    request_body = UpdateMeBody,
    responses(
        (status = 200, description = "Zaktualizowano konto", body = PublicUser),
        (status = 400, description = "Nieprawidłowe dane wejściowe", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 404, description = "Użytkownik nie istnieje", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "auth"
)]
pub async fn update_me(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<UpdateMeBody>,
) -> AppResult<Json<PublicUser>> {
    let mut user = state
        .db
        .find_user_by_id(&auth.user.id)
        .await?
        .ok_or_else(|| AppError::NotFound("Użytkownik nie istnieje.".into()))?;

    let mut changed = false;

    if let Some(name) = body.display_name {
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() {
            return Err(AppError::BadRequest(
                "Nazwa wyświetlana nie może być pusta.".into(),
            ));
        }
        if trimmed != user.display_name {
            user.display_name = trimmed;
            changed = true;
        }
    }

    if let Some(theme) = body.ui_theme {
        let normalized = normalize_ui_theme(&theme).ok_or_else(|| {
            AppError::BadRequest("Nieznany motyw paneli.".into())
        })?;
        if normalized != user.ui_theme {
            user.ui_theme = normalized;
            changed = true;
        }
    }

    if let Some(photo_url) = body.photo_url {
        let next = {
            let trimmed = photo_url.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        };
        if next != user.photo_url {
            user.photo_url = next;
            changed = true;
        }
    }

    if let Some(prefs) = body.notification_prefs {
        if prefs != user.notification_prefs {
            user.notification_prefs = prefs;
            changed = true;
        }
    }

    if let Some(new_password) = body.new_password {
        if new_password.is_empty() {
            return Err(AppError::BadRequest("Nowe hasło nie może być puste.".into()));
        }
        if new_password.len() < 6 {
            return Err(AppError::BadRequest(
                "Nowe hasło musi mieć co najmniej 6 znaków.".into(),
            ));
        }
        let current = body.current_password.as_deref().unwrap_or("");
        if current.is_empty() {
            return Err(AppError::BadRequest(
                "Podaj aktualne hasło, aby ustawić nowe.".into(),
            ));
        }
        if !verify_password(current, &user.password_hash)? {
            return Err(AppError::BadRequest("Nieprawidłowe aktualne hasło.".into()));
        }
        user.password_hash = hash_password(&new_password)?;
        changed = true;
    }

    if !changed {
        tracing::debug!(user_id = %user.id, "update_me: bez zmian");
        return Ok(Json(PublicUser::from(&user)));
    }

    state.db.update_user(&user).await?;
    if user.roles.contains(&crate::models::role::Role::Zawodnik) {
        state
            .db
            .sync_photo_user_to_profile(&user.id, &user.photo_url)
            .await?;
    }
    tracing::info!(
        user_id = %user.id,
        email = %user.email,
        "update_me: zaktualizowano konto"
    );
    state
        .db
        .append_log(
            LogLevel::Info,
            "settings",
            &format!("Zaktualizowano własne ustawienia konta {}", user.email),
            Some(&auth.user.id),
        )
        .await?;

    Ok(Json(PublicUser::from(&user)))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RequestVerificationBody {
    /// Nowy adres (opcjonalnie). Brak / null = weryfikuj aktualny e-mail.
    pub email: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/auth/email/request-verification",
    request_body = RequestVerificationBody,
    responses(
        (status = 200, description = "Wysłano / pominięto (dev)", body = PublicUser),
        (status = 400, description = "Nieprawidłowe dane", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "auth"
)]
pub async fn request_email_verification(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<RequestVerificationBody>,
) -> AppResult<Json<PublicUser>> {
    let mut user = state
        .db
        .find_user_by_id(&auth.user.id)
        .await?
        .ok_or_else(|| AppError::NotFound("Użytkownik nie istnieje.".into()))?;

    let requested = body
        .email
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase());

    let target = requested.clone().unwrap_or_else(|| user.email.clone());

    if !target.contains('@') || target.len() < 5 {
        return Err(AppError::BadRequest("Podaj poprawny adres e-mail.".into()));
    }

    // Zmiana na inny adres — sprawdź unikalność.
    if target != user.email {
        if let Some(existing) = state.db.find_user_by_email(&target).await? {
            if existing.id != user.id {
                return Err(AppError::BadRequest(
                    "Konto z tym e-mailem już istnieje.".into(),
                ));
            }
        }
    }

    // Dev / .local — auto-verify.
    if is_dev_email(&target) {
        if target != user.email {
            user.email = target;
        }
        user.email_verified = true;
        user.pending_email = None;
        state.db.update_user(&user).await?;
        return Ok(Json(PublicUser::from(&user)));
    }

    if user.email_verified && requested.is_none() {
        return Ok(Json(PublicUser::from(&user)));
    }

    if target != user.email {
        user.pending_email = Some(target.clone());
        user.email_verified = false;
    } else {
        user.pending_email = None;
    }
    state.db.update_user(&user).await?;

    let raw = new_token();
    let token_hash = hash_token(&raw);
    state
        .db
        .invalidate_email_tokens_for_user(&user.id, EmailTokenPurpose::Verify)
        .await?;
    let expires = chrono::Utc::now() + chrono::Duration::hours(48);
    state
        .db
        .upsert_email_token(&EmailToken {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: user.id.clone(),
            purpose: EmailTokenPurpose::Verify,
            token_hash,
            target_email: target.clone(),
            expires_at: expires.to_rfc3339(),
            used_at: None,
        })
        .await?;

    let origin = Mailer::primary_frontend_origin(&state.config);
    let verify_url = format!(
        "{}/weryfikacja-emaila?token={}",
        origin.trim_end_matches('/'),
        raw
    );
    let (subject, html) = templates::verify_email(&user.display_name, &verify_url);
    if !state.db.is_flag_enabled("email_verification").await {
        return Err(AppError::BadRequest(
            "Wysyłka e-maili weryfikacyjnych jest wyłączona (flaga email_verification)."
                .into(),
        ));
    }
    state.mailer.send(&target, &subject, &html).await?;

    Ok(Json(PublicUser::from(&user)))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ConfirmEmailBody {
    pub token: String,
}

#[utoipa::path(
    post,
    path = "/api/auth/email/confirm",
    request_body = ConfirmEmailBody,
    responses(
        (status = 200, description = "E-mail potwierdzony", body = OkResponse),
        (status = 400, description = "Nieprawidłowy lub wygasły token", body = ErrorBody),
    ),
    tag = "auth"
)]
pub async fn confirm_email(
    State(state): State<AppState>,
    Json(body): Json<ConfirmEmailBody>,
) -> AppResult<Json<OkResponse>> {
    let raw = body.token.trim();
    if raw.is_empty() {
        return Err(AppError::BadRequest("Brak tokenu weryfikacji.".into()));
    }
    let token_hash = hash_token(raw);
    let token = state
        .db
        .find_email_token_by_hash(&token_hash, EmailTokenPurpose::Verify)
        .await?
        .ok_or_else(|| AppError::BadRequest("Nieprawidłowy lub użyty link weryfikacji.".into()))?;

    let expires = chrono::DateTime::parse_from_rfc3339(&token.expires_at)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|_| AppError::BadRequest("Nieprawidłowy token.".into()))?;
    if expires < chrono::Utc::now() {
        return Err(AppError::BadRequest("Link weryfikacji wygasł.".into()));
    }

    let mut user = state
        .db
        .find_user_by_id(&token.user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Użytkownik nie istnieje.".into()))?;

    let target = token.target_email.to_ascii_lowercase();
    if target != user.email {
        if let Some(existing) = state.db.find_user_by_email(&target).await? {
            if existing.id != user.id {
                return Err(AppError::BadRequest(
                    "Konto z tym e-mailem już istnieje.".into(),
                ));
            }
        }
        user.email = target;
    }
    user.email_verified = true;
    user.pending_email = None;
    state.db.update_user(&user).await?;
    state.db.mark_email_token_used(token).await?;

    Ok(Json(OkResponse { ok: true }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ForgotPasswordBody {
    pub email: String,
}

#[utoipa::path(
    post,
    path = "/api/auth/forgot-password",
    request_body = ForgotPasswordBody,
    responses(
        (status = 200, description = "Jeśli konto istnieje — wysłano mail", body = OkResponse),
        (status = 400, description = "Nieprawidłowe dane", body = ErrorBody),
    ),
    tag = "auth"
)]
pub async fn forgot_password(
    State(state): State<AppState>,
    Json(body): Json<ForgotPasswordBody>,
) -> AppResult<Json<OkResponse>> {
    let email = body.email.trim().to_ascii_lowercase();
    if email.is_empty() {
        return Err(AppError::BadRequest("Podaj e-mail.".into()));
    }

    // Zawsze 200 — bez enumeracji kont.
    let reset_enabled = state.db.is_flag_enabled("email_password_reset").await;
    if let Some(user) = state.db.find_user_by_email(&email).await? {
        if user.is_active && reset_enabled {
            let raw = new_token();
            let token_hash = hash_token(&raw);
            state
                .db
                .invalidate_email_tokens_for_user(&user.id, EmailTokenPurpose::Reset)
                .await?;
            let expires = chrono::Utc::now() + chrono::Duration::hours(1);
            state
                .db
                .upsert_email_token(&EmailToken {
                    id: uuid::Uuid::new_v4().to_string(),
                    user_id: user.id.clone(),
                    purpose: EmailTokenPurpose::Reset,
                    token_hash,
                    target_email: user.email.clone(),
                    expires_at: expires.to_rfc3339(),
                    used_at: None,
                })
                .await?;

            let origin = Mailer::primary_frontend_origin(&state.config);
            let reset_url = format!(
                "{}/reset-hasla?token={}",
                origin.trim_end_matches('/'),
                raw
            );
            let (subject, html) = templates::reset_password(&user.display_name, &reset_url);
            if let Err(err) = state.mailer.send(&user.email, &subject, &html).await {
                tracing::warn!(error = %err, "forgot_password: send failed");
            }
        } else if user.is_active && !reset_enabled {
            tracing::info!("forgot_password: flaga email_password_reset wyłączona — pominięto wysyłkę");
        }
    }

    Ok(Json(OkResponse { ok: true }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ResetPasswordBody {
    pub token: String,
    pub new_password: String,
}

#[utoipa::path(
    post,
    path = "/api/auth/reset-password",
    request_body = ResetPasswordBody,
    responses(
        (status = 200, description = "Hasło zmienione", body = OkResponse),
        (status = 400, description = "Nieprawidłowy token / hasło", body = ErrorBody),
    ),
    tag = "auth"
)]
pub async fn reset_password(
    State(state): State<AppState>,
    Json(body): Json<ResetPasswordBody>,
) -> AppResult<Json<OkResponse>> {
    let raw = body.token.trim();
    if raw.is_empty() {
        return Err(AppError::BadRequest("Brak tokenu resetu.".into()));
    }
    if body.new_password.len() < 6 {
        return Err(AppError::BadRequest(
            "Nowe hasło musi mieć co najmniej 6 znaków.".into(),
        ));
    }

    let token_hash = hash_token(raw);
    let token = state
        .db
        .find_email_token_by_hash(&token_hash, EmailTokenPurpose::Reset)
        .await?
        .ok_or_else(|| AppError::BadRequest("Nieprawidłowy lub użyty link resetu.".into()))?;

    let expires = chrono::DateTime::parse_from_rfc3339(&token.expires_at)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|_| AppError::BadRequest("Nieprawidłowy token.".into()))?;
    if expires < chrono::Utc::now() {
        return Err(AppError::BadRequest("Link resetu hasła wygasł.".into()));
    }

    let mut user = state
        .db
        .find_user_by_id(&token.user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Użytkownik nie istnieje.".into()))?;

    user.password_hash = hash_password(&body.new_password)?;
    state.db.update_user(&user).await?;
    state.db.mark_email_token_used(token).await?;
    state
        .db
        .append_log(
            LogLevel::Info,
            "auth",
            &format!("Zresetowano hasło: {}", user.email),
            Some(&user.id),
        )
        .await?;

    Ok(Json(OkResponse { ok: true }))
}
