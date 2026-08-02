use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use utoipa::IntoParams;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::AppResult;
use crate::models::club::SystemLog;
use crate::models::role::Role;
use crate::models::user::ErrorBody;
use crate::state::AppState;

#[derive(Debug, Deserialize, IntoParams)]
pub struct LogsQuery {
    pub limit: Option<usize>,
    pub source: Option<String>,
    pub level: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/logs",
    params(LogsQuery),
    responses(
        (status = 200, description = "Lista logów systemowych", body = Vec<SystemLog>),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_logs(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<LogsQuery>,
) -> AppResult<Json<Vec<SystemLog>>> {
    ensure_roles(&auth, &[Role::Admin])?;
    let limit = query.limit.unwrap_or(200).min(1000);
    let mut logs = state.db.list_logs(limit).await?;

    if let Some(source) = query.source {
        logs.retain(|l| l.source == source);
    }
    if let Some(level) = query.level {
        logs.retain(|l| format!("{:?}", l.level).eq_ignore_ascii_case(&level));
    }

    Ok(Json(logs))
}
