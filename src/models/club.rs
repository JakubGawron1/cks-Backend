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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
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
    /// Data zawodów / treningu (YYYY-MM-DD)
    #[serde(default)]
    pub event_date: Option<String>,
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
    /// Deprecated — stały QR klubowy nie jest powiązany z treningiem (trening rozwiązywany przy skanie).
    #[serde(default)]
    #[schema(deprecated)]
    pub event_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AttendanceRecord {
    pub id: String,
    pub user_id: String,
    pub display_name: String,
    pub checked_at: String,
    pub session_token: String,
    #[serde(default)]
    pub event_id: Option<String>,
    /// "present" | "absent" | "pending_unauthorized" | "rejected"
    #[serde(default = "default_attendance_status")]
    pub status: String,
    /// "qr" | "auto" | "manual"
    #[serde(default = "default_attendance_source")]
    pub source: String,
}

fn default_attendance_status() -> String {
    "present".into()
}

fn default_attendance_source() -> String {
    "qr".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(rename_all = "snake_case")]
pub enum WithdrawalStatus {
    Pending,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EventWithdrawal {
    pub athlete_id: String,
    #[serde(default)]
    pub user_id: Option<String>,
    pub reason: String,
    pub at: String,
    pub status: WithdrawalStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    /// "zawody" | "trening"
    pub event_type: String,
    /// YYYY-MM-DD — początek
    pub date: String,
    /// YYYY-MM-DD — koniec (włącznie). Brak / None = jednodniowe (`date`).
    #[serde(default)]
    pub end_date: Option<String>,
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// "scheduled" | "cancelled"
    pub status: String,
    #[serde(default)]
    pub cancellation_note: Option<String>,
    pub club_assigned: bool,
    /// "seeded" | "manual"
    pub source: String,
    pub locked: bool,
    /// true = wszyscy aktywni zawodnicy (treningi)
    pub all_athletes: bool,
    #[serde(default)]
    pub assigned_athlete_ids: Vec<String>,
    #[serde(default)]
    pub withdrawals: Vec<EventWithdrawal>,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
}

impl CalendarEvent {
    /// Koniec wydarzenia (włącznie) — dla jednodniowych równy `date`.
    pub fn end_date_inclusive(&self) -> &str {
        self.end_date
            .as_deref()
            .filter(|d| d.len() == 10)
            .unwrap_or(self.date.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TrainingScheduleDefaults {
    /// ISO weekdays 1=Mon … 7=Sun
    pub weekdays: Vec<u8>,
    pub time: String,
    pub end_time: String,
    pub location: String,
    pub title: String,
    pub attendance_buffer_minutes: u32,
}

impl Default for TrainingScheduleDefaults {
    fn default() -> Self {
        Self {
            weekdays: vec![1, 3, 5],
            time: "15:00".into(),
            end_time: "18:00".into(),
            location: "ul. Konopnickiej 13, Ruda Śląska".into(),
            title: "Trening klubowy".into(),
            attendance_buffer_minutes: 60,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PublicCalendarEvent {
    pub id: String,
    pub title: String,
    pub event_type: String,
    pub date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancellation_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AssignedAthleteBrief {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AthleteCalendarEvent {
    pub id: String,
    pub title: String,
    pub event_type: String,
    pub date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancellation_note: Option<String>,
    pub club_assigned: bool,
    pub all_athletes: bool,
    pub assigned_athletes: Vec<AssignedAthleteBrief>,
    pub i_am_assigned: bool,
    pub roster_announced: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub my_withdrawal_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attendance_status: Option<String>,
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
    /// Najlepsze zaakceptowane rwanie (kg)
    pub best_snatch_kg: Option<f64>,
    /// Najlepszy zaakceptowany podrzut (kg)
    pub best_clean_jerk_kg: Option<f64>,
    /// Najlepszy zaakceptowany dwubój (kg)
    pub best_total_kg: Option<f64>,
    /// Liczba zaakceptowanych startów (wyniki z zawodów)
    pub starts_count: usize,
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

/// Token urządzenia do push (FCM).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeviceToken {
    pub token: String,
    pub user_id: String,
    /// "android" | "windows" | "ios"
    pub platform: String,
    pub updated_at: String,
}
