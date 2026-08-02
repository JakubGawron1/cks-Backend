use serde::Deserialize;

use crate::config::Config;
use crate::error::{internal, AppError, AppResult};

const UPLOAD_URL: &str = "https://upload.imagekit.io/api/v1/files/upload";
const FILES_API: &str = "https://api.imagekit.io/v1/files";
/// Folder w media library (względem root). Endpoint URL może mieć własny prefix
/// (np. `…/id/slavia`) — nie duplikuj go tutaj.
const DEFAULT_FOLDER: &str = "/avatars";

#[derive(Debug, Deserialize)]
struct ImageKitUploadResponse {
    url: String,
    #[serde(rename = "fileId")]
    file_id: String,
    #[serde(rename = "filePath")]
    file_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ImageKitListedFile {
    #[serde(rename = "fileId")]
    file_id: String,
    #[serde(rename = "filePath")]
    file_path: Option<String>,
    url: Option<String>,
    name: Option<String>,
}

#[derive(Debug)]
pub struct ImageKitResult {
    pub url: String,
    pub file_id: String,
}

fn require_private_key(config: &Config) -> AppResult<&str> {
    config.imagekit_private_key.as_deref().ok_or_else(|| {
        AppError::BadRequest(
            "ImageKit nie jest skonfigurowany (IMAGEKIT_PRIVATE_KEY).".into(),
        )
    })
}

fn require_url_endpoint(config: &Config) -> AppResult<&str> {
    let endpoint = config.imagekit_url_endpoint.as_deref().ok_or_else(|| {
        AppError::BadRequest(
            "ImageKit nie jest skonfigurowany (IMAGEKIT_URL_ENDPOINT).".into(),
        )
    })?;
    if endpoint.trim().is_empty() {
        return Err(AppError::BadRequest(
            "ImageKit nie jest skonfigurowany (IMAGEKIT_URL_ENDPOINT).".into(),
        ));
    }
    Ok(endpoint.trim())
}

fn imagekit_id_from_endpoint(endpoint: &str) -> Option<&str> {
    let rest = endpoint
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let rest = rest.strip_prefix("ik.imagekit.io/")?;
    rest.split('/').next().filter(|s| !s.is_empty())
}

/// Kanoniczny publiczny URL media library: `https://ik.imagekit.io/{id}{filePath}`.
pub fn public_url(url_endpoint: &str, file_path: &str) -> String {
    let path = if file_path.starts_with('/') {
        file_path.to_string()
    } else {
        format!("/{file_path}")
    };
    if let Some(id) = imagekit_id_from_endpoint(url_endpoint) {
        return format!("https://ik.imagekit.io/{id}{path}");
    }
    let base = url_endpoint.trim().trim_end_matches('/');
    format!("{base}{path}")
}

/// Wyciąga `filePath` z publicznego URL (z lub bez prefixu endpointu).
fn file_path_from_url(url: &str, config: &Config) -> Option<String> {
    let bare = url.split('?').next()?.trim();
    if bare.is_empty() {
        return None;
    }

    if let Some(endpoint) = config.imagekit_url_endpoint.as_deref() {
        let base = endpoint.trim().trim_end_matches('/');
        if let Some(rest) = bare.strip_prefix(base) {
            if rest.is_empty() {
                return None;
            }
            return Some(if rest.starts_with('/') {
                rest.to_string()
            } else {
                format!("/{rest}")
            });
        }
    }

    if let Some(id) = config
        .imagekit_url_endpoint
        .as_deref()
        .and_then(imagekit_id_from_endpoint)
    {
        let prefix = format!("https://ik.imagekit.io/{id}");
        if let Some(rest) = bare.strip_prefix(&prefix) {
            if rest.is_empty() {
                return None;
            }
            return Some(if rest.starts_with('/') {
                rest.to_string()
            } else {
                format!("/{rest}")
            });
        }
    }

    None
}

pub async fn upload(
    config: &Config,
    bytes: &[u8],
    filename: &str,
    content_type: &str,
) -> AppResult<ImageKitResult> {
    let private_key = require_private_key(config)?;
    let public_key = config.imagekit_public_key.as_deref().ok_or_else(|| {
        AppError::BadRequest(
            "ImageKit nie jest skonfigurowany (IMAGEKIT_PUBLIC_KEY).".into(),
        )
    })?;
    let url_endpoint = require_url_endpoint(config)?;

    let safe_name = sanitize_filename(filename);
    let part = reqwest::multipart::Part::bytes(bytes.to_vec())
        .file_name(safe_name.clone())
        .mime_str(content_type)
        .map_err(internal)?;

    let form = reqwest::multipart::Form::new()
        .text("fileName", safe_name)
        .text("folder", DEFAULT_FOLDER.to_string())
        .text("useUniqueFileName", "true".to_string())
        .text("publicKey", public_key.to_string())
        .part("file", part);

    let client = reqwest::Client::new();
    let response = client
        .post(UPLOAD_URL)
        .basic_auth(private_key, None::<&str>)
        .multipart(form)
        .send()
        .await
        .map_err(internal)?;

    let status = response.status();
    let body = response.text().await.map_err(internal)?;

    if !status.is_success() {
        tracing::error!(%status, body = %body, "ImageKit upload failed");
        return Err(AppError::BadRequest(format!(
            "Upload do ImageKit nie powiódł się ({status})."
        )));
    }

    let parsed: ImageKitUploadResponse = serde_json::from_str(&body).map_err(|err| {
        tracing::error!(error = %err, body = %body, "ImageKit: nieparsowalna odpowiedź");
        internal(err)
    })?;

    // Kanoniczny URL z filePath (niezależny od path w URL endpoint).
    let url = if let Some(ref path) = parsed.file_path {
        public_url(url_endpoint, path)
    } else {
        parsed.url.split('?').next().unwrap_or(&parsed.url).to_string()
    };

    Ok(ImageKitResult {
        url,
        file_id: parsed.file_id,
    })
}

async fn find_file_id(config: &Config, url: &str) -> AppResult<Option<String>> {
    let private_key = require_private_key(config)?;
    let client = reqwest::Client::new();

    let file_path = file_path_from_url(url, config);
    let file_name = file_path
        .as_deref()
        .and_then(|p| p.rsplit('/').next())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            url.split('?')
                .next()?
                .rsplit('/')
                .next()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        });

    let mut candidates: Vec<ImageKitListedFile> = Vec::new();

    if let Some(ref name) = file_name {
        let response = client
            .get(FILES_API)
            .basic_auth(private_key, None::<&str>)
            .query(&[("name", name.as_str()), ("limit", "20")])
            .send()
            .await
            .map_err(internal)?;
        let status = response.status();
        let body = response.text().await.map_err(internal)?;
        if status.is_success() {
            match serde_json::from_str::<Vec<ImageKitListedFile>>(&body) {
                Ok(list) => candidates.extend(list),
                Err(err) => {
                    tracing::warn!(error = %err, body = %body, "ImageKit list(name): parse");
                }
            }
        } else {
            tracing::warn!(%status, body = %body, "ImageKit list(name) failed");
        }
    }

    // Fallback: pliki z folderu /avatars
    if candidates.is_empty() {
        let response = client
            .get(FILES_API)
            .basic_auth(private_key, None::<&str>)
            .query(&[("path", DEFAULT_FOLDER), ("limit", "50")])
            .send()
            .await
            .map_err(internal)?;
        let status = response.status();
        let body = response.text().await.map_err(internal)?;
        if status.is_success() {
            if let Ok(list) = serde_json::from_str::<Vec<ImageKitListedFile>>(&body) {
                candidates.extend(list);
            }
        }
    }

    let bare_url = url.split('?').next().unwrap_or(url).trim();

    if let Some(ref path) = file_path {
        if let Some(hit) = candidates.iter().find(|f| {
            f.file_path.as_deref() == Some(path.as_str())
                || f.url
                    .as_deref()
                    .map(|u| u.split('?').next().unwrap_or(u) == bare_url)
                    .unwrap_or(false)
        }) {
            return Ok(Some(hit.file_id.clone()));
        }
    }

    if let Some(hit) = candidates.iter().find(|f| {
        f.url
            .as_deref()
            .map(|u| u.split('?').next().unwrap_or(u) == bare_url)
            .unwrap_or(false)
            || f.name.as_deref() == file_name.as_deref()
    }) {
        return Ok(Some(hit.file_id.clone()));
    }

    Ok(None)
}

