use std::net::IpAddr;

use axum::http::{HeaderValue, Method};
use thiserror::Error;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::images::ImageProvider;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionMode {
    Dev,
    Production,
}

impl ProductionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Production => "production",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub production_mode: ProductionMode,
    pub database_url: String,
    pub turso_auth_token: Option<String>,
    pub jwt_secret: String,
    pub jwt_expiry_hours: i64,
    pub host: IpAddr,
    pub port: u16,
    pub frontend_origins: Vec<String>,
    pub seed_superadmin_email: String,
    pub seed_superadmin_password: String,
    /// Domyślnie ImageKit; Cloudinary — później (Todo.md).
    pub image_provider: ImageProvider,
    pub imagekit_public_key: Option<String>,
    pub imagekit_private_key: Option<String>,
    pub imagekit_url_endpoint: Option<String>,
    /// Wysyłka e-mail przez Brevo (gdy false — tylko log).
    pub email_enabled: bool,
    pub brevo_api_key: Option<String>,
    pub email_from: Option<String>,
    /// Legacy FCM server key — gdy brak, push jest no-op.
    pub fcm_server_key: Option<String>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("brak wymaganej zmiennej środowiskowej: {0}")]
    Missing(&'static str),
    #[error("nieprawidłowa wartość konfiguracji: {0}")]
    Invalid(String),
}

impl Config {
    /// Render ustawia `RENDER=true`.
    pub fn is_render() -> bool {
        std::env::var("RENDER").is_ok()
    }

    /// Hugging Face Spaces ustawia `SPACE_ID` (np. `koliber/cks-slavia`).
    pub fn is_huggingface() -> bool {
        std::env::var("SPACE_ID").is_ok()
    }

    /// Hosting produkcyjny (Render lub HF Space).
    pub fn is_hosted() -> bool {
        Self::is_render() || Self::is_huggingface()
    }

    fn production_mode_from_env() -> Result<ProductionMode, ConfigError> {
        match std::env::var("PRODUCTION_MODE") {
            Ok(raw) => match raw.trim().to_ascii_lowercase().as_str() {
                "dev" => Ok(ProductionMode::Dev),
                "production" => Ok(ProductionMode::Production),
                other => Err(ConfigError::Invalid(format!(
                    "PRODUCTION_MODE musi być 'dev' lub 'production' (otrzymano: {other})"
                ))),
            },
            // Na hostingu domyślnie production; lokalnie dev.
            Err(_) if Self::is_hosted() => Ok(ProductionMode::Production),
            Err(_) => Ok(ProductionMode::Dev),
        }
    }

    fn frontend_origins_from_env(strict: bool) -> Result<Vec<String>, ConfigError> {
        let raw = std::env::var("FRONTEND_ORIGIN").or_else(|_| std::env::var("CORS_ALLOWED_ORIGINS"));

        let raw = if strict {
            raw.map_err(|_| ConfigError::Missing("FRONTEND_ORIGIN"))?
        } else {
            raw.unwrap_or_else(|_| "http://localhost:3000,http://127.0.0.1:3000".to_string())
        };

        let origins = raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();

        if origins.is_empty() {
            return Err(ConfigError::Invalid("FRONTEND_ORIGIN".into()));
        }
        Ok(origins)
    }

