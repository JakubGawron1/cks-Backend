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
    AthleteProfile, AthleteStats, AttendanceRecord, AttendanceSession, CmsBlock, CmsPage, CmsStatus,
    CompetitionResult, ContactMessage, FeatureFlag, FlagKind, FlagRolloutStatus, LogLevel,
    Notification, ResultStatus, SiteStats, SystemLog, TrainingPlan, TrainingPlanProgress,
};
use crate::models::role::{has_role, roles_from_json, roles_to_json, Role};
use crate::models::user::UserRecord;

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
            created_at: u.created_at.clone(),
            updated_at: u.updated_at.clone(),
        }
    }
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
            self.insert_user(StoredUser {
                id: uuid::Uuid::new_v4().to_string(),
                email: config.seed_superadmin_email.clone(),
                password_hash: hash_password(&config.seed_superadmin_password)?,
                display_name: "Superadmin".into(),
                roles: roles_to_json(&[Role::Superadmin]),
                is_active: true,
                ui_theme: crate::models::user::default_ui_theme(),
                photo_url: None,
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
            })
            .await?;
        }

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
        let user = UserRecord {
            id: uuid::Uuid::new_v4().to_string(),
            email: email.trim().to_ascii_lowercase(),
            password_hash: hash_password(password)?,
            display_name: display_name.trim().to_string(),
            roles,
            is_active: true,
            ui_theme: crate::models::user::default_ui_theme(),
            photo_url: normalize_optional_url(photo_url),
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

    // --- logs ---

    pub async fn list_logs(&self, limit: usize) -> AppResult<Vec<SystemLog>> {
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
        self.kv_upsert( "system_logs", &log.id, &log).await
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

        Ok(AthleteStats {
            results_accepted: mine
                .iter()
                .filter(|r| r.status == ResultStatus::Accepted)
                .count(),
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

    pub async fn list_attendance_raw(&self) -> AppResult<Vec<AttendanceRecord>> {
        self.kv_list( "attendance").await
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
        let session = self
            .get_attendance_session()
            .await?
            .ok_or_else(|| AppError::BadRequest("Brak aktywnej sesji obecności.".into()))?;
        if session.token != token {
            return Err(AppError::BadRequest(
                "Nieprawidłowy lub nieaktualny kod QR.".into(),
            ));
        }

        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let already = self
            .list_attendance_in_window()
            .await?
            .into_iter()
            .any(|r| r.user_id == user_id && r.checked_at.starts_with(&today));
        if already {
            return Err(AppError::BadRequest(
                "Obecność na dziś jest już zapisana.".into(),
            ));
        }

        let record = AttendanceRecord {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: user_id.into(),
            display_name: display_name.into(),
            checked_at: chrono::Utc::now().to_rfc3339(),
            session_token: token.into(),
        };
        self.upsert_attendance(record.clone()).await?;
        self.prune_attendance_outside_window().await?;
        Ok(record)
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
            "users" | "system_logs" | "plan_progress" | "notifications" | "contact_messages" => {
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
