use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::auth::extractor::AuthUser;
use crate::auth::jwt::issue_token;
use crate::auth::password::{hash_password, verify_password};
use crate::error::{AppError, AppResult};
use crate::models::club::LogLevel;
use crate::models::user::{normalize_ui_theme, ErrorBody, PublicUser};
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
    )
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
    security(("bearer_auth" = []))
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
}

/// Aktualizacja własnego konta (nazwa / hasło / motyw) — dostępne dla zalogowanego użytkownika.
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
    security(("bearer_auth" = []))
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
