use axum::extract::State;
use axum::response::Html;
use axum::routing::get;
use axum::Json;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use utoipa_scalar::{Scalar, Servable as ScalarServable};
use utoipa_swagger_ui::SwaggerUi;

use crate::error::AppResult;
use crate::handlers;
use crate::models::club::HealthResponse;
use crate::models::user::ErrorBody;
use crate::openapi::ApiDoc;
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
    main { text-align: center; padding: 2rem; }
    h1 {
      margin: 0 0 0.5rem;
      font-size: clamp(1.75rem, 4vw, 2.5rem);
      font-weight: 700;
      letter-spacing: -0.02em;
    }
    p { margin: 0 0 1.75rem; opacity: 0.8; font-size: 1rem; }
    .links { display: flex; flex-wrap: wrap; gap: 0.75rem; justify-content: center; }
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
    a.btn.secondary { background: transparent; color: #f8fafc; border: 1px solid #94a3b8; }
  </style>
</head>
<body>
  <main>
    <h1>Backend CKS Slavia</h1>
    <p>API klubu — Axum / Rust / OpenAPI</p>
    <div class="links">
      <a class="btn" href="https://slavia.vercel.app/">Strona klubu</a>
      <a class="btn secondary" href="/api/docs">Scalar</a>
      <a class="btn secondary" href="/api/swagger">Swagger UI</a>
      <a class="btn secondary" href="/api/openapi.json">openapi.json</a>
    </div>
  </main>
</body>
</html>"#;

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

#[utoipa::path(
    get,
    path = "/api/health",
    responses(
        (status = 200, description = "OK", body = HealthResponse),
        (status = 500, description = "Baza niedostępna", body = ErrorBody),
    ),
    tag = "admin"
)]
async fn health(State(state): State<AppState>) -> AppResult<Json<HealthResponse>> {
    state.db.ping().await?;
    Ok(Json(HealthResponse {
        status: "ok".into(),
        service: "slavia-backend".into(),
        auth: true,
    }))
}

/// Buduje router API + pełny dokument OpenAPI (ścieżki z handlerów).
pub fn openapi_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(health))
        .routes(routes!(crate::auth::handlers::login))
        .routes(routes!(
            crate::auth::handlers::me,
            crate::auth::handlers::update_me
        ))
        .routes(routes!(crate::auth::handlers::request_email_verification))
        .routes(routes!(crate::auth::handlers::confirm_email))
        .routes(routes!(crate::auth::handlers::forgot_password))
        .routes(routes!(crate::auth::handlers::reset_password))
        .routes(routes!(
            handlers::users::list_users,
            handlers::users::create_user
        ))
        .routes(routes!(
            handlers::users::update_user,
            handlers::users::delete_user
        ))
        .routes(routes!(
            handlers::profiles::list_profiles,
            handlers::profiles::create_profile
        ))
        .routes(routes!(
            handlers::profiles::update_profile,
            handlers::profiles::delete_profile
        ))
        .routes(routes!(handlers::profiles::list_public_profiles))
        .routes(routes!(
            handlers::results::list_results,
            handlers::results::create_result
        ))
        .routes(routes!(handlers::results::update_result))
        .routes(routes!(handlers::results::list_public_results))
        .routes(routes!(
            handlers::cms::list_cms_pages,
            handlers::cms::create_cms_page
        ))
        .routes(routes!(
            handlers::cms::update_cms_page,
            handlers::cms::delete_cms_page
        ))
        .routes(routes!(handlers::logs::list_logs))
        .routes(routes!(handlers::flags::list_flags))
        .routes(routes!(handlers::flags::update_flag))
        .routes(routes!(handlers::flags::list_public_flags))
        .routes(routes!(handlers::stats::site_stats))
        .routes(routes!(handlers::db_admin::db_list_tables))
        .routes(routes!(
            handlers::db_admin::db_list_rows,
            handlers::db_admin::db_upsert_row
        ))
        .routes(routes!(handlers::db_admin::db_delete_row))
        .routes(routes!(handlers::preview::preview_start))
        .routes(routes!(handlers::preview::preview_stop))
        .routes(routes!(handlers::debug::send_test_email))
        .routes(routes!(handlers::athlete::athlete_stats))
        .routes(routes!(
            handlers::attendance::get_session,
            handlers::attendance::refresh_session
        ))
        .routes(routes!(
            handlers::attendance::list_attendance,
            handlers::attendance::check_in
        ))
        .routes(routes!(handlers::attendance::approve_attendance))
        .routes(routes!(handlers::attendance::reject_attendance))
        .routes(routes!(
            handlers::events::list_events,
            handlers::events::create_event
        ))
        .routes(routes!(handlers::events::list_public_events))
        .routes(routes!(handlers::events::list_my_events))
        .routes(routes!(
            handlers::events::get_schedule,
            handlers::events::update_schedule
        ))
        .routes(routes!(
            handlers::events::update_event,
            handlers::events::delete_event
        ))
        .routes(routes!(handlers::events::cancel_event))
        .routes(routes!(handlers::events::restore_event))
        .routes(routes!(handlers::events::withdraw_from_event))
        .routes(routes!(handlers::events::accept_withdrawal))
        .routes(routes!(handlers::events::reject_withdrawal))
        .routes(routes!(handlers::events::clear_withdrawal))
        .routes(routes!(
            handlers::plans::list_plans,
            handlers::plans::create_plan
        ))
        .routes(routes!(
            handlers::plans::update_plan,
            handlers::plans::delete_plan
        ))
        .routes(routes!(
            handlers::plans::get_my_progress,
            handlers::plans::save_progress
        ))
        .routes(routes!(handlers::contact::submit_contact))
        .routes(routes!(
            handlers::contact::list_contact_messages,
            handlers::contact::update_contact_message,
            handlers::contact::delete_contact_message
        ))
        .routes(routes!(handlers::notifications::list_notifications))
        .routes(routes!(handlers::notifications::unread_count))
        .routes(routes!(handlers::notifications::mark_all_read))
        .routes(routes!(
            handlers::notifications::update_notification,
            handlers::notifications::delete_notification
        ))
        .routes(routes!(
            handlers::devices::register_device,
            handlers::devices::unregister_device
        ))
        .routes(routes!(
            handlers::uploads::upload_image,
            handlers::uploads::delete_image
        ))
}

pub fn router(state: AppState) -> axum::Router {
    let (router, api) = openapi_router().split_for_parts();

    // Domyślny limit Axum to 2 MiB — za mało na zdjęcia telefonów.
    // 6 MiB: MAX_IMAGE_BYTES (5) + narzut multipart.
    router
        .route("/", get(index))
        .merge(SwaggerUi::new("/api/swagger").url("/api/openapi.json", api.clone()))
        .merge(Scalar::with_url("/api/docs", api))
        .layer(axum::extract::DefaultBodyLimit::max(6 * 1024 * 1024))
        .with_state(state)
}

#[cfg(test)]
mod export_tests {
    use std::path::PathBuf;

    use super::openapi_router;

    #[test]
    #[ignore = "uruchamiaj: cargo test export_openapi -- --ignored"]
    fn export_openapi() {
        let (_router, api) = openapi_router().split_for_parts();
        let json = api.to_pretty_json().expect("serialize openapi");
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("slavia-frontend")
            .join("openapi")
            .join("openapi.json");
        std::fs::create_dir_all(out.parent().unwrap()).expect("mkdir openapi");
        std::fs::write(&out, &json).expect("write openapi.json");
        eprintln!("Wrote {} ({} bytes)", out.display(), json.len());
    }
}
