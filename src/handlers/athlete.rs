use axum::extract::State;
use axum::Json;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::AppResult;
use crate::models::club::AthleteStats;
use crate::models::role::Role;
use crate::models::user::ErrorBody;
use crate::state::AppState;

#[utoipa::path(
    get,
    path = "/api/athlete/stats",
    responses(
        (status = 200, description = "Statystyki zawodnika", body = AthleteStats),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn athlete_stats(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<AthleteStats>> {
    ensure_roles(&auth, &[Role::Zawodnik, Role::Trener, Role::Admin])?;
    Ok(Json(state.db.athlete_stats(auth.effective_id()).await?))
}
