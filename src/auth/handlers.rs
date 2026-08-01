use axum::Json;
use serde::{Deserialize, Serialize};

use crate::auth::extractor::AuthUser;
use crate::auth::jwt::issue_token;
use crate::error::{AppError, AppResult};
use crate::models::user::PublicUser;
use crate::state::AppState;
use axum::extract::State;

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub token_type: &'static str,
    pub expires_in_hours: i64,
    pub user: PublicUser,
}

pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> AppResult<Json<LoginResponse>> {
    let email = body.email.trim();
    if email.is_empty() || body.password.is_empty() {
        return Err(AppError::BadRequest("Podaj e-mail i hasło.".into()));
    }

    let user = state.db.authenticate(email, &body.password).await?;
    let token = issue_token(
        &user,
        &state.config.jwt_secret,
        state.config.jwt_expiry_hours,
    )?;

    Ok(Json(LoginResponse {
        token,
        token_type: "Bearer",
        expires_in_hours: state.config.jwt_expiry_hours,
        user: PublicUser::from(&user),
    }))
}

pub async fn me(auth: AuthUser) -> AppResult<Json<PublicUser>> {
    Ok(Json(auth.public()))
}
