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

fn default_true() -> bool {
    true
}

/// Preferencje powiadomień e-mail (opt-out: brakujące = włączone).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct NotificationPrefs {
    #[serde(default = "default_true")]
    pub email_squad: bool,
    #[serde(default = "default_true")]
    pub email_training_plans: bool,
    #[serde(default = "default_true")]
    pub email_contact: bool,
}

impl Default for NotificationPrefs {
    fn default() -> Self {
        Self {
            email_squad: true,
            email_training_plans: true,
            email_contact: true,
        }
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
    #[serde(default)]
    pub email_verified: bool,
    #[serde(default)]
    pub pending_email: Option<String>,
    #[serde(default)]
    pub notification_prefs: NotificationPrefs,
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
    pub email_verified: bool,
    pub pending_email: Option<String>,
    pub notification_prefs: NotificationPrefs,
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
            email_verified: user.email_verified,
            pending_email: user.pending_email.clone(),
            notification_prefs: user.notification_prefs.clone(),
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
