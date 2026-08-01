use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::auth::jwt::decode_token;
use crate::error::{AppError, AppResult};
use crate::models::role::Role;
use crate::models::user::{PublicUser, UserRecord};
use crate::state::AppState;

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user: UserRecord,
}

impl AuthUser {
    pub fn public(&self) -> PublicUser {
        PublicUser::from(&self.user)
    }

    pub fn roles(&self) -> &[Role] {
        &self.user.roles
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("Brak tokenu autoryzacji.".into()))?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or_else(|| AppError::Unauthorized("Oczekiwano nagłówka Bearer.".into()))?;

        let claims = decode_token(token, &state.config.jwt_secret)?;
        let user = state
            .db
            .find_user_by_id(&claims.sub)
            .await?
            .ok_or_else(|| AppError::Unauthorized("Użytkownik nie istnieje.".into()))?;

        if !user.is_active {
            return Err(AppError::Forbidden("Konto jest nieaktywne.".into()));
        }

        Ok(AuthUser { user })
    }
}

/// Ekstraktor wymagający co najmniej jednej z podanych ról (superadmin zawsze przechodzi).
pub struct RequireRoles {
    pub user: AuthUser,
}

pub fn ensure_roles(user: &AuthUser, required: &[Role]) -> AppResult<()> {
    use crate::models::role::has_any_role;
    if has_any_role(user.roles(), required) {
        Ok(())
    } else {
        Err(AppError::Forbidden(
            "Brak uprawnień do tego zasobu.".into(),
        ))
    }
}
