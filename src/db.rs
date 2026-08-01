use std::path::{Path, PathBuf};
use std::sync::Arc;

use redb::{Database as RedbDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::password::{hash_password, verify_password};
use crate::config::Config;
use crate::error::{internal, AppError, AppResult};
use crate::models::club::{
    AthleteProfile, AthleteStats, AttendanceRecord, AttendanceSession, CmsBlock, CmsPage, CmsStatus,
    CompetitionResult, FeatureFlag, FlagKind, LogLevel, ResultStatus, SiteStats, SystemLog,
    TrainingPlan, TrainingPlanProgress,
};
use crate::models::role::{has_role, roles_from_json, roles_to_json, Role};
use crate::models::user::UserRecord;

const USERS_TABLE: TableDefinition<&str, &str> = TableDefinition::new("users");
const META_TABLE: TableDefinition<&str, &str> = TableDefinition::new("meta");
const PROFILES_TABLE: TableDefinition<&str, &str> = TableDefinition::new("athlete_profiles");
const FLAGS_TABLE: TableDefinition<&str, &str> = TableDefinition::new("feature_flags");
const RESULTS_TABLE: TableDefinition<&str, &str> = TableDefinition::new("competition_results");
const CMS_TABLE: TableDefinition<&str, &str> = TableDefinition::new("cms_pages");
const LOGS_TABLE: TableDefinition<&str, &str> = TableDefinition::new("system_logs");
const ATTENDANCE_TABLE: TableDefinition<&str, &str> = TableDefinition::new("attendance");
const PLANS_TABLE: TableDefinition<&str, &str> = TableDefinition::new("training_plans");
const PLAN_PROGRESS_TABLE: TableDefinition<&str, &str> = TableDefinition::new("plan_progress");

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
    created_at: String,
    updated_at: String,
}

impl From<StoredUser> for UserRecord {
    fn from(u: StoredUser) -> Self {
        let roles = roles_from_json(&u.roles).unwrap_or_default();
        Self {
            id: u.id,
            email: u.email,
            password_hash: u.password_hash,
            display_name: u.display_name,
            roles,
            is_active: u.is_active,
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
            created_at: u.created_at.clone(),
            updated_at: u.updated_at.clone(),
        }
    }
}

#[derive(Clone)]
pub struct Database {
    inner: Arc<RedbDatabase>,
}

impl Database {
    pub async fn connect(config: &Config) -> Result<Self, AppError> {
        if config.is_remote_db() {
            tracing::warn!(
                "Turso URL wykryty — lokalnie używam pliku redb. Pełne libsql/Turso: włącz w Docker/Linux (README)."
            );
        }

        let path = local_db_path(config);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(internal)?;
        }