    pub fn from_env() -> Result<Self, ConfigError> {
        let production_mode = Self::production_mode_from_env()?;
        let strict = production_mode == ProductionMode::Production || Self::is_hosted();

        let database_url = std::env::var("DATABASE_URL")
            .or_else(|_| std::env::var("TURSO_DATABASE_URL"))
            .unwrap_or_else(|_| match production_mode {
                ProductionMode::Dev => "file:./data/slavia.db".to_string(),
                ProductionMode::Production => String::new(),
            });

        if production_mode == ProductionMode::Production && database_url.is_empty() {
            return Err(ConfigError::Missing("DATABASE_URL"));
        }

        let turso_auth_token = std::env::var("TURSO_AUTH_TOKEN")
            .ok()
            .filter(|v| !v.is_empty());

        let remote = is_remote_url(&database_url);

        if production_mode == ProductionMode::Production {
            if !remote {
                return Err(ConfigError::Invalid(
                    "PRODUCTION_MODE=production wymaga DATABASE_URL=libsql://… (Turso)".into(),
                ));
            }
            if turso_auth_token.is_none() {
                return Err(ConfigError::Missing("TURSO_AUTH_TOKEN"));
            }
        } else if remote && turso_auth_token.is_none() {
            return Err(ConfigError::Missing("TURSO_AUTH_TOKEN"));
        }

        let jwt_secret = if strict {
            std::env::var("JWT_SECRET").map_err(|_| ConfigError::Missing("JWT_SECRET"))?
        } else {
            std::env::var("JWT_SECRET").unwrap_or_else(|_| {
                "dev-only-change-me-cks-slavia-super-secret-key".to_string()
            })
        };

        if jwt_secret.len() < 16 {
            return Err(ConfigError::Invalid(
                "JWT_SECRET musi mieć co najmniej 16 znaków".into(),
            ));
        }

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

        let frontend_origins = Self::frontend_origins_from_env(strict)?;

        let seed_superadmin_email = std::env::var("SEED_SUPERADMIN_EMAIL")
            .or_else(|_| std::env::var("SEED_ADMIN_EMAIL"))
            .unwrap_or_else(|_| "superadmin@cks-slavia.local".to_string());
        let seed_superadmin_password = if strict {
            std::env::var("SEED_SUPERADMIN_PASSWORD")
                .or_else(|_| std::env::var("SEED_ADMIN_PASSWORD"))
                .map_err(|_| ConfigError::Missing("SEED_SUPERADMIN_PASSWORD"))?
        } else {
            std::env::var("SEED_SUPERADMIN_PASSWORD")
                .or_else(|_| std::env::var("SEED_ADMIN_PASSWORD"))
                .unwrap_or_else(|_| "superadmin123!".to_string())
        };

        if strict && seed_superadmin_password == "superadmin123!" {
            return Err(ConfigError::Invalid(
                "SEED_SUPERADMIN_PASSWORD: ustaw własne hasło (nie domyślne)".into(),
            ));
        }

        let image_provider = match std::env::var("IMAGE_PROVIDER") {
            Ok(raw) => ImageProvider::parse(&raw).ok_or_else(|| {
                ConfigError::Invalid(
                    "IMAGE_PROVIDER musi być 'imagekit' lub 'cloudinary'".into(),
                )
            })?,
            Err(_) => ImageProvider::Imagekit,
        };

        let imagekit_public_key = std::env::var("IMAGEKIT_PUBLIC_KEY")
            .ok()
            .filter(|v| !v.is_empty());
        let imagekit_private_key = std::env::var("IMAGEKIT_PRIVATE_KEY")
            .ok()
            .filter(|v| !v.is_empty());
        let imagekit_url_endpoint = std::env::var("IMAGEKIT_URL_ENDPOINT")
            .ok()
            .filter(|v| !v.is_empty());

        let email_enabled = match std::env::var("EMAIL_ENABLED") {
            Ok(raw) => match raw.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => true,
                "0" | "false" | "no" | "off" => false,
                other => {
                    return Err(ConfigError::Invalid(format!(
                        "EMAIL_ENABLED musi być true/false (otrzymano: {other})"
                    )));
                }
            },
            // Dev: log only; production: enable when key present.
            Err(_) => {
                production_mode == ProductionMode::Production
                    && std::env::var("BREVO_API_KEY")
                        .ok()
                        .filter(|v| !v.is_empty())
                        .is_some()
            }
        };
        let brevo_api_key = std::env::var("BREVO_API_KEY")
            .ok()
            .filter(|v| !v.is_empty());
        let email_from = std::env::var("EMAIL_FROM")
            .ok()
            .filter(|v| !v.is_empty());

        let fcm_server_key = std::env::var("FCM_SERVER_KEY")
            .ok()
            .filter(|v| !v.is_empty());

        Ok(Self {
            production_mode,
            database_url,
            turso_auth_token,
            jwt_secret,
            jwt_expiry_hours,
            host,
            port,
            frontend_origins,
            seed_superadmin_email,
            seed_superadmin_password,
            image_provider,
            imagekit_public_key,
            imagekit_private_key,
            imagekit_url_endpoint,
            email_enabled,
            brevo_api_key,
            email_from,
            fcm_server_key,
        })
    }

    pub fn is_remote_db(&self) -> bool {
        is_remote_url(&self.database_url)
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
                axum::http::HeaderName::from_static("x-view-as-user"),
            ]))
    }
}

fn is_remote_url(url: &str) -> bool {
    url.starts_with("libsql://") || url.starts_with("https://")
}
