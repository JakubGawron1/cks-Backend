mod auth;
mod config;
mod db;
mod error;
mod handlers;
mod models;
mod routes;
mod state;

use std::net::SocketAddr;

use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::db::Database;
use crate::state::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new("slavia_backend=debug,tower_http=info,axum=info")
        }))
        .init();

    let config = Config::from_env()?;
    if Config::is_render() {
        tracing::info!("Render detected — PORT={}, origins={:?}", config.port, config.frontend_origins);
        if !config.is_remote_db() {
            tracing::warn!(
                "Baza plikowa na Free Render jest efemeryczna (znika po redeploy/śnie). Rozważ Turso."
            );
        }
    }

    let cors = config.cors_layer()?;
    let db = Database::connect(&config).await?;
    db.migrate().await?;
    db.seed_if_empty(&config).await?;

    let state = AppState {
        db,
        config: config.clone(),
    };

    let app = routes::router(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from((config.host, config.port));
    tracing::info!("CKS Slavia API listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
