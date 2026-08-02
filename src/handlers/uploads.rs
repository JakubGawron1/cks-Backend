use axum::extract::{Multipart, State};
use axum::Json;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::auth::extractor::AuthUser;
use crate::error::{AppError, AppResult};
use crate::images::{self, DeleteImageResponse, UploadImageResponse};
use crate::models::club::LogLevel;
use crate::models::user::ErrorBody;
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteImageBody {
    pub url: String,
}

#[utoipa::path(
    post,
    path = "/api/uploads/image",
    request_body(
        content_type = "multipart/form-data",
        description = "Plik obrazu (pole `file`)"
    ),
    responses(
        (status = 200, description = "Wgrano obraz", body = UploadImageResponse),
        (status = 400, description = "Nieprawidłowy plik / brak konfiguracji providera", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "uploads"
)]
pub async fn upload_image(
    State(state): State<AppState>,
    auth: AuthUser,
    mut multipart: Multipart,
) -> AppResult<Json<UploadImageResponse>> {
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut filename = String::from("avatar.jpg");
    let mut content_type = String::from("image/jpeg");

    while let Some(field) = multipart.next_field().await.map_err(|err| {
        AppError::BadRequest(format!("Nieprawidłowy multipart: {err}"))
    })? {
        let name = field.name().unwrap_or("").to_string();
        if name != "file" {
            continue;
        }
        if let Some(fname) = field.file_name() {
            filename = fname.to_string();
        }
        if let Some(ct) = field.content_type() {
            content_type = ct.to_string();
        }
        let data = field.bytes().await.map_err(|err| {
            let msg = err.to_string();
            if msg.contains("length limit") || msg.contains("too large") {
                AppError::BadRequest(
                    "Plik jest za duży (max 5 MB).".into(),
                )
            } else {
                AppError::BadRequest(format!("Nie udało się odczytać pliku: {err}"))
            }
        })?;
        file_bytes = Some(data.to_vec());
        break;
    }

    let bytes = file_bytes.ok_or_else(|| {
        AppError::BadRequest("Brak pliku — wyślij pole multipart `file`.".into())
    })?;

    let result =
        images::upload_image(&state.config, bytes, &filename, &content_type).await?;

    state
        .db
        .append_log(
            LogLevel::Info,
            "uploads",
            &format!(
                "Wgrano obraz ({}) przez {}",
                result.provider.as_str(),
                auth.user.email
            ),
            Some(&auth.user.id),
        )
        .await?;

    Ok(Json(result))
}

#[utoipa::path(
    delete,
    path = "/api/uploads/image",
    request_body = DeleteImageBody,
    responses(
        (status = 200, description = "Usunięto obraz (remote lub tylko lokalny URL)", body = DeleteImageResponse),
        (status = 400, description = "Nieprawidłowy URL / błąd providera", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "uploads"
)]
pub async fn delete_image(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<DeleteImageBody>,
) -> AppResult<Json<DeleteImageResponse>> {
    let result = images::delete_image(&state.config, &body.url).await?;

    state
        .db
        .append_log(
            LogLevel::Info,
            "uploads",
            &format!(
                "Usunięto obraz (remote={}) przez {}",
                result.deleted_remote, auth.user.email
            ),
            Some(&auth.user.id),
        )
        .await?;

    Ok(Json(result))
}
