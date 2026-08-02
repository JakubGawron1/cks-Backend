use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AthleteProfile {
    pub id: String,
    pub user_id: String,
    pub display_name: String,
    pub bodyweight_kg: Option<f64>,
    pub category: Option<String>,
    pub notes: Option<String>,
    /// URL zdjęcia profilowego
    #[serde(default)]
    pub photo_url: Option<String>,
    /// Data urodzenia ISO (YYYY-MM-DD)
    #[serde(default)]
    pub birth_date: Option<String>,
    /// "male" | "female" — do Sinclair
    #[serde(default)]
    pub sex: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[schema(rename_all = "lowercase")]
pub enum FlagKind {
    Stable,
    Experimental,
}

/// Stan wdrożenia flagi w kodzie (źródło prawdy: katalog backendu).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
#[schema(rename_all = "snake_case")]
pub enum FlagRolloutStatus {
    Wired,
    Partial,
    Stub,
    #[default]
    Planned,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FeatureFlag {
    pub key: String,
    pub label: String,
    pub enabled: bool,
    pub kind: FlagKind,
    /// Opis dla DevTools — katalog flag na backendzie.
    #[serde(default)]
    #[schema(required)]
    pub description: String,
    /// Czy funkcja jest już podpięta w kodzie.
    #[serde(default)]
    #[schema(required)]
    pub rollout_status: FlagRolloutStatus,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PublicFlag {
    pub key: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(rename_all = "snake_case")]
pub enum ResultStatus {
    Pending,
    Accepted,
    Rejected,
    NeedsEdit,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CompetitionResult {
    pub id: String,
    pub athlete_name: String,
    pub user_id: Option<String>,
    pub event_name: String,
    /// "competition" | "training"
    #[serde(default = "default_result_kind")]
    pub kind: String,
    pub snatch_kg: Option<f64>,
    pub clean_jerk_kg: Option<f64>,
    pub total_kg: Option<f64>,
    /// Masa ciała na zawodach (kg) — do Sinclair
    #[serde(default)]
    pub bodyweight_kg: Option<f64>,
    /// Miejsce zawodów
    #[serde(default)]
    pub venue: Option<String>,
    /// Kategoria wagowa na starcie
    #[serde(default)]
    pub category: Option<String>,
    pub status: ResultStatus,
    pub reviewer_note: Option<String>,
    pub submitted_at: String,
    pub updated_at: String,
}

fn default_result_kind() -> String {
    "competition".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AttendanceSession {
    pub token: String,
    pub label: String,
    pub created_at: String,
    pub refreshed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AttendanceRecord {
    pub id: String,
    pub user_id: String,
    pub display_name: String,
    pub checked_at: String,
    pub session_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PlanExercise {
    pub id: String,
    pub name: String,
    pub sets: Option<u32>,
    pub reps: Option<String>,
    pub load_kg: Option<f64>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TrainingPlan {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub week_label: Option<String>,
    pub exercises: Vec<PlanExercise>,
    /// Puste = widoczny dla wszystkich zawodników
    pub assigned_user_ids: Vec<String>,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PlanProgressEntry {
    pub exercise_id: String,
    pub completed: bool,
    pub athlete_note: Option<String>,
    pub actual_load_kg: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TrainingPlanProgress {
    pub id: String,
    pub plan_id: String,
    pub user_id: String,
    pub entries: Vec<PlanProgressEntry>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AthleteStats {
    pub results_accepted: usize,
    pub results_pending: usize,
    pub results_total: usize,
    pub attendance_month: usize,
    pub attendance_window: usize,
    pub plans_active: usize,
    pub plans_completed_exercises: usize,
    pub bodyweight_kg: Option<f64>,
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "lowercase")]
#[schema(rename_all = "lowercase")]
pub enum CmsStatus {
    Draft,
    Published,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CmsBlock {
    pub id: String,
    #[serde(rename = "type")]
    pub block_type: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CmsPage {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub status: CmsStatus,
    pub blocks: Vec<CmsBlock>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
#[schema(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SystemLog {
    pub id: String,
    pub level: LogLevel,
    pub source: String,
    pub message: String,
    pub actor_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SiteStats {
    pub users: usize,
    pub active_users: usize,
    pub athlete_profiles: usize,
    pub cms_pages: usize,
    pub cms_published: usize,
    pub results_pending: usize,
    pub results_total: usize,
    pub feature_flags: usize,
    pub system_logs: usize,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
    pub service: String,
    pub auth: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ContactMessage {
    pub id: String,
    pub name: String,
    pub email: String,
    /// Numer w formacie międzynarodowym, np. +48 500123456.
    pub phone: String,
    pub subject: String,
    pub body: String,
    pub read: bool,
    pub created_at: String,
    pub read_at: Option<String>,
    pub read_by: Option<String>,
}

/// Powiadomienie in-app (skrzynka dzwonka).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Notification {
    pub id: String,
    pub user_id: String,
    pub title: String,
    pub body: String,
    /// "contact" | "result" | "system"
    pub kind: String,
    /// Ścieżka frontendu, np. `/klub/wiadomosci`
    pub href: Option<String>,
    pub read: bool,
    pub created_at: String,
    pub read_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UnreadCountResponse {
    pub count: usize,
}
