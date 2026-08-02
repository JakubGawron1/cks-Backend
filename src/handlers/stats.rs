use axum::extract::State;
use axum::Json;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::AppResult;
use crate::models::club::SiteStats;
use crate::models::role::Role;
use crate::state::AppState;

pub async fn site_stats(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<SiteStats>> {
    ensure_roles(&auth, &[Role::Superadmin])?;
    Ok(Json(state.db.site_stats().await?))
}