        let db = RedbDatabase::create(&path).map_err(internal)?;
        Ok(Self {
            inner: Arc::new(db),
        })
    }

    pub async fn migrate(&self) -> AppResult<()> {
        let write = self.inner.begin_write().map_err(internal)?;
        {
            let _ = write.open_table(USERS_TABLE).map_err(internal)?;
            let _ = write.open_table(META_TABLE).map_err(internal)?;
            let _ = write.open_table(PROFILES_TABLE).map_err(internal)?;
            let _ = write.open_table(FLAGS_TABLE).map_err(internal)?;
            let _ = write.open_table(RESULTS_TABLE).map_err(internal)?;
            let _ = write.open_table(CMS_TABLE).map_err(internal)?;
            let _ = write.open_table(LOGS_TABLE).map_err(internal)?;
            let _ = write.open_table(ATTENDANCE_TABLE).map_err(internal)?;
            let _ = write.open_table(PLANS_TABLE).map_err(internal)?;
            let _ = write.open_table(PLAN_PROGRESS_TABLE).map_err(internal)?;
        }
        write.commit().map_err(internal)?;
        Ok(())
    }

    pub async fn seed_if_empty(&self, config: &Config) -> AppResult<()> {
        if self.user_count()? == 0 {
            tracing::info!("Baza pusta — tworzę konto seed superadmin");
            let now = chrono::Utc::now().to_rfc3339();
            self.insert_user(StoredUser {
                id: uuid::Uuid::new_v4().to_string(),
                email: config.seed_superadmin_email.clone(),
                password_hash: hash_password(&config.seed_superadmin_password)?,
                display_name: "Superadmin".into(),
                roles: roles_to_json(&[Role::Superadmin]),
                is_active: true,
                created_at: now.clone(),
                updated_at: now,
            })?;
            tracing::info!(
                "Seed OK — superadmin: {} (hasło z SEED_SUPERADMIN_PASSWORD)",
                config.seed_superadmin_email
            );
        }

        self.seed_defaults().await?;
        Ok(())
    }

    async fn seed_defaults(&self) -> AppResult<()> {
        if self.list_flags()?.is_empty() {
            let now = chrono::Utc::now().to_rfc3339();
            for (key, label, kind, enabled) in [
                (
                    "public_blog",
                    "Publiczny blog",
                    FlagKind::Stable,
                    true,
                ),
                (
                    "announcements_board",
                    "Tablica ogłoszeń",
                    FlagKind::Stable,
                    true,
                ),
                (
                    "experimental_live_scores",
                    "Live wyniki (eksperymentalne)",
                    FlagKind::Experimental,
                    false,
                ),
                (
                    "experimental_ai_summaries",
                    "AI podsumowania CMS",
                    FlagKind::Experimental,
                    false,
                ),
            ] {
                self.upsert_flag(FeatureFlag {
                    key: key.into(),
                    label: label.into(),
                    enabled,
                    kind,
                    updated_at: now.clone(),
                })?;
            }
        }

        if self.list_cms_pages()?.is_empty() {
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
            })?;
        }

        if self.list_results()?.is_empty() {
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
                status: ResultStatus::Pending,
                reviewer_note: None,
                submitted_at: now.clone(),
                updated_at: now,
            })?;
        }

        if self.get_attendance_session()?.is_none() {
            let now = chrono::Utc::now().to_rfc3339();
            self.set_attendance_session(AttendanceSession {
                token: uuid::Uuid::new_v4().to_string(),
                label: "Trening".into(),
                created_at: now.clone(),
                refreshed_at: now,
            })?;
        }

        Ok(())
    }

    fn user_count(&self) -> AppResult<usize> {
        Ok(self.list_users()?.len())
    }

    fn insert_user(&self, user: StoredUser) -> AppResult<()> {
        let payload = serde_json::to_string(&user).map_err(internal)?;
        let email_key = user.email.to_ascii_lowercase();
        let write = self.inner.begin_write().map_err(internal)?;
        {
            let mut table = write.open_table(USERS_TABLE).map_err(internal)?;
            if table.get(email_key.as_str()).map_err(internal)?.is_some() {
                return Err(AppError::BadRequest(
                    "Konto z tym e-mailem już istnieje.".into(),
                ));
            }
            table
                .insert(email_key.as_str(), payload.as_str())
                .map_err(internal)?;
        }
        write.commit().map_err(internal)?;
        Ok(())
    }

    pub fn list_users(&self) -> AppResult<Vec<UserRecord>> {
        let read = self.inner.begin_read().map_err(internal)?;
        let table = read.open_table(USERS_TABLE).map_err(internal)?;
        let mut users = Vec::new();
        for entry in table.iter().map_err(internal)? {
            let (_, value) = entry.map_err(internal)?;
            let stored: StoredUser = serde_json::from_str(value.value()).map_err(internal)?;
            users.push(UserRecord::from(stored));
        }
        users.sort_by(|a: &UserRecord, b: &UserRecord| a.email.cmp(&b.email));
        Ok(users)
    }

    pub async fn find_user_by_email(&self, email: &str) -> AppResult<Option<UserRecord>> {
        let key = email.trim().to_ascii_lowercase();
        let read = self.inner.begin_read().map_err(internal)?;
        let table = read.open_table(USERS_TABLE).map_err(internal)?;
        match table.get(key.as_str()).map_err(internal)? {
            Some(access) => {
                let stored: StoredUser =
                    serde_json::from_str(access.value()).map_err(internal)?;
                Ok(Some(stored.into()))
            }
            None => Ok(None),
        }
    }

    pub async fn find_user_by_id(&self, id: &str) -> AppResult<Option<UserRecord>> {
        Ok(self.list_users()?.into_iter().find(|u| u.id == id))
    }

    pub async fn authenticate(&self, email: &str, password: &str) -> AppResult<UserRecord> {
        let user = self
            .find_user_by_email(email)
            .await?
            .ok_or_else(AppError::unauthorized)?;

        if !user.is_active {
            return Err(AppError::Forbidden("Konto jest nieaktywne.".into()));
        }

        if !verify_password(password, &user.password_hash)? {
            return Err(AppError::unauthorized());
        }

        Ok(user)
    }

    pub fn create_user(
        &self,
        email: &str,
        password: &str,
        display_name: &str,
        roles: Vec<Role>,
    ) -> AppResult<UserRecord> {
        let now = chrono::Utc::now().to_rfc3339();
        let user = UserRecord {
            id: uuid::Uuid::new_v4().to_string(),
            email: email.trim().to_ascii_lowercase(),
            password_hash: hash_password(password)?,
            display_name: display_name.trim().to_string(),
            roles,
            is_active: true,
            created_at: now.clone(),
            updated_at: now,
        };
        self.insert_user(StoredUser::from(&user))?;
        Ok(user)
    }

    pub fn update_user(&self, user: &UserRecord) -> AppResult<()> {
        let existing = self
            .list_users()?
            .into_iter()
            .find(|u| u.id == user.id)
            .ok_or_else(|| AppError::NotFound("Użytkownik nie istnieje.".into()))?;

        if has_role(&existing.roles, Role::Superadmin)
            && existing.roles.contains(&Role::Superadmin)
        {
            // Chronione konta superadmin: nie wolno usuwać ról ani banować.
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

        // Jeśli e-mail się zmienił, usuń stary klucz.
        let write = self.inner.begin_write().map_err(internal)?;
        {
            let mut table = write.open_table(USERS_TABLE).map_err(internal)?;
            let old_key = existing.email.to_ascii_lowercase();
            let new_key = stored.email.to_ascii_lowercase();
            if old_key != new_key {
                if table.get(new_key.as_str()).map_err(internal)?.is_some() {
                    return Err(AppError::BadRequest(
                        "Konto z tym e-mailem już istnieje.".into(),
                    ));
                }
                table.remove(old_key.as_str()).map_err(internal)?;
            }
            let payload = serde_json::to_string(&stored).map_err(internal)?;
            table
                .insert(new_key.as_str(), payload.as_str())
                .map_err(internal)?;
        }
        write.commit().map_err(internal)?;
        Ok(())
    }

    pub fn delete_user(&self, id: &str) -> AppResult<()> {
        let existing = self
            .list_users()?
            .into_iter()
            .find(|u| u.id == id)
            .ok_or_else(|| AppError::NotFound("Użytkownik nie istnieje.".into()))?;

        if existing.roles.contains(&Role::Superadmin) {
            return Err(AppError::Forbidden(
                "Nie można usunąć konta Superadmin.".into(),
            ));
        }

        let key = existing.email.to_ascii_lowercase();
        let write = self.inner.begin_write().map_err(internal)?;
        {
            let mut table = write.open_table(USERS_TABLE).map_err(internal)?;
            table.remove(key.as_str()).map_err(internal)?;
        }
        write.commit().map_err(internal)?;
        Ok(())
    }

    // --- profiles ---

    pub fn list_profiles(&self) -> AppResult<Vec<AthleteProfile>> {
        kv_list(&self.inner, PROFILES_TABLE)
    }

    pub fn upsert_profile(&self, profile: AthleteProfile) -> AppResult<()> {
        kv_upsert(&self.inner, PROFILES_TABLE, &profile.id, &profile)
    }

    pub fn delete_profile(&self, id: &str) -> AppResult<()> {
        kv_delete(&self.inner, PROFILES_TABLE, id)
    }

    pub fn get_profile(&self, id: &str) -> AppResult<Option<AthleteProfile>> {
        kv_get(&self.inner, PROFILES_TABLE, id)
    }

    // --- flags ---

    pub fn list_flags(&self) -> AppResult<Vec<FeatureFlag>> {
        let mut flags: Vec<FeatureFlag> = kv_list(&self.inner, FLAGS_TABLE)?;
        flags.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(flags)
    }

    pub fn upsert_flag(&self, flag: FeatureFlag) -> AppResult<()> {
        kv_upsert(&self.inner, FLAGS_TABLE, &flag.key, &flag)
    }

    // --- results ---

    pub fn list_results(&self) -> AppResult<Vec<CompetitionResult>> {
        let mut items: Vec<CompetitionResult> = kv_list(&self.inner, RESULTS_TABLE)?;
        items.sort_by(|a, b| b.submitted_at.cmp(&a.submitted_at));
        Ok(items)
    }

    pub fn upsert_result(&self, result: CompetitionResult) -> AppResult<()> {
        kv_upsert(&self.inner, RESULTS_TABLE, &result.id, &result)
    }

    pub fn get_result(&self, id: &str) -> AppResult<Option<CompetitionResult>> {
        kv_get(&self.inner, RESULTS_TABLE, id)
    }

    // --- cms ---

    pub fn list_cms_pages(&self) -> AppResult<Vec<CmsPage>> {
        let mut pages: Vec<CmsPage> = kv_list(&self.inner, CMS_TABLE)?;
        pages.sort_by(|a, b| a.slug.cmp(&b.slug));
        Ok(pages)
    }

    pub fn upsert_cms_page(&self, page: CmsPage) -> AppResult<()> {
        kv_upsert(&self.inner, CMS_TABLE, &page.id, &page)
    }

    pub fn get_cms_page(&self, id: &str) -> AppResult<Option<CmsPage>> {
        kv_get(&self.inner, CMS_TABLE, id)
    }

    pub fn delete_cms_page(&self, id: &str) -> AppResult<()> {
        kv_delete(&self.inner, CMS_TABLE, id)
    }

    // --- logs ---

    pub fn list_logs(&self, limit: usize) -> AppResult<Vec<SystemLog>> {
        let mut logs: Vec<SystemLog> = kv_list(&self.inner, LOGS_TABLE)?;
        logs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        logs.truncate(limit);
        Ok(logs)
    }

    pub fn append_log(
        &self,
        level: LogLevel,
        source: &str,
        message: &str,
        actor_id: Option<&str>,
    ) -> AppResult<()> {
        let log = SystemLog {
            id: uuid::Uuid::new_v4().to_string(),
            level,
            source: source.into(),
            message: message.into(),
            actor_id: actor_id.map(|s| s.to_string()),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        kv_upsert(&self.inner, LOGS_TABLE, &log.id, &log)
    }

    // --- stats ---

    pub fn site_stats(&self) -> AppResult<SiteStats> {
        let users = self.list_users()?;
        let results = self.list_results()?;
        let pages = self.list_cms_pages()?;
        Ok(SiteStats {
            users: users.len(),
            active_users: users.iter().filter(|u| u.is_active).count(),
            athlete_profiles: self.list_profiles()?.len(),
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
            feature_flags: self.list_flags()?.len(),
            system_logs: self.list_logs(10_000)?.len(),
        })
    }

    pub fn athlete_stats(&self, user_id: &str) -> AppResult<AthleteStats> {
        let results = self.list_results()?;
        let mine: Vec<_> = results
            .iter()
            .filter(|r| r.user_id.as_deref() == Some(user_id))
            .collect();
        let attendance = self.list_attendance_in_window()?;
        let mine_att: Vec<_> = attendance
            .iter()
            .filter(|a| a.user_id == user_id)
            .collect();
        let now = chrono::Utc::now();
        let month_prefix = now.format("%Y-%m").to_string();
        let plans = self.plans_for_user(user_id)?;
        let progress = self.list_plan_progress_for_user(user_id)?;
        let completed = progress
            .iter()
            .flat_map(|p| p.entries.iter())
            .filter(|e| e.completed)
            .count();
        let profile = self
            .list_profiles()?
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

    pub fn get_attendance_session(&self) -> AppResult<Option<AttendanceSession>> {
        kv_get(&self.inner, META_TABLE, "attendance_session")
    }

    pub fn set_attendance_session(&self, session: AttendanceSession) -> AppResult<()> {
        let payload = serde_json::to_string(&session).map_err(internal)?;
        self.upsert_meta("attendance_session", &payload)
    }

    pub fn list_attendance_raw(&self) -> AppResult<Vec<AttendanceRecord>> {
        kv_list(&self.inner, ATTENDANCE_TABLE)
    }

    pub fn list_attendance_in_window(&self) -> AppResult<Vec<AttendanceRecord>> {
        let (start, end) = attendance_window_bounds();
        let mut items = self.list_attendance_raw()?;
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

    pub fn prune_attendance_outside_window(&self) -> AppResult<()> {
        let (start, end) = attendance_window_bounds();
        let all = self.list_attendance_raw()?;
        for r in all {
            let keep = chrono::DateTime::parse_from_rfc3339(&r.checked_at)
                .map(|dt| {
                    let t = dt.with_timezone(&chrono::Utc);
                    t >= start && t <= end
                })
                .unwrap_or(false);
            if !keep {
                kv_delete(&self.inner, ATTENDANCE_TABLE, &r.id)?;
            }
        }
        Ok(())
    }

    pub fn upsert_attendance(&self, record: AttendanceRecord) -> AppResult<()> {
        kv_upsert(&self.inner, ATTENDANCE_TABLE, &record.id, &record)
    }

    pub fn check_in_attendance(
        &self,
        user_id: &str,
        display_name: &str,
        token: &str,
    ) -> AppResult<AttendanceRecord> {
        let session = self
            .get_attendance_session()?
            .ok_or_else(|| AppError::BadRequest("Brak aktywnej sesji obecności.".into()))?;
        if session.token != token {
            return Err(AppError::BadRequest(
                "Nieprawidłowy lub nieaktualny kod QR.".into(),
            ));
        }

        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let already = self.list_attendance_in_window()?.into_iter().any(|r| {
            r.user_id == user_id && r.checked_at.starts_with(&today)
        });
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
        self.upsert_attendance(record.clone())?;
        self.prune_attendance_outside_window()?;
        Ok(record)
    }

    // --- training plans ---

    pub fn list_plans(&self) -> AppResult<Vec<TrainingPlan>> {
        let mut plans: Vec<TrainingPlan> = kv_list(&self.inner, PLANS_TABLE)?;
        plans.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(plans)
    }

    pub fn get_plan(&self, id: &str) -> AppResult<Option<TrainingPlan>> {
        kv_get(&self.inner, PLANS_TABLE, id)
    }

    pub fn upsert_plan(&self, plan: TrainingPlan) -> AppResult<()> {
        kv_upsert(&self.inner, PLANS_TABLE, &plan.id, &plan)
    }

    pub fn delete_plan(&self, id: &str) -> AppResult<()> {
        kv_delete(&self.inner, PLANS_TABLE, id)
    }

    pub fn plans_for_user(&self, user_id: &str) -> AppResult<Vec<TrainingPlan>> {
        Ok(self
            .list_plans()?
            .into_iter()
            .filter(|p| {
                p.assigned_user_ids.is_empty() || p.assigned_user_ids.iter().any(|id| id == user_id)
            })
            .collect())
    }

    pub fn list_plan_progress(&self) -> AppResult<Vec<TrainingPlanProgress>> {
        kv_list(&self.inner, PLAN_PROGRESS_TABLE)
    }

    pub fn list_plan_progress_for_user(
        &self,
        user_id: &str,
    ) -> AppResult<Vec<TrainingPlanProgress>> {
        Ok(self
            .list_plan_progress()?
            .into_iter()
            .filter(|p| p.user_id == user_id)
            .collect())
    }

    pub fn get_plan_progress(
        &self,
        plan_id: &str,
        user_id: &str,
    ) -> AppResult<Option<TrainingPlanProgress>> {
        let key = format!("{plan_id}:{user_id}");
        kv_get(&self.inner, PLAN_PROGRESS_TABLE, &key)
    }

    pub fn upsert_plan_progress(&self, progress: TrainingPlanProgress) -> AppResult<()> {
        let key = format!("{}:{}", progress.plan_id, progress.user_id);
        let mut p = progress;
        p.id = key.clone();
        kv_upsert(&self.inner, PLAN_PROGRESS_TABLE, &key, &p)
    }

    // --- generic DB admin ---

    pub fn db_list_tables(&self) -> Vec<&'static str> {
        MANAGED_TABLES.to_vec()
    }

    pub fn db_list_rows(&self, table: &str) -> AppResult<Vec<Value>> {
        match table {
            "users" => Ok(self
                .list_users()?
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
            "athlete_profiles" => values_from_list(self.list_profiles()?),
            "feature_flags" => values_from_list(self.list_flags()?),
            "competition_results" => values_from_list(self.list_results()?),
            "cms_pages" => values_from_list(self.list_cms_pages()?),
            "system_logs" => values_from_list(self.list_logs(500)?),
            "attendance" => values_from_list(self.list_attendance_raw()?),
            "training_plans" => values_from_list(self.list_plans()?),
            "plan_progress" => values_from_list(self.list_plan_progress()?),
            "meta" => self.list_meta_raw(),
            _ => Err(AppError::NotFound(format!("Nieznana tabela: {table}"))),
        }
    }

    pub fn db_upsert_row(&self, table: &str, row: Value) -> AppResult<()> {
        match table {
            "athlete_profiles" => {
                let profile: AthleteProfile = serde_json::from_value(row).map_err(|e| {
                    AppError::BadRequest(format!("Nieprawidłowy wiersz: {e}"))
                })?;
                self.upsert_profile(profile)
            }
            "feature_flags" => {
                let flag: FeatureFlag = serde_json::from_value(row).map_err(|e| {
                    AppError::BadRequest(format!("Nieprawidłowy wiersz: {e}"))
                })?;
                self.upsert_flag(flag)
            }
            "competition_results" => {
                let result: CompetitionResult = serde_json::from_value(row).map_err(|e| {
                    AppError::BadRequest(format!("Nieprawidłowy wiersz: {e}"))
                })?;
                self.upsert_result(result)
            }
            "cms_pages" => {
                let page: CmsPage = serde_json::from_value(row)
                    .map_err(|e| AppError::BadRequest(format!("Nieprawidłowy wiersz: {e}")))?;
                self.upsert_cms_page(page)
            }
            "training_plans" => {
                let plan: TrainingPlan = serde_json::from_value(row)
                    .map_err(|e| AppError::BadRequest(format!("Nieprawidłowy wiersz: {e}")))?;
                self.upsert_plan(plan)
            }
            "attendance" => {
                let rec: AttendanceRecord = serde_json::from_value(row)
                    .map_err(|e| AppError::BadRequest(format!("Nieprawidłowy wiersz: {e}")))?;
                self.upsert_attendance(rec)
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
                self.upsert_meta(key, &value)
            }
            "users" | "system_logs" | "plan_progress" => Err(AppError::Forbidden(
                "Edycja tej tabeli tylko przez dedykowane API.".into(),
            )),
            _ => Err(AppError::NotFound(format!("Nieznana tabela: {table}"))),
        }
    }

    pub fn db_delete_row(&self, table: &str, id: &str) -> AppResult<()> {
        match table {
            "athlete_profiles" => self.delete_profile(id),
            "cms_pages" => self.delete_cms_page(id),
            "feature_flags" => kv_delete(&self.inner, FLAGS_TABLE, id),
            "competition_results" => kv_delete(&self.inner, RESULTS_TABLE, id),
            "training_plans" => self.delete_plan(id),
            "attendance" => kv_delete(&self.inner, ATTENDANCE_TABLE, id),
            "meta" => self.delete_meta(id),
            "users" => self.delete_user(id),
            "system_logs" => kv_delete(&self.inner, LOGS_TABLE, id),
            "plan_progress" => kv_delete(&self.inner, PLAN_PROGRESS_TABLE, id),
            _ => Err(AppError::NotFound(format!("Nieznana tabela: {table}"))),
        }
    }

    fn list_meta_raw(&self) -> AppResult<Vec<Value>> {
        let read = self.inner.begin_read().map_err(internal)?;
        let table = read.open_table(META_TABLE).map_err(internal)?;
        let mut rows = Vec::new();
        for entry in table.iter().map_err(internal)? {
            let (k, v) = entry.map_err(internal)?;
            rows.push(serde_json::json!({
                "key": k.value(),
                "value": v.value()
            }));
        }
        Ok(rows)
    }

    fn upsert_meta(&self, key: &str, value: &str) -> AppResult<()> {
        let write = self.inner.begin_write().map_err(internal)?;
        {
            let mut table = write.open_table(META_TABLE).map_err(internal)?;
            table.insert(key, value).map_err(internal)?;
        }
        write.commit().map_err(internal)?;
        Ok(())
    }

    fn delete_meta(&self, key: &str) -> AppResult<()> {
        let write = self.inner.begin_write().map_err(internal)?;
        {
            let mut table = write.open_table(META_TABLE).map_err(internal)?;
            table.remove(key).map_err(internal)?;
        }
        write.commit().map_err(internal)?;
        Ok(())
    }
}

fn values_from_list<T: Serialize>(items: Vec<T>) -> AppResult<Vec<Value>> {
    items
        .into_iter()
        .map(|item| serde_json::to_value(item).map_err(internal))
        .collect()
}

fn kv_list<T: for<'de> Deserialize<'de>>(
    db: &Arc<RedbDatabase>,
    table_def: TableDefinition<&str, &str>,
) -> AppResult<Vec<T>> {
    let read = db.begin_read().map_err(internal)?;
    let table = read.open_table(table_def).map_err(internal)?;
    let mut items = Vec::new();
    for entry in table.iter().map_err(internal)? {
        let (_, value) = entry.map_err(internal)?;
        items.push(serde_json::from_str(value.value()).map_err(internal)?);
    }
    Ok(items)
}

fn kv_get<T: for<'de> Deserialize<'de>>(
    db: &Arc<RedbDatabase>,
    table_def: TableDefinition<&str, &str>,
    key: &str,
) -> AppResult<Option<T>> {
    let read = db.begin_read().map_err(internal)?;
    let table = read.open_table(table_def).map_err(internal)?;
    match table.get(key).map_err(internal)? {
        Some(access) => Ok(Some(
            serde_json::from_str(access.value()).map_err(internal)?,
        )),
        None => Ok(None),
    }
}

fn kv_upsert<T: Serialize>(
    db: &Arc<RedbDatabase>,
    table_def: TableDefinition<&str, &str>,
    key: &str,
    value: &T,
) -> AppResult<()> {
    let payload = serde_json::to_string(value).map_err(internal)?;
    let write = db.begin_write().map_err(internal)?;
    {
        let mut table = write.open_table(table_def).map_err(internal)?;
        table.insert(key, payload.as_str()).map_err(internal)?;
    }
    write.commit().map_err(internal)?;
    Ok(())
}

fn kv_delete(
    db: &Arc<RedbDatabase>,
    table_def: TableDefinition<&str, &str>,
    key: &str,
) -> AppResult<()> {
    let write = db.begin_write().map_err(internal)?;
    {
        let mut table = write.open_table(table_def).map_err(internal)?;
        table.remove(key).map_err(internal)?;
    }
    write.commit().map_err(internal)?;
    Ok(())
}

fn attendance_window_bounds() -> (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) {
    use chrono::{Datelike, Duration, TimeZone, Utc};
    let now = Utc::now();
    let year = now.year();
    // Aktualny rok ± 2 miesiące
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
    if config.is_remote_db() {
        return PathBuf::from("./data/slavia.redb");
    }

    if path.extension().is_some_and(|ext| {
        ext.eq_ignore_ascii_case("db") || ext.eq_ignore_ascii_case("sqlite")
    }) {
        path.with_extension("redb")
    } else {
        path.to_path_buf()
    }
}
