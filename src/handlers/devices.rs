use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::auth::extractor::AuthUser;
use crate::error::AppResult;
use crate::models::club::DeviceToken;
use crate::models::user::{ErrorBody, OkResponse};
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct RegisterDeviceBody {
    pub token: String,
    /// "android" | "windows" | "ios"
    #[serde(default = "default_platform")]
    pub platform: String,
}

fn default_platform() -> String {
    "android".into()
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UnregisterDeviceBody {
    pub token: String,
}

#[utoipa::path(
    post,
    path = "/api/devices",
    request_body = RegisterDeviceBody,
    responses(
        (status = 200, description = "Zarejestrowano token urządzenia", body = DeviceToken),
        (status = 401, description = "Unauthorized", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "devices"
)]
pub async fn register_device(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<RegisterDeviceBody>,
) -> AppResult<Json<DeviceToken>> {
    let token = body.token.trim().to_string();
    let platform = body.platform.trim().to_ascii_lowercase();
    let device = state
        .db
        .upsert_device_token(&auth.user.id, &token, &platform)
        .await?;
    Ok(Json(device))
}

#[utoipa::path(
    delete,
    path = "/api/devices",
    request_body = UnregisterDeviceBody,
    responses(
        (status = 200, description = "Usunięto token", body = OkResponse),
        (status = 401, description = "Unauthorized", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "devices"
)]
pub async fn unregister_device(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<UnregisterDeviceBody>,
) -> AppResult<Json<OkResponse>> {
    let _ = auth;
    state.db.delete_device_token(body.token.trim()).await?;
    Ok(Json(OkResponse { ok: true }))
}
