use std::net::IpAddr;

use axum::http::{HeaderValue, Method};
use thiserror::Error;
use tower_http::cors::{AllowOrigin, CorsLayer};

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub turso_auth_token: Option<String>,
    pub jwt_secret: String,
    pub jwt_expiry_hours: i64,
    pub host: IpAddr,
    pub port: u16,
    pub frontend_origins: Vec<String>,
    pub seed_superadmin_email: String,
    pub seed_superadmin_password: String,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("brak wymaganej zmiennej środowiskowej: {0}")]
    Missing(&'static str),
    #[error("nieprawidłowa wartość konfiguracji: {0}")]
    Invalid(String),
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "file:./data/slavia.redb".to_string());

        let turso_auth_token = std::env::var("TURSO_AUTH_TOKEN").ok().filter(|v| !v.is_empty());

        if (database_url.starts_with("libsql://") || database_url.starts_with("https://"))
            && turso_auth_token.is_none()
        {
            return Err(ConfigError::Missing("TURSO_AUTH_TOKEN"));
        }

        let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| {
            "dev-only-change-me-cks-slavia-super-secret-key".to_string()
        });

        let jwt_expiry_hours = std::env::var("JWT_EXPIRY_HOURS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(72);

        let host = std::env::var("HOST")
            .unwrap_or_else(|_| "0.0.0.0".to_string())
            .parse()
            .map_err(|_| ConfigError::Invalid("HOST".into()))?;

        let port = std::env::var("PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8080);

        let frontend_origins = std::env::var("FRONTEND_ORIGIN")
            .unwrap_or_else(|_| "http://localhost:3000,http://127.0.0.1:3000".to_string())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();

        if frontend_origins.is_empty() {
            return Err(ConfigError::Invalid("FRONTEND_ORIGIN".into()));
        }

        let seed_superadmin_email = std::env::var("SEED_SUPERADMIN_EMAIL")
            .or_else(|_| std::env::var("SEED_ADMIN_EMAIL"))
            .unwrap_or_else(|_| "superadmin@cks-slavia.local".to_string());
        let seed_superadmin_password = std::env::var("SEED_SUPERADMIN_PASSWORD")
            .or_else(|_| std::env::var("SEED_ADMIN_PASSWORD"))
            .unwrap_or_else(|_| "superadmin123!".to_string());

        Ok(Self {
            database_url,
            turso_auth_token,
            jwt_secret,
            jwt_expiry_hours,
            host,
            port,
            frontend_origins,
            seed_superadmin_email,
            seed_superadmin_password,
        })
    }

    pub fn is_remote_db(&self) -> bool {
        self.database_url.starts_with("libsql://")
            || self.database_url.starts_with("https://")
    }

    pub fn cors_layer(&self) -> Result<CorsLayer, ConfigError> {
        let origins = self
            .frontend_origins
            .iter()
            .map(|o| {
                o.parse::<HeaderValue>()
                    .map_err(|_| ConfigError::Invalid(format!("FRONTEND_ORIGIN: {o}")))
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers([
                axum::http::header::AUTHORIZATION,
                axum::http::header::CONTENT_TYPE,
                axum::http::header::ACCEPT,
            ]))
    }
}
