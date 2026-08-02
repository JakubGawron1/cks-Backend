use axum::extract::State;
use axum::Json;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::AppResult;
use crate::models::club::SiteStats;
use crate::models::role::Role;
use crate::models::user::ErrorBody;
use crate::state::AppState;

#[utoipa::path(
    get,
    path = "/api/admin/stats",
    responses(
        (status = 200, description = "Statystyki serwisu", body = SiteStats),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn site_stats(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<SiteStats>> {
    ensure_roles(&auth, &[Role::Superadmin])?;
    Ok(Json(state.db.site_stats().await?))
}
