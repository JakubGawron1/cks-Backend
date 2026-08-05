use std::path::{Path, PathBuf};
use std::sync::Arc;

use libsql::{params, Builder, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::auth::password::{hash_password, verify_password};
use crate::config::Config;
use crate::error::{internal, AppError, AppResult};
use crate::models::club::{
    AthleteProfile, AthleteStats, AttendanceRecord, AttendanceSession, CalendarEvent, CmsBlock,
    CmsPage, CmsStatus, CompetitionResult, ContactMessage, FeatureFlag, FlagKind,
    FlagRolloutStatus, LogLevel, Notification, ResultStatus, SiteStats, SystemLog,
    TrainingPlan, TrainingPlanProgress, TrainingScheduleDefaults, WithdrawalStatus,
};
use crate::models::role::{has_role, roles_from_json, roles_to_json, Role};
use crate::models::user::{NotificationPrefs, UserRecord};

pub const MANAGED_TABLES: &[&str] = &[
    "users",
    "athlete_profiles",
    "feature_flags",
    "competition_results",
    "cms_pages",
    "system_logs",
    "attendance",
    "training_plans",
    "plan_progress",
    "contact_messages",
    "notifications",
    "device_tokens",
    "calendar_events",
    "email_tokens",
    "meta",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredUser {
    id: String,
    email: String,
    password_hash: String,
    display_name: String,
    roles: String,
    is_active: bool,
    #[serde(default = "crate::models::user::default_ui_theme")]
    ui_theme: String,
    #[serde(default)]
    photo_url: Option<String>,
    #[serde(default)]
    email_verified: bool,
    #[serde(default)]
    pending_email: Option<String>,
    #[serde(default)]
    notification_prefs: NotificationPrefs,
    created_at: String,
    updated_at: String,
}

impl From<StoredUser> for UserRecord {
    fn from(u: StoredUser) -> Self {
        let roles = roles_from_json(&u.roles).unwrap_or_default();
        let ui_theme = crate::models::user::normalize_ui_theme(&u.ui_theme)
            .unwrap_or_else(crate::models::user::default_ui_theme);
        Self {
            id: u.id,
            email: u.email,
            password_hash: u.password_hash,
            display_name: u.display_name,
            roles,
            is_active: u.is_active,
            ui_theme,
            photo_url: normalize_optional_url(u.photo_url),
            email_verified: u.email_verified,
            pending_email: u.pending_email.and_then(|e| {
                let t = e.trim().to_ascii_lowercase();
                if t.is_empty() { None } else { Some(t) }
            }),
            notification_prefs: u.notification_prefs,
            created_at: u.created_at,
            updated_at: u.updated_at,
        }
    }
}

impl From<&UserRecord> for StoredUser {
    fn from(u: &UserRecord) -> Self {
        Self {
            id: u.id.clone(),
            email: u.email.clone(),
            password_hash: u.password_hash.clone(),
            display_name: u.display_name.clone(),
            roles: roles_to_json(&u.roles),
            is_active: u.is_active,
            ui_theme: u.ui_theme.clone(),
            photo_url: u.photo_url.clone(),
            email_verified: u.email_verified,
            pending_email: u.pending_email.clone(),
            notification_prefs: u.notification_prefs.clone(),
            created_at: u.created_at.clone(),
            updated_at: u.updated_at.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EmailTokenPurpose {
    Verify,
    Reset,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailToken {
    pub id: String,
    pub user_id: String,
    pub purpose: EmailTokenPurpose,
    pub token_hash: String,
    pub target_email: String,
    pub expires_at: String,
    pub used_at: Option<String>,
}

fn normalize_optional_url(raw: Option<String>) -> Option<String> {
    raw.and_then(|s| {
        let t = s.trim().to_string();
        if t.is_empty() { None } else { Some(t) }
    })
}

#[derive(Clone)]
pub struct Database {
    inner: Arc<Mutex<DbInner>>,
}

struct DbInner {
    conn: Connection,
    /// Gdy Some — baza remote (Turso); umożliwia odświeżenie streamu Hrana.
    remote: Option<RemoteDb>,
}

#[derive(Clone)]
struct RemoteDb {
    url: String,
    token: String,
}

fn is_stale_hrana_error(err: &dyn std::fmt::Display) -> bool {
    let s = err.to_string().to_ascii_lowercase();
    s.contains("stream not found")
        || s.contains("stream has expired")
        || s.contains("stream_expired")
        || s.contains("hrana_closed")
        || s.contains("baton invalid")
        || s.contains("baton reused")
}

fn is_stale_app_error(err: &AppError) -> bool {
    match err {
        AppError::Internal(inner) => is_stale_hrana_error(inner),
        _ => false,
    }
}

impl Database {
    pub async fn connect(config: &Config) -> Result<Self, AppError> {
        let (conn, remote) = if config.is_remote_db() {
            let token = config
                .turso_auth_token
                .clone()
                .ok_or_else(|| internal("Brak TURSO_AUTH_TOKEN"))?;
            tracing::info!(
                "Łączenie z Turso ({})",
                config.production_mode.as_str()
            );
            let remote = RemoteDb {
                url: config.database_url.clone(),
                token: token.clone(),
            };
            let db = Builder::new_remote(remote.url.clone(), remote.token.clone())
                .build()
                .await
                .map_err(internal)?;
            let conn = db.connect().map_err(internal)?;
            (conn, Some(remote))
        } else {
            let path = local_db_path(config);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(internal)?;
            }
            tracing::info!(
                "Lokalna baza libSQL: {} ({})",
                path.display(),
                config.production_mode.as_str()
            );
            let db = Builder::new_local(path.to_string_lossy().as_ref())
                .build()
                .await
                .map_err(internal)?;
            let conn = db.connect().map_err(internal)?;
            (conn, None)
        };

        Ok(Self {
            inner: Arc::new(Mutex::new(DbInner { conn, remote })),
        })
    }

    async fn has_remote(&self) -> bool {
        self.inner.lock().await.remote.is_some()
    }

    async fn reconnect(&self) -> AppResult<()> {
        let mut inner = self.inner.lock().await;
        let Some(remote) = inner.remote.clone() else {
            return Ok(());
        };
        tracing::warn!("Turso/Hrana: odświeżam połączenie (wygasły stream)");
        let db = Builder::new_remote(remote.url, remote.token)
            .build()
            .await
            .map_err(internal)?;
        inner.conn = db.connect().map_err(internal)?;
        tracing::info!("Turso/Hrana: ponowne połączenie OK");
        Ok(())
    }

    /// Wykonaj operację na Connection; przy wygasłym streamie Hrana — reconnect + 1 retry.
    async fn db_op<T, F, Fut>(&self, op: F) -> AppResult<T>
    where
        F: Fn(Connection) -> Fut,
        Fut: std::future::Future<Output = AppResult<T>>,
    {
        for attempt in 0..2u8 {
            let conn = self.inner.lock().await.conn.clone();
            match op(conn).await {
                Ok(value) => return Ok(value),
                Err(err)
                    if attempt == 0 && is_stale_app_error(&err) && self.has_remote().await =>
                {
                    tracing::warn!(error = %err, "Turso stream nieaktualny — retry po reconnect");
                    self.reconnect().await?;
                }
                Err(err) => return Err(err),
            }
        }
        Err(internal(
            "Baza: ponowne połączenie nie przywróciło dostępu (Hrana).",
        ))
    }

    /// Lekki ping bazy (health / readiness).
    pub async fn ping(&self) -> AppResult<()> {
        self.db_op(|conn| async move {
            conn.execute("SELECT 1", ()).await.map_err(internal)?;
            Ok(())
        })
        .await
    }

    pub async fn migrate(&self) -> AppResult<()> {
        tracing::debug!(tables = MANAGED_TABLES.len(), "CREATE TABLE IF NOT EXISTS…");
        for table in MANAGED_TABLES {
            let sql = format!(
                "CREATE TABLE IF NOT EXISTS {table} (
                    key TEXT PRIMARY KEY NOT NULL,
                    value TEXT NOT NULL
                )"
            );
            let sql_clone = sql.clone();
            self.db_op(|conn| {
                let sql = sql_clone.clone();
                async move {
                    conn.execute(&sql, ()).await.map_err(internal)?;
                    Ok(())
                }
            })
            .await?;
        }
        tracing::info!(tables = MANAGED_TABLES.len(), "migracje OK");
        Ok(())
    }

    pub async fn seed_if_empty(&self, config: &Config) -> AppResult<()> {
        if self.user_count().await? == 0 {
            tracing::info!("Baza pusta — tworzę konto seed superadmin");
            let now = chrono::Utc::now().to_rfc3339();
            let email = config.seed_superadmin_email.clone();
            let email_verified = true; // seed zawsze zweryfikowany
            self.insert_user(StoredUser {
                id: uuid::Uuid::new_v4().to_string(),
                email,
                password_hash: hash_password(&config.seed_superadmin_password)?,
                display_name: "Superadmin".into(),
                roles: roles_to_json(&[Role::Superadmin]),
                is_active: true,
                ui_theme: crate::models::user::default_ui_theme(),
                photo_url: None,
                email_verified,
                pending_email: None,
                notification_prefs: NotificationPrefs::default(),
                created_at: now.clone(),
                updated_at: now,
            })
            .await?;
            tracing::info!(
                "Seed OK — superadmin: {} (hasło z SEED_SUPERADMIN_PASSWORD)",
                config.seed_superadmin_email
            );
        }

        self.seed_defaults().await?;
        Ok(())
    }

    async fn seed_defaults(&self) -> AppResult<()> {
        // Katalog flag — backend jest źródłem prawdy dla frontendu (DevTools + public).
        tracing::debug!("synchronizacja katalogu feature flags");
        self.sync_flag_catalog().await?;

        if self.list_cms_pages().await?.is_empty() {
            tracing::info!("seed: domyślna strona CMS");
            let now = chrono::Utc::now().to_rfc3339();
            self.upsert_cms_page(CmsPage {
                id: uuid::Uuid::new_v4().to_string(),
                slug: "o-klubie".into(),
                title: "O klubie".into(),
                status: CmsStatus::Draft,
                blocks: vec![CmsBlock {
                    id: uuid::Uuid::new_v4().to_string(),
                    block_type: "paragraph".into(),
                    content: "CKS Slavia Ruda Śląska — dwubój olimpijski.".into(),
                }],
                created_at: now.clone(),
                updated_at: now,
            })
            .await?;
        }

        if self.list_results().await?.is_empty() {
            tracing::info!("seed: przykładowy wynik zawodów");
            let now = chrono::Utc::now().to_rfc3339();
            self.upsert_result(CompetitionResult {
                id: uuid::Uuid::new_v4().to_string(),
                athlete_name: "Jan Kowalski".into(),
                user_id: None,
                event_name: "Puchar Śląska 2026".into(),
                event_date: Some("2026-03-15".into()),
                kind: "competition".into(),
                snatch_kg: Some(110.0),
                clean_jerk_kg: Some(140.0),
                total_kg: Some(250.0),
                bodyweight_kg: Some(89.0),
                venue: Some("Katowice".into()),
                category: Some("89 kg".into()),
                status: ResultStatus::Pending,
                reviewer_note: None,
                submitted_at: now.clone(),
                updated_at: now,
            })
            .await?;
        }

        if self.get_attendance_session().await?.is_none() {
            tracing::info!("seed: sesja obecności");
            let now = chrono::Utc::now().to_rfc3339();
            self.set_attendance_session(AttendanceSession {
                token: uuid::Uuid::new_v4().to_string(),
                label: "Trening".into(),
                created_at: now.clone(),
                refreshed_at: now,
                event_id: None,
            })
            .await?;
        }

        self.ensure_training_schedule_defaults().await?;
        self.seed_training_events().await?;
        let _ = self.reconcile_past_training_attendance().await;

        Ok(())
    }

    async fn user_count(&self) -> AppResult<usize> {
        Ok(self.list_users().await?.len())
    }

    async fn insert_user(&self, user: StoredUser) -> AppResult<()> {
        let payload = serde_json::to_string(&user).map_err(internal)?;
        let email_key = user.email.to_ascii_lowercase();
        if self.kv_get_raw("users", &email_key).await?.is_some() {
            return Err(AppError::BadRequest(
                "Konto z tym e-mailem już istnieje.".into(),
            ));
        }
        self.kv_upsert_raw("users", &email_key, &payload).await
    }

    pub async fn list_users(&self) -> AppResult<Vec<UserRecord>> {
        let mut users = Vec::new();
        for value in self.kv_list_raw("users").await? {
            let stored: StoredUser = serde_json::from_str(&value).map_err(internal)?;
            users.push(UserRecord::from(stored));
        }
        users.sort_by(|a: &UserRecord, b: &UserRecord| a.email.cmp(&b.email));
        Ok(users)
    }

    pub async fn find_user_by_email(&self, email: &str) -> AppResult<Option<UserRecord>> {
        let key = email.trim().to_ascii_lowercase();
        match self.kv_get_raw("users", &key).await? {
            Some(payload) => {
                let stored: StoredUser = serde_json::from_str(&payload).map_err(internal)?;
                Ok(Some(stored.into()))
            }
            None => Ok(None),
        }
    }

    pub async fn find_user_by_id(&self, id: &str) -> AppResult<Option<UserRecord>> {
        Ok(self
            .list_users()
            .await?
            .into_iter()
            .find(|u| u.id == id))
    }

    pub async fn authenticate(&self, email: &str, password: &str) -> AppResult<UserRecord> {
        let user = match self.find_user_by_email(email).await? {
            Some(u) => u,
            None => {
                tracing::warn!(email = %email, "authenticate: nieznany e-mail");
                return Err(AppError::unauthorized());
            }
        };

        if !user.is_active {
            tracing::warn!(email = %email, user_id = %user.id, "authenticate: konto nieaktywne");
            return Err(AppError::Forbidden("Konto jest nieaktywne.".into()));
        }

        if !verify_password(password, &user.password_hash)? {
            tracing::warn!(email = %email, user_id = %user.id, "authenticate: złe hasło");
            return Err(AppError::unauthorized());
        }

        Ok(user)
    }

    pub async fn create_user(
        &self,
        email: &str,
        password: &str,
        display_name: &str,
        roles: Vec<Role>,
        photo_url: Option<String>,
    ) -> AppResult<UserRecord> {
        let now = chrono::Utc::now().to_rfc3339();
        let email = email.trim().to_ascii_lowercase();
        let email_verified = crate::mail::is_dev_email(&email);
        let user = UserRecord {
            id: uuid::Uuid::new_v4().to_string(),
            email,
            password_hash: hash_password(password)?,
            display_name: display_name.trim().to_string(),
            roles,
            is_active: true,
            ui_theme: crate::models::user::default_ui_theme(),
            photo_url: normalize_optional_url(photo_url),
            email_verified,
            pending_email: None,
            notification_prefs: NotificationPrefs::default(),
            created_at: now.clone(),
            updated_at: now,
        };
        self.insert_user(StoredUser::from(&user)).await?;
        Ok(user)
    }

    pub async fn update_user(&self, user: &UserRecord) -> AppResult<()> {
        let existing = self
            .list_users()
            .await?
            .into_iter()
            .find(|u| u.id == user.id)
            .ok_or_else(|| AppError::NotFound("Użytkownik nie istnieje.".into()))?;

        if has_role(&existing.roles, Role::Superadmin)
            && existing.roles.contains(&Role::Superadmin)
        {
            if !user.roles.contains(&Role::Superadmin) {
                return Err(AppError::Forbidden(
                    "Nie można usuwać roli superadmin z chronionego konta.".into(),
                ));
            }
            if user.roles.len() < existing.roles.len()
                || !existing.roles.iter().all(|r| user.roles.contains(r))
            {
                return Err(AppError::Forbidden(
                    "Konta Superadmin nie mogą mieć usuwanych ról.".into(),
                ));
            }
            if !user.is_active {
                return Err(AppError::Forbidden(
                    "Nie można banować konta Superadmin.".into(),
                ));
            }
        }

        let mut stored = StoredUser::from(user);
        stored.updated_at = chrono::Utc::now().to_rfc3339();

        let old_key = existing.email.to_ascii_lowercase();
        let new_key = stored.email.to_ascii_lowercase();
        if old_key != new_key {
            if self.kv_get_raw("users", &new_key).await?.is_some() {
                return Err(AppError::BadRequest(
                    "Konto z tym e-mailem już istnieje.".into(),
                ));
            }
            self.kv_delete_raw("users", &old_key).await?;
        }
        let payload = serde_json::to_string(&stored).map_err(internal)?;
        self.kv_upsert_raw("users", &new_key, &payload).await
    }

    pub async fn delete_user(&self, id: &str) -> AppResult<()> {
        let existing = self
            .list_users()
            .await?
            .into_iter()
            .find(|u| u.id == id)
            .ok_or_else(|| AppError::NotFound("Użytkownik nie istnieje.".into()))?;

        if existing.roles.contains(&Role::Superadmin) {
            return Err(AppError::Forbidden(
                "Nie można usunąć konta Superadmin.".into(),
            ));
        }

        let key = existing.email.to_ascii_lowercase();
        self.kv_delete_raw("users", &key).await
    }

    // --- profiles ---

    pub async fn list_profiles(&self) -> AppResult<Vec<AthleteProfile>> {
        self.kv_list( "athlete_profiles").await
    }

    pub async fn upsert_profile(&self, profile: AthleteProfile) -> AppResult<()> {
        self.kv_upsert( "athlete_profiles", &profile.id, &profile).await
    }

    pub async fn delete_profile(&self, id: &str) -> AppResult<()> {
        self.kv_delete( "athlete_profiles", id).await
    }

    pub async fn get_profile(&self, id: &str) -> AppResult<Option<AthleteProfile>> {
        self.kv_get( "athlete_profiles", id).await
    }

    pub async fn find_profile_by_user_id(
        &self,
        user_id: &str,
    ) -> AppResult<Option<AthleteProfile>> {
        if user_id.is_empty() || user_id == "manual" {
            return Ok(None);
        }
        Ok(self
            .list_profiles()
            .await?
            .into_iter()
            .find(|p| p.user_id == user_id))
    }

    /// Po akceptacji wyniku z zawodów: kategoria + masa z ważenia stają się oficjalne w profilu
    /// (statystyki / panel czytają `AthleteProfile.category`).
    pub async fn apply_accepted_competition_to_profile(
        &self,
        result: &CompetitionResult,
        profile_id: Option<&str>,
    ) -> AppResult<bool> {
        if !result.kind.eq_ignore_ascii_case("competition") {
            return Ok(false);
        }
        if result.status != ResultStatus::Accepted {
            return Ok(false);
        }
        let Some(category) = result
            .category
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return Ok(false);
        };

        let profile = if let Some(pid) = profile_id.map(str::trim).filter(|s| !s.is_empty()) {
            self.get_profile(pid).await?
        } else if let Some(uid) = result.user_id.as_deref() {
            self.find_profile_by_user_id(uid).await?
        } else {
            let name = result.athlete_name.trim().to_lowercase();
            self.list_profiles().await?.into_iter().find(|p| {
                p.display_name.trim().to_lowercase() == name
            })
        };

        let Some(mut profile) = profile else {
            return Ok(false);
        };

        let mut changed = false;
        if profile.category.as_deref() != Some(category) {
            profile.category = Some(category.to_string());
            changed = true;
        }
        if let Some(bw) = result.bodyweight_kg.filter(|v| v.is_finite() && *v > 0.0) {
            if profile.bodyweight_kg != Some(bw) {
                profile.bodyweight_kg = Some(bw);
                changed = true;
            }
        }
        if !changed {
            return Ok(false);
        }
        profile.updated_at = chrono::Utc::now().to_rfc3339();
        self.upsert_profile(profile).await?;
        Ok(true)
    }

    /// Zawodnik: zdjęcie konta = zdjęcie profilu — synchronizacja w obie strony.
    pub async fn sync_photo_user_to_profile(
        &self,
        user_id: &str,
        photo_url: &Option<String>,
    ) -> AppResult<()> {
        if let Some(mut profile) = self.find_profile_by_user_id(user_id).await? {
            let next = normalize_optional_url(photo_url.clone());
            if profile.photo_url != next {
                profile.photo_url = next;
                profile.updated_at = chrono::Utc::now().to_rfc3339();
                self.upsert_profile(profile).await?;
            }
        }
        Ok(())
    }

    pub async fn sync_photo_profile_to_user(
        &self,
        user_id: &str,
        photo_url: &Option<String>,
    ) -> AppResult<()> {
        if user_id.is_empty() || user_id == "manual" {
            return Ok(());
        }
        if let Some(mut user) = self.find_user_by_id(user_id).await? {
            let next = normalize_optional_url(photo_url.clone());
            if user.photo_url != next {
                user.photo_url = next;
                self.update_user(&user).await?;
            }
        }
        Ok(())
    }

    // --- flags ---

    /// Definicje dostępnych flag (klucz, etykieta, kind, opis, status, domyślne enabled).
    fn flag_catalog() -> &'static [(&'static str, &'static str, FlagKind, &'static str, FlagRolloutStatus, bool)] {
        &[
            (
                "public_blog",
                "Publiczny blog",
                FlagKind::Stable,
                "Publiczna sekcja aktualności / blogu na witrynie (linki w nagłówku i stopce). Gdy wyłączona, trasy i linki znikają.",
                FlagRolloutStatus::Wired,
                true,
            ),
            (
                "announcements_board",
                "Tablica ogłoszeń",
                FlagKind::Stable,
                "Tablica ogłoszeń klubowych widoczna na stronie publicznej. Flaga steruje dostępnością `/ogloszenia`.",
                FlagRolloutStatus::Wired,
                true,
            ),
            (
                "public_calendar",
                "Kalendarz publiczny",
                FlagKind::Stable,
                "Publiczny kalendarz klubowy na witrynie (`/kalendarz`) — linki w nawigacji i stopce.",
                FlagRolloutStatus::Wired,
                true,
            ),
            (
                "club_calendar",
                "Kalendarz kadrowy",
                FlagKind::Stable,
                "Kalendarz zawodów i treningów w panelu klubowym (`/klub/kalendarz`) — CRUD, skład, terminarz.",
                FlagRolloutStatus::Wired,
                true,
            ),
            (
                "athlete_calendar",
                "Kalendarz zawodnika",
                FlagKind::Stable,
                "Kalendarz w panelu zawodnika (`/panel/kalendarz`) — skład, rezygnacje, obecność.",
                FlagRolloutStatus::Wired,
                true,
            ),
            (
                "ui_toasts",
                "Powiadomienia toast",
                FlagKind::Stable,
                "Globalne powiadomienia toast (prawy dolny róg) po akcjach w panelach i na witrynie — sukces, błąd, info.",
                FlagRolloutStatus::Wired,
                true,
            ),
            (
                "experimental_live_scores",
                "Live wyniki (eksperymentalne)",
                FlagKind::Experimental,
                "Eksperymentalny podgląd wyników na żywo (zawody / trening). Na razie tylko rezerwacja klucza — brak UI i API live.",
                FlagRolloutStatus::Planned,
                false,
            ),
            (
                "experimental_ai_summaries",
                "AI podsumowania CMS",
                FlagKind::Experimental,
                "Automatyczne podsumowania treści CMS (szkice stron) z pomocą AI. Funkcja nie jest jeszcze zaimplementowana.",
                FlagRolloutStatus::Planned,
                false,
            ),
            (
                "experimental_panel_themes",
                "Eksperymentalne motywy paneli",
                FlagKind::Experimental,
                "Eksperymentalne motywy paneli (Kapsuła, Studio, Dok) — inny układ, zaokrąglenia i nawigacja. Domyślnie wyłączone; w ustawieniach konta pojawiają się dopiero po włączeniu.",
                FlagRolloutStatus::Wired,
                false,
            ),
            (
                "experimental_notification_emails",
                "E-maile powiadomień (eksperymentalne)",
                FlagKind::Experimental,
                "Dodatkowe maile o składzie zawodów, planach treningowych i formularzu kontaktowym (3 kategorie w ustawieniach). In-app (dzwonek) działa zawsze. Domyślnie wyłączone.",
                FlagRolloutStatus::Wired,
                false,
            ),
            (
                "email_password_reset",
                "E-mail: reset hasła",
                FlagKind::Stable,
                "Wysyłka linku do ustawienia nowego hasła (zapomniane hasło). Domyślnie włączone.",
                FlagRolloutStatus::Wired,
                true,
            ),
            (
                "email_verification",
                "E-mail: weryfikacja adresu",
                FlagKind::Stable,
                "Wysyłka linku weryfikacyjnego przy potwierdzeniu / zmianie e-maila konta. Domyślnie włączone.",
                FlagRolloutStatus::Wired,
                true,
            ),
            (
                "email_contact_confirmation",
                "E-mail: potwierdzenie kontaktu",
                FlagKind::Stable,
                "Potwierdzenie do nadawcy po wysłaniu formularza kontaktowego. Domyślnie włączone.",
                FlagRolloutStatus::Wired,
                true,
            ),
            (
                "email_test",
                "E-mail: test DevTools",
                FlagKind::Stable,
                "Wysyłka testowego e-maila z zakładki Debug w DevTools (superadmin). Domyślnie włączone.",
                FlagRolloutStatus::Wired,
                true,
            ),
        ]
    }

    /// Tworzy brakujące flagi i synchronizuje metadane z katalogu (bez zmiany `enabled`).
    async fn sync_flag_catalog(&self) -> AppResult<()> {
        let existing = self.list_flags().await?;
        let now = chrono::Utc::now().to_rfc3339();

        for &(key, label, kind, description, status, default_enabled) in Self::flag_catalog() {
            if let Some(mut flag) = existing.iter().find(|f| f.key == key).cloned() {
                let meta_changed = flag.label != label
                    || flag.kind != kind
                    || flag.description != description
                    || flag.rollout_status != status;
                if meta_changed {
                    flag.label = label.into();
                    flag.kind = kind;
                    flag.description = description.into();
                    flag.rollout_status = status;
                    self.upsert_flag(flag).await?;
                }
            } else {
                self.upsert_flag(FeatureFlag {
                    key: key.into(),
                    label: label.into(),
                    enabled: default_enabled,
                    kind,
                    description: description.into(),
                    rollout_status: status,
                    updated_at: now.clone(),
                })
                .await?;
            }
        }
        Ok(())
    }

    /// Czy flaga jest włączona. Brak wpisu w DB → `false` (bezpieczny default).
    pub async fn is_flag_enabled(&self, key: &str) -> bool {
        match self.list_flags().await {
            Ok(flags) => flags
                .iter()
                .find(|f| f.key == key)
                .map(|f| f.enabled)
                .unwrap_or(false),
            Err(err) => {
                tracing::warn!(error = %err, key, "is_flag_enabled: list_flags failed");
                false
            }
        }
    }

    pub async fn list_flags(&self) -> AppResult<Vec<FeatureFlag>> {
        let mut flags: Vec<FeatureFlag> = self.kv_list( "feature_flags").await?;
        // Stable najpierw, potem Experimental; wewnątrz kategorii alfabetycznie.
        flags.sort_by(|a, b| match (&a.kind, &b.kind) {
            (FlagKind::Stable, FlagKind::Experimental) => std::cmp::Ordering::Less,
            (FlagKind::Experimental, FlagKind::Stable) => std::cmp::Ordering::Greater,
            _ => a.key.cmp(&b.key),
        });
        Ok(flags)
    }

    pub async fn upsert_flag(&self, flag: FeatureFlag) -> AppResult<()> {
        self.kv_upsert( "feature_flags", &flag.key, &flag).await
    }

    // --- results ---

    pub async fn list_results(&self) -> AppResult<Vec<CompetitionResult>> {
        let mut items: Vec<CompetitionResult> = self.kv_list( "competition_results").await?;
        items.sort_by(|a, b| b.submitted_at.cmp(&a.submitted_at));
        Ok(items)
    }

    pub async fn upsert_result(&self, result: CompetitionResult) -> AppResult<()> {
        self.kv_upsert( "competition_results", &result.id, &result).await
    }

    pub async fn get_result(&self, id: &str) -> AppResult<Option<CompetitionResult>> {
        self.kv_get( "competition_results", id).await
    }

    // --- cms ---

    pub async fn list_cms_pages(&self) -> AppResult<Vec<CmsPage>> {
        let mut pages: Vec<CmsPage> = self.kv_list( "cms_pages").await?;
        pages.sort_by(|a, b| a.slug.cmp(&b.slug));
        Ok(pages)
    }

    pub async fn upsert_cms_page(&self, page: CmsPage) -> AppResult<()> {
        self.kv_upsert( "cms_pages", &page.id, &page).await
    }

    pub async fn get_cms_page(&self, id: &str) -> AppResult<Option<CmsPage>> {
        self.kv_get( "cms_pages", id).await
    }

    pub async fn delete_cms_page(&self, id: &str) -> AppResult<()> {
        self.kv_delete( "cms_pages", id).await
    }

    // --- contact messages ---

    pub async fn list_contact_messages(&self) -> AppResult<Vec<ContactMessage>> {
        let mut items: Vec<ContactMessage> = self.kv_list( "contact_messages").await?;
        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(items)
    }

    pub async fn get_contact_message(&self, id: &str) -> AppResult<Option<ContactMessage>> {
        self.kv_get( "contact_messages", id).await
    }

    pub async fn upsert_contact_message(&self, message: ContactMessage) -> AppResult<()> {
        self.kv_upsert( "contact_messages", &message.id, &message).await
    }

    pub async fn delete_contact_message(&self, id: &str) -> AppResult<()> {
        self.kv_delete( "contact_messages", id).await
    }

    // --- notifications ---

    pub async fn list_notifications_for_user(
        &self,
        user_id: &str,
    ) -> AppResult<Vec<Notification>> {
        let mut items: Vec<Notification> = self.kv_list( "notifications").await?;
        items.retain(|n| n.user_id == user_id);
        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(items)
    }

    pub async fn get_notification(&self, id: &str) -> AppResult<Option<Notification>> {
        self.kv_get( "notifications", id).await
    }

    pub async fn upsert_notification(&self, notification: Notification) -> AppResult<()> {
        self.kv_upsert( "notifications", &notification.id, &notification).await
    }

    pub async fn delete_notification(&self, id: &str) -> AppResult<()> {
        self.kv_delete( "notifications", id).await
    }

    pub async fn unread_notification_count(&self, user_id: &str) -> AppResult<usize> {
        let items = self.list_notifications_for_user(user_id).await?;
        Ok(items.into_iter().filter(|n| !n.read).count())
    }

    pub async fn mark_all_notifications_read(&self, user_id: &str) -> AppResult<usize> {
        let now = chrono::Utc::now().to_rfc3339();
        let items = self.list_notifications_for_user(user_id).await?;
        let mut updated = 0usize;
        for mut n in items {
            if n.read {
                continue;
            }
            n.read = true;
            n.read_at = Some(now.clone());
            self.upsert_notification(n).await?;
            updated += 1;
        }
        Ok(updated)
    }

    /// Tworzy powiadomienie dla jednego użytkownika.
    pub async fn create_notification(
        &self,
        user_id: &str,
        title: &str,
        body: &str,
        kind: &str,
        href: Option<&str>,
    ) -> AppResult<Notification> {
        let notification = Notification {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: user_id.to_string(),
            title: title.to_string(),
            body: body.to_string(),
            kind: kind.to_string(),
            href: href.map(|s| s.to_string()),
            read: false,
            created_at: chrono::Utc::now().to_rfc3339(),
            read_at: None,
        };
        self.upsert_notification(notification.clone()).await?;
        Ok(notification)
    }

    // --- device tokens (FCM) ---

    pub async fn upsert_device_token(
        &self,
        user_id: &str,
        token: &str,
        platform: &str,
    ) -> AppResult<crate::models::club::DeviceToken> {
        let device = crate::models::club::DeviceToken {
            token: token.to_string(),
            user_id: user_id.to_string(),
            platform: platform.to_string(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        self.kv_upsert("device_tokens", token, &device).await?;
        Ok(device)
    }

    pub async fn delete_device_token(&self, token: &str) -> AppResult<()> {
        self.kv_delete("device_tokens", token).await
    }

    pub async fn list_devices_for_user(
        &self,
        user_id: &str,
    ) -> AppResult<Vec<crate::models::club::DeviceToken>> {
        let items: Vec<crate::models::club::DeviceToken> =
            self.kv_list("device_tokens").await?;
        Ok(items
            .into_iter()
            .filter(|d| d.user_id == user_id)
            .collect())
    }

    /// Powiadamia aktywnych użytkowników z rolami kadry (trener / admin / superadmin).
    pub async fn notify_staff(
        &self,
        title: &str,
        body: &str,
        kind: &str,
        href: Option<&str>,
        exclude_user_id: Option<&str>,
    ) -> AppResult<usize> {
        let staff_roles = [Role::Trener, Role::Admin, Role::Superadmin];
        let users = self.list_users().await?;
        let mut count = 0usize;
        for user in users {
            if !user.is_active {
                continue;
            }
            if let Some(exclude) = exclude_user_id {
                if user.id == exclude {
                    continue;
                }
            }
            let is_staff = user
                .roles
                .iter()
                .any(|r| staff_roles.contains(r));
            if !is_staff {
                continue;
            }
            self.create_notification(&user.id, title, body, kind, href)
                .await?;
            count += 1;
        }
        Ok(count)
    }

    // --- email tokens ---

    pub async fn upsert_email_token(&self, token: &EmailToken) -> AppResult<()> {
        self.kv_upsert("email_tokens", &token.id, token).await
    }

    pub async fn find_email_token_by_hash(
        &self,
        token_hash: &str,
        purpose: EmailTokenPurpose,
    ) -> AppResult<Option<EmailToken>> {
        let all: Vec<EmailToken> = self.kv_list("email_tokens").await?;
        Ok(all.into_iter().find(|t| {
            t.token_hash == token_hash && t.purpose == purpose && t.used_at.is_none()
        }))
    }

    pub async fn invalidate_email_tokens_for_user(
        &self,
        user_id: &str,
        purpose: EmailTokenPurpose,
    ) -> AppResult<()> {
        let all: Vec<EmailToken> = self.kv_list("email_tokens").await?;
        let now = chrono::Utc::now().to_rfc3339();
        for mut t in all {
            if t.user_id == user_id && t.purpose == purpose && t.used_at.is_none() {
                t.used_at = Some(now.clone());
                self.upsert_email_token(&t).await?;
            }
        }
        Ok(())
    }

    pub async fn mark_email_token_used(&self, mut token: EmailToken) -> AppResult<()> {
        token.used_at = Some(chrono::Utc::now().to_rfc3339());
        self.upsert_email_token(&token).await
    }

    // --- logs ---

    /// Logi systemowe trzymane są maksymalnie 7 dni.
    const SYSTEM_LOG_RETENTION_DAYS: i64 = 7;

    pub async fn purge_old_system_logs(&self) -> AppResult<usize> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(Self::SYSTEM_LOG_RETENTION_DAYS);
        let all: Vec<SystemLog> = self.kv_list("system_logs").await?;
        let mut deleted = 0usize;
        for log in all {
            let too_old = chrono::DateTime::parse_from_rfc3339(&log.created_at)
                .map(|dt| dt.with_timezone(&chrono::Utc) < cutoff)
                .unwrap_or(true);
            if too_old {
                self.kv_delete("system_logs", &log.id).await?;
                deleted += 1;
            }
        }
        if deleted > 0 {
            tracing::info!(
                deleted,
                retention_days = Self::SYSTEM_LOG_RETENTION_DAYS,
                "usunięto stare logi systemowe"
            );
        }
        Ok(deleted)
    }

    pub async fn list_logs(&self, limit: usize) -> AppResult<Vec<SystemLog>> {
        self.purge_old_system_logs().await?;
        let mut logs: Vec<SystemLog> = self.kv_list( "system_logs").await?;
        logs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        logs.truncate(limit);
        Ok(logs)
    }

    pub async fn append_log(
        &self,
        level: LogLevel,
        source: &str,
        message: &str,
        actor_id: Option<&str>,
    ) -> AppResult<()> {
        match level {
            LogLevel::Info => {
                tracing::info!(source, actor_id, "{message}");
            }
            LogLevel::Warn => {
                tracing::warn!(source, actor_id, "{message}");
            }
            LogLevel::Error => {
                tracing::error!(source, actor_id, "{message}");
            }
        }

        let log = SystemLog {
            id: uuid::Uuid::new_v4().to_string(),
            level,
            source: source.into(),
            message: message.into(),
            actor_id: actor_id.map(|s| s.to_string()),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        self.kv_upsert( "system_logs", &log.id, &log).await?;
        self.purge_old_system_logs().await?;
        Ok(())
    }

    // --- stats ---

    pub async fn site_stats(&self) -> AppResult<SiteStats> {
        let users = self.list_users().await?;
        let results = self.list_results().await?;
        let pages = self.list_cms_pages().await?;
        Ok(SiteStats {
            users: users.len(),
            active_users: users.iter().filter(|u| u.is_active).count(),
            athlete_profiles: self.list_profiles().await?.len(),
            cms_pages: pages.len(),
            cms_published: pages
                .iter()
                .filter(|p| p.status == CmsStatus::Published)
                .count(),
            results_pending: results
                .iter()
                .filter(|r| r.status == ResultStatus::Pending)
                .count(),
            results_total: results.len(),
            feature_flags: self.list_flags().await?.len(),
            system_logs: self.list_logs(10_000).await?.len(),
        })
    }

    pub async fn athlete_stats(&self, user_id: &str) -> AppResult<AthleteStats> {
        let results = self.list_results().await?;
        let mine: Vec<_> = results
            .iter()
            .filter(|r| r.user_id.as_deref() == Some(user_id))
            .collect();
        let attendance = self.list_attendance_in_window().await?;
        let mine_att: Vec<_> = attendance
            .iter()
            .filter(|a| a.user_id == user_id)
            .collect();
        let now = chrono::Utc::now();
        let month_prefix = now.format("%Y-%m").to_string();
        let plans = self.plans_for_user(user_id).await?;
        let progress = self.list_plan_progress_for_user(user_id).await?;
        let completed = progress
            .iter()
            .flat_map(|p| p.entries.iter())
            .filter(|e| e.completed)
            .count();
        let profile = self
            .list_profiles()
            .await?
            .into_iter()
            .find(|p| p.user_id == user_id);

        let accepted: Vec<_> = mine
            .iter()
            .filter(|r| r.status == ResultStatus::Accepted)
            .collect();
        let competition_accepted: Vec<_> = accepted
            .iter()
            .filter(|r| r.kind.eq_ignore_ascii_case("competition"))
            .collect();

        let best_snatch_kg = accepted
            .iter()
            .filter_map(|r| r.snatch_kg)
            .filter(|v| v.is_finite() && *v > 0.0)
            .fold(None, |acc: Option<f64>, v| {
                Some(acc.map_or(v, |a| a.max(v)))
            });
        let best_clean_jerk_kg = accepted
            .iter()
            .filter_map(|r| r.clean_jerk_kg)
            .filter(|v| v.is_finite() && *v > 0.0)
            .fold(None, |acc: Option<f64>, v| {
                Some(acc.map_or(v, |a| a.max(v)))
            });
        let best_total_kg = accepted
            .iter()
            .filter_map(|r| {
                r.total_kg.or_else(|| match (r.snatch_kg, r.clean_jerk_kg) {
                    (Some(s), Some(c)) => Some(s + c),
                    _ => None,
                })
            })
            .filter(|v| v.is_finite() && *v > 0.0)
            .fold(None, |acc: Option<f64>, v| {
                Some(acc.map_or(v, |a| a.max(v)))
            });

        Ok(AthleteStats {
            results_accepted: accepted.len(),
            results_pending: mine
                .iter()
                .filter(|r| r.status == ResultStatus::Pending)
                .count(),
            results_total: mine.len(),
            attendance_month: mine_att
                .iter()
                .filter(|a| a.checked_at.starts_with(&month_prefix))
                .count(),
            attendance_window: mine_att.len(),
            plans_active: plans.len(),
            plans_completed_exercises: completed,
            bodyweight_kg: profile.as_ref().and_then(|p| p.bodyweight_kg),
            category: profile.and_then(|p| p.category),
            best_snatch_kg,
            best_clean_jerk_kg,
            best_total_kg,
            starts_count: competition_accepted.len(),
        })
    }

    // --- attendance ---

    pub async fn get_attendance_session(&self) -> AppResult<Option<AttendanceSession>> {
        self.kv_get( "meta", "attendance_session").await
    }

    pub async fn set_attendance_session(&self, session: AttendanceSession) -> AppResult<()> {
        let payload = serde_json::to_string(&session).map_err(internal)?;
        self.upsert_meta("attendance_session", &payload).await
    }

    /// Odczyt stałej sesji QR — seed przy pierwszym GET, bez rotacji tokenu.
    pub async fn ensure_attendance_session(&self) -> AppResult<AttendanceSession> {
        if let Some(session) = self.get_attendance_session().await? {
            return Ok(session);
        }
        let now = chrono::Utc::now().to_rfc3339();
        let session = AttendanceSession {
            token: uuid::Uuid::new_v4().to_string(),
            label: "Trening klubowy".into(),
            created_at: now.clone(),
            refreshed_at: now,
            event_id: None,
        };
        self.set_attendance_session(session.clone()).await?;
        Ok(session)
    }

    pub async fn list_attendance_raw(&self) -> AppResult<Vec<AttendanceRecord>> {
        self.kv_list( "attendance").await
    }

    pub async fn get_attendance_record(&self, id: &str) -> AppResult<Option<AttendanceRecord>> {
        self.kv_get("attendance", id).await
    }

    pub async fn list_attendance_in_window(&self) -> AppResult<Vec<AttendanceRecord>> {
        let (start, end) = attendance_window_bounds();
        let mut items = self.list_attendance_raw().await?;
        items.retain(|r| {
            chrono::DateTime::parse_from_rfc3339(&r.checked_at)
                .map(|dt| {
                    let t = dt.with_timezone(&chrono::Utc);
                    t >= start && t <= end
                })
                .unwrap_or(false)
        });
        items.sort_by(|a, b| b.checked_at.cmp(&a.checked_at));
        Ok(items)
    }

    pub async fn prune_attendance_outside_window(&self) -> AppResult<()> {
        let (start, end) = attendance_window_bounds();
        let all = self.list_attendance_raw().await?;
        for r in all {
            let keep = chrono::DateTime::parse_from_rfc3339(&r.checked_at)
                .map(|dt| {
                    let t = dt.with_timezone(&chrono::Utc);
                    t >= start && t <= end
                })
                .unwrap_or(false);
            if !keep {
                self.kv_delete( "attendance", &r.id).await?;
            }
        }
        Ok(())
    }

    pub async fn upsert_attendance(&self, record: AttendanceRecord) -> AppResult<()> {
        self.kv_upsert( "attendance", &record.id, &record).await
    }

    pub async fn check_in_attendance(
        &self,
        user_id: &str,
        display_name: &str,
        token: &str,
    ) -> AppResult<AttendanceRecord> {
        const NO_TRAINING_MSG: &str = "Dziś nie ma treningu w tym terminie.";

        let session = self
            .get_attendance_session()
            .await?
            .ok_or_else(|| AppError::BadRequest("Brak aktywnej sesji obecności.".into()))?;
        if session.token != token {
            return Err(AppError::BadRequest(
                "Nieprawidłowy lub nieaktualny kod QR.".into(),
            ));
        }

        let defaults = self.get_training_schedule_defaults().await?;
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let today_training = self.find_today_scheduled_training(&today).await?;

        let profiles = self.list_profiles().await?;
        let my_profile_ids: Vec<String> = profiles
            .iter()
            .filter(|p| p.user_id == user_id)
            .map(|p| p.id.clone())
            .collect();

        // Autoryzowana ścieżka: trening dnia + okno czasowe
        if let Some(ref event) = today_training {
            if attendance_window_open(event, &defaults) {
                if self.athlete_has_accepted_withdrawal(event, &my_profile_ids, user_id) {
                    return Err(AppError::BadRequest(
                        "Zrezygnowałeś z tego treningu — check-in niedostępny.".into(),
                    ));
                }
                if !self.athlete_is_effectively_assigned(event, &my_profile_ids, user_id) {
                    return Err(AppError::BadRequest(
                        "Nie jesteś na liście uczestników tego treningu.".into(),
                    ));
                }

                let existing = self.list_attendance_raw().await?;
                if existing.iter().any(|r| {
                    r.user_id == user_id
                        && r.event_id.as_deref() == Some(event.id.as_str())
                        && r.status == "present"
                }) {
                    return Err(AppError::BadRequest(
                        "Obecność na tym treningu jest już zapisana.".into(),
                    ));
                }

                // Usuń auto-absent / pending / rejected dla tego eventu
                for r in existing.iter().filter(|r| {
                    r.user_id == user_id && r.event_id.as_deref() == Some(event.id.as_str())
                }) {
                    self.kv_delete("attendance", &r.id).await?;
                }

                let record = AttendanceRecord {
                    id: uuid::Uuid::new_v4().to_string(),
                    user_id: user_id.into(),
                    display_name: display_name.into(),
                    checked_at: chrono::Utc::now().to_rfc3339(),
                    session_token: token.into(),
                    event_id: Some(event.id.clone()),
                    status: "present".into(),
                    source: "qr".into(),
                };
                self.upsert_attendance(record.clone()).await?;
                self.prune_attendance_outside_window().await?;
                return Ok(record);
            }
        }

        // Poza oknem / brak treningu — ten sam komunikat dla zawodnika + cichy pending
        let event_id = today_training.as_ref().map(|e| e.id.clone());
        let existing = self.list_attendance_raw().await?;
        let already_pending = existing.iter().any(|r| {
            r.user_id == user_id
                && r.status == "pending_unauthorized"
                && match (&event_id, &r.event_id) {
                    (Some(a), Some(b)) => a == b,
                    (None, None) => r.checked_at.starts_with(&today),
                    _ => false,
                }
        });
        if already_pending {
            return Err(AppError::BadRequest(NO_TRAINING_MSG.into()));
        }
        // Już present na tym evencie (np. wcześniejszy skan) — nie twórz pending
        if let Some(ref eid) = event_id {
            if existing.iter().any(|r| {
                r.user_id == user_id
                    && r.event_id.as_deref() == Some(eid.as_str())
                    && r.status == "present"
            }) {
                return Err(AppError::BadRequest(
                    "Obecność na tym treningu jest już zapisana.".into(),
                ));
            }
        }

        let record = AttendanceRecord {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: user_id.into(),
            display_name: display_name.into(),
            checked_at: chrono::Utc::now().to_rfc3339(),
            session_token: token.into(),
            event_id: event_id.clone(),
            status: "pending_unauthorized".into(),
            source: "qr".into(),
        };
        self.upsert_attendance(record).await?;

        let body = match today_training.as_ref() {
            Some(e) => format!(
                "{} zeskanował QR poza oknem treningu „{}” ({})",
                display_name, e.title, e.date
            ),
            None => format!(
                "{} zeskanował QR w dniu bez treningu ({})",
                display_name, today
            ),
        };
        let _ = self
            .notify_staff(
                "Nieautoryzowany skan obecności",
                &body,
                "attendance",
                Some("/klub/obecnosc"),
                Some(user_id),
            )
            .await;

        Err(AppError::BadRequest(NO_TRAINING_MSG.into()))
    }

    async fn find_today_scheduled_training(
        &self,
        today: &str,
    ) -> AppResult<Option<CalendarEvent>> {
        Ok(self
            .list_events()
            .await?
            .into_iter()
            .find(|e| {
                e.event_type == "trening" && e.status == "scheduled" && e.date == today
            }))
    }

    pub async fn approve_unauthorized_attendance(
        &self,
        id: &str,
        event_id_override: Option<&str>,
    ) -> AppResult<AttendanceRecord> {
        let mut record = self
            .get_attendance_record(id)
            .await?
            .ok_or_else(|| AppError::NotFound("Rekord obecności nie istnieje.".into()))?;
        if record.status != "pending_unauthorized" {
            return Err(AppError::BadRequest(
                "Można zaakceptować tylko oczekujące nieautoryzowane skany.".into(),
            ));
        }

        let event_id = event_id_override
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| record.event_id.clone())
            .ok_or_else(|| {
                AppError::BadRequest(
                    "Wybierz trening, do którego przypisać obecność.".into(),
                )
            })?;

        let event = self
            .get_event(&event_id)
            .await?
            .ok_or_else(|| AppError::BadRequest("Nie znaleziono treningu.".into()))?;
        if event.event_type != "trening" {
            return Err(AppError::BadRequest(
                "Obecność można powiązać tylko z treningiem.".into(),
            ));
        }

        // Usuń inne wpisy tego zawodnika dla eventu (absent/pending)
        let existing = self.list_attendance_raw().await?;
        for r in existing.iter().filter(|r| {
            r.id != record.id
                && r.user_id == record.user_id
                && r.event_id.as_deref() == Some(event_id.as_str())
        }) {
            self.kv_delete("attendance", &r.id).await?;
        }

        record.event_id = Some(event_id);
        record.status = "present".into();
        record.source = "manual".into();
        record.checked_at = chrono::Utc::now().to_rfc3339();
        self.upsert_attendance(record.clone()).await?;
        Ok(record)
    }

    pub async fn reject_unauthorized_attendance(
        &self,
        id: &str,
    ) -> AppResult<AttendanceRecord> {
        let mut record = self
            .get_attendance_record(id)
            .await?
            .ok_or_else(|| AppError::NotFound("Rekord obecności nie istnieje.".into()))?;
        if record.status != "pending_unauthorized" {
            return Err(AppError::BadRequest(
                "Można odrzucić tylko oczekujące nieautoryzowane skany.".into(),
            ));
        }
        record.status = "rejected".into();
        self.upsert_attendance(record.clone()).await?;
        Ok(record)
    }

    // --- calendar events ---

    pub async fn list_events(&self) -> AppResult<Vec<CalendarEvent>> {
        let mut items: Vec<CalendarEvent> = self.kv_list("calendar_events").await?;
        items.sort_by(|a, b| {
            a.date
                .cmp(&b.date)
                .then(a.time.as_deref().unwrap_or("").cmp(b.time.as_deref().unwrap_or("")))
        });
        Ok(items)
    }

    pub async fn get_event(&self, id: &str) -> AppResult<Option<CalendarEvent>> {
        self.kv_get("calendar_events", id).await
    }

    pub async fn upsert_event(&self, event: CalendarEvent) -> AppResult<()> {
        self.kv_upsert("calendar_events", &event.id, &event).await
    }

    pub async fn delete_event(&self, id: &str) -> AppResult<()> {
        self.kv_delete("calendar_events", id).await
    }

    pub async fn list_events_in_range(
        &self,
        from: Option<&str>,
        to: Option<&str>,
    ) -> AppResult<Vec<CalendarEvent>> {
        Ok(self
            .list_events()
            .await?
            .into_iter()
            .filter(|e| {
                let end = e.end_date_inclusive();
                if let Some(f) = from {
                    if end < f {
                        return false;
                    }
                }
                if let Some(t) = to {
                    if e.date.as_str() > t {
                        return false;
                    }
                }
                true
            })
            .collect())
    }

    pub async fn list_public_events(
        &self,
        from: Option<&str>,
        to: Option<&str>,
    ) -> AppResult<Vec<CalendarEvent>> {
        Ok(self
            .list_events_in_range(from, to)
            .await?
            .into_iter()
            .filter(|e| e.club_assigned)
            .collect())
    }

    pub async fn get_training_schedule_defaults(&self) -> AppResult<TrainingScheduleDefaults> {
        let raw: Option<String> = self.kv_get_raw("meta", "calendar_training_defaults").await?;
        if let Some(s) = raw {
            if let Ok(parsed) = serde_json::from_str::<TrainingScheduleDefaults>(&s) {
                return Ok(parsed);
            }
        }
        Ok(TrainingScheduleDefaults::default())
    }

    pub async fn set_training_schedule_defaults(
        &self,
        defaults: &TrainingScheduleDefaults,
    ) -> AppResult<()> {
        let payload = serde_json::to_string(defaults).map_err(internal)?;
        self.upsert_meta("calendar_training_defaults", &payload).await
    }

    pub async fn ensure_training_schedule_defaults(&self) -> AppResult<()> {
        let raw: Option<String> = self.kv_get_raw("meta", "calendar_training_defaults").await?;
        if raw.is_none() {
            self.set_training_schedule_defaults(&TrainingScheduleDefaults::default())
                .await?;
        }
        Ok(())
    }

    async fn get_training_skip_dates(&self) -> AppResult<Vec<String>> {
        let raw: Option<String> = self.kv_get_raw("meta", "calendar_training_skip_dates").await?;
        if let Some(s) = raw {
            if let Ok(v) = serde_json::from_str::<Vec<String>>(&s) {
                return Ok(v);
            }
        }
        Ok(Vec::new())
    }

    async fn set_training_skip_dates(&self, dates: &[String]) -> AppResult<()> {
        let payload = serde_json::to_string(dates).map_err(internal)?;
        self.upsert_meta("calendar_training_skip_dates", &payload).await
    }

    pub async fn add_training_skip_date(&self, date: &str) -> AppResult<()> {
        let mut dates = self.get_training_skip_dates().await?;
        if !dates.iter().any(|d| d == date) {
            dates.push(date.to_string());
            self.set_training_skip_dates(&dates).await?;
        }
        Ok(())
    }

    /// Seed / dociągnięcie treningów wg harmonogramu (od dziś, ~3 miesiące).
    pub async fn seed_training_events(&self) -> AppResult<usize> {
        self.ensure_training_schedule_defaults().await?;
        let defaults = self.get_training_schedule_defaults().await?;
        let skip = self.get_training_skip_dates().await?;
        let existing = self.list_events().await?;
        let existing_trening_dates: std::collections::HashSet<String> = existing
            .iter()
            .filter(|e| e.event_type == "trening")
            .map(|e| e.date.clone())
            .collect();

        let today = chrono::Local::now().date_naive();
        let horizon = today + chrono::Duration::days(92);
        let mut created = 0usize;
        let now = chrono::Utc::now().to_rfc3339();

        let mut day = today;
        while day <= horizon {
            use chrono::Datelike;
            let iso_weekday = day.weekday().number_from_monday() as u8;
            if defaults.weekdays.contains(&iso_weekday) {
                let date_key = day.format("%Y-%m-%d").to_string();
                if !skip.iter().any(|d| d == &date_key)
                    && !existing_trening_dates.contains(&date_key)
                {
                    let event = CalendarEvent {
                        id: uuid::Uuid::new_v4().to_string(),
                        title: defaults.title.clone(),
                        event_type: "trening".into(),
                        date: date_key,
                        end_date: None,
                        time: Some(defaults.time.clone()),
                        location: Some(defaults.location.clone()),
                        description: None,
                        status: "scheduled".into(),
                        cancellation_note: None,
                        club_assigned: true,
                        source: "seeded".into(),
                        locked: false,
                        all_athletes: true,
                        assigned_athlete_ids: vec![],
                        withdrawals: vec![],
                        created_by: "system".into(),
                        created_at: now.clone(),
                        updated_at: now.clone(),
                    };
                    self.upsert_event(event).await?;
                    created += 1;
                }
            }
            day += chrono::Duration::days(1);
        }

        let until = horizon.format("%Y-%m-%d").to_string();
        self.upsert_meta("calendar_trainings_seeded_until", &until)
            .await?;
        if created > 0 {
            tracing::info!(created, "seed: treningi kalendarza");
        }
        Ok(created)
    }

    /// Po zmianie weekdays: usuń przyszłe seeded !locked poza nowym harmonogramem.
    pub async fn prune_seeded_trainings_outside_schedule(
        &self,
        defaults: &TrainingScheduleDefaults,
    ) -> AppResult<usize> {
        let today = chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();
        let events = self.list_events().await?;
        let mut removed = 0usize;
        for e in events {
            if e.event_type != "trening" || e.source != "seeded" || e.locked {
                continue;
            }
            if e.date.as_str() < today.as_str() {
                continue;
            }
            let Ok(naive) = chrono::NaiveDate::parse_from_str(&e.date, "%Y-%m-%d") else {
                continue;
            };
            use chrono::Datelike;
            let wd = naive.weekday().number_from_monday() as u8;
            if !defaults.weekdays.contains(&wd) {
                self.delete_event(&e.id).await?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn athlete_has_accepted_withdrawal(
        &self,
        event: &CalendarEvent,
        profile_ids: &[String],
        user_id: &str,
    ) -> bool {
        event.withdrawals.iter().any(|w| {
            w.status == WithdrawalStatus::Accepted
                && (profile_ids.contains(&w.athlete_id)
                    || w.user_id.as_deref() == Some(user_id))
        })
    }

    pub fn athlete_is_effectively_assigned(
        &self,
        event: &CalendarEvent,
        profile_ids: &[String],
        user_id: &str,
    ) -> bool {
        if self.athlete_has_accepted_withdrawal(event, profile_ids, user_id) {
            return false;
        }
        if event.all_athletes {
            return true;
        }
        profile_ids
            .iter()
            .any(|pid| event.assigned_athlete_ids.contains(pid))
    }

    /// Po zamknięciu okna treningu oznacza nieobecnych.
    pub async fn reconcile_attendance_for_event(&self, event_id: &str) -> AppResult<usize> {
        let event = match self.get_event(event_id).await? {
            Some(e) => e,
            None => return Ok(0),
        };
        if event.event_type != "trening" || event.status != "scheduled" {
            return Ok(0);
        }

        let defaults = self.get_training_schedule_defaults().await?;
        if !attendance_window_closed(&event, &defaults) {
            return Ok(0);
        }

        let profiles = self.list_profiles().await?;
        let users = self.list_users().await?;
        let mut existing = self.list_attendance_raw().await?;
        self.reconcile_absents_for_event(&event, &profiles, &users, &mut existing)
            .await
    }

    async fn reconcile_absents_for_event(
        &self,
        event: &CalendarEvent,
        profiles: &[AthleteProfile],
        users: &[UserRecord],
        existing: &mut Vec<AttendanceRecord>,
    ) -> AppResult<usize> {
        let event_id = event.id.as_str();
        let mut created = 0usize;
        let now = chrono::Utc::now().to_rfc3339();

        for profile in profiles {
            if profile.user_id == "manual" || profile.user_id.is_empty() {
                continue;
            }
            let user = users.iter().find(|u| u.id == profile.user_id);
            if let Some(u) = user {
                if !u.is_active || !u.roles.iter().any(|r| *r == Role::Zawodnik) {
                    continue;
                }
            } else {
                continue;
            }

            let profile_ids = [profile.id.clone()];
            if self.athlete_has_accepted_withdrawal(event, &profile_ids, &profile.user_id) {
                continue;
            }
            if !event.all_athletes && !event.assigned_athlete_ids.contains(&profile.id) {
                continue;
            }

            let has_present = existing.iter().any(|r| {
                r.user_id == profile.user_id
                    && r.event_id.as_deref() == Some(event_id)
                    && r.status == "present"
            });
            if has_present {
                continue;
            }
            let has_terminal = existing.iter().any(|r| {
                r.user_id == profile.user_id
                    && r.event_id.as_deref() == Some(event_id)
                    && (r.status == "absent"
                        || r.status == "pending_unauthorized"
                        || r.status == "rejected")
            });
            if has_terminal {
                continue;
            }

            let record = AttendanceRecord {
                id: uuid::Uuid::new_v4().to_string(),
                user_id: profile.user_id.clone(),
                display_name: profile.display_name.clone(),
                checked_at: now.clone(),
                session_token: String::new(),
                event_id: Some(event_id.to_string()),
                status: "absent".into(),
                source: "auto".into(),
            };
            self.upsert_attendance(record.clone()).await?;
            existing.push(record);
            created += 1;
        }
        Ok(created)
    }

    pub async fn reconcile_past_training_attendance(&self) -> AppResult<()> {
        self.reconcile_past_training_attendance_since_days(i64::MAX)
            .await
    }

    /// Auto-nieobecności tylko dla treningów z datą ≥ (dziś − days).
    /// Unika N× list_profiles/users/attendance na każde GET /events/mine.
    pub async fn reconcile_past_training_attendance_since_days(
        &self,
        days: i64,
    ) -> AppResult<()> {
        let defaults = self.get_training_schedule_defaults().await?;
        let events = self.list_events().await?;
        let cutoff = if days == i64::MAX {
            None
        } else {
            let d = chrono::Local::now().date_naive() - chrono::Duration::days(days.max(0));
            Some(d.format("%Y-%m-%d").to_string())
        };

        let closed: Vec<CalendarEvent> = events
            .into_iter()
            .filter(|e| {
                e.event_type == "trening"
                    && e.status == "scheduled"
                    && attendance_window_closed(e, &defaults)
                    && cutoff
                        .as_ref()
                        .map(|c| e.date.as_str() >= c.as_str())
                        .unwrap_or(true)
            })
            .collect();
        if closed.is_empty() {
            return Ok(());
        }

        let profiles = self.list_profiles().await?;
        let users = self.list_users().await?;
        let mut existing = self.list_attendance_raw().await?;
        for event in &closed {
            let _ = self
                .reconcile_absents_for_event(event, &profiles, &users, &mut existing)
                .await?;
        }
        Ok(())
    }

    // --- training plans ---

    pub async fn list_plans(&self) -> AppResult<Vec<TrainingPlan>> {
        let mut plans: Vec<TrainingPlan> = self.kv_list( "training_plans").await?;
        plans.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(plans)
    }

    pub async fn get_plan(&self, id: &str) -> AppResult<Option<TrainingPlan>> {
        self.kv_get( "training_plans", id).await
    }

    pub async fn upsert_plan(&self, plan: TrainingPlan) -> AppResult<()> {
        self.kv_upsert( "training_plans", &plan.id, &plan).await
    }

    pub async fn delete_plan(&self, id: &str) -> AppResult<()> {
        self.kv_delete( "training_plans", id).await
    }

    pub async fn plans_for_user(&self, user_id: &str) -> AppResult<Vec<TrainingPlan>> {
        Ok(self
            .list_plans()
            .await?
            .into_iter()
            .filter(|p| {
                p.assigned_user_ids.is_empty() || p.assigned_user_ids.iter().any(|id| id == user_id)
            })
            .collect())
    }

    pub async fn list_plan_progress(&self) -> AppResult<Vec<TrainingPlanProgress>> {
        self.kv_list( "plan_progress").await
    }

    pub async fn list_plan_progress_for_user(
        &self,
        user_id: &str,
    ) -> AppResult<Vec<TrainingPlanProgress>> {
        Ok(self
            .list_plan_progress()
            .await?
            .into_iter()
            .filter(|p| p.user_id == user_id)
            .collect())
    }

    pub async fn get_plan_progress(
        &self,
        plan_id: &str,
        user_id: &str,
    ) -> AppResult<Option<TrainingPlanProgress>> {
        let key = format!("{plan_id}:{user_id}");
        self.kv_get( "plan_progress", &key).await
    }

    pub async fn upsert_plan_progress(&self, progress: TrainingPlanProgress) -> AppResult<()> {
        let key = format!("{}:{}", progress.plan_id, progress.user_id);
        let mut p = progress;
        p.id = key.clone();
        self.kv_upsert( "plan_progress", &key, &p).await
    }

    // --- generic DB admin ---

    pub fn db_list_tables(&self) -> Vec<&'static str> {
        MANAGED_TABLES.to_vec()
    }

    pub async fn db_list_rows(&self, table: &str) -> AppResult<Vec<Value>> {
        match table {
            "users" => Ok(self
                .list_users()
                .await?
                .into_iter()
                .map(|u| {
                    serde_json::json!({
                        "id": u.id,
                        "email": u.email,
                        "display_name": u.display_name,
                        "roles": u.roles,
                        "is_active": u.is_active,
                        "created_at": u.created_at,
                        "updated_at": u.updated_at,
                        "password_hash": "[redacted]"
                    })
                })
                .collect()),
            "athlete_profiles" => values_from_list(self.list_profiles().await?),
            "feature_flags" => values_from_list(self.list_flags().await?),
            "competition_results" => values_from_list(self.list_results().await?),
            "cms_pages" => values_from_list(self.list_cms_pages().await?),
            "system_logs" => values_from_list(self.list_logs(500).await?),
            "attendance" => values_from_list(self.list_attendance_raw().await?),
            "training_plans" => values_from_list(self.list_plans().await?),
            "plan_progress" => values_from_list(self.list_plan_progress().await?),
            "contact_messages" => values_from_list(self.list_contact_messages().await?),
            "notifications" => {
                let items: Vec<Notification> = self.kv_list( "notifications").await?;
                values_from_list(items)
            }
            "device_tokens" => {
                let items: Vec<crate::models::club::DeviceToken> =
                    self.kv_list("device_tokens").await?;
                values_from_list(items)
            }
            "calendar_events" => values_from_list(self.list_events().await?),
            "email_tokens" => {
                let items: Vec<EmailToken> = self.kv_list("email_tokens").await?;
                values_from_list(items)
            }
            "meta" => self.list_meta_raw().await,
            _ => Err(AppError::NotFound(format!("Nieznana tabela: {table}"))),
        }
    }

    pub async fn db_upsert_row(&self, table: &str, row: Value) -> AppResult<()> {
        match table {
            "athlete_profiles" => {
                let profile: AthleteProfile = serde_json::from_value(row).map_err(|e| {
                    AppError::BadRequest(format!("Nieprawidłowy wiersz: {e}"))
                })?;
                self.upsert_profile(profile).await
            }
            "feature_flags" => {
                let flag: FeatureFlag = serde_json::from_value(row).map_err(|e| {
                    AppError::BadRequest(format!("Nieprawidłowy wiersz: {e}"))
                })?;
                self.upsert_flag(flag).await
            }
            "competition_results" => {
                let result: CompetitionResult = serde_json::from_value(row).map_err(|e| {
                    AppError::BadRequest(format!("Nieprawidłowy wiersz: {e}"))
                })?;
                self.upsert_result(result).await
            }
            "cms_pages" => {
                let page: CmsPage = serde_json::from_value(row)
                    .map_err(|e| AppError::BadRequest(format!("Nieprawidłowy wiersz: {e}")))?;
                self.upsert_cms_page(page).await
            }
            "training_plans" => {
                let plan: TrainingPlan = serde_json::from_value(row)
                    .map_err(|e| AppError::BadRequest(format!("Nieprawidłowy wiersz: {e}")))?;
                self.upsert_plan(plan).await
            }
            "attendance" => {
                let rec: AttendanceRecord = serde_json::from_value(row)
                    .map_err(|e| AppError::BadRequest(format!("Nieprawidłowy wiersz: {e}")))?;
                self.upsert_attendance(rec).await
            }
            "calendar_events" => {
                let event: CalendarEvent = serde_json::from_value(row)
                    .map_err(|e| AppError::BadRequest(format!("Nieprawidłowy wiersz: {e}")))?;
                self.upsert_event(event).await
            }
            "meta" => {
                let key = row
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AppError::BadRequest("meta wymaga pola key".into()))?;
                let value = row
                    .get("value")
                    .cloned()
                    .unwrap_or(Value::Null)
                    .to_string();
                self.upsert_meta(key, &value).await
            }
            "users" | "system_logs" | "plan_progress" | "notifications" | "contact_messages"
            | "device_tokens" | "email_tokens" => {
                Err(AppError::Forbidden(
                    "Edycja tej tabeli tylko przez dedykowane API.".into(),
                ))
            }
            _ => Err(AppError::NotFound(format!("Nieznana tabela: {table}"))),
        }
    }

    pub async fn db_delete_row(&self, table: &str, id: &str) -> AppResult<()> {
        match table {
            "athlete_profiles" => self.delete_profile(id).await,
            "cms_pages" => self.delete_cms_page(id).await,
            "feature_flags" => self.kv_delete( "feature_flags", id).await,
            "competition_results" => self.kv_delete( "competition_results", id).await,
            "training_plans" => self.delete_plan(id).await,
            "attendance" => self.kv_delete( "attendance", id).await,
            "calendar_events" => self.delete_event(id).await,
            "meta" => self.delete_meta(id).await,
            "users" => self.delete_user(id).await,
            "system_logs" => self.kv_delete( "system_logs", id).await,
            "plan_progress" => self.kv_delete( "plan_progress", id).await,
            "contact_messages" => self.delete_contact_message(id).await,
            "notifications" => self.delete_notification(id).await,
            _ => Err(AppError::NotFound(format!("Nieznana tabela: {table}"))),
        }
    }

    async fn list_meta_raw(&self) -> AppResult<Vec<Value>> {
        ensure_table("meta")?;
        self.db_op(|conn| async move {
            let mut rows_iter = conn
                .query("SELECT key, value FROM meta", ())
                .await
                .map_err(internal)?;
            let mut rows = Vec::new();
            while let Some(row) = rows_iter.next().await.map_err(internal)? {
                let key: String = row.get(0).map_err(internal)?;
                let value: String = row.get(1).map_err(internal)?;
                rows.push(serde_json::json!({ "key": key, "value": value }));
            }
            Ok(rows)
        })
        .await
    }

    async fn upsert_meta(&self, key: &str, value: &str) -> AppResult<()> {
        self.kv_upsert_raw("meta", key, value).await
    }

    async fn delete_meta(&self, key: &str) -> AppResult<()> {
        self.kv_delete_raw("meta", key).await
    }

    async fn kv_get_raw(&self, table: &str, key: &str) -> AppResult<Option<String>> {
        ensure_table(table)?;
        let sql = format!("SELECT value FROM {table} WHERE key = ?1");
        let key = key.to_string();
        self.db_op(|conn| {
            let sql = sql.clone();
            let key = key.clone();
            async move {
                let mut rows = conn.query(&sql, params![key]).await.map_err(internal)?;
                match rows.next().await.map_err(internal)? {
                    Some(row) => Ok(Some(row.get::<String>(0).map_err(internal)?)),
                    None => Ok(None),
                }
            }
        })
        .await
    }

    async fn kv_list_raw(&self, table: &str) -> AppResult<Vec<String>> {
        ensure_table(table)?;
        let sql = format!("SELECT value FROM {table}");
        self.db_op(|conn| {
            let sql = sql.clone();
            async move {
                let mut rows = conn.query(&sql, ()).await.map_err(internal)?;
                let mut items = Vec::new();
                while let Some(row) = rows.next().await.map_err(internal)? {
                    items.push(row.get::<String>(0).map_err(internal)?);
                }
                Ok(items)
            }
        })
        .await
    }

    async fn kv_upsert_raw(&self, table: &str, key: &str, value: &str) -> AppResult<()> {
        ensure_table(table)?;
        let sql = format!(
            "INSERT INTO {table} (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value"
        );
        let key = key.to_string();
        let value = value.to_string();
        self.db_op(|conn| {
            let sql = sql.clone();
            let key = key.clone();
            let value = value.clone();
            async move {
                conn.execute(&sql, params![key, value])
                    .await
                    .map_err(internal)?;
                Ok(())
            }
        })
        .await
    }

    async fn kv_delete_raw(&self, table: &str, key: &str) -> AppResult<()> {
        ensure_table(table)?;
        let sql = format!("DELETE FROM {table} WHERE key = ?1");
        let key = key.to_string();
        self.db_op(|conn| {
            let sql = sql.clone();
            let key = key.clone();
            async move {
                conn.execute(&sql, params![key]).await.map_err(internal)?;
                Ok(())
            }
        })
        .await
    }

    async fn kv_list<T: for<'de> Deserialize<'de> + Send + 'static>(
        &self,
        table: &str,
    ) -> AppResult<Vec<T>> {
        ensure_table(table)?;
        let sql = format!("SELECT value FROM {table}");
        self.db_op(|conn| {
            let sql = sql.clone();
            async move {
                let mut rows = conn.query(&sql, ()).await.map_err(internal)?;
                let mut items = Vec::new();
                while let Some(row) = rows.next().await.map_err(internal)? {
                    let value: String = row.get(0).map_err(internal)?;
                    items.push(serde_json::from_str(&value).map_err(internal)?);
                }
                Ok(items)
            }
        })
        .await
    }

    async fn kv_get<T: for<'de> Deserialize<'de> + Send + 'static>(
        &self,
        table: &str,
        key: &str,
    ) -> AppResult<Option<T>> {
        ensure_table(table)?;
        let sql = format!("SELECT value FROM {table} WHERE key = ?1");
        let key = key.to_string();
        self.db_op(|conn| {
            let sql = sql.clone();
            let key = key.clone();
            async move {
                let mut rows = conn.query(&sql, params![key]).await.map_err(internal)?;
                match rows.next().await.map_err(internal)? {
                    Some(row) => {
                        let value: String = row.get(0).map_err(internal)?;
                        Ok(Some(serde_json::from_str(&value).map_err(internal)?))
                    }
                    None => Ok(None),
                }
            }
        })
        .await
    }

    async fn kv_upsert<T: Serialize + Sync>(
        &self,
        table: &str,
        key: &str,
        value: &T,
    ) -> AppResult<()> {
        ensure_table(table)?;
        let payload = serde_json::to_string(value).map_err(internal)?;
        self.kv_upsert_raw(table, key, &payload).await
    }

    async fn kv_delete(&self, table: &str, key: &str) -> AppResult<()> {
        self.kv_delete_raw(table, key).await
    }
}

fn values_from_list<T: Serialize>(items: Vec<T>) -> AppResult<Vec<Value>> {
    items
        .into_iter()
        .map(|item| serde_json::to_value(item).map_err(internal))
        .collect()
}

fn ensure_table(table: &str) -> AppResult<()> {
    if MANAGED_TABLES.contains(&table) {
        Ok(())
    } else {
        Err(AppError::NotFound(format!("Nieznana tabela: {table}")))
    }
}

fn attendance_window_bounds() -> (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) {
    use chrono::{Datelike, Duration, TimeZone, Utc};
    let now = Utc::now();
    let year = now.year();
    let year_start = Utc
        .with_ymd_and_hms(year, 1, 1, 0, 0, 0)
        .single()
        .unwrap_or(now);
    let year_end = Utc
        .with_ymd_and_hms(year, 12, 31, 23, 59, 59)
        .single()
        .unwrap_or(now);
    let start = year_start - Duration::days(62);
    let end = year_end + Duration::days(62);
    (start, end)
}

/// Czy okno skanowania (time–end_time ± buffer) już się zamknęło dla wydarzenia.
fn attendance_window_closed(
    event: &CalendarEvent,
    defaults: &TrainingScheduleDefaults,
) -> bool {
    let Some((_, end)) = attendance_scan_window(event, defaults) else {
        return false;
    };
    chrono::Local::now().naive_local() > end
}

/// Czy bieżący moment mieści się w oknie [time − buffer, end_time + buffer].
fn attendance_window_open(
    event: &CalendarEvent,
    defaults: &TrainingScheduleDefaults,
) -> bool {
    let Some((start, end)) = attendance_scan_window(event, defaults) else {
        return false;
    };
    let now = chrono::Local::now().naive_local();
    now >= start && now <= end
}

fn attendance_scan_window(
    event: &CalendarEvent,
    defaults: &TrainingScheduleDefaults,
) -> Option<(chrono::NaiveDateTime, chrono::NaiveDateTime)> {
    use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

    let date = NaiveDate::parse_from_str(&event.date, "%Y-%m-%d").ok()?;
    let start_hm = event
        .time
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(defaults.time.as_str());
    let end_hm = defaults.end_time.as_str();
    let start_time = NaiveTime::parse_from_str(start_hm, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(start_hm, "%H:%M:%S"))
        .unwrap_or_else(|_| NaiveTime::from_hms_opt(15, 0, 0).unwrap());
    let end_time = NaiveTime::parse_from_str(end_hm, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(end_hm, "%H:%M:%S"))
        .unwrap_or_else(|_| NaiveTime::from_hms_opt(18, 0, 0).unwrap());
    let buffer = chrono::Duration::minutes(defaults.attendance_buffer_minutes as i64);
    let start = NaiveDateTime::new(date, start_time) - buffer;
    let end = NaiveDateTime::new(date, end_time) + buffer;
    Some((start, end))
}

fn local_db_path(config: &Config) -> PathBuf {
    let raw = config
        .database_url
        .strip_prefix("file:")
        .unwrap_or(&config.database_url);
    let path = Path::new(raw);
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("redb"))
    {
        path.with_extension("db")
    } else {
        path.to_path_buf()
    }
}
