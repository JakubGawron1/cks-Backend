use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::auth::handlers::{login, me};
use crate::handlers;
use crate::state::AppState;

async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "slavia-backend",
        "auth": true
    }))
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/login", post(login))
        .route("/api/auth/me", get(me))
        .route("/api/users", get(handlers::list_users).post(handlers::create_user))
        .route(
            "/api/users/{id}",
            patch(handlers::update_user).delete(handlers::delete_user),
        )
        .route(
            "/api/profiles",
            get(handlers::list_profiles).post(handlers::create_profile),
        )
        .route(
            "/api/profiles/{id}",
            patch(handlers::update_profile).delete(handlers::delete_profile),
        )
        .route(
            "/api/results",
            get(handlers::list_results).post(handlers::create_result),
        )
        .route("/api/results/{id}", patch(handlers::update_result))
        .route(
            "/api/cms/pages",
            get(handlers::list_cms_pages).post(handlers::create_cms_page),
        )
        .route(
            "/api/cms/pages/{id}",
            patch(handlers::update_cms_page).delete(handlers::delete_cms_page),
        )
        .route("/api/logs", get(handlers::list_logs))
        .route("/api/admin/flags", get(handlers::list_flags))
        .route("/api/admin/flags/{key}", patch(handlers::update_flag))
        .route("/api/admin/stats", get(handlers::site_stats))
        .route("/api/admin/db/tables", get(handlers::db_list_tables))
        .route(
            "/api/admin/db/{table}",
            get(handlers::db_list_rows).post(handlers::db_upsert_row),
        )
        .route(
            "/api/admin/db/{table}/{id}",
            delete(handlers::db_delete_row),
        )
        .route("/api/admin/preview/start", post(handlers::preview_start))
        .route("/api/admin/preview/stop", post(handlers::preview_stop))
        .route("/api/athlete/stats", get(handlers::athlete_stats))
        .route(
            "/api/attendance/session",
            get(handlers::get_session).post(handlers::refresh_session),
        )
        .route(
            "/api/attendance",
            get(handlers::list_attendance).post(handlers::check_in),
        )
        .route(
            "/api/plans",
            get(handlers::list_plans).post(handlers::create_plan),
        )
        .route(
            "/api/plans/{id}",
            patch(handlers::update_plan).delete(handlers::delete_plan),
        )
        .route(
            "/api/plans/{id}/progress",
            get(handlers::get_my_progress).put(handlers::save_progress),
        )
        .with_state(state)
}
