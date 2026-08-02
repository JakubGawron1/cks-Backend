use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::{AppError, AppResult};
use crate::models::club::LogLevel;
use crate::models::role::{has_any_role, Role};
use crate::models::user::{ErrorBody, OkResponse, PublicUser};
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateUserBody {
    pub email: String,
    pub password: String,
    pub display_name: String,
    pub roles: Vec<Role>,
    pub photo_url: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateUserBody {
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub roles: Option<Vec<Role>>,
    pub is_active: Option<bool>,
    pub password: Option<String>,
    pub photo_url: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/users",
    responses(
        (status = 200, description = "Lista użytkowników", body = Vec<PublicUser>),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_users(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<PublicUser>>> {
    ensure_roles(&auth, &[Role::Trener, Role::Admin])?;
    let users = state.db.list_users().await?;
    // Admin/SA: pełna lista; trener: tylko konta z rolą zawodnik
    let filtered: Vec<PublicUser> = if has_any_role(auth.roles(), &[Role::Admin]) {
        users.iter().map(PublicUser::from).collect()
    } else {
        users
            .iter()
            .filter(|u| u.roles.contains(&Role::Zawodnik))
            .map(PublicUser::from)
            .collect()
    };
    Ok(Json(filtered))
}

#[utoipa::path(
    post,
    path = "/api/users",
    request_body = CreateUserBody,
    responses(
        (status = 200, description = "Utworzono użytkownika", body = PublicUser),
        (status = 400, description = "Nieprawidłowe dane wejściowe", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateUserBody>,
) -> AppResult<Json<PublicUser>> {
    ensure_roles(&auth, &[Role::Trener, Role::Admin])?;

    if body.email.trim().is_empty() || body.password.is_empty() || body.display_name.trim().is_empty()
    {
        return Err(AppError::BadRequest(
            "Wymagane: e-mail, hasło i nazwa wyświetlana.".into(),
        ));
    }

    let is_admin = has_any_role(auth.roles(), &[Role::Admin]);
    let roles = if is_admin {
        // Tylko superadmin może nadawać rolę superadmin
        if body.roles.contains(&Role::Superadmin)
            && !auth.roles().contains(&Role::Superadmin)
        {
            return Err(AppError::Forbidden(
                "Tylko Superadmin może nadawać rolę superadmin.".into(),
            ));
        }
        body.roles
    } else {
        // Trener może tworzyć wyłącznie konta zawodników
        vec![Role::Zawodnik]
    };

    let user = state.db.create_user(
        &body.email,
        &body.password,
        &body.display_name,
        roles,
        body.photo_url,
    ).await?;

    if user.roles.contains(&Role::Zawodnik) {
        state
            .db
            .sync_photo_user_to_profile(&user.id, &user.photo_url)
            .await?;
    }

    state.db.append_log(
        LogLevel::Info,
        "users",
        &format!("Utworzono konto {}", user.email),
        Some(&auth.user.id),
    ).await?;

    Ok(Json(PublicUser::from(&user)))
}

#[utoipa::path(
    patch,
    path = "/api/users/{id}",
    params(("id" = String, Path, description = "ID użytkownika")),
    request_body = UpdateUserBody,
    responses(
        (status = 200, description = "Zaktualizowano użytkownika", body = PublicUser),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Użytkownik nie istnieje", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateUserBody>,
) -> AppResult<Json<PublicUser>> {
    ensure_roles(&auth, &[Role::Admin])?;

    let mut user = state
        .db
        .find_user_by_id(&id)
        .await?
        .ok_or_else(|| AppError::NotFound("Użytkownik nie istnieje.".into()))?;

    if let Some(email) = body.email {
        user.email = email.trim().to_ascii_lowercase();
    }
    if let Some(name) = body.display_name {
        user.display_name = name.trim().to_string();
    }
    if let Some(roles) = body.roles {
        if roles.contains(&Role::Superadmin)
            && !auth.roles().contains(&Role::Superadmin)
            && !user.roles.contains(&Role::Superadmin)
        {
            return Err(AppError::Forbidden(
                "Tylko Superadmin może nadawać rolę superadmin.".into(),
            ));
        }
        user.roles = roles;
    }
    if let Some(active) = body.is_active {
        user.is_active = active;
    }
    if let Some(password) = body.password {
        if !password.is_empty() {
            user.password_hash = crate::auth::password::hash_password(&password)?;
        }
    }
    if let Some(photo_url) = body.photo_url {
        let trimmed = photo_url.trim().to_string();
        user.photo_url = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        };
    }

    state.db.update_user(&user).await?;

    if user.roles.contains(&Role::Zawodnik) {
        state
            .db
            .sync_photo_user_to_profile(&user.id, &user.photo_url)
            .await?;
    }

    state.db.append_log(
        LogLevel::Info,
        "users",
        &format!("Zaktualizowano konto {}", user.email),
        Some(&auth.user.id),
    ).await?;

    Ok(Json(PublicUser::from(&user)))
}

#[utoipa::path(
    delete,
    path = "/api/users/{id}",
    params(("id" = String, Path, description = "ID użytkownika")),
    responses(
        (status = 200, description = "Usunięto użytkownika", body = OkResponse),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Użytkownik nie istnieje", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<OkResponse>> {
    ensure_roles(&auth, &[Role::Admin])?;

    let target = state
        .db
        .find_user_by_id(&id)
        .await?
        .ok_or_else(|| AppError::NotFound("Użytkownik nie istnieje.".into()))?;

    state.db.delete_user(&id).await?;
    state.db.append_log(
        LogLevel::Warn,
        "users",
        &format!("Usunięto konto {}", target.email),
        Some(&auth.user.id),
    ).await?;

    Ok(Json(OkResponse { ok: true }))
}
