mod auth;
mod config;
mod db;
mod error;
mod handlers;
mod images;
mod mail;
mod models;
mod openapi;
mod push;
mod routes;
mod state;
mod weightlifting_categories;

use std::net::SocketAddr;
use std::time::Duration;

use tower_http::classify::ServerErrorsFailureClass;
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tower_http::LatencyUnit;
use tracing::Level;
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::db::Database;
use crate::mail::Mailer;
use crate::state::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new("slavia_backend=debug,tower_http=info,axum=info")
        }))
        .with_target(true)
        .with_level(true)
        .init();

    let config = Config::from_env()?;
    tracing::info!(
        mode = %config.production_mode.as_str(),
        remote_db = config.is_remote_db(),
        host = %config.host,
        port = config.port,
        jwt_expiry_hours = config.jwt_expiry_hours,
        origins = ?config.frontend_origins,
        "konfiguracja załadowana"
    );
    if Config::is_hosted() {
        let host = if Config::is_huggingface() {
            "Hugging Face Space"
        } else {
            "Render"
        };
        tracing::info!(
            hosting = host,
            port = config.port,
            origins = ?config.frontend_origins,
            "wykryto hosting"
        );
        if !config.is_remote_db() {
            tracing::warn!(
                "Baza plikowa na hostingu jest efemeryczna — ustaw Turso (PRODUCTION_MODE=production)."
            );
        }
    }

    let cors = config.cors_layer()?;
    tracing::info!("łączenie z bazą…");
    let db = Database::connect(&config).await?;
    tracing::info!("migracje schematu…");
    db.migrate().await?;
    tracing::info!("seed / synchronizacja katalogów…");
    db.seed_if_empty(&config).await?;
    let purged = db.purge_old_system_logs().await?;
    if purged > 0 {
        tracing::info!(purged, "startup: usunięto logi starsze niż 7 dni");
    }
    tracing::info!("baza gotowa");

    let mailer = Mailer::from_config(&config);
    tracing::info!(
        email_enabled = config.email_enabled,
        has_brevo_key = config.brevo_api_key.is_some(),
        "mailer gotowy"
    );
    let state = AppState {
        db,
        config: config.clone(),
        mailer,
    };

    let trace = TraceLayer::new_for_http()
        .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
        .on_request(DefaultOnRequest::new().level(Level::INFO))
        .on_response(
            DefaultOnResponse::new()
                .level(Level::INFO)
                .latency_unit(LatencyUnit::Millis),
        )
        .on_failure(
            |error: ServerErrorsFailureClass, latency: Duration, _span: &tracing::Span| {
                tracing::error!(
                    ?error,
                    latency_ms = latency.as_millis(),
                    "request failure"
                );
            },
        );

    let app = routes::router(state).layer(cors).layer(trace);

    let addr = SocketAddr::from((config.host, config.port));
    tracing::info!(%addr, "CKS Slavia API listening");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
