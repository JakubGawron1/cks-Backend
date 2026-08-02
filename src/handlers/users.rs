use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::{AppError, AppResult};
use crate::models::club::LogLevel;
use crate::models::role::Role;
use crate::models::user::PublicUser;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CreateUserBody {
    pub email: String,
    pub password: String,
    pub display_name: String,
    pub roles: Vec<Role>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserBody {
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub roles: Option<Vec<Role>>,
    pub is_active: Option<bool>,
    pub password: Option<String>,
}

pub async fn list_users(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<PublicUser>>> {
    ensure_roles(&auth, &[Role::Admin])?;
    let users = state
        .db
        .list_users().await?
        .iter()
        .map(PublicUser::from)
        .collect();
    Ok(Json(users))
}

pub async fn create_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateUserBody>,
) -> AppResult<Json<PublicUser>> {
    ensure_roles(&auth, &[Role::Admin])?;

    if body.email.trim().is_empty() || body.password.is_empty() || body.display_name.trim().is_empty()
    {
        return Err(AppError::BadRequest(
            "Wymagane: e-mail, hasło i nazwa wyświetlana.".into(),
        ));
    }

    // Tylko superadmin może nadawać rolę superadmin
    if body.roles.contains(&Role::Superadmin)
        && !auth.roles().contains(&Role::Superadmin)
    {
        return Err(AppError::Forbidden(
            "Tylko Superadmin może nadawać rolę superadmin.".into(),
        ));
    }

    let user = state.db.create_user(
        &body.email,
        &body.password,
        &body.display_name,
        body.roles,
    ).await?;

    state.db.append_log(
        LogLevel::Info,
        "users",
        &format!("Utworzono konto {}", user.email),
        Some(&auth.user.id),
    ).await?;

    Ok(Json(PublicUser::from(&user)))
}

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

    state.db.update_user(&user).await?;
    state.db.append_log(
        LogLevel::Info,
        "users",
        &format!("Zaktualizowano konto {}", user.email),
        Some(&auth.user.id),
    ).await?;

    Ok(Json(PublicUser::from(&user)))
}

pub async fn delete_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
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

    Ok(Json(serde_json::json!({ "ok": true })))
}
