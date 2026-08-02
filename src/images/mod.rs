//! Providery obrazów — upload przez backend.
//! Domyślnie ImageKit; Cloudinary zaplanowane (patrz Todo.md).

pub mod imagekit;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::config::Config;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
#[schema(rename_all = "lowercase")]
pub enum ImageProvider {
    Imagekit,
    Cloudinary,
}

impl ImageProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Imagekit => "imagekit",
            Self::Cloudinary => "cloudinary",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "imagekit" => Some(Self::Imagekit),
            "cloudinary" => Some(Self::Cloudinary),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UploadImageResponse {
    pub url: String,
    pub provider: ImageProvider,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DeleteImageResponse {
    pub ok: bool,
    /// true = usunięto z providera; false = tylko lokalne czyszczenie URL (zewnętrzny host)
    pub deleted_remote: bool,
}

pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024; // 5 MiB

pub fn allowed_image_content_type(ct: &str) -> bool {
    matches!(
        ct.to_ascii_lowercase().as_str(),
        "image/jpeg" | "image/jpg" | "image/png" | "image/webp" | "image/gif"
    )
}

pub async fn upload_image(
    config: &Config,
    bytes: Vec<u8>,
    filename: &str,
    content_type: &str,
) -> AppResult<UploadImageResponse> {
    if bytes.is_empty() {
        return Err(AppError::BadRequest("Pusty plik.".into()));
    }
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(AppError::BadRequest(
            "Plik jest za duży (max 5 MB).".into(),
        ));
    }
    if !allowed_image_content_type(content_type) {
        return Err(AppError::BadRequest(
            "Dozwolone formaty: JPEG, PNG, WebP, GIF.".into(),
        ));
    }

    match config.image_provider {
        ImageProvider::Imagekit => {
            let result = imagekit::upload(config, &bytes, filename, content_type).await?;
            Ok(UploadImageResponse {
                url: result.url,
                provider: ImageProvider::Imagekit,
                file_id: Some(result.file_id),
            })
        }
        ImageProvider::Cloudinary => Err(AppError::BadRequest(
            "Provider Cloudinary nie jest jeszcze wdrożony. Ustaw IMAGE_PROVIDER=imagekit."
                .into(),
        )),
    }
}

/// Usuwa obraz z aktywnego providera (gdy URL należy do niego).
/// Zewnętrzne URL-e (fallback) — OK bez remote delete.
pub async fn delete_image(config: &Config, url: &str) -> AppResult<DeleteImageResponse> {
    let url = url.trim();
    if url.is_empty() {
        return Err(AppError::BadRequest("Brak URL do usunięcia.".into()));
    }

    match config.image_provider {
        ImageProvider::Imagekit => {
            if !imagekit::is_imagekit_url(url, config) {
                return Ok(DeleteImageResponse {
                    ok: true,
                    deleted_remote: false,
                });
            }
            imagekit::delete_by_url(config, url).await?;
            Ok(DeleteImageResponse {
                ok: true,
                deleted_remote: true,
            })
        }
        ImageProvider::Cloudinary => Err(AppError::BadRequest(
            "Provider Cloudinary nie jest jeszcze wdrożony.".into(),
        )),
    }
}
