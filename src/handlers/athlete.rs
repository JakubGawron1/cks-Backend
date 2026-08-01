use axum::extract::State;
use axum::Json;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::AppResult;
use crate::models::club::AthleteStats;
use crate::models::role::Role;
use crate::state::AppState;

pub async fn athlete_stats(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<AthleteStats>> {
    ensure_roles(&auth, &[Role::Zawodnik, Role::Trener, Role::Admin])?;
    Ok(Json(state.db.athlete_stats(&auth.user.id)?))
}
