use axum::extract::FromRequestParts;
use axum::http::{Method, request::Parts};

use crate::auth::jwt::decode_token;
use crate::error::{AppError, AppResult};
use crate::models::role::{has_any_role, Role};
use crate::models::user::{PublicUser, UserRecord};
use crate::state::AppState;

pub const VIEW_AS_HEADER: &str = "x-view-as-user";

#[derive(Debug, Clone)]
pub struct AuthUser {
    /// Zawsze podmiot JWT (prawdziwy actor).
    pub user: UserRecord,
    /// Opcjonalny target podglądu (tylko gdy actor = superadmin + nagłówek).
    pub view_as: Option<UserRecord>,
}

impl AuthUser {
    pub fn public(&self) -> PublicUser {
        PublicUser::from(self.effective())
    }

    pub fn roles(&self) -> &[Role] {
        &self.user.roles
    }

    pub fn effective(&self) -> &UserRecord {
        self.view_as.as_ref().unwrap_or(&self.user)
    }

    pub fn effective_id(&self) -> &str {
        &self.effective().id
    }

    pub fn is_previewing(&self) -> bool {
        self.view_as.is_some()
    }
}

fn is_preview_control_path(path: &str) -> bool {
    path == "/api/admin/preview/start" || path == "/api/admin/preview/stop"
}

fn is_safe_method(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let path = parts.uri.path().to_string();
        let method = parts.method.clone();
        let auth_header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                tracing::debug!(%path, "auth: brak nagłówka Authorization");
                AppError::Unauthorized("Brak tokenu autoryzacji.".into())
            })?;

        let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
            tracing::warn!(%path, "auth: nieprawidłowy schemat Authorization");
            AppError::Unauthorized("Oczekiwano nagłówka Bearer.".into())
        })?;

        let claims = decode_token(token, &state.config.jwt_secret).map_err(|err| {
            tracing::warn!(%path, error = %err, "auth: nieprawidłowy lub wygasły token");
            err
        })?;
        let user = state
            .db
            .find_user_by_id(&claims.sub)
            .await?
            .ok_or_else(|| {
                tracing::warn!(%path, user_id = %claims.sub, "auth: użytkownik z tokenu nie istnieje");
                AppError::Unauthorized("Użytkownik nie istnieje.".into())
            })?;

        if !user.is_active {
            tracing::warn!(%path, user_id = %user.id, email = %user.email, "auth: konto nieaktywne");
            return Err(AppError::Forbidden("Konto jest nieaktywne.".into()));
        }

        let view_as_raw = parts
            .headers
            .get(VIEW_AS_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let view_as = if let Some(target_id) = view_as_raw {
            if !has_any_role(&user.roles, &[Role::Superadmin]) {
                tracing::warn!(
                    %path,
                    actor = %user.email,
                    "auth: X-View-As-User bez roli superadmin"
                );
                return Err(AppError::Forbidden(
                    "Podgląd konta wymaga roli superadmin.".into(),
                ));
            }
            if target_id == user.id {
                return Err(AppError::BadRequest(
                    "Nie można podglądać własnego konta.".into(),
                ));
            }
            let target = state
                .db
                .find_user_by_id(&target_id)
                .await?
                .ok_or_else(|| AppError::NotFound("Użytkownik podglądu nie istnieje.".into()))?;
            if !target.is_active {
                return Err(AppError::Forbidden(
                    "Konto podglądane jest nieaktywne.".into(),
                ));
            }
            Some(target)
        } else {
            None
        };

        if view_as.is_some() && !is_safe_method(&method) && !is_preview_control_path(&path) {
            tracing::info!(
                %path,
                %method,
                actor = %user.email,
                target = view_as.as_ref().map(|t| t.email.as_str()).unwrap_or("-"),
                "auth: odrzucono mutację w trybie podglądu"
            );
            return Err(AppError::Forbidden(
                "Podgląd konta jest tylko do odczytu.".into(),
            ));
        }

        tracing::debug!(
            %path,
            user_id = %user.id,
            email = %user.email,
            view_as = view_as.as_ref().map(|t| t.id.as_str()),
            "auth: OK"
        );
        Ok(AuthUser { user, view_as })
    }
}

/// Ekstraktor wymagający co najmniej jednej z podanych ról (superadmin zawsze przechodzi).
pub struct RequireRoles {
    pub user: AuthUser,
}

pub fn ensure_roles(user: &AuthUser, required: &[Role]) -> AppResult<()> {
    if has_any_role(user.roles(), required) {
        Ok(())
    } else {
        tracing::warn!(
            user_id = %user.user.id,
            email = %user.user.email,
            roles = ?user.roles(),
            required = ?required,
            "brak uprawnień"
        );
        Err(AppError::Forbidden(
            "Brak uprawnień do tego zasobu.".into(),
        ))
    }
}
