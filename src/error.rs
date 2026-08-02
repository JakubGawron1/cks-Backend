use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use thiserror::Error;

use crate::models::user::ErrorBody;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    NotFound(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl AppError {
    pub fn unauthorized() -> Self {
        Self::Unauthorized("Nieprawidłowy e-mail lub hasło.".into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            AppError::Forbidden(_) => StatusCode::FORBIDDEN,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        let message = match &self {
            AppError::Internal(err) => {
                tracing::error!(status = %status.as_u16(), error = ?err, "internal error");
                "Wewnętrzny błąd serwera.".to_string()
            }
            AppError::BadRequest(msg) => {
                tracing::warn!(status = %status.as_u16(), message = %msg, "bad request");
                msg.clone()
            }
            AppError::Unauthorized(msg) => {
                tracing::warn!(status = %status.as_u16(), message = %msg, "unauthorized");
                msg.clone()
            }
            AppError::Forbidden(msg) => {
                tracing::warn!(status = %status.as_u16(), message = %msg, "forbidden");
                msg.clone()
            }
            AppError::NotFound(msg) => {
                tracing::warn!(status = %status.as_u16(), message = %msg, "not found");
                msg.clone()
            }
        };

        (status, Json(ErrorBody { error: message })).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;

/// Helper: mapuj dowolny błąd do Internal.
pub fn internal<E: std::fmt::Display + std::fmt::Debug + Send + Sync + 'static>(
    err: E,
) -> AppError {
    AppError::Internal(anyhow::anyhow!("{err}"))
}