/// Usuwa plik z ImageKit: najpierw resolve `fileId`, potem `DELETE /v1/files/{id}`.
/// Brak pliku (404 / nie znaleziono) = sukces.
pub async fn delete_by_url(config: &Config, url: &str) -> AppResult<()> {
    let private_key = require_private_key(config)?;
    let url = url.trim();
    if url.is_empty() {
        return Err(AppError::BadRequest("Brak URL do usunięcia.".into()));
    }

    let Some(file_id) = find_file_id(config, url).await? else {
        tracing::info!(url, "ImageKit: brak pliku do usunięcia (już skasowany?)");
        return Ok(());
    };

    let client = reqwest::Client::new();
    let response = client
        .delete(format!("{FILES_API}/{file_id}"))
        .basic_auth(private_key, None::<&str>)
        .send()
        .await
        .map_err(internal)?;

    let status = response.status();
    let body = response.text().await.map_err(internal)?;

    if status.is_success() || status.as_u16() == 404 {
        return Ok(());
    }

    tracing::warn!(%status, body = %body, %file_id, url, "ImageKit DELETE file failed");
    Err(AppError::BadRequest(format!(
        "Usuwanie z ImageKit nie powiodło się ({status})."
    )))
}

pub fn is_imagekit_url(url: &str, config: &Config) -> bool {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return false;
    }
    if let Some(endpoint) = config.imagekit_url_endpoint.as_deref() {
        let base = endpoint.trim_end_matches('/');
        if !base.is_empty() && trimmed.starts_with(base) {
            return true;
        }
        if let Some(id) = imagekit_id_from_endpoint(endpoint) {
            if trimmed.contains(&format!("ik.imagekit.io/{id}")) {
                return true;
            }
        }
    }
    trimmed.contains("ik.imagekit.io")
}

fn sanitize_filename(raw: &str) -> String {
    let name = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("avatar")
        .trim();
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() || cleaned == "." || cleaned.starts_with('.') {
        "avatar.jpg".into()
    } else {
        cleaned
    }
}
