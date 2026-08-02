use axum::response::Html;
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::auth::handlers::{login, me};
use crate::handlers;
use crate::state::AppState;

const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="pl">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Backend CKS Slavia</title>
  <style>
    :root { color-scheme: light; }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      min-height: 100vh;
      display: grid;
      place-items: center;
      font-family: "Segoe UI", system-ui, sans-serif;
      background: linear-gradient(160deg, #0f172a 0%, #1e293b 50%, #0f766e 100%);
      color: #f8fafc;
    }
    main {
      text-align: center;
      padding: 2rem;
    }
    h1 {
      margin: 0 0 0.5rem;
      font-size: clamp(1.75rem, 4vw, 2.5rem);
      font-weight: 700;
      letter-spacing: -0.02em;
    }
    p {
      margin: 0 0 1.75rem;
      opacity: 0.8;
      font-size: 1rem;
    }
    a.btn {
      display: inline-block;
      padding: 0.85rem 1.5rem;
      border-radius: 0.5rem;
      background: #f8fafc;
      color: #0f172a;
      font-weight: 600;
      text-decoration: none;
      transition: transform 0.15s ease, box-shadow 0.15s ease;
    }
    a.btn:hover {
      transform: translateY(-1px);
      box-shadow: 0 8px 24px rgba(0, 0, 0, 0.25);
    }
  </style>
</head>
<body>
  <main>
    <h1>Backend CKS Slavia</h1>
    <p>API klubu — Axum / Rust</p>
    <a class="btn" href="https://slavia.vercel.app/">Przejdź do strony klubu</a>
  </main>
</body>
</html>"#;

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "slavia-backend",
        "auth": true
    }))
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
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
