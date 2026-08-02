use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::role::Role;

pub const DEFAULT_UI_THEME: &str = "classic";

pub const ALLOWED_UI_THEMES: &[&str] = &[
    // stable
    "classic",
    "dawn",
    "graphite",
    "forest",
    "arena",
    "mist",
    "ember",
    "slate",
    "sand",
    "night",
    // experimental
    "capsule",
    "studio",
    "dock",
    "bloom",
    "chalk",
    "forge",
    "ribbon",
    "pulse",
    "neon",
    "vapor",
];

pub fn default_ui_theme() -> String {
    DEFAULT_UI_THEME.to_string()
}

pub fn normalize_ui_theme(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if ALLOWED_UI_THEMES.contains(&trimmed) {
        Some(trimmed.to_string())
    } else {
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PublicUser {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub roles: Vec<Role>,
    #[serde(default)]
    pub is_active: bool,
    /// Motyw paneli (zawodnik / klub) — przypisany do konta.
    #[serde(default = "default_ui_theme")]
    pub ui_theme: String,
    /// Zdjęcie konta (dla zawodnika = zdjęcie profilu publicznego).
    #[serde(default)]
    pub photo_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UserRecord {
    pub id: String,
    pub email: String,
    pub password_hash: String,
    pub display_name: String,
    pub roles: Vec<Role>,
    pub is_active: bool,
    pub ui_theme: String,
    pub photo_url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<&UserRecord> for PublicUser {
    fn from(user: &UserRecord) -> Self {
        Self {
            id: user.id.clone(),
            email: user.email.clone(),
            display_name: user.display_name.clone(),
            roles: user.roles.clone(),
            is_active: user.is_active,
            ui_theme: user.ui_theme.clone(),
            photo_url: user.photo_url.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OkResponse {
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ErrorBody {
    pub error: String,
}
